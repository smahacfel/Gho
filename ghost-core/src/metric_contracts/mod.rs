//! Shared foundation for the `metric_contracts_v1_1` evidence plane.
//!
//! This module deliberately contains contracts and validation only. PR1 does
//! not attach these types to [`crate::checkpoint::MaterializedFeatureSet`],
//! does not change Gatekeeper authority, and does not activate decision schema
//! v34. Producers and durable emission are introduced by later, separately
//! accepted milestones.

mod canonical_hash;
mod effective_config;
mod evidence;
mod identity;
mod projection;
mod registry;
mod status;

pub use canonical_hash::{
    canonical_jcs_bytes_v1, CanonicalHashErrorV1, CanonicalHashV1, CanonicalI64StringV1,
    CanonicalNullableV1, CanonicalU128StringV1, CanonicalU64StringV1,
};
pub use effective_config::{
    metric_contract_effective_config_hash, MetricContractEffectiveConfigBuilderV1,
    MetricContractEffectiveConfigErrorV1, MetricContractEffectiveConfigHashPayloadV1,
    MetricEffectiveConfigEntryV1, MetricEffectiveConfigKeyV1, MetricEffectiveConfigValueKindV1,
    MetricEffectiveConfigValueV1, ResolvedMetricContractEffectiveConfigV1,
    METRIC_EFFECTIVE_CONFIG_KEYS_V1,
};
pub use evidence::*;
pub use identity::*;
pub use projection::*;
pub use registry::*;
pub use status::*;
