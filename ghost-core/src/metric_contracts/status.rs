use super::{
    MetricAuthorityClass, MetricContractId, MetricContractProfileV1, MetricContractRolloutMode,
    MetricRolloutRoleV1, MetricSurfaceId, METRIC_CONTRACTS_V1_1,
};
use crate::checkpoint::types::{
    EvidenceDegradedReason, EvidenceStatus, EvidenceUnavailableReason, FeatureEvidenceStatus,
    MetricEvidenceQuality,
};
use crate::tx_intelligence::types::{
    FscEvidenceStatus, FscExcludedReason, DBIA_NO_DEV_BUY_REASON,
    FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON, FSC_BUYER_IDENTITY_UNAVAILABLE_REASON,
    FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON, FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
    FSC_GLOBAL_RECIPIENT_EVICTED_REASON, FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
    FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON, FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
    FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON, FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
    FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON, FSC_RELATIVE_FUNDING_TOO_SMALL_REASON,
    FSC_ROLLING_STATE_UNAVAILABLE_REASON, FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
    FTDI_INSUFFICIENT_BUYS_REASON, FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricAvailabilityV1 {
    Available,
    Unavailable,
    NotConfigured,
    NotRecordedLegacySchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricMeasurementQualityV1 {
    Measured,
    Degraded,
    Insufficient,
    Stale,
    Fallback,
    LegacyDefault,
    NotApplicable,
}

/// Contract-membership check for typed reason families carried by the generic
/// canonical envelope. Implementations must fail closed for a reason that is
/// valid syntactically but attached to the wrong metric family.
pub trait MetricEvidenceReasonContractV1 {
    fn belongs_to_contract(&self, contract_id: MetricContractId) -> bool;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MetricEvidenceEnvelopeErrorV1 {
    #[error("non-available metric must use not_applicable quality")]
    NonAvailableQuality,
    #[error("non-available metric cannot be policy-actionable")]
    NonAvailableActionable,
    #[error("available metric cannot use not_applicable quality")]
    AvailableNotApplicable,
    #[error(
        "insufficient, stale, legacy-default, or not-applicable evidence cannot be actionable"
    )]
    InvalidActionableQuality,
    #[error("authority class {0:?} cannot be policy-actionable")]
    InvalidActionableAuthority(MetricAuthorityClass),
    #[error("profile does not contain surface {0:?}")]
    SurfaceMissingFromProfile(MetricSurfaceId),
    #[error("metric contract registry does not contain surface {0:?}")]
    SurfaceMissingFromRegistry(MetricSurfaceId),
    #[error("evidence field expects surface {expected:?}, got {actual:?}")]
    UnexpectedSurfaceForEvidenceField {
        expected: MetricSurfaceId,
        actual: MetricSurfaceId,
    },
    #[error("envelope authority class does not match profile for surface {0:?}")]
    AuthorityClassMismatch(MetricSurfaceId),
    #[error("surface {0:?} is not authoritative in selected rollout mode")]
    SurfaceNotAuthoritative(MetricSurfaceId),
    #[error("surface {surface:?} belongs to {expected:?}, envelope uses {actual:?}")]
    ContractSurfaceMismatch {
        surface: MetricSurfaceId,
        expected: MetricContractId,
        actual: MetricContractId,
    },
    #[error("contract {contract:?} uses version {actual}, expected {expected}")]
    ContractVersionMismatch {
        contract: MetricContractId,
        expected: u16,
        actual: u16,
    },
    #[error("reason code family does not belong to contract {0:?}")]
    ReasonContractMismatch(MetricContractId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetricEvidenceEnvelopeV1<R> {
    pub contract_id: MetricContractId,
    pub contract_version: u16,
    pub surface_id: MetricSurfaceId,
    pub authority_class: MetricAuthorityClass,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub policy_actionable: bool,
    pub reason_codes: Vec<R>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMetricEvidenceEnvelopeV1<R> {
    contract_id: MetricContractId,
    contract_version: u16,
    surface_id: MetricSurfaceId,
    authority_class: MetricAuthorityClass,
    availability: MetricAvailabilityV1,
    measurement_quality: MetricMeasurementQualityV1,
    policy_actionable: bool,
    reason_codes: Vec<R>,
}

impl<R> MetricEvidenceEnvelopeV1<R> {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        contract_id: MetricContractId,
        contract_version: u16,
        surface_id: MetricSurfaceId,
        authority_class: MetricAuthorityClass,
        availability: MetricAvailabilityV1,
        measurement_quality: MetricMeasurementQualityV1,
        policy_actionable: bool,
        reason_codes: Vec<R>,
    ) -> Result<Self, MetricEvidenceEnvelopeErrorV1> {
        let envelope = Self {
            contract_id,
            contract_version,
            surface_id,
            authority_class,
            availability,
            measurement_quality,
            policy_actionable,
            reason_codes,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), MetricEvidenceEnvelopeErrorV1> {
        let definition = METRIC_CONTRACTS_V1_1
            .iter()
            .find(|definition| definition.surfaces.contains(&self.surface_id))
            .ok_or(MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromRegistry(
                self.surface_id,
            ))?;
        if definition.id != self.contract_id {
            return Err(MetricEvidenceEnvelopeErrorV1::ContractSurfaceMismatch {
                surface: self.surface_id,
                expected: definition.id,
                actual: self.contract_id,
            });
        }
        if definition.version != self.contract_version {
            return Err(MetricEvidenceEnvelopeErrorV1::ContractVersionMismatch {
                contract: self.contract_id,
                expected: definition.version,
                actual: self.contract_version,
            });
        }

        if self.availability != MetricAvailabilityV1::Available {
            if self.measurement_quality != MetricMeasurementQualityV1::NotApplicable {
                return Err(MetricEvidenceEnvelopeErrorV1::NonAvailableQuality);
            }
            if self.policy_actionable {
                return Err(MetricEvidenceEnvelopeErrorV1::NonAvailableActionable);
            }
            return Ok(());
        }

        if self.measurement_quality == MetricMeasurementQualityV1::NotApplicable {
            return Err(MetricEvidenceEnvelopeErrorV1::AvailableNotApplicable);
        }
        if self.policy_actionable
            && matches!(
                self.measurement_quality,
                MetricMeasurementQualityV1::Insufficient
                    | MetricMeasurementQualityV1::Stale
                    | MetricMeasurementQualityV1::LegacyDefault
            )
        {
            return Err(MetricEvidenceEnvelopeErrorV1::InvalidActionableQuality);
        }
        if self.policy_actionable
            && !matches!(
                self.authority_class,
                MetricAuthorityClass::Authoritative | MetricAuthorityClass::EquivalentCutover
            )
        {
            return Err(MetricEvidenceEnvelopeErrorV1::InvalidActionableAuthority(
                self.authority_class,
            ));
        }
        Ok(())
    }

    pub fn validate_for_profile(
        &self,
        profile: &MetricContractProfileV1,
        mode: MetricContractRolloutMode,
    ) -> Result<(), MetricEvidenceEnvelopeErrorV1>
    where
        R: MetricEvidenceReasonContractV1,
    {
        self.validate()?;
        self.validate_reason_contracts()?;
        let entry = profile.entry_for(self.surface_id).ok_or(
            MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromProfile(self.surface_id),
        )?;
        if entry.contract_id != self.contract_id {
            return Err(MetricEvidenceEnvelopeErrorV1::ContractSurfaceMismatch {
                surface: self.surface_id,
                expected: entry.contract_id,
                actual: self.contract_id,
            });
        }
        if entry.authority_class != self.authority_class {
            return Err(MetricEvidenceEnvelopeErrorV1::AuthorityClassMismatch(
                self.surface_id,
            ));
        }
        if self.policy_actionable
            && entry.role_for(mode) != MetricRolloutRoleV1::PolicyAuthoritative
        {
            return Err(MetricEvidenceEnvelopeErrorV1::SurfaceNotAuthoritative(
                self.surface_id,
            ));
        }
        Ok(())
    }
}

impl<R: MetricEvidenceReasonContractV1> MetricEvidenceEnvelopeV1<R> {
    fn validate_reason_contracts(&self) -> Result<(), MetricEvidenceEnvelopeErrorV1> {
        let valid = self
            .reason_codes
            .iter()
            .all(|reason| reason.belongs_to_contract(self.contract_id));
        if !valid {
            return Err(MetricEvidenceEnvelopeErrorV1::ReasonContractMismatch(
                self.contract_id,
            ));
        }
        Ok(())
    }

    pub fn validate_typed(&self) -> Result<(), MetricEvidenceEnvelopeErrorV1> {
        self.validate()?;
        self.validate_reason_contracts()
    }
}

impl<'de, R> Deserialize<'de> for MetricEvidenceEnvelopeV1<R>
where
    R: Deserialize<'de> + MetricEvidenceReasonContractV1,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawMetricEvidenceEnvelopeV1::<R>::deserialize(deserializer)?;
        let envelope = Self::try_new(
            raw.contract_id,
            raw.contract_version,
            raw.surface_id,
            raw.authority_class,
            raw.availability,
            raw.measurement_quality,
            raw.policy_actionable,
            raw.reason_codes,
        )
        .map_err(serde::de::Error::custom)?;
        envelope
            .validate_reason_contracts()
            .map_err(serde::de::Error::custom)?;
        Ok(envelope)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyEvidenceAdapterContextV1 {
    pub contract_id: MetricContractId,
    pub contract_version: u16,
    pub surface_id: MetricSurfaceId,
    pub authority_class: MetricAuthorityClass,
    /// Whether a legacy `Clean` value is actionable under the currently
    /// selected contract-specific sample/readiness gate.
    pub clean_policy_actionable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyStatusReasonV1 {
    Degraded,
    Unavailable,
    InsufficientSample,
    Stale,
    Fallback,
    ShadowOnly,
    NotConfigured,
    CarriedForward,
    NotAllowed,
    UnavailableSource,
    CleanWithLegacyReasons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyEvidenceDegradedReasonV1 {
    SegmentSequencePartial,
    SegmentSignerCoveragePartial,
    TxIntelLowSample,
    AccountStateFallback,
    CheckpointHistorySparse,
    CurveEvidencePartial,
    SybilEvidencePartial,
    AlphaEvidencePartial,
    ManipulationEvidencePartial,
    IdentityEvidenceFallback,
    TrajectoryEvidenceSparse,
    PddSequencePartial,
    CpvEvidencePartial,
    FscEvidencePartial,
    OrganicBroadeningInsufficient,
    ManipulationContradictionPartial,
    DecisionTimeSeriesPricePartial,
    DecisionTimeSeriesTruncated,
    EvidenceStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyEvidenceUnavailableReasonV1 {
    NotMaterialized,
    IdentityMissing,
    SegmentSequenceMissing,
    SegmentSignerDataMissing,
    TxIntelMissing,
    AccountStateMissing,
    CheckpointHistoryMissing,
    CurveDataMissing,
    TrajectoryMissing,
    PddSequenceMissing,
    SybilMetricsMissing,
    AlphaFingerprintMissing,
    CpvMetricsMissing,
    FscMetricsMissing,
    OrganicBroadeningMissing,
    ManipulationContradictionMissing,
    ExecutionNotRun,
    NotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtdiEvidenceReasonV1 {
    InsufficientBuyTransactions,
    InsufficientUniqueBuyers,
    RawFeeTopologyUnavailable,
    LegacyBuyTransactionActionabilityGate,
    UniqueBuyerActionabilityCounterfactual,
    CoordinationHhiExportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevBuyEvidenceReasonV1 {
    CreatorUnknown,
    NoEligibleBuy,
    CreateSignatureUnavailable,
    CreateSignatureNotMatched,
    FailedTransactionExcluded,
    DuplicateExcluded,
    DustExcluded,
    LegacyFirstObservedIncludesAcceptedFailed,
    PrimaryBuyCounterfactual,
    CandidateHistoryTruncated,
    CompatibilityPrimaryIncludesAcceptedFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxTimingEvidenceReasonV1 {
    InsufficientTransactions,
    TimestampUnavailable,
    OrderingIdentityUnavailable,
    SourceWindowTruncated,
    ExactSameMillisecond,
    ClusterBelowFiftyMilliseconds,
    RecentWindow,
    LegacyTransactionCountDenominator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Top3EvidenceReasonV1 {
    PreferredFieldUnavailable,
    CompatibilityAliasFallback,
    PreferredAliasMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipEvidenceReasonV1 {
    NoEligibleBuyers,
    NoAnchor,
    MissingStableIdentity,
    MissingStableOrder,
    DuplicateEvent,
    FailedTransactionExcluded,
    DustExcluded,
    WalletCapReached,
    ReconnectGap,
    OutOfOrderEvent,
    ClosedNonFlipper,
    LegacySlotGapOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingSourceEvidenceReasonV1 {
    FundingLaneUnavailable,
    RollingStateUnavailable,
    IndexCold,
    NoBuyerCohort,
    InsufficientKnownSources,
    InsufficientNonNeutralSupport,
    LowCoverage,
    NeutralOnly,
    BuyerIdentityUnavailable,
    BuyTimestampUnavailable,
    NoRetainedRecipientHistory,
    LookbackWindowExhausted,
    NoPrebuyTransferInWindow,
    SameSlotOrderingUnavailable,
    LowAttributionConfidence,
    AbsoluteAttributionTooSmall,
    RelativeFundingTooSmall,
    PerRecipientHistoryOverflow,
    GlobalRecipientEvicted,
    LegacyScalarPresenceOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManipulationEvidenceReasonV1 {
    RawFieldAbsent,
    LegacyDefaultZero,
    LegacyDefaultFalse,
    ThresholdNotConfigured,
    DerivedInPolicy,
    MomentumWithoutBroadening,
    VolumeSpikeWithoutNewSigners,
    HighBuyPressureWithHighTop3,
    FixedSizeOrRampingPattern,
    TimingBundleConcentration,
    EarlyTop3Concentration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReserveVelocityEvidenceReasonV1 {
    BootstrapFirstUpdate,
    ZeroDeltaTime,
    FallbackState,
    SourceUnavailable,
    MeasuredZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentBuySellEvidenceReasonV1 {
    EmptyWindow,
    SellCountZero,
    ZeroDenominator,
    FailedTransactionExcluded,
    LegacySellZeroReturnsBuyCount,
    LoggingOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason_family", content = "detail", rename_all = "snake_case")]
pub enum MetricEvidenceReasonV1 {
    LegacyStatus(LegacyStatusReasonV1),
    LegacyDegraded(LegacyEvidenceDegradedReasonV1),
    LegacyUnavailable(LegacyEvidenceUnavailableReasonV1),
    Ftdi(FtdiEvidenceReasonV1),
    DevBuy(DevBuyEvidenceReasonV1),
    TxTiming(TxTimingEvidenceReasonV1),
    Top3(Top3EvidenceReasonV1),
    Flip(FlipEvidenceReasonV1),
    FundingSource(FundingSourceEvidenceReasonV1),
    Manipulation(ManipulationEvidenceReasonV1),
    ReserveVelocity(ReserveVelocityEvidenceReasonV1),
    RecentBuySell(RecentBuySellEvidenceReasonV1),
    UnmappedLegacyString {
        contract_id: MetricContractId,
        raw: String,
    },
}

impl MetricEvidenceReasonContractV1 for MetricEvidenceReasonV1 {
    fn belongs_to_contract(&self, contract_id: MetricContractId) -> bool {
        match self {
            Self::LegacyStatus(_) | Self::LegacyDegraded(_) | Self::LegacyUnavailable(_) => true,
            Self::Ftdi(_) => contract_id == MetricContractId::FeeTopologyDiversityIndex,
            Self::DevBuy(_) => contract_id == MetricContractId::DevBuy,
            Self::TxTiming(_) => contract_id == MetricContractId::SameMsTxRatio,
            Self::Top3(_) => contract_id == MetricContractId::Top3SignerVolumeRatio,
            Self::Flip(_) => contract_id == MetricContractId::FlipRatio,
            Self::FundingSource(_) => matches!(
                contract_id,
                MetricContractId::FundingSourceConcentration | MetricContractId::FscEvidenceStatus
            ),
            Self::Manipulation(_) => contract_id == MetricContractId::ManipulationContradiction,
            Self::ReserveVelocity(_) => contract_id == MetricContractId::ReserveVelocity,
            Self::RecentBuySell(_) => contract_id == MetricContractId::RecentBuySell,
            Self::UnmappedLegacyString {
                contract_id: reason_contract,
                raw,
            } => *reason_contract == contract_id && !raw.trim().is_empty(),
        }
    }
}

fn status_parts(
    status: EvidenceStatus,
    clean_policy_actionable: bool,
) -> (
    MetricAvailabilityV1,
    MetricMeasurementQualityV1,
    bool,
    Vec<MetricEvidenceReasonV1>,
) {
    match status {
        EvidenceStatus::Clean => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
            clean_policy_actionable,
            Vec::new(),
        ),
        EvidenceStatus::Degraded => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Degraded,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::Degraded,
            )],
        ),
        EvidenceStatus::Unavailable => (
            MetricAvailabilityV1::Unavailable,
            MetricMeasurementQualityV1::NotApplicable,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::Unavailable,
            )],
        ),
        EvidenceStatus::InsufficientSample => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Insufficient,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::InsufficientSample,
            )],
        ),
        EvidenceStatus::Stale => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Stale,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::Stale,
            )],
        ),
        EvidenceStatus::Fallback => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Fallback,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::Fallback,
            )],
        ),
        EvidenceStatus::ShadowOnly => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::ShadowOnly,
            )],
        ),
        EvidenceStatus::NotConfigured => (
            MetricAvailabilityV1::NotConfigured,
            MetricMeasurementQualityV1::NotApplicable,
            false,
            vec![MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::NotConfigured,
            )],
        ),
    }
}

pub fn adapt_evidence_status_v1(
    context: LegacyEvidenceAdapterContextV1,
    status: EvidenceStatus,
) -> Result<MetricEvidenceEnvelopeV1<MetricEvidenceReasonV1>, MetricEvidenceEnvelopeErrorV1> {
    let (availability, quality, actionable, reasons) =
        status_parts(status, context.clean_policy_actionable);
    MetricEvidenceEnvelopeV1::try_new(
        context.contract_id,
        context.contract_version,
        context.surface_id,
        context.authority_class,
        availability,
        quality,
        actionable,
        reasons,
    )
}

pub fn adapt_metric_evidence_quality_v1(
    context: LegacyEvidenceAdapterContextV1,
    quality: MetricEvidenceQuality,
) -> Result<MetricEvidenceEnvelopeV1<MetricEvidenceReasonV1>, MetricEvidenceEnvelopeErrorV1> {
    let (availability, measurement_quality, actionable, reason) = match quality {
        MetricEvidenceQuality::Clean => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
            context.clean_policy_actionable,
            None,
        ),
        MetricEvidenceQuality::DegradedLowSample => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Degraded,
            false,
            Some(LegacyStatusReasonV1::Degraded),
        ),
        MetricEvidenceQuality::CarriedForward => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Fallback,
            false,
            Some(LegacyStatusReasonV1::CarriedForward),
        ),
        MetricEvidenceQuality::InsufficientSample => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Insufficient,
            false,
            Some(LegacyStatusReasonV1::InsufficientSample),
        ),
        MetricEvidenceQuality::Stale => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Stale,
            false,
            Some(LegacyStatusReasonV1::Stale),
        ),
        MetricEvidenceQuality::NotAllowed => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Degraded,
            false,
            Some(LegacyStatusReasonV1::NotAllowed),
        ),
        MetricEvidenceQuality::UnavailableSource | MetricEvidenceQuality::Unavailable => (
            MetricAvailabilityV1::Unavailable,
            MetricMeasurementQualityV1::NotApplicable,
            false,
            Some(if quality == MetricEvidenceQuality::UnavailableSource {
                LegacyStatusReasonV1::UnavailableSource
            } else {
                LegacyStatusReasonV1::Unavailable
            }),
        ),
        MetricEvidenceQuality::NotConfigured => (
            MetricAvailabilityV1::NotConfigured,
            MetricMeasurementQualityV1::NotApplicable,
            false,
            Some(LegacyStatusReasonV1::NotConfigured),
        ),
    };
    MetricEvidenceEnvelopeV1::try_new(
        context.contract_id,
        context.contract_version,
        context.surface_id,
        context.authority_class,
        availability,
        measurement_quality,
        actionable,
        reason
            .map(|reason| vec![MetricEvidenceReasonV1::LegacyStatus(reason)])
            .unwrap_or_default(),
    )
}

pub fn adapt_fsc_evidence_status_v1(
    context: LegacyEvidenceAdapterContextV1,
    status: FscEvidenceStatus,
) -> Result<MetricEvidenceEnvelopeV1<MetricEvidenceReasonV1>, MetricEvidenceEnvelopeErrorV1> {
    match status {
        FscEvidenceStatus::Clean => adapt_evidence_status_v1(context, EvidenceStatus::Clean),
        FscEvidenceStatus::Degraded => adapt_evidence_status_v1(context, EvidenceStatus::Degraded),
        FscEvidenceStatus::Unavailable => {
            adapt_evidence_status_v1(context, EvidenceStatus::Unavailable)
        }
    }
}

pub fn adapt_feature_evidence_status_v1(
    context: LegacyEvidenceAdapterContextV1,
    status: &FeatureEvidenceStatus,
) -> Result<MetricEvidenceEnvelopeV1<MetricEvidenceReasonV1>, MetricEvidenceEnvelopeErrorV1> {
    let mut envelope = adapt_evidence_status_v1(context, status.status)?;
    envelope.reason_codes.extend(
        status
            .degraded_reasons
            .iter()
            .copied()
            .map(adapt_evidence_degraded_reason_v1),
    );
    envelope.reason_codes.extend(
        status
            .unavailable_reasons
            .iter()
            .copied()
            .map(adapt_evidence_unavailable_reason_v1),
    );

    if status.status == EvidenceStatus::Clean
        && (!status.degraded_reasons.is_empty() || !status.unavailable_reasons.is_empty())
    {
        envelope.measurement_quality = MetricMeasurementQualityV1::Degraded;
        envelope.policy_actionable = false;
        envelope
            .reason_codes
            .push(MetricEvidenceReasonV1::LegacyStatus(
                LegacyStatusReasonV1::CleanWithLegacyReasons,
            ));
    }
    envelope.validate()?;
    Ok(envelope)
}

pub fn adapt_evidence_degraded_reason_v1(reason: EvidenceDegradedReason) -> MetricEvidenceReasonV1 {
    let mapped = match reason {
        EvidenceDegradedReason::SegmentSequencePartial => {
            LegacyEvidenceDegradedReasonV1::SegmentSequencePartial
        }
        EvidenceDegradedReason::SegmentSignerCoveragePartial => {
            LegacyEvidenceDegradedReasonV1::SegmentSignerCoveragePartial
        }
        EvidenceDegradedReason::TxIntelLowSample => {
            LegacyEvidenceDegradedReasonV1::TxIntelLowSample
        }
        EvidenceDegradedReason::AccountStateFallback => {
            LegacyEvidenceDegradedReasonV1::AccountStateFallback
        }
        EvidenceDegradedReason::CheckpointHistorySparse => {
            LegacyEvidenceDegradedReasonV1::CheckpointHistorySparse
        }
        EvidenceDegradedReason::CurveEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::CurveEvidencePartial
        }
        EvidenceDegradedReason::SybilEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::SybilEvidencePartial
        }
        EvidenceDegradedReason::AlphaEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::AlphaEvidencePartial
        }
        EvidenceDegradedReason::ManipulationEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::ManipulationEvidencePartial
        }
        EvidenceDegradedReason::IdentityEvidenceFallback => {
            LegacyEvidenceDegradedReasonV1::IdentityEvidenceFallback
        }
        EvidenceDegradedReason::TrajectoryEvidenceSparse => {
            LegacyEvidenceDegradedReasonV1::TrajectoryEvidenceSparse
        }
        EvidenceDegradedReason::PddSequencePartial => {
            LegacyEvidenceDegradedReasonV1::PddSequencePartial
        }
        EvidenceDegradedReason::CpvEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::CpvEvidencePartial
        }
        EvidenceDegradedReason::FscEvidencePartial => {
            LegacyEvidenceDegradedReasonV1::FscEvidencePartial
        }
        EvidenceDegradedReason::OrganicBroadeningInsufficient => {
            LegacyEvidenceDegradedReasonV1::OrganicBroadeningInsufficient
        }
        EvidenceDegradedReason::ManipulationContradictionPartial => {
            LegacyEvidenceDegradedReasonV1::ManipulationContradictionPartial
        }
        EvidenceDegradedReason::DecisionTimeSeriesPricePartial => {
            LegacyEvidenceDegradedReasonV1::DecisionTimeSeriesPricePartial
        }
        EvidenceDegradedReason::DecisionTimeSeriesTruncated => {
            LegacyEvidenceDegradedReasonV1::DecisionTimeSeriesTruncated
        }
        EvidenceDegradedReason::EvidenceStale => LegacyEvidenceDegradedReasonV1::EvidenceStale,
    };
    MetricEvidenceReasonV1::LegacyDegraded(mapped)
}

pub fn adapt_evidence_unavailable_reason_v1(
    reason: EvidenceUnavailableReason,
) -> MetricEvidenceReasonV1 {
    let mapped = match reason {
        EvidenceUnavailableReason::NotMaterialized => {
            LegacyEvidenceUnavailableReasonV1::NotMaterialized
        }
        EvidenceUnavailableReason::IdentityMissing => {
            LegacyEvidenceUnavailableReasonV1::IdentityMissing
        }
        EvidenceUnavailableReason::SegmentSequenceMissing => {
            LegacyEvidenceUnavailableReasonV1::SegmentSequenceMissing
        }
        EvidenceUnavailableReason::SegmentSignerDataMissing => {
            LegacyEvidenceUnavailableReasonV1::SegmentSignerDataMissing
        }
        EvidenceUnavailableReason::TxIntelMissing => {
            LegacyEvidenceUnavailableReasonV1::TxIntelMissing
        }
        EvidenceUnavailableReason::AccountStateMissing => {
            LegacyEvidenceUnavailableReasonV1::AccountStateMissing
        }
        EvidenceUnavailableReason::CheckpointHistoryMissing => {
            LegacyEvidenceUnavailableReasonV1::CheckpointHistoryMissing
        }
        EvidenceUnavailableReason::CurveDataMissing => {
            LegacyEvidenceUnavailableReasonV1::CurveDataMissing
        }
        EvidenceUnavailableReason::TrajectoryMissing => {
            LegacyEvidenceUnavailableReasonV1::TrajectoryMissing
        }
        EvidenceUnavailableReason::PddSequenceMissing => {
            LegacyEvidenceUnavailableReasonV1::PddSequenceMissing
        }
        EvidenceUnavailableReason::SybilMetricsMissing => {
            LegacyEvidenceUnavailableReasonV1::SybilMetricsMissing
        }
        EvidenceUnavailableReason::AlphaFingerprintMissing => {
            LegacyEvidenceUnavailableReasonV1::AlphaFingerprintMissing
        }
        EvidenceUnavailableReason::CpvMetricsMissing => {
            LegacyEvidenceUnavailableReasonV1::CpvMetricsMissing
        }
        EvidenceUnavailableReason::FscMetricsMissing => {
            LegacyEvidenceUnavailableReasonV1::FscMetricsMissing
        }
        EvidenceUnavailableReason::OrganicBroadeningMissing => {
            LegacyEvidenceUnavailableReasonV1::OrganicBroadeningMissing
        }
        EvidenceUnavailableReason::ManipulationContradictionMissing => {
            LegacyEvidenceUnavailableReasonV1::ManipulationContradictionMissing
        }
        EvidenceUnavailableReason::ExecutionNotRun => {
            LegacyEvidenceUnavailableReasonV1::ExecutionNotRun
        }
        EvidenceUnavailableReason::NotConfigured => {
            LegacyEvidenceUnavailableReasonV1::NotConfigured
        }
    };
    MetricEvidenceReasonV1::LegacyUnavailable(mapped)
}

pub fn adapt_fsc_excluded_reason_v1(reason: FscExcludedReason) -> MetricEvidenceReasonV1 {
    MetricEvidenceReasonV1::FundingSource(match reason {
        FscExcludedReason::FundingLaneUnavailable => {
            FundingSourceEvidenceReasonV1::FundingLaneUnavailable
        }
        FscExcludedReason::IndexCold => FundingSourceEvidenceReasonV1::IndexCold,
        FscExcludedReason::NoBuyerCohort => FundingSourceEvidenceReasonV1::NoBuyerCohort,
        FscExcludedReason::InsufficientNonNeutralSupport => {
            FundingSourceEvidenceReasonV1::InsufficientNonNeutralSupport
        }
        FscExcludedReason::LowCoverage => FundingSourceEvidenceReasonV1::LowCoverage,
        FscExcludedReason::NeutralOnly => FundingSourceEvidenceReasonV1::NeutralOnly,
        FscExcludedReason::SameSlotOrderingUnavailable => {
            FundingSourceEvidenceReasonV1::SameSlotOrderingUnavailable
        }
        FscExcludedReason::LowAttributionConfidence => {
            FundingSourceEvidenceReasonV1::LowAttributionConfidence
        }
    })
}

/// Convert a legacy free-text reason into a typed compatibility reason. Unknown
/// strings are preserved explicitly; they never disappear or become Clean.
pub fn adapt_legacy_metric_reason_v1(
    contract_id: MetricContractId,
    raw: &str,
) -> MetricEvidenceReasonV1 {
    let raw = raw.trim();
    match (contract_id, raw) {
        (MetricContractId::FeeTopologyDiversityIndex, FTDI_INSUFFICIENT_BUYS_REASON) => {
            MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::InsufficientBuyTransactions)
        }
        (MetricContractId::FeeTopologyDiversityIndex, FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON) => {
            MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::RawFeeTopologyUnavailable)
        }
        (MetricContractId::DevBuy, DBIA_NO_DEV_BUY_REASON) => {
            MetricEvidenceReasonV1::DevBuy(DevBuyEvidenceReasonV1::NoEligibleBuy)
        }
        (MetricContractId::FlipRatio, "FLIP_RATIO_NO_BUYERS") => {
            MetricEvidenceReasonV1::Flip(FlipEvidenceReasonV1::NoEligibleBuyers)
        }
        (MetricContractId::FlipRatio, "WALLET_CAP_REACHED")
        | (MetricContractId::FlipRatio, "OWNER_WALLET_CAP_REACHED") => {
            MetricEvidenceReasonV1::Flip(FlipEvidenceReasonV1::WalletCapReached)
        }
        (MetricContractId::FundingSourceConcentration, FSC_FUNDING_STREAM_UNAVAILABLE_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::FundingLaneUnavailable,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_ROLLING_STATE_UNAVAILABLE_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::RollingStateUnavailable,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_INSUFFICIENT_KNOWN_SOURCES_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::InsufficientKnownSources,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_BUYER_IDENTITY_UNAVAILABLE_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::BuyerIdentityUnavailable,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::BuyTimestampUnavailable,
            )
        }
        (
            MetricContractId::FundingSourceConcentration,
            FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
        ) => MetricEvidenceReasonV1::FundingSource(
            FundingSourceEvidenceReasonV1::NoRetainedRecipientHistory,
        ),
        (MetricContractId::FundingSourceConcentration, FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::LookbackWindowExhausted,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::NoPrebuyTransferInWindow,
            )
        }
        (
            MetricContractId::FundingSourceConcentration,
            FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
        ) => MetricEvidenceReasonV1::FundingSource(
            FundingSourceEvidenceReasonV1::SameSlotOrderingUnavailable,
        ),
        (MetricContractId::FundingSourceConcentration, FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::LowAttributionConfidence,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::AbsoluteAttributionTooSmall,
            )
        }
        (MetricContractId::FundingSourceConcentration, FSC_RELATIVE_FUNDING_TOO_SMALL_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::RelativeFundingTooSmall,
            )
        }
        (
            MetricContractId::FundingSourceConcentration,
            FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON,
        ) => MetricEvidenceReasonV1::FundingSource(
            FundingSourceEvidenceReasonV1::PerRecipientHistoryOverflow,
        ),
        (MetricContractId::FundingSourceConcentration, FSC_GLOBAL_RECIPIENT_EVICTED_REASON) => {
            MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::GlobalRecipientEvicted,
            )
        }
        (MetricContractId::ManipulationContradiction, "momentum_without_broadening") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::MomentumWithoutBroadening,
            )
        }
        (MetricContractId::ManipulationContradiction, "volume_spike_without_new_signers") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::VolumeSpikeWithoutNewSigners,
            )
        }
        (MetricContractId::ManipulationContradiction, "high_buy_pressure_with_high_top3") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::HighBuyPressureWithHighTop3,
            )
        }
        (MetricContractId::ManipulationContradiction, "fixed_size_or_ramping_pattern") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::FixedSizeOrRampingPattern,
            )
        }
        (MetricContractId::ManipulationContradiction, "timing_bundle_concentration") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::TimingBundleConcentration,
            )
        }
        (MetricContractId::ManipulationContradiction, "early_top3_concentration") => {
            MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::EarlyTop3Concentration,
            )
        }
        (MetricContractId::RecentBuySell, "rce_a0_not_evaluated_logging_only") => {
            MetricEvidenceReasonV1::RecentBuySell(RecentBuySellEvidenceReasonV1::LoggingOnly)
        }
        _ => MetricEvidenceReasonV1::UnmappedLegacyString {
            contract_id,
            raw: raw.to_string(),
        },
    }
}

#[must_use]
pub fn adapt_legacy_metric_reason_list_v1(
    contract_id: MetricContractId,
    raw_reasons: &[String],
) -> Vec<MetricEvidenceReasonV1> {
    raw_reasons
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(|raw| adapt_legacy_metric_reason_v1(contract_id, raw))
        .collect()
}
