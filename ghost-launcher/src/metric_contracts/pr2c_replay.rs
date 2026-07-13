use ghost_core::metric_contracts::{
    CanonicalHashV1, MetricContractDecisionEvidenceProjectionV1,
    MetricContractDecisionProjectionWireV1, MetricContractDecisionSummaryV1,
    MetricContractEvidenceTransportErrorV1, MetricContractEvidenceTransportV1,
    MetricContractProfileV1, MetricContractProjectionErrorV1, MetricContractProjectionWireErrorV1,
    MetricContractProjectionWireV1SchemaManifest, MetricDecisionProjectionBuildContextV1,
    ResolvedMetricContractEffectiveConfigV1, METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
    METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
    METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Pr2cReplayInputV2 {
    pub decision_v34: MetricContractDecisionSummaryV1,
    pub evidence: MetricContractEvidenceTransportV1,
    pub decision_time_projection: MetricContractDecisionEvidenceProjectionV1,
    pub effective_config: ResolvedMetricContractEffectiveConfigV1,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pr2cReplayResultV2 {
    pub rebuilt_projection: MetricContractDecisionEvidenceProjectionV1,
    pub semantic_projection_hash: CanonicalHashV1,
    pub wire_version: u16,
}

#[derive(Debug, Error)]
pub enum Pr2cReplayErrorV2 {
    #[error(transparent)]
    Evidence(#[from] MetricContractEvidenceTransportErrorV1),
    #[error(transparent)]
    Projection(#[from] MetricContractProjectionErrorV1),
    #[error(transparent)]
    Wire(#[from] MetricContractProjectionWireErrorV1),
    #[error(transparent)]
    Profile(#[from] ghost_core::metric_contracts::MetricContractProfileErrorV1),
    #[error(transparent)]
    Hash(#[from] ghost_core::metric_contracts::CanonicalHashErrorV1),
    #[error("v34/evidence record identity or evidence hash mismatch")]
    PairMismatch,
    #[error("v34/evidence/profile/effective-config provenance mismatch")]
    ProvenanceMismatch,
    #[error("decision-time projection differs from the projection rebuilt from full evidence")]
    ProjectionFullEvidenceMismatch,
    #[error("decision-time and rebuilt semantic projection hashes differ")]
    ProjectionHashMismatch,
    #[error("compiled Compact JSON Wire V1 codebook differs from the frozen manifest")]
    CodebookManifestMismatch,
}

fn source_cutoff(
    projection: &MetricContractDecisionEvidenceProjectionV1,
) -> ghost_core::metric_contracts::MetricContractDecisionSourceCutoffV1 {
    projection
        .fee_topology_diversity_index
        .legacy_value
        .source_cutoff
        .clone()
}

pub fn replay_metric_contract_record_v2(
    input: Pr2cReplayInputV2,
) -> Result<Pr2cReplayResultV2, Pr2cReplayErrorV2> {
    let codebook = MetricContractProjectionWireV1SchemaManifest::current();
    if !codebook.has_closed_table_counts()
        || codebook.blake3_hex().ok().as_deref()
            != Some(METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3)
    {
        return Err(Pr2cReplayErrorV2::CodebookManifestMismatch);
    }
    input.evidence.validate_hash()?;
    input
        .effective_config
        .validate_hash()
        .map_err(|_| Pr2cReplayErrorV2::ProvenanceMismatch)?;
    if input.decision_v34.evidence_record_id != input.evidence.payload.record_identity
        || input.decision_v34.evidence_sha256 != input.evidence.evidence_sha256
        || input.decision_v34.evidence_schema_version != METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1
        || input.decision_v34.metric_contract_schema_version
            != input.decision_time_projection.schema_version
    {
        return Err(Pr2cReplayErrorV2::PairMismatch);
    }
    let profile = MetricContractProfileV1::profile_a()?;
    let profile_hash = profile.canonical_hash()?;
    if input.decision_v34.rollout_mode != input.evidence.payload.rollout_mode
        || input.decision_v34.profile_id != input.evidence.payload.profile_id
        || input.decision_v34.profile_hash != profile_hash
        || input.evidence.payload.profile_hash != profile_hash
        || input.decision_v34.metric_contract_effective_config_hash
            != input.effective_config.metric_contract_effective_config_hash
        || input.evidence.payload.metric_contract_effective_config_hash
            != input.effective_config.metric_contract_effective_config_hash
    {
        return Err(Pr2cReplayErrorV2::ProvenanceMismatch);
    }
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: input.decision_v34.rollout_mode,
        profile: &profile,
        effective_config: &input.effective_config,
        source_cutoff: source_cutoff(&input.decision_time_projection),
    };
    let decision_time_hash = input
        .decision_time_projection
        .validated_canonical_hash(&context)?;
    let rebuilt_projection = MetricContractDecisionEvidenceProjectionV1::try_from_evidence(
        &input.evidence.payload.contracts,
        &context,
    )?;
    if rebuilt_projection != input.decision_time_projection {
        return Err(Pr2cReplayErrorV2::ProjectionFullEvidenceMismatch);
    }
    let rebuilt_hash = rebuilt_projection.validated_canonical_hash(&context)?;
    if rebuilt_hash != decision_time_hash {
        return Err(Pr2cReplayErrorV2::ProjectionHashMismatch);
    }
    let wire = MetricContractDecisionProjectionWireV1::try_from_domain(&rebuilt_projection)?;
    if wire.w != METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1
        || wire.clone().try_into_domain()? != rebuilt_projection
    {
        return Err(Pr2cReplayErrorV2::ProjectionFullEvidenceMismatch);
    }
    Ok(Pr2cReplayResultV2 {
        rebuilt_projection,
        semantic_projection_hash: rebuilt_hash,
        wire_version: wire.w,
    })
}
