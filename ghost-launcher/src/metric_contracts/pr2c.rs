use super::{Pr2bCompleteMetricContractSnapshotV1, Pr2bTimedCompleteMetricContractSnapshotV1};
use crate::components::gatekeeper::{GatekeeperAssessment, GatekeeperDecision};
use ghost_core::metric_contracts::{
    CanonicalHashV1, CanonicalNullableV1, MetricContractDecisionSummaryV1,
    MetricContractEvidenceHashPayloadV1, MetricContractEvidenceTransportErrorV1,
    MetricContractEvidenceTransportV1, MetricContractId, MetricContractPairedRecordV1,
    MetricContractPolicyEquivalenceEvidenceV1, MetricContractPolicyEquivalenceSnapshotV1,
    MetricContractProfileV1, MetricContractProjectionErrorV1, MetricContractRolloutMode,
    MetricDecisionProjectionBuildContextV1, MetricEvidenceRecordIdentityV1, MetricRolloutRoleV1,
    ResolvedMetricContractEffectiveConfigV1, StableEventIdentityV1,
    METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34, METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
};
use std::collections::BTreeSet;
use thiserror::Error;

/// Freezes the policy fields covered by the PR2C equivalence lane from one
/// already-materialized assessment and one concrete policy evaluation.
///
/// Keeping this conversion next to the durable pair builder lets runtime and
/// regression tests prove that the comparator snapshot comes from the real
/// pure evaluator without adding live-state reads or a second feature build.
#[must_use]
pub fn pr2c_policy_equivalence_snapshot_v1(
    assessment: &GatekeeperAssessment,
    decision: Option<&GatekeeperDecision>,
) -> MetricContractPolicyEquivalenceSnapshotV1 {
    let terminal_reason = assessment
        .terminal_reason_code
        .map(ghost_brain::oracle::reason_code::GatekeeperReasonCode::as_log_str);
    MetricContractPolicyEquivalenceSnapshotV1 {
        verdict: decision.map_or_else(
            || "TIMEOUT_WITHOUT_POLICY_DECISION".to_string(),
            |value| format!("{:?}", value.verdict_type),
        ),
        primary_reason_code: decision
            .and_then(|value| value.reason_code)
            .map(ghost_brain::oracle::reason_code::GatekeeperReasonCode::as_log_str)
            .or(terminal_reason)
            .unwrap_or_else(|| "MISSING_TYPED_REASON".to_string()),
        ordered_reason_chain: decision
            .map(|value| vec![value.reason_chain.clone()])
            .unwrap_or_default(),
        phase_pass_vector: vec![
            assessment.phase1_passed,
            assessment.phase2_passed,
            assessment.phase3_passed,
            assessment.phase4_passed,
            assessment.phase5_passed,
            assessment.phase6_passed,
        ],
        soft_points: decision.map_or(0, |value| i64::from(value.soft_points)),
        selector_soft_score_bits: decision
            .map_or(0, |value| u64::from(value.selector_soft_score.score)),
        hard_fail_classification: decision
            .and_then(|value| value.hard_fail_reason.clone())
            .unwrap_or_else(|| "none".to_string()),
    }
}

#[derive(Debug, Clone)]
pub struct Pr2cDecisionRecordContextV1<'a> {
    pub record_identity: MetricEvidenceRecordIdentityV1,
    pub stable_event_identity: Option<StableEventIdentityV1>,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile: &'a MetricContractProfileV1,
    pub effective_config: &'a ResolvedMetricContractEffectiveConfigV1,
    pub authoritative_policy: &'a MetricContractPolicyEquivalenceSnapshotV1,
    pub comparator_policy: &'a MetricContractPolicyEquivalenceSnapshotV1,
    /// False means the second policy evaluation did not produce a comparable
    /// result. It is persisted as `NotEvaluable`, never normalized to Equal.
    pub comparator_evaluable: bool,
    pub comparator_elapsed_us: u32,
    pub metric_contract_serialize_us: u32,
    pub metric_contract_build_and_serialize_us: u32,
    pub projection_build_and_validate_us: u32,
    pub gatekeeper_config_hash: &'a str,
    pub brain_config_hash: Option<&'a str>,
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
    #[error("PR2C full build-path duration does not fit the durable u32 metric")]
    DurationOverflow,
}

pub fn pr2c_contract_sets_v1(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pr2cCounterfactualLaneStatusV1 {
    NotEvaluable,
    Equal,
    Different,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pr2cCounterfactualEvaluationV1 {
    pub dev_primary: Pr2cCounterfactualLaneStatusV1,
    pub corrected_ftdi_actionability: Pr2cCounterfactualLaneStatusV1,
}

impl Pr2cCounterfactualEvaluationV1 {
    #[must_use]
    pub const fn delta_present(self) -> bool {
        matches!(self.dev_primary, Pr2cCounterfactualLaneStatusV1::Different)
            || matches!(
                self.corrected_ftdi_actionability,
                Pr2cCounterfactualLaneStatusV1::Different
            )
    }

    #[must_use]
    pub const fn any_not_evaluable(self) -> bool {
        matches!(
            self.dev_primary,
            Pr2cCounterfactualLaneStatusV1::NotEvaluable
        ) || matches!(
            self.corrected_ftdi_actionability,
            Pr2cCounterfactualLaneStatusV1::NotEvaluable
        )
    }
}

pub fn evaluate_pr2c_counterfactual_lanes_v1(
    projection: &ghost_core::metric_contracts::MetricContractDecisionEvidenceProjectionV1,
) -> Pr2cCounterfactualEvaluationV1 {
    fn f64_lane(
        left: &CanonicalNullableV1<f64>,
        right: &CanonicalNullableV1<f64>,
    ) -> Pr2cCounterfactualLaneStatusV1 {
        match (left, right) {
            (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
                if left.to_bits() == right.to_bits() {
                    Pr2cCounterfactualLaneStatusV1::Equal
                } else {
                    Pr2cCounterfactualLaneStatusV1::Different
                }
            }
            _ => Pr2cCounterfactualLaneStatusV1::NotEvaluable,
        }
    }

    fn bool_lane(
        left: &CanonicalNullableV1<bool>,
        right: &CanonicalNullableV1<bool>,
    ) -> Pr2cCounterfactualLaneStatusV1 {
        match (left, right) {
            (CanonicalNullableV1::Value(left), CanonicalNullableV1::Value(right)) => {
                if left == right {
                    Pr2cCounterfactualLaneStatusV1::Equal
                } else {
                    Pr2cCounterfactualLaneStatusV1::Different
                }
            }
            _ => Pr2cCounterfactualLaneStatusV1::NotEvaluable,
        }
    }

    Pr2cCounterfactualEvaluationV1 {
        dev_primary: f64_lane(
            &projection.dev_buy.mfs_primary_v1.value,
            &projection.dev_buy.effective_policy.value,
        ),
        corrected_ftdi_actionability: bool_lane(
            &projection
                .fee_topology_diversity_index
                .legacy_buy_tx_actionability
                .value,
            &projection
                .fee_topology_diversity_index
                .unique_buyer_actionability_v2
                .value,
        ),
    }
}

pub fn build_pr2c_paired_record_v1(
    snapshot: &Pr2bCompleteMetricContractSnapshotV1,
    context: &Pr2cDecisionRecordContextV1<'_>,
) -> Result<MetricContractPairedRecordV1, Pr2cRecordBuildErrorV1> {
    build_pr2c_paired_record_inner_v1(snapshot, None, None, context)
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
        Some(validated.full_path_started()),
        context,
    )
}

/// Production timing boundary shared by OracleRuntime and the release
/// harness. The resulting sample starts before the first canonical producer,
/// includes evidence/projection validation, the real second policy evaluation
/// and terminal pair construction. The paired writer adds serialization of
/// the exact final v34/evidence bytes before recording the histogram sample.
pub fn build_pr2c_timed_paired_record_from_validated_snapshot_v1(
    validated: &Pr2bTimedCompleteMetricContractSnapshotV1,
    context: &Pr2cDecisionRecordContextV1<'_>,
) -> Result<MetricContractPairedRecordV1, Pr2cRecordBuildErrorV1> {
    let pair_started = std::time::Instant::now();
    let mut pair = build_pr2c_paired_record_from_validated_snapshot_v1(validated, context)?;
    let pair_construction_us = u32::try_from(pair_started.elapsed().as_micros())
        .map_err(|_| Pr2cRecordBuildErrorV1::DurationOverflow)?;
    pair.metric_contract_build_and_serialize_us = validated
        .timings()
        .metric_contract_build_and_validate_us
        .checked_add(context.comparator_elapsed_us)
        .ok_or(Pr2cRecordBuildErrorV1::DurationOverflow)?
        .checked_add(pair_construction_us)
        .ok_or(Pr2cRecordBuildErrorV1::DurationOverflow)?;
    pair.projection_build_and_validate_us = validated.timings().projection_build_and_validate_us;
    Ok(pair)
}

fn build_pr2c_paired_record_inner_v1(
    snapshot: &Pr2bCompleteMetricContractSnapshotV1,
    prevalidated_projection_hash: Option<&CanonicalHashV1>,
    full_path_started: Option<std::time::Instant>,
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
        source_cutoff: snapshot.source_cutoff.clone(),
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
        || snapshot
            .compact_projection
            .fee_topology_diversity_index
            .legacy_value
            .source_cutoff
            != snapshot.source_cutoff
    {
        return Err(Pr2cRecordBuildErrorV1::ContextMismatch);
    }

    let policy_equivalence = MetricContractPolicyEquivalenceEvidenceV1 {
        policy_version: ghost_brain::oracle::GATEKEEPER_VERSION.to_string(),
        gatekeeper_config_hash: CanonicalHashV1::parse(context.gatekeeper_config_hash)
            .map_err(|_| Pr2cRecordBuildErrorV1::ContextMismatch)?,
        comparator_evaluable: context.comparator_evaluable,
        authoritative: context.authoritative_policy.clone(),
        comparator: context.comparator_policy.clone(),
    };
    policy_equivalence
        .validate()
        .map_err(|_| Pr2cRecordBuildErrorV1::ContextMismatch)?;
    let deltas = policy_equivalence.recompute_deltas();
    let stable_event_identity = context
        .stable_event_identity
        .clone()
        .map(CanonicalNullableV1::Value)
        .unwrap_or(CanonicalNullableV1::Null);
    let payload = MetricContractEvidenceHashPayloadV1 {
        evidence_schema_version: METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
        record_identity: context.record_identity.clone(),
        stable_event_identity,
        source_cutoff: snapshot.source_cutoff.clone(),
        rollout_mode: context.rollout_mode,
        profile_id: context.profile.payload().profile_id,
        profile_hash: profile_hash.clone(),
        metric_contract_effective_config_hash: context
            .effective_config
            .metric_contract_effective_config_hash
            .clone(),
        policy_equivalence,
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
            writer_timestamp_ms: 0,
            rotation_part_index: 0,
        }
    } else {
        MetricContractEvidenceTransportV1::try_new(payload, 0, 0)?
    };
    let (authoritative_contracts, comparator_contracts) = pr2c_contract_sets_v1(context.profile);
    let counterfactual = evaluate_pr2c_counterfactual_lanes_v1(&snapshot.compact_projection);
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
        counterfactual_delta_present: counterfactual.delta_present(),
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
        metric_contract_full_path_started: full_path_started
            .unwrap_or_else(std::time::Instant::now),
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
