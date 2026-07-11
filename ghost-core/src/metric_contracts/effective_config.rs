use super::{
    CanonicalHashErrorV1, CanonicalHashV1, CanonicalNullableV1, CanonicalU64StringV1,
    MetricContractFoundationConfigV1, MetricContractId, MetricContractProfileErrorV1,
    MetricContractProfileIdV1, MetricContractRolloutMode, METRIC_CONTRACT_REGISTRY_ID_V1_1,
    METRIC_CONTRACT_SCHEMA_VERSION_V1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Closed vocabulary of every resolved producer/status/comparator setting that
/// belongs to `metric_contract_effective_config_hash` V1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEffectiveConfigKeyV1 {
    FtdiPopulationSuccessfulBuy,
    FtdiFirstSamplePerSigner,
    FtdiMissingSignerBehavior,
    FtdiMissingTopologyBehavior,
    FtdiDiagnosticMinUniqueBuyers,
    FtdiLegacyCleanMinBuyTransactions,
    FtdiCandidateCleanMinUniqueBuyers,
    FtdiDenominatorRule,

    DevTxIntelSuccessEligibility,
    DevTxIntelDustThresholdSol,
    DevTxIntelDedupeKey,
    DevTxIntelDedupeCapacity,
    DevFirstObservedAnchorRule,
    DevPrimarySuccessRequired,
    DevPrimaryDustThresholdSol,
    DevPrimaryDedupeKey,
    DevPrimaryAnchorRule,
    DevMissingCreatorBehavior,

    SameMsExactDeltaMs,
    SameMsLegacyPopulation,
    SameMsLegacyDenominatorRule,
    SameMsClusterUpperBoundExclusiveMs,
    SameMsRecentWindowMs,
    SameMsRecentPopulation,
    SameMsRecentDenominatorRule,

    Top3PreferredField,
    Top3FallbackAlias,
    Top3Scale,
    Top3MismatchBehavior,

    FlipLegacyWindowSemantics,
    FlipCandidateWallClockWindowMs,
    FlipCandidateMaxSlotGap,
    FlipCandidateDumpRatio,
    FlipCandidateAnchorRule,
    FlipCandidateOrderPolicy,
    FlipCandidateSuccessRequired,
    FlipCandidateDustThresholdSol,
    FlipCandidateDedupeKey,
    FlipCandidateDedupeCapacity,
    FlipCandidateEvictionPolicy,
    FlipCandidateMaxWallets,
    FlipCandidateReconnectBehavior,

    FscLegacyFormula,
    FscFundingLookbackWindowMs,
    FscMinAbsStoreLamports,
    FscMinAbsAttributionLamports,
    FscMinRelativeToBuy,
    FscMinAttributionConfidenceBps,
    FscPerRecipientCapacity,
    FscGlobalRecipientCapacity,
    FscWarmupWindowMs,
    FscMinTotalBuyers,
    FscMinKnownNonNeutralBuyers,
    FscMinKnownCoverage,
    FscMinNonNeutralKnownCoverage,
    FscSameSlotOrderingPolicy,
    FscNeutralFunderSetVersion,
    FscNeutralFunderSetHash,
    FscFundingStreamUnavailableBehavior,
    FscLegacyStatusMapping,
    FscV2StatusMapping,

    ManipulationNumericPresenceVersion,
    ManipulationBooleanPresenceVersion,
    ManipulationHighFlagDerivationVersion,
    ManipulationHighSameMsThreshold,
    ManipulationHighBundleThreshold,
    ManipulationHighTop3Threshold,
    ManipulationHighHhiThreshold,
    ManipulationHighSignerCountThreshold,
    ManipulationHighDevConcentrationThreshold,
    ManipulationMissingRawBehavior,
    ManipulationMeasuredFieldsMaskVersion,

    ReserveVelocitySourceClock,
    ReserveVelocityFirstUpdateBehavior,
    ReserveVelocityZeroDeltaTimeBehavior,
    ReserveVelocityFallbackBehavior,
    ReserveVelocityUnit,

    RecentBuySellWindowMs,
    RecentBuySellSuccessfulOnly,
    RecentBuySellBoundaryPolicy,
    RecentBuySellSameMsNumeratorRule,
    RecentBuySellLegacyRatioRule,
    RecentBuySellUnboundedRatioRule,
    RecentBuySellBoundedShareRule,
    RecentBuySellZeroDenominatorBehavior,

    ComparatorNormalizationVersion,
    ComparatorFloatEquivalenceRule,
    ComparatorEquivalenceLaneVersion,
    ComparatorActionabilityMappingVersion,
    ComparatorStatusMappingVersion,
    ComparatorLegacyMissingFieldBehavior,
}

pub const METRIC_EFFECTIVE_CONFIG_KEYS_V1: &[MetricEffectiveConfigKeyV1] = &[
    MetricEffectiveConfigKeyV1::FtdiPopulationSuccessfulBuy,
    MetricEffectiveConfigKeyV1::FtdiFirstSamplePerSigner,
    MetricEffectiveConfigKeyV1::FtdiMissingSignerBehavior,
    MetricEffectiveConfigKeyV1::FtdiMissingTopologyBehavior,
    MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers,
    MetricEffectiveConfigKeyV1::FtdiLegacyCleanMinBuyTransactions,
    MetricEffectiveConfigKeyV1::FtdiCandidateCleanMinUniqueBuyers,
    MetricEffectiveConfigKeyV1::FtdiDenominatorRule,
    MetricEffectiveConfigKeyV1::DevTxIntelSuccessEligibility,
    MetricEffectiveConfigKeyV1::DevTxIntelDustThresholdSol,
    MetricEffectiveConfigKeyV1::DevTxIntelDedupeKey,
    MetricEffectiveConfigKeyV1::DevTxIntelDedupeCapacity,
    MetricEffectiveConfigKeyV1::DevFirstObservedAnchorRule,
    MetricEffectiveConfigKeyV1::DevPrimarySuccessRequired,
    MetricEffectiveConfigKeyV1::DevPrimaryDustThresholdSol,
    MetricEffectiveConfigKeyV1::DevPrimaryDedupeKey,
    MetricEffectiveConfigKeyV1::DevPrimaryAnchorRule,
    MetricEffectiveConfigKeyV1::DevMissingCreatorBehavior,
    MetricEffectiveConfigKeyV1::SameMsExactDeltaMs,
    MetricEffectiveConfigKeyV1::SameMsLegacyPopulation,
    MetricEffectiveConfigKeyV1::SameMsLegacyDenominatorRule,
    MetricEffectiveConfigKeyV1::SameMsClusterUpperBoundExclusiveMs,
    MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
    MetricEffectiveConfigKeyV1::SameMsRecentPopulation,
    MetricEffectiveConfigKeyV1::SameMsRecentDenominatorRule,
    MetricEffectiveConfigKeyV1::Top3PreferredField,
    MetricEffectiveConfigKeyV1::Top3FallbackAlias,
    MetricEffectiveConfigKeyV1::Top3Scale,
    MetricEffectiveConfigKeyV1::Top3MismatchBehavior,
    MetricEffectiveConfigKeyV1::FlipLegacyWindowSemantics,
    MetricEffectiveConfigKeyV1::FlipCandidateWallClockWindowMs,
    MetricEffectiveConfigKeyV1::FlipCandidateMaxSlotGap,
    MetricEffectiveConfigKeyV1::FlipCandidateDumpRatio,
    MetricEffectiveConfigKeyV1::FlipCandidateAnchorRule,
    MetricEffectiveConfigKeyV1::FlipCandidateOrderPolicy,
    MetricEffectiveConfigKeyV1::FlipCandidateSuccessRequired,
    MetricEffectiveConfigKeyV1::FlipCandidateDustThresholdSol,
    MetricEffectiveConfigKeyV1::FlipCandidateDedupeKey,
    MetricEffectiveConfigKeyV1::FlipCandidateDedupeCapacity,
    MetricEffectiveConfigKeyV1::FlipCandidateEvictionPolicy,
    MetricEffectiveConfigKeyV1::FlipCandidateMaxWallets,
    MetricEffectiveConfigKeyV1::FlipCandidateReconnectBehavior,
    MetricEffectiveConfigKeyV1::FscLegacyFormula,
    MetricEffectiveConfigKeyV1::FscFundingLookbackWindowMs,
    MetricEffectiveConfigKeyV1::FscMinAbsStoreLamports,
    MetricEffectiveConfigKeyV1::FscMinAbsAttributionLamports,
    MetricEffectiveConfigKeyV1::FscMinRelativeToBuy,
    MetricEffectiveConfigKeyV1::FscMinAttributionConfidenceBps,
    MetricEffectiveConfigKeyV1::FscPerRecipientCapacity,
    MetricEffectiveConfigKeyV1::FscGlobalRecipientCapacity,
    MetricEffectiveConfigKeyV1::FscWarmupWindowMs,
    MetricEffectiveConfigKeyV1::FscMinTotalBuyers,
    MetricEffectiveConfigKeyV1::FscMinKnownNonNeutralBuyers,
    MetricEffectiveConfigKeyV1::FscMinKnownCoverage,
    MetricEffectiveConfigKeyV1::FscMinNonNeutralKnownCoverage,
    MetricEffectiveConfigKeyV1::FscSameSlotOrderingPolicy,
    MetricEffectiveConfigKeyV1::FscNeutralFunderSetVersion,
    MetricEffectiveConfigKeyV1::FscNeutralFunderSetHash,
    MetricEffectiveConfigKeyV1::FscFundingStreamUnavailableBehavior,
    MetricEffectiveConfigKeyV1::FscLegacyStatusMapping,
    MetricEffectiveConfigKeyV1::FscV2StatusMapping,
    MetricEffectiveConfigKeyV1::ManipulationNumericPresenceVersion,
    MetricEffectiveConfigKeyV1::ManipulationBooleanPresenceVersion,
    MetricEffectiveConfigKeyV1::ManipulationHighFlagDerivationVersion,
    MetricEffectiveConfigKeyV1::ManipulationHighSameMsThreshold,
    MetricEffectiveConfigKeyV1::ManipulationHighBundleThreshold,
    MetricEffectiveConfigKeyV1::ManipulationHighTop3Threshold,
    MetricEffectiveConfigKeyV1::ManipulationHighHhiThreshold,
    MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold,
    MetricEffectiveConfigKeyV1::ManipulationHighDevConcentrationThreshold,
    MetricEffectiveConfigKeyV1::ManipulationMissingRawBehavior,
    MetricEffectiveConfigKeyV1::ManipulationMeasuredFieldsMaskVersion,
    MetricEffectiveConfigKeyV1::ReserveVelocitySourceClock,
    MetricEffectiveConfigKeyV1::ReserveVelocityFirstUpdateBehavior,
    MetricEffectiveConfigKeyV1::ReserveVelocityZeroDeltaTimeBehavior,
    MetricEffectiveConfigKeyV1::ReserveVelocityFallbackBehavior,
    MetricEffectiveConfigKeyV1::ReserveVelocityUnit,
    MetricEffectiveConfigKeyV1::RecentBuySellWindowMs,
    MetricEffectiveConfigKeyV1::RecentBuySellSuccessfulOnly,
    MetricEffectiveConfigKeyV1::RecentBuySellBoundaryPolicy,
    MetricEffectiveConfigKeyV1::RecentBuySellSameMsNumeratorRule,
    MetricEffectiveConfigKeyV1::RecentBuySellLegacyRatioRule,
    MetricEffectiveConfigKeyV1::RecentBuySellUnboundedRatioRule,
    MetricEffectiveConfigKeyV1::RecentBuySellBoundedShareRule,
    MetricEffectiveConfigKeyV1::RecentBuySellZeroDenominatorBehavior,
    MetricEffectiveConfigKeyV1::ComparatorNormalizationVersion,
    MetricEffectiveConfigKeyV1::ComparatorFloatEquivalenceRule,
    MetricEffectiveConfigKeyV1::ComparatorEquivalenceLaneVersion,
    MetricEffectiveConfigKeyV1::ComparatorActionabilityMappingVersion,
    MetricEffectiveConfigKeyV1::ComparatorStatusMappingVersion,
    MetricEffectiveConfigKeyV1::ComparatorLegacyMissingFieldBehavior,
];

impl MetricEffectiveConfigKeyV1 {
    #[must_use]
    pub const fn contract_id(self) -> Option<MetricContractId> {
        use MetricEffectiveConfigKeyV1 as Key;
        match self {
            Key::FtdiPopulationSuccessfulBuy
            | Key::FtdiFirstSamplePerSigner
            | Key::FtdiMissingSignerBehavior
            | Key::FtdiMissingTopologyBehavior
            | Key::FtdiDiagnosticMinUniqueBuyers
            | Key::FtdiLegacyCleanMinBuyTransactions
            | Key::FtdiCandidateCleanMinUniqueBuyers
            | Key::FtdiDenominatorRule => Some(MetricContractId::FeeTopologyDiversityIndex),
            Key::DevTxIntelSuccessEligibility
            | Key::DevTxIntelDustThresholdSol
            | Key::DevTxIntelDedupeKey
            | Key::DevTxIntelDedupeCapacity
            | Key::DevFirstObservedAnchorRule
            | Key::DevPrimarySuccessRequired
            | Key::DevPrimaryDustThresholdSol
            | Key::DevPrimaryDedupeKey
            | Key::DevPrimaryAnchorRule
            | Key::DevMissingCreatorBehavior => Some(MetricContractId::DevBuy),
            Key::SameMsExactDeltaMs
            | Key::SameMsLegacyPopulation
            | Key::SameMsLegacyDenominatorRule
            | Key::SameMsClusterUpperBoundExclusiveMs
            | Key::SameMsRecentWindowMs
            | Key::SameMsRecentPopulation
            | Key::SameMsRecentDenominatorRule => Some(MetricContractId::SameMsTxRatio),
            Key::Top3PreferredField
            | Key::Top3FallbackAlias
            | Key::Top3Scale
            | Key::Top3MismatchBehavior => Some(MetricContractId::Top3SignerVolumeRatio),
            Key::FlipLegacyWindowSemantics
            | Key::FlipCandidateWallClockWindowMs
            | Key::FlipCandidateMaxSlotGap
            | Key::FlipCandidateDumpRatio
            | Key::FlipCandidateAnchorRule
            | Key::FlipCandidateOrderPolicy
            | Key::FlipCandidateSuccessRequired
            | Key::FlipCandidateDustThresholdSol
            | Key::FlipCandidateDedupeKey
            | Key::FlipCandidateDedupeCapacity
            | Key::FlipCandidateEvictionPolicy
            | Key::FlipCandidateMaxWallets
            | Key::FlipCandidateReconnectBehavior => Some(MetricContractId::FlipRatio),
            Key::FscLegacyFormula
            | Key::FscFundingLookbackWindowMs
            | Key::FscMinAbsStoreLamports
            | Key::FscMinAbsAttributionLamports
            | Key::FscMinRelativeToBuy
            | Key::FscMinAttributionConfidenceBps
            | Key::FscPerRecipientCapacity
            | Key::FscGlobalRecipientCapacity
            | Key::FscWarmupWindowMs
            | Key::FscMinTotalBuyers
            | Key::FscMinKnownNonNeutralBuyers
            | Key::FscMinKnownCoverage
            | Key::FscMinNonNeutralKnownCoverage
            | Key::FscSameSlotOrderingPolicy
            | Key::FscNeutralFunderSetVersion
            | Key::FscNeutralFunderSetHash
            | Key::FscFundingStreamUnavailableBehavior => {
                Some(MetricContractId::FundingSourceConcentration)
            }
            Key::FscLegacyStatusMapping | Key::FscV2StatusMapping => {
                Some(MetricContractId::FscEvidenceStatus)
            }
            Key::ManipulationNumericPresenceVersion
            | Key::ManipulationBooleanPresenceVersion
            | Key::ManipulationHighFlagDerivationVersion
            | Key::ManipulationHighSameMsThreshold
            | Key::ManipulationHighBundleThreshold
            | Key::ManipulationHighTop3Threshold
            | Key::ManipulationHighHhiThreshold
            | Key::ManipulationHighSignerCountThreshold
            | Key::ManipulationHighDevConcentrationThreshold
            | Key::ManipulationMissingRawBehavior
            | Key::ManipulationMeasuredFieldsMaskVersion => {
                Some(MetricContractId::ManipulationContradiction)
            }
            Key::ReserveVelocitySourceClock
            | Key::ReserveVelocityFirstUpdateBehavior
            | Key::ReserveVelocityZeroDeltaTimeBehavior
            | Key::ReserveVelocityFallbackBehavior
            | Key::ReserveVelocityUnit => Some(MetricContractId::ReserveVelocity),
            Key::RecentBuySellWindowMs
            | Key::RecentBuySellSuccessfulOnly
            | Key::RecentBuySellBoundaryPolicy
            | Key::RecentBuySellSameMsNumeratorRule
            | Key::RecentBuySellLegacyRatioRule
            | Key::RecentBuySellUnboundedRatioRule
            | Key::RecentBuySellBoundedShareRule
            | Key::RecentBuySellZeroDenominatorBehavior => Some(MetricContractId::RecentBuySell),
            Key::ComparatorNormalizationVersion
            | Key::ComparatorFloatEquivalenceRule
            | Key::ComparatorEquivalenceLaneVersion
            | Key::ComparatorActionabilityMappingVersion
            | Key::ComparatorStatusMappingVersion
            | Key::ComparatorLegacyMissingFieldBehavior => None,
        }
    }

    #[must_use]
    pub const fn value_kind(self) -> MetricEffectiveConfigValueKindV1 {
        use MetricEffectiveConfigKeyV1 as Key;
        match self {
            Key::FtdiFirstSamplePerSigner
            | Key::DevPrimarySuccessRequired
            | Key::FlipCandidateSuccessRequired
            | Key::RecentBuySellSuccessfulOnly => MetricEffectiveConfigValueKindV1::Boolean,

            Key::FscMinAttributionConfidenceBps => MetricEffectiveConfigValueKindV1::Unsigned,

            Key::DevTxIntelDustThresholdSol | Key::DevPrimaryDustThresholdSol => {
                MetricEffectiveConfigValueKindV1::FiniteNumber
            }

            Key::FlipCandidateDumpRatio
            | Key::FscMinRelativeToBuy
            | Key::FscMinKnownCoverage
            | Key::FscMinNonNeutralKnownCoverage
            | Key::ManipulationHighSameMsThreshold
            | Key::ManipulationHighBundleThreshold
            | Key::ManipulationHighTop3Threshold
            | Key::ManipulationHighHhiThreshold
            | Key::ManipulationHighDevConcentrationThreshold => {
                MetricEffectiveConfigValueKindV1::Ratio
            }

            Key::FtdiDiagnosticMinUniqueBuyers
            | Key::FtdiLegacyCleanMinBuyTransactions
            | Key::FtdiCandidateCleanMinUniqueBuyers
            | Key::DevTxIntelDedupeCapacity
            | Key::SameMsExactDeltaMs
            | Key::SameMsClusterUpperBoundExclusiveMs
            | Key::SameMsRecentWindowMs
            | Key::FlipCandidateWallClockWindowMs
            | Key::FlipCandidateMaxSlotGap
            | Key::FlipCandidateDedupeCapacity
            | Key::FlipCandidateMaxWallets
            | Key::FscFundingLookbackWindowMs
            | Key::FscMinAbsStoreLamports
            | Key::FscMinAbsAttributionLamports
            | Key::FscPerRecipientCapacity
            | Key::FscGlobalRecipientCapacity
            | Key::FscWarmupWindowMs
            | Key::FscMinTotalBuyers
            | Key::FscMinKnownNonNeutralBuyers
            | Key::ManipulationHighSignerCountThreshold
            | Key::RecentBuySellWindowMs => MetricEffectiveConfigValueKindV1::WideUnsigned,

            Key::FscNeutralFunderSetVersion => MetricEffectiveConfigValueKindV1::NullableText,

            Key::FscNeutralFunderSetHash => MetricEffectiveConfigValueKindV1::NullableHash,

            _ => MetricEffectiveConfigValueKindV1::Enum,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEffectiveConfigValueKindV1 {
    Boolean,
    Unsigned,
    FiniteNumber,
    Ratio,
    WideUnsigned,
    Text,
    NullableText,
    NullableHash,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MetricEffectiveConfigValueV1 {
    Boolean(bool),
    Unsigned(u32),
    FiniteNumber(f64),
    Ratio(f64),
    WideUnsigned(CanonicalU64StringV1),
    Text(String),
    NullableText(CanonicalNullableV1<String>),
    NullableHash(CanonicalNullableV1<CanonicalHashV1>),
    Enum(String),
}

impl MetricEffectiveConfigValueV1 {
    #[must_use]
    pub const fn kind(&self) -> MetricEffectiveConfigValueKindV1 {
        match self {
            Self::Boolean(_) => MetricEffectiveConfigValueKindV1::Boolean,
            Self::Unsigned(_) => MetricEffectiveConfigValueKindV1::Unsigned,
            Self::FiniteNumber(_) => MetricEffectiveConfigValueKindV1::FiniteNumber,
            Self::Ratio(_) => MetricEffectiveConfigValueKindV1::Ratio,
            Self::WideUnsigned(_) => MetricEffectiveConfigValueKindV1::WideUnsigned,
            Self::Text(_) => MetricEffectiveConfigValueKindV1::Text,
            Self::NullableText(_) => MetricEffectiveConfigValueKindV1::NullableText,
            Self::NullableHash(_) => MetricEffectiveConfigValueKindV1::NullableHash,
            Self::Enum(_) => MetricEffectiveConfigValueKindV1::Enum,
        }
    }

    fn validate(&self) -> Result<(), MetricContractEffectiveConfigErrorV1> {
        match self {
            Self::FiniteNumber(value) if !value.is_finite() => {
                Err(MetricContractEffectiveConfigErrorV1::NonFiniteValue)
            }
            Self::Ratio(value) if !value.is_finite() || !(0.0..=1.0).contains(value) => {
                Err(MetricContractEffectiveConfigErrorV1::InvalidRatio)
            }
            Self::Text(value) | Self::Enum(value) if value.trim().is_empty() => {
                Err(MetricContractEffectiveConfigErrorV1::BlankTextValue)
            }
            Self::NullableText(CanonicalNullableV1::Value(value)) if value.trim().is_empty() => {
                Err(MetricContractEffectiveConfigErrorV1::BlankTextValue)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricEffectiveConfigEntryV1 {
    pub key: MetricEffectiveConfigKeyV1,
    pub value: MetricEffectiveConfigValueV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractEffectiveConfigHashPayloadV1 {
    pub registry_id: String,
    pub schema_version: u16,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile_id: MetricContractProfileIdV1,
    pub profile_hash: CanonicalHashV1,
    pub entries: Vec<MetricEffectiveConfigEntryV1>,
}

pub fn metric_contract_effective_config_hash(
    payload: &MetricContractEffectiveConfigHashPayloadV1,
) -> Result<CanonicalHashV1, CanonicalHashErrorV1> {
    CanonicalHashV1::digest(payload)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedMetricContractEffectiveConfigV1 {
    pub payload: MetricContractEffectiveConfigHashPayloadV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
}

impl ResolvedMetricContractEffectiveConfigV1 {
    pub fn try_from_payload(
        mut payload: MetricContractEffectiveConfigHashPayloadV1,
    ) -> Result<Self, MetricContractEffectiveConfigErrorV1> {
        validate_and_sort_entries(&mut payload.entries)?;
        if payload.registry_id != METRIC_CONTRACT_REGISTRY_ID_V1_1 {
            return Err(MetricContractEffectiveConfigErrorV1::UnknownRegistry(
                payload.registry_id,
            ));
        }
        if payload.schema_version != METRIC_CONTRACT_SCHEMA_VERSION_V1 {
            return Err(MetricContractEffectiveConfigErrorV1::UnsupportedSchema(
                payload.schema_version,
            ));
        }
        let selected_profile = MetricContractFoundationConfigV1 {
            metric_contract_rollout_mode: payload.rollout_mode,
            metric_contract_profile: payload.profile_id,
        }
        .resolve_profile()?;
        if selected_profile.canonical_hash()? != payload.profile_hash {
            return Err(MetricContractEffectiveConfigErrorV1::ProfileHashMismatch);
        }
        let hash = metric_contract_effective_config_hash(&payload)?;
        Ok(Self {
            payload,
            metric_contract_effective_config_hash: hash,
        })
    }

    pub fn validate_hash(&self) -> Result<(), MetricContractEffectiveConfigErrorV1> {
        let validated = Self::try_from_payload(self.payload.clone())?;
        if validated.metric_contract_effective_config_hash
            != self.metric_contract_effective_config_hash
        {
            return Err(MetricContractEffectiveConfigErrorV1::HashMismatch);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResolvedMetricContractEffectiveConfigV1 {
    payload: MetricContractEffectiveConfigHashPayloadV1,
    metric_contract_effective_config_hash: CanonicalHashV1,
}

impl<'de> Deserialize<'de> for ResolvedMetricContractEffectiveConfigV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawResolvedMetricContractEffectiveConfigV1::deserialize(deserializer)?;
        let resolved = Self::try_from_payload(raw.payload).map_err(serde::de::Error::custom)?;
        if resolved.metric_contract_effective_config_hash
            != raw.metric_contract_effective_config_hash
        {
            return Err(serde::de::Error::custom(
                MetricContractEffectiveConfigErrorV1::HashMismatch,
            ));
        }
        Ok(resolved)
    }
}

#[derive(Debug, Error)]
pub enum MetricContractEffectiveConfigErrorV1 {
    #[error(transparent)]
    Profile(#[from] MetricContractProfileErrorV1),
    #[error(transparent)]
    Hash(#[from] CanonicalHashErrorV1),
    #[error("unknown metric contract registry id: {0}")]
    UnknownRegistry(String),
    #[error("unsupported metric contract effective-config schema: {0}")]
    UnsupportedSchema(u16),
    #[error("duplicate effective-config key: {0:?}")]
    DuplicateKey(MetricEffectiveConfigKeyV1),
    #[error("missing effective-config key: {0:?}")]
    MissingKey(MetricEffectiveConfigKeyV1),
    #[error("effective-config key {key:?} expects {expected:?}, got {actual:?}")]
    WrongValueKind {
        key: MetricEffectiveConfigKeyV1,
        expected: MetricEffectiveConfigValueKindV1,
        actual: MetricEffectiveConfigValueKindV1,
    },
    #[error("effective-config contains a non-finite number")]
    NonFiniteValue,
    #[error("effective-config ratio must be finite and within [0, 1]")]
    InvalidRatio,
    #[error("effective-config text/enum value must not be blank")]
    BlankTextValue,
    #[error("effective-config profile hash does not match the selected compiled profile")]
    ProfileHashMismatch,
    #[error("metric_contract_effective_config_hash mismatch")]
    HashMismatch,
}

fn validate_and_sort_entries(
    entries: &mut [MetricEffectiveConfigEntryV1],
) -> Result<(), MetricContractEffectiveConfigErrorV1> {
    let expected = METRIC_EFFECTIVE_CONFIG_KEYS_V1
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for entry in entries.iter() {
        if !seen.insert(entry.key) {
            return Err(MetricContractEffectiveConfigErrorV1::DuplicateKey(
                entry.key,
            ));
        }
        let expected_kind = entry.key.value_kind();
        let actual_kind = entry.value.kind();
        if expected_kind != actual_kind {
            return Err(MetricContractEffectiveConfigErrorV1::WrongValueKind {
                key: entry.key,
                expected: expected_kind,
                actual: actual_kind,
            });
        }
        entry.value.validate()?;
    }
    if let Some(key) = expected.difference(&seen).next() {
        return Err(MetricContractEffectiveConfigErrorV1::MissingKey(*key));
    }
    entries.sort_by_key(|entry| entry.key);
    Ok(())
}

pub struct MetricContractEffectiveConfigBuilderV1 {
    registry_id: String,
    schema_version: u16,
    rollout_mode: MetricContractRolloutMode,
    profile_id: MetricContractProfileIdV1,
    profile_hash: CanonicalHashV1,
    entries: BTreeMap<MetricEffectiveConfigKeyV1, MetricEffectiveConfigValueV1>,
}

impl MetricContractEffectiveConfigBuilderV1 {
    pub fn new(
        foundation: MetricContractFoundationConfigV1,
    ) -> Result<Self, MetricContractEffectiveConfigErrorV1> {
        let profile = foundation.resolve_profile()?;
        let profile_hash = profile.canonical_hash()?;
        Ok(Self {
            registry_id: METRIC_CONTRACT_REGISTRY_ID_V1_1.to_string(),
            schema_version: METRIC_CONTRACT_SCHEMA_VERSION_V1,
            rollout_mode: foundation.metric_contract_rollout_mode,
            profile_id: foundation.metric_contract_profile,
            profile_hash,
            entries: BTreeMap::new(),
        })
    }

    pub fn insert(
        &mut self,
        key: MetricEffectiveConfigKeyV1,
        value: MetricEffectiveConfigValueV1,
    ) -> Result<&mut Self, MetricContractEffectiveConfigErrorV1> {
        if self.entries.insert(key, value).is_some() {
            return Err(MetricContractEffectiveConfigErrorV1::DuplicateKey(key));
        }
        Ok(self)
    }

    pub fn build(
        self,
    ) -> Result<ResolvedMetricContractEffectiveConfigV1, MetricContractEffectiveConfigErrorV1> {
        ResolvedMetricContractEffectiveConfigV1::try_from_payload(
            MetricContractEffectiveConfigHashPayloadV1 {
                registry_id: self.registry_id,
                schema_version: self.schema_version,
                rollout_mode: self.rollout_mode,
                profile_id: self.profile_id,
                profile_hash: self.profile_hash,
                entries: self
                    .entries
                    .into_iter()
                    .map(|(key, value)| MetricEffectiveConfigEntryV1 { key, value })
                    .collect(),
            },
        )
    }
}
