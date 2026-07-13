use super::{
    CanonicalHashErrorV1, CanonicalHashV1, CanonicalNullableV1, CanonicalU128StringV1,
    CanonicalU64StringV1, MetricAvailabilityV1, MetricContractId, MetricContractProfileIdV1,
    MetricContractProfileV1, MetricContractRolloutMode, MetricEvidenceEnvelopeErrorV1,
    MetricEvidenceEnvelopeV1, MetricEvidenceReasonV1, MetricEvidenceRecordIdentityV1,
    MetricMeasurementQualityV1, MetricSurfaceId, StableEventIdentityV1,
};
use crate::checkpoint::EvidenceStatus;
use crate::tx_intelligence::types::FscEvidenceStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Current decision schema retained by PR1. This constant is a compatibility
/// assertion, not an activation of schema v34.
pub const LEGACY_GATEKEEPER_DECISION_SCHEMA_VERSION_V33: u32 = 33;
pub const LEGACY_V3_REPLAY_PAYLOAD_SCHEMA_VERSION_V1: u16 = 1;
pub const METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34: u32 = 34;
pub const METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1: u16 = 1;

pub type CanonicalMetricEnvelopeV1 = MetricEvidenceEnvelopeV1<MetricEvidenceReasonV1>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FtdiValueMeasurementV1 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub value: CanonicalNullableV1<f64>,
    pub unique_topology_count: u32,
    pub unique_buyer_sample_count: u32,
    pub buy_transaction_sample_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FtdiEvidenceV1 {
    pub legacy_value: FtdiValueMeasurementV1,
    pub value_v1: FtdiValueMeasurementV1,
    pub legacy_actionability_envelope: CanonicalMetricEnvelopeV1,
    pub legacy_buy_tx_actionable: bool,
    pub unique_buyer_actionability_v2_envelope: CanonicalMetricEnvelopeV1,
    pub unique_buyer_actionable_v2: bool,
    pub coordination_hhi_export_envelope: CanonicalMetricEnvelopeV1,
    pub coordination_hhi: CanonicalNullableV1<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevBuySelectionModeV1 {
    LegacyFirstObserved,
    CreateSignatureMatch,
    EarliestEligibleCreatorBuy,
    NoEligibleBuy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevBuyEvidenceV1 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub amount_sol: CanonicalNullableV1<f64>,
    pub creator_known: bool,
    pub create_signature: CanonicalNullableV1<String>,
    pub create_signature_matched: bool,
    pub selection_mode: DevBuySelectionModeV1,
    pub selected_signature: CanonicalNullableV1<String>,
    pub selected_slot: CanonicalNullableV1<CanonicalU64StringV1>,
    pub selected_transaction_index: CanonicalNullableV1<u32>,
    pub eligible_buy_count: u32,
}

/// Surface-qualified dev evidence. Similar historical field names are never
/// used as a substitute for an explicit producer/materialization/policy role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevBuyContractEvidenceV1 {
    pub tx_intel_first_observed: DevBuyEvidenceV1,
    pub gatekeeper_buffer_primary: DevBuyEvidenceV1,
    pub mfs_first_observed: DevBuyEvidenceV1,
    pub mfs_primary_v1: DevBuyEvidenceV1,
    pub effective_policy: DevBuyEvidenceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxTimingSourceV1 {
    TxIntelFullObservationExactLegacy,
    TxTimingFullObservationExactV1,
    PhaseDiversityClusterLt50Ms,
    RceRecentExact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxTimingPopulationV1 {
    AcceptedTransactions,
    SuccessfulTransactions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxTimingMeasurementEvidenceV1 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub source: TxTimingSourceV1,
    pub population: TxTimingPopulationV1,
    pub canonical_dedupe_applied: bool,
    pub dust_filter_sol: CanonicalNullableV1<f64>,
    pub window_ms: CanonicalNullableV1<u32>,
    pub numerator: u32,
    pub denominator: u32,
    pub ratio: CanonicalNullableV1<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxTimingEvidenceV1 {
    pub legacy_exact: TxTimingMeasurementEvidenceV1,
    pub exact_v1: TxTimingMeasurementEvidenceV1,
    pub cluster_lt_50ms: TxTimingMeasurementEvidenceV1,
    pub recent_exact: TxTimingMeasurementEvidenceV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Top3SignerVolumeEvidenceV1 {
    pub preferred_envelope: CanonicalMetricEnvelopeV1,
    pub preferred_ratio: CanonicalNullableV1<f64>,
    pub compatibility_alias_envelope: CanonicalMetricEnvelopeV1,
    pub compatibility_alias_ratio: CanonicalNullableV1<f64>,
    pub effective_selector_envelope: CanonicalMetricEnvelopeV1,
    pub effective_ratio: CanonicalNullableV1<f64>,
    pub preferred_alias_bitwise_equal: CanonicalNullableV1<bool>,
    pub used_compatibility_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipOwnerStatusV2 {
    NoAnchor,
    Tracking,
    Flipper,
    ClosedNonFlipper,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlipOwnerEvidenceV2 {
    pub owner_id: String,
    pub status: FlipOwnerStatusV2,
    pub anchor_event_identity: CanonicalNullableV1<StableEventIdentityV1>,
    pub anchor_slot: CanonicalNullableV1<CanonicalU64StringV1>,
    pub anchor_timestamp_ms: CanonicalNullableV1<CanonicalU64StringV1>,
    pub pre_anchor_sell_count: u32,
    pub cumulative_eligible_buy_tokens: CanonicalU128StringV1,
    pub cumulative_eligible_sell_tokens: CanonicalU128StringV1,
    pub qualifying_sell_event_identity: CanonicalNullableV1<StableEventIdentityV1>,
    pub qualifying_sell_slot: CanonicalNullableV1<CanonicalU64StringV1>,
    pub qualifying_sell_timestamp_ms: CanonicalNullableV1<CanonicalU64StringV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlipRatioEvidenceV2 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub ratio: CanonicalNullableV1<f64>,
    pub eligible_buyer_count: u32,
    pub flipper_count: u32,
    pub wall_clock_window_ms: u32,
    pub max_slot_gap: u32,
    pub dump_ratio: f64,
    pub owners: Vec<FlipOwnerEvidenceV2>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlipRatioContractEvidenceV1 {
    pub legacy_envelope: CanonicalMetricEnvelopeV1,
    pub legacy_slot_gap_ratio: CanonicalNullableV1<f64>,
    pub hybrid_v2: FlipRatioEvidenceV2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FundingSourceLegacyMeasurementV1 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub ratio: CanonicalNullableV1<f64>,
    pub distinct_known_source_count: u32,
    pub known_source_sample_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FundingSourceContractEvidenceV1 {
    pub legacy_source: FundingSourceLegacyMeasurementV1,
    pub legacy_v1: FundingSourceLegacyMeasurementV1,
    pub v2_envelope: CanonicalMetricEnvelopeV1,
    pub v2_status: FscEvidenceStatus,
    pub known_coverage: CanonicalNullableV1<f64>,
    pub non_neutral_known_coverage: CanonicalNullableV1<f64>,
    pub known_buyer_count: u32,
    pub known_non_neutral_buyer_count: u32,
    pub total_buyer_count: u32,
    pub provider: CanonicalNullableV1<String>,
    pub config_hash: CanonicalNullableV1<CanonicalHashV1>,
    pub coordination_hhi_export_envelope: CanonicalMetricEnvelopeV1,
    pub coordination_hhi: CanonicalNullableV1<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FscStatusEvidenceV1 {
    pub envelope: CanonicalMetricEnvelopeV1,
    pub legacy_scalar_present: bool,
    pub legacy_feature_status: EvidenceStatus,
    pub fsc_v2_status: CanonicalNullableV1<FscEvidenceStatus>,
    pub fsc_v2_coverage: CanonicalNullableV1<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ManipulationNumericFieldIdV2 {
    SameMsTxRatio,
    BundleSuspicionRatio,
    Top3SignerVolumeRatio,
    Hhi,
    MaxTxPerSigner,
    DevVolumeRatio,
    ContradictionScore,
}

impl ManipulationNumericFieldIdV2 {
    #[must_use]
    pub const fn measured_mask_bit(self) -> u16 {
        1 << (self as u16)
    }

    #[must_use]
    pub const fn has_derived_high_flag(self) -> bool {
        !matches!(self, Self::ContradictionScore)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManipulationNumericFieldEvidenceV2 {
    pub field_id: ManipulationNumericFieldIdV2,
    pub value: CanonicalNullableV1<f64>,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reason_codes: Vec<MetricEvidenceReasonV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManipulationDerivedFlagEvidenceV2 {
    pub field_id: ManipulationNumericFieldIdV2,
    pub raw_value: CanonicalNullableV1<f64>,
    pub raw_availability: MetricAvailabilityV1,
    pub raw_measurement_quality: MetricMeasurementQualityV1,
    pub derived_value: CanonicalNullableV1<bool>,
    pub comparator: ManipulationComparatorV1,
    pub threshold: CanonicalNullableV1<f64>,
    pub policy_stage: String,
    pub policy_version: String,
    pub config_hash: CanonicalHashV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManipulationComparatorV1 {
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManipulationLegacyHighFlagEvidenceV1 {
    pub field_id: ManipulationNumericFieldIdV2,
    pub value: bool,
    pub field_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManipulationNumericEvidenceV2 {
    pub legacy_numeric_envelope: CanonicalMetricEnvelopeV1,
    pub numeric_v2_envelope: CanonicalMetricEnvelopeV1,
    pub measured_fields_mask: u16,
    pub legacy_fields: Vec<ManipulationNumericFieldEvidenceV2>,
    pub fields: Vec<ManipulationNumericFieldEvidenceV2>,
    pub legacy_high_flags_envelope: CanonicalMetricEnvelopeV1,
    pub legacy_high_flags: Vec<ManipulationLegacyHighFlagEvidenceV1>,
    pub derived_high_flags_envelope: CanonicalMetricEnvelopeV1,
    pub derived_high_flags: Vec<ManipulationDerivedFlagEvidenceV2>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReserveVelocityStatusV1 {
    FirstUpdate,
    Measured,
    ZeroDeltaTime,
    BootstrapFallback,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReserveVelocitySourceClockV1 {
    ReceiveTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveVelocityEvidenceV1 {
    pub legacy_envelope: CanonicalMetricEnvelopeV1,
    pub legacy_velocity_sol_per_sec: f64,
    pub v1_envelope: CanonicalMetricEnvelopeV1,
    pub velocity_sol_per_sec: CanonicalNullableV1<f64>,
    pub previous_real_sol_reserves_lamports: CanonicalNullableV1<CanonicalU64StringV1>,
    pub current_real_sol_reserves_lamports: CanonicalNullableV1<CanonicalU64StringV1>,
    pub interval_ms: CanonicalNullableV1<u32>,
    pub accepted_update_count: u32,
    pub source_clock: ReserveVelocitySourceClockV1,
    pub status: ReserveVelocityStatusV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentBuySellEvidenceV1 {
    pub legacy_envelope: CanonicalMetricEnvelopeV1,
    pub v1_envelope: CanonicalMetricEnvelopeV1,
    pub window_ms: u32,
    pub buy_count: u32,
    pub sell_count: u32,
    pub transaction_count: u32,
    pub legacy_buy_sell_scalar: CanonicalNullableV1<f64>,
    pub buy_to_sell_ratio: CanonicalNullableV1<f64>,
    pub buy_share: CanonicalNullableV1<f64>,
}

/// Full typed evidence set. Every family is a required key; unavailable values
/// live inside their envelope/required-nullable fields rather than disappearing
/// from the semantic payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractsEvidenceSetV1 {
    pub fee_topology_diversity_index: FtdiEvidenceV1,
    pub dev_buy: DevBuyContractEvidenceV1,
    pub same_ms_tx_ratio: TxTimingEvidenceV1,
    pub top3_signer_volume_ratio: Top3SignerVolumeEvidenceV1,
    pub flip_ratio: FlipRatioContractEvidenceV1,
    pub funding_source_concentration: FundingSourceContractEvidenceV1,
    pub fsc_evidence_status: FscStatusEvidenceV1,
    pub manipulation_contradiction: ManipulationNumericEvidenceV2,
    pub reserve_velocity: ReserveVelocityEvidenceV1,
    pub recent_buy_sell: RecentBuySellEvidenceV1,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MetricContractEvidenceSemanticErrorV1 {
    #[error("evidence field {0} must be finite")]
    NonFinite(&'static str),
    #[error("evidence ratio {0} must be within [0, 1]")]
    RatioOutOfRange(&'static str),
    #[error("evidence count invariant failed for {0}")]
    CountInvariant(&'static str),
    #[error("derived ratio does not match counts for {0}")]
    DerivedRatioMismatch(&'static str),
    #[error("timing source does not match evidence field {0}")]
    TimingSourceMismatch(&'static str),
    #[error("top3 preferred/alias/effective selector invariant failed")]
    Top3SelectorInvariant,
    #[error("duplicate manipulation field {0:?}")]
    DuplicateManipulationField(ManipulationNumericFieldIdV2),
    #[error("missing manipulation field {0:?}")]
    MissingManipulationField(ManipulationNumericFieldIdV2),
    #[error("manipulation field {0:?} has inconsistent value/availability/quality")]
    ManipulationFieldStatus(ManipulationNumericFieldIdV2),
    #[error("manipulation measured_fields_mask does not match present measured fields")]
    ManipulationMeasuredMaskMismatch,
    #[error("reserve velocity status/value/count/interval invariant failed")]
    ReserveVelocityInvariant,
    #[error("recent buy/sell count or denominator invariant failed")]
    RecentBuySellInvariant,
    #[error("flip owner state or aggregate invariant failed")]
    FlipOwnerInvariant,
    #[error("legacy FSC scalar/status cross-check invariant failed")]
    FscStatusInvariant,
    #[error("FSC v2 full-evidence invariant failed for {0}")]
    FscV2Invariant(&'static str),
    #[error("dev-buy evidence invariant failed for {0}")]
    DevBuyInvariant(&'static str),
    #[error("derived manipulation flag invariant failed for {0:?}")]
    ManipulationDerivedFlagInvariant(ManipulationNumericFieldIdV2),
}

fn finite_value(
    field: &'static str,
    value: f64,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if !value.is_finite() {
        return Err(MetricContractEvidenceSemanticErrorV1::NonFinite(field));
    }
    Ok(())
}

fn bounded_ratio(
    field: &'static str,
    value: &CanonicalNullableV1<f64>,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if let CanonicalNullableV1::Value(value) = value {
        finite_value(field, *value)?;
        if !(0.0..=1.0).contains(value) {
            return Err(MetricContractEvidenceSemanticErrorV1::RatioOutOfRange(
                field,
            ));
        }
    }
    Ok(())
}

fn count_ratio(
    field: &'static str,
    numerator: u32,
    denominator: u32,
    value: &CanonicalNullableV1<f64>,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if numerator > denominator {
        return Err(MetricContractEvidenceSemanticErrorV1::CountInvariant(field));
    }
    match (denominator, value) {
        (0, CanonicalNullableV1::Null) => Ok(()),
        (0, CanonicalNullableV1::Value(_)) | (_, CanonicalNullableV1::Null) => Err(
            MetricContractEvidenceSemanticErrorV1::DerivedRatioMismatch(field),
        ),
        (_, CanonicalNullableV1::Value(value)) => {
            bounded_ratio(field, &CanonicalNullableV1::Value(*value))?;
            let expected = f64::from(numerator) / f64::from(denominator);
            if value.to_bits() != expected.to_bits() {
                return Err(MetricContractEvidenceSemanticErrorV1::DerivedRatioMismatch(
                    field,
                ));
            }
            Ok(())
        }
    }
}

fn validate_ftdi_measurement(
    field: &'static str,
    evidence: &FtdiValueMeasurementV1,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if evidence.unique_buyer_sample_count > evidence.buy_transaction_sample_count {
        return Err(MetricContractEvidenceSemanticErrorV1::CountInvariant(field));
    }
    if evidence.unique_buyer_sample_count < 2 || evidence.unique_topology_count == 0 {
        return if evidence.unique_topology_count == 0 && evidence.value.is_null() {
            Ok(())
        } else {
            Err(MetricContractEvidenceSemanticErrorV1::DerivedRatioMismatch(
                field,
            ))
        };
    }
    count_ratio(
        field,
        evidence.unique_topology_count,
        evidence.unique_buyer_sample_count,
        &evidence.value,
    )
}

fn validate_dev_buy(
    field: &'static str,
    evidence: &DevBuyEvidenceV1,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if let CanonicalNullableV1::Value(amount_sol) = evidence.amount_sol {
        finite_value(field, amount_sol)?;
        if amount_sol < 0.0 {
            return Err(MetricContractEvidenceSemanticErrorV1::DevBuyInvariant(
                field,
            ));
        }
    }
    for value in [&evidence.create_signature, &evidence.selected_signature] {
        if matches!(value, CanonicalNullableV1::Value(value) if value.trim().is_empty()) {
            return Err(MetricContractEvidenceSemanticErrorV1::DevBuyInvariant(
                field,
            ));
        }
    }

    let selection_present = matches!(evidence.amount_sol, CanonicalNullableV1::Value(_))
        && matches!(evidence.selected_signature, CanonicalNullableV1::Value(_))
        && matches!(evidence.selected_slot, CanonicalNullableV1::Value(_))
        && evidence.eligible_buy_count > 0;
    let selection_absent = matches!(evidence.amount_sol, CanonicalNullableV1::Null)
        && matches!(evidence.selected_signature, CanonicalNullableV1::Null)
        && matches!(evidence.selected_slot, CanonicalNullableV1::Null)
        && matches!(
            evidence.selected_transaction_index,
            CanonicalNullableV1::Null
        )
        && evidence.eligible_buy_count == 0
        && !evidence.create_signature_matched;

    let valid = match evidence.selection_mode {
        DevBuySelectionModeV1::NoEligibleBuy => selection_absent,
        DevBuySelectionModeV1::CreateSignatureMatch => {
            selection_present
                && evidence.creator_known
                && evidence.create_signature_matched
                && evidence.create_signature == evidence.selected_signature
        }
        DevBuySelectionModeV1::LegacyFirstObserved
        | DevBuySelectionModeV1::EarliestEligibleCreatorBuy => {
            selection_present && evidence.creator_known
        }
    };
    if !valid {
        return Err(MetricContractEvidenceSemanticErrorV1::DevBuyInvariant(
            field,
        ));
    }
    Ok(())
}

fn validate_timing_measurement(
    field: &'static str,
    evidence: &TxTimingMeasurementEvidenceV1,
    expected_source: TxTimingSourceV1,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if evidence.source != expected_source {
        return Err(MetricContractEvidenceSemanticErrorV1::TimingSourceMismatch(
            field,
        ));
    }
    count_ratio(
        field,
        evidence.numerator,
        evidence.denominator,
        &evidence.ratio,
    )
}

fn validate_fsc_legacy_measurement(
    field: &'static str,
    evidence: &FundingSourceLegacyMeasurementV1,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if evidence.distinct_known_source_count > evidence.known_source_sample_count {
        return Err(MetricContractEvidenceSemanticErrorV1::CountInvariant(field));
    }
    // The active legacy producer returns `None` until at least two known
    // sources exist. A single known source is therefore insufficient, not a
    // measured 0.0 concentration.
    let expected = if evidence.known_source_sample_count < 2 {
        CanonicalNullableV1::Null
    } else {
        CanonicalNullableV1::Value(
            1.0 - f64::from(evidence.distinct_known_source_count)
                / f64::from(evidence.known_source_sample_count),
        )
    };
    bounded_ratio(field, &evidence.ratio)?;
    if evidence.ratio != expected {
        return Err(MetricContractEvidenceSemanticErrorV1::DerivedRatioMismatch(
            field,
        ));
    }
    Ok(())
}

fn validate_fsc_v2_evidence(
    evidence: &FundingSourceContractEvidenceV1,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if evidence.known_non_neutral_buyer_count > evidence.known_buyer_count {
        return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
            "known non-neutral buyers exceed known buyers",
        ));
    }
    if evidence.known_buyer_count > evidence.total_buyer_count {
        return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
            "known buyers exceed total buyers",
        ));
    }

    match evidence.v2_status {
        FscEvidenceStatus::Clean | FscEvidenceStatus::Degraded => {
            if evidence.total_buyer_count == 0 {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "available status requires a non-empty buyer cohort",
                ));
            }
            let CanonicalNullableV1::Value(known_coverage) = &evidence.known_coverage else {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "available status requires known coverage",
                ));
            };
            let CanonicalNullableV1::Value(non_neutral_known_coverage) =
                &evidence.non_neutral_known_coverage
            else {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "available status requires non-neutral known coverage",
                ));
            };
            let expected_known =
                f64::from(evidence.known_buyer_count) / f64::from(evidence.total_buyer_count);
            if known_coverage.to_bits() != expected_known.to_bits() {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "known coverage does not match buyer counts",
                ));
            }
            let expected_non_neutral = f64::from(evidence.known_non_neutral_buyer_count)
                / f64::from(evidence.total_buyer_count);
            if non_neutral_known_coverage.to_bits() != expected_non_neutral.to_bits() {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "non-neutral known coverage does not match buyer counts",
                ));
            }
        }
        FscEvidenceStatus::Unavailable => {
            if !matches!(&evidence.known_coverage, CanonicalNullableV1::Null)
                || !matches!(
                    &evidence.non_neutral_known_coverage,
                    CanonicalNullableV1::Null
                )
                || evidence.known_buyer_count != 0
                || evidence.known_non_neutral_buyer_count != 0
            {
                return Err(MetricContractEvidenceSemanticErrorV1::FscV2Invariant(
                    "unavailable status cannot expose known counts or coverage",
                ));
            }
        }
    }
    Ok(())
}

fn validate_flip_owner(
    owner: &FlipOwnerEvidenceV2,
    wall_clock_window_ms: u32,
    max_slot_gap: u32,
    dump_ratio: f64,
) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
    if owner.owner_id.trim().is_empty() {
        return Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant);
    }

    let anchor_complete = matches!(owner.anchor_event_identity, CanonicalNullableV1::Value(_))
        && matches!(owner.anchor_slot, CanonicalNullableV1::Value(_))
        && matches!(owner.anchor_timestamp_ms, CanonicalNullableV1::Value(_));
    let anchor_absent = matches!(owner.anchor_event_identity, CanonicalNullableV1::Null)
        && matches!(owner.anchor_slot, CanonicalNullableV1::Null)
        && matches!(owner.anchor_timestamp_ms, CanonicalNullableV1::Null);
    let qualifying_complete =
        matches!(
            owner.qualifying_sell_event_identity,
            CanonicalNullableV1::Value(_)
        ) && matches!(owner.qualifying_sell_slot, CanonicalNullableV1::Value(_))
            && matches!(
                owner.qualifying_sell_timestamp_ms,
                CanonicalNullableV1::Value(_)
            );
    let qualifying_absent = matches!(
        owner.qualifying_sell_event_identity,
        CanonicalNullableV1::Null
    ) && matches!(owner.qualifying_sell_slot, CanonicalNullableV1::Null)
        && matches!(
            owner.qualifying_sell_timestamp_ms,
            CanonicalNullableV1::Null
        );

    let valid = match owner.status {
        FlipOwnerStatusV2::NoAnchor => {
            anchor_absent
                && qualifying_absent
                && owner.cumulative_eligible_buy_tokens.get() == 0
                && owner.cumulative_eligible_sell_tokens.get() == 0
        }
        FlipOwnerStatusV2::Tracking | FlipOwnerStatusV2::ClosedNonFlipper => {
            anchor_complete && qualifying_absent && owner.cumulative_eligible_buy_tokens.get() > 0
        }
        FlipOwnerStatusV2::Flipper => {
            let order_and_window_valid = match (
                &owner.anchor_slot,
                &owner.anchor_timestamp_ms,
                &owner.qualifying_sell_slot,
                &owner.qualifying_sell_timestamp_ms,
            ) {
                (
                    CanonicalNullableV1::Value(anchor_slot),
                    CanonicalNullableV1::Value(anchor_timestamp_ms),
                    CanonicalNullableV1::Value(sell_slot),
                    CanonicalNullableV1::Value(sell_timestamp_ms),
                ) => {
                    sell_slot
                        .get()
                        .checked_sub(anchor_slot.get())
                        .is_some_and(|gap| gap <= u64::from(max_slot_gap))
                        && sell_timestamp_ms
                            .get()
                            .checked_sub(anchor_timestamp_ms.get())
                            .is_some_and(|gap| gap <= u64::from(wall_clock_window_ms))
                }
                _ => false,
            };
            anchor_complete
                && qualifying_complete
                && owner.cumulative_eligible_buy_tokens.get() > 0
                && (owner.cumulative_eligible_sell_tokens.get() as f64)
                    >= (owner.cumulative_eligible_buy_tokens.get() as f64 * dump_ratio)
                && order_and_window_valid
        }
    };
    if !valid {
        return Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant);
    }
    Ok(())
}

const MANIPULATION_NUMERIC_FIELDS_V2: [ManipulationNumericFieldIdV2; 7] = [
    ManipulationNumericFieldIdV2::SameMsTxRatio,
    ManipulationNumericFieldIdV2::BundleSuspicionRatio,
    ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
    ManipulationNumericFieldIdV2::Hhi,
    ManipulationNumericFieldIdV2::MaxTxPerSigner,
    ManipulationNumericFieldIdV2::DevVolumeRatio,
    ManipulationNumericFieldIdV2::ContradictionScore,
];

pub const MANIPULATION_DERIVED_POLICY_STAGE_V1: &str = "gatekeeper_v3_shadow_evidence";
pub const MANIPULATION_DERIVED_POLICY_VERSION_V1: &str = "v1";

fn nullable_f64_bits_equal(
    left: &CanonicalNullableV1<f64>,
    right: &CanonicalNullableV1<f64>,
) -> bool {
    match (left, right) {
        (CanonicalNullableV1::Null, CanonicalNullableV1::Null) => true,
        (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
            left.to_bits() == right.to_bits()
        }
        _ => false,
    }
}

fn validate_manipulation_field_set(
    fields: &[ManipulationNumericFieldEvidenceV2],
) -> Result<u16, MetricContractEvidenceSemanticErrorV1> {
    let mut seen = BTreeSet::new();
    let mut measured_mask = 0_u16;
    for field in fields {
        if !seen.insert(field.field_id) {
            return Err(
                MetricContractEvidenceSemanticErrorV1::DuplicateManipulationField(field.field_id),
            );
        }
        match (field.availability, field.measurement_quality, &field.value) {
            (MetricAvailabilityV1::Available, quality, CanonicalNullableV1::Value(value))
                if quality != MetricMeasurementQualityV1::NotApplicable =>
            {
                finite_value("manipulation_numeric", *value)?;
                if field.field_id == ManipulationNumericFieldIdV2::MaxTxPerSigner {
                    if *value < 0.0 {
                        return Err(MetricContractEvidenceSemanticErrorV1::RatioOutOfRange(
                            "max_tx_per_signer",
                        ));
                    }
                } else if !(0.0..=1.0).contains(value) {
                    return Err(MetricContractEvidenceSemanticErrorV1::RatioOutOfRange(
                        "manipulation_numeric",
                    ));
                }
                if matches!(
                    quality,
                    MetricMeasurementQualityV1::Measured | MetricMeasurementQualityV1::Degraded
                ) {
                    measured_mask |= field.field_id.measured_mask_bit();
                }
            }
            (
                availability,
                MetricMeasurementQualityV1::NotApplicable,
                CanonicalNullableV1::Null,
            ) if availability != MetricAvailabilityV1::Available => {}
            _ => {
                return Err(
                    MetricContractEvidenceSemanticErrorV1::ManipulationFieldStatus(field.field_id),
                )
            }
        }
    }
    for expected in MANIPULATION_NUMERIC_FIELDS_V2 {
        if !seen.contains(&expected) {
            return Err(MetricContractEvidenceSemanticErrorV1::MissingManipulationField(expected));
        }
    }
    Ok(measured_mask)
}

impl MetricContractsEvidenceSetV1 {
    /// Validate mathematical and presence invariants that do not depend on
    /// producer thresholds. Threshold/readiness checks remain producer-specific
    /// and are bound through the effective-config hash.
    pub fn validate_semantics(&self) -> Result<(), MetricContractEvidenceSemanticErrorV1> {
        validate_ftdi_measurement(
            "ftdi_legacy_value",
            &self.fee_topology_diversity_index.legacy_value,
        )?;
        validate_ftdi_measurement("ftdi_value_v1", &self.fee_topology_diversity_index.value_v1)?;

        validate_dev_buy(
            "tx_intel_dev_first_observed",
            &self.dev_buy.tx_intel_first_observed,
        )?;
        validate_dev_buy(
            "gatekeeper_buffer_dev_primary",
            &self.dev_buy.gatekeeper_buffer_primary,
        )?;
        validate_dev_buy("mfs_dev_first_observed", &self.dev_buy.mfs_first_observed)?;
        validate_dev_buy("mfs_dev_primary_v1", &self.dev_buy.mfs_primary_v1)?;
        validate_dev_buy("effective_policy_dev_buy", &self.dev_buy.effective_policy)?;

        validate_timing_measurement(
            "same_ms_legacy_exact",
            &self.same_ms_tx_ratio.legacy_exact,
            TxTimingSourceV1::TxIntelFullObservationExactLegacy,
        )?;
        validate_timing_measurement(
            "same_ms_exact_v1",
            &self.same_ms_tx_ratio.exact_v1,
            TxTimingSourceV1::TxTimingFullObservationExactV1,
        )?;
        validate_timing_measurement(
            "same_ms_cluster_lt_50ms",
            &self.same_ms_tx_ratio.cluster_lt_50ms,
            TxTimingSourceV1::PhaseDiversityClusterLt50Ms,
        )?;
        validate_timing_measurement(
            "same_ms_recent_exact",
            &self.same_ms_tx_ratio.recent_exact,
            TxTimingSourceV1::RceRecentExact,
        )?;

        let top3 = &self.top3_signer_volume_ratio;
        bounded_ratio("top3_preferred", &top3.preferred_ratio)?;
        bounded_ratio("top3_compatibility_alias", &top3.compatibility_alias_ratio)?;
        bounded_ratio("top3_effective", &top3.effective_ratio)?;
        let expected_bitwise_equal = match (&top3.preferred_ratio, &top3.compatibility_alias_ratio)
        {
            (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
                CanonicalNullableV1::Value(left.to_bits() == right.to_bits())
            }
            _ => CanonicalNullableV1::Null,
        };
        let selector_valid = match &top3.preferred_ratio {
            CanonicalNullableV1::Value(_) => {
                !top3.used_compatibility_fallback
                    && nullable_f64_bits_equal(&top3.effective_ratio, &top3.preferred_ratio)
            }
            CanonicalNullableV1::Null => match &top3.compatibility_alias_ratio {
                CanonicalNullableV1::Value(_) => {
                    top3.used_compatibility_fallback
                        && nullable_f64_bits_equal(
                            &top3.effective_ratio,
                            &top3.compatibility_alias_ratio,
                        )
                }
                CanonicalNullableV1::Null => {
                    !top3.used_compatibility_fallback
                        && matches!(top3.effective_ratio, CanonicalNullableV1::Null)
                }
            },
        };
        if top3.preferred_alias_bitwise_equal != expected_bitwise_equal || !selector_valid {
            return Err(MetricContractEvidenceSemanticErrorV1::Top3SelectorInvariant);
        }

        bounded_ratio(
            "flip_legacy_slot_gap",
            &self.flip_ratio.legacy_slot_gap_ratio,
        )?;
        let flip = &self.flip_ratio.hybrid_v2;
        bounded_ratio("flip_hybrid_v2", &flip.ratio)?;
        bounded_ratio(
            "flip_hybrid_v2_dump_ratio",
            &CanonicalNullableV1::Value(flip.dump_ratio),
        )?;
        if flip.wall_clock_window_ms == 0 || flip.dump_ratio <= 0.0 {
            return Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant);
        }
        count_ratio(
            "flip_hybrid_v2",
            flip.flipper_count,
            flip.eligible_buyer_count,
            &flip.ratio,
        )?;
        let mut owners = BTreeSet::new();
        let mut anchored_owner_count = 0_u32;
        let mut flipper_owner_count = 0_u32;
        for owner in &flip.owners {
            if !owners.insert(owner.owner_id.as_str()) {
                return Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant);
            }
            validate_flip_owner(
                owner,
                flip.wall_clock_window_ms,
                flip.max_slot_gap,
                flip.dump_ratio,
            )?;
            if owner.status != FlipOwnerStatusV2::NoAnchor {
                anchored_owner_count = anchored_owner_count
                    .checked_add(1)
                    .ok_or(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant)?;
            }
            if owner.status == FlipOwnerStatusV2::Flipper {
                flipper_owner_count = flipper_owner_count
                    .checked_add(1)
                    .ok_or(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant)?;
            }
        }
        if anchored_owner_count != flip.eligible_buyer_count
            || flipper_owner_count != flip.flipper_count
        {
            return Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant);
        }

        let funding = &self.funding_source_concentration;
        validate_fsc_legacy_measurement("fsc_legacy_source", &funding.legacy_source)?;
        validate_fsc_legacy_measurement("fsc_legacy_v1", &funding.legacy_v1)?;
        validate_fsc_v2_evidence(funding)?;
        bounded_ratio("fsc_known_coverage", &funding.known_coverage)?;
        bounded_ratio(
            "fsc_non_neutral_known_coverage",
            &funding.non_neutral_known_coverage,
        )?;
        bounded_ratio("coordination_fsc_hhi", &funding.coordination_hhi)?;
        let fsc_status = &self.fsc_evidence_status;
        let legacy_scalar_present =
            matches!(funding.legacy_source.ratio, CanonicalNullableV1::Value(_));
        if fsc_status.legacy_scalar_present != legacy_scalar_present
            || fsc_status.fsc_v2_status != CanonicalNullableV1::Value(funding.v2_status)
            || !nullable_f64_bits_equal(&fsc_status.fsc_v2_coverage, &funding.known_coverage)
        {
            return Err(MetricContractEvidenceSemanticErrorV1::FscStatusInvariant);
        }

        let manipulation = &self.manipulation_contradiction;
        validate_manipulation_field_set(&manipulation.legacy_fields)?;
        let measured_mask = validate_manipulation_field_set(&manipulation.fields)?;
        match (
            manipulation.numeric_v2_envelope.availability,
            manipulation.numeric_v2_envelope.measurement_quality,
        ) {
            (MetricAvailabilityV1::Available, MetricMeasurementQualityV1::Measured)
                if manipulation.fields.iter().all(|field| {
                    !field.value.is_null()
                        && field.availability == MetricAvailabilityV1::Available
                        && field.measurement_quality == MetricMeasurementQualityV1::Measured
                }) => {}
            (MetricAvailabilityV1::Available, MetricMeasurementQualityV1::Degraded)
                if manipulation.fields.iter().all(|field| {
                    field.value.is_null()
                        || field.measurement_quality == MetricMeasurementQualityV1::Degraded
                }) => {}
            (availability, MetricMeasurementQualityV1::NotApplicable)
                if availability != MetricAvailabilityV1::Available
                    && manipulation
                        .fields
                        .iter()
                        .all(|field| field.value.is_null()) => {}
            _ => {
                return Err(MetricContractEvidenceSemanticErrorV1::ManipulationMeasuredMaskMismatch)
            }
        }
        if manipulation.measured_fields_mask != measured_mask {
            return Err(MetricContractEvidenceSemanticErrorV1::ManipulationMeasuredMaskMismatch);
        }
        let expected_high_fields = MANIPULATION_NUMERIC_FIELDS_V2
            .into_iter()
            .filter(|field| field.has_derived_high_flag())
            .collect::<BTreeSet<_>>();
        let mut legacy_high_fields = BTreeSet::new();
        for flag in &manipulation.legacy_high_flags {
            if !legacy_high_fields.insert(flag.field_id) {
                return Err(
                    MetricContractEvidenceSemanticErrorV1::DuplicateManipulationField(
                        flag.field_id,
                    ),
                );
            }
        }
        let mut derived_high_fields = BTreeSet::new();
        for flag in &manipulation.derived_high_flags {
            if !derived_high_fields.insert(flag.field_id) {
                return Err(
                    MetricContractEvidenceSemanticErrorV1::DuplicateManipulationField(
                        flag.field_id,
                    ),
                );
            }
            let source_field = manipulation
                .fields
                .iter()
                .find(|field| field.field_id == flag.field_id)
                .ok_or(
                    MetricContractEvidenceSemanticErrorV1::MissingManipulationField(flag.field_id),
                )?;
            if !nullable_f64_bits_equal(&source_field.value, &flag.raw_value)
                || source_field.availability != flag.raw_availability
                || source_field.measurement_quality != flag.raw_measurement_quality
            {
                return Err(
                    MetricContractEvidenceSemanticErrorV1::ManipulationDerivedFlagInvariant(
                        flag.field_id,
                    ),
                );
            }
            match (&flag.raw_value, &flag.threshold, &flag.derived_value) {
                (
                    CanonicalNullableV1::Value(raw),
                    CanonicalNullableV1::Value(threshold),
                    CanonicalNullableV1::Value(derived),
                ) if flag.raw_availability == MetricAvailabilityV1::Available
                    && flag.raw_measurement_quality
                        != MetricMeasurementQualityV1::NotApplicable =>
                {
                    finite_value("manipulation_derived_raw", *raw)?;
                    finite_value("manipulation_derived_threshold", *threshold)?;
                    if *threshold < 0.0 {
                        return Err(
                            MetricContractEvidenceSemanticErrorV1::ManipulationDerivedFlagInvariant(
                                flag.field_id,
                            ),
                        );
                    }
                    let expected = match flag.comparator {
                        ManipulationComparatorV1::GreaterThan => raw > threshold,
                        ManipulationComparatorV1::GreaterThanOrEqual => raw >= threshold,
                    };
                    if *derived != expected {
                        return Err(
                            MetricContractEvidenceSemanticErrorV1::ManipulationDerivedFlagInvariant(
                                flag.field_id,
                            ),
                        );
                    }
                }
                (
                    CanonicalNullableV1::Value(raw),
                    CanonicalNullableV1::Null,
                    CanonicalNullableV1::Null,
                ) if flag.raw_availability == MetricAvailabilityV1::Available
                    && flag.raw_measurement_quality
                        != MetricMeasurementQualityV1::NotApplicable =>
                {
                    finite_value("manipulation_derived_raw", *raw)?;
                }
                (CanonicalNullableV1::Null, _, CanonicalNullableV1::Null)
                    if flag.raw_availability != MetricAvailabilityV1::Available
                        && flag.raw_measurement_quality
                            == MetricMeasurementQualityV1::NotApplicable => {}
                _ => {
                    return Err(
                        MetricContractEvidenceSemanticErrorV1::ManipulationDerivedFlagInvariant(
                            flag.field_id,
                        ),
                    )
                }
            }
            if flag.policy_stage != MANIPULATION_DERIVED_POLICY_STAGE_V1
                || flag.policy_version != MANIPULATION_DERIVED_POLICY_VERSION_V1
            {
                return Err(
                    MetricContractEvidenceSemanticErrorV1::ManipulationFieldStatus(flag.field_id),
                );
            }
        }
        if legacy_high_fields != expected_high_fields || derived_high_fields != expected_high_fields
        {
            let missing = expected_high_fields
                .difference(&derived_high_fields)
                .next()
                .or_else(|| expected_high_fields.difference(&legacy_high_fields).next())
                .copied()
                .unwrap_or(ManipulationNumericFieldIdV2::ContradictionScore);
            return Err(MetricContractEvidenceSemanticErrorV1::MissingManipulationField(missing));
        }

        let reserve = &self.reserve_velocity;
        finite_value(
            "reserve_velocity_legacy",
            reserve.legacy_velocity_sol_per_sec,
        )?;
        if let CanonicalNullableV1::Value(value) = reserve.velocity_sol_per_sec {
            finite_value("reserve_velocity_v1", value)?;
        }
        if reserve.source_clock != ReserveVelocitySourceClockV1::ReceiveTime {
            return Err(MetricContractEvidenceSemanticErrorV1::ReserveVelocityInvariant);
        }
        let reserve_valid = match reserve.status {
            ReserveVelocityStatusV1::Measured => {
                match (
                    &reserve.previous_real_sol_reserves_lamports,
                    &reserve.current_real_sol_reserves_lamports,
                    &reserve.interval_ms,
                    &reserve.velocity_sol_per_sec,
                ) {
                    (
                        CanonicalNullableV1::Value(previous),
                        CanonicalNullableV1::Value(current),
                        CanonicalNullableV1::Value(interval_ms),
                        CanonicalNullableV1::Value(velocity),
                    ) if *interval_ms > 0 && reserve.accepted_update_count >= 2 => {
                        let delta_sol =
                            (current.get() as f64 - previous.get() as f64) / 1_000_000_000.0;
                        let expected = delta_sol / (f64::from(*interval_ms) / 1_000.0);
                        velocity.to_bits() == expected.to_bits()
                            && reserve.legacy_velocity_sol_per_sec.to_bits() == velocity.to_bits()
                    }
                    _ => false,
                }
            }
            ReserveVelocityStatusV1::FirstUpdate => {
                reserve.accepted_update_count == 1
                    && matches!(
                        reserve.previous_real_sol_reserves_lamports,
                        CanonicalNullableV1::Null
                    )
                    && matches!(
                        reserve.current_real_sol_reserves_lamports,
                        CanonicalNullableV1::Value(_)
                    )
                    && matches!(reserve.interval_ms, CanonicalNullableV1::Null)
                    && matches!(reserve.velocity_sol_per_sec, CanonicalNullableV1::Null)
            }
            ReserveVelocityStatusV1::BootstrapFallback => {
                reserve.accepted_update_count == 0
                    && matches!(reserve.interval_ms, CanonicalNullableV1::Null)
                    && matches!(reserve.velocity_sol_per_sec, CanonicalNullableV1::Null)
            }
            ReserveVelocityStatusV1::Unavailable => {
                matches!(reserve.velocity_sol_per_sec, CanonicalNullableV1::Null)
            }
            ReserveVelocityStatusV1::ZeroDeltaTime => {
                reserve.accepted_update_count >= 2
                    && matches!(
                        reserve.previous_real_sol_reserves_lamports,
                        CanonicalNullableV1::Value(_)
                    )
                    && matches!(
                        reserve.current_real_sol_reserves_lamports,
                        CanonicalNullableV1::Value(_)
                    )
                    && matches!(reserve.interval_ms, CanonicalNullableV1::Value(0))
                    && matches!(reserve.velocity_sol_per_sec, CanonicalNullableV1::Null)
            }
        };
        if !reserve_valid {
            return Err(MetricContractEvidenceSemanticErrorV1::ReserveVelocityInvariant);
        }

        let recent = &self.recent_buy_sell;
        if recent.buy_count.checked_add(recent.sell_count) != Some(recent.transaction_count) {
            return Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant);
        }
        if let CanonicalNullableV1::Value(value) = recent.legacy_buy_sell_scalar {
            finite_value("recent_legacy_buy_sell", value)?;
            if value < 0.0 {
                return Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant);
            }
        }
        let expected_legacy_scalar = if recent.transaction_count == 0 {
            CanonicalNullableV1::Null
        } else if recent.sell_count == 0 {
            CanonicalNullableV1::Value(f64::from(recent.buy_count))
        } else {
            CanonicalNullableV1::Value(f64::from(recent.buy_count) / f64::from(recent.sell_count))
        };
        if !nullable_f64_bits_equal(&recent.legacy_buy_sell_scalar, &expected_legacy_scalar) {
            return Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant);
        }
        match (recent.sell_count, &recent.buy_to_sell_ratio) {
            (0, CanonicalNullableV1::Null) => {}
            (0, CanonicalNullableV1::Value(_)) | (_, CanonicalNullableV1::Null) => {
                return Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant)
            }
            (sell_count, CanonicalNullableV1::Value(value)) => {
                finite_value("recent_buy_to_sell_ratio", *value)?;
                let expected = f64::from(recent.buy_count) / f64::from(sell_count);
                if value.to_bits() != expected.to_bits() {
                    return Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant);
                }
            }
        }
        count_ratio(
            "recent_buy_share",
            recent.buy_count,
            recent.transaction_count,
            &recent.buy_share,
        )?;

        Ok(())
    }
}

fn validate_expected_surface(
    envelope: &CanonicalMetricEnvelopeV1,
    expected: MetricSurfaceId,
    profile: &MetricContractProfileV1,
    mode: MetricContractRolloutMode,
) -> Result<(), MetricEvidenceEnvelopeErrorV1> {
    if envelope.surface_id != expected {
        return Err(
            MetricEvidenceEnvelopeErrorV1::UnexpectedSurfaceForEvidenceField {
                expected,
                actual: envelope.surface_id,
            },
        );
    }
    envelope.validate_for_profile(profile, mode)
}

impl MetricContractsEvidenceSetV1 {
    /// Validate all 32 surface slots against the selected immutable authority
    /// profile. This prevents a valid envelope from being serialized under the
    /// wrong contract field and prevents one status from standing in for
    /// legacy, candidate, compatibility, or export-only surfaces.
    pub fn validate_for_profile(
        &self,
        profile: &MetricContractProfileV1,
        mode: MetricContractRolloutMode,
    ) -> Result<(), MetricEvidenceEnvelopeErrorV1> {
        use MetricSurfaceId as Surface;

        for (envelope, expected) in [
            (
                &self.fee_topology_diversity_index.legacy_value.envelope,
                Surface::TxIntelFeeTopologyDiversityLegacy,
            ),
            (
                &self.fee_topology_diversity_index.value_v1.envelope,
                Surface::FtdiValueEvidenceV1,
            ),
            (
                &self
                    .fee_topology_diversity_index
                    .legacy_actionability_envelope,
                Surface::FtdiLegacyBuyTxActionability,
            ),
            (
                &self
                    .fee_topology_diversity_index
                    .unique_buyer_actionability_v2_envelope,
                Surface::FtdiUniqueBuyerActionabilityV2,
            ),
            (
                &self
                    .fee_topology_diversity_index
                    .coordination_hhi_export_envelope,
                Surface::CoordinationFeeTopologyHhiExportV1,
            ),
            (
                &self.dev_buy.tx_intel_first_observed.envelope,
                Surface::TxIntelDevFirstObservedBuySol,
            ),
            (
                &self.dev_buy.gatekeeper_buffer_primary.envelope,
                Surface::GatekeeperBufferDevPrimaryBuySol,
            ),
            (
                &self.dev_buy.mfs_first_observed.envelope,
                Surface::MfsDevFirstObservedBuySol,
            ),
            (
                &self.dev_buy.mfs_primary_v1.envelope,
                Surface::MfsDevPrimaryBuySolV1,
            ),
            (
                &self.dev_buy.effective_policy.envelope,
                Surface::EffectivePolicyDevBuySol,
            ),
            (
                &self.same_ms_tx_ratio.legacy_exact.envelope,
                Surface::TxIntelSameMsCollisionRatioExact,
            ),
            (
                &self.same_ms_tx_ratio.exact_v1.envelope,
                Surface::TxTimingExactSameMsEvidenceV1,
            ),
            (
                &self.same_ms_tx_ratio.cluster_lt_50ms.envelope,
                Surface::TxIntelBundleClusterRatioLt50Ms,
            ),
            (
                &self.same_ms_tx_ratio.recent_exact.envelope,
                Surface::RceSameMsCollisionRatioRecentExact,
            ),
            (
                &self.top3_signer_volume_ratio.preferred_envelope,
                Surface::TxIntelTop3SignerVolumeRatioPreferred,
            ),
            (
                &self.top3_signer_volume_ratio.compatibility_alias_envelope,
                Surface::TxIntelTop3VolumePctCompatibilityAlias,
            ),
            (
                &self.top3_signer_volume_ratio.effective_selector_envelope,
                Surface::TxIntelTop3EffectiveSelector,
            ),
            (
                &self.flip_ratio.legacy_envelope,
                Surface::EarlyFingerprintFlipRatioLegacySlotGap,
            ),
            (
                &self.flip_ratio.hybrid_v2.envelope,
                Surface::FlipRatioHybridEvidenceV2,
            ),
            (
                &self.funding_source_concentration.legacy_source.envelope,
                Surface::TxIntelFundingSourceConcentrationLegacy,
            ),
            (
                &self.funding_source_concentration.legacy_v1.envelope,
                Surface::FundingSourceConcentrationLegacyEvidenceV1,
            ),
            (
                &self.funding_source_concentration.v2_envelope,
                Surface::FundingSourceV2ReadinessEvidence,
            ),
            (
                &self
                    .funding_source_concentration
                    .coordination_hhi_export_envelope,
                Surface::CoordinationFundingSourceHhiExportV1,
            ),
            (
                &self.fsc_evidence_status.envelope,
                Surface::MaterializedFscStatusCompatibility,
            ),
            (
                &self.manipulation_contradiction.legacy_numeric_envelope,
                Surface::MfsManipulationNumericLegacyDefaults,
            ),
            (
                &self.manipulation_contradiction.numeric_v2_envelope,
                Surface::ManipulationNumericEvidenceV2,
            ),
            (
                &self.manipulation_contradiction.legacy_high_flags_envelope,
                Surface::MfsManipulationHighFlagsLegacyDefaults,
            ),
            (
                &self.manipulation_contradiction.derived_high_flags_envelope,
                Surface::PolicyDerivedManipulationHighFlagsV2,
            ),
            (
                &self.reserve_velocity.legacy_envelope,
                Surface::AccountStateReserveVelocityScalarLegacy,
            ),
            (
                &self.reserve_velocity.v1_envelope,
                Surface::ReserveVelocityEvidenceV1,
            ),
            (
                &self.recent_buy_sell.legacy_envelope,
                Surface::RceBuySellRatioRecentLegacy,
            ),
            (
                &self.recent_buy_sell.v1_envelope,
                Surface::RecentBuySellEvidenceV1,
            ),
        ] {
            validate_expected_surface(envelope, expected, profile, mode)?;
        }
        Ok(())
    }
}

/// Schema-defined semantic payload for the evidence SHA. It intentionally has
/// no evidence hash, writer timestamp, rotation part, or transport metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractEvidenceHashPayloadV1 {
    pub evidence_schema_version: u16,
    pub record_identity: MetricEvidenceRecordIdentityV1,
    pub stable_event_identity: CanonicalNullableV1<StableEventIdentityV1>,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile_id: MetricContractProfileIdV1,
    pub profile_hash: CanonicalHashV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
    pub contracts: MetricContractsEvidenceSetV1,
}

impl MetricContractEvidenceHashPayloadV1 {
    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, CanonicalHashErrorV1> {
        CanonicalHashV1::digest(self)
    }

    pub fn validate_profile_hash(&self) -> Result<(), MetricContractEvidenceTransportErrorV1> {
        if self.evidence_schema_version != METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1 {
            return Err(
                MetricContractEvidenceTransportErrorV1::UnsupportedEvidenceSchema(
                    self.evidence_schema_version,
                ),
            );
        }
        let profile = super::MetricContractFoundationConfigV1 {
            metric_contract_rollout_mode: self.rollout_mode,
            metric_contract_profile: self.profile_id,
        }
        .resolve_profile()?;
        if profile.canonical_hash()? != self.profile_hash {
            return Err(MetricContractEvidenceTransportErrorV1::ProfileHashMismatch);
        }
        self.contracts
            .validate_for_profile(&profile, self.rollout_mode)?;
        self.contracts.validate_semantics()?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum MetricContractEvidenceTransportErrorV1 {
    #[error(transparent)]
    Profile(#[from] super::MetricContractProfileErrorV1),
    #[error(transparent)]
    Hash(#[from] CanonicalHashErrorV1),
    #[error(transparent)]
    Envelope(#[from] MetricEvidenceEnvelopeErrorV1),
    #[error(transparent)]
    Semantic(#[from] MetricContractEvidenceSemanticErrorV1),
    #[error("unsupported metric contract evidence schema: {0}")]
    UnsupportedEvidenceSchema(u16),
    #[error("metric contract evidence profile hash does not match the selected compiled profile")]
    ProfileHashMismatch,
    #[error("metric contract evidence SHA-256 mismatch")]
    HashMismatch,
}

/// Transport wrapper is deliberately separate from the semantic hash payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractEvidenceTransportV1 {
    pub payload: MetricContractEvidenceHashPayloadV1,
    pub evidence_sha256: CanonicalHashV1,
    pub writer_timestamp_ms: u64,
    pub rotation_part_index: u32,
}

impl MetricContractEvidenceTransportV1 {
    pub fn try_new(
        payload: MetricContractEvidenceHashPayloadV1,
        writer_timestamp_ms: u64,
        rotation_part_index: u32,
    ) -> Result<Self, MetricContractEvidenceTransportErrorV1> {
        payload.validate_profile_hash()?;
        let evidence_sha256 = payload.canonical_hash()?;
        Ok(Self {
            payload,
            evidence_sha256,
            writer_timestamp_ms,
            rotation_part_index,
        })
    }

    pub fn validate_hash(&self) -> Result<(), MetricContractEvidenceTransportErrorV1> {
        self.payload.validate_profile_hash()?;
        if self.payload.canonical_hash()? != self.evidence_sha256 {
            return Err(MetricContractEvidenceTransportErrorV1::HashMismatch);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMetricContractEvidenceTransportV1 {
    payload: MetricContractEvidenceHashPayloadV1,
    evidence_sha256: CanonicalHashV1,
    writer_timestamp_ms: u64,
    rotation_part_index: u32,
}

impl<'de> Deserialize<'de> for MetricContractEvidenceTransportV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawMetricContractEvidenceTransportV1::deserialize(deserializer)?;
        let transport = Self::try_new(
            raw.payload,
            raw.writer_timestamp_ms,
            raw.rotation_part_index,
        )
        .map_err(serde::de::Error::custom)?;
        if transport.evidence_sha256 != raw.evidence_sha256 {
            return Err(serde::de::Error::custom(
                MetricContractEvidenceTransportErrorV1::HashMismatch,
            ));
        }
        Ok(transport)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparatorDeltaStatusV1 {
    Equal,
    Different,
    NotEvaluable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractComparatorSummaryV1 {
    pub verdict: ComparatorDeltaStatusV1,
    pub reason: ComparatorDeltaStatusV1,
    pub phase: ComparatorDeltaStatusV1,
    pub soft_points: ComparatorDeltaStatusV1,
}

/// Additive compact-v34 type reserved for PR2C. PR1 defines and round-trips the
/// schema but does not insert it into `GatekeeperBuyLog` or change v33 emission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractDecisionSummaryV1 {
    pub metric_contract_schema_version: u16,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile_id: MetricContractProfileIdV1,
    pub profile_hash: CanonicalHashV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
    pub evidence_record_id: MetricEvidenceRecordIdentityV1,
    pub evidence_sha256: CanonicalHashV1,
    pub evidence_schema_version: u16,
    pub authoritative_contracts: Vec<MetricContractId>,
    pub comparator_contracts: Vec<MetricContractId>,
    pub equivalence_deltas: MetricContractComparatorSummaryV1,
    pub counterfactual_delta_present: bool,
    pub comparator_elapsed_us: u32,
    pub metric_contract_serialize_us: u32,
    pub measured_fields_mask: u16,
}
