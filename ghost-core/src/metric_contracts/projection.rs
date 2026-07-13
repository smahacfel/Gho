use super::{
    CanonicalHashErrorV1, CanonicalHashV1, CanonicalMetricEnvelopeV1, CanonicalNullableV1,
    CanonicalU64StringV1, DevBuyContractEvidenceV1, DevBuySelectionModeV1,
    FlipRatioContractEvidenceV1, FscStatusEvidenceV1, FtdiEvidenceV1,
    FundingSourceContractEvidenceV1, ManipulationComparatorV1, ManipulationNumericEvidenceV2,
    ManipulationNumericFieldEvidenceV2, ManipulationNumericFieldIdV2, MetricAuthorityClass,
    MetricAvailabilityV1, MetricContractEffectiveConfigErrorV1,
    MetricContractEvidenceSemanticErrorV1, MetricContractId, MetricContractProfileIdV1,
    MetricContractProfileV1, MetricContractRolloutMode, MetricContractsEvidenceSetV1,
    MetricEffectiveConfigKeyV1, MetricEffectiveConfigValueV1, MetricEvidenceEnvelopeErrorV1,
    MetricEvidenceReasonV1, MetricMeasurementQualityV1, MetricRolloutRoleV1, MetricSurfaceId,
    RecentBuySellEvidenceV1, ReserveVelocitySourceClockV1, ReserveVelocityStatusV1,
    ResolvedMetricContractEffectiveConfigV1, Top3SignerVolumeEvidenceV1, TxTimingEvidenceV1,
    TxTimingPopulationV1,
};
use crate::checkpoint::EvidenceStatus;
use crate::tx_intelligence::types::FscEvidenceStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1: u16 = 1;
pub const METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1: usize = 8;
pub const METRIC_CONTRACT_PRODUCER_SCHEMA_VERSION_V1: u16 = 1;
pub const METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1: usize = 16 * 1024;
pub const METRIC_CONTRACT_PROJECTION_SERIALIZED_P95_TARGET_BYTES_V1: usize = 12 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDecisionReasonSummaryV1 {
    pub codes: Vec<MetricEvidenceReasonV1>,
    pub omitted_count: u16,
}

impl MetricDecisionReasonSummaryV1 {
    pub fn try_from_codes(
        codes: &[MetricEvidenceReasonV1],
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let mut seen = BTreeSet::new();
        for code in codes {
            let canonical = serde_json::to_string(code)
                .map_err(CanonicalHashErrorV1::from)
                .map_err(MetricContractProjectionErrorV1::Hash)?;
            if !seen.insert(canonical) {
                return Err(MetricContractProjectionErrorV1::DuplicateReasonCode);
            }
        }
        let omitted = codes
            .len()
            .saturating_sub(METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1);
        let omitted_count = u16::try_from(omitted)
            .map_err(|_| MetricContractProjectionErrorV1::ReasonOmittedCountOverflow)?;
        Ok(Self {
            codes: codes
                .iter()
                .take(METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1)
                .cloned()
                .collect(),
            omitted_count,
        })
    }

    pub fn validate(&self) -> Result<(), MetricContractProjectionErrorV1> {
        if self.codes.len() > METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1
            || (self.omitted_count > 0
                && self.codes.len() != METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "compact reason bound/omission count",
            ));
        }
        MetricDecisionReasonSummaryV1::try_from_codes(&self.codes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDecisionEnvelopeV1 {
    pub contract_id: MetricContractId,
    pub contract_version: u16,
    pub surface_id: MetricSurfaceId,
    pub authority_class: MetricAuthorityClass,
    pub rollout_role: MetricRolloutRoleV1,
    pub availability: super::MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub policy_actionable: bool,
    pub reasons: MetricDecisionReasonSummaryV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricContractProducerIdV1 {
    FeeTopologyDiversityProducer,
    TxIntelligenceEngine,
    TxIntelEffectiveTop3Selector,
    TxIntelligenceFingerprintAggregator,
    FundingSourceIndex,
    MaterializedFscStatusAdapter,
    ManipulationEvidenceAdapter,
    ManipulationPolicyDerivation,
    AccountStateCore,
    RecentBuySellWindowProducer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractDecisionSourceCutoffV1 {
    pub decision_timestamp_ms: CanonicalU64StringV1,
    pub decision_slot: CanonicalNullableV1<CanonicalU64StringV1>,
}

impl MetricContractDecisionSourceCutoffV1 {
    pub fn try_new(
        decision_timestamp_ms: u64,
        decision_slot: Option<u64>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        if decision_timestamp_ms == 0 {
            return Err(MetricContractProjectionErrorV1::MissingSourceCutoff);
        }
        Ok(Self {
            decision_timestamp_ms: CanonicalU64StringV1::new(decision_timestamp_ms),
            decision_slot: decision_slot.map(CanonicalU64StringV1::new).into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDecisionSurfaceValueV1<T> {
    pub envelope: MetricDecisionEnvelopeV1,
    pub value: CanonicalNullableV1<T>,
    pub producer_id: MetricContractProducerIdV1,
    pub producer_schema_version: u16,
    pub source_cutoff: MetricContractDecisionSourceCutoffV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDecisionFieldValueV1<T> {
    pub value: CanonicalNullableV1<T>,
    pub availability: super::MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reasons: MetricDecisionReasonSummaryV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDecisionRatioV1 {
    pub surface: MetricDecisionSurfaceValueV1<f64>,
    pub numerator: u32,
    pub denominator: u32,
    pub population: TxTimingPopulationV1,
    pub window_ms: CanonicalNullableV1<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FtdiDecisionProjectionV1 {
    pub legacy_value: MetricDecisionSurfaceValueV1<f64>,
    pub value_v1: MetricDecisionSurfaceValueV1<f64>,
    pub unique_topology_count: u32,
    pub unique_buyer_sample_count: u32,
    pub buy_transaction_sample_count: u32,
    pub legacy_buy_tx_actionability: MetricDecisionSurfaceValueV1<bool>,
    pub unique_buyer_actionability_v2: MetricDecisionSurfaceValueV1<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevBuyDecisionProjectionV1 {
    pub tx_intel_first_observed: MetricDecisionSurfaceValueV1<f64>,
    pub mfs_first_observed: MetricDecisionSurfaceValueV1<f64>,
    pub mfs_primary_v1: MetricDecisionSurfaceValueV1<f64>,
    pub effective_policy: MetricDecisionSurfaceValueV1<f64>,
    pub creator_known: bool,
    pub create_signature_matched: bool,
    pub primary_selection_mode: DevBuySelectionModeV1,
    pub primary_eligible_buy_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxTimingDecisionProjectionV1 {
    pub legacy_exact: MetricDecisionRatioV1,
    pub exact_v1: MetricDecisionRatioV1,
    pub cluster_lt_50ms: MetricDecisionRatioV1,
    pub recent_exact: MetricDecisionRatioV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Top3DecisionProjectionV1 {
    pub preferred: MetricDecisionSurfaceValueV1<f64>,
    pub compatibility_alias: MetricDecisionSurfaceValueV1<f64>,
    pub effective: MetricDecisionSurfaceValueV1<f64>,
    pub preferred_alias_bitwise_equal: CanonicalNullableV1<bool>,
    pub used_compatibility_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FundingDecisionProjectionV1 {
    pub legacy_source: MetricDecisionSurfaceValueV1<f64>,
    pub legacy_v1: MetricDecisionSurfaceValueV1<f64>,
    pub distinct_known_source_count: u32,
    pub known_source_sample_count: u32,
    pub fsc_v2: MetricDecisionSurfaceValueV1<FscEvidenceStatus>,
    pub known_coverage: MetricDecisionFieldValueV1<f64>,
    pub non_neutral_known_coverage: MetricDecisionFieldValueV1<f64>,
    pub known_buyer_count: u32,
    pub total_buyer_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FscStatusDecisionProjectionV1 {
    pub compatibility_status: MetricDecisionSurfaceValueV1<EvidenceStatus>,
    pub legacy_scalar_present: bool,
    pub legacy_feature_status: EvidenceStatus,
    pub fsc_v2_status: CanonicalNullableV1<FscEvidenceStatus>,
    pub fsc_v2_coverage: CanonicalNullableV1<f64>,
}

// PR2B compact family schemas. Full owner/event collections stay in durable
// evidence; these closed types contain only decision-time aggregates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlipDecisionProjectionV1 {
    pub legacy_slot_gap_ratio: MetricDecisionSurfaceValueV1<f64>,
    pub hybrid_v2_ratio: MetricDecisionSurfaceValueV1<f64>,
    pub eligible_buyer_count: u32,
    pub flipper_count: u32,
    pub wall_clock_window_ms: u32,
    pub max_slot_gap: u32,
    pub dump_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManipulationDecisionProjectionV1 {
    pub legacy_numeric_envelope: MetricDecisionEnvelopeV1,
    pub numeric_v2_envelope: MetricDecisionEnvelopeV1,
    pub measured_fields_mask: u16,
    pub same_ms_tx_ratio: MetricDecisionFieldValueV1<f64>,
    pub bundle_suspicion_ratio: MetricDecisionFieldValueV1<f64>,
    pub top3_signer_volume_ratio: MetricDecisionFieldValueV1<f64>,
    pub hhi: MetricDecisionFieldValueV1<f64>,
    pub max_tx_per_signer: MetricDecisionFieldValueV1<f64>,
    pub dev_volume_ratio: MetricDecisionFieldValueV1<f64>,
    pub contradiction_score: MetricDecisionFieldValueV1<f64>,
    pub legacy_high_recorded_mask: u16,
    pub legacy_high_true_mask: u16,
    pub derived_high_evaluable_mask: u16,
    pub derived_high_true_mask: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveVelocityDecisionProjectionV1 {
    pub legacy_velocity: MetricDecisionSurfaceValueV1<f64>,
    pub velocity_v1: MetricDecisionSurfaceValueV1<f64>,
    pub previous_real_sol_reserves_lamports: MetricDecisionFieldValueV1<CanonicalU64StringV1>,
    pub current_real_sol_reserves_lamports: MetricDecisionFieldValueV1<CanonicalU64StringV1>,
    pub interval_ms: MetricDecisionFieldValueV1<u32>,
    pub accepted_update_count: u32,
    pub source_clock: ReserveVelocitySourceClockV1,
    pub status: ReserveVelocityStatusV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentBuySellDecisionProjectionV1 {
    pub legacy_scalar: MetricDecisionSurfaceValueV1<f64>,
    pub v1_envelope: MetricDecisionEnvelopeV1,
    pub window_ms: u32,
    pub buy_count: u32,
    pub sell_count: u32,
    pub transaction_count: u32,
    pub buy_to_sell_ratio: MetricDecisionFieldValueV1<f64>,
    pub buy_share: MetricDecisionFieldValueV1<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractDecisionEvidenceProjectionV1 {
    pub schema_version: u16,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile_id: MetricContractProfileIdV1,
    pub profile_hash: CanonicalHashV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
    pub fee_topology_diversity_index: FtdiDecisionProjectionV1,
    pub dev_buy: DevBuyDecisionProjectionV1,
    pub same_ms_tx_ratio: TxTimingDecisionProjectionV1,
    pub top3_signer_volume_ratio: Top3DecisionProjectionV1,
    pub flip_ratio: FlipDecisionProjectionV1,
    pub funding_source_concentration: FundingDecisionProjectionV1,
    pub fsc_evidence_status: FscStatusDecisionProjectionV1,
    pub manipulation_contradiction: ManipulationDecisionProjectionV1,
    pub reserve_velocity: ReserveVelocityDecisionProjectionV1,
    pub recent_buy_sell: RecentBuySellDecisionProjectionV1,
}

impl MetricContractDecisionEvidenceProjectionV1 {
    pub fn validated_canonical_hash(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<CanonicalHashV1, MetricContractProjectionErrorV1> {
        self.validate_context(context)?;
        let serialized_bytes = self.authoritative_serialized_size_bytes()?;
        ::metrics::histogram!(
            "metric_contract_projection_serialized_bytes",
            serialized_bytes as f64
        );
        if serialized_bytes > METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1 {
            return Err(MetricContractProjectionErrorV1::ProjectionTooLarge {
                actual_bytes: serialized_bytes,
                hard_max_bytes: METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1,
            });
        }
        CanonicalHashV1::digest(self).map_err(MetricContractProjectionErrorV1::Hash)
    }

    /// Exact uncompressed Compact JSON Wire V1 representation embedded by the
    /// field-level `MaterializedFeatureSet` serializer. Domain serde and the
    /// canonical semantic hash intentionally remain independent of this wire.
    pub fn authoritative_serialized_bytes(
        &self,
    ) -> Result<Vec<u8>, MetricContractProjectionErrorV1> {
        super::MetricContractDecisionProjectionWireV1::try_from_domain(self)?
            .json_bytes()
            .map_err(Into::into)
    }

    pub fn authoritative_serialized_size_bytes(
        &self,
    ) -> Result<usize, MetricContractProjectionErrorV1> {
        self.authoritative_serialized_bytes()
            .map(|bytes| bytes.len())
    }

    pub fn verbose_domain_json_diagnostic_size_bytes(
        &self,
    ) -> Result<usize, MetricContractProjectionErrorV1> {
        serde_json::to_vec(self)
            .map(|bytes| bytes.len())
            .map_err(|_| MetricContractProjectionErrorV1::ProjectionSerialization)
    }

    pub fn bincode_diagnostic_size_bytes(&self) -> Result<usize, MetricContractProjectionErrorV1> {
        bincode::serialize(self)
            .map(|bytes| bytes.len())
            .map_err(|_| MetricContractProjectionErrorV1::ProjectionSerialization)
    }

    pub fn validate_context(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        let profile_hash = context.validate_and_profile_hash()?;
        self.validate_context_with_validated_context(context, &profile_hash)
    }

    fn validate_context_with_validated_context(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
        profile_hash: &CanonicalHashV1,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        if self.schema_version != METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1 {
            return Err(
                MetricContractProjectionErrorV1::UnsupportedProjectionSchema(self.schema_version),
            );
        }
        if self.rollout_mode != context.rollout_mode
            || self.profile_id != context.profile.payload().profile_id
            || self.profile_hash != *profile_hash
            || self.metric_contract_effective_config_hash
                != context
                    .effective_config
                    .metric_contract_effective_config_hash
        {
            return Err(MetricContractProjectionErrorV1::ProjectionContextMismatch);
        }
        for (surface, expected, producer) in [
            (
                &self.fee_topology_diversity_index.legacy_value,
                MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
            ),
            (
                &self.fee_topology_diversity_index.value_v1,
                MetricSurfaceId::FtdiValueEvidenceV1,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
            ),
        ] {
            surface.validate(expected, producer, context)?;
        }
        self.fee_topology_diversity_index
            .legacy_buy_tx_actionability
            .validate(
                MetricSurfaceId::FtdiLegacyBuyTxActionability,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?;
        self.fee_topology_diversity_index
            .unique_buyer_actionability_v2
            .validate(
                MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?;
        for (surface, expected, producer) in [
            (
                &self.dev_buy.tx_intel_first_observed,
                MetricSurfaceId::TxIntelDevFirstObservedBuySol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.dev_buy.mfs_first_observed,
                MetricSurfaceId::MfsDevFirstObservedBuySol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.dev_buy.mfs_primary_v1,
                MetricSurfaceId::MfsDevPrimaryBuySolV1,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.dev_buy.effective_policy,
                MetricSurfaceId::EffectivePolicyDevBuySol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
        ] {
            surface.validate(expected, producer, context)?;
        }
        for (ratio, expected, producer) in [
            (
                &self.same_ms_tx_ratio.legacy_exact,
                MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.same_ms_tx_ratio.exact_v1,
                MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.same_ms_tx_ratio.cluster_lt_50ms,
                MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
                MetricContractProducerIdV1::TxIntelligenceEngine,
            ),
            (
                &self.same_ms_tx_ratio.recent_exact,
                MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
                MetricContractProducerIdV1::RecentBuySellWindowProducer,
            ),
        ] {
            ratio.surface.validate(expected, producer, context)?;
        }
        for (surface, expected, producer) in [
            (
                &self.top3_signer_volume_ratio.preferred,
                MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
            ),
            (
                &self.top3_signer_volume_ratio.compatibility_alias,
                MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
            ),
            (
                &self.top3_signer_volume_ratio.effective,
                MetricSurfaceId::TxIntelTop3EffectiveSelector,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
            ),
            (
                &self.flip_ratio.legacy_slot_gap_ratio,
                MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
            ),
            (
                &self.flip_ratio.hybrid_v2_ratio,
                MetricSurfaceId::FlipRatioHybridEvidenceV2,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
            ),
            (
                &self.funding_source_concentration.legacy_source,
                MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
                MetricContractProducerIdV1::FundingSourceIndex,
            ),
            (
                &self.funding_source_concentration.legacy_v1,
                MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1,
                MetricContractProducerIdV1::FundingSourceIndex,
            ),
        ] {
            surface.validate(expected, producer, context)?;
        }
        self.funding_source_concentration.fsc_v2.validate(
            MetricSurfaceId::FundingSourceV2ReadinessEvidence,
            MetricContractProducerIdV1::FundingSourceIndex,
            context,
        )?;
        self.funding_source_concentration
            .known_coverage
            .validate()?;
        self.funding_source_concentration
            .non_neutral_known_coverage
            .validate()?;
        self.fsc_evidence_status.compatibility_status.validate(
            MetricSurfaceId::MaterializedFscStatusCompatibility,
            MetricContractProducerIdV1::MaterializedFscStatusAdapter,
            context,
        )?;
        self.flip_ratio.legacy_slot_gap_ratio.validate(
            MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
            MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
            context,
        )?;
        self.flip_ratio.hybrid_v2_ratio.validate(
            MetricSurfaceId::FlipRatioHybridEvidenceV2,
            MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
            context,
        )?;
        self.reserve_velocity.legacy_velocity.validate(
            MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
            MetricContractProducerIdV1::AccountStateCore,
            context,
        )?;
        self.reserve_velocity.velocity_v1.validate(
            MetricSurfaceId::ReserveVelocityEvidenceV1,
            MetricContractProducerIdV1::AccountStateCore,
            context,
        )?;
        self.recent_buy_sell.legacy_scalar.validate(
            MetricSurfaceId::RceBuySellRatioRecentLegacy,
            MetricContractProducerIdV1::RecentBuySellWindowProducer,
            context,
        )?;
        validate_compact_envelope(
            &self.recent_buy_sell.v1_envelope,
            MetricSurfaceId::RecentBuySellEvidenceV1,
            context,
        )?;
        validate_compact_envelope(
            &self.manipulation_contradiction.legacy_numeric_envelope,
            MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
            context,
        )?;
        validate_compact_envelope(
            &self.manipulation_contradiction.numeric_v2_envelope,
            MetricSurfaceId::ManipulationNumericEvidenceV2,
            context,
        )?;
        for field in [
            &self.manipulation_contradiction.same_ms_tx_ratio,
            &self.manipulation_contradiction.bundle_suspicion_ratio,
            &self.manipulation_contradiction.top3_signer_volume_ratio,
            &self.manipulation_contradiction.hhi,
            &self.manipulation_contradiction.max_tx_per_signer,
            &self.manipulation_contradiction.dev_volume_ratio,
            &self.manipulation_contradiction.contradiction_score,
        ] {
            field.validate()?;
        }
        self.reserve_velocity
            .previous_real_sol_reserves_lamports
            .validate()?;
        self.reserve_velocity
            .current_real_sol_reserves_lamports
            .validate()?;
        self.reserve_velocity.interval_ms.validate()?;
        self.recent_buy_sell.buy_to_sell_ratio.validate()?;
        self.recent_buy_sell.buy_share.validate()?;
        self.fee_topology_diversity_index
            .validate_semantics(context)?;
        self.dev_buy.validate_semantics(context)?;
        self.same_ms_tx_ratio.validate_semantics(context)?;
        self.top3_signer_volume_ratio.validate_semantics(context)?;
        self.funding_source_concentration
            .validate_semantics(context)?;
        self.fsc_evidence_status
            .validate_semantics(&self.funding_source_concentration, context)?;
        self.flip_ratio.validate_semantics(context)?;
        self.manipulation_contradiction
            .validate_semantics(context)?;
        self.reserve_velocity.validate_semantics(context)?;
        self.recent_buy_sell.validate_semantics(context)?;
        Ok(())
    }
}

fn validate_value_status_coherence<T>(
    value: &CanonicalNullableV1<T>,
    availability: super::MetricAvailabilityV1,
    measurement_quality: MetricMeasurementQualityV1,
) -> Result<(), MetricContractProjectionErrorV1> {
    match availability {
        super::MetricAvailabilityV1::Available => {
            if value.is_null() {
                return Err(MetricContractProjectionErrorV1::ValueStatusInvariant(
                    "available evidence must contain a value",
                ));
            }
            if measurement_quality == MetricMeasurementQualityV1::NotApplicable {
                return Err(MetricContractProjectionErrorV1::ValueStatusInvariant(
                    "available evidence cannot be not_applicable",
                ));
            }
        }
        super::MetricAvailabilityV1::Unavailable
        | super::MetricAvailabilityV1::NotConfigured
        | super::MetricAvailabilityV1::NotRecordedLegacySchema => {
            if !value.is_null() {
                return Err(MetricContractProjectionErrorV1::ValueStatusInvariant(
                    "non-available evidence cannot contain a value",
                ));
            }
            if measurement_quality != MetricMeasurementQualityV1::NotApplicable {
                return Err(MetricContractProjectionErrorV1::ValueStatusInvariant(
                    "non-available evidence must be not_applicable",
                ));
            }
        }
    }
    Ok(())
}

impl<T> MetricDecisionFieldValueV1<T> {
    pub fn validate(&self) -> Result<(), MetricContractProjectionErrorV1> {
        self.reasons.validate()?;
        validate_value_status_coherence(&self.value, self.availability, self.measurement_quality)
    }
}

impl<T> MetricDecisionSurfaceValueV1<T> {
    pub fn validate(
        &self,
        expected_surface: MetricSurfaceId,
        expected_producer: MetricContractProducerIdV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        if self.producer_id != expected_producer {
            return Err(MetricContractProjectionErrorV1::ProducerMismatch {
                surface: expected_surface,
                expected: expected_producer,
                actual: self.producer_id,
            });
        }
        if self.producer_schema_version == 0 {
            return Err(MetricContractProjectionErrorV1::MissingProducerSchema);
        }
        if self.source_cutoff.decision_timestamp_ms.get() == 0 {
            return Err(MetricContractProjectionErrorV1::MissingSourceCutoff);
        }
        if self.source_cutoff != context.source_cutoff {
            return Err(MetricContractProjectionErrorV1::ProjectionContextMismatch);
        }
        validate_compact_envelope(&self.envelope, expected_surface, context)?;
        validate_value_status_coherence(
            &self.value,
            self.envelope.availability,
            self.envelope.measurement_quality,
        )
    }
}

pub struct MetricDecisionProjectionBuildContextV1<'a> {
    pub rollout_mode: MetricContractRolloutMode,
    pub profile: &'a MetricContractProfileV1,
    pub effective_config: &'a ResolvedMetricContractEffectiveConfigV1,
    pub source_cutoff: MetricContractDecisionSourceCutoffV1,
}

impl MetricDecisionProjectionBuildContextV1<'_> {
    pub fn validate(&self) -> Result<(), MetricContractProjectionErrorV1> {
        self.validate_and_profile_hash().map(|_| ())
    }

    fn validate_and_profile_hash(
        &self,
    ) -> Result<CanonicalHashV1, MetricContractProjectionErrorV1> {
        self.effective_config.validate_hash()?;
        let profile_hash = self.profile.canonical_hash()?;
        let payload = &self.effective_config.payload;
        if payload.rollout_mode != self.rollout_mode
            || payload.profile_id != self.profile.payload().profile_id
            || payload.profile_hash != profile_hash
        {
            return Err(MetricContractProjectionErrorV1::ProjectionContextMismatch);
        }
        if self.source_cutoff.decision_timestamp_ms.get() == 0 {
            return Err(MetricContractProjectionErrorV1::MissingSourceCutoff);
        }
        Ok(profile_hash)
    }
}

#[derive(Debug, Error)]
pub enum MetricContractProjectionErrorV1 {
    #[error(transparent)]
    Hash(#[from] CanonicalHashErrorV1),
    #[error(transparent)]
    EffectiveConfig(#[from] MetricContractEffectiveConfigErrorV1),
    #[error(transparent)]
    Envelope(#[from] MetricEvidenceEnvelopeErrorV1),
    #[error(transparent)]
    EvidenceSemantics(#[from] MetricContractEvidenceSemanticErrorV1),
    #[error(transparent)]
    Wire(#[from] super::MetricContractProjectionWireErrorV1),
    #[error("compact projection deterministic serialization failed")]
    ProjectionSerialization,
    #[error("duplicate reason code in compact projection input")]
    DuplicateReasonCode,
    #[error("omitted reason count exceeds u16")]
    ReasonOmittedCountOverflow,
    #[error("projection producer schema version must be non-zero")]
    MissingProducerSchema,
    #[error("projection producer mismatch for {surface:?}: expected {expected:?}, got {actual:?}")]
    ProducerMismatch {
        surface: MetricSurfaceId,
        expected: MetricContractProducerIdV1,
        actual: MetricContractProducerIdV1,
    },
    #[error("projection source cutoff is missing")]
    MissingSourceCutoff,
    #[error("projection value/status invariant failed: {0}")]
    ValueStatusInvariant(&'static str),
    #[error("projection context does not match profile/effective config")]
    ProjectionContextMismatch,
    #[error("unsupported metric-contract decision projection schema {0}")]
    UnsupportedProjectionSchema(u16),
    #[error("projection family invariant failed: {0}")]
    FamilyInvariant(&'static str),
    #[error("projection/effective-config parity failed for {key:?}: {invariant}")]
    EffectiveConfigParity {
        key: MetricEffectiveConfigKeyV1,
        invariant: &'static str,
    },
    #[error("compact projection serialized size {actual_bytes} exceeds hard max {hard_max_bytes}")]
    ProjectionTooLarge {
        actual_bytes: usize,
        hard_max_bytes: usize,
    },
}

fn compact_envelope(
    source: &CanonicalMetricEnvelopeV1,
    expected_surface: MetricSurfaceId,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<MetricDecisionEnvelopeV1, MetricContractProjectionErrorV1> {
    if source.surface_id != expected_surface {
        return Err(
            MetricEvidenceEnvelopeErrorV1::UnexpectedSurfaceForEvidenceField {
                expected: expected_surface,
                actual: source.surface_id,
            }
            .into(),
        );
    }
    source.validate_for_profile(context.profile, context.rollout_mode)?;
    let assignment = context.profile.entry_for(expected_surface).ok_or(
        MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromProfile(expected_surface),
    )?;
    Ok(MetricDecisionEnvelopeV1 {
        contract_id: source.contract_id,
        contract_version: source.contract_version,
        surface_id: source.surface_id,
        authority_class: source.authority_class,
        rollout_role: assignment.role_for(context.rollout_mode),
        availability: source.availability,
        measurement_quality: source.measurement_quality,
        policy_actionable: source.policy_actionable,
        reasons: MetricDecisionReasonSummaryV1::try_from_codes(&source.reason_codes)?,
    })
}

fn validate_compact_envelope(
    envelope: &MetricDecisionEnvelopeV1,
    expected_surface: MetricSurfaceId,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<(), MetricContractProjectionErrorV1> {
    if envelope.surface_id != expected_surface {
        return Err(
            MetricEvidenceEnvelopeErrorV1::UnexpectedSurfaceForEvidenceField {
                expected: expected_surface,
                actual: envelope.surface_id,
            }
            .into(),
        );
    }
    envelope.reasons.validate()?;
    let canonical = CanonicalMetricEnvelopeV1::try_new(
        envelope.contract_id,
        envelope.contract_version,
        envelope.surface_id,
        envelope.authority_class,
        envelope.availability,
        envelope.measurement_quality,
        envelope.policy_actionable,
        envelope.reasons.codes.clone(),
    )?;
    canonical.validate_for_profile(context.profile, context.rollout_mode)?;
    let expected_role = context
        .profile
        .entry_for(expected_surface)
        .ok_or(MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromProfile(
            expected_surface,
        ))?
        .role_for(context.rollout_mode);
    if envelope.rollout_role != expected_role {
        return Err(MetricContractProjectionErrorV1::ProjectionContextMismatch);
    }
    Ok(())
}

fn surface_value<T: Clone>(
    source: &CanonicalMetricEnvelopeV1,
    expected_surface: MetricSurfaceId,
    value: &CanonicalNullableV1<T>,
    producer_id: MetricContractProducerIdV1,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<MetricDecisionSurfaceValueV1<T>, MetricContractProjectionErrorV1> {
    if METRIC_CONTRACT_PRODUCER_SCHEMA_VERSION_V1 == 0 {
        return Err(MetricContractProjectionErrorV1::MissingProducerSchema);
    }
    let surface = MetricDecisionSurfaceValueV1 {
        envelope: compact_envelope(source, expected_surface, context)?,
        value: value.clone(),
        producer_id,
        producer_schema_version: METRIC_CONTRACT_PRODUCER_SCHEMA_VERSION_V1,
        source_cutoff: context.source_cutoff.clone(),
    };
    surface.validate(expected_surface, producer_id, context)?;
    Ok(surface)
}

fn actionability_value(
    source: &CanonicalMetricEnvelopeV1,
    expected_surface: MetricSurfaceId,
    value: bool,
    producer_id: MetricContractProducerIdV1,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<MetricDecisionSurfaceValueV1<bool>, MetricContractProjectionErrorV1> {
    surface_value(
        source,
        expected_surface,
        &CanonicalNullableV1::Value(value),
        producer_id,
        context,
    )
}

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

fn nullable_f64_value(value: &CanonicalNullableV1<f64>) -> Option<f64> {
    match value {
        CanonicalNullableV1::Null => None,
        CanonicalNullableV1::Value(value) => Some(*value),
    }
}

fn validate_optional_unit_ratio(
    value: &CanonicalNullableV1<f64>,
    invariant: &'static str,
) -> Result<Option<f64>, MetricContractProjectionErrorV1> {
    let value = nullable_f64_value(value);
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err(MetricContractProjectionErrorV1::FamilyInvariant(invariant));
    }
    Ok(value)
}

fn validate_optional_nonnegative_finite(
    value: &CanonicalNullableV1<f64>,
    invariant: &'static str,
) -> Result<(), MetricContractProjectionErrorV1> {
    if nullable_f64_value(value).is_some_and(|value| !value.is_finite() || value < 0.0) {
        return Err(MetricContractProjectionErrorV1::FamilyInvariant(invariant));
    }
    Ok(())
}

fn required_bool(
    value: &CanonicalNullableV1<bool>,
    invariant: &'static str,
) -> Result<bool, MetricContractProjectionErrorV1> {
    match value {
        CanonicalNullableV1::Value(value) => Ok(*value),
        CanonicalNullableV1::Null => {
            Err(MetricContractProjectionErrorV1::FamilyInvariant(invariant))
        }
    }
}

fn effective_config_wide_unsigned(
    context: &MetricDecisionProjectionBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    invariant: &'static str,
) -> Result<u64, MetricContractProjectionErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::WideUnsigned(value)) => Ok(value.get()),
        _ => Err(MetricContractProjectionErrorV1::EffectiveConfigParity { key, invariant }),
    }
}

fn effective_config_ratio(
    context: &MetricDecisionProjectionBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    invariant: &'static str,
) -> Result<f64, MetricContractProjectionErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Ratio(value))
            if value.is_finite() && (0.0..=1.0).contains(value) =>
        {
            Ok(*value)
        }
        _ => Err(MetricContractProjectionErrorV1::EffectiveConfigParity { key, invariant }),
    }
}

fn effective_config_enum_matches(
    context: &MetricDecisionProjectionBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    expected: &'static str,
    invariant: &'static str,
) -> Result<(), MetricContractProjectionErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Enum(actual)) if actual == expected => Ok(()),
        _ => Err(MetricContractProjectionErrorV1::EffectiveConfigParity { key, invariant }),
    }
}

fn effective_config_boolean_matches(
    context: &MetricDecisionProjectionBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    expected: bool,
    invariant: &'static str,
) -> Result<(), MetricContractProjectionErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Boolean(actual)) if *actual == expected => Ok(()),
        _ => Err(MetricContractProjectionErrorV1::EffectiveConfigParity { key, invariant }),
    }
}

impl FtdiDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::FtdiPopulationSuccessfulBuy,
                "successful_buy",
                "FTDI population semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::FtdiDenominatorRule,
                "unique_topologies_over_unique_first_buyer_samples",
                "FTDI denominator semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::FtdiMissingSignerBehavior,
                "legacy_empty_signer_identity",
                "FTDI missing-signer semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::FtdiMissingTopologyBehavior,
                "unavailable_entire_metric",
                "FTDI missing-topology semantics",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        effective_config_boolean_matches(
            context,
            MetricEffectiveConfigKeyV1::FtdiFirstSamplePerSigner,
            true,
            "FTDI first-sample-per-signer semantics",
        )?;
        if !nullable_f64_bits_equal(&self.legacy_value.value, &self.value_v1.value) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FTDI legacy/typed value parity",
            ));
        }
        if self.unique_topology_count > self.unique_buyer_sample_count
            || self.unique_buyer_sample_count > self.buy_transaction_sample_count
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FTDI count ordering",
            ));
        }

        let value_present = match validate_optional_unit_ratio(
            &self.legacy_value.value,
            "FTDI value must be finite ratio",
        )? {
            Some(value) => {
                if self.unique_buyer_sample_count == 0 {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "FTDI value requires unique-buyer denominator",
                    ));
                }
                let expected =
                    self.unique_topology_count as f64 / self.unique_buyer_sample_count as f64;
                if value.to_bits() != expected.to_bits() {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "FTDI value/count parity",
                    ));
                }
                true
            }
            None => {
                if self.unique_topology_count != 0 {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "null FTDI value cannot claim measured topologies",
                    ));
                }
                false
            }
        };

        let legacy_gate = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FtdiLegacyCleanMinBuyTransactions,
            "FTDI legacy sample gate config",
        )?;
        let corrected_gate = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FtdiCandidateCleanMinUniqueBuyers,
            "FTDI corrected sample gate config",
        )?;
        let diagnostic_gate = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers,
            "FTDI diagnostic sample gate config",
        )?;
        if value_present && u64::from(self.unique_buyer_sample_count) < diagnostic_gate {
            return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                key: MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers,
                invariant: "measured FTDI value is below configured diagnostic sample gate",
            });
        }
        let expected_legacy =
            value_present && u64::from(self.buy_transaction_sample_count) >= legacy_gate;
        let expected_corrected =
            value_present && u64::from(self.unique_buyer_sample_count) >= corrected_gate;
        let legacy_actionable = required_bool(
            &self.legacy_buy_tx_actionability.value,
            "FTDI legacy actionability value",
        )?;
        let corrected_actionable = required_bool(
            &self.unique_buyer_actionability_v2.value,
            "FTDI corrected actionability value",
        )?;
        if legacy_actionable != expected_legacy || corrected_actionable != expected_corrected {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FTDI actionability/count parity",
            ));
        }
        let expected_policy_actionable = expected_legacy
            && self.legacy_buy_tx_actionability.envelope.rollout_role
                == MetricRolloutRoleV1::PolicyAuthoritative;
        if self.legacy_buy_tx_actionability.envelope.policy_actionable != expected_policy_actionable
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FTDI legacy policy actionability",
            ));
        }
        if self
            .unique_buyer_actionability_v2
            .envelope
            .policy_actionable
            || self.unique_buyer_actionability_v2.envelope.authority_class
                != MetricAuthorityClass::Counterfactual
            || self.unique_buyer_actionability_v2.envelope.rollout_role
                != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "corrected FTDI actionability must remain counterfactual",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &FtdiEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &FtdiEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        if evidence.legacy_value.unique_topology_count != evidence.value_v1.unique_topology_count
            || evidence.legacy_value.unique_buyer_sample_count
                != evidence.value_v1.unique_buyer_sample_count
            || evidence.legacy_value.buy_transaction_sample_count
                != evidence.value_v1.buy_transaction_sample_count
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "ftdi count parity",
            ));
        }
        if !nullable_f64_bits_equal(&evidence.legacy_value.value, &evidence.value_v1.value) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FTDI legacy/typed value parity",
            ));
        }
        let projection = Self {
            legacy_value: surface_value(
                &evidence.legacy_value.envelope,
                MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
                &evidence.legacy_value.value,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?,
            value_v1: surface_value(
                &evidence.value_v1.envelope,
                MetricSurfaceId::FtdiValueEvidenceV1,
                &evidence.value_v1.value,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?,
            unique_topology_count: evidence.legacy_value.unique_topology_count,
            unique_buyer_sample_count: evidence.legacy_value.unique_buyer_sample_count,
            buy_transaction_sample_count: evidence.legacy_value.buy_transaction_sample_count,
            legacy_buy_tx_actionability: actionability_value(
                &evidence.legacy_actionability_envelope,
                MetricSurfaceId::FtdiLegacyBuyTxActionability,
                evidence.legacy_buy_tx_actionable,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?,
            unique_buyer_actionability_v2: actionability_value(
                &evidence.unique_buyer_actionability_v2_envelope,
                MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
                evidence.unique_buyer_actionable_v2,
                MetricContractProducerIdV1::FeeTopologyDiversityProducer,
                context,
            )?,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl DevBuyDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::DevTxIntelSuccessEligibility,
                "accepted_successful_or_failed",
                "dev first-observed eligibility semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::DevFirstObservedAnchorRule,
                "first_accepted_creator_buy_in_ingest_order",
                "dev first-observed anchor semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::DevPrimaryAnchorRule,
                "create_signature_then_earliest_eligible_creator_buy",
                "dev primary selection semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::DevMissingCreatorBehavior,
                "unavailable",
                "dev missing-creator semantics",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        effective_config_boolean_matches(
            context,
            MetricEffectiveConfigKeyV1::DevPrimarySuccessRequired,
            true,
            "dev primary success requirement",
        )?;
        if !nullable_f64_bits_equal(
            &self.tx_intel_first_observed.value,
            &self.mfs_first_observed.value,
        ) || !nullable_f64_bits_equal(
            &self.mfs_first_observed.value,
            &self.effective_policy.value,
        ) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "dev first-observed/effective-policy parity",
            ));
        }
        for value in [
            &self.tx_intel_first_observed.value,
            &self.mfs_first_observed.value,
            &self.mfs_primary_v1.value,
            &self.effective_policy.value,
        ] {
            validate_optional_nonnegative_finite(value, "dev buy must be finite non-negative")?;
        }

        match &self.mfs_primary_v1.value {
            CanonicalNullableV1::Value(_) => {
                if !self.creator_known
                    || self.primary_eligible_buy_count == 0
                    || !matches!(
                        self.primary_selection_mode,
                        DevBuySelectionModeV1::CreateSignatureMatch
                            | DevBuySelectionModeV1::EarliestEligibleCreatorBuy
                    )
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "dev primary value/selection parity",
                    ));
                }
            }
            CanonicalNullableV1::Null => {
                if self.primary_selection_mode != DevBuySelectionModeV1::NoEligibleBuy
                    || self.primary_eligible_buy_count != 0
                    || self.create_signature_matched
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "null dev primary must be no-eligible-buy",
                    ));
                }
            }
        }
        if self.create_signature_matched
            != (self.primary_selection_mode == DevBuySelectionModeV1::CreateSignatureMatch)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "dev primary create-signature selection parity",
            ));
        }
        if self.mfs_primary_v1.envelope.policy_actionable
            || self.mfs_primary_v1.envelope.authority_class != MetricAuthorityClass::Counterfactual
            || self.mfs_primary_v1.envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "dev primary must remain counterfactual",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &DevBuyContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &DevBuyContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        if !nullable_f64_bits_equal(
            &evidence.tx_intel_first_observed.amount_sol,
            &evidence.mfs_first_observed.amount_sol,
        ) || !nullable_f64_bits_equal(
            &evidence.mfs_first_observed.amount_sol,
            &evidence.effective_policy.amount_sol,
        ) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "dev first-observed/effective-policy parity",
            ));
        }
        let projection = Self {
            tx_intel_first_observed: surface_value(
                &evidence.tx_intel_first_observed.envelope,
                MetricSurfaceId::TxIntelDevFirstObservedBuySol,
                &evidence.tx_intel_first_observed.amount_sol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            mfs_first_observed: surface_value(
                &evidence.mfs_first_observed.envelope,
                MetricSurfaceId::MfsDevFirstObservedBuySol,
                &evidence.mfs_first_observed.amount_sol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            mfs_primary_v1: surface_value(
                &evidence.mfs_primary_v1.envelope,
                MetricSurfaceId::MfsDevPrimaryBuySolV1,
                &evidence.mfs_primary_v1.amount_sol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            effective_policy: surface_value(
                &evidence.effective_policy.envelope,
                MetricSurfaceId::EffectivePolicyDevBuySol,
                &evidence.effective_policy.amount_sol,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            creator_known: evidence.mfs_primary_v1.creator_known,
            create_signature_matched: evidence.mfs_primary_v1.create_signature_matched,
            primary_selection_mode: evidence.mfs_primary_v1.selection_mode,
            primary_eligible_buy_count: evidence.mfs_primary_v1.eligible_buy_count,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

fn timing_ratio(
    measurement: &super::TxTimingMeasurementEvidenceV1,
    expected_surface: MetricSurfaceId,
    producer_id: MetricContractProducerIdV1,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<MetricDecisionRatioV1, MetricContractProjectionErrorV1> {
    Ok(MetricDecisionRatioV1 {
        surface: surface_value(
            &measurement.envelope,
            expected_surface,
            &measurement.ratio,
            producer_id,
            context,
        )?,
        numerator: measurement.numerator,
        denominator: measurement.denominator,
        population: measurement.population,
        window_ms: measurement.window_ms.clone(),
    })
}

impl MetricDecisionRatioV1 {
    pub fn validate_semantics(&self) -> Result<(), MetricContractProjectionErrorV1> {
        if self.numerator > self.denominator {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "timing numerator cannot exceed denominator",
            ));
        }
        if self.denominator == 0 {
            if !self.surface.value.is_null() {
                return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                    "zero timing denominator requires null ratio",
                ));
            }
            return Ok(());
        }
        let CanonicalNullableV1::Value(value) = &self.surface.value else {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "positive timing denominator requires ratio",
            ));
        };
        let expected = self.numerator as f64 / self.denominator as f64;
        if !value.is_finite() || value.to_bits() != expected.to_bits() {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "timing ratio/count parity",
            ));
        }
        Ok(())
    }
}

impl TxTimingDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::SameMsLegacyPopulation,
                "accepted_non_dust_successful_or_failed",
                "legacy timing population semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::SameMsLegacyDenominatorRule,
                "adjacent_exact_collisions_over_transaction_count",
                "legacy timing denominator semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::SameMsRecentPopulation,
                "successful_accepted_recent_window",
                "recent timing population semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::SameMsRecentDenominatorRule,
                "same_timestamp_extras_over_transaction_count",
                "recent timing denominator semantics",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::SameMsExactDeltaMs,
                0,
                "exact same-ms delta must remain zero in projection schema V1",
            ),
            (
                MetricEffectiveConfigKeyV1::SameMsClusterUpperBoundExclusiveMs,
                50,
                "cluster upper bound must remain exclusive 50ms in projection schema V1",
            ),
        ] {
            let actual = effective_config_wide_unsigned(context, key, invariant)?;
            if actual != expected {
                return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                    key,
                    invariant,
                });
            }
        }
        let recent_window_ms = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
            "recent timing window must be a wide unsigned value",
        )?;
        let recent_window_ms = u32::try_from(recent_window_ms).map_err(|_| {
            MetricContractProjectionErrorV1::EffectiveConfigParity {
                key: MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
                invariant: "recent timing window does not fit compact u32 representation",
            }
        })?;
        for ratio in [
            &self.legacy_exact,
            &self.exact_v1,
            &self.cluster_lt_50ms,
            &self.recent_exact,
        ] {
            ratio.validate_semantics()?;
        }
        if self.legacy_exact.numerator != self.exact_v1.numerator
            || self.legacy_exact.denominator != self.exact_v1.denominator
            || !nullable_f64_bits_equal(
                &self.legacy_exact.surface.value,
                &self.exact_v1.surface.value,
            )
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "same-ms legacy/exact typed parity",
            ));
        }
        if self.legacy_exact.population != TxTimingPopulationV1::AcceptedTransactions
            || self.exact_v1.population != TxTimingPopulationV1::AcceptedTransactions
            || self.cluster_lt_50ms.population != TxTimingPopulationV1::AcceptedTransactions
            || self.recent_exact.population != TxTimingPopulationV1::SuccessfulTransactions
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "timing population contract",
            ));
        }
        if !self.legacy_exact.window_ms.is_null()
            || !self.exact_v1.window_ms.is_null()
            || !self.cluster_lt_50ms.window_ms.is_null()
            || self.recent_exact.window_ms != CanonicalNullableV1::Value(recent_window_ms)
        {
            return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                key: MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
                invariant: "compact recent timing window disagrees with effective config",
            });
        }
        if self.cluster_lt_50ms.surface.envelope.policy_actionable
            || self.recent_exact.surface.envelope.policy_actionable
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "timing evidence-only/logging-only surfaces cannot be actionable",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &TxTimingEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &TxTimingEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        if evidence.legacy_exact.numerator != evidence.exact_v1.numerator
            || evidence.legacy_exact.denominator != evidence.exact_v1.denominator
            || !nullable_f64_bits_equal(&evidence.legacy_exact.ratio, &evidence.exact_v1.ratio)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "same-ms legacy/exact typed parity",
            ));
        }
        let projection = Self {
            legacy_exact: timing_ratio(
                &evidence.legacy_exact,
                MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            exact_v1: timing_ratio(
                &evidence.exact_v1,
                MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            cluster_lt_50ms: timing_ratio(
                &evidence.cluster_lt_50ms,
                MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
                MetricContractProducerIdV1::TxIntelligenceEngine,
                context,
            )?,
            recent_exact: timing_ratio(
                &evidence.recent_exact,
                MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
                MetricContractProducerIdV1::RecentBuySellWindowProducer,
                context,
            )?,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl Top3DecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::Top3PreferredField,
                "top3_signer_volume_ratio",
                "top3 preferred-field semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::Top3FallbackAlias,
                "top3_volume_pct",
                "top3 fallback-alias semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::Top3Scale,
                "ratio_0_1",
                "top3 scale semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::Top3MismatchBehavior,
                "preferred_authoritative_emit_mismatch_telemetry",
                "top3 mismatch semantics",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        for value in [
            &self.preferred.value,
            &self.compatibility_alias.value,
            &self.effective.value,
        ] {
            validate_optional_unit_ratio(value, "top3 value must be finite ratio")?;
        }
        let expected_equal = match (&self.preferred.value, &self.compatibility_alias.value) {
            (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
                CanonicalNullableV1::Value(left.to_bits() == right.to_bits())
            }
            _ => CanonicalNullableV1::Null,
        };
        let expected_effective = match &self.preferred.value {
            CanonicalNullableV1::Value(_) => &self.preferred.value,
            CanonicalNullableV1::Null => &self.compatibility_alias.value,
        };
        let expected_fallback =
            self.preferred.value.is_null() && !self.compatibility_alias.value.is_null();
        if !nullable_f64_bits_equal(&self.effective.value, expected_effective)
            || self.used_compatibility_fallback != expected_fallback
            || self.preferred_alias_bitwise_equal != expected_equal
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "top3 selector parity",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &Top3SignerVolumeEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &Top3SignerVolumeEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let expected_equal = match (
            &evidence.preferred_ratio,
            &evidence.compatibility_alias_ratio,
        ) {
            (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
                CanonicalNullableV1::Value(left.to_bits() == right.to_bits())
            }
            _ => CanonicalNullableV1::Null,
        };
        let selector_valid = match &evidence.preferred_ratio {
            CanonicalNullableV1::Value(_) => {
                !evidence.used_compatibility_fallback
                    && nullable_f64_bits_equal(&evidence.effective_ratio, &evidence.preferred_ratio)
            }
            CanonicalNullableV1::Null => match &evidence.compatibility_alias_ratio {
                CanonicalNullableV1::Value(_) => {
                    evidence.used_compatibility_fallback
                        && nullable_f64_bits_equal(
                            &evidence.effective_ratio,
                            &evidence.compatibility_alias_ratio,
                        )
                }
                CanonicalNullableV1::Null => {
                    !evidence.used_compatibility_fallback
                        && matches!(evidence.effective_ratio, CanonicalNullableV1::Null)
                }
            },
        };
        if evidence.preferred_alias_bitwise_equal != expected_equal || !selector_valid {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "top3 selector parity",
            ));
        }
        let projection = Self {
            preferred: surface_value(
                &evidence.preferred_envelope,
                MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
                &evidence.preferred_ratio,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
                context,
            )?,
            compatibility_alias: surface_value(
                &evidence.compatibility_alias_envelope,
                MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
                &evidence.compatibility_alias_ratio,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
                context,
            )?,
            effective: surface_value(
                &evidence.effective_selector_envelope,
                MetricSurfaceId::TxIntelTop3EffectiveSelector,
                &evidence.effective_ratio,
                MetricContractProducerIdV1::TxIntelEffectiveTop3Selector,
                context,
            )?,
            preferred_alias_bitwise_equal: evidence.preferred_alias_bitwise_equal.clone(),
            used_compatibility_fallback: evidence.used_compatibility_fallback,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

fn field_value_from_envelope<T: Clone>(
    value: &CanonicalNullableV1<T>,
    envelope: &CanonicalMetricEnvelopeV1,
) -> Result<MetricDecisionFieldValueV1<T>, MetricContractProjectionErrorV1> {
    let field = MetricDecisionFieldValueV1 {
        value: value.clone(),
        availability: envelope.availability,
        measurement_quality: envelope.measurement_quality,
        reasons: MetricDecisionReasonSummaryV1::try_from_codes(&envelope.reason_codes)?,
    };
    field.validate()?;
    Ok(field)
}

fn presence_aware_field_value<T: Clone>(
    value: &CanonicalNullableV1<T>,
    envelope: &CanonicalMetricEnvelopeV1,
    present_quality: MetricMeasurementQualityV1,
) -> Result<MetricDecisionFieldValueV1<T>, MetricContractProjectionErrorV1> {
    let (availability, measurement_quality) = if value.is_null() {
        (
            super::MetricAvailabilityV1::Unavailable,
            MetricMeasurementQualityV1::NotApplicable,
        )
    } else {
        (super::MetricAvailabilityV1::Available, present_quality)
    };
    let field = MetricDecisionFieldValueV1 {
        value: value.clone(),
        availability,
        measurement_quality,
        reasons: MetricDecisionReasonSummaryV1::try_from_codes(&envelope.reason_codes)?,
    };
    field.validate()?;
    Ok(field)
}

impl FundingDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        effective_config_enum_matches(
            context,
            MetricEffectiveConfigKeyV1::FscLegacyFormula,
            "one_minus_distinct_known_sources_over_known_source_samples",
            "legacy FSC formula semantics",
        )?;
        effective_config_enum_matches(
            context,
            MetricEffectiveConfigKeyV1::FscFundingStreamUnavailableBehavior,
            "legacy_null_and_v2_unavailable",
            "FSC unavailable-stream semantics",
        )?;
        let minimum_known_samples = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
            "legacy FSC minimum must be a wide unsigned value",
        )?;
        if minimum_known_samples == 0 {
            return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                key: MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
                invariant: "legacy FSC minimum must keep the denominator non-zero",
            });
        }
        if !nullable_f64_bits_equal(&self.legacy_source.value, &self.legacy_v1.value) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "legacy FSC value parity",
            ));
        }
        if self.distinct_known_source_count > self.known_source_sample_count {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "legacy FSC source counts",
            ));
        }
        if u64::from(self.known_source_sample_count) < minimum_known_samples {
            if !self.legacy_source.value.is_null()
                || !self.legacy_v1.value.is_null()
                || self.legacy_source.envelope.availability
                    == super::MetricAvailabilityV1::Available
                || self.legacy_v1.envelope.availability == super::MetricAvailabilityV1::Available
            {
                return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                    key: MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
                    invariant: "legacy FSC below configured minimum must be null and non-available",
                });
            }
        } else {
            let expected = 1.0
                - self.distinct_known_source_count as f64 / self.known_source_sample_count as f64;
            match &self.legacy_source.value {
                CanonicalNullableV1::Value(value)
                    if value.is_finite() && value.to_bits() == expected.to_bits() => {}
                _ => {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "legacy FSC formula parity",
                    ));
                }
            }
        }
        validate_optional_unit_ratio(&self.legacy_source.value, "legacy FSC ratio range")?;
        let known_coverage = validate_optional_unit_ratio(
            &self.known_coverage.value,
            "FSC v2 known coverage range",
        )?;
        let non_neutral_coverage = validate_optional_unit_ratio(
            &self.non_neutral_known_coverage.value,
            "FSC v2 non-neutral coverage range",
        )?;
        let minimum_total_buyers = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FscMinTotalBuyers,
            "FSC minimum total buyers must be a wide unsigned value",
        )?;
        let minimum_known_coverage = effective_config_ratio(
            context,
            MetricEffectiveConfigKeyV1::FscMinKnownCoverage,
            "FSC minimum known coverage must be a ratio",
        )?;
        let minimum_non_neutral_known_coverage = effective_config_ratio(
            context,
            MetricEffectiveConfigKeyV1::FscMinNonNeutralKnownCoverage,
            "FSC minimum non-neutral known coverage must be a ratio",
        )?;
        if self.known_buyer_count > self.total_buyer_count {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC v2 buyer counts",
            ));
        }
        if self.total_buyer_count == 0
            && (self.known_buyer_count != 0
                || known_coverage.is_some()
                || non_neutral_coverage.is_some()
                || !self.fsc_v2.value.is_null())
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "zero-total FSC v2 evidence must remain fail-closed",
            ));
        }
        match &self.fsc_v2.value {
            CanonicalNullableV1::Value(FscEvidenceStatus::Clean)
            | CanonicalNullableV1::Value(FscEvidenceStatus::Degraded) => {
                let (Some(known_coverage), Some(non_neutral_coverage)) =
                    (known_coverage, non_neutral_coverage)
                else {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "available FSC v2 status requires coverage",
                    ));
                };
                if self.total_buyer_count == 0 {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "available FSC v2 status requires buyers",
                    ));
                }
                let expected_known = self.known_buyer_count as f64 / self.total_buyer_count as f64;
                if known_coverage.to_bits() != expected_known.to_bits() {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "FSC v2 known count/coverage parity",
                    ));
                }
                if self.fsc_v2.value == CanonicalNullableV1::Value(FscEvidenceStatus::Clean)
                    && (u64::from(self.total_buyer_count) < minimum_total_buyers
                        || known_coverage < minimum_known_coverage
                        || non_neutral_coverage < minimum_non_neutral_known_coverage)
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "clean FSC v2 status is below effective-config minimum",
                    ));
                }
            }
            CanonicalNullableV1::Value(FscEvidenceStatus::Unavailable) => {
                return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                    "unavailable FSC v2 must use null surface value",
                ));
            }
            CanonicalNullableV1::Null => {
                if known_coverage.is_some()
                    || non_neutral_coverage.is_some()
                    || self.known_buyer_count != 0
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "unavailable FSC v2 cannot expose known counts or measured coverage",
                    ));
                }
            }
        }
        if self.fsc_v2.envelope.policy_actionable
            || self.fsc_v2.envelope.authority_class != MetricAuthorityClass::EvidenceOnly
            || self.fsc_v2.envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC v2 must remain evidence-only",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &FundingSourceContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &FundingSourceContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        if evidence.legacy_source.distinct_known_source_count
            != evidence.legacy_v1.distinct_known_source_count
            || evidence.legacy_source.known_source_sample_count
                != evidence.legacy_v1.known_source_sample_count
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "legacy FSC count parity",
            ));
        }
        if !nullable_f64_bits_equal(&evidence.legacy_source.ratio, &evidence.legacy_v1.ratio) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "legacy FSC value parity",
            ));
        }
        let fsc_v2_value = match evidence.v2_status {
            FscEvidenceStatus::Clean | FscEvidenceStatus::Degraded => {
                CanonicalNullableV1::Value(evidence.v2_status)
            }
            FscEvidenceStatus::Unavailable => CanonicalNullableV1::Null,
        };
        let projection = Self {
            legacy_source: surface_value(
                &evidence.legacy_source.envelope,
                MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
                &evidence.legacy_source.ratio,
                MetricContractProducerIdV1::FundingSourceIndex,
                context,
            )?,
            legacy_v1: surface_value(
                &evidence.legacy_v1.envelope,
                MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1,
                &evidence.legacy_v1.ratio,
                MetricContractProducerIdV1::FundingSourceIndex,
                context,
            )?,
            distinct_known_source_count: evidence.legacy_source.distinct_known_source_count,
            known_source_sample_count: evidence.legacy_source.known_source_sample_count,
            fsc_v2: surface_value(
                &evidence.v2_envelope,
                MetricSurfaceId::FundingSourceV2ReadinessEvidence,
                &fsc_v2_value,
                MetricContractProducerIdV1::FundingSourceIndex,
                context,
            )?,
            known_coverage: field_value_from_envelope(
                &evidence.known_coverage,
                &evidence.v2_envelope,
            )?,
            non_neutral_known_coverage: field_value_from_envelope(
                &evidence.non_neutral_known_coverage,
                &evidence.v2_envelope,
            )?,
            known_buyer_count: evidence.known_buyer_count,
            total_buyer_count: evidence.total_buyer_count,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl FscStatusDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        funding: &FundingDecisionProjectionV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        effective_config_enum_matches(
            context,
            MetricEffectiveConfigKeyV1::FscLegacyStatusMapping,
            "legacy_scalar_presence_compatibility",
            "FSC legacy status mapping",
        )?;
        effective_config_enum_matches(
            context,
            MetricEffectiveConfigKeyV1::FscV2StatusMapping,
            "decision_time_status_coverage_lane_health",
            "FSC v2 status mapping",
        )?;
        let legacy_value_present = !funding.legacy_source.value.is_null();
        if self.legacy_scalar_present != legacy_value_present
            || self.compatibility_status.value
                != CanonicalNullableV1::Value(self.legacy_feature_status)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC compatibility/legacy presence parity",
            ));
        }
        if (self.legacy_scalar_present && self.legacy_feature_status != EvidenceStatus::Clean)
            || (!self.legacy_scalar_present && self.legacy_feature_status == EvidenceStatus::Clean)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC legacy status/presence parity",
            ));
        }
        if self.compatibility_status.envelope.policy_actionable
            || self.compatibility_status.envelope.authority_class
                != MetricAuthorityClass::Compatibility
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC compatibility status cannot grant v2 authority",
            ));
        }

        let status_coverage =
            validate_optional_unit_ratio(&self.fsc_v2_coverage, "FSC status coverage range")?;
        let funding_coverage = validate_optional_unit_ratio(
            &funding.known_coverage.value,
            "FSC funding coverage range",
        )?;
        match &self.fsc_v2_status {
            CanonicalNullableV1::Value(FscEvidenceStatus::Clean) => {
                if funding.fsc_v2.value != CanonicalNullableV1::Value(FscEvidenceStatus::Clean)
                    || funding.fsc_v2.envelope.measurement_quality
                        != MetricMeasurementQualityV1::Measured
                    || status_coverage.is_none()
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "clean FSC v2 status/readiness parity",
                    ));
                }
            }
            CanonicalNullableV1::Value(FscEvidenceStatus::Degraded) => {
                if funding.fsc_v2.value != CanonicalNullableV1::Value(FscEvidenceStatus::Degraded)
                    || funding.fsc_v2.envelope.measurement_quality
                        != MetricMeasurementQualityV1::Degraded
                    || status_coverage.is_none()
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "degraded FSC v2 status/readiness parity",
                    ));
                }
            }
            CanonicalNullableV1::Value(FscEvidenceStatus::Unavailable)
            | CanonicalNullableV1::Null => {
                if !funding.fsc_v2.value.is_null() || status_coverage.is_some() {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "unavailable FSC v2 cannot appear measured",
                    ));
                }
            }
        }
        if status_coverage.map(f64::to_bits) != funding_coverage.map(f64::to_bits) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "FSC v2 status/funding coverage parity",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &FscStatusEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &FscStatusEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        Ok(Self {
            compatibility_status: surface_value(
                &evidence.envelope,
                MetricSurfaceId::MaterializedFscStatusCompatibility,
                &CanonicalNullableV1::Value(evidence.legacy_feature_status),
                MetricContractProducerIdV1::MaterializedFscStatusAdapter,
                context,
            )?,
            legacy_scalar_present: evidence.legacy_scalar_present,
            legacy_feature_status: evidence.legacy_feature_status,
            fsc_v2_status: evidence.fsc_v2_status.clone(),
            fsc_v2_coverage: evidence.fsc_v2_coverage.clone(),
        })
    }
}

impl FlipDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::FlipLegacyWindowSemantics,
                "first_buy_slot_to_last_sell_slot_gap",
                "legacy flip semantics",
            ),
            (
                MetricEffectiveConfigKeyV1::FlipCandidateAnchorRule,
                "first_eligible_buy",
                "flip anchor rule",
            ),
            (
                MetricEffectiveConfigKeyV1::FlipCandidateOrderPolicy,
                "stable_tx_key",
                "flip order policy",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        effective_config_boolean_matches(
            context,
            MetricEffectiveConfigKeyV1::FlipCandidateSuccessRequired,
            true,
            "flip success requirement",
        )?;
        let configured_window = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FlipCandidateWallClockWindowMs,
            "flip wall-clock window",
        )?;
        let configured_slot_gap = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::FlipCandidateMaxSlotGap,
            "flip slot-gap window",
        )?;
        let configured_dump_ratio = effective_config_ratio(
            context,
            MetricEffectiveConfigKeyV1::FlipCandidateDumpRatio,
            "flip dump ratio",
        )?;
        if u64::from(self.wall_clock_window_ms) != configured_window
            || u64::from(self.max_slot_gap) != configured_slot_gap
            || self.dump_ratio.to_bits() != configured_dump_ratio.to_bits()
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "flip aggregate/config parity",
            ));
        }
        validate_optional_unit_ratio(&self.legacy_slot_gap_ratio.value, "legacy flip ratio range")?;
        let ratio =
            validate_optional_unit_ratio(&self.hybrid_v2_ratio.value, "hybrid flip ratio range")?;
        if self.flipper_count > self.eligible_buyer_count {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "flip aggregate count ordering",
            ));
        }
        match (self.eligible_buyer_count, ratio) {
            (0, None) => {}
            (0, Some(_)) | (_, None) => {
                return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                    "flip denominator/value presence",
                ));
            }
            (denominator, Some(value)) => {
                let expected = f64::from(self.flipper_count) / f64::from(denominator);
                if value.to_bits() != expected.to_bits() {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "flip aggregate ratio parity",
                    ));
                }
            }
        }
        if self.hybrid_v2_ratio.envelope.policy_actionable
            || self.hybrid_v2_ratio.envelope.authority_class != MetricAuthorityClass::EvidenceOnly
            || self.hybrid_v2_ratio.envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "flip v2 must remain evidence-only",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &FlipRatioContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &FlipRatioContractEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let projection = Self {
            legacy_slot_gap_ratio: surface_value(
                &evidence.legacy_envelope,
                MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
                &evidence.legacy_slot_gap_ratio,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
                context,
            )?,
            hybrid_v2_ratio: surface_value(
                &evidence.hybrid_v2.envelope,
                MetricSurfaceId::FlipRatioHybridEvidenceV2,
                &evidence.hybrid_v2.ratio,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
                context,
            )?,
            eligible_buyer_count: evidence.hybrid_v2.eligible_buyer_count,
            flipper_count: evidence.hybrid_v2.flipper_count,
            wall_clock_window_ms: evidence.hybrid_v2.wall_clock_window_ms,
            max_slot_gap: evidence.hybrid_v2.max_slot_gap,
            dump_ratio: evidence.hybrid_v2.dump_ratio,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

const MANIPULATION_COMPACT_FIELDS_V2: [ManipulationNumericFieldIdV2; 7] = [
    ManipulationNumericFieldIdV2::SameMsTxRatio,
    ManipulationNumericFieldIdV2::BundleSuspicionRatio,
    ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
    ManipulationNumericFieldIdV2::Hhi,
    ManipulationNumericFieldIdV2::MaxTxPerSigner,
    ManipulationNumericFieldIdV2::DevVolumeRatio,
    ManipulationNumericFieldIdV2::ContradictionScore,
];

fn manipulation_field_value(
    field: &ManipulationNumericFieldEvidenceV2,
) -> Result<MetricDecisionFieldValueV1<f64>, MetricContractProjectionErrorV1> {
    let value = MetricDecisionFieldValueV1 {
        value: field.value.clone(),
        availability: field.availability,
        measurement_quality: field.measurement_quality,
        reasons: MetricDecisionReasonSummaryV1::try_from_codes(&field.reason_codes)?,
    };
    value.validate()?;
    Ok(value)
}

impl ManipulationDecisionProjectionV1 {
    fn field(&self, id: ManipulationNumericFieldIdV2) -> &MetricDecisionFieldValueV1<f64> {
        match id {
            ManipulationNumericFieldIdV2::SameMsTxRatio => &self.same_ms_tx_ratio,
            ManipulationNumericFieldIdV2::BundleSuspicionRatio => &self.bundle_suspicion_ratio,
            ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => &self.top3_signer_volume_ratio,
            ManipulationNumericFieldIdV2::Hhi => &self.hhi,
            ManipulationNumericFieldIdV2::MaxTxPerSigner => &self.max_tx_per_signer,
            ManipulationNumericFieldIdV2::DevVolumeRatio => &self.dev_volume_ratio,
            ManipulationNumericFieldIdV2::ContradictionScore => &self.contradiction_score,
        }
    }

    fn threshold(
        context: &MetricDecisionProjectionBuildContextV1<'_>,
        id: ManipulationNumericFieldIdV2,
    ) -> Result<Option<f64>, MetricContractProjectionErrorV1> {
        let threshold = match id {
            ManipulationNumericFieldIdV2::SameMsTxRatio => effective_config_ratio(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighSameMsThreshold,
                "manipulation same-ms threshold",
            )?,
            ManipulationNumericFieldIdV2::BundleSuspicionRatio => effective_config_ratio(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighBundleThreshold,
                "manipulation bundle threshold",
            )?,
            ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => effective_config_ratio(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighTop3Threshold,
                "manipulation top3 threshold",
            )?,
            ManipulationNumericFieldIdV2::Hhi => effective_config_ratio(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighHhiThreshold,
                "manipulation HHI threshold",
            )?,
            ManipulationNumericFieldIdV2::MaxTxPerSigner => {
                let value = effective_config_wide_unsigned(
                    context,
                    MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold,
                    "manipulation signer-count threshold",
                )?;
                if value > (1_u64 << 53) {
                    return Err(MetricContractProjectionErrorV1::EffectiveConfigParity {
                        key: MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold,
                        invariant: "manipulation signer threshold is not exactly representable",
                    });
                }
                value as f64
            }
            ManipulationNumericFieldIdV2::DevVolumeRatio => effective_config_ratio(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighDevConcentrationThreshold,
                "manipulation dev threshold",
            )?,
            ManipulationNumericFieldIdV2::ContradictionScore => return Ok(None),
        };
        Ok(Some(threshold))
    }

    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::ManipulationNumericPresenceVersion,
                "v2_field_presence",
                "manipulation numeric presence version",
            ),
            (
                MetricEffectiveConfigKeyV1::ManipulationBooleanPresenceVersion,
                "v2_field_presence",
                "manipulation boolean presence version",
            ),
            (
                MetricEffectiveConfigKeyV1::ManipulationHighFlagDerivationVersion,
                "policy_stage_v1",
                "manipulation derivation version",
            ),
            (
                MetricEffectiveConfigKeyV1::ManipulationMissingRawBehavior,
                "unavailable_not_false",
                "manipulation missing raw behavior",
            ),
            (
                MetricEffectiveConfigKeyV1::ManipulationMeasuredFieldsMaskVersion,
                "v1_u16",
                "manipulation measured mask version",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        let mut expected_measured = 0_u16;
        let mut expected_evaluable = 0_u16;
        let mut expected_true = 0_u16;
        for id in MANIPULATION_COMPACT_FIELDS_V2 {
            let field = self.field(id);
            field.validate()?;
            if let CanonicalNullableV1::Value(value) = field.value {
                if !matches!(
                    field.measurement_quality,
                    MetricMeasurementQualityV1::Measured | MetricMeasurementQualityV1::Degraded
                ) {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "manipulation compact measured-field quality",
                    ));
                }
                if self.numeric_v2_envelope.measurement_quality
                    == MetricMeasurementQualityV1::Degraded
                    && field.measurement_quality != MetricMeasurementQualityV1::Degraded
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "manipulation group quality upper bound",
                    ));
                }
                if !value.is_finite()
                    || (id != ManipulationNumericFieldIdV2::MaxTxPerSigner
                        && !(0.0..=1.0).contains(&value))
                    || (id == ManipulationNumericFieldIdV2::MaxTxPerSigner && value < 0.0)
                {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "manipulation compact field range",
                    ));
                }
                expected_measured |= id.measured_mask_bit();
                if let Some(threshold) = Self::threshold(context, id)? {
                    expected_evaluable |= id.measured_mask_bit();
                    if value > threshold {
                        expected_true |= id.measured_mask_bit();
                    }
                }
            }
        }
        if self.measured_fields_mask != expected_measured
            || self.derived_high_evaluable_mask != expected_evaluable
            || self.derived_high_true_mask != expected_true
            || self.legacy_high_true_mask & !self.legacy_high_recorded_mask != 0
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "manipulation field/mask parity",
            ));
        }
        if self.numeric_v2_envelope.availability != MetricAvailabilityV1::Available
            && expected_measured != 0
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "manipulation unavailable group cannot contain measured fields",
            ));
        }
        if self.numeric_v2_envelope.policy_actionable
            || self.numeric_v2_envelope.authority_class != MetricAuthorityClass::EquivalentCutover
            || self.numeric_v2_envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "manipulation v2 must remain evidence-only",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &ManipulationNumericEvidenceV2,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &ManipulationNumericEvidenceV2,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let find = |id| {
            evidence
                .fields
                .iter()
                .find(|field| field.field_id == id)
                .ok_or(MetricContractProjectionErrorV1::FamilyInvariant(
                    "missing manipulation field",
                ))
        };
        let mut legacy_recorded = 0_u16;
        let mut legacy_true = 0_u16;
        for flag in &evidence.legacy_high_flags {
            if flag.field_recorded {
                legacy_recorded |= flag.field_id.measured_mask_bit();
                if flag.value {
                    legacy_true |= flag.field_id.measured_mask_bit();
                }
            }
        }
        let mut derived_evaluable = 0_u16;
        let mut derived_true = 0_u16;
        for flag in &evidence.derived_high_flags {
            if flag.config_hash
                != context
                    .effective_config
                    .metric_contract_effective_config_hash
                || flag.comparator != ManipulationComparatorV1::GreaterThan
                || flag.policy_stage != super::MANIPULATION_DERIVED_POLICY_STAGE_V1
                || flag.policy_version != super::MANIPULATION_DERIVED_POLICY_VERSION_V1
            {
                return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                    "manipulation derived provenance",
                ));
            }
            let expected_threshold = Self::threshold(context, flag.field_id)?;
            if expected_threshold.map(f64::to_bits)
                != match flag.threshold {
                    CanonicalNullableV1::Value(value) => Some(value.to_bits()),
                    CanonicalNullableV1::Null => None,
                }
            {
                return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                    "manipulation derived threshold/config parity",
                ));
            }
            if let CanonicalNullableV1::Value(value) = flag.derived_value {
                derived_evaluable |= flag.field_id.measured_mask_bit();
                if value {
                    derived_true |= flag.field_id.measured_mask_bit();
                }
            }
        }
        let projection = Self {
            legacy_numeric_envelope: compact_envelope(
                &evidence.legacy_numeric_envelope,
                MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
                context,
            )?,
            numeric_v2_envelope: compact_envelope(
                &evidence.numeric_v2_envelope,
                MetricSurfaceId::ManipulationNumericEvidenceV2,
                context,
            )?,
            measured_fields_mask: evidence.measured_fields_mask,
            same_ms_tx_ratio: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::SameMsTxRatio,
            )?)?,
            bundle_suspicion_ratio: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::BundleSuspicionRatio,
            )?)?,
            top3_signer_volume_ratio: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
            )?)?,
            hhi: manipulation_field_value(find(ManipulationNumericFieldIdV2::Hhi)?)?,
            max_tx_per_signer: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::MaxTxPerSigner,
            )?)?,
            dev_volume_ratio: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::DevVolumeRatio,
            )?)?,
            contradiction_score: manipulation_field_value(find(
                ManipulationNumericFieldIdV2::ContradictionScore,
            )?)?,
            legacy_high_recorded_mask: legacy_recorded,
            legacy_high_true_mask: legacy_true,
            derived_high_evaluable_mask: derived_evaluable,
            derived_high_true_mask: derived_true,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl ReserveVelocityDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::ReserveVelocitySourceClock,
                "receive_time",
                "reserve source clock",
            ),
            (
                MetricEffectiveConfigKeyV1::ReserveVelocityFirstUpdateBehavior,
                "typed_first_update",
                "reserve first-update behavior",
            ),
            (
                MetricEffectiveConfigKeyV1::ReserveVelocityZeroDeltaTimeBehavior,
                "unavailable",
                "reserve zero-delta behavior",
            ),
            (
                MetricEffectiveConfigKeyV1::ReserveVelocityFallbackBehavior,
                "typed_fallback_not_zero",
                "reserve fallback behavior",
            ),
            (
                MetricEffectiveConfigKeyV1::ReserveVelocityUnit,
                "sol_per_second",
                "reserve velocity unit",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        if self.source_clock != ReserveVelocitySourceClockV1::ReceiveTime {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "reserve source clock",
            ));
        }
        if nullable_f64_value(&self.legacy_velocity.value).is_some_and(|value| !value.is_finite()) {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "legacy reserve velocity finite",
            ));
        }
        let valid = match self.status {
            ReserveVelocityStatusV1::Measured => {
                let (
                    CanonicalNullableV1::Value(previous),
                    CanonicalNullableV1::Value(current),
                    CanonicalNullableV1::Value(interval_ms),
                    CanonicalNullableV1::Value(velocity),
                ) = (
                    &self.previous_real_sol_reserves_lamports.value,
                    &self.current_real_sol_reserves_lamports.value,
                    &self.interval_ms.value,
                    &self.velocity_v1.value,
                )
                else {
                    return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                        "measured reserve velocity presence",
                    ));
                };
                if *interval_ms == 0 || self.accepted_update_count < 2 || !velocity.is_finite() {
                    false
                } else {
                    let delta_sol =
                        (current.get() as f64 - previous.get() as f64) / 1_000_000_000.0;
                    let expected = delta_sol / (f64::from(*interval_ms) / 1_000.0);
                    velocity.to_bits() == expected.to_bits()
                        && nullable_f64_value(&self.legacy_velocity.value)
                            .is_some_and(|legacy| legacy.to_bits() == velocity.to_bits())
                }
            }
            ReserveVelocityStatusV1::FirstUpdate => {
                self.accepted_update_count == 1
                    && self.previous_real_sol_reserves_lamports.value.is_null()
                    && !self.current_real_sol_reserves_lamports.value.is_null()
                    && self.velocity_v1.value.is_null()
                    && self.interval_ms.value.is_null()
            }
            ReserveVelocityStatusV1::ZeroDeltaTime => {
                self.accepted_update_count >= 2
                    && self.velocity_v1.value.is_null()
                    && matches!(self.interval_ms.value, CanonicalNullableV1::Value(0))
            }
            ReserveVelocityStatusV1::BootstrapFallback => {
                self.accepted_update_count == 0
                    && self.velocity_v1.value.is_null()
                    && self.interval_ms.value.is_null()
            }
            ReserveVelocityStatusV1::Unavailable => self.velocity_v1.value.is_null(),
        };
        if !valid {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "reserve velocity status/count/value parity",
            ));
        }
        if self.velocity_v1.envelope.policy_actionable
            || self.velocity_v1.envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "reserve velocity v1 must remain non-policy",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &super::ReserveVelocityEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &super::ReserveVelocityEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let legacy = CanonicalNullableV1::Value(evidence.legacy_velocity_sol_per_sec);
        let projection = Self {
            legacy_velocity: surface_value(
                &evidence.legacy_envelope,
                MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
                &legacy,
                MetricContractProducerIdV1::AccountStateCore,
                context,
            )?,
            velocity_v1: surface_value(
                &evidence.v1_envelope,
                MetricSurfaceId::ReserveVelocityEvidenceV1,
                &evidence.velocity_sol_per_sec,
                MetricContractProducerIdV1::AccountStateCore,
                context,
            )?,
            previous_real_sol_reserves_lamports: presence_aware_field_value(
                &evidence.previous_real_sol_reserves_lamports,
                &evidence.v1_envelope,
                MetricMeasurementQualityV1::Measured,
            )?,
            current_real_sol_reserves_lamports: presence_aware_field_value(
                &evidence.current_real_sol_reserves_lamports,
                &evidence.v1_envelope,
                MetricMeasurementQualityV1::Measured,
            )?,
            interval_ms: presence_aware_field_value(
                &evidence.interval_ms,
                &evidence.v1_envelope,
                if evidence.status == ReserveVelocityStatusV1::Measured {
                    MetricMeasurementQualityV1::Measured
                } else {
                    MetricMeasurementQualityV1::Degraded
                },
            )?,
            accepted_update_count: evidence.accepted_update_count,
            source_clock: evidence.source_clock,
            status: evidence.status,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl RecentBuySellDecisionProjectionV1 {
    pub fn validate_semantics(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        effective_config_boolean_matches(
            context,
            MetricEffectiveConfigKeyV1::RecentBuySellSuccessfulOnly,
            true,
            "recent buy/sell population",
        )?;
        for (key, expected, invariant) in [
            (
                MetricEffectiveConfigKeyV1::RecentBuySellBoundaryPolicy,
                "inclusive_start_and_end",
                "recent boundary policy",
            ),
            (
                MetricEffectiveConfigKeyV1::RecentBuySellSameMsNumeratorRule,
                "sum_timestamp_multiplicity_minus_one",
                "recent same-ms rule",
            ),
            (
                MetricEffectiveConfigKeyV1::RecentBuySellLegacyRatioRule,
                "sell_zero_returns_buy_count",
                "recent legacy ratio rule",
            ),
            (
                MetricEffectiveConfigKeyV1::RecentBuySellUnboundedRatioRule,
                "buy_count_over_sell_count_or_null",
                "recent unbounded ratio rule",
            ),
            (
                MetricEffectiveConfigKeyV1::RecentBuySellBoundedShareRule,
                "buy_count_over_transaction_count",
                "recent bounded share rule",
            ),
            (
                MetricEffectiveConfigKeyV1::RecentBuySellZeroDenominatorBehavior,
                "null_unavailable",
                "recent zero-denominator rule",
            ),
        ] {
            effective_config_enum_matches(context, key, expected, invariant)?;
        }
        let window = effective_config_wide_unsigned(
            context,
            MetricEffectiveConfigKeyV1::RecentBuySellWindowMs,
            "recent buy/sell window",
        )?;
        if u64::from(self.window_ms) != window
            || self.buy_count.checked_add(self.sell_count) != Some(self.transaction_count)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "recent window/count parity",
            ));
        }
        let expected_legacy = if self.transaction_count == 0 {
            None
        } else if self.sell_count == 0 {
            Some(f64::from(self.buy_count))
        } else {
            Some(f64::from(self.buy_count) / f64::from(self.sell_count))
        };
        if nullable_f64_value(&self.legacy_scalar.value).map(f64::to_bits)
            != expected_legacy.map(f64::to_bits)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "recent legacy scalar reconstruction",
            ));
        }
        let expected_unbounded =
            (self.sell_count > 0).then(|| f64::from(self.buy_count) / f64::from(self.sell_count));
        if nullable_f64_value(&self.buy_to_sell_ratio.value).map(f64::to_bits)
            != expected_unbounded.map(f64::to_bits)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "recent unbounded ratio reconstruction",
            ));
        }
        let expected_share = (self.transaction_count > 0)
            .then(|| f64::from(self.buy_count) / f64::from(self.transaction_count));
        if nullable_f64_value(&self.buy_share.value).map(f64::to_bits)
            != expected_share.map(f64::to_bits)
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "recent bounded share reconstruction",
            ));
        }
        if self.v1_envelope.policy_actionable
            || self.v1_envelope.authority_class != MetricAuthorityClass::LoggingOnly
            || self.v1_envelope.rollout_role != MetricRolloutRoleV1::NonPolicy
        {
            return Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "recent buy/sell must remain logging-only non-policy",
            ));
        }
        Ok(())
    }

    pub fn try_from_evidence(
        evidence: &RecentBuySellEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        context.validate()?;
        Self::try_from_evidence_with_validated_context(evidence, context)
    }

    fn try_from_evidence_with_validated_context(
        evidence: &RecentBuySellEvidenceV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let projection = Self {
            legacy_scalar: surface_value(
                &evidence.legacy_envelope,
                MetricSurfaceId::RceBuySellRatioRecentLegacy,
                &evidence.legacy_buy_sell_scalar,
                MetricContractProducerIdV1::RecentBuySellWindowProducer,
                context,
            )?,
            v1_envelope: compact_envelope(
                &evidence.v1_envelope,
                MetricSurfaceId::RecentBuySellEvidenceV1,
                context,
            )?,
            window_ms: evidence.window_ms,
            buy_count: evidence.buy_count,
            sell_count: evidence.sell_count,
            transaction_count: evidence.transaction_count,
            buy_to_sell_ratio: presence_aware_field_value(
                &evidence.buy_to_sell_ratio,
                &evidence.v1_envelope,
                MetricMeasurementQualityV1::Measured,
            )?,
            buy_share: presence_aware_field_value(
                &evidence.buy_share,
                &evidence.v1_envelope,
                MetricMeasurementQualityV1::Measured,
            )?,
        };
        projection.validate_semantics(context)?;
        Ok(projection)
    }
}

impl MetricContractDecisionEvidenceProjectionV1 {
    pub fn try_from_evidence(
        evidence: &MetricContractsEvidenceSetV1,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<Self, MetricContractProjectionErrorV1> {
        let profile_hash = context.validate_and_profile_hash()?;
        evidence.validate_semantics()?;
        let projection = Self {
            schema_version: METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1,
            rollout_mode: context.rollout_mode,
            profile_id: context.profile.payload().profile_id.clone(),
            profile_hash: profile_hash.clone(),
            metric_contract_effective_config_hash: context
                .effective_config
                .metric_contract_effective_config_hash
                .clone(),
            fee_topology_diversity_index:
                FtdiDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.fee_topology_diversity_index,
                    context,
                )?,
            dev_buy: DevBuyDecisionProjectionV1::try_from_evidence_with_validated_context(
                &evidence.dev_buy,
                context,
            )?,
            same_ms_tx_ratio:
                TxTimingDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.same_ms_tx_ratio,
                    context,
                )?,
            top3_signer_volume_ratio:
                Top3DecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.top3_signer_volume_ratio,
                    context,
                )?,
            flip_ratio: FlipDecisionProjectionV1::try_from_evidence_with_validated_context(
                &evidence.flip_ratio,
                context,
            )?,
            funding_source_concentration:
                FundingDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.funding_source_concentration,
                    context,
                )?,
            fsc_evidence_status:
                FscStatusDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.fsc_evidence_status,
                    context,
                )?,
            manipulation_contradiction:
                ManipulationDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.manipulation_contradiction,
                    context,
                )?,
            reserve_velocity:
                ReserveVelocityDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.reserve_velocity,
                    context,
                )?,
            recent_buy_sell:
                RecentBuySellDecisionProjectionV1::try_from_evidence_with_validated_context(
                    &evidence.recent_buy_sell,
                    context,
                )?,
        };
        projection.validate_context_with_validated_context(context, &profile_hash)?;
        Ok(projection)
    }
}
