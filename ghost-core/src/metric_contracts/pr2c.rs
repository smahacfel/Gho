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

/// Durable, content-addressed input for the PR2C policy-equivalence lane.
///
/// The compact v34 row stores only the derived delta vector.  These two
/// normalized snapshots and their exact policy/config provenance live in the
/// semantic evidence hash so replay can independently derive that vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractPolicyEquivalenceEvidenceV1 {
    pub policy_version: String,
    pub gatekeeper_config_hash: CanonicalHashV1,
    pub comparator_evaluable: bool,
    pub authoritative: MetricContractPolicyEquivalenceSnapshotV1,
    pub comparator: MetricContractPolicyEquivalenceSnapshotV1,
}

impl MetricContractPolicyEquivalenceEvidenceV1 {
    pub fn validate(&self) -> Result<(), MetricContractPairErrorV1> {
        fn valid_snapshot(snapshot: &MetricContractPolicyEquivalenceSnapshotV1) -> bool {
            !snapshot.verdict.trim().is_empty()
                && !snapshot.primary_reason_code.trim().is_empty()
                && !snapshot.hard_fail_classification.trim().is_empty()
                && snapshot.phase_pass_vector.len() == 6
                && snapshot.ordered_reason_chain.len() <= 32
        }

        if self.policy_version.trim().is_empty()
            || self.policy_version.len() > 64
            || !valid_snapshot(&self.authoritative)
            || !valid_snapshot(&self.comparator)
        {
            return Err(MetricContractPairErrorV1::PolicyEvidenceInvariant);
        }
        Ok(())
    }

    #[must_use]
    pub fn recompute_deltas(&self) -> MetricContractComparatorSummaryV1 {
        if self.comparator_evaluable {
            self.authoritative.compare(&self.comparator)
        } else {
            MetricContractPolicyEquivalenceSnapshotV1::not_evaluable_comparison()
        }
    }
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
    /// Monotonic origin captured before the first canonical producer call.
    /// The paired writer reads this only after constructing the exact final
    /// v34/evidence bytes. It is not durable semantic or transport data.
    pub metric_contract_full_path_started: std::time::Instant,
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
    #[error("durable policy-equivalence evidence is invalid")]
    PolicyEvidenceInvariant,
    #[error("v34 equivalence deltas do not match durable policy snapshots")]
    EquivalenceDeltaMismatch,
}

impl MetricContractPairedRecordV1 {
    pub fn validate_pair(&self) -> Result<(), MetricContractPairErrorV1> {
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
        if self.effective_config.validate_hash().is_err()
            || self.effective_config.metric_contract_effective_config_hash
                != self.decision_v34.metric_contract_effective_config_hash
            || CanonicalHashV1::parse(&self.gatekeeper_config_hash)
                .ok()
                .as_ref()
                != Some(
                    &self
                        .evidence
                        .payload
                        .policy_equivalence
                        .gatekeeper_config_hash,
                )
        {
            return Err(MetricContractPairErrorV1::ProvenanceMismatch);
        }
        self.evidence.payload.policy_equivalence.validate()?;
        if self.decision_v34.equivalence_deltas
            != self.evidence.payload.policy_equivalence.recompute_deltas()
        {
            return Err(MetricContractPairErrorV1::EquivalenceDeltaMismatch);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricContractCutoverScopeV1 {
    #[serde(rename = "metric_contracts_v1_1_profile_a_equivalence_only")]
    MetricContractsV1_1ProfileAEquivalenceOnly,
}
