use ghost_core::features::coordination::{
    severity_high, severity_low, CoordinationRiskConfig, CoordinationRiskFeatures, DegradedReason,
    FundingVisibility, MetricEvidenceStatus, MetricValue,
};
#[test]
fn metric_value_carries_status_and_degraded_reasons() {
    let mut value = MetricValue::new(0.25, 0.75, 0.8, 5, 0.9, MetricEvidenceStatus::Degraded);
    value
        .degraded_reasons
        .push(DegradedReason::MissingPrePostBalances);

    assert_eq!(value.value, 0.25);
    assert_eq!(value.status, MetricEvidenceStatus::Degraded);
    assert_eq!(value.sample_n, 5);
    assert_eq!(value.coverage, 0.9);
    assert_eq!(
        value.degraded_reasons.as_slice(),
        &[DegradedReason::MissingPrePostBalances]
    );
}

#[test]
fn fsc_unavailable_is_not_clean_zero_or_penalty() {
    let features = CoordinationRiskFeatures::default();

    assert_eq!(features.funding_source_concentration, None);
    assert_eq!(features.funding_visibility, FundingVisibility::Unavailable);
    assert!(features
        .degraded_reasons
        .contains(&DegradedReason::FundingLaneUnavailable));
    assert_eq!(features.total_coordination_penalty, None);
    assert_eq!(features.interaction_penalty, None);
}

#[test]
fn severity_helpers_are_metric_local_and_do_not_encode_scoring() {
    assert_approx_eq(severity_low(0.10, 0.20), 0.5);
    assert_eq!(severity_low(0.30, 0.20), 0.0);
    assert_eq!(severity_low(0.10, 0.0), 0.0);

    assert_approx_eq(severity_high(0.80, 0.60), 0.5);
    assert_eq!(severity_high(0.40, 0.60), 0.0);
    assert_eq!(severity_high(0.80, 1.0), 0.0);
}

#[test]
fn coordination_risk_config_defaults_are_inert_export_only() {
    let config = CoordinationRiskConfig::default();

    assert!(!config.enabled);
    assert!(config.export_only);
    assert_eq!(config.min_unique_buyers_for_diagnostics, 3);
    assert_eq!(config.min_unique_buyers_for_soft_scoring, 5);
    assert_eq!(config.funding_visibility, FundingVisibility::Unavailable);
}

#[test]
fn coordination_risk_config_deserializes_missing_fields_as_legacy_safe_defaults() {
    let config: CoordinationRiskConfig =
        serde_json::from_str("{}").expect("empty legacy config should deserialize");

    assert_eq!(config, CoordinationRiskConfig::default());
}

#[test]
fn coordination_risk_features_deserialize_missing_fields_as_unavailable_without_inventing_reasons()
{
    let features: CoordinationRiskFeatures =
        serde_json::from_str("{}").expect("empty legacy features should deserialize");

    assert_eq!(features.funding_source_concentration, None);
    assert_eq!(features.funding_visibility, FundingVisibility::Unavailable);
    assert!(features.degraded_reasons.is_empty());
}

#[test]
fn available_funding_visibility_roundtrip_does_not_reintroduce_unavailable_reason() {
    let mut features = CoordinationRiskFeatures::default();
    features.funding_visibility = FundingVisibility::Available;
    features.degraded_reasons.clear();

    let encoded = serde_json::to_string(&features).expect("features should serialize");
    let decoded: CoordinationRiskFeatures =
        serde_json::from_str(&encoded).expect("features should deserialize");

    assert_eq!(decoded.funding_visibility, FundingVisibility::Available);
    assert!(decoded.degraded_reasons.is_empty());
}

fn assert_approx_eq(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual}, expected={expected}"
    );
}
