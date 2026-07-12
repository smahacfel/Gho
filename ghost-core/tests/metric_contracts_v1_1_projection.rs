use ghost_core::checkpoint::{EvidenceStatus, MaterializedEvidenceStatus};
use ghost_core::metric_contracts::*;
use ghost_core::tx_intelligence::types::FscEvidenceStatus;
use serde_json::{json, Value};

fn value_for_key(key: MetricEffectiveConfigKeyV1) -> MetricEffectiveConfigValueV1 {
    match key {
        MetricEffectiveConfigKeyV1::FtdiLegacyCleanMinBuyTransactions
        | MetricEffectiveConfigKeyV1::FtdiCandidateCleanMinUniqueBuyers => {
            return MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(3));
        }
        MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers => {
            return MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(2));
        }
        MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples => {
            return MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(2));
        }
        MetricEffectiveConfigKeyV1::FtdiPopulationSuccessfulBuy => {
            return MetricEffectiveConfigValueV1::Enum("successful_buy".to_string());
        }
        MetricEffectiveConfigKeyV1::FtdiMissingSignerBehavior => {
            return MetricEffectiveConfigValueV1::Enum("legacy_empty_signer_identity".to_string());
        }
        MetricEffectiveConfigKeyV1::FtdiMissingTopologyBehavior => {
            return MetricEffectiveConfigValueV1::Enum("unavailable_entire_metric".to_string());
        }
        MetricEffectiveConfigKeyV1::FtdiDenominatorRule => {
            return MetricEffectiveConfigValueV1::Enum(
                "unique_topologies_over_unique_first_buyer_samples".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::DevTxIntelSuccessEligibility => {
            return MetricEffectiveConfigValueV1::Enum("accepted_successful_or_failed".to_string());
        }
        MetricEffectiveConfigKeyV1::DevFirstObservedAnchorRule => {
            return MetricEffectiveConfigValueV1::Enum(
                "first_accepted_creator_buy_in_ingest_order".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::DevPrimaryAnchorRule => {
            return MetricEffectiveConfigValueV1::Enum(
                "create_signature_then_earliest_eligible_creator_buy".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::DevMissingCreatorBehavior => {
            return MetricEffectiveConfigValueV1::Enum("unavailable".to_string());
        }
        MetricEffectiveConfigKeyV1::SameMsLegacyPopulation => {
            return MetricEffectiveConfigValueV1::Enum(
                "accepted_non_dust_successful_or_failed".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::SameMsLegacyDenominatorRule => {
            return MetricEffectiveConfigValueV1::Enum(
                "adjacent_exact_collisions_over_transaction_count".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::SameMsRecentPopulation => {
            return MetricEffectiveConfigValueV1::Enum(
                "successful_accepted_recent_window".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::SameMsRecentDenominatorRule => {
            return MetricEffectiveConfigValueV1::Enum(
                "same_timestamp_extras_over_transaction_count".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::Top3PreferredField => {
            return MetricEffectiveConfigValueV1::Enum("top3_signer_volume_ratio".to_string());
        }
        MetricEffectiveConfigKeyV1::Top3FallbackAlias => {
            return MetricEffectiveConfigValueV1::Enum("top3_volume_pct".to_string());
        }
        MetricEffectiveConfigKeyV1::Top3Scale => {
            return MetricEffectiveConfigValueV1::Enum("ratio_0_1".to_string());
        }
        MetricEffectiveConfigKeyV1::Top3MismatchBehavior => {
            return MetricEffectiveConfigValueV1::Enum(
                "preferred_authoritative_emit_mismatch_telemetry".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::FscLegacyFormula => {
            return MetricEffectiveConfigValueV1::Enum(
                "one_minus_distinct_known_sources_over_known_source_samples".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::FscFundingStreamUnavailableBehavior => {
            return MetricEffectiveConfigValueV1::Enum(
                "legacy_null_and_v2_unavailable".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::FscLegacyStatusMapping => {
            return MetricEffectiveConfigValueV1::Enum(
                "legacy_scalar_presence_compatibility".to_string(),
            );
        }
        MetricEffectiveConfigKeyV1::FscV2StatusMapping => {
            return MetricEffectiveConfigValueV1::Enum(
                "decision_time_status_coverage_lane_health".to_string(),
            );
        }
        _ => {}
    }
    match key.value_kind() {
        MetricEffectiveConfigValueKindV1::Boolean => MetricEffectiveConfigValueV1::Boolean(true),
        MetricEffectiveConfigValueKindV1::Unsigned => MetricEffectiveConfigValueV1::Unsigned(7),
        MetricEffectiveConfigValueKindV1::FiniteNumber => {
            MetricEffectiveConfigValueV1::FiniteNumber(0.005)
        }
        MetricEffectiveConfigValueKindV1::Ratio => MetricEffectiveConfigValueV1::Ratio(0.5),
        MetricEffectiveConfigValueKindV1::WideUnsigned => {
            MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(10_000))
        }
        MetricEffectiveConfigValueKindV1::Text => {
            MetricEffectiveConfigValueV1::Text(format!("{key:?}"))
        }
        MetricEffectiveConfigValueKindV1::NullableText => {
            MetricEffectiveConfigValueV1::NullableText(CanonicalNullableV1::Value(format!(
                "{key:?}"
            )))
        }
        MetricEffectiveConfigValueKindV1::NullableHash => {
            MetricEffectiveConfigValueV1::NullableHash(CanonicalNullableV1::Value(
                CanonicalHashV1::parse("3".repeat(64)).unwrap(),
            ))
        }
        MetricEffectiveConfigValueKindV1::Enum => {
            MetricEffectiveConfigValueV1::Enum(format!("{key:?}"))
        }
    }
}

fn effective_config() -> ResolvedMetricContractEffectiveConfigV1 {
    let mut builder =
        MetricContractEffectiveConfigBuilderV1::new(MetricContractFoundationConfigV1::default())
            .unwrap();
    for key in METRIC_EFFECTIVE_CONFIG_KEYS_V1 {
        builder.insert(*key, value_for_key(*key)).unwrap();
    }
    builder.build().unwrap()
}

fn effective_config_with_value(
    key: MetricEffectiveConfigKeyV1,
    value: MetricEffectiveConfigValueV1,
) -> ResolvedMetricContractEffectiveConfigV1 {
    let mut payload = effective_config().payload;
    payload
        .entries
        .iter_mut()
        .find(|entry| entry.key == key)
        .unwrap()
        .value = value;
    ResolvedMetricContractEffectiveConfigV1::try_from_payload(payload).unwrap()
}

fn projection_context<'a>(
    profile: &'a MetricContractProfileV1,
    config: &'a ResolvedMetricContractEffectiveConfigV1,
) -> MetricDecisionProjectionBuildContextV1<'a> {
    MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile,
        effective_config: config,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    }
}

fn measured(surface: MetricSurfaceId) -> CanonicalMetricEnvelopeV1 {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let assignment = profile.entry_for(surface).unwrap();
    MetricEvidenceEnvelopeV1::try_new(
        assignment.contract_id,
        1,
        surface,
        assignment.authority_class,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::Measured,
        false,
        Vec::new(),
    )
    .unwrap()
}

fn dev(surface: MetricSurfaceId, mode: DevBuySelectionModeV1) -> DevBuyEvidenceV1 {
    DevBuyEvidenceV1 {
        envelope: measured(surface),
        amount_sol: CanonicalNullableV1::Value(1.25),
        creator_known: true,
        create_signature: CanonicalNullableV1::Value("create".to_string()),
        create_signature_matched: mode == DevBuySelectionModeV1::CreateSignatureMatch,
        selection_mode: mode,
        selected_signature: CanonicalNullableV1::Value("selected".to_string()),
        selected_slot: CanonicalNullableV1::Value(CanonicalU64StringV1::new(42)),
        selected_transaction_index: CanonicalNullableV1::Value(3),
        eligible_buy_count: 2,
    }
}

fn timing(
    surface: MetricSurfaceId,
    source: TxTimingSourceV1,
    population: TxTimingPopulationV1,
    window_ms: Option<u32>,
) -> TxTimingMeasurementEvidenceV1 {
    TxTimingMeasurementEvidenceV1 {
        envelope: measured(surface),
        source,
        population,
        canonical_dedupe_applied: true,
        dust_filter_sol: CanonicalNullableV1::Value(0.005),
        window_ms: window_ms.into(),
        numerator: 1,
        denominator: 4,
        ratio: CanonicalNullableV1::Value(0.25),
    }
}

fn pr2a_evidence() -> (
    FtdiEvidenceV1,
    DevBuyContractEvidenceV1,
    TxTimingEvidenceV1,
    Top3SignerVolumeEvidenceV1,
    FundingSourceContractEvidenceV1,
    FscStatusEvidenceV1,
) {
    let ftdi_measurement = |surface| FtdiValueMeasurementV1 {
        envelope: measured(surface),
        value: CanonicalNullableV1::Value(0.5),
        unique_topology_count: 1,
        unique_buyer_sample_count: 2,
        buy_transaction_sample_count: 3,
    };
    let mut legacy_actionability_envelope = measured(MetricSurfaceId::FtdiLegacyBuyTxActionability);
    legacy_actionability_envelope.policy_actionable = true;
    let ftdi = FtdiEvidenceV1 {
        legacy_value: ftdi_measurement(MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy),
        value_v1: ftdi_measurement(MetricSurfaceId::FtdiValueEvidenceV1),
        legacy_actionability_envelope,
        legacy_buy_tx_actionable: true,
        unique_buyer_actionability_v2_envelope: measured(
            MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
        ),
        unique_buyer_actionable_v2: false,
        coordination_hhi_export_envelope: measured(
            MetricSurfaceId::CoordinationFeeTopologyHhiExportV1,
        ),
        coordination_hhi: CanonicalNullableV1::Value(1.0),
    };
    let dev_buy = DevBuyContractEvidenceV1 {
        tx_intel_first_observed: dev(
            MetricSurfaceId::TxIntelDevFirstObservedBuySol,
            DevBuySelectionModeV1::LegacyFirstObserved,
        ),
        gatekeeper_buffer_primary: dev(
            MetricSurfaceId::GatekeeperBufferDevPrimaryBuySol,
            DevBuySelectionModeV1::CreateSignatureMatch,
        ),
        mfs_first_observed: dev(
            MetricSurfaceId::MfsDevFirstObservedBuySol,
            DevBuySelectionModeV1::LegacyFirstObserved,
        ),
        mfs_primary_v1: dev(
            MetricSurfaceId::MfsDevPrimaryBuySolV1,
            DevBuySelectionModeV1::CreateSignatureMatch,
        ),
        effective_policy: dev(
            MetricSurfaceId::EffectivePolicyDevBuySol,
            DevBuySelectionModeV1::LegacyFirstObserved,
        ),
    };
    let timing = TxTimingEvidenceV1 {
        legacy_exact: timing(
            MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
            TxTimingSourceV1::TxIntelFullObservationExactLegacy,
            TxTimingPopulationV1::AcceptedTransactions,
            None,
        ),
        exact_v1: timing(
            MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
            TxTimingSourceV1::TxTimingFullObservationExactV1,
            TxTimingPopulationV1::AcceptedTransactions,
            None,
        ),
        cluster_lt_50ms: timing(
            MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
            TxTimingSourceV1::PhaseDiversityClusterLt50Ms,
            TxTimingPopulationV1::AcceptedTransactions,
            None,
        ),
        recent_exact: timing(
            MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
            TxTimingSourceV1::RceRecentExact,
            TxTimingPopulationV1::SuccessfulTransactions,
            Some(10_000),
        ),
    };
    let top3 = Top3SignerVolumeEvidenceV1 {
        preferred_envelope: measured(MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred),
        preferred_ratio: CanonicalNullableV1::Value(0.6),
        compatibility_alias_envelope: measured(
            MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
        ),
        compatibility_alias_ratio: CanonicalNullableV1::Value(0.6),
        effective_selector_envelope: measured(MetricSurfaceId::TxIntelTop3EffectiveSelector),
        effective_ratio: CanonicalNullableV1::Value(0.6),
        preferred_alias_bitwise_equal: CanonicalNullableV1::Value(true),
        used_compatibility_fallback: false,
    };
    let legacy_fsc = |surface| FundingSourceLegacyMeasurementV1 {
        envelope: measured(surface),
        ratio: CanonicalNullableV1::Value(0.5),
        distinct_known_source_count: 1,
        known_source_sample_count: 2,
    };
    let funding = FundingSourceContractEvidenceV1 {
        legacy_source: legacy_fsc(MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy),
        legacy_v1: legacy_fsc(MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1),
        v2_envelope: measured(MetricSurfaceId::FundingSourceV2ReadinessEvidence),
        v2_status: FscEvidenceStatus::Clean,
        known_coverage: CanonicalNullableV1::Value(1.0),
        non_neutral_known_coverage: CanonicalNullableV1::Value(1.0),
        known_buyer_count: 2,
        total_buyer_count: 2,
        provider: CanonicalNullableV1::Value("provider".to_string()),
        config_hash: CanonicalNullableV1::Value(CanonicalHashV1::parse("4".repeat(64)).unwrap()),
        coordination_hhi_export_envelope: measured(
            MetricSurfaceId::CoordinationFundingSourceHhiExportV1,
        ),
        coordination_hhi: CanonicalNullableV1::Value(1.0),
    };
    let status = FscStatusEvidenceV1 {
        envelope: measured(MetricSurfaceId::MaterializedFscStatusCompatibility),
        legacy_scalar_present: true,
        legacy_feature_status: EvidenceStatus::Clean,
        fsc_v2_status: CanonicalNullableV1::Value(FscEvidenceStatus::Clean),
        fsc_v2_coverage: CanonicalNullableV1::Value(1.0),
    };
    (ftdi, dev_buy, timing, top3, funding, status)
}

fn compact_envelope(
    surface: MetricSurfaceId,
    profile: &MetricContractProfileV1,
) -> MetricDecisionEnvelopeV1 {
    let assignment = profile.entry_for(surface).unwrap();
    MetricDecisionEnvelopeV1 {
        contract_id: assignment.contract_id,
        contract_version: 1,
        surface_id: surface,
        authority_class: assignment.authority_class,
        rollout_role: assignment.role_for(MetricContractRolloutMode::Legacy),
        availability: MetricAvailabilityV1::Available,
        measurement_quality: MetricMeasurementQualityV1::Measured,
        policy_actionable: false,
        reasons: MetricDecisionReasonSummaryV1 {
            codes: Vec::new(),
            omitted_count: 0,
        },
    }
}

fn surface<T>(
    surface_id: MetricSurfaceId,
    value: T,
    producer_id: MetricContractProducerIdV1,
    profile: &MetricContractProfileV1,
) -> MetricDecisionSurfaceValueV1<T> {
    MetricDecisionSurfaceValueV1 {
        envelope: compact_envelope(surface_id, profile),
        value: CanonicalNullableV1::Value(value),
        producer_id,
        producer_schema_version: 1,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    }
}

fn field<T>(value: T) -> MetricDecisionFieldValueV1<T> {
    MetricDecisionFieldValueV1 {
        value: CanonicalNullableV1::Value(value),
        availability: MetricAvailabilityV1::Available,
        measurement_quality: MetricMeasurementQualityV1::Measured,
        reasons: MetricDecisionReasonSummaryV1 {
            codes: Vec::new(),
            omitted_count: 0,
        },
    }
}

fn complete_projection() -> MetricContractDecisionEvidenceProjectionV1 {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let cutoff = MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: cutoff,
    };
    let (ftdi, dev, timing, top3, funding, fsc_status) = pr2a_evidence();
    let legacy_manip = compact_envelope(
        MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
        &profile,
    );
    let v2_manip = compact_envelope(MetricSurfaceId::ManipulationNumericEvidenceV2, &profile);
    let recent_v1 = compact_envelope(MetricSurfaceId::RecentBuySellEvidenceV1, &profile);
    MetricContractDecisionEvidenceProjectionV1 {
        schema_version: METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1,
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile_id: profile.payload().profile_id,
        profile_hash: profile.canonical_hash().unwrap(),
        metric_contract_effective_config_hash: config.metric_contract_effective_config_hash.clone(),
        fee_topology_diversity_index: FtdiDecisionProjectionV1::try_from_evidence(&ftdi, &context)
            .unwrap(),
        dev_buy: DevBuyDecisionProjectionV1::try_from_evidence(&dev, &context).unwrap(),
        same_ms_tx_ratio: TxTimingDecisionProjectionV1::try_from_evidence(&timing, &context)
            .unwrap(),
        top3_signer_volume_ratio: Top3DecisionProjectionV1::try_from_evidence(&top3, &context)
            .unwrap(),
        flip_ratio: FlipDecisionProjectionV1 {
            legacy_slot_gap_ratio: surface(
                MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
                0.2,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
                &profile,
            ),
            hybrid_v2_ratio: surface(
                MetricSurfaceId::FlipRatioHybridEvidenceV2,
                0.2,
                MetricContractProducerIdV1::TxIntelligenceFingerprintAggregator,
                &profile,
            ),
            eligible_buyer_count: 5,
            flipper_count: 1,
            wall_clock_window_ms: 10_000,
            max_slot_gap: 20,
            dump_ratio: 0.5,
        },
        funding_source_concentration: FundingDecisionProjectionV1::try_from_evidence(
            &funding, &context,
        )
        .unwrap(),
        fsc_evidence_status: FscStatusDecisionProjectionV1::try_from_evidence(
            &fsc_status,
            &context,
        )
        .unwrap(),
        manipulation_contradiction: ManipulationDecisionProjectionV1 {
            legacy_numeric_envelope: legacy_manip,
            numeric_v2_envelope: v2_manip,
            measured_fields_mask: 0x7f,
            same_ms_tx_ratio: field(0.1),
            bundle_suspicion_ratio: field(0.2),
            top3_signer_volume_ratio: field(0.3),
            hhi: field(0.1),
            max_tx_per_signer: field(2.0),
            dev_volume_ratio: field(0.1),
            contradiction_score: field(0.0),
            legacy_high_recorded_mask: 0x3f,
            legacy_high_true_mask: 0,
            derived_high_evaluable_mask: 0x3f,
            derived_high_true_mask: 0,
        },
        reserve_velocity: ReserveVelocityDecisionProjectionV1 {
            legacy_velocity: surface(
                MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
                1.0,
                MetricContractProducerIdV1::AccountStateCore,
                &profile,
            ),
            velocity_v1: surface(
                MetricSurfaceId::ReserveVelocityEvidenceV1,
                1.0,
                MetricContractProducerIdV1::AccountStateCore,
                &profile,
            ),
            previous_real_sol_reserves_lamports: field(CanonicalU64StringV1::new(1)),
            current_real_sol_reserves_lamports: field(CanonicalU64StringV1::new(2)),
            interval_ms: field(1_000),
            accepted_update_count: 2,
            source_clock: ReserveVelocitySourceClockV1::ReceiveTime,
            status: ReserveVelocityStatusV1::Measured,
        },
        recent_buy_sell: RecentBuySellDecisionProjectionV1 {
            legacy_scalar: surface(
                MetricSurfaceId::RceBuySellRatioRecentLegacy,
                2.0,
                MetricContractProducerIdV1::RecentBuySellWindowProducer,
                &profile,
            ),
            v1_envelope: recent_v1,
            window_ms: 10_000,
            buy_count: 2,
            sell_count: 1,
            transaction_count: 3,
            buy_to_sell_ratio: field(2.0),
            buy_share: field(2.0 / 3.0),
        },
    }
}

fn assert_projection_rejected(projection: &MetricContractDecisionEvidenceProjectionV1) {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    };
    assert!(projection.validate_context(&context).is_err());
}

fn mutate_leaves(value: &Value, path: &mut Vec<String>, paths: &mut Vec<Vec<String>>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                mutate_leaves(child, path, paths);
                path.pop();
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                path.push(index.to_string());
                mutate_leaves(child, path, paths);
                path.pop();
            }
        }
        _ => paths.push(path.clone()),
    }
}

fn mutate_at(value: &mut Value, path: &[String]) {
    let mut cursor = value;
    for part in &path[..path.len() - 1] {
        cursor = if let Ok(index) = part.parse::<usize>() {
            &mut cursor.as_array_mut().unwrap()[index]
        } else {
            cursor.as_object_mut().unwrap().get_mut(part).unwrap()
        };
    }
    let leaf = path.last().unwrap();
    let target = if let Ok(index) = leaf.parse::<usize>() {
        &mut cursor.as_array_mut().unwrap()[index]
    } else {
        cursor.as_object_mut().unwrap().get_mut(leaf).unwrap()
    };
    *target = match target {
        Value::Null => Value::Bool(true),
        Value::Bool(value) => Value::Bool(!*value),
        Value::Number(value) => json!(value.as_f64().unwrap_or_default() + 1.0),
        Value::String(value) => Value::String(format!("{value}_changed")),
        Value::Array(_) | Value::Object(_) => unreachable!(),
    };
}

#[test]
fn projection_hash_is_deterministic_and_sensitive_to_every_semantic_leaf() {
    let projection = complete_projection();
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    };
    let first = projection.validated_canonical_hash(&context).unwrap();
    assert_eq!(
        first,
        projection.validated_canonical_hash(&context).unwrap()
    );
    let base = serde_json::to_value(&projection).unwrap();
    let mut paths = Vec::new();
    mutate_leaves(&base, &mut Vec::new(), &mut paths);
    assert!(paths.len() > 100);
    for path in paths {
        let mut changed = base.clone();
        mutate_at(&mut changed, &path);
        assert_ne!(
            CanonicalHashV1::digest(&base).unwrap(),
            CanonicalHashV1::digest(&changed).unwrap(),
            "leaf {path:?}"
        );
    }
}

#[test]
fn projection_schema_is_closed_and_partial_root_is_rejected() {
    let projection = complete_projection();
    let mut value = serde_json::to_value(&projection).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), json!(1));
    assert!(serde_json::from_value::<MetricContractDecisionEvidenceProjectionV1>(value).is_err());

    let mut partial = serde_json::to_value(&projection).unwrap();
    partial.as_object_mut().unwrap().remove("flip_ratio");
    assert!(serde_json::from_value::<MetricContractDecisionEvidenceProjectionV1>(partial).is_err());

    let mut family = serde_json::to_value(&projection.fee_topology_diversity_index).unwrap();
    family
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), json!(1));
    assert!(serde_json::from_value::<FtdiDecisionProjectionV1>(family).is_err());
}

#[test]
fn field_and_surface_value_status_incoherence_is_rejected() {
    let mut available_null = field(1.0);
    available_null.value = CanonicalNullableV1::Null;
    assert!(matches!(
        available_null.validate(),
        Err(MetricContractProjectionErrorV1::ValueStatusInvariant(_))
    ));

    for availability in [
        MetricAvailabilityV1::Unavailable,
        MetricAvailabilityV1::NotConfigured,
        MetricAvailabilityV1::NotRecordedLegacySchema,
    ] {
        let mut non_available_value = field(1.0);
        non_available_value.availability = availability;
        non_available_value.measurement_quality = MetricMeasurementQualityV1::NotApplicable;
        assert!(matches!(
            non_available_value.validate(),
            Err(MetricContractProjectionErrorV1::ValueStatusInvariant(_))
        ));
    }

    let mut available_not_applicable = field(1.0);
    available_not_applicable.measurement_quality = MetricMeasurementQualityV1::NotApplicable;
    assert!(matches!(
        available_not_applicable.validate(),
        Err(MetricContractProjectionErrorV1::ValueStatusInvariant(_))
    ));

    let mut projection = complete_projection();
    projection.top3_signer_volume_ratio.preferred.value = CanonicalNullableV1::Null;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection
        .top3_signer_volume_ratio
        .preferred
        .envelope
        .availability = MetricAvailabilityV1::Unavailable;
    projection
        .top3_signer_volume_ratio
        .preferred
        .envelope
        .measurement_quality = MetricMeasurementQualityV1::NotApplicable;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection
        .top3_signer_volume_ratio
        .preferred
        .envelope
        .measurement_quality = MetricMeasurementQualityV1::NotApplicable;
    assert_projection_rejected(&projection);
}

#[test]
fn timing_semantics_reject_count_ratio_population_and_window_drift() {
    let mut projection = complete_projection();
    projection.same_ms_tx_ratio.cluster_lt_50ms.numerator = 5;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.same_ms_tx_ratio.cluster_lt_50ms.surface.value = CanonicalNullableV1::Value(0.5);
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.same_ms_tx_ratio.cluster_lt_50ms.population =
        TxTimingPopulationV1::SuccessfulTransactions;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.same_ms_tx_ratio.recent_exact.window_ms = CanonicalNullableV1::Value(9_999);
    assert_projection_rejected(&projection);
}

#[test]
fn ftdi_top3_and_fsc_cross_field_drift_is_rejected() {
    let mut projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .unique_topology_count = 2;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.top3_signer_volume_ratio.effective.value = CanonicalNullableV1::Value(0.2);
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.dev_buy.primary_selection_mode = DevBuySelectionModeV1::NoEligibleBuy;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection
        .funding_source_concentration
        .known_source_sample_count = 1;
    projection
        .funding_source_concentration
        .distinct_known_source_count = 1;
    projection.funding_source_concentration.legacy_source.value = CanonicalNullableV1::Value(0.0);
    projection.funding_source_concentration.legacy_v1.value = CanonicalNullableV1::Value(0.0);
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.fsc_evidence_status.legacy_scalar_present = false;
    assert_projection_rejected(&projection);

    projection = complete_projection();
    projection.fsc_evidence_status.fsc_v2_status =
        CanonicalNullableV1::Value(FscEvidenceStatus::Unavailable);
    projection.fsc_evidence_status.fsc_v2_coverage = CanonicalNullableV1::Null;
    assert_projection_rejected(&projection);
}

#[test]
fn validated_hash_rejects_semantically_invalid_projection() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    };
    let mut projection = complete_projection();
    projection.same_ms_tx_ratio.recent_exact.numerator =
        projection.same_ms_tx_ratio.recent_exact.denominator + 1;
    assert!(projection.validated_canonical_hash(&context).is_err());
}

fn assert_config_parity_error(
    error: MetricContractProjectionErrorV1,
    expected_key: MetricEffectiveConfigKeyV1,
) {
    assert!(
        matches!(
            &error,
            MetricContractProjectionErrorV1::EffectiveConfigParity { key, .. }
                if *key == expected_key
        ),
        "expected effective-config parity error for {expected_key:?}, got {error:?}"
    );
}

#[test]
fn standard_projection_uses_current_window_and_fsc_minimum_and_hashes_validly() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    assert_eq!(
        config.value(MetricEffectiveConfigKeyV1::SameMsRecentWindowMs),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(10_000)
        ))
    );
    assert_eq!(
        config.value(MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(2)
        ))
    );
    let context = projection_context(&profile, &config);
    let projection = complete_projection();
    projection.validate_context(&context).unwrap();
    projection.validated_canonical_hash(&context).unwrap();
}

#[test]
fn timing_window_mismatch_is_rejected_after_effective_config_is_rehashed() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config_with_value(
        MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
        MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(9_999)),
    );
    let context = projection_context(&profile, &config);
    let mut projection = complete_projection();
    projection.metric_contract_effective_config_hash =
        config.metric_contract_effective_config_hash.clone();

    let error = projection.validate_context(&context).unwrap_err();
    assert_config_parity_error(error, MetricEffectiveConfigKeyV1::SameMsRecentWindowMs);
    let error = projection.validated_canonical_hash(&context).unwrap_err();
    assert_config_parity_error(error, MetricEffectiveConfigKeyV1::SameMsRecentWindowMs);
}

#[test]
fn fsc_minimum_mismatch_is_rejected_after_effective_config_is_rehashed() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config_with_value(
        MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
        MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(3)),
    );
    let context = projection_context(&profile, &config);
    let mut projection = complete_projection();
    projection.metric_contract_effective_config_hash =
        config.metric_contract_effective_config_hash.clone();

    let error = projection.validate_context(&context).unwrap_err();
    assert_config_parity_error(
        error,
        MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
    );
    let error = projection.validated_canonical_hash(&context).unwrap_err();
    assert_config_parity_error(
        error,
        MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples,
    );
}

#[test]
fn timing_window_validation_is_driven_by_effective_config_not_a_local_constant() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config_with_value(
        MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
        MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(9_999)),
    );
    let context = projection_context(&profile, &config);
    let mut projection = complete_projection();
    projection.metric_contract_effective_config_hash =
        config.metric_contract_effective_config_hash.clone();
    projection.same_ms_tx_ratio.recent_exact.window_ms = CanonicalNullableV1::Value(9_999);

    projection.validate_context(&context).unwrap();
    projection.validated_canonical_hash(&context).unwrap();
}

#[test]
fn projection_rejects_other_pr2a_semantic_config_contradictions_with_valid_hashes() {
    let replacements = [
        (
            MetricEffectiveConfigKeyV1::FtdiPopulationSuccessfulBuy,
            MetricEffectiveConfigValueV1::Enum("all_transactions".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiFirstSamplePerSigner,
            MetricEffectiveConfigValueV1::Boolean(false),
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiDiagnosticMinUniqueBuyers,
            MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(3)),
        ),
        (
            MetricEffectiveConfigKeyV1::FtdiDenominatorRule,
            MetricEffectiveConfigValueV1::Enum("transaction_count".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::DevFirstObservedAnchorRule,
            MetricEffectiveConfigValueV1::Enum("last_creator_buy".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::DevPrimarySuccessRequired,
            MetricEffectiveConfigValueV1::Boolean(false),
        ),
        (
            MetricEffectiveConfigKeyV1::DevPrimaryAnchorRule,
            MetricEffectiveConfigValueV1::Enum("earliest_only".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyPopulation,
            MetricEffectiveConfigValueV1::Enum("successful_only".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsLegacyDenominatorRule,
            MetricEffectiveConfigValueV1::Enum("pair_count".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsRecentPopulation,
            MetricEffectiveConfigValueV1::Enum("accepted_all".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::SameMsRecentDenominatorRule,
            MetricEffectiveConfigValueV1::Enum("adjacent_pairs".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::Top3PreferredField,
            MetricEffectiveConfigValueV1::Enum("top3_volume_pct".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::Top3FallbackAlias,
            MetricEffectiveConfigValueV1::Enum("top3_signer_volume_ratio".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::Top3Scale,
            MetricEffectiveConfigValueV1::Enum("percent_0_100".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::Top3MismatchBehavior,
            MetricEffectiveConfigValueV1::Enum("alias_authoritative".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::FscLegacyFormula,
            MetricEffectiveConfigValueV1::Enum("distinct_over_samples".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::FscFundingStreamUnavailableBehavior,
            MetricEffectiveConfigValueV1::Enum("legacy_zero".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::FscLegacyStatusMapping,
            MetricEffectiveConfigValueV1::Enum("status_only".to_string()),
        ),
        (
            MetricEffectiveConfigKeyV1::FscV2StatusMapping,
            MetricEffectiveConfigValueV1::Enum("coverage_only".to_string()),
        ),
    ];

    for (key, value) in replacements {
        let profile = MetricContractProfileV1::profile_a().unwrap();
        let config = effective_config_with_value(key, value);
        let context = projection_context(&profile, &config);
        let mut projection = complete_projection();
        projection.metric_contract_effective_config_hash =
            config.metric_contract_effective_config_hash.clone();
        let error = projection.validate_context(&context).unwrap_err();
        assert_config_parity_error(error, key);
    }
}

#[test]
fn timing_window_that_does_not_fit_compact_representation_is_rejected() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config_with_value(
        MetricEffectiveConfigKeyV1::SameMsRecentWindowMs,
        MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(
            u64::from(u32::MAX) + 1,
        )),
    );
    let context = projection_context(&profile, &config);
    let mut projection = complete_projection();
    projection.metric_contract_effective_config_hash =
        config.metric_contract_effective_config_hash.clone();

    let error = projection.validate_context(&context).unwrap_err();
    assert_config_parity_error(error, MetricEffectiveConfigKeyV1::SameMsRecentWindowMs);
}

#[test]
fn pr2a_family_projection_builders_reject_representation_drift() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap(),
    };
    let (mut ftdi, mut dev, mut timing, mut top3, mut funding, _) = pr2a_evidence();

    ftdi.value_v1.value = CanonicalNullableV1::Value(0.75);
    assert!(FtdiDecisionProjectionV1::try_from_evidence(&ftdi, &context).is_err());

    dev.effective_policy.amount_sol = CanonicalNullableV1::Value(2.0);
    assert!(DevBuyDecisionProjectionV1::try_from_evidence(&dev, &context).is_err());

    timing.exact_v1.numerator += 1;
    assert!(TxTimingDecisionProjectionV1::try_from_evidence(&timing, &context).is_err());

    top3.effective_ratio = CanonicalNullableV1::Value(0.1);
    assert!(Top3DecisionProjectionV1::try_from_evidence(&top3, &context).is_err());

    funding.legacy_v1.ratio = CanonicalNullableV1::Value(0.25);
    assert!(FundingDecisionProjectionV1::try_from_evidence(&funding, &context).is_err());
}

#[test]
fn reason_summary_is_bounded_counts_omissions_and_rejects_duplicates() {
    let reasons = vec![
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::InsufficientBuyTransactions),
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::InsufficientUniqueBuyers),
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::RawFeeTopologyUnavailable),
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::LegacyBuyTransactionActionabilityGate),
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::UniqueBuyerActionabilityCounterfactual),
        MetricEvidenceReasonV1::Ftdi(FtdiEvidenceReasonV1::CoordinationHhiExportOnly),
        MetricEvidenceReasonV1::LegacyStatus(LegacyStatusReasonV1::Degraded),
        MetricEvidenceReasonV1::LegacyStatus(LegacyStatusReasonV1::Unavailable),
        MetricEvidenceReasonV1::LegacyStatus(LegacyStatusReasonV1::Stale),
    ];
    let summary = MetricDecisionReasonSummaryV1::try_from_codes(&reasons).unwrap();
    assert_eq!(summary.codes.len(), 8);
    assert_eq!(summary.omitted_count, 1);
    assert!(MetricDecisionReasonSummaryV1::try_from_codes(&[
        reasons[0].clone(),
        reasons[0].clone(),
    ])
    .is_err());
    assert!(MetricDecisionReasonSummaryV1 {
        codes: vec![reasons[0].clone()],
        omitted_count: 1,
    }
    .validate()
    .is_err());
}

#[test]
fn effective_config_builder_rejects_missing_duplicate_wrong_kind_and_non_finite_inputs() {
    let foundation = MetricContractFoundationConfigV1::default();
    let missing = MetricContractEffectiveConfigBuilderV1::new(foundation).unwrap();
    assert!(matches!(
        missing.build(),
        Err(MetricContractEffectiveConfigErrorV1::MissingKey(_))
    ));

    let key = MetricEffectiveConfigKeyV1::FtdiFirstSamplePerSigner;
    let mut duplicate = MetricContractEffectiveConfigBuilderV1::new(foundation).unwrap();
    duplicate
        .insert(key, MetricEffectiveConfigValueV1::Boolean(true))
        .unwrap();
    assert!(matches!(
        duplicate.insert(key, MetricEffectiveConfigValueV1::Boolean(false)),
        Err(MetricContractEffectiveConfigErrorV1::DuplicateKey(actual)) if actual == key
    ));

    let mut wrong_kind = MetricContractEffectiveConfigBuilderV1::new(foundation).unwrap();
    wrong_kind
        .insert(key, MetricEffectiveConfigValueV1::Enum("true".to_string()))
        .unwrap();
    assert!(matches!(
        wrong_kind.build(),
        Err(MetricContractEffectiveConfigErrorV1::WrongValueKind { key: actual, .. })
            if actual == key
    ));

    let finite_key = MetricEffectiveConfigKeyV1::DevTxIntelDustThresholdSol;
    let mut non_finite = MetricContractEffectiveConfigBuilderV1::new(foundation).unwrap();
    non_finite
        .insert(
            finite_key,
            MetricEffectiveConfigValueV1::FiniteNumber(f64::NAN),
        )
        .unwrap();
    assert!(matches!(
        non_finite.build(),
        Err(MetricContractEffectiveConfigErrorV1::NonFiniteValue)
    ));
}

#[test]
fn historical_materialized_status_without_fsc_split_defaults_fail_closed() {
    let status: MaterializedEvidenceStatus = serde_json::from_value(json!({})).unwrap();
    assert_eq!(status.fsc_legacy.status, EvidenceStatus::Unavailable);
    assert_eq!(status.fsc_v2.status, EvidenceStatus::Unavailable);
}

#[test]
fn exact_surface_role_producer_config_and_cutoff_validation_fail_closed() {
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let config = effective_config();
    let cutoff = MetricContractDecisionSourceCutoffV1::try_new(1_000, Some(42)).unwrap();
    let context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
        source_cutoff: cutoff,
    };
    let mut projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .legacy_value
        .producer_schema_version = 0;
    assert!(matches!(
        projection.validate_context(&context),
        Err(MetricContractProjectionErrorV1::MissingProducerSchema)
    ));
    projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .legacy_value
        .producer_id = MetricContractProducerIdV1::FundingSourceIndex;
    assert!(matches!(
        projection.validate_context(&context),
        Err(MetricContractProjectionErrorV1::ProducerMismatch { .. })
    ));
    projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .legacy_value
        .source_cutoff = MetricContractDecisionSourceCutoffV1 {
        decision_timestamp_ms: CanonicalU64StringV1::new(0),
        decision_slot: CanonicalNullableV1::Null,
    };
    assert!(projection.validate_context(&context).is_err());
    projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .legacy_value
        .envelope
        .rollout_role = MetricRolloutRoleV1::PolicyComparator;
    assert!(projection.validate_context(&context).is_err());
    projection = complete_projection();
    projection
        .fee_topology_diversity_index
        .legacy_value
        .envelope
        .surface_id = MetricSurfaceId::FtdiValueEvidenceV1;
    assert!(projection.validate_context(&context).is_err());
    projection = complete_projection();
    projection.profile_hash = CanonicalHashV1::parse("0".repeat(64)).unwrap();
    assert!(projection.validate_context(&context).is_err());
    projection = complete_projection();
    projection.metric_contract_effective_config_hash =
        CanonicalHashV1::parse("0".repeat(64)).unwrap();
    assert!(projection.validate_context(&context).is_err());
    projection = complete_projection();
    projection.rollout_mode = MetricContractRolloutMode::DualCompute;
    assert!(projection.validate_context(&context).is_err());
}

#[test]
fn projection_contains_no_owner_or_event_heavy_audit_collections() {
    let json = serde_json::to_string(&complete_projection()).unwrap();
    for forbidden in [
        "\"owners\"",
        "anchor_event_identity",
        "qualifying_sell_event_identity",
        "cumulative_eligible_buy_tokens",
        "writer_timestamp_ms",
        "rotation_metadata",
    ] {
        assert!(
            !json.contains(forbidden),
            "forbidden projection detail: {forbidden}"
        );
    }
}
