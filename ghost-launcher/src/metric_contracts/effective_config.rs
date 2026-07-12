use crate::session::observation::RCE_RECENT_WINDOW_MS_V1;
use crate::tx_intelligence::{
    FundingSourceConfig, TxIntelligenceConfig, BUNDLE_CLUSTER_THRESHOLD_MS,
    FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1, MIN_CLEAN_BUY_SAMPLE_COUNT,
    MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2, MIN_DIAGNOSTIC_SAMPLE_COUNT,
};
use ghost_brain::config::{FscV2Config, GatekeeperV2Config, MetricContractFoundationConfigV1};
use ghost_core::metric_contracts::{
    CanonicalU64StringV1, MetricContractEffectiveConfigBuilderV1,
    MetricContractEffectiveConfigErrorV1, MetricEffectiveConfigKeyV1 as Key,
    MetricEffectiveConfigValueV1 as Value, ResolvedMetricContractEffectiveConfigV1,
};
use seer::early_fingerprint::EarlyFingerprintConfig;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetricContractRuntimeConfigErrorV1 {
    #[error(transparent)]
    EffectiveConfig(#[from] MetricContractEffectiveConfigErrorV1),
    #[error("resolved producer settings disagree: {0}")]
    ProducerSettingsMismatch(&'static str),
    #[error("resolved producer setting overflows its metric-contract representation: {0}")]
    ProducerSettingOverflow(&'static str),
}

fn wide(value: u64) -> Value {
    Value::WideUnsigned(CanonicalU64StringV1::new(value))
}

fn enum_value(value: &str) -> Value {
    Value::Enum(value.to_string())
}

fn insert(
    builder: &mut MetricContractEffectiveConfigBuilderV1,
    key: Key,
    value: Value,
) -> Result<(), MetricContractRuntimeConfigErrorV1> {
    builder.insert(key, value)?;
    Ok(())
}

fn usize_to_u64(
    value: usize,
    field: &'static str,
) -> Result<u64, MetricContractRuntimeConfigErrorV1> {
    u64::try_from(value)
        .map_err(|_| MetricContractRuntimeConfigErrorV1::ProducerSettingOverflow(field))
}

fn checked_millis(
    seconds: u64,
    field: &'static str,
) -> Result<u64, MetricContractRuntimeConfigErrorV1> {
    seconds
        .checked_mul(1_000)
        .ok_or(MetricContractRuntimeConfigErrorV1::ProducerSettingOverflow(
            field,
        ))
}

fn same_f64(left: f64, right: f64) -> bool {
    left.to_bits() == right.to_bits()
}

fn validate_resolved_producer_settings(
    gatekeeper: &GatekeeperV2Config,
    tx: &TxIntelligenceConfig,
) -> Result<(), MetricContractRuntimeConfigErrorV1> {
    for (matches, field) in [
        (
            same_f64(tx.min_sol_threshold, gatekeeper.min_sol_threshold),
            "min_sol_threshold",
        ),
        (
            tx.observation_window_ms == gatekeeper.max_wait_time_ms,
            "observation_window_ms",
        ),
        (
            same_f64(tx.max_same_ms_tx_ratio, gatekeeper.max_same_ms_tx_ratio),
            "max_same_ms_tx_ratio",
        ),
        (
            same_f64(tx.max_top3_volume_pct, gatekeeper.max_top3_volume_pct),
            "max_top3_volume_pct",
        ),
        (same_f64(tx.max_hhi, gatekeeper.max_hhi), "max_hhi"),
        (
            tx.max_tx_per_signer == gatekeeper.max_tx_per_signer,
            "max_tx_per_signer",
        ),
        (
            same_f64(tx.max_dev_volume_ratio, gatekeeper.max_dev_volume_ratio),
            "max_dev_volume_ratio",
        ),
    ] {
        if !matches {
            return Err(MetricContractRuntimeConfigErrorV1::ProducerSettingsMismatch(field));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_metric_contract_effective_config_v1(
    foundation: MetricContractFoundationConfigV1,
    gatekeeper: &GatekeeperV2Config,
    tx: &TxIntelligenceConfig,
    fingerprint: &EarlyFingerprintConfig,
    funding: &FundingSourceConfig,
    fsc_v2: Option<&FscV2Config>,
) -> Result<ResolvedMetricContractEffectiveConfigV1, MetricContractRuntimeConfigErrorV1> {
    validate_resolved_producer_settings(gatekeeper, tx)?;
    let funding_producer_snapshot = funding
        .metric_contract_producer_config_snapshot(fsc_v2)
        .map_err(|_| {
            MetricContractRuntimeConfigErrorV1::ProducerSettingsMismatch(
                "fsc producer settings snapshot",
            )
        })?;
    let mut builder = MetricContractEffectiveConfigBuilderV1::new(foundation)?;

    insert(
        &mut builder,
        Key::FtdiPopulationSuccessfulBuy,
        enum_value("successful_buy"),
    )?;
    insert(
        &mut builder,
        Key::FtdiFirstSamplePerSigner,
        Value::Boolean(true),
    )?;
    insert(
        &mut builder,
        Key::FtdiMissingSignerBehavior,
        enum_value("legacy_empty_signer_identity"),
    )?;
    insert(
        &mut builder,
        Key::FtdiMissingTopologyBehavior,
        enum_value("unavailable_entire_metric"),
    )?;
    insert(
        &mut builder,
        Key::FtdiDiagnosticMinUniqueBuyers,
        wide(usize_to_u64(
            MIN_DIAGNOSTIC_SAMPLE_COUNT,
            "MIN_DIAGNOSTIC_SAMPLE_COUNT",
        )?),
    )?;
    insert(
        &mut builder,
        Key::FtdiLegacyCleanMinBuyTransactions,
        wide(MIN_CLEAN_BUY_SAMPLE_COUNT),
    )?;
    insert(
        &mut builder,
        Key::FtdiCandidateCleanMinUniqueBuyers,
        wide(MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2),
    )?;
    insert(
        &mut builder,
        Key::FtdiDenominatorRule,
        enum_value("unique_topologies_over_unique_first_buyer_samples"),
    )?;

    insert(
        &mut builder,
        Key::DevTxIntelSuccessEligibility,
        enum_value("accepted_successful_or_failed"),
    )?;
    insert(
        &mut builder,
        Key::DevTxIntelDustThresholdSol,
        Value::FiniteNumber(tx.min_sol_threshold),
    )?;
    insert(
        &mut builder,
        Key::DevTxIntelDedupeKey,
        enum_value("tx_key_v1"),
    )?;
    insert(
        &mut builder,
        Key::DevTxIntelDedupeCapacity,
        wide(usize_to_u64(tx.tx_key_capacity, "tx_key_capacity")?),
    )?;
    insert(
        &mut builder,
        Key::DevFirstObservedAnchorRule,
        enum_value("first_accepted_creator_buy_in_ingest_order"),
    )?;
    insert(
        &mut builder,
        Key::DevPrimarySuccessRequired,
        Value::Boolean(true),
    )?;
    insert(
        &mut builder,
        Key::DevPrimaryDustThresholdSol,
        Value::FiniteNumber(tx.min_sol_threshold),
    )?;
    insert(
        &mut builder,
        Key::DevPrimaryDedupeKey,
        enum_value("tx_key_v1"),
    )?;
    insert(
        &mut builder,
        Key::DevPrimaryDedupeCapacity,
        wide(usize_to_u64(tx.tx_key_capacity, "tx_key_capacity")?),
    )?;
    insert(
        &mut builder,
        Key::DevPrimaryAnchorRule,
        enum_value("create_signature_then_earliest_eligible_creator_buy"),
    )?;
    insert(
        &mut builder,
        Key::DevMissingCreatorBehavior,
        enum_value("unavailable"),
    )?;

    insert(&mut builder, Key::SameMsExactDeltaMs, wide(0))?;
    insert(
        &mut builder,
        Key::SameMsLegacyPopulation,
        enum_value("accepted_non_dust_successful_or_failed"),
    )?;
    insert(
        &mut builder,
        Key::SameMsLegacyDenominatorRule,
        enum_value("adjacent_exact_collisions_over_transaction_count"),
    )?;
    insert(
        &mut builder,
        Key::SameMsLegacyDustThresholdSol,
        Value::FiniteNumber(tx.min_sol_threshold),
    )?;
    insert(
        &mut builder,
        Key::SameMsLegacyDedupeKey,
        enum_value("tx_key_v1"),
    )?;
    insert(
        &mut builder,
        Key::SameMsLegacyDedupeCapacity,
        wide(usize_to_u64(tx.tx_key_capacity, "tx_key_capacity")?),
    )?;
    insert(
        &mut builder,
        Key::SameMsClusterUpperBoundExclusiveMs,
        wide(BUNDLE_CLUSTER_THRESHOLD_MS),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentWindowMs,
        wide(RCE_RECENT_WINDOW_MS_V1),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentPopulation,
        enum_value("successful_accepted_recent_window"),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentDenominatorRule,
        enum_value("same_timestamp_extras_over_transaction_count"),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentDedupeKey,
        enum_value("gatekeeper_buffer_tx_key_v1"),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentRetentionCapacity,
        wide(usize_to_u64(
            gatekeeper.decision_time_series_tx_capacity.max(1),
            "decision_time_series_tx_capacity",
        )?),
    )?;
    insert(
        &mut builder,
        Key::SameMsRecentRetentionPolicy,
        enum_value(match gatekeeper.decision_time_series_retention_policy {
            ghost_core::checkpoint::DecisionTimeSeriesRetentionPolicy::TruncateWithStatus => {
                "truncate_with_status"
            }
        }),
    )?;

    insert(
        &mut builder,
        Key::Top3PreferredField,
        enum_value("top3_signer_volume_ratio"),
    )?;
    insert(
        &mut builder,
        Key::Top3FallbackAlias,
        enum_value("top3_volume_pct"),
    )?;
    insert(&mut builder, Key::Top3Scale, enum_value("ratio_0_1"))?;
    insert(
        &mut builder,
        Key::Top3MismatchBehavior,
        enum_value("preferred_authoritative_emit_mismatch_telemetry"),
    )?;

    insert(
        &mut builder,
        Key::FlipLegacyWindowSemantics,
        enum_value("first_buy_slot_to_last_sell_slot_gap"),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateWallClockWindowMs,
        wide(checked_millis(
            fingerprint.window_secs,
            "fingerprint.window_secs",
        )?),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateMaxSlotGap,
        wide(fingerprint.max_flip_slots),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateDumpRatio,
        Value::Ratio(fingerprint.flip_dump_pct),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateAnchorRule,
        enum_value("first_eligible_buy"),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateOrderPolicy,
        enum_value("stable_tx_key"),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateSuccessRequired,
        Value::Boolean(true),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateDustThresholdSol,
        Value::FiniteNumber(tx.min_sol_threshold),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateDedupeKey,
        enum_value("tx_key_v1"),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateDedupeCapacity,
        wide(usize_to_u64(tx.tx_key_capacity, "tx_key_capacity")?),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateEvictionPolicy,
        enum_value("fail_closed_on_capacity_or_gap"),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateMaxWallets,
        wide(usize_to_u64(
            fingerprint.max_wallets,
            "fingerprint.max_wallets",
        )?),
    )?;
    insert(
        &mut builder,
        Key::FlipCandidateReconnectBehavior,
        enum_value("unavailable_on_gap"),
    )?;

    insert(
        &mut builder,
        Key::FscLegacyFormula,
        enum_value("one_minus_distinct_known_sources_over_known_source_samples"),
    )?;
    insert(
        &mut builder,
        Key::FscLegacyMinKnownSourceSamples,
        wide(FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1),
    )?;
    insert(
        &mut builder,
        Key::FscFundingLookbackWindowMs,
        wide(funding.lookback_window_ms),
    )?;
    insert(
        &mut builder,
        Key::FscMinAbsStoreLamports,
        wide(funding.min_abs_store_lamports),
    )?;
    insert(
        &mut builder,
        Key::FscMinAbsAttributionLamports,
        wide(funding.min_abs_attribution_lamports),
    )?;
    insert(
        &mut builder,
        Key::FscMinRelativeToBuy,
        Value::Ratio(funding.min_rel_to_buy),
    )?;
    insert(
        &mut builder,
        Key::FscMinAttributionConfidenceBps,
        Value::Unsigned(u32::from(funding.min_attribution_confidence_bps)),
    )?;
    insert(
        &mut builder,
        Key::FscPerRecipientCapacity,
        wide(usize_to_u64(
            funding.per_recipient_cap,
            "fsc_per_recipient_cap",
        )?),
    )?;
    insert(
        &mut builder,
        Key::FscGlobalRecipientCapacity,
        wide(usize_to_u64(
            funding.global_recipient_cap,
            "fsc_global_recipient_cap",
        )?),
    )?;
    insert(
        &mut builder,
        Key::FscWarmupWindowMs,
        wide(funding_producer_snapshot.warmup_window_ms()),
    )?;
    insert(
        &mut builder,
        Key::FscMinTotalBuyers,
        wide(funding.min_total_buyers),
    )?;
    insert(
        &mut builder,
        Key::FscMinKnownNonNeutralBuyers,
        wide(funding.min_known_non_neutral_buyers),
    )?;
    insert(
        &mut builder,
        Key::FscMinKnownCoverage,
        Value::Ratio(funding.min_known_coverage),
    )?;
    insert(
        &mut builder,
        Key::FscMinNonNeutralKnownCoverage,
        Value::Ratio(funding.min_non_neutral_known_coverage),
    )?;
    insert(
        &mut builder,
        Key::FscSameSlotOrderingPolicy,
        enum_value(
            funding_producer_snapshot
                .same_slot_ordering_policy()
                .metric_contract_value(),
        ),
    )?;
    insert(
        &mut builder,
        Key::FscNeutralFunderSetVersion,
        Value::NullableText(funding.neutral_funder_set_version.clone().into()),
    )?;
    insert(
        &mut builder,
        Key::FscNeutralFunderSetHash,
        Value::NullableHash(
            funding
                .metric_contract_neutral_funder_set_hash()
                .map_err(MetricContractEffectiveConfigErrorV1::Hash)?
                .into(),
        ),
    )?;
    insert(
        &mut builder,
        Key::FscFundingStreamUnavailableBehavior,
        enum_value("legacy_null_and_v2_unavailable"),
    )?;
    insert(
        &mut builder,
        Key::FscLegacyStatusMapping,
        enum_value("legacy_scalar_presence_compatibility"),
    )?;
    insert(
        &mut builder,
        Key::FscV2StatusMapping,
        enum_value("decision_time_status_coverage_lane_health"),
    )?;

    insert(
        &mut builder,
        Key::ManipulationNumericPresenceVersion,
        enum_value("v2_field_presence"),
    )?;
    insert(
        &mut builder,
        Key::ManipulationBooleanPresenceVersion,
        enum_value("v2_field_presence"),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighFlagDerivationVersion,
        enum_value("policy_stage_v1"),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighSameMsThreshold,
        Value::Ratio(gatekeeper.hard_fail_same_ms_tx_ratio),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighBundleThreshold,
        Value::Ratio(gatekeeper.max_same_ms_tx_ratio),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighTop3Threshold,
        Value::Ratio(gatekeeper.hard_fail_top3_volume_pct),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighHhiThreshold,
        Value::Ratio(gatekeeper.hard_fail_hhi),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighSignerCountThreshold,
        wide(usize_to_u64(
            gatekeeper.max_tx_per_signer,
            "max_tx_per_signer",
        )?),
    )?;
    insert(
        &mut builder,
        Key::ManipulationHighDevConcentrationThreshold,
        Value::Ratio(gatekeeper.max_dev_volume_ratio),
    )?;
    insert(
        &mut builder,
        Key::ManipulationMissingRawBehavior,
        enum_value("unavailable_not_false"),
    )?;
    insert(
        &mut builder,
        Key::ManipulationMeasuredFieldsMaskVersion,
        enum_value("v1_u16"),
    )?;

    insert(
        &mut builder,
        Key::ReserveVelocitySourceClock,
        enum_value("receive_time"),
    )?;
    insert(
        &mut builder,
        Key::ReserveVelocityFirstUpdateBehavior,
        enum_value("typed_first_update"),
    )?;
    insert(
        &mut builder,
        Key::ReserveVelocityZeroDeltaTimeBehavior,
        enum_value("unavailable"),
    )?;
    insert(
        &mut builder,
        Key::ReserveVelocityFallbackBehavior,
        enum_value("typed_fallback_not_zero"),
    )?;
    insert(
        &mut builder,
        Key::ReserveVelocityUnit,
        enum_value("sol_per_second"),
    )?;

    insert(
        &mut builder,
        Key::RecentBuySellWindowMs,
        wide(RCE_RECENT_WINDOW_MS_V1),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellSuccessfulOnly,
        Value::Boolean(true),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellBoundaryPolicy,
        enum_value("inclusive_start_and_end"),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellSameMsNumeratorRule,
        enum_value("sum_timestamp_multiplicity_minus_one"),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellLegacyRatioRule,
        enum_value("sell_zero_returns_buy_count"),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellUnboundedRatioRule,
        enum_value("buy_count_over_sell_count_or_null"),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellBoundedShareRule,
        enum_value("buy_count_over_transaction_count"),
    )?;
    insert(
        &mut builder,
        Key::RecentBuySellZeroDenominatorBehavior,
        enum_value("null_unavailable"),
    )?;

    insert(
        &mut builder,
        Key::ComparatorNormalizationVersion,
        enum_value("v1"),
    )?;
    insert(
        &mut builder,
        Key::ComparatorFloatEquivalenceRule,
        enum_value("bitwise_for_equivalent_surfaces"),
    )?;
    insert(
        &mut builder,
        Key::ComparatorEquivalenceLaneVersion,
        enum_value("v1"),
    )?;
    insert(
        &mut builder,
        Key::ComparatorActionabilityMappingVersion,
        enum_value("profile_a_v1"),
    )?;
    insert(
        &mut builder,
        Key::ComparatorStatusMappingVersion,
        enum_value("canonical_status_v1"),
    )?;
    insert(
        &mut builder,
        Key::ComparatorLegacyMissingFieldBehavior,
        enum_value("not_recorded_legacy_schema"),
    )?;

    builder.build().map_err(Into::into)
}
