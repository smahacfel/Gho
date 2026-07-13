use super::{
    build_pr2a_evidence_families_v1, Pr2aEvidenceBuildContextV1, Pr2aFrozenProducerInputsV1,
    Pr2aProducerErrorV1,
};
use crate::tx_intelligence::FlipV2ProducerSnapshotV1;
use ghost_core::account_state_core::types::AccountStateReserveVelocitySnapshotV1;
use ghost_core::checkpoint::{
    EvidenceStatus, ManipulationContradictionFeatures, MaterializedFeatureSet,
};
use ghost_core::metric_contracts::{
    CanonicalHashV1, CanonicalMetricEnvelopeV1, CanonicalNullableV1, CanonicalU64StringV1,
    FlipEvidenceReasonV1, FlipRatioContractEvidenceV1, FlipRatioEvidenceV2,
    ManipulationComparatorV1, ManipulationDerivedFlagEvidenceV2, ManipulationEvidenceReasonV1,
    ManipulationLegacyHighFlagEvidenceV1, ManipulationNumericEvidenceV2,
    ManipulationNumericFieldEvidenceV2, ManipulationNumericFieldIdV2, MetricAvailabilityV1,
    MetricContractDecisionEvidenceProjectionV1, MetricContractDecisionSourceCutoffV1,
    MetricContractId, MetricContractProfileV1, MetricContractRolloutMode,
    MetricContractsEvidenceSetV1, MetricDecisionProjectionBuildContextV1,
    MetricDecisionProjectionValidatedContextV1, MetricEffectiveConfigKeyV1,
    MetricEffectiveConfigValueV1, MetricEvidenceEnvelopeErrorV1, MetricEvidenceReasonV1,
    MetricMeasurementQualityV1, MetricRolloutRoleV1, MetricSurfaceId,
    RecentBuySellEvidenceReasonV1, RecentBuySellEvidenceV1, ReserveVelocityEvidenceReasonV1,
    ReserveVelocityEvidenceV1, ReserveVelocitySourceClockV1, ReserveVelocityStatusV1,
    ResolvedMetricContractEffectiveConfigV1, MANIPULATION_DERIVED_POLICY_STAGE_V1,
    MANIPULATION_DERIVED_POLICY_VERSION_V1,
};
use std::collections::BTreeSet;
use std::time::Instant;
use thiserror::Error;

pub const PR2B_FAMILY_PRODUCER_SCHEMA_VERSION_V1: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pr2bEffectiveConfigValidationBoundaryV1 {
    CompactValidated,
    FrozenProducerBoundaryValidated,
}

pub const PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1: &[(
    MetricEffectiveConfigKeyV1,
    Pr2bEffectiveConfigValidationBoundaryV1,
)] = &[
    (
        MetricEffectiveConfigKeyV1::FlipLegacyWindowSemantics,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateWallClockWindowMs,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateMaxSlotGap,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateDumpRatio,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateAnchorRule,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateOrderPolicy,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateSuccessRequired,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateDustThresholdSol,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateDedupeKey,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateDedupeCapacity,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateEvictionPolicy,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateMaxWallets,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::FlipCandidateReconnectBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationNumericPresenceVersion,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationBooleanPresenceVersion,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighFlagDerivationVersion,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighSameMsThreshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighBundleThreshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighTop3Threshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighHhiThreshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationHighDevConcentrationThreshold,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationMissingRawBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ManipulationMeasuredFieldsMaskVersion,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ReserveVelocitySourceClock,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ReserveVelocityFirstUpdateBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ReserveVelocityZeroDeltaTimeBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ReserveVelocityFallbackBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::ReserveVelocityUnit,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellWindowMs,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellSuccessfulOnly,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellBoundaryPolicy,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellSameMsNumeratorRule,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellLegacyRatioRule,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellUnboundedRatioRule,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellBoundedShareRule,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
    (
        MetricEffectiveConfigKeyV1::RecentBuySellZeroDenominatorBehavior,
        Pr2bEffectiveConfigValidationBoundaryV1::CompactValidated,
    ),
];

#[derive(Debug, Clone, PartialEq)]
pub struct RecentBuySellProducerSnapshotV1 {
    pub window_ms: u64,
    pub buy_count: u64,
    pub sell_count: u64,
    pub transaction_count: u64,
    pub failed_transaction_count: u64,
    pub source_complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManipulationProducerFieldV2<T> {
    pub value: Option<T>,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reasons: Vec<MetricEvidenceReasonV1>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManipulationProducerSnapshotV2 {
    pub same_ms_tx_ratio: ManipulationProducerFieldV2<f64>,
    pub bundle_suspicion_ratio: ManipulationProducerFieldV2<f64>,
    pub top3_signer_volume_ratio: ManipulationProducerFieldV2<f64>,
    pub hhi: ManipulationProducerFieldV2<f64>,
    pub max_tx_per_signer: ManipulationProducerFieldV2<u64>,
    pub dev_volume_ratio: ManipulationProducerFieldV2<f64>,
    pub contradiction_score: ManipulationProducerFieldV2<f64>,
    pub group_status: EvidenceStatus,
    pub group_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManipulationFrozenSnapshotV2 {
    pub legacy: ManipulationContradictionFeatures,
    pub typed: ManipulationProducerSnapshotV2,
}

fn manipulation_snapshot_field<T>(
    value: Option<T>,
    group_status: EvidenceStatus,
    group_reasons: &[MetricEvidenceReasonV1],
) -> ManipulationProducerFieldV2<T> {
    match value {
        Some(value) => ManipulationProducerFieldV2 {
            value: Some(value),
            availability: MetricAvailabilityV1::Available,
            measurement_quality: if group_status == EvidenceStatus::Clean {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::Degraded
            },
            reasons: group_reasons.to_vec(),
        },
        None => ManipulationProducerFieldV2 {
            value: None,
            availability: MetricAvailabilityV1::Unavailable,
            measurement_quality: MetricMeasurementQualityV1::NotApplicable,
            reasons: vec![MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::RawFieldAbsent,
            )],
        },
    }
}

/// Freeze legacy V3 compatibility values and true V2 per-field presence from
/// the same already-materialized owner snapshot. Scalar defaults are retained
/// only in `legacy`; they never establish V2 field presence.
#[must_use]
pub fn freeze_manipulation_producer_snapshot_v2(
    materialized: &MaterializedFeatureSet,
    legacy: ManipulationContradictionFeatures,
) -> ManipulationFrozenSnapshotV2 {
    let tx = &materialized.tx_intel_features;
    let alpha = &materialized.alpha_fingerprint;
    let organic = &materialized.organic_broadening;
    let group_can_expose_values = matches!(
        legacy.status,
        EvidenceStatus::Clean | EvidenceStatus::Degraded
    );

    let same_ms = group_can_expose_values
        .then_some(tx.same_ms_tx_ratio)
        .filter(|_| tx.tx_count > 0);
    let bundle = group_can_expose_values
        .then_some(tx.bundle_suspicion_ratio)
        .filter(|_| tx.tx_count > 0);
    let top3 = group_can_expose_values
        .then_some(tx.top3_signer_volume_ratio)
        .flatten();
    let signer_population_evaluable = tx.tx_count > 0 && tx.unique_signers > 0;
    let hhi = group_can_expose_values
        .then_some(tx.hhi)
        .filter(|_| signer_population_evaluable);
    let max_tx = group_can_expose_values
        .then_some(tx.max_tx_per_signer)
        .filter(|_| signer_population_evaluable);
    let dev = group_can_expose_values
        .then_some(tx.dev_volume_ratio)
        .filter(|_| tx.tx_count > 0 && tx.total_volume_sol > 0.0 && tx.dev_wallet_known);
    let contradiction_components_evaluable = tx.tx_count > 0
        && tx.top3_signer_volume_ratio.is_some()
        && organic.sequence_available
        && materialized.tx_segment_sequence.is_some()
        && alpha.fixed_size_buy_ratio.is_some()
        && alpha.early_top3_buy_volume_pct_3s.is_some();
    let contradiction = group_can_expose_values
        .then_some(legacy.contradiction_score)
        .filter(|_| contradiction_components_evaluable);

    let all_required_present = same_ms.is_some()
        && bundle.is_some()
        && top3.is_some()
        && hhi.is_some()
        && max_tx.is_some()
        && dev.is_some()
        && contradiction.is_some();
    let any_present = same_ms.is_some()
        || bundle.is_some()
        || top3.is_some()
        || hhi.is_some()
        || max_tx.is_some()
        || dev.is_some()
        || contradiction.is_some();
    let group_status = match legacy.status {
        EvidenceStatus::Clean if !all_required_present => EvidenceStatus::Degraded,
        status if any_present => status,
        _ => EvidenceStatus::Unavailable,
    };
    let mut group_reasons = legacy.reasons.clone();
    if legacy.status == EvidenceStatus::Clean && group_status == EvidenceStatus::Degraded {
        group_reasons.push("typed_field_presence_partial".to_string());
    }
    let typed_reasons = group_reasons
        .iter()
        .map(|reason| {
            ghost_core::metric_contracts::adapt_legacy_metric_reason_v1(
                MetricContractId::ManipulationContradiction,
                reason,
            )
        })
        .collect::<Vec<_>>();

    ManipulationFrozenSnapshotV2 {
        legacy,
        typed: ManipulationProducerSnapshotV2 {
            same_ms_tx_ratio: manipulation_snapshot_field(same_ms, group_status, &typed_reasons),
            bundle_suspicion_ratio: manipulation_snapshot_field(
                bundle,
                group_status,
                &typed_reasons,
            ),
            top3_signer_volume_ratio: manipulation_snapshot_field(
                top3,
                group_status,
                &typed_reasons,
            ),
            hhi: manipulation_snapshot_field(hhi, group_status, &typed_reasons),
            max_tx_per_signer: manipulation_snapshot_field(max_tx, group_status, &typed_reasons),
            dev_volume_ratio: manipulation_snapshot_field(dev, group_status, &typed_reasons),
            contradiction_score: manipulation_snapshot_field(
                contradiction,
                group_status,
                &typed_reasons,
            ),
            group_status,
            group_reasons,
        },
    }
}

pub struct Pr2bFrozenProducerInputsV1<'a> {
    pub pr2a: Pr2aFrozenProducerInputsV1<'a>,
    pub legacy_flip_ratio: Option<f64>,
    pub flip_v2: &'a FlipV2ProducerSnapshotV1,
    pub manipulation: &'a ManipulationFrozenSnapshotV2,
    pub reserve_velocity: &'a AccountStateReserveVelocitySnapshotV1,
    pub recent_buy_sell: &'a RecentBuySellProducerSnapshotV1,
}

pub struct Pr2bBuildContextV1<'a> {
    pub rollout_mode: MetricContractRolloutMode,
    pub profile: &'a MetricContractProfileV1,
    pub effective_config: &'a ResolvedMetricContractEffectiveConfigV1,
    pub source_cutoff: MetricContractDecisionSourceCutoffV1,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pr2bCompleteMetricContractSnapshotV1 {
    pub full_evidence: MetricContractsEvidenceSetV1,
    pub compact_projection: MetricContractDecisionEvidenceProjectionV1,
}

/// Runtime-only timings captured inside the one canonical PR2B build. They
/// are deliberately outside the immutable evidence/projection snapshot so
/// timing noise cannot affect equality, serialization or semantic hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pr2bCompleteMetricContractBuildTimingsV1 {
    pub metric_contract_build_and_validate_us: u32,
    pub projection_build_and_validate_us: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pr2bTimedCompleteMetricContractSnapshotV1 {
    snapshot: Pr2bCompleteMetricContractSnapshotV1,
    timings: Pr2bCompleteMetricContractBuildTimingsV1,
    validated_projection_hash: CanonicalHashV1,
}

impl Pr2bTimedCompleteMetricContractSnapshotV1 {
    #[must_use]
    pub fn snapshot(&self) -> &Pr2bCompleteMetricContractSnapshotV1 {
        &self.snapshot
    }

    #[must_use]
    pub const fn timings(&self) -> Pr2bCompleteMetricContractBuildTimingsV1 {
        self.timings
    }

    /// Hash produced by the validated-only projection path during the one
    /// canonical PR2B build. Keeping it inside this opaque wrapper lets PR2C
    /// reuse the proof without re-validating or re-hashing the projection.
    #[must_use]
    pub fn validated_projection_hash(&self) -> &CanonicalHashV1 {
        &self.validated_projection_hash
    }

    #[must_use]
    pub fn into_snapshot(self) -> Pr2bCompleteMetricContractSnapshotV1 {
        self.snapshot
    }
}

#[derive(Debug, Error)]
pub enum Pr2bProducerErrorV1 {
    #[error(transparent)]
    Pr2a(#[from] Pr2aProducerErrorV1),
    #[error(transparent)]
    Envelope(#[from] MetricEvidenceEnvelopeErrorV1),
    #[error(transparent)]
    Projection(#[from] ghost_core::metric_contracts::MetricContractProjectionErrorV1),
    #[error(transparent)]
    Evidence(#[from] ghost_core::metric_contracts::MetricContractEvidenceSemanticErrorV1),
    #[error("PR2B producer count overflows compact representation: {0}")]
    CountOverflow(&'static str),
    #[error("PR2B producer/config mismatch: {0}")]
    ProducerConfigMismatch(&'static str),
    #[error("PR2B producer invariant failed: {0}")]
    ProducerInvariant(&'static str),
    #[error("PR2B resource timing exceeds u32 microseconds: {0}")]
    TimingOverflow(&'static str),
}

fn checked_u32(value: u64, field: &'static str) -> Result<u32, Pr2bProducerErrorV1> {
    u32::try_from(value).map_err(|_| Pr2bProducerErrorV1::CountOverflow(field))
}

fn envelope(
    context: &Pr2bBuildContextV1<'_>,
    contract_id: MetricContractId,
    surface_id: MetricSurfaceId,
    availability: MetricAvailabilityV1,
    quality: MetricMeasurementQualityV1,
    policy_actionable: bool,
    reasons: Vec<MetricEvidenceReasonV1>,
) -> Result<CanonicalMetricEnvelopeV1, Pr2bProducerErrorV1> {
    let assignment = context.profile.entry_for(surface_id).ok_or(
        MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromProfile(surface_id),
    )?;
    let envelope = CanonicalMetricEnvelopeV1::try_new(
        contract_id,
        1,
        surface_id,
        assignment.authority_class,
        availability,
        quality,
        policy_actionable,
        reasons,
    )?;
    envelope.validate_for_profile(context.profile, context.rollout_mode)?;
    Ok(envelope)
}

fn policy_authoritative(context: &Pr2bBuildContextV1<'_>, surface: MetricSurfaceId) -> bool {
    context.profile.entry_for(surface).is_some_and(|entry| {
        entry.role_for(context.rollout_mode) == MetricRolloutRoleV1::PolicyAuthoritative
    })
}

fn config_wide(
    context: &Pr2bBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
) -> Result<u64, Pr2bProducerErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::WideUnsigned(value)) => Ok(value.get()),
        _ => Err(Pr2bProducerErrorV1::ProducerConfigMismatch(
            "wide unsigned setting",
        )),
    }
}

fn config_ratio(
    context: &Pr2bBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
) -> Result<f64, Pr2bProducerErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Ratio(value)) if value.is_finite() => Ok(*value),
        _ => Err(Pr2bProducerErrorV1::ProducerConfigMismatch("ratio setting")),
    }
}

fn config_finite(
    context: &Pr2bBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
) -> Result<f64, Pr2bProducerErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::FiniteNumber(value)) if value.is_finite() => Ok(*value),
        _ => Err(Pr2bProducerErrorV1::ProducerConfigMismatch(
            "finite setting",
        )),
    }
}

fn config_enum(
    context: &Pr2bBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    expected: &'static str,
    field: &'static str,
) -> Result<(), Pr2bProducerErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Enum(actual)) if actual == expected => Ok(()),
        _ => Err(Pr2bProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_bool(
    context: &Pr2bBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    expected: bool,
    field: &'static str,
) -> Result<(), Pr2bProducerErrorV1> {
    match context.effective_config.value(key) {
        Some(MetricEffectiveConfigValueV1::Boolean(actual)) if *actual == expected => Ok(()),
        _ => Err(Pr2bProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

pub fn build_flip_evidence_v2(
    legacy_ratio: Option<f64>,
    snapshot: &FlipV2ProducerSnapshotV1,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<FlipRatioContractEvidenceV1, Pr2bProducerErrorV1> {
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::FlipLegacyWindowSemantics,
            "first_buy_slot_to_last_sell_slot_gap",
            "legacy flip semantics",
        ),
        (
            MetricEffectiveConfigKeyV1::FlipCandidateAnchorRule,
            "first_eligible_buy",
            "flip anchor rule",
        ),
        (
            MetricEffectiveConfigKeyV1::FlipCandidateOrderPolicy,
            "stable_tx_key",
            "flip order policy",
        ),
        (
            MetricEffectiveConfigKeyV1::FlipCandidateDedupeKey,
            "tx_key_v1",
            "flip dedupe key",
        ),
        (
            MetricEffectiveConfigKeyV1::FlipCandidateEvictionPolicy,
            "fail_closed_on_capacity_or_gap",
            "flip eviction policy",
        ),
        (
            MetricEffectiveConfigKeyV1::FlipCandidateReconnectBehavior,
            "unavailable_on_gap",
            "flip reconnect behavior",
        ),
    ] {
        config_enum(context, key, expected, field)?;
    }
    config_bool(
        context,
        MetricEffectiveConfigKeyV1::FlipCandidateSuccessRequired,
        true,
        "flip success requirement",
    )?;
    if snapshot.config.wall_clock_window_ms
        != Some(config_wide(
            context,
            MetricEffectiveConfigKeyV1::FlipCandidateWallClockWindowMs,
        )?)
        || snapshot.config.max_slot_gap
            != config_wide(context, MetricEffectiveConfigKeyV1::FlipCandidateMaxSlotGap)?
        || snapshot.config.dump_ratio.to_bits()
            != config_ratio(context, MetricEffectiveConfigKeyV1::FlipCandidateDumpRatio)?.to_bits()
        || snapshot.config.dust_threshold_sol.to_bits()
            != config_finite(
                context,
                MetricEffectiveConfigKeyV1::FlipCandidateDustThresholdSol,
            )?
            .to_bits()
        || u64::try_from(snapshot.config.dedupe_capacity).ok()
            != Some(config_wide(
                context,
                MetricEffectiveConfigKeyV1::FlipCandidateDedupeCapacity,
            )?)
        || u64::try_from(snapshot.config.max_wallets).ok()
            != Some(config_wide(
                context,
                MetricEffectiveConfigKeyV1::FlipCandidateMaxWallets,
            )?)
    {
        return Err(Pr2bProducerErrorV1::ProducerConfigMismatch(
            "flip producer snapshot",
        ));
    }
    let legacy_value = match legacy_ratio {
        Some(value) => CanonicalNullableV1::Value(value),
        None => CanonicalNullableV1::Null,
    };
    let legacy_available = legacy_ratio.is_some();
    let legacy_envelope = envelope(
        context,
        MetricContractId::FlipRatio,
        MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
        if legacy_available {
            MetricAvailabilityV1::Available
        } else {
            MetricAvailabilityV1::Unavailable
        },
        if legacy_available {
            MetricMeasurementQualityV1::Measured
        } else {
            MetricMeasurementQualityV1::NotApplicable
        },
        false,
        vec![MetricEvidenceReasonV1::Flip(
            FlipEvidenceReasonV1::LegacySlotGapOnly,
        )],
    )?;
    let ratio = match snapshot.ratio {
        Some(value) => CanonicalNullableV1::Value(value),
        None => CanonicalNullableV1::Null,
    };
    let measured = snapshot.evaluable && snapshot.ratio.is_some();
    let reasons = snapshot
        .reasons
        .iter()
        .copied()
        .map(MetricEvidenceReasonV1::Flip)
        .collect::<Vec<_>>();
    Ok(FlipRatioContractEvidenceV1 {
        legacy_envelope,
        legacy_slot_gap_ratio: legacy_value,
        hybrid_v2: FlipRatioEvidenceV2 {
            envelope: envelope(
                context,
                MetricContractId::FlipRatio,
                MetricSurfaceId::FlipRatioHybridEvidenceV2,
                if measured {
                    MetricAvailabilityV1::Available
                } else {
                    MetricAvailabilityV1::Unavailable
                },
                if measured {
                    MetricMeasurementQualityV1::Measured
                } else {
                    MetricMeasurementQualityV1::NotApplicable
                },
                false,
                reasons,
            )?,
            ratio,
            eligible_buyer_count: checked_u32(snapshot.eligible_buyer_count, "flip buyers")?,
            flipper_count: checked_u32(snapshot.flipper_count, "flip flippers")?,
            wall_clock_window_ms: checked_u32(
                snapshot.config.wall_clock_window_ms.ok_or(
                    Pr2bProducerErrorV1::ProducerInvariant("flip window arithmetic overflow"),
                )?,
                "flip window",
            )?,
            max_slot_gap: checked_u32(snapshot.config.max_slot_gap, "flip slot gap")?,
            dump_ratio: snapshot.config.dump_ratio,
            owners: snapshot.owners.clone(),
        },
    })
}

const MANIPULATION_FIELDS: [ManipulationNumericFieldIdV2; 7] = [
    ManipulationNumericFieldIdV2::SameMsTxRatio,
    ManipulationNumericFieldIdV2::BundleSuspicionRatio,
    ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
    ManipulationNumericFieldIdV2::Hhi,
    ManipulationNumericFieldIdV2::MaxTxPerSigner,
    ManipulationNumericFieldIdV2::DevVolumeRatio,
    ManipulationNumericFieldIdV2::ContradictionScore,
];

fn manipulation_raw(
    features: &ManipulationContradictionFeatures,
    id: ManipulationNumericFieldIdV2,
) -> f64 {
    match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => features.same_ms_tx_ratio,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => features.bundle_suspicion_ratio,
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => features.top3_volume_pct,
        ManipulationNumericFieldIdV2::Hhi => features.hhi,
        ManipulationNumericFieldIdV2::MaxTxPerSigner => features.max_tx_per_signer as f64,
        ManipulationNumericFieldIdV2::DevVolumeRatio => features.dev_volume_ratio,
        ManipulationNumericFieldIdV2::ContradictionScore => features.contradiction_score,
    }
}

fn manipulation_legacy_high(
    features: &ManipulationContradictionFeatures,
    id: ManipulationNumericFieldIdV2,
) -> bool {
    match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => features.high_same_ms_tx_ratio,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => features.high_bundle_suspicion_ratio,
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => features.high_top3_volume_pct,
        ManipulationNumericFieldIdV2::Hhi => features.high_hhi,
        ManipulationNumericFieldIdV2::MaxTxPerSigner => features.high_signer_concentration,
        ManipulationNumericFieldIdV2::DevVolumeRatio => features.high_dev_concentration,
        ManipulationNumericFieldIdV2::ContradictionScore => false,
    }
}

fn manipulation_threshold(
    context: &Pr2bBuildContextV1<'_>,
    id: ManipulationNumericFieldIdV2,
) -> Result<Option<f64>, Pr2bProducerErrorV1> {
    Ok(match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => Some(config_ratio(
            context,
            MetricEffectiveConfigKeyV1::ManipulationHighSameMsThreshold,
        )?),
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => Some(config_ratio(
            context,
            MetricEffectiveConfigKeyV1::ManipulationHighBundleThreshold,
        )?),
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => Some(config_ratio(
            context,
            MetricEffectiveConfigKeyV1::ManipulationHighTop3Threshold,
        )?),
        ManipulationNumericFieldIdV2::Hhi => Some(config_ratio(
            context,
            MetricEffectiveConfigKeyV1::ManipulationHighHhiThreshold,
        )?),
        ManipulationNumericFieldIdV2::MaxTxPerSigner => {
            let value = config_wide(
                context,
                MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold,
            )?;
            if value > (1_u64 << 53) {
                return Err(Pr2bProducerErrorV1::ProducerConfigMismatch(
                    "manipulation signer threshold is not exactly representable",
                ));
            }
            Some(value as f64)
        }
        ManipulationNumericFieldIdV2::DevVolumeRatio => Some(config_ratio(
            context,
            MetricEffectiveConfigKeyV1::ManipulationHighDevConcentrationThreshold,
        )?),
        ManipulationNumericFieldIdV2::ContradictionScore => None,
    })
}

pub fn build_manipulation_evidence_v2(
    snapshot: &ManipulationFrozenSnapshotV2,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<ManipulationNumericEvidenceV2, Pr2bProducerErrorV1> {
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::ManipulationNumericPresenceVersion,
            "v2_field_presence",
            "manipulation numeric presence version",
        ),
        (
            MetricEffectiveConfigKeyV1::ManipulationBooleanPresenceVersion,
            "v2_field_presence",
            "manipulation boolean presence version",
        ),
        (
            MetricEffectiveConfigKeyV1::ManipulationHighFlagDerivationVersion,
            "policy_stage_v1",
            "manipulation derivation version",
        ),
        (
            MetricEffectiveConfigKeyV1::ManipulationMissingRawBehavior,
            "unavailable_not_false",
            "manipulation missing raw behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::ManipulationMeasuredFieldsMaskVersion,
            "v1_u16",
            "manipulation measured mask version",
        ),
    ] {
        config_enum(context, key, expected, field)?;
    }
    let features = &snapshot.legacy;
    let typed = &snapshot.typed;
    if features.max_tx_per_signer > (1_u64 << 53)
        || typed
            .max_tx_per_signer
            .value
            .is_some_and(|value| value > (1_u64 << 53))
    {
        return Err(Pr2bProducerErrorV1::ProducerInvariant(
            "manipulation signer count is not exactly representable",
        ));
    }
    for id in MANIPULATION_FIELDS {
        let value = manipulation_raw(features, id);
        if !value.is_finite()
            || (id != ManipulationNumericFieldIdV2::MaxTxPerSigner && !(0.0..=1.0).contains(&value))
            || (id == ManipulationNumericFieldIdV2::MaxTxPerSigner && value < 0.0)
        {
            return Err(Pr2bProducerErrorV1::ProducerInvariant(
                "manipulation numeric field range",
            ));
        }
    }
    let legacy_available = matches!(
        features.status,
        EvidenceStatus::Clean | EvidenceStatus::Degraded
    );
    let legacy_quality = if features.status == EvidenceStatus::Clean {
        MetricMeasurementQualityV1::Measured
    } else {
        MetricMeasurementQualityV1::Degraded
    };
    let numeric_reasons = features
        .reasons
        .iter()
        .map(|reason| {
            ghost_core::metric_contracts::adapt_legacy_metric_reason_v1(
                MetricContractId::ManipulationContradiction,
                reason,
            )
        })
        .collect::<Vec<_>>();
    let legacy_fields = MANIPULATION_FIELDS
        .into_iter()
        .map(|id| ManipulationNumericFieldEvidenceV2 {
            field_id: id,
            value: CanonicalNullableV1::Value(manipulation_raw(features, id)),
            availability: MetricAvailabilityV1::Available,
            measurement_quality: if legacy_available {
                legacy_quality
            } else {
                MetricMeasurementQualityV1::LegacyDefault
            },
            reason_codes: if legacy_available {
                numeric_reasons.clone()
            } else {
                vec![MetricEvidenceReasonV1::Manipulation(
                    ManipulationEvidenceReasonV1::LegacyDefaultZero,
                )]
            },
        })
        .collect::<Vec<_>>();
    let typed_parts = |id| match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => (
            typed.same_ms_tx_ratio.value,
            typed.same_ms_tx_ratio.availability,
            typed.same_ms_tx_ratio.measurement_quality,
            typed.same_ms_tx_ratio.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => (
            typed.bundle_suspicion_ratio.value,
            typed.bundle_suspicion_ratio.availability,
            typed.bundle_suspicion_ratio.measurement_quality,
            typed.bundle_suspicion_ratio.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => (
            typed.top3_signer_volume_ratio.value,
            typed.top3_signer_volume_ratio.availability,
            typed.top3_signer_volume_ratio.measurement_quality,
            typed.top3_signer_volume_ratio.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::Hhi => (
            typed.hhi.value,
            typed.hhi.availability,
            typed.hhi.measurement_quality,
            typed.hhi.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::MaxTxPerSigner => (
            typed.max_tx_per_signer.value.map(|value| value as f64),
            typed.max_tx_per_signer.availability,
            typed.max_tx_per_signer.measurement_quality,
            typed.max_tx_per_signer.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::DevVolumeRatio => (
            typed.dev_volume_ratio.value,
            typed.dev_volume_ratio.availability,
            typed.dev_volume_ratio.measurement_quality,
            typed.dev_volume_ratio.reasons.as_slice(),
        ),
        ManipulationNumericFieldIdV2::ContradictionScore => (
            typed.contradiction_score.value,
            typed.contradiction_score.availability,
            typed.contradiction_score.measurement_quality,
            typed.contradiction_score.reasons.as_slice(),
        ),
    };
    let fields = MANIPULATION_FIELDS
        .into_iter()
        .map(|id| {
            let (value, availability, measurement_quality, reasons) = typed_parts(id);
            let coherent = match (value, availability, measurement_quality) {
                (
                    Some(_),
                    MetricAvailabilityV1::Available,
                    MetricMeasurementQualityV1::Measured | MetricMeasurementQualityV1::Degraded,
                ) => true,
                (None, availability, MetricMeasurementQualityV1::NotApplicable)
                    if availability != MetricAvailabilityV1::Available =>
                {
                    true
                }
                _ => false,
            };
            if !coherent {
                return Err(Pr2bProducerErrorV1::ProducerInvariant(
                    "manipulation producer field presence/status coherence",
                ));
            }
            if typed.group_status == EvidenceStatus::Clean
                && measurement_quality != MetricMeasurementQualityV1::Measured
            {
                return Err(Pr2bProducerErrorV1::ProducerInvariant(
                    "clean manipulation group requires every field measured",
                ));
            }
            if typed.group_status == EvidenceStatus::Degraded
                && value.is_some()
                && measurement_quality != MetricMeasurementQualityV1::Degraded
            {
                return Err(Pr2bProducerErrorV1::ProducerInvariant(
                    "manipulation field quality exceeds group quality",
                ));
            }
            Ok(ManipulationNumericFieldEvidenceV2 {
                field_id: id,
                value: value.into(),
                availability,
                measurement_quality,
                reason_codes: reasons.to_vec(),
            })
        })
        .collect::<Result<Vec<_>, Pr2bProducerErrorV1>>()?;
    let any_typed_available = fields.iter().any(|field| !field.value.is_null());
    if any_typed_available
        != matches!(
            typed.group_status,
            EvidenceStatus::Clean | EvidenceStatus::Degraded
        )
    {
        return Err(Pr2bProducerErrorV1::ProducerInvariant(
            "manipulation group availability/presence coherence",
        ));
    }
    let typed_quality = match typed.group_status {
        EvidenceStatus::Clean => MetricMeasurementQualityV1::Measured,
        EvidenceStatus::Degraded => MetricMeasurementQualityV1::Degraded,
        _ => MetricMeasurementQualityV1::NotApplicable,
    };
    let typed_reasons = typed
        .group_reasons
        .iter()
        .map(|reason| {
            ghost_core::metric_contracts::adapt_legacy_metric_reason_v1(
                MetricContractId::ManipulationContradiction,
                reason,
            )
        })
        .collect::<Vec<_>>();
    let measured_fields_mask = fields
        .iter()
        .filter(|field| !field.value.is_null())
        .fold(0_u16, |mask, field| {
            mask | field.field_id.measured_mask_bit()
        });
    let legacy_high_flags = MANIPULATION_FIELDS
        .into_iter()
        .filter(|id| id.has_derived_high_flag())
        .map(|id| ManipulationLegacyHighFlagEvidenceV1 {
            field_id: id,
            value: manipulation_legacy_high(features, id),
            field_recorded: legacy_available,
        })
        .collect::<Vec<_>>();
    let config_hash = context
        .effective_config
        .metric_contract_effective_config_hash
        .clone();
    let derived_high_flags = fields
        .iter()
        .filter(|field| field.field_id.has_derived_high_flag())
        .map(|field| {
            let threshold = manipulation_threshold(context, field.field_id)?;
            let derived = match (&field.value, threshold) {
                (CanonicalNullableV1::Value(raw), Some(threshold)) => {
                    CanonicalNullableV1::Value(*raw > threshold)
                }
                _ => CanonicalNullableV1::Null,
            };
            Ok(ManipulationDerivedFlagEvidenceV2 {
                field_id: field.field_id,
                raw_value: field.value.clone(),
                raw_availability: field.availability,
                raw_measurement_quality: field.measurement_quality,
                derived_value: derived,
                comparator: ManipulationComparatorV1::GreaterThan,
                threshold: threshold.into(),
                policy_stage: MANIPULATION_DERIVED_POLICY_STAGE_V1.to_string(),
                policy_version: MANIPULATION_DERIVED_POLICY_VERSION_V1.to_string(),
                config_hash: config_hash.clone(),
            })
        })
        .collect::<Result<Vec<_>, Pr2bProducerErrorV1>>()?;
    Ok(ManipulationNumericEvidenceV2 {
        legacy_numeric_envelope: envelope(
            context,
            MetricContractId::ManipulationContradiction,
            MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
            MetricAvailabilityV1::Available,
            if legacy_available {
                legacy_quality
            } else {
                MetricMeasurementQualityV1::LegacyDefault
            },
            policy_authoritative(
                context,
                MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
            ) && legacy_available,
            numeric_reasons.clone(),
        )?,
        numeric_v2_envelope: envelope(
            context,
            MetricContractId::ManipulationContradiction,
            MetricSurfaceId::ManipulationNumericEvidenceV2,
            if any_typed_available {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            typed_quality,
            false,
            typed_reasons,
        )?,
        measured_fields_mask,
        legacy_fields,
        fields,
        legacy_high_flags_envelope: envelope(
            context,
            MetricContractId::ManipulationContradiction,
            MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
            MetricAvailabilityV1::Available,
            if legacy_available {
                legacy_quality
            } else {
                MetricMeasurementQualityV1::LegacyDefault
            },
            policy_authoritative(
                context,
                MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
            ) && legacy_available,
            Vec::new(),
        )?,
        legacy_high_flags,
        derived_high_flags_envelope: envelope(
            context,
            MetricContractId::ManipulationContradiction,
            MetricSurfaceId::PolicyDerivedManipulationHighFlagsV2,
            if any_typed_available {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            typed_quality,
            false,
            vec![MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::DerivedInPolicy,
            )],
        )?,
        derived_high_flags,
    })
}

pub fn build_reserve_velocity_evidence_v1(
    snapshot: &AccountStateReserveVelocitySnapshotV1,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<ReserveVelocityEvidenceV1, Pr2bProducerErrorV1> {
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::ReserveVelocitySourceClock,
            "receive_time",
            "reserve source clock",
        ),
        (
            MetricEffectiveConfigKeyV1::ReserveVelocityFirstUpdateBehavior,
            "typed_first_update",
            "reserve first-update behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::ReserveVelocityZeroDeltaTimeBehavior,
            "unavailable",
            "reserve zero-delta behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::ReserveVelocityFallbackBehavior,
            "typed_fallback_not_zero",
            "reserve fallback behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::ReserveVelocityUnit,
            "sol_per_second",
            "reserve unit",
        ),
    ] {
        config_enum(context, key, expected, field)?;
    }
    if !snapshot.legacy_velocity_sol_per_sec.is_finite() {
        return Err(Pr2bProducerErrorV1::ProducerInvariant(
            "non-finite reserve velocity",
        ));
    }
    let producer_valid = match snapshot.status {
        ReserveVelocityStatusV1::Measured => match (
            snapshot.previous_real_sol_reserves_lamports,
            snapshot.current_real_sol_reserves_lamports,
            snapshot.interval_ms,
        ) {
            (Some(previous), Some(current), Some(interval_ms))
                if interval_ms > 0 && snapshot.accepted_update_count >= 2 =>
            {
                let delta_sol = (current as f64 - previous as f64) / 1_000_000_000.0;
                let expected = delta_sol / (interval_ms as f64 / 1_000.0);
                snapshot.legacy_velocity_sol_per_sec.to_bits() == expected.to_bits()
            }
            _ => false,
        },
        ReserveVelocityStatusV1::FirstUpdate => {
            snapshot.accepted_update_count == 1
                && snapshot.previous_real_sol_reserves_lamports.is_none()
                && snapshot.current_real_sol_reserves_lamports.is_some()
                && snapshot.interval_ms.is_none()
        }
        ReserveVelocityStatusV1::ZeroDeltaTime => {
            snapshot.accepted_update_count >= 2
                && snapshot.previous_real_sol_reserves_lamports.is_some()
                && snapshot.current_real_sol_reserves_lamports.is_some()
                && snapshot.interval_ms == Some(0)
        }
        ReserveVelocityStatusV1::BootstrapFallback => {
            snapshot.accepted_update_count == 0 && snapshot.interval_ms.is_none()
        }
        ReserveVelocityStatusV1::Unavailable => true,
    };
    if !producer_valid {
        return Err(Pr2bProducerErrorV1::ProducerInvariant(
            "reserve velocity status/count/interval/formula parity",
        ));
    }
    let measured = snapshot.status == ReserveVelocityStatusV1::Measured;
    let reason = match snapshot.status {
        ReserveVelocityStatusV1::FirstUpdate => {
            Some(ReserveVelocityEvidenceReasonV1::BootstrapFirstUpdate)
        }
        ReserveVelocityStatusV1::ZeroDeltaTime => {
            Some(ReserveVelocityEvidenceReasonV1::ZeroDeltaTime)
        }
        ReserveVelocityStatusV1::BootstrapFallback => {
            Some(ReserveVelocityEvidenceReasonV1::FallbackState)
        }
        ReserveVelocityStatusV1::Unavailable => {
            Some(ReserveVelocityEvidenceReasonV1::SourceUnavailable)
        }
        ReserveVelocityStatusV1::Measured if snapshot.legacy_velocity_sol_per_sec == 0.0 => {
            Some(ReserveVelocityEvidenceReasonV1::MeasuredZero)
        }
        ReserveVelocityStatusV1::Measured => None,
    };
    let reasons = reason
        .into_iter()
        .map(MetricEvidenceReasonV1::ReserveVelocity)
        .collect::<Vec<_>>();
    Ok(ReserveVelocityEvidenceV1 {
        legacy_envelope: envelope(
            context,
            MetricContractId::ReserveVelocity,
            MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
            MetricAvailabilityV1::Available,
            if measured {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::LegacyDefault
            },
            false,
            reasons.clone(),
        )?,
        legacy_velocity_sol_per_sec: snapshot.legacy_velocity_sol_per_sec,
        v1_envelope: envelope(
            context,
            MetricContractId::ReserveVelocity,
            MetricSurfaceId::ReserveVelocityEvidenceV1,
            if measured {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            if measured {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::NotApplicable
            },
            false,
            reasons,
        )?,
        velocity_sol_per_sec: if measured {
            CanonicalNullableV1::Value(snapshot.legacy_velocity_sol_per_sec)
        } else {
            CanonicalNullableV1::Null
        },
        previous_real_sol_reserves_lamports: snapshot
            .previous_real_sol_reserves_lamports
            .map(CanonicalU64StringV1::new)
            .into(),
        current_real_sol_reserves_lamports: snapshot
            .current_real_sol_reserves_lamports
            .map(CanonicalU64StringV1::new)
            .into(),
        interval_ms: snapshot
            .interval_ms
            .map(|value| checked_u32(value, "reserve interval"))
            .transpose()?
            .into(),
        accepted_update_count: checked_u32(snapshot.accepted_update_count, "reserve update count")?,
        source_clock: ReserveVelocitySourceClockV1::ReceiveTime,
        status: snapshot.status,
    })
}

pub fn build_recent_buy_sell_evidence_v1(
    snapshot: &RecentBuySellProducerSnapshotV1,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<RecentBuySellEvidenceV1, Pr2bProducerErrorV1> {
    config_bool(
        context,
        MetricEffectiveConfigKeyV1::RecentBuySellSuccessfulOnly,
        true,
        "recent successful-only population",
    )?;
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::RecentBuySellBoundaryPolicy,
            "inclusive_start_and_end",
            "recent boundary policy",
        ),
        (
            MetricEffectiveConfigKeyV1::RecentBuySellSameMsNumeratorRule,
            "sum_timestamp_multiplicity_minus_one",
            "recent same-ms numerator rule",
        ),
        (
            MetricEffectiveConfigKeyV1::RecentBuySellLegacyRatioRule,
            "sell_zero_returns_buy_count",
            "recent legacy ratio rule",
        ),
        (
            MetricEffectiveConfigKeyV1::RecentBuySellUnboundedRatioRule,
            "buy_count_over_sell_count_or_null",
            "recent unbounded ratio rule",
        ),
        (
            MetricEffectiveConfigKeyV1::RecentBuySellBoundedShareRule,
            "buy_count_over_transaction_count",
            "recent bounded share rule",
        ),
        (
            MetricEffectiveConfigKeyV1::RecentBuySellZeroDenominatorBehavior,
            "null_unavailable",
            "recent zero-denominator rule",
        ),
    ] {
        config_enum(context, key, expected, field)?;
    }
    if snapshot.window_ms
        != config_wide(context, MetricEffectiveConfigKeyV1::RecentBuySellWindowMs)?
        || snapshot.buy_count.checked_add(snapshot.sell_count) != Some(snapshot.transaction_count)
        || !snapshot.source_complete
    {
        return Err(Pr2bProducerErrorV1::ProducerConfigMismatch(
            "recent buy/sell snapshot",
        ));
    }
    let buy_count = checked_u32(snapshot.buy_count, "recent buys")?;
    let sell_count = checked_u32(snapshot.sell_count, "recent sells")?;
    let transaction_count = checked_u32(snapshot.transaction_count, "recent transactions")?;
    let legacy = if transaction_count == 0 {
        CanonicalNullableV1::Null
    } else if sell_count == 0 {
        CanonicalNullableV1::Value(f64::from(buy_count))
    } else {
        CanonicalNullableV1::Value(f64::from(buy_count) / f64::from(sell_count))
    };
    let ratio = if sell_count == 0 {
        CanonicalNullableV1::Null
    } else {
        CanonicalNullableV1::Value(f64::from(buy_count) / f64::from(sell_count))
    };
    let share = if transaction_count == 0 {
        CanonicalNullableV1::Null
    } else {
        CanonicalNullableV1::Value(f64::from(buy_count) / f64::from(transaction_count))
    };
    let mut reasons = vec![MetricEvidenceReasonV1::RecentBuySell(
        RecentBuySellEvidenceReasonV1::LoggingOnly,
    )];
    if transaction_count == 0 {
        reasons.push(MetricEvidenceReasonV1::RecentBuySell(
            RecentBuySellEvidenceReasonV1::EmptyWindow,
        ));
        reasons.push(MetricEvidenceReasonV1::RecentBuySell(
            RecentBuySellEvidenceReasonV1::ZeroDenominator,
        ));
    } else if sell_count == 0 {
        reasons.push(MetricEvidenceReasonV1::RecentBuySell(
            RecentBuySellEvidenceReasonV1::SellCountZero,
        ));
        reasons.push(MetricEvidenceReasonV1::RecentBuySell(
            RecentBuySellEvidenceReasonV1::LegacySellZeroReturnsBuyCount,
        ));
    }
    if snapshot.failed_transaction_count > 0 {
        reasons.push(MetricEvidenceReasonV1::RecentBuySell(
            RecentBuySellEvidenceReasonV1::FailedTransactionExcluded,
        ));
    }
    let available = transaction_count > 0;
    Ok(RecentBuySellEvidenceV1 {
        legacy_envelope: envelope(
            context,
            MetricContractId::RecentBuySell,
            MetricSurfaceId::RceBuySellRatioRecentLegacy,
            if available {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            if available {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::NotApplicable
            },
            false,
            reasons.clone(),
        )?,
        v1_envelope: envelope(
            context,
            MetricContractId::RecentBuySell,
            MetricSurfaceId::RecentBuySellEvidenceV1,
            if available {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            if available {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::NotApplicable
            },
            false,
            reasons,
        )?,
        window_ms: checked_u32(snapshot.window_ms, "recent window")?,
        buy_count,
        sell_count,
        transaction_count,
        legacy_buy_sell_scalar: legacy,
        buy_to_sell_ratio: ratio,
        buy_share: share,
    })
}

fn build_pr2b_complete_metric_contract_snapshot_inner_v1(
    inputs: Pr2bFrozenProducerInputsV1<'_>,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<Pr2bTimedCompleteMetricContractSnapshotV1, Pr2bProducerErrorV1> {
    let complete_started = Instant::now();
    let projection_context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: context.rollout_mode,
        profile: context.profile,
        effective_config: context.effective_config,
        source_cutoff: context.source_cutoff.clone(),
    };
    let validated_projection_context =
        MetricDecisionProjectionValidatedContextV1::try_new(&projection_context)?;
    let pr2a_context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: context.rollout_mode,
        profile: context.profile,
        effective_config: context.effective_config,
    };
    let pr2a = build_pr2a_evidence_families_v1(inputs.pr2a, &pr2a_context)?;
    let full_evidence = MetricContractsEvidenceSetV1 {
        fee_topology_diversity_index: pr2a.fee_topology_diversity_index,
        dev_buy: pr2a.dev_buy,
        same_ms_tx_ratio: pr2a.same_ms_tx_ratio,
        top3_signer_volume_ratio: pr2a.top3_signer_volume_ratio,
        flip_ratio: build_flip_evidence_v2(inputs.legacy_flip_ratio, inputs.flip_v2, context)?,
        funding_source_concentration: pr2a.funding_source_concentration,
        fsc_evidence_status: pr2a.fsc_evidence_status,
        manipulation_contradiction: build_manipulation_evidence_v2(inputs.manipulation, context)?,
        reserve_velocity: build_reserve_velocity_evidence_v1(inputs.reserve_velocity, context)?,
        recent_buy_sell: build_recent_buy_sell_evidence_v1(inputs.recent_buy_sell, context)?,
    };
    let validated_projection_inputs =
        validated_projection_context.validate_evidence(&full_evidence)?;
    let projection_started = Instant::now();
    let (compact_projection, validated_projection_hash) =
        validated_projection_inputs.build_with_validated_canonical_hash()?;
    let projection_build_and_validate_us = u32::try_from(projection_started.elapsed().as_micros())
        .map_err(|_| Pr2bProducerErrorV1::TimingOverflow("projection"))?;
    let metric_contract_build_and_validate_us =
        u32::try_from(complete_started.elapsed().as_micros())
            .map_err(|_| Pr2bProducerErrorV1::TimingOverflow("complete snapshot"))?;
    Ok(Pr2bTimedCompleteMetricContractSnapshotV1 {
        snapshot: Pr2bCompleteMetricContractSnapshotV1 {
            full_evidence,
            compact_projection,
        },
        timings: Pr2bCompleteMetricContractBuildTimingsV1 {
            metric_contract_build_and_validate_us,
            projection_build_and_validate_us,
        },
        validated_projection_hash,
    })
}

pub fn build_pr2b_complete_metric_contract_snapshot_v1(
    inputs: Pr2bFrozenProducerInputsV1<'_>,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<Pr2bCompleteMetricContractSnapshotV1, Pr2bProducerErrorV1> {
    build_pr2b_complete_metric_contract_snapshot_inner_v1(inputs, context)
        .map(Pr2bTimedCompleteMetricContractSnapshotV1::into_snapshot)
}

pub fn build_pr2b_timed_complete_metric_contract_snapshot_v1(
    inputs: Pr2bFrozenProducerInputsV1<'_>,
    context: &Pr2bBuildContextV1<'_>,
) -> Result<Pr2bTimedCompleteMetricContractSnapshotV1, Pr2bProducerErrorV1> {
    build_pr2b_complete_metric_contract_snapshot_inner_v1(inputs, context)
}

pub fn pr2b_key_boundary_set_is_closed() -> bool {
    let expected = ghost_core::metric_contracts::METRIC_EFFECTIVE_CONFIG_KEYS_V1
        .iter()
        .copied()
        .filter(|key| {
            matches!(
                key.contract_id(),
                Some(MetricContractId::FlipRatio)
                    | Some(MetricContractId::ManipulationContradiction)
                    | Some(MetricContractId::ReserveVelocity)
                    | Some(MetricContractId::RecentBuySell)
            )
        })
        .collect::<BTreeSet<_>>();
    let actual = PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1
        .iter()
        .map(|(key, _)| *key)
        .collect::<BTreeSet<_>>();
    expected == actual && actual.len() == PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1.len()
}
