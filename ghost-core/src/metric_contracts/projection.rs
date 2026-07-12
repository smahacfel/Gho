use super::{
    CanonicalHashErrorV1, CanonicalHashV1, CanonicalMetricEnvelopeV1, CanonicalNullableV1,
    CanonicalU64StringV1, DevBuyContractEvidenceV1, DevBuySelectionModeV1, FscStatusEvidenceV1,
    FtdiEvidenceV1, FundingSourceContractEvidenceV1, ManipulationComparatorV1,
    MetricAuthorityClass, MetricContractEffectiveConfigErrorV1, MetricContractId,
    MetricContractProfileIdV1, MetricContractProfileV1, MetricContractRolloutMode,
    MetricEvidenceEnvelopeErrorV1, MetricEvidenceReasonV1, MetricMeasurementQualityV1,
    MetricRolloutRoleV1, MetricSurfaceId, RecentBuySellEvidenceV1, ReserveVelocitySourceClockV1,
    ReserveVelocityStatusV1, ResolvedMetricContractEffectiveConfigV1, Top3SignerVolumeEvidenceV1,
    TxTimingEvidenceV1, TxTimingPopulationV1,
};
use crate::checkpoint::EvidenceStatus;
use crate::tx_intelligence::types::FscEvidenceStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1: u16 = 1;
pub const METRIC_DECISION_MAX_REASON_CODES_PER_VALUE_V1: usize = 8;
pub const METRIC_CONTRACT_PRODUCER_SCHEMA_VERSION_V1: u16 = 1;

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

// PR2B projection schema slots are defined here so the root is closed. PR2A
// intentionally provides no producers or family builders for these types.
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
    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, MetricContractProjectionErrorV1> {
        if self.schema_version != METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1 {
            return Err(
                MetricContractProjectionErrorV1::UnsupportedProjectionSchema(self.schema_version),
            );
        }
        CanonicalHashV1::digest(self).map_err(MetricContractProjectionErrorV1::Hash)
    }

    pub fn validate_context(
        &self,
        context: &MetricDecisionProjectionBuildContextV1<'_>,
    ) -> Result<(), MetricContractProjectionErrorV1> {
        context.validate()?;
        if self.schema_version != METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1 {
            return Err(
                MetricContractProjectionErrorV1::UnsupportedProjectionSchema(self.schema_version),
            );
        }
        if self.rollout_mode != context.rollout_mode
            || self.profile_id != context.profile.payload().profile_id
            || self.profile_hash != context.profile.canonical_hash()?
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
        Ok(())
    }
}

impl<T> MetricDecisionFieldValueV1<T> {
    pub fn validate(&self) -> Result<(), MetricContractProjectionErrorV1> {
        self.reasons.validate()
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
        validate_compact_envelope(&self.envelope, expected_surface, context)
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
        self.effective_config.validate_hash()?;
        let payload = &self.effective_config.payload;
        if payload.rollout_mode != self.rollout_mode
            || payload.profile_id != self.profile.payload().profile_id
            || payload.profile_hash != self.profile.canonical_hash()?
        {
            return Err(MetricContractProjectionErrorV1::ProjectionContextMismatch);
        }
        if self.source_cutoff.decision_timestamp_ms.get() == 0 {
            return Err(MetricContractProjectionErrorV1::MissingSourceCutoff);
        }
        Ok(())
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
    #[error("projection context does not match profile/effective config")]
    ProjectionContextMismatch,
    #[error("unsupported metric-contract decision projection schema {0}")]
    UnsupportedProjectionSchema(u16),
    #[error("projection family invariant failed: {0}")]
    FamilyInvariant(&'static str),
}

fn compact_envelope(
    source: &CanonicalMetricEnvelopeV1,
    expected_surface: MetricSurfaceId,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<MetricDecisionEnvelopeV1, MetricContractProjectionErrorV1> {
    context.validate()?;
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
    context.validate()?;
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
    Ok(MetricDecisionSurfaceValueV1 {
        envelope: compact_envelope(source, expected_surface, context)?,
        value: value.clone(),
        producer_id,
        producer_schema_version: METRIC_CONTRACT_PRODUCER_SCHEMA_VERSION_V1,
        source_cutoff: context.source_cutoff.clone(),
    })
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

impl FtdiDecisionProjectionV1 {
    pub fn try_from_evidence(
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
        Ok(Self {
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
        })
    }
}

impl DevBuyDecisionProjectionV1 {
    pub fn try_from_evidence(
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
        Ok(Self {
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
        })
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

impl TxTimingDecisionProjectionV1 {
    pub fn try_from_evidence(
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
        Ok(Self {
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
        })
    }
}

impl Top3DecisionProjectionV1 {
    pub fn try_from_evidence(
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
        Ok(Self {
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
        })
    }
}

fn field_value_from_envelope<T: Clone>(
    value: &CanonicalNullableV1<T>,
    envelope: &CanonicalMetricEnvelopeV1,
) -> Result<MetricDecisionFieldValueV1<T>, MetricContractProjectionErrorV1> {
    Ok(MetricDecisionFieldValueV1 {
        value: value.clone(),
        availability: envelope.availability,
        measurement_quality: envelope.measurement_quality,
        reasons: MetricDecisionReasonSummaryV1::try_from_codes(&envelope.reason_codes)?,
    })
}

impl FundingDecisionProjectionV1 {
    pub fn try_from_evidence(
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
        Ok(Self {
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
                &CanonicalNullableV1::Value(evidence.v2_status),
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
        })
    }
}

impl FscStatusDecisionProjectionV1 {
    pub fn try_from_evidence(
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

// Keep these imports exercised by the closed schema without introducing PR2B
// builders in PR2A.
const _: Option<ManipulationComparatorV1> = None;
const _: Option<RecentBuySellEvidenceV1> = None;
