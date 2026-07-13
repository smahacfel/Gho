use super::{
    metric_contract_projection_wire_v1_mapping_tables,
    metric_contract_projection_wire_v1_tuple_layouts, CanonicalHashV1, ComparatorDeltaStatusV1,
    MetricContractComparatorSummaryV1, MetricContractDecisionEvidenceProjectionV1,
    MetricContractDecisionSummaryV1, MetricContractEvidenceTransportV1,
    MetricEvidenceRecordIdentityV1, ResolvedMetricContractEffectiveConfigV1, StableEventIdentityV1,
    METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const METRIC_CONTRACT_WIRE_V1_TUPLE_TABLE_COUNT: usize = 18;
pub const METRIC_CONTRACT_WIRE_V1_MAPPING_TABLE_COUNT: usize = 28;
pub const METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3: &str =
    "70d79931f3f9a82720e46f622d439930a087431e305d14c02d88dcd26568fc7f";
pub const PR2C_COMPARATOR_P99_MAX_US: u32 = 1_000;
pub const PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US: u32 = 5_000;
pub const PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US: u32 = 5_000;
pub const PR2C_SERIALIZE_P99_MAX_US: u32 = 1_000;
pub const PR2C_LOGGER_ENQUEUE_WAIT_P99_MAX_US: u32 = 1_000;
pub const BURN_IN_CONTRACT_V1_CANONICAL_HASH: &str =
    "40872b8c1ab8fcd8ecb4b1612e35fcf9dc157cbb1109546c7490c7d006f00ffd";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProjectionWireV1LayoutEntry {
    pub position: u16,
    pub domain_field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProjectionWireV1LayoutTable {
    pub name: String,
    pub entries: Vec<MetricContractProjectionWireV1LayoutEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProjectionWireV1MappingEntry {
    pub code: u16,
    pub domain_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProjectionWireV1MappingTable {
    pub name: String,
    pub entries: Vec<MetricContractProjectionWireV1MappingEntry>,
}

/// Complete ordered codebook for Compact JSON Wire V1. Both outer table order
/// and every inner position/code are semantic. Any change requires Wire V2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractProjectionWireV1SchemaManifest {
    pub wire_version: u16,
    pub tuple_layouts: Vec<MetricContractProjectionWireV1LayoutTable>,
    pub mapping_tables: Vec<MetricContractProjectionWireV1MappingTable>,
}

impl MetricContractProjectionWireV1SchemaManifest {
    #[must_use]
    pub fn current() -> Self {
        let tuple_layouts = metric_contract_projection_wire_v1_tuple_layouts()
            .iter()
            .map(
                |(name, entries)| MetricContractProjectionWireV1LayoutTable {
                    name: (*name).to_string(),
                    entries: entries
                        .iter()
                        .enumerate()
                        .map(
                            |(position, field)| MetricContractProjectionWireV1LayoutEntry {
                                position: u16::try_from(position)
                                    .expect("Wire V1 tuple layouts are bounded below u16::MAX"),
                                domain_field: (*field).to_string(),
                            },
                        )
                        .collect(),
                },
            )
            .collect();
        let mapping_tables = metric_contract_projection_wire_v1_mapping_tables()
            .iter()
            .map(
                |(name, entries)| MetricContractProjectionWireV1MappingTable {
                    name: (*name).to_string(),
                    entries: entries
                        .iter()
                        .enumerate()
                        .map(|(code, value)| MetricContractProjectionWireV1MappingEntry {
                            code: u16::try_from(code)
                                .expect("Wire V1 enum tables are bounded below u16::MAX"),
                            domain_value: (*value).to_string(),
                        })
                        .collect(),
                },
            )
            .collect();
        Self {
            wire_version: METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
            tuple_layouts,
            mapping_tables,
        }
    }

    pub fn canonical_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json_canonicalizer::to_vec(self)
    }

    pub fn blake3_hex(&self) -> Result<String, serde_json::Error> {
        Ok(blake3::hash(&self.canonical_json()?).to_hex().to_string())
    }

    #[must_use]
    pub fn has_closed_table_counts(&self) -> bool {
        self.wire_version == METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1
            && self.tuple_layouts.len() == METRIC_CONTRACT_WIRE_V1_TUPLE_TABLE_COUNT
            && self.mapping_tables.len() == METRIC_CONTRACT_WIRE_V1_MAPPING_TABLE_COUNT
            && self
                .tuple_layouts
                .iter()
                .map(|table| table.name.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == self.tuple_layouts.len()
            && self
                .mapping_tables
                .iter()
                .map(|table| table.name.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == self.mapping_tables.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractPolicyEquivalenceSnapshotV1 {
    pub verdict: String,
    pub primary_reason_code: String,
    pub ordered_reason_chain: Vec<String>,
    pub phase_pass_vector: Vec<bool>,
    pub soft_points: i64,
    pub selector_soft_score_bits: u64,
    pub hard_fail_classification: String,
}

impl MetricContractPolicyEquivalenceSnapshotV1 {
    #[must_use]
    pub fn compare(&self, candidate: &Self) -> MetricContractComparatorSummaryV1 {
        fn delta<T: PartialEq>(left: &T, right: &T) -> ComparatorDeltaStatusV1 {
            if left == right {
                ComparatorDeltaStatusV1::Equal
            } else {
                ComparatorDeltaStatusV1::Different
            }
        }
        MetricContractComparatorSummaryV1 {
            verdict: delta(&self.verdict, &candidate.verdict),
            primary_reason_code: delta(&self.primary_reason_code, &candidate.primary_reason_code),
            ordered_reason_chain: delta(
                &self.ordered_reason_chain,
                &candidate.ordered_reason_chain,
            ),
            phase_pass_vector: delta(&self.phase_pass_vector, &candidate.phase_pass_vector),
            soft_points: delta(&self.soft_points, &candidate.soft_points),
            selector_soft_score: delta(
                &self.selector_soft_score_bits,
                &candidate.selector_soft_score_bits,
            ),
            hard_fail_classification: delta(
                &self.hard_fail_classification,
                &candidate.hard_fail_classification,
            ),
        }
    }

    /// A missing authoritative or comparator evaluation is durable evidence
    /// of non-evaluability, never proof of equality and never policy drift.
    #[must_use]
    pub const fn not_evaluable_comparison() -> MetricContractComparatorSummaryV1 {
        MetricContractComparatorSummaryV1 {
            verdict: ComparatorDeltaStatusV1::NotEvaluable,
            primary_reason_code: ComparatorDeltaStatusV1::NotEvaluable,
            ordered_reason_chain: ComparatorDeltaStatusV1::NotEvaluable,
            phase_pass_vector: ComparatorDeltaStatusV1::NotEvaluable,
            soft_points: ComparatorDeltaStatusV1::NotEvaluable,
            selector_soft_score: ComparatorDeltaStatusV1::NotEvaluable,
            hard_fail_classification: ComparatorDeltaStatusV1::NotEvaluable,
        }
    }
}

impl MetricContractComparatorSummaryV1 {
    #[must_use]
    pub fn is_zero_drift(&self) -> bool {
        [
            self.verdict,
            self.primary_reason_code,
            self.ordered_reason_chain,
            self.phase_pass_vector,
            self.soft_points,
            self.selector_soft_score,
            self.hard_fail_classification,
        ]
        .into_iter()
        .all(|status| status == ComparatorDeltaStatusV1::Equal)
    }

    #[must_use]
    pub fn has_policy_drift(&self) -> bool {
        [
            self.verdict,
            self.primary_reason_code,
            self.ordered_reason_chain,
            self.phase_pass_vector,
            self.soft_points,
            self.selector_soft_score,
            self.hard_fail_classification,
        ]
        .into_iter()
        .any(|status| status == ComparatorDeltaStatusV1::Different)
    }

    #[must_use]
    pub fn is_not_evaluable(&self) -> bool {
        [
            self.verdict,
            self.primary_reason_code,
            self.ordered_reason_chain,
            self.phase_pass_vector,
            self.soft_points,
            self.selector_soft_score,
            self.hard_fail_classification,
        ]
        .into_iter()
        .any(|status| status == ComparatorDeltaStatusV1::NotEvaluable)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricContractPairedRecordV1 {
    pub decision_v34: MetricContractDecisionSummaryV1,
    pub evidence: MetricContractEvidenceTransportV1,
    pub decision_time_projection: MetricContractDecisionEvidenceProjectionV1,
    pub decision_time_projection_hash: CanonicalHashV1,
    pub stable_event_identity: Option<StableEventIdentityV1>,
    /// Runtime-only resource samples. They are carried to the bounded writer
    /// manifest and never enter v34, the evidence hash or projection hash.
    pub metric_contract_build_and_serialize_us: u32,
    pub projection_build_and_validate_us: u32,
    pub gatekeeper_config_hash: String,
    pub brain_config_hash: Option<String>,
    /// In-memory/run-manifest replay context. This is never copied into the
    /// compact v34 row or full-evidence JSONL row.
    pub effective_config: ResolvedMetricContractEffectiveConfigV1,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MetricContractPairErrorV1 {
    #[error("v34/evidence record identity mismatch")]
    RecordIdentityMismatch,
    #[error("v34/evidence SHA mismatch")]
    EvidenceHashMismatch,
    #[error("v34/evidence schema mismatch")]
    EvidenceSchemaMismatch,
    #[error("v34/evidence profile or effective-config provenance mismatch")]
    ProvenanceMismatch,
}

impl MetricContractPairedRecordV1 {
    pub fn validate_pair(&self) -> Result<(), MetricContractPairErrorV1> {
        self.validate_pair_common(true)
    }

    /// Structural paired-record validation for a caller holding a typed proof
    /// that the exact effective config was already hash-validated. Durable
    /// deserialization and replay must continue to call `validate_pair()`.
    pub fn validate_pair_with_prevalidated_effective_config(
        &self,
    ) -> Result<(), MetricContractPairErrorV1> {
        self.validate_pair_common(false)
    }

    fn validate_pair_common(
        &self,
        revalidate_effective_config_hash: bool,
    ) -> Result<(), MetricContractPairErrorV1> {
        if self.decision_v34.evidence_record_id != self.evidence.payload.record_identity {
            return Err(MetricContractPairErrorV1::RecordIdentityMismatch);
        }
        if self.decision_v34.evidence_sha256 != self.evidence.evidence_sha256 {
            return Err(MetricContractPairErrorV1::EvidenceHashMismatch);
        }
        if self.decision_v34.evidence_schema_version
            != self.evidence.payload.evidence_schema_version
        {
            return Err(MetricContractPairErrorV1::EvidenceSchemaMismatch);
        }
        if self.decision_v34.rollout_mode != self.evidence.payload.rollout_mode
            || self.decision_v34.profile_id != self.evidence.payload.profile_id
            || self.decision_v34.profile_hash != self.evidence.payload.profile_hash
            || self.decision_v34.metric_contract_effective_config_hash
                != self.evidence.payload.metric_contract_effective_config_hash
        {
            return Err(MetricContractPairErrorV1::ProvenanceMismatch);
        }
        if (revalidate_effective_config_hash && self.effective_config.validate_hash().is_err())
            || self.effective_config.metric_contract_effective_config_hash
                != self.decision_v34.metric_contract_effective_config_hash
            || self.gatekeeper_config_hash.trim().is_empty()
        {
            return Err(MetricContractPairErrorV1::ProvenanceMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn record_identity(&self) -> &MetricEvidenceRecordIdentityV1 {
        &self.evidence.payload.record_identity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MetricContractAuditTerminalClassV1 {
    PassCutoverReady,
    NotEvaluable,
    FailSchemaOrReplay,
    FailPolicyDrift,
    FailResourceBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BurnInResourceLimitsV1 {
    pub comparator_p99_us: u32,
    pub metric_contract_build_and_serialize_p99_us: u32,
    pub projection_build_and_validate_p99_us: u32,
    pub logger_enqueue_wait_p99_us: u32,
    pub writer_queue_high_water_max_ratio: f64,
    pub projection_p95_bytes: u32,
    pub projection_hard_max_bytes: u32,
    pub sidecar_p95_bytes: u32,
    pub sidecar_p99_bytes: u32,
    pub combined_gb_per_hour_delta_max_ratio: f64,
    pub v34_p95_increase_max_bytes: u32,
    pub v34_p95_increase_max_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BurnInContractPayloadV1 {
    pub burn_in_contract_version: u16,
    pub minimum_non_overlapping_runs: u16,
    pub minimum_run_duration_ms: u64,
    pub minimum_utc_4h_buckets: u16,
    pub minimum_aggregate_duration_ms: u64,
    pub minimum_unique_decisions: u64,
    pub minimum_dev_known_decisions: u64,
    pub minimum_clean_flip_v2_evaluable: u64,
    pub minimum_real_dev_legacy_v2_divergences: u64,
    pub metric_contract_schema_version: u16,
    pub projection_wire_version: u16,
    pub evidence_schema_version: u16,
    pub decision_schema_version: u32,
    pub wire_schema_manifest_blake3: String,
    pub resource_limits: BurnInResourceLimitsV1,
    pub require_zero_policy_drift: bool,
    pub require_zero_dropped_rows: bool,
    pub require_zero_writer_failures: bool,
    pub require_zero_orphans: bool,
    pub invalidation_rules: Vec<String>,
    pub frozen_at: String,
    pub owner_approval_identity: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BurnInContractV1 {
    pub payload: BurnInContractPayloadV1,
    pub contract_canonical_hash: CanonicalHashV1,
}

#[derive(Debug, Error)]
pub enum BurnInContractErrorV1 {
    #[error(transparent)]
    Hash(#[from] super::CanonicalHashErrorV1),
    #[error("BURN_IN_CONTRACT_V1 canonical hash mismatch")]
    HashMismatch,
    #[error("BURN_IN_CONTRACT_V1 contains an invalid frozen gate")]
    InvalidGate,
}

impl BurnInContractV1 {
    pub fn try_new(payload: BurnInContractPayloadV1) -> Result<Self, BurnInContractErrorV1> {
        if payload.burn_in_contract_version == 0
            || payload.minimum_non_overlapping_runs < 3
            || payload.minimum_run_duration_ms < 3_600_000
            || payload.minimum_utc_4h_buckets < 2
            || payload.frozen_at.trim().is_empty()
            || payload.owner_approval_identity.trim().is_empty()
            || payload.wire_schema_manifest_blake3.len() != 64
            || payload.metric_contract_schema_version
                != super::METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1
            || payload.projection_wire_version
                != super::METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1
            || payload.evidence_schema_version != super::METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1
            || payload.decision_schema_version != super::METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34
            || payload.wire_schema_manifest_blake3
                != METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3
            || payload.resource_limits.comparator_p99_us != PR2C_COMPARATOR_P99_MAX_US
            || payload
                .resource_limits
                .metric_contract_build_and_serialize_p99_us
                != PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US
            || payload.resource_limits.projection_build_and_validate_p99_us
                != PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US
            || payload.resource_limits.logger_enqueue_wait_p99_us
                != PR2C_LOGGER_ENQUEUE_WAIT_P99_MAX_US
            || payload.resource_limits.writer_queue_high_water_max_ratio != 0.8
            || payload.resource_limits.projection_p95_bytes != 12 * 1_024
            || payload.resource_limits.projection_hard_max_bytes != 16 * 1_024
            || payload.resource_limits.sidecar_p95_bytes != 24 * 1_024
            || payload.resource_limits.sidecar_p99_bytes != 48 * 1_024
            || payload.resource_limits.combined_gb_per_hour_delta_max_ratio != 0.25
            || payload.resource_limits.v34_p95_increase_max_bytes != 8 * 1_024
            || payload.resource_limits.v34_p95_increase_max_ratio != 0.10
            || !payload.require_zero_policy_drift
            || !payload.require_zero_dropped_rows
            || !payload.require_zero_writer_failures
            || !payload.require_zero_orphans
        {
            return Err(BurnInContractErrorV1::InvalidGate);
        }
        let contract_canonical_hash = CanonicalHashV1::digest(&payload)?;
        if contract_canonical_hash.as_str() != BURN_IN_CONTRACT_V1_CANONICAL_HASH {
            return Err(BurnInContractErrorV1::InvalidGate);
        }
        Ok(Self {
            payload,
            contract_canonical_hash,
        })
    }

    pub fn validate_hash(&self) -> Result<(), BurnInContractErrorV1> {
        let rebuilt = Self::try_new(self.payload.clone())?;
        if rebuilt.contract_canonical_hash != self.contract_canonical_hash {
            return Err(BurnInContractErrorV1::HashMismatch);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBurnInContractV1 {
    payload: BurnInContractPayloadV1,
    contract_canonical_hash: CanonicalHashV1,
}

impl<'de> Deserialize<'de> for BurnInContractV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawBurnInContractV1::deserialize(deserializer)?;
        let contract = Self::try_new(raw.payload).map_err(serde::de::Error::custom)?;
        if contract.contract_canonical_hash != raw.contract_canonical_hash {
            return Err(serde::de::Error::custom(
                BurnInContractErrorV1::HashMismatch,
            ));
        }
        Ok(contract)
    }
}
