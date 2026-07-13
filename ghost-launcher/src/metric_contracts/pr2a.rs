use crate::components::gatekeeper::GatekeeperDevPrimaryCompatibilitySnapshotV1;
use crate::tx_intelligence::{
    FscComputation, FtdiComputation, FundingSourceConfig, FundingSourceProducerConfigSnapshotV1,
    TxIntelligenceMetricContractSnapshotV1, TxTimingProducerSnapshotV1,
};
use ghost_core::checkpoint::EvidenceStatus;
use ghost_core::metric_contracts::{
    adapt_fsc_excluded_reason_v1, adapt_legacy_metric_reason_v1, CanonicalHashV1,
    CanonicalMetricEnvelopeV1, CanonicalNullableV1, CanonicalU64StringV1, DevBuyContractEvidenceV1,
    DevBuyEvidenceReasonV1, DevBuyEvidenceV1, DevBuySelectionModeV1, FscStatusEvidenceV1,
    FtdiEvidenceReasonV1, FtdiEvidenceV1, FtdiValueMeasurementV1, FundingSourceContractEvidenceV1,
    FundingSourceEvidenceReasonV1, FundingSourceLegacyMeasurementV1, MetricAuthorityClass,
    MetricAvailabilityV1, MetricContractEffectiveConfigErrorV1, MetricContractId,
    MetricContractProfileV1, MetricContractProjectionErrorV1, MetricContractRolloutMode,
    MetricDecisionProjectionBuildContextV1, MetricDecisionProjectionValidatedStaticContextV1,
    MetricEffectiveConfigKeyV1, MetricEffectiveConfigValueV1, MetricEvidenceEnvelopeErrorV1,
    MetricEvidenceReasonV1, MetricMeasurementQualityV1, MetricRolloutRoleV1, MetricSurfaceId,
    ResolvedMetricContractEffectiveConfigV1, Top3EvidenceReasonV1, Top3SignerVolumeEvidenceV1,
    TxTimingEvidenceReasonV1, TxTimingEvidenceV1, TxTimingMeasurementEvidenceV1,
    TxTimingPopulationV1, TxTimingSourceV1,
};
use ghost_core::tx_intelligence::types::FscEvidenceStatus;
use std::collections::BTreeSet;
use thiserror::Error;

pub const PR2A_FAMILY_PRODUCER_SCHEMA_VERSION_V1: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pr2aEffectiveConfigValidationBoundaryV1 {
    CompactValidated,
    FrozenProducerBoundaryValidated,
}

macro_rules! pr2a_boundary_table {
    (
        compact [$( $compact:ident ),* $(,)?]
        frozen [$( $frozen:ident ),* $(,)?]
    ) => {
        &[
            $(
                (
                    MetricEffectiveConfigKeyV1::$compact,
                    Pr2aEffectiveConfigValidationBoundaryV1::CompactValidated,
                ),
            )*
            $(
                (
                    MetricEffectiveConfigKeyV1::$frozen,
                    Pr2aEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated,
                ),
            )*
        ]
    };
}

/// Closed PR2A key classification. Tests compare this table against the core
/// vocabulary by contract id, so adding a PR2A key without exactly one
/// validation boundary fails closed.
pub const PR2A_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1: &[(
    MetricEffectiveConfigKeyV1,
    Pr2aEffectiveConfigValidationBoundaryV1,
)] = pr2a_boundary_table!(
    compact [
        FtdiPopulationSuccessfulBuy,
        FtdiFirstSamplePerSigner,
        FtdiMissingSignerBehavior,
        FtdiMissingTopologyBehavior,
        FtdiDiagnosticMinUniqueBuyers,
        FtdiLegacyCleanMinBuyTransactions,
        FtdiCandidateCleanMinUniqueBuyers,
        FtdiDenominatorRule,
        DevTxIntelSuccessEligibility,
        DevFirstObservedAnchorRule,
        DevPrimarySuccessRequired,
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
        FscLegacyFormula,
        FscLegacyMinKnownSourceSamples,
        FscMinTotalBuyers,
        FscMinKnownCoverage,
        FscMinNonNeutralKnownCoverage,
        FscFundingStreamUnavailableBehavior,
        FscLegacyStatusMapping,
        FscV2StatusMapping,
    ]
    frozen [
        DevTxIntelDustThresholdSol,
        DevTxIntelDedupeKey,
        DevTxIntelDedupeCapacity,
        DevPrimaryDustThresholdSol,
        DevPrimaryDedupeKey,
        DevPrimaryDedupeCapacity,
        SameMsLegacyDustThresholdSol,
        SameMsLegacyDedupeKey,
        SameMsLegacyDedupeCapacity,
        SameMsRecentDedupeKey,
        SameMsRecentRetentionCapacity,
        SameMsRecentRetentionPolicy,
        FscFundingLookbackWindowMs,
        FscMinAbsStoreLamports,
        FscMinAbsAttributionLamports,
        FscMinRelativeToBuy,
        FscMinAttributionConfidenceBps,
        FscPerRecipientCapacity,
        FscGlobalRecipientCapacity,
        FscWarmupWindowMs,
        FscSameSlotOrderingPolicy,
        FscNeutralFunderSetVersion,
        FscNeutralFunderSetHash,
        FscMinKnownNonNeutralBuyers,
    ]
);

#[derive(Debug, Clone, PartialEq)]
pub struct Pr2aParitySensitiveEvidenceFamiliesV1 {
    pub fee_topology_diversity_index: FtdiEvidenceV1,
    pub dev_buy: DevBuyContractEvidenceV1,
    pub same_ms_tx_ratio: TxTimingEvidenceV1,
    pub top3_signer_volume_ratio: Top3SignerVolumeEvidenceV1,
    pub funding_source_concentration: FundingSourceContractEvidenceV1,
    pub fsc_evidence_status: FscStatusEvidenceV1,
}

pub struct Pr2aFrozenProducerInputsV1<'a> {
    pub ftdi: &'a FtdiComputation,
    pub tx_intelligence: &'a TxIntelligenceMetricContractSnapshotV1,
    pub gatekeeper_dev_primary: &'a GatekeeperDevPrimaryCompatibilitySnapshotV1,
    pub recent_exact_timing: &'a TxTimingProducerSnapshotV1,
    pub fsc: &'a FscComputation,
    pub funding_source_config: &'a FundingSourceConfig,
    pub funding_source_producer_config: &'a FundingSourceProducerConfigSnapshotV1,
}

pub struct Pr2aEvidenceBuildContextV1<'a> {
    pub rollout_mode: MetricContractRolloutMode,
    pub profile: &'a MetricContractProfileV1,
    pub effective_config: &'a ResolvedMetricContractEffectiveConfigV1,
}

impl Pr2aEvidenceBuildContextV1<'_> {
    pub fn validate(&self) -> Result<(), Pr2aProducerErrorV1> {
        self.effective_config.validate_hash()?;
        let payload = &self.effective_config.payload;
        if payload.rollout_mode != self.rollout_mode
            || payload.profile_id != self.profile.payload().profile_id
            || payload.profile_hash != self.profile.canonical_hash()?
        {
            return Err(Pr2aProducerErrorV1::ConfigProfileMismatch);
        }
        Ok(())
    }
}

/// Opaque local proof that the immutable profile/effective-config context was
/// hash-validated once for this producer build. Individual public family
/// builders create their own proof; the complete PR2A boundary shares one
/// proof across all six families and all envelopes.
struct ValidatedPr2aEvidenceBuildContextV1<'context, 'inputs> {
    context: &'context Pr2aEvidenceBuildContextV1<'inputs>,
}

impl<'context, 'inputs> ValidatedPr2aEvidenceBuildContextV1<'context, 'inputs> {
    fn try_new(
        context: &'context Pr2aEvidenceBuildContextV1<'inputs>,
    ) -> Result<Self, Pr2aProducerErrorV1> {
        context.validate()?;
        Ok(Self { context })
    }
}

#[derive(Debug, Error)]
pub enum Pr2aProducerErrorV1 {
    #[error(transparent)]
    Envelope(#[from] MetricEvidenceEnvelopeErrorV1),
    #[error(transparent)]
    EffectiveConfig(#[from] MetricContractEffectiveConfigErrorV1),
    #[error(transparent)]
    Hash(#[from] ghost_core::metric_contracts::CanonicalHashErrorV1),
    #[error(transparent)]
    Projection(#[from] MetricContractProjectionErrorV1),
    #[error("metric-contract effective config does not match profile/mode")]
    ConfigProfileMismatch,
    #[error("producer count exceeds compact/full evidence width: {0}")]
    CountOverflow(&'static str),
    #[error("producer emitted a non-finite value: {0}")]
    NonFinite(&'static str),
    #[error("producer ratio is outside [0,1]: {0}")]
    RatioOutOfRange(&'static str),
    #[error("producer value must be non-negative: {0}")]
    NegativeValue(&'static str),
    #[error("duplicate typed reason emitted by producer")]
    DuplicateReason,
    #[error("frozen producer settings disagree with effective config: {0}")]
    ProducerConfigMismatch(&'static str),
    #[error("frozen producer snapshot violates its semantic invariant: {0}")]
    ProducerInvariant(&'static str),
}

fn checked_u32(value: u64, field: &'static str) -> Result<u32, Pr2aProducerErrorV1> {
    u32::try_from(value).map_err(|_| Pr2aProducerErrorV1::CountOverflow(field))
}

fn reasons_unique(reasons: &[MetricEvidenceReasonV1]) -> Result<(), Pr2aProducerErrorV1> {
    let mut seen = BTreeSet::new();
    for reason in reasons {
        let encoded = serde_json::to_string(reason)
            .map_err(ghost_core::metric_contracts::CanonicalHashErrorV1::from)?;
        if !seen.insert(encoded) {
            return Err(Pr2aProducerErrorV1::DuplicateReason);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn envelope(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    contract_id: MetricContractId,
    surface_id: MetricSurfaceId,
    availability: MetricAvailabilityV1,
    measurement_quality: MetricMeasurementQualityV1,
    policy_actionable: bool,
    reasons: Vec<MetricEvidenceReasonV1>,
) -> Result<CanonicalMetricEnvelopeV1, Pr2aProducerErrorV1> {
    reasons_unique(&reasons)?;
    let assignment = context.profile.entry_for(surface_id).ok_or(
        MetricEvidenceEnvelopeErrorV1::SurfaceMissingFromProfile(surface_id),
    )?;
    if assignment.contract_id != contract_id {
        return Err(MetricEvidenceEnvelopeErrorV1::ContractSurfaceMismatch {
            surface: surface_id,
            expected: assignment.contract_id,
            actual: contract_id,
        }
        .into());
    }
    let result = CanonicalMetricEnvelopeV1::try_new(
        contract_id,
        1,
        surface_id,
        assignment.authority_class,
        availability,
        measurement_quality,
        policy_actionable,
        reasons,
    )?;
    result.validate_for_profile(context.profile, context.rollout_mode)?;
    Ok(result)
}

fn surface_is_policy_authoritative(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    surface: MetricSurfaceId,
) -> bool {
    context
        .profile
        .entry_for(surface)
        .is_some_and(|assignment| {
            assignment.role_for(context.rollout_mode) == MetricRolloutRoleV1::PolicyAuthoritative
                && matches!(
                    assignment.authority_class,
                    MetricAuthorityClass::Authoritative | MetricAuthorityClass::EquivalentCutover
                )
        })
}

fn finite_ratio(value: Option<f64>, field: &'static str) -> Result<(), Pr2aProducerErrorV1> {
    if let Some(value) = value {
        if !value.is_finite() {
            return Err(Pr2aProducerErrorV1::NonFinite(field));
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(Pr2aProducerErrorV1::RatioOutOfRange(field));
        }
    }
    Ok(())
}

fn finite_nonnegative(value: Option<f64>, field: &'static str) -> Result<(), Pr2aProducerErrorV1> {
    if let Some(value) = value {
        if !value.is_finite() {
            return Err(Pr2aProducerErrorV1::NonFinite(field));
        }
        if value < 0.0 {
            return Err(Pr2aProducerErrorV1::NegativeValue(field));
        }
    }
    Ok(())
}

fn ratio_bits_equal(actual: Option<f64>, expected: Option<f64>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => actual.to_bits() == expected.to_bits(),
        _ => false,
    }
}

fn validate_count_ratio(
    numerator: u64,
    denominator: u64,
    ratio: Option<f64>,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    if numerator > denominator {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(field));
    }
    let expected = (denominator > 0).then(|| numerator as f64 / denominator as f64);
    if !ratio_bits_equal(ratio, expected) {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(field));
    }
    Ok(())
}

fn config_value<'a>(
    context: &'a Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    field: &'static str,
) -> Result<&'a MetricEffectiveConfigValueV1, Pr2aProducerErrorV1> {
    context
        .effective_config
        .value(key)
        .ok_or(Pr2aProducerErrorV1::ProducerConfigMismatch(field))
}

fn config_finite_matches(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    actual: f64,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::FiniteNumber(expected)
            if expected.to_bits() == actual.to_bits() =>
        {
            Ok(())
        }
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_wide_matches(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    actual: u64,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::WideUnsigned(expected) if expected.get() == actual => Ok(()),
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_wide_value(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    field: &'static str,
) -> Result<u64, Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::WideUnsigned(value) => Ok(value.get()),
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_ratio_value(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    field: &'static str,
) -> Result<f64, Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::Ratio(value)
            if value.is_finite() && (0.0..=1.0).contains(value) =>
        {
            Ok(*value)
        }
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_boolean_matches(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    actual: bool,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::Boolean(expected) if *expected == actual => Ok(()),
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_enum_matches(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    actual: &str,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::Enum(expected) if expected == actual => Ok(()),
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn config_nullable_text_matches(
    context: &Pr2aEvidenceBuildContextV1<'_>,
    key: MetricEffectiveConfigKeyV1,
    actual: &Option<String>,
    field: &'static str,
) -> Result<(), Pr2aProducerErrorV1> {
    let actual: CanonicalNullableV1<String> = actual.clone().into();
    match config_value(context, key, field)? {
        MetricEffectiveConfigValueV1::NullableText(expected) if expected == &actual => Ok(()),
        _ => Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
    }
}

fn validate_ftdi_producer_config(
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    // These settings are not all reconstructable from the compact counts. They
    // are therefore frozen and checked here, before full evidence or a compact
    // projection can be built from the producer result.
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::FtdiPopulationSuccessfulBuy,
            "successful_buy",
            "ftdi.population",
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiMissingSignerBehavior,
            "legacy_empty_signer_identity",
            "ftdi.missing_signer_behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiMissingTopologyBehavior,
            "unavailable_entire_metric",
            "ftdi.missing_topology_behavior",
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiDenominatorRule,
            "unique_topologies_over_unique_first_buyer_samples",
            "ftdi.denominator_rule",
        ),
    ] {
        config_enum_matches(context, key, expected, field)?;
    }
    config_boolean_matches(
        context,
        MetricEffectiveConfigKeyV1::FtdiFirstSamplePerSigner,
        true,
        "ftdi.first_sample_per_signer",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers,
        u64::try_from(crate::tx_intelligence::MIN_DIAGNOSTIC_SAMPLE_COUNT)
            .map_err(|_| Pr2aProducerErrorV1::CountOverflow("ftdi.diagnostic_min"))?,
        "ftdi.diagnostic_min",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::FtdiLegacyCleanMinBuyTransactions,
        crate::tx_intelligence::MIN_CLEAN_BUY_SAMPLE_COUNT,
        "ftdi.legacy_clean_min",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::FtdiCandidateCleanMinUniqueBuyers,
        crate::tx_intelligence::MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2,
        "ftdi.corrected_clean_min",
    )
}

fn validate_tx_intelligence_snapshot_config(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    for (key, field) in [
        (
            MetricEffectiveConfigKeyV1::DevTxIntelDustThresholdSol,
            "dev.tx_intel_dust_threshold",
        ),
        (
            MetricEffectiveConfigKeyV1::DevPrimaryDustThresholdSol,
            "dev.primary_dust_threshold",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyDustThresholdSol,
            "timing.legacy_dust_threshold",
        ),
    ] {
        config_finite_matches(context, key, tx.producer_dust_filter_sol, field)?;
    }
    for (key, field) in [
        (
            MetricEffectiveConfigKeyV1::DevTxIntelDedupeCapacity,
            "dev.tx_intel_dedupe_capacity",
        ),
        (
            MetricEffectiveConfigKeyV1::DevPrimaryDedupeCapacity,
            "dev.primary_dedupe_capacity",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyDedupeCapacity,
            "timing.legacy_dedupe_capacity",
        ),
    ] {
        config_wide_matches(context, key, tx.producer_dedupe_capacity, field)?;
    }
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::SameMsExactDeltaMs,
        0,
        "timing.exact_delta_ms",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::SameMsClusterUpperBoundExclusiveMs,
        crate::tx_intelligence::BUNDLE_CLUSTER_THRESHOLD_MS,
        "timing.cluster_upper_bound_ms",
    )?;
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::DevTxIntelDedupeKey,
            "tx_key_v1",
            "dev.tx_intel_dedupe_key",
        ),
        (
            MetricEffectiveConfigKeyV1::DevPrimaryDedupeKey,
            "tx_key_v1",
            "dev.primary_dedupe_key",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyPopulation,
            "accepted_non_dust_successful_or_failed",
            "timing.legacy_population",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyDenominatorRule,
            "adjacent_exact_collisions_over_transaction_count",
            "timing.legacy_denominator_rule",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyDedupeKey,
            "tx_key_v1",
            "timing.legacy_dedupe_key",
        ),
    ] {
        config_enum_matches(context, key, expected, field)?;
    }

    let expected_dust = Some(tx.producer_dust_filter_sol);
    if tx.exact_same_ms.dust_filter_sol != expected_dust
        || tx.cluster_lt_50ms.dust_filter_sol != expected_dust
        || tx.exact_same_ms.source_state_capacity != Some(tx.producer_dedupe_capacity)
        || tx.cluster_lt_50ms.source_state_capacity != Some(tx.producer_dedupe_capacity)
        || !tx.exact_same_ms.canonical_dedupe_applied
        || !tx.cluster_lt_50ms.canonical_dedupe_applied
    {
        return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "tx_intelligence.snapshot",
        ));
    }
    Ok(())
}

fn validate_dev_producer_config(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    validate_tx_intelligence_snapshot_config(tx, context)?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::DevTxIntelSuccessEligibility,
        "accepted_successful_or_failed",
        "dev.tx_intel_success_eligibility",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::DevFirstObservedAnchorRule,
        "first_accepted_creator_buy_in_ingest_order",
        "dev.first_observed_anchor",
    )?;
    config_boolean_matches(
        context,
        MetricEffectiveConfigKeyV1::DevPrimarySuccessRequired,
        true,
        "dev.primary_success_required",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::DevPrimaryAnchorRule,
        "create_signature_then_earliest_eligible_creator_buy",
        "dev.primary_anchor",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::DevMissingCreatorBehavior,
        "unavailable",
        "dev.missing_creator_behavior",
    )
}

fn validate_top3_producer_config(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    validate_tx_intelligence_snapshot_config(tx, context)?;
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::Top3PreferredField,
            "top3_signer_volume_ratio",
            "top3.preferred_field",
        ),
        (
            MetricEffectiveConfigKeyV1::Top3FallbackAlias,
            "top3_volume_pct",
            "top3.fallback_alias",
        ),
        (
            MetricEffectiveConfigKeyV1::Top3Scale,
            "ratio_0_1",
            "top3.scale",
        ),
        (
            MetricEffectiveConfigKeyV1::Top3MismatchBehavior,
            "preferred_authoritative_emit_mismatch_telemetry",
            "top3.mismatch_behavior",
        ),
    ] {
        config_enum_matches(context, key, expected, field)?;
    }
    Ok(())
}

fn validate_funding_producer_config(
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    if producer_config.producer_config_hash() != config.metric_contract_producer_config_hash() {
        return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "fsc.producer_config_snapshot",
        ));
    }
    for (key, actual, field) in [
        (
            MetricEffectiveConfigKeyV1::FscFundingLookbackWindowMs,
            config.lookback_window_ms,
            "fsc.lookback_window_ms",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinAbsStoreLamports,
            config.min_abs_store_lamports,
            "fsc.min_abs_store_lamports",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinAbsAttributionLamports,
            config.min_abs_attribution_lamports,
            "fsc.min_abs_attribution_lamports",
        ),
        (
            MetricEffectiveConfigKeyV1::FscPerRecipientCapacity,
            u64::try_from(config.per_recipient_cap)
                .map_err(|_| Pr2aProducerErrorV1::CountOverflow("fsc.per_recipient_cap"))?,
            "fsc.per_recipient_cap",
        ),
        (
            MetricEffectiveConfigKeyV1::FscGlobalRecipientCapacity,
            u64::try_from(config.global_recipient_cap)
                .map_err(|_| Pr2aProducerErrorV1::CountOverflow("fsc.global_recipient_cap"))?,
            "fsc.global_recipient_cap",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinTotalBuyers,
            config.min_total_buyers,
            "fsc.min_total_buyers",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinKnownNonNeutralBuyers,
            config.min_known_non_neutral_buyers,
            "fsc.min_known_non_neutral_buyers",
        ),
    ] {
        config_wide_matches(context, key, actual, field)?;
    }
    match config_value(
        context,
        MetricEffectiveConfigKeyV1::FscMinAttributionConfidenceBps,
        "fsc.min_attribution_confidence_bps",
    )? {
        MetricEffectiveConfigValueV1::Unsigned(expected)
            if *expected == u32::from(config.min_attribution_confidence_bps) => {}
        _ => {
            return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
                "fsc.min_attribution_confidence_bps",
            ));
        }
    }
    for (key, actual, field) in [
        (
            MetricEffectiveConfigKeyV1::FscMinRelativeToBuy,
            config.min_rel_to_buy,
            "fsc.min_relative_to_buy",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinKnownCoverage,
            config.min_known_coverage,
            "fsc.min_known_coverage",
        ),
        (
            MetricEffectiveConfigKeyV1::FscMinNonNeutralKnownCoverage,
            config.min_non_neutral_known_coverage,
            "fsc.min_non_neutral_known_coverage",
        ),
    ] {
        match config_value(context, key, field)? {
            MetricEffectiveConfigValueV1::Ratio(expected)
                if expected.to_bits() == actual.to_bits() => {}
            _ => return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)),
        }
    }
    let actual_neutral_hash: CanonicalNullableV1<CanonicalHashV1> =
        config.metric_contract_neutral_funder_set_hash()?.into();
    match config_value(
        context,
        MetricEffectiveConfigKeyV1::FscNeutralFunderSetHash,
        "fsc.neutral_funder_set_hash",
    )? {
        MetricEffectiveConfigValueV1::NullableHash(expected)
            if expected == &actual_neutral_hash => {}
        _ => {
            return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
                "fsc.neutral_funder_set_hash",
            ));
        }
    }
    config_nullable_text_matches(
        context,
        MetricEffectiveConfigKeyV1::FscNeutralFunderSetVersion,
        &config.neutral_funder_set_version,
        "fsc.neutral_funder_set_version",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::FscWarmupWindowMs,
        producer_config.warmup_window_ms(),
        "fsc.warmup_window_ms",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::FscSameSlotOrderingPolicy,
        producer_config
            .same_slot_ordering_policy()
            .metric_contract_value(),
        "fsc.same_slot_ordering_policy",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::FscLegacyFormula,
        "one_minus_distinct_known_sources_over_known_source_samples",
        "fsc.legacy_formula",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
        crate::tx_intelligence::FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1,
        "fsc.legacy_min_known_source_samples",
    )
}

fn validate_recent_timing_snapshot_config(
    snapshot: &TxTimingProducerSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    let window_ms = snapshot
        .window_ms
        .ok_or(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "timing.recent_window_ms",
        ))?;
    let capacity =
        snapshot
            .source_state_capacity
            .ok_or(Pr2aProducerErrorV1::ProducerConfigMismatch(
                "timing.recent_retention_capacity",
            ))?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
        window_ms,
        "timing.recent_window_ms",
    )?;
    config_wide_matches(
        context,
        MetricEffectiveConfigKeyV1::SameMsRecentRetentionCapacity,
        capacity,
        "timing.recent_retention_capacity",
    )?;
    config_enum_matches(
        context,
        MetricEffectiveConfigKeyV1::SameMsRecentRetentionPolicy,
        "truncate_with_status",
        "timing.recent_retention_policy",
    )?;
    for (key, expected, field) in [
        (
            MetricEffectiveConfigKeyV1::SameMsRecentPopulation,
            "successful_accepted_recent_window",
            "timing.recent_population",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsRecentDenominatorRule,
            "same_timestamp_extras_over_transaction_count",
            "timing.recent_denominator_rule",
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsRecentDedupeKey,
            "gatekeeper_buffer_tx_key_v1",
            "timing.recent_dedupe_key",
        ),
    ] {
        config_enum_matches(context, key, expected, field)?;
    }
    if snapshot.dust_filter_sol.is_some() || !snapshot.canonical_dedupe_applied {
        return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "timing.recent_snapshot",
        ));
    }
    Ok(())
}

pub fn build_pr2a_evidence_families_v1(
    inputs: Pr2aFrozenProducerInputsV1<'_>,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<Pr2aParitySensitiveEvidenceFamiliesV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_pr2a_evidence_families_with_validated_context_v1(inputs, &validated)
}

/// Runtime-only entry point for a session context whose immutable profile and
/// effective config were already validated when installed. This does not
/// weaken the public raw builder: arbitrary callers still use
/// `build_pr2a_evidence_families_v1` and pay full trust-boundary validation.
pub(crate) fn build_pr2a_evidence_families_with_validated_static_context_v1(
    inputs: Pr2aFrozenProducerInputsV1<'_>,
    static_context: &MetricDecisionProjectionValidatedStaticContextV1,
) -> Result<Pr2aParitySensitiveEvidenceFamiliesV1, Pr2aProducerErrorV1> {
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: static_context.rollout_mode(),
        profile: static_context.profile(),
        effective_config: static_context.effective_config(),
    };
    let validated = ValidatedPr2aEvidenceBuildContextV1 { context: &context };
    build_pr2a_evidence_families_with_validated_context_v1(inputs, &validated)
}

fn build_pr2a_evidence_families_with_validated_context_v1(
    inputs: Pr2aFrozenProducerInputsV1<'_>,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<Pr2aParitySensitiveEvidenceFamiliesV1, Pr2aProducerErrorV1> {
    Ok(Pr2aParitySensitiveEvidenceFamiliesV1 {
        fee_topology_diversity_index: build_ftdi_evidence_v1_validated(inputs.ftdi, validated)?,
        dev_buy: build_dev_buy_evidence_v1_validated(
            inputs.tx_intelligence,
            inputs.gatekeeper_dev_primary,
            validated,
        )?,
        same_ms_tx_ratio: build_tx_timing_evidence_v1_validated(
            inputs.tx_intelligence,
            inputs.recent_exact_timing,
            validated,
        )?,
        top3_signer_volume_ratio: build_top3_evidence_v1_validated(
            inputs.tx_intelligence,
            validated,
        )?,
        funding_source_concentration: build_funding_evidence_v1_validated(
            inputs.fsc,
            inputs.funding_source_config,
            inputs.funding_source_producer_config,
            validated,
        )?,
        fsc_evidence_status: build_fsc_status_evidence_v1_validated(
            inputs.fsc,
            inputs.funding_source_config,
            inputs.funding_source_producer_config,
            validated,
        )?,
    })
}

pub fn build_ftdi_evidence_v1(
    computation: &FtdiComputation,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<FtdiEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_ftdi_evidence_v1_validated(computation, &validated)
}

fn build_ftdi_evidence_v1_validated(
    computation: &FtdiComputation,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<FtdiEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_ftdi_producer_config(context)?;
    if computation.unique_topology_count > computation.signer_sample_count {
        return Err(Pr2aProducerErrorV1::ProducerInvariant("ftdi.counts"));
    }
    let expected_value = computation
        .fee_topology_diversity_index
        .map(|_| computation.unique_topology_count as f64 / computation.signer_sample_count as f64);
    if (computation.fee_topology_diversity_index.is_some() && computation.signer_sample_count < 2)
        || !ratio_bits_equal(computation.fee_topology_diversity_index, expected_value)
        || computation.legacy_buy_tx_actionable
            != (expected_value.is_some()
                && computation.buy_sample_count
                    >= crate::tx_intelligence::MIN_CLEAN_BUY_SAMPLE_COUNT)
        || computation.unique_buyer_actionable_v2
            != (expected_value.is_some()
                && computation.signer_sample_count
                    >= crate::tx_intelligence::MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2)
    {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(
            "ftdi.value_or_actionability",
        ));
    }
    finite_ratio(
        computation.fee_topology_diversity_index,
        "fee_topology_diversity_index",
    )?;
    finite_ratio(computation.coordination_hhi, "coordination_hhi")?;
    let unique_topology_count = checked_u32(
        computation.unique_topology_count,
        "ftdi.unique_topology_count",
    )?;
    let unique_buyer_sample_count =
        checked_u32(computation.signer_sample_count, "ftdi.signer_sample_count")?;
    let buy_transaction_sample_count =
        checked_u32(computation.buy_sample_count, "ftdi.buy_sample_count")?;
    let legacy_reasons = computation
        .degraded_reasons
        .iter()
        .map(|reason| {
            adapt_legacy_metric_reason_v1(MetricContractId::FeeTopologyDiversityIndex, reason)
        })
        .collect::<Vec<_>>();
    let (legacy_availability, legacy_quality) =
        if computation.fee_topology_diversity_index.is_some() {
            if computation.legacy_buy_tx_actionable {
                (
                    MetricAvailabilityV1::Available,
                    MetricMeasurementQualityV1::Measured,
                )
            } else {
                (
                    MetricAvailabilityV1::Available,
                    MetricMeasurementQualityV1::Insufficient,
                )
            }
        } else {
            (
                MetricAvailabilityV1::Unavailable,
                MetricMeasurementQualityV1::NotApplicable,
            )
        };
    let value_v1_reasons = if computation.fee_topology_diversity_index.is_some() {
        Vec::new()
    } else if computation.signer_sample_count < 2 {
        vec![MetricEvidenceReasonV1::Ftdi(
            FtdiEvidenceReasonV1::InsufficientUniqueBuyers,
        )]
    } else {
        vec![MetricEvidenceReasonV1::Ftdi(
            FtdiEvidenceReasonV1::RawFeeTopologyUnavailable,
        )]
    };
    let value_v1_quality = if computation.fee_topology_diversity_index.is_some() {
        MetricMeasurementQualityV1::Measured
    } else {
        MetricMeasurementQualityV1::NotApplicable
    };
    let value_v1_availability = if computation.fee_topology_diversity_index.is_some() {
        MetricAvailabilityV1::Available
    } else {
        MetricAvailabilityV1::Unavailable
    };
    let legacy_value = FtdiValueMeasurementV1 {
        envelope: envelope(
            context,
            MetricContractId::FeeTopologyDiversityIndex,
            MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
            legacy_availability,
            legacy_quality,
            computation.legacy_buy_tx_actionable
                && surface_is_policy_authoritative(
                    context,
                    MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
                ),
            legacy_reasons,
        )?,
        value: computation.fee_topology_diversity_index.into(),
        unique_topology_count,
        unique_buyer_sample_count,
        buy_transaction_sample_count,
    };
    let value_v1 = FtdiValueMeasurementV1 {
        envelope: envelope(
            context,
            MetricContractId::FeeTopologyDiversityIndex,
            MetricSurfaceId::FtdiValueEvidenceV1,
            value_v1_availability,
            value_v1_quality,
            computation.fee_topology_diversity_index.is_some()
                && surface_is_policy_authoritative(context, MetricSurfaceId::FtdiValueEvidenceV1),
            value_v1_reasons,
        )?,
        value: computation.fee_topology_diversity_index.into(),
        unique_topology_count,
        unique_buyer_sample_count,
        buy_transaction_sample_count,
    };
    let legacy_actionability_quality = if computation.legacy_buy_tx_actionable {
        MetricMeasurementQualityV1::Measured
    } else {
        MetricMeasurementQualityV1::Insufficient
    };
    let corrected_quality = if computation.unique_buyer_actionable_v2 {
        MetricMeasurementQualityV1::Measured
    } else {
        MetricMeasurementQualityV1::Insufficient
    };

    Ok(FtdiEvidenceV1 {
        legacy_value,
        value_v1,
        legacy_actionability_envelope: envelope(
            context,
            MetricContractId::FeeTopologyDiversityIndex,
            MetricSurfaceId::FtdiLegacyBuyTxActionability,
            MetricAvailabilityV1::Available,
            legacy_actionability_quality,
            computation.legacy_buy_tx_actionable
                && surface_is_policy_authoritative(
                    context,
                    MetricSurfaceId::FtdiLegacyBuyTxActionability,
                ),
            vec![MetricEvidenceReasonV1::Ftdi(
                FtdiEvidenceReasonV1::LegacyBuyTransactionActionabilityGate,
            )],
        )?,
        legacy_buy_tx_actionable: computation.legacy_buy_tx_actionable,
        unique_buyer_actionability_v2_envelope: envelope(
            context,
            MetricContractId::FeeTopologyDiversityIndex,
            MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
            MetricAvailabilityV1::Available,
            corrected_quality,
            false,
            vec![MetricEvidenceReasonV1::Ftdi(
                FtdiEvidenceReasonV1::UniqueBuyerActionabilityCounterfactual,
            )],
        )?,
        unique_buyer_actionable_v2: computation.unique_buyer_actionable_v2,
        coordination_hhi_export_envelope: envelope(
            context,
            MetricContractId::FeeTopologyDiversityIndex,
            MetricSurfaceId::CoordinationFeeTopologyHhiExportV1,
            if computation.coordination_hhi.is_some() {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            if computation.coordination_hhi.is_some() {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::NotApplicable
            },
            false,
            vec![MetricEvidenceReasonV1::Ftdi(
                FtdiEvidenceReasonV1::CoordinationHhiExportOnly,
            )],
        )?,
        coordination_hhi: computation.coordination_hhi.into(),
    })
}

trait DevSnapshotViewV1 {
    fn amount_sol(&self) -> Option<f64>;
    fn creator_known(&self) -> bool;
    fn create_signature(&self) -> Option<String>;
    fn create_signature_matched(&self) -> bool;
    fn selection_mode(&self) -> DevBuySelectionModeV1;
    fn selected_signature(&self) -> Option<String>;
    fn selected_slot(&self) -> Option<u64>;
    fn selected_transaction_index(&self) -> Option<u32>;
    fn eligible_buy_count(&self) -> u64;
    fn selected_success(&self) -> Option<bool>;
    fn selection_complete(&self) -> bool;
}

impl DevSnapshotViewV1 for crate::tx_intelligence::DevBuyProducerSnapshotV1 {
    fn amount_sol(&self) -> Option<f64> {
        self.amount_sol
    }
    fn creator_known(&self) -> bool {
        self.creator_known
    }
    fn create_signature(&self) -> Option<String> {
        self.create_signature.clone()
    }
    fn create_signature_matched(&self) -> bool {
        self.create_signature_matched
    }
    fn selection_mode(&self) -> DevBuySelectionModeV1 {
        self.selection_mode
    }
    fn selected_signature(&self) -> Option<String> {
        self.selected_signature.clone()
    }
    fn selected_slot(&self) -> Option<u64> {
        self.selected_slot
    }
    fn selected_transaction_index(&self) -> Option<u32> {
        self.selected_transaction_index
    }
    fn eligible_buy_count(&self) -> u64 {
        self.eligible_buy_count
    }
    fn selected_success(&self) -> Option<bool> {
        self.selected_success
    }
    fn selection_complete(&self) -> bool {
        self.selection_complete
    }
}

impl DevSnapshotViewV1 for GatekeeperDevPrimaryCompatibilitySnapshotV1 {
    fn amount_sol(&self) -> Option<f64> {
        self.amount_sol
    }
    fn creator_known(&self) -> bool {
        self.creator_known
    }
    fn create_signature(&self) -> Option<String> {
        self.create_signature.clone()
    }
    fn create_signature_matched(&self) -> bool {
        self.create_signature_matched
    }
    fn selection_mode(&self) -> DevBuySelectionModeV1 {
        self.selection_mode
    }
    fn selected_signature(&self) -> Option<String> {
        self.selected_signature.clone()
    }
    fn selected_slot(&self) -> Option<u64> {
        self.selected_slot
    }
    fn selected_transaction_index(&self) -> Option<u32> {
        self.selected_transaction_index
    }
    fn eligible_buy_count(&self) -> u64 {
        self.eligible_buy_count
    }
    fn selected_success(&self) -> Option<bool> {
        self.selected_success
    }
    fn selection_complete(&self) -> bool {
        true
    }
}

fn dev_evidence_from_snapshot(
    snapshot: &impl DevSnapshotViewV1,
    surface: MetricSurfaceId,
    context: &Pr2aEvidenceBuildContextV1<'_>,
    counterfactual: bool,
    compatibility: bool,
) -> Result<DevBuyEvidenceV1, Pr2aProducerErrorV1> {
    if snapshot
        .amount_sol()
        .is_some_and(|value| !value.is_finite())
    {
        return Err(Pr2aProducerErrorV1::NonFinite("dev_buy.amount_sol"));
    }
    let selection_complete = snapshot.selection_complete();
    let amount = if selection_complete {
        snapshot.amount_sol()
    } else {
        None
    };
    let mut reasons = Vec::new();
    if !snapshot.creator_known() {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::CreatorUnknown,
        ));
    } else if amount.is_none() && selection_complete {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::NoEligibleBuy,
        ));
    }
    if snapshot.create_signature().is_none() {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::CreateSignatureUnavailable,
        ));
    } else if amount.is_some() && !snapshot.create_signature_matched() {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::CreateSignatureNotMatched,
        ));
    }
    if !selection_complete {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::CandidateHistoryTruncated,
        ));
    }
    if snapshot.selected_success() == Some(false) {
        reasons.push(MetricEvidenceReasonV1::DevBuy(if compatibility {
            DevBuyEvidenceReasonV1::CompatibilityPrimaryIncludesAcceptedFailed
        } else {
            DevBuyEvidenceReasonV1::LegacyFirstObservedIncludesAcceptedFailed
        }));
    }
    if counterfactual {
        reasons.push(MetricEvidenceReasonV1::DevBuy(
            DevBuyEvidenceReasonV1::PrimaryBuyCounterfactual,
        ));
    }
    let availability = if amount.is_some() {
        MetricAvailabilityV1::Available
    } else {
        MetricAvailabilityV1::Unavailable
    };
    let quality = if amount.is_none() {
        MetricMeasurementQualityV1::NotApplicable
    } else if snapshot.selected_success() == Some(false) {
        MetricMeasurementQualityV1::Degraded
    } else {
        MetricMeasurementQualityV1::Measured
    };
    let policy_actionable = amount.is_some()
        && !counterfactual
        && !compatibility
        && surface_is_policy_authoritative(context, surface);

    Ok(DevBuyEvidenceV1 {
        envelope: envelope(
            context,
            MetricContractId::DevBuy,
            surface,
            availability,
            quality,
            policy_actionable,
            reasons,
        )?,
        amount_sol: amount.into(),
        creator_known: snapshot.creator_known(),
        create_signature: snapshot.create_signature().into(),
        create_signature_matched: selection_complete && snapshot.create_signature_matched(),
        selection_mode: if selection_complete {
            snapshot.selection_mode()
        } else {
            DevBuySelectionModeV1::NoEligibleBuy
        },
        selected_signature: if selection_complete {
            snapshot.selected_signature()
        } else {
            None
        }
        .into(),
        selected_slot: if selection_complete {
            snapshot.selected_slot().map(CanonicalU64StringV1::new)
        } else {
            None
        }
        .into(),
        selected_transaction_index: if selection_complete {
            snapshot.selected_transaction_index()
        } else {
            None
        }
        .into(),
        eligible_buy_count: if selection_complete {
            checked_u32(snapshot.eligible_buy_count(), "dev.eligible_buy_count")?
        } else {
            0
        },
    })
}

pub fn build_dev_buy_evidence_v1(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    gatekeeper: &GatekeeperDevPrimaryCompatibilitySnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<DevBuyContractEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_dev_buy_evidence_v1_validated(tx, gatekeeper, &validated)
}

fn build_dev_buy_evidence_v1_validated(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    gatekeeper: &GatekeeperDevPrimaryCompatibilitySnapshotV1,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<DevBuyContractEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_dev_producer_config(tx, context)?;
    let tx_first = dev_evidence_from_snapshot(
        &tx.dev_first_observed,
        MetricSurfaceId::TxIntelDevFirstObservedBuySol,
        context,
        false,
        false,
    )?;
    let gatekeeper_primary = dev_evidence_from_snapshot(
        gatekeeper,
        MetricSurfaceId::GatekeeperBufferDevPrimaryBuySol,
        context,
        false,
        true,
    )?;
    let mfs_first = dev_evidence_from_snapshot(
        &tx.dev_first_observed,
        MetricSurfaceId::MfsDevFirstObservedBuySol,
        context,
        false,
        false,
    )?;
    let mfs_primary = dev_evidence_from_snapshot(
        &tx.dev_primary_v1,
        MetricSurfaceId::MfsDevPrimaryBuySolV1,
        context,
        true,
        false,
    )?;
    let effective = dev_evidence_from_snapshot(
        &tx.dev_first_observed,
        MetricSurfaceId::EffectivePolicyDevBuySol,
        context,
        false,
        false,
    )?;
    Ok(DevBuyContractEvidenceV1 {
        tx_intel_first_observed: tx_first,
        gatekeeper_buffer_primary: gatekeeper_primary,
        mfs_first_observed: mfs_first,
        mfs_primary_v1: mfs_primary,
        effective_policy: effective,
    })
}

fn timing_measurement(
    snapshot: &TxTimingProducerSnapshotV1,
    source: TxTimingSourceV1,
    population: TxTimingPopulationV1,
    surface: MetricSurfaceId,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<TxTimingMeasurementEvidenceV1, Pr2aProducerErrorV1> {
    finite_ratio(snapshot.ratio, "timing.ratio")?;
    finite_nonnegative(snapshot.dust_filter_sol, "timing.dust_filter_sol")?;
    validate_count_ratio(
        snapshot.numerator,
        snapshot.denominator,
        snapshot.ratio,
        "timing.count_ratio",
    )?;
    let numerator = checked_u32(snapshot.numerator, "timing.numerator")?;
    let denominator = checked_u32(snapshot.denominator, "timing.denominator")?;
    let mut reasons = match source {
        TxTimingSourceV1::TxIntelFullObservationExactLegacy => vec![
            MetricEvidenceReasonV1::TxTiming(TxTimingEvidenceReasonV1::ExactSameMillisecond),
            MetricEvidenceReasonV1::TxTiming(
                TxTimingEvidenceReasonV1::LegacyTransactionCountDenominator,
            ),
        ],
        TxTimingSourceV1::TxTimingFullObservationExactV1 => vec![MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::ExactSameMillisecond,
        )],
        TxTimingSourceV1::PhaseDiversityClusterLt50Ms => vec![MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::ClusterBelowFiftyMilliseconds,
        )],
        TxTimingSourceV1::RceRecentExact => vec![MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::RecentWindow,
        )],
    };
    if snapshot.fallback_timestamp_count > 0 {
        reasons.push(MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::TimestampUnavailable,
        ));
    }
    if snapshot.fallback_ordering_count > 0 {
        reasons.push(MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::OrderingIdentityUnavailable,
        ));
    }
    if !snapshot.source_complete {
        reasons.push(MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::SourceWindowTruncated,
        ));
    }
    if denominator < 2 {
        reasons.push(MetricEvidenceReasonV1::TxTiming(
            TxTimingEvidenceReasonV1::InsufficientTransactions,
        ));
    }
    let availability = if denominator == 0 {
        MetricAvailabilityV1::Unavailable
    } else {
        MetricAvailabilityV1::Available
    };
    let quality = if denominator == 0 {
        MetricMeasurementQualityV1::NotApplicable
    } else if denominator < 2 {
        MetricMeasurementQualityV1::Insufficient
    } else if snapshot.fallback_timestamp_count > 0
        || snapshot.fallback_ordering_count > 0
        || !snapshot.source_complete
    {
        MetricMeasurementQualityV1::Degraded
    } else {
        MetricMeasurementQualityV1::Measured
    };
    let policy_actionable = snapshot.ratio.is_some()
        && surface_is_policy_authoritative(context, surface)
        && !matches!(quality, MetricMeasurementQualityV1::Insufficient);
    let window_ms = snapshot
        .window_ms
        .map(|value| checked_u32(value, "timing.window_ms"))
        .transpose()?;
    Ok(TxTimingMeasurementEvidenceV1 {
        envelope: envelope(
            context,
            MetricContractId::SameMsTxRatio,
            surface,
            availability,
            quality,
            policy_actionable,
            reasons,
        )?,
        source,
        population,
        canonical_dedupe_applied: snapshot.canonical_dedupe_applied,
        dust_filter_sol: snapshot.dust_filter_sol.into(),
        window_ms: window_ms.into(),
        numerator,
        denominator,
        ratio: snapshot.ratio.into(),
    })
}

pub fn build_tx_timing_evidence_v1(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    recent_exact: &TxTimingProducerSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<TxTimingEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_tx_timing_evidence_v1_validated(tx, recent_exact, &validated)
}

fn build_tx_timing_evidence_v1_validated(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    recent_exact: &TxTimingProducerSnapshotV1,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<TxTimingEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_tx_intelligence_snapshot_config(tx, context)?;
    validate_recent_timing_snapshot_config(recent_exact, context)?;
    Ok(TxTimingEvidenceV1 {
        legacy_exact: timing_measurement(
            &tx.exact_same_ms,
            TxTimingSourceV1::TxIntelFullObservationExactLegacy,
            TxTimingPopulationV1::AcceptedTransactions,
            MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
            context,
        )?,
        exact_v1: timing_measurement(
            &tx.exact_same_ms,
            TxTimingSourceV1::TxTimingFullObservationExactV1,
            TxTimingPopulationV1::AcceptedTransactions,
            MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
            context,
        )?,
        cluster_lt_50ms: timing_measurement(
            &tx.cluster_lt_50ms,
            TxTimingSourceV1::PhaseDiversityClusterLt50Ms,
            TxTimingPopulationV1::AcceptedTransactions,
            MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
            context,
        )?,
        recent_exact: timing_measurement(
            recent_exact,
            TxTimingSourceV1::RceRecentExact,
            TxTimingPopulationV1::SuccessfulTransactions,
            MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
            context,
        )?,
    })
}

fn top3_envelope(
    value: Option<f64>,
    surface: MetricSurfaceId,
    context: &Pr2aEvidenceBuildContextV1<'_>,
    reasons: Vec<MetricEvidenceReasonV1>,
) -> Result<CanonicalMetricEnvelopeV1, Pr2aProducerErrorV1> {
    envelope(
        context,
        MetricContractId::Top3SignerVolumeRatio,
        surface,
        if value.is_some() {
            MetricAvailabilityV1::Available
        } else {
            MetricAvailabilityV1::Unavailable
        },
        if value.is_some() {
            MetricMeasurementQualityV1::Measured
        } else {
            MetricMeasurementQualityV1::NotApplicable
        },
        value.is_some() && surface_is_policy_authoritative(context, surface),
        reasons,
    )
}

pub fn build_top3_evidence_v1(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<Top3SignerVolumeEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_top3_evidence_v1_validated(tx, &validated)
}

fn build_top3_evidence_v1_validated(
    tx: &TxIntelligenceMetricContractSnapshotV1,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<Top3SignerVolumeEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_top3_producer_config(tx, context)?;
    finite_ratio(tx.top3.preferred_ratio, "top3.preferred")?;
    finite_ratio(tx.top3.compatibility_alias_ratio, "top3.alias")?;
    finite_ratio(tx.top3.effective_ratio, "top3.effective")?;
    let expected_effective = tx
        .top3
        .preferred_ratio
        .or(tx.top3.compatibility_alias_ratio);
    let expected_equal = match (tx.top3.preferred_ratio, tx.top3.compatibility_alias_ratio) {
        (Some(left), Some(right)) => Some(left.to_bits() == right.to_bits()),
        _ => None,
    };
    if !ratio_bits_equal(tx.top3.effective_ratio, expected_effective)
        || tx.top3.preferred_alias_bitwise_equal != expected_equal
        || tx.top3.used_compatibility_fallback
            != (tx.top3.preferred_ratio.is_none() && tx.top3.compatibility_alias_ratio.is_some())
    {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(
            "top3.effective_selector",
        ));
    }
    let mismatch = tx.top3.preferred_alias_bitwise_equal == Some(false);
    let fallback = tx.top3.used_compatibility_fallback;
    ::metrics::counter!(
        "metric_contract_top3_selector_total",
        1,
        "outcome" => if mismatch {
            "preferred_alias_mismatch"
        } else if fallback {
            "compatibility_fallback"
        } else {
            "preferred"
        }
    );
    let mismatch_reason = mismatch.then_some(MetricEvidenceReasonV1::Top3(
        Top3EvidenceReasonV1::PreferredAliasMismatch,
    ));
    let preferred_reasons = mismatch_reason.clone().into_iter().collect();
    let mut alias_reasons = mismatch_reason.into_iter().collect::<Vec<_>>();
    if fallback {
        alias_reasons.push(MetricEvidenceReasonV1::Top3(
            Top3EvidenceReasonV1::CompatibilityAliasFallback,
        ));
    }
    if tx.top3.preferred_ratio.is_none() {
        alias_reasons.push(MetricEvidenceReasonV1::Top3(
            Top3EvidenceReasonV1::PreferredFieldUnavailable,
        ));
    }
    Ok(Top3SignerVolumeEvidenceV1 {
        preferred_envelope: top3_envelope(
            tx.top3.preferred_ratio,
            MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
            context,
            preferred_reasons,
        )?,
        preferred_ratio: tx.top3.preferred_ratio.into(),
        compatibility_alias_envelope: top3_envelope(
            tx.top3.compatibility_alias_ratio,
            MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
            context,
            alias_reasons,
        )?,
        compatibility_alias_ratio: tx.top3.compatibility_alias_ratio.into(),
        effective_selector_envelope: top3_envelope(
            tx.top3.effective_ratio,
            MetricSurfaceId::TxIntelTop3EffectiveSelector,
            context,
            Vec::new(),
        )?,
        effective_ratio: tx.top3.effective_ratio.into(),
        preferred_alias_bitwise_equal: tx.top3.preferred_alias_bitwise_equal.into(),
        used_compatibility_fallback: fallback,
    })
}

fn fsc_legacy_status(
    computation: &FscComputation,
) -> (MetricAvailabilityV1, MetricMeasurementQualityV1) {
    if computation.funding_source_concentration.is_some() {
        (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
        )
    } else {
        (
            MetricAvailabilityV1::Unavailable,
            MetricMeasurementQualityV1::NotApplicable,
        )
    }
}

fn validate_fsc_computation_provenance(
    computation: &FscComputation,
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
) -> Result<(), Pr2aProducerErrorV1> {
    let evidence = &computation.funding_source_v2;
    if evidence.config_hash != producer_config.producer_config_hash() {
        return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "fsc.computation_config_hash",
        ));
    }

    // Defensive embedded-setting checks make corruption attributable even
    // though the owner-produced fingerprint above binds every config field.
    if evidence.min_abs_store_lamports != config.min_abs_store_lamports
        || evidence.min_abs_attribution_lamports != config.min_abs_attribution_lamports
        || evidence.min_rel_to_buy.to_bits() != config.min_rel_to_buy.to_bits()
        || evidence.ttl_seconds != config.lookback_window_ms / 1_000
        || evidence.neutral_funder_set_version != config.neutral_funder_set_version
        || evidence.neutral_funder_set_hash
            != config.metric_contract_neutral_funder_set_producer_hash()
    {
        return Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "fsc.computation_embedded_settings",
        ));
    }
    Ok(())
}

fn validate_fsc_computation_semantics(
    computation: &FscComputation,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<(), Pr2aProducerErrorV1> {
    let evidence = &computation.funding_source_v2;
    let total_buyers = u64::from(evidence.total_buyers);
    let known_buyers = u64::from(evidence.known_buyers);
    let known_non_neutral_buyers = u64::from(evidence.known_non_neutral_buyers);

    if known_non_neutral_buyers > known_buyers || known_buyers > total_buyers {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_buyer_counts",
        ));
    }
    if total_buyers == 0 {
        if known_buyers != 0
            || known_non_neutral_buyers != 0
            || evidence.known_coverage.to_bits() != 0.0_f64.to_bits()
            || evidence.non_neutral_known_coverage.to_bits() != 0.0_f64.to_bits()
            || evidence.status != FscEvidenceStatus::Unavailable
        {
            return Err(Pr2aProducerErrorV1::ProducerInvariant("fsc.v2_zero_total"));
        }
        return Ok(());
    }

    let expected_known_coverage = known_buyers as f64 / total_buyers as f64;
    if evidence.known_coverage.to_bits() != expected_known_coverage.to_bits() {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_known_coverage",
        ));
    }
    let expected_non_neutral_coverage = known_non_neutral_buyers as f64 / total_buyers as f64;
    if evidence.non_neutral_known_coverage.to_bits() != expected_non_neutral_coverage.to_bits() {
        return Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_non_neutral_known_coverage",
        ));
    }

    if evidence.status == FscEvidenceStatus::Clean {
        let minimum_total_buyers = config_wide_value(
            context,
            MetricEffectiveConfigKeyV1::FscMinTotalBuyers,
            "fsc.min_total_buyers",
        )?;
        let minimum_known_non_neutral_buyers = config_wide_value(
            context,
            MetricEffectiveConfigKeyV1::FscMinKnownNonNeutralBuyers,
            "fsc.min_known_non_neutral_buyers",
        )?;
        let minimum_known_coverage = config_ratio_value(
            context,
            MetricEffectiveConfigKeyV1::FscMinKnownCoverage,
            "fsc.min_known_coverage",
        )?;
        let minimum_non_neutral_known_coverage = config_ratio_value(
            context,
            MetricEffectiveConfigKeyV1::FscMinNonNeutralKnownCoverage,
            "fsc.min_non_neutral_known_coverage",
        )?;
        if total_buyers < minimum_total_buyers
            || known_non_neutral_buyers < minimum_known_non_neutral_buyers
            || evidence.known_coverage < minimum_known_coverage
            || evidence.non_neutral_known_coverage < minimum_non_neutral_known_coverage
        {
            return Err(Pr2aProducerErrorV1::ProducerInvariant(
                "fsc.v2_clean_effective_config_minimum",
            ));
        }
    }
    Ok(())
}

pub fn build_funding_evidence_v1(
    computation: &FscComputation,
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<FundingSourceContractEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_funding_evidence_v1_validated(computation, config, producer_config, &validated)
}

fn build_funding_evidence_v1_validated(
    computation: &FscComputation,
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<FundingSourceContractEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_funding_producer_config(config, producer_config, context)?;
    validate_fsc_computation_provenance(computation, config, producer_config)?;
    validate_fsc_computation_semantics(computation, context)?;
    finite_ratio(computation.funding_source_concentration, "fsc.legacy")?;
    finite_ratio(
        Some(computation.funding_source_v2.known_coverage),
        "fsc.known_coverage",
    )?;
    finite_ratio(
        Some(computation.funding_source_v2.non_neutral_known_coverage),
        "fsc.non_neutral_known_coverage",
    )?;
    if computation.distinct_known_source_count > computation.known_source_sample_count {
        return Err(Pr2aProducerErrorV1::ProducerInvariant("fsc.legacy_counts"));
    }
    let expected_legacy = (computation.known_source_sample_count
        >= crate::tx_intelligence::FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1)
        .then(|| {
            1.0 - computation.distinct_known_source_count as f64
                / computation.known_source_sample_count as f64
        });
    if !ratio_bits_equal(computation.funding_source_concentration, expected_legacy) {
        return Err(Pr2aProducerErrorV1::ProducerInvariant("fsc.legacy_formula"));
    }
    let distinct_known_source_count = checked_u32(
        computation.distinct_known_source_count,
        "fsc.distinct_known_source_count",
    )?;
    let known_source_sample_count = checked_u32(
        computation.known_source_sample_count,
        "fsc.known_source_sample_count",
    )?;
    let legacy_reasons = computation
        .degraded_reasons
        .iter()
        .map(|reason| {
            adapt_legacy_metric_reason_v1(MetricContractId::FundingSourceConcentration, reason)
        })
        .collect::<Vec<_>>();
    let (legacy_availability, legacy_quality) = fsc_legacy_status(computation);
    let legacy_measurement =
        |surface| -> Result<FundingSourceLegacyMeasurementV1, Pr2aProducerErrorV1> {
            Ok(FundingSourceLegacyMeasurementV1 {
                envelope: envelope(
                    context,
                    MetricContractId::FundingSourceConcentration,
                    surface,
                    legacy_availability,
                    legacy_quality,
                    computation.funding_source_concentration.is_some()
                        && surface_is_policy_authoritative(context, surface),
                    legacy_reasons.clone(),
                )?,
                ratio: computation.funding_source_concentration.into(),
                distinct_known_source_count,
                known_source_sample_count,
            })
        };
    let (v2_availability, v2_quality) = match computation.funding_source_v2.status {
        FscEvidenceStatus::Clean => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
        ),
        FscEvidenceStatus::Degraded => (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Degraded,
        ),
        FscEvidenceStatus::Unavailable => (
            MetricAvailabilityV1::Unavailable,
            MetricMeasurementQualityV1::NotApplicable,
        ),
    };
    let v2_reasons = computation
        .funding_source_v2
        .excluded_reason
        .map(adapt_fsc_excluded_reason_v1)
        .into_iter()
        .collect::<Vec<_>>();
    let hhi = computation.funding_source_v2.hhi_norm_count;
    finite_ratio(hhi, "fsc.coordination_hhi")?;
    let v2_coverage = match computation.funding_source_v2.status {
        FscEvidenceStatus::Unavailable => CanonicalNullableV1::Null,
        FscEvidenceStatus::Clean | FscEvidenceStatus::Degraded => {
            CanonicalNullableV1::Value(computation.funding_source_v2.known_coverage)
        }
    };
    let v2_non_neutral_coverage = match computation.funding_source_v2.status {
        FscEvidenceStatus::Unavailable => CanonicalNullableV1::Null,
        FscEvidenceStatus::Clean | FscEvidenceStatus::Degraded => {
            CanonicalNullableV1::Value(computation.funding_source_v2.non_neutral_known_coverage)
        }
    };
    Ok(FundingSourceContractEvidenceV1 {
        legacy_source: legacy_measurement(
            MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
        )?,
        legacy_v1: legacy_measurement(MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1)?,
        v2_envelope: envelope(
            context,
            MetricContractId::FundingSourceConcentration,
            MetricSurfaceId::FundingSourceV2ReadinessEvidence,
            v2_availability,
            v2_quality,
            false,
            v2_reasons,
        )?,
        v2_status: computation.funding_source_v2.status,
        known_coverage: v2_coverage,
        non_neutral_known_coverage: v2_non_neutral_coverage,
        known_buyer_count: u32::from(computation.funding_source_v2.known_buyers),
        known_non_neutral_buyer_count: u32::from(
            computation.funding_source_v2.known_non_neutral_buyers,
        ),
        total_buyer_count: u32::from(computation.funding_source_v2.total_buyers),
        provider: CanonicalNullableV1::Value(computation.funding_source_v2.provider.clone()),
        config_hash: CanonicalNullableV1::Value(
            context
                .effective_config
                .metric_contract_effective_config_hash
                .clone(),
        ),
        coordination_hhi_export_envelope: envelope(
            context,
            MetricContractId::FundingSourceConcentration,
            MetricSurfaceId::CoordinationFundingSourceHhiExportV1,
            if hhi.is_some() {
                MetricAvailabilityV1::Available
            } else {
                MetricAvailabilityV1::Unavailable
            },
            if hhi.is_some() {
                MetricMeasurementQualityV1::Measured
            } else {
                MetricMeasurementQualityV1::NotApplicable
            },
            false,
            Vec::new(),
        )?,
        coordination_hhi: hhi.into(),
    })
}

pub fn build_fsc_status_evidence_v1(
    computation: &FscComputation,
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
    context: &Pr2aEvidenceBuildContextV1<'_>,
) -> Result<FscStatusEvidenceV1, Pr2aProducerErrorV1> {
    let validated = ValidatedPr2aEvidenceBuildContextV1::try_new(context)?;
    build_fsc_status_evidence_v1_validated(computation, config, producer_config, &validated)
}

fn build_fsc_status_evidence_v1_validated(
    computation: &FscComputation,
    config: &FundingSourceConfig,
    producer_config: &FundingSourceProducerConfigSnapshotV1,
    validated: &ValidatedPr2aEvidenceBuildContextV1<'_, '_>,
) -> Result<FscStatusEvidenceV1, Pr2aProducerErrorV1> {
    let context = validated.context;
    validate_funding_producer_config(config, producer_config, context)?;
    validate_fsc_computation_provenance(computation, config, producer_config)?;
    validate_fsc_computation_semantics(computation, context)?;
    let legacy_scalar_present = computation.funding_source_concentration.is_some();
    let legacy_feature_status = if legacy_scalar_present {
        EvidenceStatus::Clean
    } else if computation.diagnostics.buyer_sample_count > 0
        || !computation.degraded_reasons.is_empty()
    {
        EvidenceStatus::Degraded
    } else {
        EvidenceStatus::Unavailable
    };
    let (availability, quality) = if legacy_scalar_present {
        (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
        )
    } else {
        (
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Degraded,
        )
    };
    Ok(FscStatusEvidenceV1 {
        envelope: envelope(
            context,
            MetricContractId::FscEvidenceStatus,
            MetricSurfaceId::MaterializedFscStatusCompatibility,
            availability,
            quality,
            false,
            vec![MetricEvidenceReasonV1::FundingSource(
                FundingSourceEvidenceReasonV1::LegacyScalarPresenceOnly,
            )],
        )?,
        legacy_scalar_present,
        legacy_feature_status,
        fsc_v2_status: CanonicalNullableV1::Value(computation.funding_source_v2.status),
        fsc_v2_coverage: match computation.funding_source_v2.status {
            FscEvidenceStatus::Unavailable => CanonicalNullableV1::Null,
            FscEvidenceStatus::Clean | FscEvidenceStatus::Degraded => {
                CanonicalNullableV1::Value(computation.funding_source_v2.known_coverage)
            }
        },
    })
}

pub fn projection_hash_v1(
    projection: &ghost_core::metric_contracts::MetricContractDecisionEvidenceProjectionV1,
    context: &MetricDecisionProjectionBuildContextV1<'_>,
) -> Result<CanonicalHashV1, Pr2aProducerErrorV1> {
    projection
        .validated_canonical_hash(context)
        .map_err(Into::into)
}
