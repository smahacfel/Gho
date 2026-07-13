use super::{Pr2bCompleteMetricContractSnapshotV1, Pr2bTimedCompleteMetricContractSnapshotV1};
use ghost_core::metric_contracts::{
    CanonicalHashV1, CanonicalNullableV1, MetricContractDecisionSummaryV1,
    MetricContractEvidenceHashPayloadV1, MetricContractEvidenceTransportErrorV1,
    MetricContractEvidenceTransportV1, MetricContractId, MetricContractPairedRecordV1,
    MetricContractPolicyEquivalenceSnapshotV1, MetricContractProfileV1,
    MetricContractProjectionErrorV1, MetricContractRolloutMode,
    MetricDecisionProjectionBuildContextV1, MetricEvidenceRecordIdentityV1, MetricRolloutRoleV1,
    ResolvedMetricContractEffectiveConfigV1, StableEventIdentityV1,
    METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34, METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Pr2cDecisionRecordContextV1<'a> {
    pub record_identity: MetricEvidenceRecordIdentityV1,
    pub stable_event_identity: Option<StableEventIdentityV1>,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile: &'a MetricContractProfileV1,
    pub effective_config: &'a ResolvedMetricContractEffectiveConfigV1,
    pub authoritative_policy: &'a MetricContractPolicyEquivalenceSnapshotV1,
    pub comparator_policy: &'a MetricContractPolicyEquivalenceSnapshotV1,
    pub counterfactual_delta_present: bool,
    pub comparator_elapsed_us: u32,
    pub metric_contract_serialize_us: u32,
    pub metric_contract_build_and_serialize_us: u32,
    pub projection_build_and_validate_us: u32,
    pub gatekeeper_config_hash: &'a str,
    pub brain_config_hash: Option<&'a str>,
    pub writer_timestamp_ms: u64,
}

#[derive(Debug, Error)]
pub enum Pr2cRecordBuildErrorV1 {
    #[error(transparent)]
    Projection(#[from] MetricContractProjectionErrorV1),
    #[error(transparent)]
    Evidence(#[from] MetricContractEvidenceTransportErrorV1),
    #[error(transparent)]
    Hash(#[from] ghost_core::metric_contracts::CanonicalHashErrorV1),
    #[error("PR2C snapshot provenance does not match the supplied frozen context")]
    ContextMismatch,
    #[error("PR2C equivalence comparator detected active policy drift")]
    PolicyDrift,
}

fn contract_sets(
    profile: &MetricContractProfileV1,
) -> (Vec<MetricContractId>, Vec<MetricContractId>) {
    let authoritative = profile
        .payload()
        .entries
        .iter()
        .filter(|entry| entry.legacy_role == MetricRolloutRoleV1::PolicyAuthoritative)
        .map(|entry| entry.contract_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let comparator = profile
        .payload()
        .entries
        .iter()
        .filter(|entry| entry.dual_compute_role == MetricRolloutRoleV1::PolicyComparator)
        .map(|entry| entry.contract_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (authoritative, comparator)
}

fn projection_source_cutoff(
    snapshot: &Pr2bCompleteMetricContractSnapshotV1,
) -> ghost_core::metric_contracts::MetricContractDecisionSourceCutoffV1 {
    snapshot
        .compact_projection
        .fee_topology_diversity_index
        .legacy_value
        .source_cutoff
        .clone()
}

pub fn build_pr2c_paired_record_v1(
    snapshot: &Pr2bCompleteMetricContractSnapshotV1,
    context: &Pr2cDecisionRecordContextV1<'_>,
) -> Result<MetricContractPairedRecordV1, Pr2cRecordBuildErrorV1> {
    build_pr2c_paired_record_inner_v1(snapshot, None, context)
}

/// Runtime fast path from the opaque validated PR2B snapshot proof. Replay
/// intentionally does not use this path: replay rebuilds and validates the
/// projection from durable full evidence.
pub fn build_pr2c_paired_record_from_validated_snapshot_v1(
    validated: &Pr2bTimedCompleteMetricContractSnapshotV1,
    context: &Pr2cDecisionRecordContextV1<'_>,
) -> Result<MetricContractPairedRecordV1, Pr2cRecordBuildErrorV1> {
    build_pr2c_paired_record_inner_v1(
        validated.snapshot(),
        Some(validated.validated_projection_hash()),
        context,
    )
}

fn build_pr2c_paired_record_inner_v1(
    snapshot: &Pr2bCompleteMetricContractSnapshotV1,
    prevalidated_projection_hash: Option<&CanonicalHashV1>,
    context: &Pr2cDecisionRecordContextV1<'_>,
) -> Result<MetricContractPairedRecordV1, Pr2cRecordBuildErrorV1> {
    let profile_hash = match prevalidated_projection_hash {
        Some(_) => snapshot.compact_projection.profile_hash.clone(),
        None => context.profile.canonical_hash()?,
    };
    let projection_context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: context.rollout_mode,
        profile: context.profile,
        effective_config: context.effective_config,
        source_cutoff: projection_source_cutoff(snapshot),
    };
    let projection_hash = match prevalidated_projection_hash {
        Some(hash) => hash.clone(),
        None => snapshot
            .compact_projection
            .validated_canonical_hash(&projection_context)?,
    };
    if snapshot.compact_projection.rollout_mode != context.rollout_mode
        || snapshot.compact_projection.profile_id != context.profile.payload().profile_id
        || snapshot.compact_projection.profile_hash != profile_hash
        || snapshot
            .compact_projection
            .metric_contract_effective_config_hash
            != context
                .effective_config
                .metric_contract_effective_config_hash
    {
        return Err(Pr2cRecordBuildErrorV1::ContextMismatch);
    }

    let deltas = context
        .authoritative_policy
        .compare(context.comparator_policy);
    if !deltas.is_zero_drift() {
        return Err(Pr2cRecordBuildErrorV1::PolicyDrift);
    }
    let stable_event_identity = context
        .stable_event_identity
        .clone()
        .map(CanonicalNullableV1::Value)
        .unwrap_or(CanonicalNullableV1::Null);
    let payload = MetricContractEvidenceHashPayloadV1 {
        evidence_schema_version: METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
        record_identity: context.record_identity.clone(),
        stable_event_identity,
        rollout_mode: context.rollout_mode,
        profile_id: context.profile.payload().profile_id,
        profile_hash: profile_hash.clone(),
        metric_contract_effective_config_hash: context
            .effective_config
            .metric_contract_effective_config_hash
            .clone(),
        contracts: snapshot.full_evidence.clone(),
    };
    let evidence = if prevalidated_projection_hash.is_some() {
        // The opaque PR2B wrapper proves that the exact full evidence set was
        // already validated semantically and against the selected profile.
        // Only the record-scoped payload and canonical evidence hash are new
        // here. Durable writer reconstruction, deserialization and replay use
        // `try_new` and repeat the full validation independently.
        let evidence_sha256 = payload.canonical_hash()?;
        MetricContractEvidenceTransportV1 {
            payload,
            evidence_sha256,
            writer_timestamp_ms: context.writer_timestamp_ms,
            rotation_part_index: 0,
        }
    } else {
        MetricContractEvidenceTransportV1::try_new(payload, context.writer_timestamp_ms, 0)?
    };
    let (authoritative_contracts, comparator_contracts) = contract_sets(context.profile);
    let decision_v34 = MetricContractDecisionSummaryV1 {
        metric_contract_schema_version: snapshot.compact_projection.schema_version,
        rollout_mode: context.rollout_mode,
        profile_id: context.profile.payload().profile_id,
        profile_hash,
        metric_contract_effective_config_hash: context
            .effective_config
            .metric_contract_effective_config_hash
            .clone(),
        evidence_record_id: context.record_identity.clone(),
        evidence_sha256: evidence.evidence_sha256.clone(),
        evidence_schema_version: METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
        authoritative_contracts,
        comparator_contracts,
        equivalence_deltas: deltas,
        counterfactual_delta_present: context.counterfactual_delta_present,
        comparator_elapsed_us: context.comparator_elapsed_us,
        metric_contract_serialize_us: context.metric_contract_serialize_us,
        measured_fields_mask: snapshot
            .compact_projection
            .manipulation_contradiction
            .measured_fields_mask,
    };
    let paired = MetricContractPairedRecordV1 {
        decision_v34,
        evidence,
        decision_time_projection: snapshot.compact_projection.clone(),
        decision_time_projection_hash: projection_hash,
        stable_event_identity: context.stable_event_identity.clone(),
        metric_contract_build_and_serialize_us: context.metric_contract_build_and_serialize_us,
        projection_build_and_validate_us: context.projection_build_and_validate_us,
        gatekeeper_config_hash: context.gatekeeper_config_hash.to_string(),
        brain_config_hash: context.brain_config_hash.map(str::to_string),
        effective_config: context.effective_config.clone(),
    };
    if prevalidated_projection_hash.is_some() {
        paired
            .validate_pair_with_prevalidated_effective_config()
            .map_err(|_| Pr2cRecordBuildErrorV1::ContextMismatch)?;
    } else {
        paired
            .validate_pair()
            .map_err(|_| Pr2cRecordBuildErrorV1::ContextMismatch)?;
    }
    debug_assert_eq!(METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34, 34);
    Ok(paired)
}
