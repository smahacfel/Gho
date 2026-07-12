use super::*;
use crate::checkpoint::EvidenceStatus;
use crate::tx_intelligence::types::FscEvidenceStatus;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

pub const METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractDecisionProjectionWireV1 {
    pub w: u16,
    pub d: Vec<Value>,
}

#[derive(Debug, Error)]
pub enum MetricContractProjectionWireErrorV1 {
    #[error("unsupported metric-contract projection wire version {0}")]
    UnsupportedVersion(u16),
    #[error("wire value at {path} must be an array of length {expected}, got {actual}")]
    TupleLength {
        path: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("wire value at {0} has the wrong JSON type")]
    InvalidValue(&'static str),
    #[error("invalid {kind} wire enum code {code}")]
    InvalidEnumCode { kind: &'static str, code: u64 },
    #[error("unknown {kind} domain enum representation {value}")]
    UnknownDomainEnum { kind: &'static str, value: String },
    #[error("metric-contract projection wire JSON serialization failed")]
    Serialization(#[source] serde_json::Error),
}

const CONTRACT_IDS: &[&str] = &[
    "fee_topology_diversity_index",
    "dev_buy",
    "same_ms_tx_ratio",
    "top3_signer_volume_ratio",
    "flip_ratio",
    "funding_source_concentration",
    "fsc_evidence_status",
    "manipulation_contradiction",
    "reserve_velocity",
    "recent_buy_sell",
];

const SURFACE_IDS: &[&str] = &[
    "tx_intel_fee_topology_diversity_legacy",
    "ftdi_value_evidence_v1",
    "ftdi_legacy_buy_tx_actionability",
    "ftdi_unique_buyer_actionability_v2",
    "coordination_fee_topology_hhi_export_v1",
    "tx_intel_dev_first_observed_buy_sol",
    "gatekeeper_buffer_dev_primary_buy_sol",
    "mfs_dev_first_observed_buy_sol",
    "mfs_dev_primary_buy_sol_v1",
    "effective_policy_dev_buy_sol",
    "tx_intel_same_ms_collision_ratio_exact",
    "tx_timing_exact_same_ms_evidence_v1",
    "tx_intel_bundle_cluster_ratio_lt50_ms",
    "rce_same_ms_collision_ratio_recent_exact",
    "tx_intel_top3_signer_volume_ratio_preferred",
    "tx_intel_top3_volume_pct_compatibility_alias",
    "tx_intel_top3_effective_selector",
    "early_fingerprint_flip_ratio_legacy_slot_gap",
    "flip_ratio_hybrid_evidence_v2",
    "tx_intel_funding_source_concentration_legacy",
    "funding_source_concentration_legacy_evidence_v1",
    "funding_source_v2_readiness_evidence",
    "materialized_fsc_status_compatibility",
    "coordination_funding_source_hhi_export_v1",
    "mfs_manipulation_numeric_legacy_defaults",
    "manipulation_numeric_evidence_v2",
    "mfs_manipulation_high_flags_legacy_defaults",
    "policy_derived_manipulation_high_flags_v2",
    "account_state_reserve_velocity_scalar_legacy",
    "reserve_velocity_evidence_v1",
    "rce_buy_sell_ratio_recent_legacy",
    "recent_buy_sell_evidence_v1",
];

const ROLLOUT_MODES: &[&str] = &["legacy", "dual_compute", "v2"];
const PROFILE_IDS: &[&str] = &["metric_contracts_v1_1_profile_a"];
const AUTHORITY_CLASSES: &[&str] = &[
    "authoritative",
    "equivalent_cutover",
    "compatibility",
    "counterfactual",
    "evidence_only",
    "logging_only",
    "export_only",
];
const ROLLOUT_ROLES: &[&str] = &["policy_authoritative", "policy_comparator", "non_policy"];
const AVAILABILITIES: &[&str] = &[
    "available",
    "unavailable",
    "not_configured",
    "not_recorded_legacy_schema",
];
const QUALITIES: &[&str] = &[
    "measured",
    "degraded",
    "insufficient",
    "stale",
    "fallback",
    "legacy_default",
    "not_applicable",
];
const PRODUCERS: &[&str] = &[
    "fee_topology_diversity_producer",
    "tx_intelligence_engine",
    "tx_intel_effective_top3_selector",
    "tx_intelligence_fingerprint_aggregator",
    "funding_source_index",
    "materialized_fsc_status_adapter",
    "manipulation_evidence_adapter",
    "manipulation_policy_derivation",
    "account_state_core",
    "recent_buy_sell_window_producer",
];
const DEV_SELECTION_MODES: &[&str] = &[
    "legacy_first_observed",
    "create_signature_match",
    "earliest_eligible_creator_buy",
    "no_eligible_buy",
];
const TIMING_POPULATIONS: &[&str] = &["accepted_transactions", "successful_transactions"];
const EVIDENCE_STATUSES: &[&str] = &[
    "clean",
    "degraded",
    "unavailable",
    "insufficient_sample",
    "stale",
    "fallback",
    "shadow_only",
    "not_configured",
];
const FSC_STATUSES: &[&str] = &["clean", "degraded", "unavailable"];
const RESERVE_SOURCE_CLOCKS: &[&str] = &["receive_time"];
const RESERVE_STATUSES: &[&str] = &[
    "first_update",
    "measured",
    "zero_delta_time",
    "bootstrap_fallback",
    "unavailable",
];

const REASON_FAMILIES: &[&str] = &[
    "legacy_status",
    "legacy_degraded",
    "legacy_unavailable",
    "ftdi",
    "dev_buy",
    "tx_timing",
    "top3",
    "flip",
    "funding_source",
    "manipulation",
    "reserve_velocity",
    "recent_buy_sell",
    "unmapped_legacy_string",
];
const LEGACY_STATUS_REASONS: &[&str] = &[
    "degraded",
    "unavailable",
    "insufficient_sample",
    "stale",
    "fallback",
    "shadow_only",
    "not_configured",
    "carried_forward",
    "not_allowed",
    "unavailable_source",
    "clean_with_legacy_reasons",
];
const LEGACY_DEGRADED_REASONS: &[&str] = &[
    "segment_sequence_partial",
    "segment_signer_coverage_partial",
    "tx_intel_low_sample",
    "account_state_fallback",
    "checkpoint_history_sparse",
    "curve_evidence_partial",
    "sybil_evidence_partial",
    "alpha_evidence_partial",
    "manipulation_evidence_partial",
    "identity_evidence_fallback",
    "trajectory_evidence_sparse",
    "pdd_sequence_partial",
    "cpv_evidence_partial",
    "fsc_evidence_partial",
    "organic_broadening_insufficient",
    "manipulation_contradiction_partial",
    "decision_time_series_price_partial",
    "decision_time_series_truncated",
    "evidence_stale",
];
const LEGACY_UNAVAILABLE_REASONS: &[&str] = &[
    "not_materialized",
    "identity_missing",
    "segment_sequence_missing",
    "segment_signer_data_missing",
    "tx_intel_missing",
    "account_state_missing",
    "checkpoint_history_missing",
    "curve_data_missing",
    "trajectory_missing",
    "pdd_sequence_missing",
    "sybil_metrics_missing",
    "alpha_fingerprint_missing",
    "cpv_metrics_missing",
    "fsc_metrics_missing",
    "organic_broadening_missing",
    "manipulation_contradiction_missing",
    "execution_not_run",
    "not_configured",
];
const FTDI_REASONS: &[&str] = &[
    "insufficient_buy_transactions",
    "insufficient_unique_buyers",
    "raw_fee_topology_unavailable",
    "legacy_buy_transaction_actionability_gate",
    "unique_buyer_actionability_counterfactual",
    "coordination_hhi_export_only",
];
const DEV_BUY_REASONS: &[&str] = &[
    "creator_unknown",
    "no_eligible_buy",
    "create_signature_unavailable",
    "create_signature_not_matched",
    "failed_transaction_excluded",
    "duplicate_excluded",
    "dust_excluded",
    "legacy_first_observed_includes_accepted_failed",
    "primary_buy_counterfactual",
    "candidate_history_truncated",
    "compatibility_primary_includes_accepted_failed",
];
const TIMING_REASONS: &[&str] = &[
    "insufficient_transactions",
    "timestamp_unavailable",
    "ordering_identity_unavailable",
    "source_window_truncated",
    "exact_same_millisecond",
    "cluster_below_fifty_milliseconds",
    "recent_window",
    "legacy_transaction_count_denominator",
];
const TOP3_REASONS: &[&str] = &[
    "preferred_field_unavailable",
    "compatibility_alias_fallback",
    "preferred_alias_mismatch",
];
const FLIP_REASONS: &[&str] = &[
    "no_eligible_buyers",
    "no_anchor",
    "missing_stable_identity",
    "missing_stable_order",
    "missing_resolved_owner",
    "duplicate_event",
    "identity_order_conflict",
    "duplicate_order_conflict",
    "failed_transaction_excluded",
    "dust_excluded",
    "wallet_cap_reached",
    "reconnect_gap",
    "out_of_order_event",
    "arithmetic_overflow",
    "closed_non_flipper",
    "legacy_slot_gap_only",
];
const FUNDING_REASONS: &[&str] = &[
    "funding_lane_unavailable",
    "rolling_state_unavailable",
    "index_cold",
    "no_buyer_cohort",
    "insufficient_known_sources",
    "insufficient_non_neutral_support",
    "low_coverage",
    "neutral_only",
    "buyer_identity_unavailable",
    "buy_timestamp_unavailable",
    "no_retained_recipient_history",
    "lookback_window_exhausted",
    "no_prebuy_transfer_in_window",
    "same_slot_ordering_unavailable",
    "low_attribution_confidence",
    "absolute_attribution_too_small",
    "relative_funding_too_small",
    "per_recipient_history_overflow",
    "global_recipient_evicted",
    "legacy_scalar_presence_only",
];
const MANIPULATION_REASONS: &[&str] = &[
    "raw_field_absent",
    "legacy_default_zero",
    "legacy_default_false",
    "threshold_not_configured",
    "derived_in_policy",
    "momentum_without_broadening",
    "volume_spike_without_new_signers",
    "high_buy_pressure_with_high_top3",
    "fixed_size_or_ramping_pattern",
    "timing_bundle_concentration",
    "early_top3_concentration",
];
const RESERVE_REASONS: &[&str] = &[
    "bootstrap_first_update",
    "zero_delta_time",
    "fallback_state",
    "source_unavailable",
    "measured_zero",
];
const RECENT_REASONS: &[&str] = &[
    "empty_window",
    "sell_count_zero",
    "zero_denominator",
    "failed_transaction_excluded",
    "legacy_sell_zero_returns_buy_count",
    "logging_only",
];

const WIRE_OBJECT_LAYOUT: &[&str] = &["w: wire_schema_version", "d: projection_root"];
const ROOT_LAYOUT: &[&str] = &[
    "schema_version",
    "rollout_mode",
    "profile_id",
    "profile_hash",
    "metric_contract_effective_config_hash",
    "fee_topology_diversity_index",
    "dev_buy",
    "same_ms_tx_ratio",
    "top3_signer_volume_ratio",
    "flip_ratio",
    "funding_source_concentration",
    "fsc_evidence_status",
    "manipulation_contradiction",
    "reserve_velocity",
    "recent_buy_sell",
];
const FTDI_LAYOUT: &[&str] = &[
    "legacy_value",
    "value_v1",
    "unique_topology_count",
    "unique_buyer_sample_count",
    "buy_transaction_sample_count",
    "legacy_buy_tx_actionability",
    "unique_buyer_actionability_v2",
];
const DEV_BUY_LAYOUT: &[&str] = &[
    "tx_intel_first_observed",
    "mfs_first_observed",
    "mfs_primary_v1",
    "effective_policy",
    "creator_known",
    "create_signature_matched",
    "primary_selection_mode",
    "primary_eligible_buy_count",
];
const TIMING_LAYOUT: &[&str] = &[
    "legacy_exact",
    "exact_v1",
    "cluster_lt_50ms",
    "recent_exact",
];
const TOP3_LAYOUT: &[&str] = &[
    "preferred",
    "compatibility_alias",
    "effective",
    "preferred_alias_bitwise_equal",
    "used_compatibility_fallback",
];
const FLIP_LAYOUT: &[&str] = &[
    "legacy_slot_gap_ratio",
    "hybrid_v2_ratio",
    "eligible_buyer_count",
    "flipper_count",
    "wall_clock_window_ms",
    "max_slot_gap",
    "dump_ratio",
];
const FUNDING_LAYOUT: &[&str] = &[
    "legacy_source",
    "legacy_v1",
    "distinct_known_source_count",
    "known_source_sample_count",
    "fsc_v2",
    "known_coverage",
    "non_neutral_known_coverage",
    "known_buyer_count",
    "total_buyer_count",
];
const FSC_STATUS_LAYOUT: &[&str] = &[
    "compatibility_status",
    "legacy_scalar_present",
    "legacy_feature_status",
    "fsc_v2_status",
    "fsc_v2_coverage",
];
const MANIPULATION_LAYOUT: &[&str] = &[
    "legacy_numeric_envelope",
    "numeric_v2_envelope",
    "measured_fields_mask",
    "same_ms_tx_ratio",
    "bundle_suspicion_ratio",
    "top3_signer_volume_ratio",
    "hhi",
    "max_tx_per_signer",
    "dev_volume_ratio",
    "contradiction_score",
    "legacy_high_recorded_mask",
    "legacy_high_true_mask",
    "derived_high_evaluable_mask",
    "derived_high_true_mask",
];
const RESERVE_LAYOUT: &[&str] = &[
    "legacy_velocity",
    "velocity_v1",
    "previous_real_sol_reserves_lamports",
    "current_real_sol_reserves_lamports",
    "interval_ms",
    "accepted_update_count",
    "source_clock",
    "status",
];
const RECENT_LAYOUT: &[&str] = &[
    "legacy_scalar",
    "v1_envelope",
    "window_ms",
    "buy_count",
    "sell_count",
    "transaction_count",
    "buy_to_sell_ratio",
    "buy_share",
];
const ENVELOPE_LAYOUT: &[&str] = &[
    "contract_id",
    "contract_version",
    "surface_id",
    "authority_class",
    "rollout_role",
    "availability",
    "measurement_quality",
    "policy_actionable",
    "reasons",
];
const SURFACE_LAYOUT: &[&str] = &[
    "envelope",
    "value",
    "producer_id",
    "producer_schema_version",
    "source_cutoff",
];
const FIELD_LAYOUT: &[&str] = &["value", "availability", "measurement_quality", "reasons"];
const CUTOFF_LAYOUT: &[&str] = &["decision_timestamp_ms", "decision_slot"];
const REASON_SUMMARY_LAYOUT: &[&str] = &["codes", "omitted_count"];
const RATIO_LAYOUT: &[&str] = &[
    "surface",
    "numerator",
    "denominator",
    "population",
    "window_ms",
];

fn scalar<T: Serialize>(value: &T) -> Result<Value, MetricContractProjectionWireErrorV1> {
    serde_json::to_value(value).map_err(MetricContractProjectionWireErrorV1::Serialization)
}

fn decode_scalar<T: DeserializeOwned>(
    value: Value,
    path: &'static str,
) -> Result<T, MetricContractProjectionWireErrorV1> {
    serde_json::from_value(value)
        .map_err(|_| MetricContractProjectionWireErrorV1::InvalidValue(path))
}

fn enum_code<T: Serialize>(
    value: &T,
    table: &[&str],
    kind: &'static str,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    let Value::String(name) = scalar(value)? else {
        return Err(MetricContractProjectionWireErrorV1::InvalidValue(kind));
    };
    let code = table
        .iter()
        .position(|candidate| *candidate == name)
        .ok_or(MetricContractProjectionWireErrorV1::UnknownDomainEnum { kind, value: name })?;
    Ok(Value::from(code as u64))
}

fn decode_enum<T: DeserializeOwned>(
    value: Value,
    table: &[&str],
    kind: &'static str,
) -> Result<T, MetricContractProjectionWireErrorV1> {
    let code = value
        .as_u64()
        .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(kind))?;
    let name = table
        .get(code as usize)
        .ok_or(MetricContractProjectionWireErrorV1::InvalidEnumCode { kind, code })?;
    serde_json::from_value(Value::String((*name).to_string()))
        .map_err(|_| MetricContractProjectionWireErrorV1::InvalidValue(kind))
}

fn tuple(
    value: Value,
    path: &'static str,
    expected: usize,
) -> Result<Vec<Value>, MetricContractProjectionWireErrorV1> {
    let Value::Array(values) = value else {
        return Err(MetricContractProjectionWireErrorV1::InvalidValue(path));
    };
    if values.len() != expected {
        return Err(MetricContractProjectionWireErrorV1::TupleLength {
            path,
            expected,
            actual: values.len(),
        });
    }
    Ok(values)
}

fn nullable<T>(
    value: &CanonicalNullableV1<T>,
    encode: fn(&T) -> Result<Value, MetricContractProjectionWireErrorV1>,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    match value {
        CanonicalNullableV1::Null => Ok(Value::Null),
        CanonicalNullableV1::Value(value) => encode(value),
    }
}

fn decode_nullable<T>(
    value: Value,
    decode: fn(Value) -> Result<T, MetricContractProjectionWireErrorV1>,
) -> Result<CanonicalNullableV1<T>, MetricContractProjectionWireErrorV1> {
    if value.is_null() {
        Ok(CanonicalNullableV1::Null)
    } else {
        decode(value).map(CanonicalNullableV1::Value)
    }
}

fn reason_detail_table(family_code: usize) -> Option<&'static [&'static str]> {
    match family_code {
        0 => Some(LEGACY_STATUS_REASONS),
        1 => Some(LEGACY_DEGRADED_REASONS),
        2 => Some(LEGACY_UNAVAILABLE_REASONS),
        3 => Some(FTDI_REASONS),
        4 => Some(DEV_BUY_REASONS),
        5 => Some(TIMING_REASONS),
        6 => Some(TOP3_REASONS),
        7 => Some(FLIP_REASONS),
        8 => Some(FUNDING_REASONS),
        9 => Some(MANIPULATION_REASONS),
        10 => Some(RESERVE_REASONS),
        11 => Some(RECENT_REASONS),
        _ => None,
    }
}

fn encode_reason(
    reason: &MetricEvidenceReasonV1,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    let Value::Object(object) = scalar(reason)? else {
        return Err(MetricContractProjectionWireErrorV1::InvalidValue("reason"));
    };
    let family = object.get("reason_family").and_then(Value::as_str).ok_or(
        MetricContractProjectionWireErrorV1::InvalidValue("reason.family"),
    )?;
    let family_code = REASON_FAMILIES
        .iter()
        .position(|candidate| *candidate == family)
        .ok_or(MetricContractProjectionWireErrorV1::UnknownDomainEnum {
            kind: "reason family",
            value: family.to_string(),
        })?;
    let detail = object
        .get("detail")
        .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(
            "reason.detail",
        ))?;
    if family_code == 12 {
        let contract =
            detail
                .get("contract_id")
                .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(
                    "reason.contract",
                ))?;
        let raw = detail.get("raw").and_then(Value::as_str).ok_or(
            MetricContractProjectionWireErrorV1::InvalidValue("reason.raw"),
        )?;
        let contract: MetricContractId = decode_scalar(contract.clone(), "reason.contract")?;
        return Ok(Value::Array(vec![
            Value::from(family_code as u64),
            enum_code(&contract, CONTRACT_IDS, "contract id")?,
            Value::String(raw.to_string()),
        ]));
    }
    let detail_name = detail
        .as_str()
        .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(
            "reason.detail",
        ))?;
    let table = reason_detail_table(family_code).ok_or(
        MetricContractProjectionWireErrorV1::InvalidValue("reason family"),
    )?;
    let detail_code = table
        .iter()
        .position(|candidate| *candidate == detail_name)
        .ok_or(MetricContractProjectionWireErrorV1::UnknownDomainEnum {
            kind: "reason detail",
            value: detail_name.to_string(),
        })?;
    Ok(Value::Array(vec![
        Value::from(family_code as u64),
        Value::from(detail_code as u64),
    ]))
}

fn decode_reason(
    value: Value,
) -> Result<MetricEvidenceReasonV1, MetricContractProjectionWireErrorV1> {
    let Value::Array(values) = value else {
        return Err(MetricContractProjectionWireErrorV1::InvalidValue("reason"));
    };
    let family_code = values.first().and_then(Value::as_u64).ok_or(
        MetricContractProjectionWireErrorV1::InvalidValue("reason.family"),
    )?;
    let family = REASON_FAMILIES.get(family_code as usize).ok_or(
        MetricContractProjectionWireErrorV1::InvalidEnumCode {
            kind: "reason family",
            code: family_code,
        },
    )?;
    let domain = if family_code == 12 {
        if values.len() != 3 {
            return Err(MetricContractProjectionWireErrorV1::TupleLength {
                path: "reason.unmapped",
                expected: 3,
                actual: values.len(),
            });
        }
        let contract: MetricContractId =
            decode_enum(values[1].clone(), CONTRACT_IDS, "contract id")?;
        let raw = values[2]
            .as_str()
            .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(
                "reason.raw",
            ))?;
        serde_json::json!({
            "reason_family": family,
            "detail": { "contract_id": contract, "raw": raw }
        })
    } else {
        if values.len() != 2 {
            return Err(MetricContractProjectionWireErrorV1::TupleLength {
                path: "reason.typed",
                expected: 2,
                actual: values.len(),
            });
        }
        let detail_code =
            values[1]
                .as_u64()
                .ok_or(MetricContractProjectionWireErrorV1::InvalidValue(
                    "reason.detail",
                ))?;
        let table = reason_detail_table(family_code as usize).ok_or(
            MetricContractProjectionWireErrorV1::InvalidValue("reason family"),
        )?;
        let detail = table.get(detail_code as usize).ok_or(
            MetricContractProjectionWireErrorV1::InvalidEnumCode {
                kind: "reason detail",
                code: detail_code,
            },
        )?;
        serde_json::json!({ "reason_family": family, "detail": detail })
    };
    serde_json::from_value(domain)
        .map_err(|_| MetricContractProjectionWireErrorV1::InvalidValue("reason"))
}

fn encode_reasons(
    value: &MetricDecisionReasonSummaryV1,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        Value::Array(
            value
                .codes
                .iter()
                .map(encode_reason)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::from(value.omitted_count),
    ]))
}

fn decode_reasons(
    value: Value,
) -> Result<MetricDecisionReasonSummaryV1, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "reason summary", 2)?;
    let codes = match values.remove(0) {
        Value::Array(values) => values
            .into_iter()
            .map(decode_reason)
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(MetricContractProjectionWireErrorV1::InvalidValue(
                "reason summary.codes",
            ))
        }
    };
    Ok(MetricDecisionReasonSummaryV1 {
        codes,
        omitted_count: decode_scalar(values.remove(0), "reason summary.omitted_count")?,
    })
}

fn encode_envelope(
    value: &MetricDecisionEnvelopeV1,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        enum_code(&value.contract_id, CONTRACT_IDS, "contract id")?,
        Value::from(value.contract_version),
        enum_code(&value.surface_id, SURFACE_IDS, "surface id")?,
        enum_code(&value.authority_class, AUTHORITY_CLASSES, "authority class")?,
        enum_code(&value.rollout_role, ROLLOUT_ROLES, "rollout role")?,
        enum_code(&value.availability, AVAILABILITIES, "availability")?,
        enum_code(&value.measurement_quality, QUALITIES, "measurement quality")?,
        Value::Bool(value.policy_actionable),
        encode_reasons(&value.reasons)?,
    ]))
}

fn decode_envelope(
    value: Value,
) -> Result<MetricDecisionEnvelopeV1, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "decision envelope", 9)?;
    Ok(MetricDecisionEnvelopeV1 {
        contract_id: decode_enum(values.remove(0), CONTRACT_IDS, "contract id")?,
        contract_version: decode_scalar(values.remove(0), "decision envelope.contract_version")?,
        surface_id: decode_enum(values.remove(0), SURFACE_IDS, "surface id")?,
        authority_class: decode_enum(values.remove(0), AUTHORITY_CLASSES, "authority class")?,
        rollout_role: decode_enum(values.remove(0), ROLLOUT_ROLES, "rollout role")?,
        availability: decode_enum(values.remove(0), AVAILABILITIES, "availability")?,
        measurement_quality: decode_enum(values.remove(0), QUALITIES, "measurement quality")?,
        policy_actionable: decode_scalar(values.remove(0), "decision envelope.policy_actionable")?,
        reasons: decode_reasons(values.remove(0))?,
    })
}

fn encode_cutoff(
    value: &MetricContractDecisionSourceCutoffV1,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        scalar(&value.decision_timestamp_ms)?,
        scalar(&value.decision_slot)?,
    ]))
}

fn decode_cutoff(
    value: Value,
) -> Result<MetricContractDecisionSourceCutoffV1, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "source cutoff", 2)?;
    Ok(MetricContractDecisionSourceCutoffV1 {
        decision_timestamp_ms: decode_scalar(values.remove(0), "source cutoff.timestamp")?,
        decision_slot: decode_scalar(values.remove(0), "source cutoff.slot")?,
    })
}

fn encode_surface<T>(
    value: &MetricDecisionSurfaceValueV1<T>,
    encode_value: fn(&T) -> Result<Value, MetricContractProjectionWireErrorV1>,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        encode_envelope(&value.envelope)?,
        nullable(&value.value, encode_value)?,
        enum_code(&value.producer_id, PRODUCERS, "producer id")?,
        Value::from(value.producer_schema_version),
        encode_cutoff(&value.source_cutoff)?,
    ]))
}

fn decode_surface<T>(
    value: Value,
    decode_value: fn(Value) -> Result<T, MetricContractProjectionWireErrorV1>,
) -> Result<MetricDecisionSurfaceValueV1<T>, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "surface value", 5)?;
    Ok(MetricDecisionSurfaceValueV1 {
        envelope: decode_envelope(values.remove(0))?,
        value: decode_nullable(values.remove(0), decode_value)?,
        producer_id: decode_enum(values.remove(0), PRODUCERS, "producer id")?,
        producer_schema_version: decode_scalar(values.remove(0), "surface producer schema")?,
        source_cutoff: decode_cutoff(values.remove(0))?,
    })
}

fn encode_field<T>(
    value: &MetricDecisionFieldValueV1<T>,
    encode_value: fn(&T) -> Result<Value, MetricContractProjectionWireErrorV1>,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        nullable(&value.value, encode_value)?,
        enum_code(&value.availability, AVAILABILITIES, "availability")?,
        enum_code(&value.measurement_quality, QUALITIES, "measurement quality")?,
        encode_reasons(&value.reasons)?,
    ]))
}

fn decode_field<T>(
    value: Value,
    decode_value: fn(Value) -> Result<T, MetricContractProjectionWireErrorV1>,
) -> Result<MetricDecisionFieldValueV1<T>, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "field value", 4)?;
    Ok(MetricDecisionFieldValueV1 {
        value: decode_nullable(values.remove(0), decode_value)?,
        availability: decode_enum(values.remove(0), AVAILABILITIES, "availability")?,
        measurement_quality: decode_enum(values.remove(0), QUALITIES, "measurement quality")?,
        reasons: decode_reasons(values.remove(0))?,
    })
}

fn enc_f64(value: &f64) -> Result<Value, MetricContractProjectionWireErrorV1> {
    scalar(value)
}
fn dec_f64(value: Value) -> Result<f64, MetricContractProjectionWireErrorV1> {
    decode_scalar(value, "f64")
}
fn enc_bool(value: &bool) -> Result<Value, MetricContractProjectionWireErrorV1> {
    scalar(value)
}
fn dec_bool(value: Value) -> Result<bool, MetricContractProjectionWireErrorV1> {
    decode_scalar(value, "bool")
}
fn enc_u32(value: &u32) -> Result<Value, MetricContractProjectionWireErrorV1> {
    scalar(value)
}
fn dec_u32(value: Value) -> Result<u32, MetricContractProjectionWireErrorV1> {
    decode_scalar(value, "u32")
}
fn enc_u64s(value: &CanonicalU64StringV1) -> Result<Value, MetricContractProjectionWireErrorV1> {
    scalar(value)
}
fn dec_u64s(value: Value) -> Result<CanonicalU64StringV1, MetricContractProjectionWireErrorV1> {
    decode_scalar(value, "canonical u64 string")
}
fn enc_fsc(value: &FscEvidenceStatus) -> Result<Value, MetricContractProjectionWireErrorV1> {
    enum_code(value, FSC_STATUSES, "FSC status")
}
fn dec_fsc(value: Value) -> Result<FscEvidenceStatus, MetricContractProjectionWireErrorV1> {
    decode_enum(value, FSC_STATUSES, "FSC status")
}
fn enc_evidence_status(
    value: &EvidenceStatus,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    enum_code(value, EVIDENCE_STATUSES, "evidence status")
}
fn dec_evidence_status(
    value: Value,
) -> Result<EvidenceStatus, MetricContractProjectionWireErrorV1> {
    decode_enum(value, EVIDENCE_STATUSES, "evidence status")
}

fn encode_ratio(
    value: &MetricDecisionRatioV1,
) -> Result<Value, MetricContractProjectionWireErrorV1> {
    Ok(Value::Array(vec![
        encode_surface(&value.surface, enc_f64)?,
        Value::from(value.numerator),
        Value::from(value.denominator),
        enum_code(&value.population, TIMING_POPULATIONS, "timing population")?,
        scalar(&value.window_ms)?,
    ]))
}

fn decode_ratio(
    value: Value,
) -> Result<MetricDecisionRatioV1, MetricContractProjectionWireErrorV1> {
    let mut values = tuple(value, "decision ratio", 5)?;
    Ok(MetricDecisionRatioV1 {
        surface: decode_surface(values.remove(0), dec_f64)?,
        numerator: decode_scalar(values.remove(0), "ratio numerator")?,
        denominator: decode_scalar(values.remove(0), "ratio denominator")?,
        population: decode_enum(values.remove(0), TIMING_POPULATIONS, "timing population")?,
        window_ms: decode_scalar(values.remove(0), "ratio window")?,
    })
}

impl MetricContractDecisionProjectionWireV1 {
    pub fn try_from_domain(
        projection: &MetricContractDecisionEvidenceProjectionV1,
    ) -> Result<Self, MetricContractProjectionWireErrorV1> {
        let ftdi = &projection.fee_topology_diversity_index;
        let dev = &projection.dev_buy;
        let timing = &projection.same_ms_tx_ratio;
        let top3 = &projection.top3_signer_volume_ratio;
        let flip = &projection.flip_ratio;
        let funding = &projection.funding_source_concentration;
        let fsc_status = &projection.fsc_evidence_status;
        let manipulation = &projection.manipulation_contradiction;
        let reserve = &projection.reserve_velocity;
        let recent = &projection.recent_buy_sell;
        Ok(Self {
            w: METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
            d: vec![
                Value::from(projection.schema_version),
                enum_code(&projection.rollout_mode, ROLLOUT_MODES, "rollout mode")?,
                enum_code(&projection.profile_id, PROFILE_IDS, "profile id")?,
                scalar(&projection.profile_hash)?,
                scalar(&projection.metric_contract_effective_config_hash)?,
                Value::Array(vec![
                    encode_surface(&ftdi.legacy_value, enc_f64)?,
                    encode_surface(&ftdi.value_v1, enc_f64)?,
                    Value::from(ftdi.unique_topology_count),
                    Value::from(ftdi.unique_buyer_sample_count),
                    Value::from(ftdi.buy_transaction_sample_count),
                    encode_surface(&ftdi.legacy_buy_tx_actionability, enc_bool)?,
                    encode_surface(&ftdi.unique_buyer_actionability_v2, enc_bool)?,
                ]),
                Value::Array(vec![
                    encode_surface(&dev.tx_intel_first_observed, enc_f64)?,
                    encode_surface(&dev.mfs_first_observed, enc_f64)?,
                    encode_surface(&dev.mfs_primary_v1, enc_f64)?,
                    encode_surface(&dev.effective_policy, enc_f64)?,
                    Value::Bool(dev.creator_known),
                    Value::Bool(dev.create_signature_matched),
                    enum_code(
                        &dev.primary_selection_mode,
                        DEV_SELECTION_MODES,
                        "dev selection mode",
                    )?,
                    Value::from(dev.primary_eligible_buy_count),
                ]),
                Value::Array(vec![
                    encode_ratio(&timing.legacy_exact)?,
                    encode_ratio(&timing.exact_v1)?,
                    encode_ratio(&timing.cluster_lt_50ms)?,
                    encode_ratio(&timing.recent_exact)?,
                ]),
                Value::Array(vec![
                    encode_surface(&top3.preferred, enc_f64)?,
                    encode_surface(&top3.compatibility_alias, enc_f64)?,
                    encode_surface(&top3.effective, enc_f64)?,
                    scalar(&top3.preferred_alias_bitwise_equal)?,
                    Value::Bool(top3.used_compatibility_fallback),
                ]),
                Value::Array(vec![
                    encode_surface(&flip.legacy_slot_gap_ratio, enc_f64)?,
                    encode_surface(&flip.hybrid_v2_ratio, enc_f64)?,
                    Value::from(flip.eligible_buyer_count),
                    Value::from(flip.flipper_count),
                    Value::from(flip.wall_clock_window_ms),
                    Value::from(flip.max_slot_gap),
                    scalar(&flip.dump_ratio)?,
                ]),
                Value::Array(vec![
                    encode_surface(&funding.legacy_source, enc_f64)?,
                    encode_surface(&funding.legacy_v1, enc_f64)?,
                    Value::from(funding.distinct_known_source_count),
                    Value::from(funding.known_source_sample_count),
                    encode_surface(&funding.fsc_v2, enc_fsc)?,
                    encode_field(&funding.known_coverage, enc_f64)?,
                    encode_field(&funding.non_neutral_known_coverage, enc_f64)?,
                    Value::from(funding.known_buyer_count),
                    Value::from(funding.total_buyer_count),
                ]),
                Value::Array(vec![
                    encode_surface(&fsc_status.compatibility_status, enc_evidence_status)?,
                    Value::Bool(fsc_status.legacy_scalar_present),
                    enum_code(
                        &fsc_status.legacy_feature_status,
                        EVIDENCE_STATUSES,
                        "evidence status",
                    )?,
                    nullable(&fsc_status.fsc_v2_status, enc_fsc)?,
                    scalar(&fsc_status.fsc_v2_coverage)?,
                ]),
                Value::Array(vec![
                    encode_envelope(&manipulation.legacy_numeric_envelope)?,
                    encode_envelope(&manipulation.numeric_v2_envelope)?,
                    Value::from(manipulation.measured_fields_mask),
                    encode_field(&manipulation.same_ms_tx_ratio, enc_f64)?,
                    encode_field(&manipulation.bundle_suspicion_ratio, enc_f64)?,
                    encode_field(&manipulation.top3_signer_volume_ratio, enc_f64)?,
                    encode_field(&manipulation.hhi, enc_f64)?,
                    encode_field(&manipulation.max_tx_per_signer, enc_f64)?,
                    encode_field(&manipulation.dev_volume_ratio, enc_f64)?,
                    encode_field(&manipulation.contradiction_score, enc_f64)?,
                    Value::from(manipulation.legacy_high_recorded_mask),
                    Value::from(manipulation.legacy_high_true_mask),
                    Value::from(manipulation.derived_high_evaluable_mask),
                    Value::from(manipulation.derived_high_true_mask),
                ]),
                Value::Array(vec![
                    encode_surface(&reserve.legacy_velocity, enc_f64)?,
                    encode_surface(&reserve.velocity_v1, enc_f64)?,
                    encode_field(&reserve.previous_real_sol_reserves_lamports, enc_u64s)?,
                    encode_field(&reserve.current_real_sol_reserves_lamports, enc_u64s)?,
                    encode_field(&reserve.interval_ms, enc_u32)?,
                    Value::from(reserve.accepted_update_count),
                    enum_code(
                        &reserve.source_clock,
                        RESERVE_SOURCE_CLOCKS,
                        "reserve source clock",
                    )?,
                    enum_code(&reserve.status, RESERVE_STATUSES, "reserve status")?,
                ]),
                Value::Array(vec![
                    encode_surface(&recent.legacy_scalar, enc_f64)?,
                    encode_envelope(&recent.v1_envelope)?,
                    Value::from(recent.window_ms),
                    Value::from(recent.buy_count),
                    Value::from(recent.sell_count),
                    Value::from(recent.transaction_count),
                    encode_field(&recent.buy_to_sell_ratio, enc_f64)?,
                    encode_field(&recent.buy_share, enc_f64)?,
                ]),
            ],
        })
    }

    pub fn try_into_domain(
        self,
    ) -> Result<MetricContractDecisionEvidenceProjectionV1, MetricContractProjectionWireErrorV1>
    {
        if self.w != METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1 {
            return Err(MetricContractProjectionWireErrorV1::UnsupportedVersion(
                self.w,
            ));
        }
        let mut root = tuple(Value::Array(self.d), "projection root", 15)?;
        let schema_version = decode_scalar(root.remove(0), "root.schema_version")?;
        let rollout_mode = decode_enum(root.remove(0), ROLLOUT_MODES, "rollout mode")?;
        let profile_id = decode_enum(root.remove(0), PROFILE_IDS, "profile id")?;
        let profile_hash = decode_scalar(root.remove(0), "root.profile_hash")?;
        let metric_contract_effective_config_hash =
            decode_scalar(root.remove(0), "root.config_hash")?;

        let mut f = tuple(root.remove(0), "family.ftdi", 7)?;
        let fee_topology_diversity_index = FtdiDecisionProjectionV1 {
            legacy_value: decode_surface(f.remove(0), dec_f64)?,
            value_v1: decode_surface(f.remove(0), dec_f64)?,
            unique_topology_count: decode_scalar(f.remove(0), "ftdi.unique_topology_count")?,
            unique_buyer_sample_count: decode_scalar(
                f.remove(0),
                "ftdi.unique_buyer_sample_count",
            )?,
            buy_transaction_sample_count: decode_scalar(
                f.remove(0),
                "ftdi.buy_transaction_sample_count",
            )?,
            legacy_buy_tx_actionability: decode_surface(f.remove(0), dec_bool)?,
            unique_buyer_actionability_v2: decode_surface(f.remove(0), dec_bool)?,
        };
        let mut f = tuple(root.remove(0), "family.dev_buy", 8)?;
        let dev_buy = DevBuyDecisionProjectionV1 {
            tx_intel_first_observed: decode_surface(f.remove(0), dec_f64)?,
            mfs_first_observed: decode_surface(f.remove(0), dec_f64)?,
            mfs_primary_v1: decode_surface(f.remove(0), dec_f64)?,
            effective_policy: decode_surface(f.remove(0), dec_f64)?,
            creator_known: decode_scalar(f.remove(0), "dev.creator_known")?,
            create_signature_matched: decode_scalar(f.remove(0), "dev.signature_matched")?,
            primary_selection_mode: decode_enum(
                f.remove(0),
                DEV_SELECTION_MODES,
                "dev selection mode",
            )?,
            primary_eligible_buy_count: decode_scalar(f.remove(0), "dev.eligible_buy_count")?,
        };
        let mut f = tuple(root.remove(0), "family.timing", 4)?;
        let same_ms_tx_ratio = TxTimingDecisionProjectionV1 {
            legacy_exact: decode_ratio(f.remove(0))?,
            exact_v1: decode_ratio(f.remove(0))?,
            cluster_lt_50ms: decode_ratio(f.remove(0))?,
            recent_exact: decode_ratio(f.remove(0))?,
        };
        let mut f = tuple(root.remove(0), "family.top3", 5)?;
        let top3_signer_volume_ratio = Top3DecisionProjectionV1 {
            preferred: decode_surface(f.remove(0), dec_f64)?,
            compatibility_alias: decode_surface(f.remove(0), dec_f64)?,
            effective: decode_surface(f.remove(0), dec_f64)?,
            preferred_alias_bitwise_equal: decode_scalar(f.remove(0), "top3.bitwise_equal")?,
            used_compatibility_fallback: decode_scalar(f.remove(0), "top3.fallback")?,
        };
        let mut f = tuple(root.remove(0), "family.flip", 7)?;
        let flip_ratio = FlipDecisionProjectionV1 {
            legacy_slot_gap_ratio: decode_surface(f.remove(0), dec_f64)?,
            hybrid_v2_ratio: decode_surface(f.remove(0), dec_f64)?,
            eligible_buyer_count: decode_scalar(f.remove(0), "flip.buyers")?,
            flipper_count: decode_scalar(f.remove(0), "flip.flippers")?,
            wall_clock_window_ms: decode_scalar(f.remove(0), "flip.window")?,
            max_slot_gap: decode_scalar(f.remove(0), "flip.slot_gap")?,
            dump_ratio: decode_scalar(f.remove(0), "flip.dump_ratio")?,
        };
        let mut f = tuple(root.remove(0), "family.funding", 9)?;
        let funding_source_concentration = FundingDecisionProjectionV1 {
            legacy_source: decode_surface(f.remove(0), dec_f64)?,
            legacy_v1: decode_surface(f.remove(0), dec_f64)?,
            distinct_known_source_count: decode_scalar(f.remove(0), "funding.distinct")?,
            known_source_sample_count: decode_scalar(f.remove(0), "funding.samples")?,
            fsc_v2: decode_surface(f.remove(0), dec_fsc)?,
            known_coverage: decode_field(f.remove(0), dec_f64)?,
            non_neutral_known_coverage: decode_field(f.remove(0), dec_f64)?,
            known_buyer_count: decode_scalar(f.remove(0), "funding.known_buyers")?,
            total_buyer_count: decode_scalar(f.remove(0), "funding.total_buyers")?,
        };
        let mut f = tuple(root.remove(0), "family.fsc_status", 5)?;
        let fsc_evidence_status = FscStatusDecisionProjectionV1 {
            compatibility_status: decode_surface(f.remove(0), dec_evidence_status)?,
            legacy_scalar_present: decode_scalar(f.remove(0), "fsc_status.legacy_present")?,
            legacy_feature_status: decode_enum(f.remove(0), EVIDENCE_STATUSES, "evidence status")?,
            fsc_v2_status: decode_nullable(f.remove(0), dec_fsc)?,
            fsc_v2_coverage: decode_scalar(f.remove(0), "fsc_status.coverage")?,
        };
        let mut f = tuple(root.remove(0), "family.manipulation", 14)?;
        let manipulation_contradiction = ManipulationDecisionProjectionV1 {
            legacy_numeric_envelope: decode_envelope(f.remove(0))?,
            numeric_v2_envelope: decode_envelope(f.remove(0))?,
            measured_fields_mask: decode_scalar(f.remove(0), "manipulation.measured_mask")?,
            same_ms_tx_ratio: decode_field(f.remove(0), dec_f64)?,
            bundle_suspicion_ratio: decode_field(f.remove(0), dec_f64)?,
            top3_signer_volume_ratio: decode_field(f.remove(0), dec_f64)?,
            hhi: decode_field(f.remove(0), dec_f64)?,
            max_tx_per_signer: decode_field(f.remove(0), dec_f64)?,
            dev_volume_ratio: decode_field(f.remove(0), dec_f64)?,
            contradiction_score: decode_field(f.remove(0), dec_f64)?,
            legacy_high_recorded_mask: decode_scalar(f.remove(0), "manipulation.legacy_recorded")?,
            legacy_high_true_mask: decode_scalar(f.remove(0), "manipulation.legacy_true")?,
            derived_high_evaluable_mask: decode_scalar(
                f.remove(0),
                "manipulation.derived_evaluable",
            )?,
            derived_high_true_mask: decode_scalar(f.remove(0), "manipulation.derived_true")?,
        };
        let mut f = tuple(root.remove(0), "family.reserve", 8)?;
        let reserve_velocity = ReserveVelocityDecisionProjectionV1 {
            legacy_velocity: decode_surface(f.remove(0), dec_f64)?,
            velocity_v1: decode_surface(f.remove(0), dec_f64)?,
            previous_real_sol_reserves_lamports: decode_field(f.remove(0), dec_u64s)?,
            current_real_sol_reserves_lamports: decode_field(f.remove(0), dec_u64s)?,
            interval_ms: decode_field(f.remove(0), dec_u32)?,
            accepted_update_count: decode_scalar(f.remove(0), "reserve.accepted_updates")?,
            source_clock: decode_enum(f.remove(0), RESERVE_SOURCE_CLOCKS, "reserve source clock")?,
            status: decode_enum(f.remove(0), RESERVE_STATUSES, "reserve status")?,
        };
        let mut f = tuple(root.remove(0), "family.recent", 8)?;
        let recent_buy_sell = RecentBuySellDecisionProjectionV1 {
            legacy_scalar: decode_surface(f.remove(0), dec_f64)?,
            v1_envelope: decode_envelope(f.remove(0))?,
            window_ms: decode_scalar(f.remove(0), "recent.window")?,
            buy_count: decode_scalar(f.remove(0), "recent.buy_count")?,
            sell_count: decode_scalar(f.remove(0), "recent.sell_count")?,
            transaction_count: decode_scalar(f.remove(0), "recent.tx_count")?,
            buy_to_sell_ratio: decode_field(f.remove(0), dec_f64)?,
            buy_share: decode_field(f.remove(0), dec_f64)?,
        };
        Ok(MetricContractDecisionEvidenceProjectionV1 {
            schema_version,
            rollout_mode,
            profile_id,
            profile_hash,
            metric_contract_effective_config_hash,
            fee_topology_diversity_index,
            dev_buy,
            same_ms_tx_ratio,
            top3_signer_volume_ratio,
            flip_ratio,
            funding_source_concentration,
            fsc_evidence_status,
            manipulation_contradiction,
            reserve_velocity,
            recent_buy_sell,
        })
    }

    pub fn json_bytes(&self) -> Result<Vec<u8>, MetricContractProjectionWireErrorV1> {
        serde_json::to_vec(self).map_err(MetricContractProjectionWireErrorV1::Serialization)
    }
}

pub fn serialize_optional_projection_wire_v1<S>(
    value: &Option<MetricContractDecisionEvidenceProjectionV1>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(projection) => MetricContractDecisionProjectionWireV1::try_from_domain(projection)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize_optional_projection_wire_v1<'de, D>(
    deserializer: D,
) -> Result<Option<MetricContractDecisionEvidenceProjectionV1>, D::Error>
where
    D: Deserializer<'de>,
{
    let wire = MetricContractDecisionProjectionWireV1::deserialize(deserializer)?;
    wire.try_into_domain()
        .map(Some)
        .map_err(serde::de::Error::custom)
}

pub fn metric_contract_projection_wire_v1_mapping_tables(
) -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("contract_id", CONTRACT_IDS),
        ("surface_id", SURFACE_IDS),
        ("rollout_mode", ROLLOUT_MODES),
        ("profile_id", PROFILE_IDS),
        ("authority_class", AUTHORITY_CLASSES),
        ("rollout_role", ROLLOUT_ROLES),
        ("availability", AVAILABILITIES),
        ("measurement_quality", QUALITIES),
        ("producer_id", PRODUCERS),
        ("dev_selection_mode", DEV_SELECTION_MODES),
        ("timing_population", TIMING_POPULATIONS),
        ("evidence_status", EVIDENCE_STATUSES),
        ("fsc_status", FSC_STATUSES),
        ("reserve_source_clock", RESERVE_SOURCE_CLOCKS),
        ("reserve_status", RESERVE_STATUSES),
        ("reason_family", REASON_FAMILIES),
        ("reason.legacy_status", LEGACY_STATUS_REASONS),
        ("reason.legacy_degraded", LEGACY_DEGRADED_REASONS),
        ("reason.legacy_unavailable", LEGACY_UNAVAILABLE_REASONS),
        ("reason.ftdi", FTDI_REASONS),
        ("reason.dev_buy", DEV_BUY_REASONS),
        ("reason.tx_timing", TIMING_REASONS),
        ("reason.top3", TOP3_REASONS),
        ("reason.flip", FLIP_REASONS),
        ("reason.funding_source", FUNDING_REASONS),
        ("reason.manipulation", MANIPULATION_REASONS),
        ("reason.reserve_velocity", RESERVE_REASONS),
        ("reason.recent_buy_sell", RECENT_REASONS),
    ]
}

/// Closed position-to-domain-field contract for Compact JSON Wire V1. The
/// array index is the wire position; changing any entry requires Wire V2.
pub fn metric_contract_projection_wire_v1_tuple_layouts(
) -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("wire_object", WIRE_OBJECT_LAYOUT),
        ("root", ROOT_LAYOUT),
        ("family.ftdi", FTDI_LAYOUT),
        ("family.dev_buy", DEV_BUY_LAYOUT),
        ("family.timing", TIMING_LAYOUT),
        ("family.top3", TOP3_LAYOUT),
        ("family.flip", FLIP_LAYOUT),
        ("family.funding", FUNDING_LAYOUT),
        ("family.fsc_status", FSC_STATUS_LAYOUT),
        ("family.manipulation", MANIPULATION_LAYOUT),
        ("family.reserve", RESERVE_LAYOUT),
        ("family.recent", RECENT_LAYOUT),
        ("common.envelope", ENVELOPE_LAYOUT),
        ("common.surface", SURFACE_LAYOUT),
        ("common.field", FIELD_LAYOUT),
        ("common.cutoff", CUTOFF_LAYOUT),
        ("common.reason_summary", REASON_SUMMARY_LAYOUT),
        ("common.ratio", RATIO_LAYOUT),
    ]
}
