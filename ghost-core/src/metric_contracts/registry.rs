use super::{CanonicalHashErrorV1, CanonicalHashV1};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use thiserror::Error;

pub const METRIC_CONTRACT_REGISTRY_ID_V1_1: &str = "metric_contracts_v1_1";
pub const METRIC_CONTRACT_SCHEMA_VERSION_V1: u16 = 1;
pub const METRIC_CONTRACT_PROFILE_A_ID: &str = "metric_contracts_v1_1_profile_a";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricContractId {
    FeeTopologyDiversityIndex,
    DevBuy,
    SameMsTxRatio,
    Top3SignerVolumeRatio,
    FlipRatio,
    FundingSourceConcentration,
    FscEvidenceStatus,
    ManipulationContradiction,
    ReserveVelocity,
    RecentBuySell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricSurfaceId {
    TxIntelFeeTopologyDiversityLegacy,
    FtdiValueEvidenceV1,
    FtdiLegacyBuyTxActionability,
    FtdiUniqueBuyerActionabilityV2,
    CoordinationFeeTopologyHhiExportV1,
    TxIntelDevFirstObservedBuySol,
    GatekeeperBufferDevPrimaryBuySol,
    MfsDevFirstObservedBuySol,
    MfsDevPrimaryBuySolV1,
    EffectivePolicyDevBuySol,
    TxIntelSameMsCollisionRatioExact,
    TxTimingExactSameMsEvidenceV1,
    TxIntelBundleClusterRatioLt50Ms,
    RceSameMsCollisionRatioRecentExact,
    TxIntelTop3SignerVolumeRatioPreferred,
    TxIntelTop3VolumePctCompatibilityAlias,
    TxIntelTop3EffectiveSelector,
    EarlyFingerprintFlipRatioLegacySlotGap,
    FlipRatioHybridEvidenceV2,
    TxIntelFundingSourceConcentrationLegacy,
    FundingSourceConcentrationLegacyEvidenceV1,
    FundingSourceV2ReadinessEvidence,
    MaterializedFscStatusCompatibility,
    CoordinationFundingSourceHhiExportV1,
    MfsManipulationNumericLegacyDefaults,
    ManipulationNumericEvidenceV2,
    MfsManipulationHighFlagsLegacyDefaults,
    PolicyDerivedManipulationHighFlagsV2,
    AccountStateReserveVelocityScalarLegacy,
    ReserveVelocityEvidenceV1,
    RceBuySellRatioRecentLegacy,
    RecentBuySellEvidenceV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricContractDefinitionV1 {
    pub id: MetricContractId,
    pub version: u16,
    pub canonical_name: &'static str,
    pub unit: &'static str,
    pub population_and_denominator: &'static str,
    pub interpretation: &'static str,
    pub surfaces: &'static [MetricSurfaceId],
}

const FTDI_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
    MetricSurfaceId::FtdiValueEvidenceV1,
    MetricSurfaceId::FtdiLegacyBuyTxActionability,
    MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
    MetricSurfaceId::CoordinationFeeTopologyHhiExportV1,
];
const DEV_BUY_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::TxIntelDevFirstObservedBuySol,
    MetricSurfaceId::GatekeeperBufferDevPrimaryBuySol,
    MetricSurfaceId::MfsDevFirstObservedBuySol,
    MetricSurfaceId::MfsDevPrimaryBuySolV1,
    MetricSurfaceId::EffectivePolicyDevBuySol,
];
const SAME_MS_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
    MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
    MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
    MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
];
const TOP3_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
    MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
    MetricSurfaceId::TxIntelTop3EffectiveSelector,
];
const FLIP_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
    MetricSurfaceId::FlipRatioHybridEvidenceV2,
];
const FSC_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
    MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1,
    MetricSurfaceId::FundingSourceV2ReadinessEvidence,
    MetricSurfaceId::CoordinationFundingSourceHhiExportV1,
];
const FSC_STATUS_SURFACES: &[MetricSurfaceId] =
    &[MetricSurfaceId::MaterializedFscStatusCompatibility];
const MANIPULATION_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
    MetricSurfaceId::ManipulationNumericEvidenceV2,
    MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
    MetricSurfaceId::PolicyDerivedManipulationHighFlagsV2,
];
const RESERVE_VELOCITY_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
    MetricSurfaceId::ReserveVelocityEvidenceV1,
];
const RECENT_BUY_SELL_SURFACES: &[MetricSurfaceId] = &[
    MetricSurfaceId::RceBuySellRatioRecentLegacy,
    MetricSurfaceId::RecentBuySellEvidenceV1,
];

/// Normative registry of the ten interpretation-repair contract families.
pub const METRIC_CONTRACTS_V1_1: [MetricContractDefinitionV1; 10] = [
    MetricContractDefinitionV1 {
        id: MetricContractId::FeeTopologyDiversityIndex,
        version: 1,
        canonical_name: "fee_topology_diversity_index",
        unit: "ratio_0_1",
        population_and_denominator:
            "successful BUY; first sample per unique signer; unique topologies / unique buyer samples",
        interpretation:
            "runtime topology diversity; coordination HHI is a distinct export-only surface",
        surfaces: FTDI_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::DevBuy,
        version: 1,
        canonical_name: "dev_buy",
        unit: "sol",
        population_and_denominator:
            "surface-qualified creator BUY selection; scalar amount has no denominator",
        interpretation:
            "legacy policy uses TxIntel first-observed creator BUY; primary creator BUY is counterfactual",
        surfaces: DEV_BUY_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::SameMsTxRatio,
        version: 1,
        canonical_name: "same_ms_tx_ratio",
        unit: "ratio_0_1",
        population_and_denominator:
            "source-qualified exact collisions or <50ms clusters; legacy exact denominator is transaction count",
        interpretation: "exact same-ms and sub-50ms clustering are different metrics",
        surfaces: SAME_MS_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::Top3SignerVolumeRatio,
        version: 1,
        canonical_name: "top3_signer_volume_ratio",
        unit: "ratio_0_1",
        population_and_denominator: "top-three signer absolute volume / total absolute signer volume",
        interpretation:
            "preferred ratio with legacy top3_volume_pct fallback; despite pct alias the scale is 0..1",
        surfaces: TOP3_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::FlipRatio,
        version: 1,
        canonical_name: "flip_ratio",
        unit: "ratio_0_1",
        population_and_denominator:
            "unique eligible buyer owners classified by an explicitly anchored sell state machine",
        interpretation:
            "legacy flip_ratio_10s is slot-gap based; hybrid wall-clock+slot V2 remains evidence-only",
        surfaces: FLIP_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::FundingSourceConcentration,
        version: 1,
        canonical_name: "funding_source_concentration",
        unit: "ratio_0_1_or_typed_v2",
        population_and_denominator:
            "legacy distinct-known source collision ratio; FSC v2 has independent coverage/readiness population",
        interpretation: "legacy scalar is not HHI or volume concentration; FSC v2 stays evidence-only",
        surfaces: FSC_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::FscEvidenceStatus,
        version: 1,
        canonical_name: "evidence_status.fsc",
        unit: "status",
        population_and_denominator: "legacy scalar availability and FSC v2 readiness are evaluated separately",
        interpretation:
            "legacy Clean cannot be used as proof that funding_source_v2 is measured or policy-actionable",
        surfaces: FSC_STATUS_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::ManipulationContradiction,
        version: 1,
        canonical_name: "manipulation_contradiction_features",
        unit: "presence_aware_numeric_and_derived_flags",
        population_and_denominator:
            "per-field presence plus source-qualified raw value; thresholds are policy-stage inputs",
        interpretation:
            "legacy default false/zero is not measurement proof; V2 flags are derived from present raw evidence",
        surfaces: MANIPULATION_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::ReserveVelocity,
        version: 1,
        canonical_name: "reserve_velocity_sol_per_sec",
        unit: "sol_per_second",
        population_and_denominator:
            "delta real SOL reserves / receive-time interval between two accepted account updates",
        interpretation: "per-update rate with explicit bootstrap/zero-delta/fallback status, not a continuous sampler",
        surfaces: RESERVE_VELOCITY_SURFACES,
    },
    MetricContractDefinitionV1 {
        id: MetricContractId::RecentBuySell,
        version: 1,
        canonical_name: "recent_buy_sell",
        unit: "counts_unbounded_ratio_and_bounded_share",
        population_and_denominator:
            "successful transactions in source-qualified recent window; raw buy and sell counts retained",
        interpretation:
            "buy/sell ratio is optional and unbounded; buy share is separately bounded; sell=0 is not a bounded ratio",
        surfaces: RECENT_BUY_SELL_SURFACES,
    },
];

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MetricContractRolloutMode {
    #[default]
    Legacy,
    DualCompute,
    V2,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum MetricContractProfileIdV1 {
    #[default]
    #[serde(rename = "metric_contracts_v1_1_profile_a")]
    MetricContractsV1_1ProfileA,
}

impl MetricContractProfileIdV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MetricContractsV1_1ProfileA => METRIC_CONTRACT_PROFILE_A_ID,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricAuthorityClass {
    Authoritative,
    EquivalentCutover,
    Compatibility,
    Counterfactual,
    EvidenceOnly,
    LoggingOnly,
    ExportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricRolloutRoleV1 {
    PolicyAuthoritative,
    PolicyComparator,
    NonPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricAuthorityAssignmentV1 {
    pub contract_id: MetricContractId,
    pub surface_id: MetricSurfaceId,
    pub authority_class: MetricAuthorityClass,
    pub legacy_role: MetricRolloutRoleV1,
    pub dual_compute_role: MetricRolloutRoleV1,
    pub v2_role: MetricRolloutRoleV1,
}

impl MetricAuthorityAssignmentV1 {
    #[must_use]
    pub const fn role_for(self, mode: MetricContractRolloutMode) -> MetricRolloutRoleV1 {
        match mode {
            MetricContractRolloutMode::Legacy => self.legacy_role,
            MetricContractRolloutMode::DualCompute => self.dual_compute_role,
            MetricContractRolloutMode::V2 => self.v2_role,
        }
    }
}

const AUTH: MetricRolloutRoleV1 = MetricRolloutRoleV1::PolicyAuthoritative;
const CMP: MetricRolloutRoleV1 = MetricRolloutRoleV1::PolicyComparator;
const NONE: MetricRolloutRoleV1 = MetricRolloutRoleV1::NonPolicy;

const fn assignment(
    contract_id: MetricContractId,
    surface_id: MetricSurfaceId,
    authority_class: MetricAuthorityClass,
    legacy_role: MetricRolloutRoleV1,
    dual_compute_role: MetricRolloutRoleV1,
    v2_role: MetricRolloutRoleV1,
) -> MetricAuthorityAssignmentV1 {
    MetricAuthorityAssignmentV1 {
        contract_id,
        surface_id,
        authority_class,
        legacy_role,
        dual_compute_role,
        v2_role,
    }
}

/// Profile A authority matrix. The global rollout mode selects a column; it
/// never promotes a `Counterfactual`, `EvidenceOnly`, `LoggingOnly`, or
/// `ExportOnly` surface to policy authority.
pub const METRIC_CONTRACT_PROFILE_A_ENTRIES_V1: [MetricAuthorityAssignmentV1; 32] = [
    assignment(
        MetricContractId::FeeTopologyDiversityIndex,
        MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        NONE,
    ),
    assignment(
        MetricContractId::FeeTopologyDiversityIndex,
        MetricSurfaceId::FtdiValueEvidenceV1,
        MetricAuthorityClass::EquivalentCutover,
        NONE,
        CMP,
        AUTH,
    ),
    assignment(
        MetricContractId::FeeTopologyDiversityIndex,
        MetricSurfaceId::FtdiLegacyBuyTxActionability,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::FeeTopologyDiversityIndex,
        MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
        MetricAuthorityClass::Counterfactual,
        NONE,
        CMP,
        CMP,
    ),
    assignment(
        MetricContractId::FeeTopologyDiversityIndex,
        MetricSurfaceId::CoordinationFeeTopologyHhiExportV1,
        MetricAuthorityClass::ExportOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::DevBuy,
        MetricSurfaceId::TxIntelDevFirstObservedBuySol,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::DevBuy,
        MetricSurfaceId::GatekeeperBufferDevPrimaryBuySol,
        MetricAuthorityClass::Compatibility,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::DevBuy,
        MetricSurfaceId::MfsDevFirstObservedBuySol,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::DevBuy,
        MetricSurfaceId::MfsDevPrimaryBuySolV1,
        MetricAuthorityClass::Counterfactual,
        NONE,
        CMP,
        CMP,
    ),
    assignment(
        MetricContractId::DevBuy,
        MetricSurfaceId::EffectivePolicyDevBuySol,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::SameMsTxRatio,
        MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        NONE,
    ),
    assignment(
        MetricContractId::SameMsTxRatio,
        MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
        MetricAuthorityClass::EquivalentCutover,
        NONE,
        CMP,
        AUTH,
    ),
    assignment(
        MetricContractId::SameMsTxRatio,
        MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
        MetricAuthorityClass::EvidenceOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::SameMsTxRatio,
        MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
        MetricAuthorityClass::LoggingOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::Top3SignerVolumeRatio,
        MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::Top3SignerVolumeRatio,
        MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
        MetricAuthorityClass::Compatibility,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::Top3SignerVolumeRatio,
        MetricSurfaceId::TxIntelTop3EffectiveSelector,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::FlipRatio,
        MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
        MetricAuthorityClass::Compatibility,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::FlipRatio,
        MetricSurfaceId::FlipRatioHybridEvidenceV2,
        MetricAuthorityClass::EvidenceOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::FundingSourceConcentration,
        MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        NONE,
    ),
    assignment(
        MetricContractId::FundingSourceConcentration,
        MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1,
        MetricAuthorityClass::EquivalentCutover,
        NONE,
        CMP,
        AUTH,
    ),
    assignment(
        MetricContractId::FundingSourceConcentration,
        MetricSurfaceId::FundingSourceV2ReadinessEvidence,
        MetricAuthorityClass::EvidenceOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::FscEvidenceStatus,
        MetricSurfaceId::MaterializedFscStatusCompatibility,
        MetricAuthorityClass::Compatibility,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::FundingSourceConcentration,
        MetricSurfaceId::CoordinationFundingSourceHhiExportV1,
        MetricAuthorityClass::ExportOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::ManipulationContradiction,
        MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::ManipulationContradiction,
        MetricSurfaceId::ManipulationNumericEvidenceV2,
        MetricAuthorityClass::EquivalentCutover,
        NONE,
        CMP,
        CMP,
    ),
    assignment(
        MetricContractId::ManipulationContradiction,
        MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
        MetricAuthorityClass::Authoritative,
        AUTH,
        AUTH,
        AUTH,
    ),
    assignment(
        MetricContractId::ManipulationContradiction,
        MetricSurfaceId::PolicyDerivedManipulationHighFlagsV2,
        MetricAuthorityClass::EquivalentCutover,
        NONE,
        CMP,
        CMP,
    ),
    assignment(
        MetricContractId::ReserveVelocity,
        MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
        MetricAuthorityClass::EvidenceOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::ReserveVelocity,
        MetricSurfaceId::ReserveVelocityEvidenceV1,
        MetricAuthorityClass::EvidenceOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::RecentBuySell,
        MetricSurfaceId::RceBuySellRatioRecentLegacy,
        MetricAuthorityClass::LoggingOnly,
        NONE,
        NONE,
        NONE,
    ),
    assignment(
        MetricContractId::RecentBuySell,
        MetricSurfaceId::RecentBuySellEvidenceV1,
        MetricAuthorityClass::LoggingOnly,
        NONE,
        NONE,
        NONE,
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractRegistryHashDefinitionV1 {
    pub id: MetricContractId,
    pub version: u16,
    pub canonical_name: String,
    pub unit: String,
    pub population_and_denominator: String,
    pub interpretation: String,
    pub surfaces: Vec<MetricSurfaceId>,
}

impl From<&MetricContractDefinitionV1> for MetricContractRegistryHashDefinitionV1 {
    fn from(definition: &MetricContractDefinitionV1) -> Self {
        Self {
            id: definition.id,
            version: definition.version,
            canonical_name: definition.canonical_name.to_string(),
            unit: definition.unit.to_string(),
            population_and_denominator: definition.population_and_denominator.to_string(),
            interpretation: definition.interpretation.to_string(),
            surfaces: definition.surfaces.to_vec(),
        }
    }
}

fn compiled_registry_hash_definitions_v1() -> Vec<MetricContractRegistryHashDefinitionV1> {
    let mut definitions = METRIC_CONTRACTS_V1_1
        .iter()
        .map(MetricContractRegistryHashDefinitionV1::from)
        .collect::<Vec<_>>();
    definitions.sort_by_key(|definition| definition.id);
    definitions
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProfileHashPayloadV1 {
    pub registry_id: String,
    pub schema_version: u16,
    pub profile_id: MetricContractProfileIdV1,
    pub registry_contracts: Vec<MetricContractRegistryHashDefinitionV1>,
    pub entries: Vec<MetricAuthorityAssignmentV1>,
}

impl MetricContractProfileHashPayloadV1 {
    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, CanonicalHashErrorV1> {
        CanonicalHashV1::digest(self)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MetricContractProfileErrorV1 {
    #[error("unknown metric contract registry id: {0}")]
    UnknownRegistry(String),
    #[error("metric contract profile contains duplicate surface: {0:?}")]
    DuplicateSurface(MetricSurfaceId),
    #[error("metric contract profile entries must use canonical contract/surface order")]
    NonCanonicalEntryOrder,
    #[error("metric contract profile is missing surface: {0:?}")]
    MissingSurface(MetricSurfaceId),
    #[error(
        "metric contract registry assigns surface {surface:?} to both {first:?} and {second:?}"
    )]
    DuplicateRegistrySurface {
        surface: MetricSurfaceId,
        first: MetricContractId,
        second: MetricContractId,
    },
    #[error("metric contract profile references a surface absent from the registry: {0:?}")]
    UnknownSurface(MetricSurfaceId),
    #[error("unsupported metric contract profile schema: {0}")]
    UnsupportedSchema(u16),
    #[error(
        "metric contract profile registry snapshot does not match compiled registry semantics"
    )]
    RegistrySemanticMismatch,
    #[error("surface {surface:?} is assigned to {actual:?}, expected {expected:?}")]
    SurfaceContractMismatch {
        surface: MetricSurfaceId,
        actual: MetricContractId,
        expected: MetricContractId,
    },
    #[error("legacy mode cannot promote non-authoritative surface {0:?}")]
    InvalidLegacyAuthority(MetricSurfaceId),
    #[error("dual-compute cannot change active authority for surface {0:?}")]
    InvalidDualComputeAuthority(MetricSurfaceId),
    #[error("V2 promotion is allowed only for EquivalentCutover surface {0:?}")]
    InvalidV2Promotion(MetricSurfaceId),
    #[error("non-policy authority class cannot be policy-authoritative: {0:?}")]
    NonPolicyClassPromoted(MetricSurfaceId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricContractProfileV1 {
    payload: MetricContractProfileHashPayloadV1,
}

impl MetricContractProfileV1 {
    pub fn profile_a() -> Result<Self, MetricContractProfileErrorV1> {
        let mut entries = METRIC_CONTRACT_PROFILE_A_ENTRIES_V1.to_vec();
        entries.sort_by_key(|entry| (entry.contract_id, entry.surface_id));
        Self::try_from_payload(MetricContractProfileHashPayloadV1 {
            registry_id: METRIC_CONTRACT_REGISTRY_ID_V1_1.to_string(),
            schema_version: METRIC_CONTRACT_SCHEMA_VERSION_V1,
            profile_id: MetricContractProfileIdV1::MetricContractsV1_1ProfileA,
            registry_contracts: compiled_registry_hash_definitions_v1(),
            entries,
        })
    }

    pub fn try_from_payload(
        payload: MetricContractProfileHashPayloadV1,
    ) -> Result<Self, MetricContractProfileErrorV1> {
        let profile = Self { payload };
        profile.validate()?;
        Ok(profile)
    }

    #[must_use]
    pub fn payload(&self) -> &MetricContractProfileHashPayloadV1 {
        &self.payload
    }

    pub fn canonical_hash(&self) -> Result<CanonicalHashV1, CanonicalHashErrorV1> {
        self.payload.canonical_hash()
    }

    #[must_use]
    pub fn entry_for(&self, surface: MetricSurfaceId) -> Option<&MetricAuthorityAssignmentV1> {
        self.payload
            .entries
            .iter()
            .find(|entry| entry.surface_id == surface)
    }

    pub fn validate(&self) -> Result<(), MetricContractProfileErrorV1> {
        if self.payload.registry_id != METRIC_CONTRACT_REGISTRY_ID_V1_1 {
            return Err(MetricContractProfileErrorV1::UnknownRegistry(
                self.payload.registry_id.clone(),
            ));
        }
        if self.payload.schema_version != METRIC_CONTRACT_SCHEMA_VERSION_V1 {
            return Err(MetricContractProfileErrorV1::UnsupportedSchema(
                self.payload.schema_version,
            ));
        }
        if self.payload.registry_contracts != compiled_registry_hash_definitions_v1() {
            return Err(MetricContractProfileErrorV1::RegistrySemanticMismatch);
        }
        let mut expected_contract_by_surface = std::collections::BTreeMap::new();
        for contract in &METRIC_CONTRACTS_V1_1 {
            for surface in contract.surfaces {
                if let Some(first) = expected_contract_by_surface.insert(*surface, contract.id) {
                    return Err(MetricContractProfileErrorV1::DuplicateRegistrySurface {
                        surface: *surface,
                        first,
                        second: contract.id,
                    });
                }
            }
        }
        let mut seen = HashSet::new();

        for entry in &self.payload.entries {
            if !seen.insert(entry.surface_id) {
                return Err(MetricContractProfileErrorV1::DuplicateSurface(
                    entry.surface_id,
                ));
            }
            let Some(expected_contract) = expected_contract_by_surface.get(&entry.surface_id)
            else {
                return Err(MetricContractProfileErrorV1::UnknownSurface(
                    entry.surface_id,
                ));
            };
            if *expected_contract != entry.contract_id {
                return Err(MetricContractProfileErrorV1::SurfaceContractMismatch {
                    surface: entry.surface_id,
                    actual: entry.contract_id,
                    expected: *expected_contract,
                });
            }

            if entry.legacy_role == AUTH
                && entry.authority_class != MetricAuthorityClass::Authoritative
            {
                return Err(MetricContractProfileErrorV1::InvalidLegacyAuthority(
                    entry.surface_id,
                ));
            }
            if (entry.dual_compute_role == AUTH) != (entry.legacy_role == AUTH) {
                return Err(MetricContractProfileErrorV1::InvalidDualComputeAuthority(
                    entry.surface_id,
                ));
            }
            if entry.v2_role == AUTH
                && entry.legacy_role != AUTH
                && entry.authority_class != MetricAuthorityClass::EquivalentCutover
            {
                return Err(MetricContractProfileErrorV1::InvalidV2Promotion(
                    entry.surface_id,
                ));
            }
            if (entry.role_for(MetricContractRolloutMode::Legacy) == AUTH
                || entry.role_for(MetricContractRolloutMode::DualCompute) == AUTH
                || entry.role_for(MetricContractRolloutMode::V2) == AUTH)
                && matches!(
                    entry.authority_class,
                    MetricAuthorityClass::Compatibility
                        | MetricAuthorityClass::Counterfactual
                        | MetricAuthorityClass::EvidenceOnly
                        | MetricAuthorityClass::LoggingOnly
                        | MetricAuthorityClass::ExportOnly
                )
            {
                return Err(MetricContractProfileErrorV1::NonPolicyClassPromoted(
                    entry.surface_id,
                ));
            }
        }

        if !self.payload.entries.windows(2).all(|window| {
            (window[0].contract_id, window[0].surface_id)
                < (window[1].contract_id, window[1].surface_id)
        }) {
            return Err(MetricContractProfileErrorV1::NonCanonicalEntryOrder);
        }

        let seen = seen.into_iter().collect::<BTreeSet<_>>();
        for surface in expected_contract_by_surface.keys() {
            if !seen.contains(surface) {
                return Err(MetricContractProfileErrorV1::MissingSurface(*surface));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractFoundationConfigV1 {
    #[serde(default)]
    pub metric_contract_rollout_mode: MetricContractRolloutMode,
    #[serde(default)]
    pub metric_contract_profile: MetricContractProfileIdV1,
}

impl Default for MetricContractFoundationConfigV1 {
    fn default() -> Self {
        Self {
            metric_contract_rollout_mode: MetricContractRolloutMode::Legacy,
            metric_contract_profile: MetricContractProfileIdV1::MetricContractsV1_1ProfileA,
        }
    }
}

impl MetricContractFoundationConfigV1 {
    pub fn resolve_profile(&self) -> Result<MetricContractProfileV1, MetricContractProfileErrorV1> {
        match self.metric_contract_profile {
            MetricContractProfileIdV1::MetricContractsV1_1ProfileA => {
                MetricContractProfileV1::profile_a()
            }
        }
    }
}
