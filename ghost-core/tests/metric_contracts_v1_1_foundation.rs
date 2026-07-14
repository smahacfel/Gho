use ghost_core::checkpoint::{
    EvidenceDegradedReason, EvidenceStatus, EvidenceUnavailableReason, FeatureEvidenceStatus,
    MetricEvidenceQuality,
};
use ghost_core::metric_contracts::*;
use ghost_core::tx_intelligence::types::{FscEvidenceStatus, FscExcludedReason};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashSet};

fn legacy_ftdi_context() -> LegacyEvidenceAdapterContextV1 {
    LegacyEvidenceAdapterContextV1 {
        contract_id: MetricContractId::FeeTopologyDiversityIndex,
        contract_version: 1,
        surface_id: MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
        authority_class: MetricAuthorityClass::Authoritative,
        clean_policy_actionable: true,
    }
}

fn value_for_key(key: MetricEffectiveConfigKeyV1) -> MetricEffectiveConfigValueV1 {
    match key.value_kind() {
        MetricEffectiveConfigValueKindV1::Boolean => MetricEffectiveConfigValueV1::Boolean(true),
        MetricEffectiveConfigValueKindV1::Unsigned => MetricEffectiveConfigValueV1::Unsigned(7),
        MetricEffectiveConfigValueKindV1::FiniteNumber => {
            MetricEffectiveConfigValueV1::FiniteNumber(0.005)
        }
        MetricEffectiveConfigValueKindV1::Ratio => MetricEffectiveConfigValueV1::Ratio(0.5),
        MetricEffectiveConfigValueKindV1::WideUnsigned => {
            MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(10_000_000))
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

fn complete_effective_config() -> ResolvedMetricContractEffectiveConfigV1 {
    let mut builder =
        MetricContractEffectiveConfigBuilderV1::new(MetricContractFoundationConfigV1::default())
            .expect("foundation profile");
    for key in METRIC_EFFECTIVE_CONFIG_KEYS_V1 {
        builder
            .insert(*key, value_for_key(*key))
            .expect("unique key");
    }
    builder.build().expect("complete resolved config")
}

fn measured_surface_envelope(surface: MetricSurfaceId) -> CanonicalMetricEnvelopeV1 {
    let profile = MetricContractProfileV1::profile_a().expect("Profile A");
    let entry = profile
        .entry_for(surface)
        .expect("registered profile surface");
    MetricEvidenceEnvelopeV1::try_new(
        entry.contract_id,
        1,
        surface,
        entry.authority_class,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::Measured,
        false,
        vec![],
    )
    .expect("valid measured envelope")
}

fn dev_surface(
    surface: MetricSurfaceId,
    selection_mode: DevBuySelectionModeV1,
) -> DevBuyEvidenceV1 {
    let selected_signature =
        if matches!(selection_mode, DevBuySelectionModeV1::CreateSignatureMatch) {
            "create-signature"
        } else {
            "buy-signature"
        };
    DevBuyEvidenceV1 {
        envelope: measured_surface_envelope(surface),
        amount_sol: CanonicalNullableV1::Value(1.25),
        creator_known: true,
        create_signature: CanonicalNullableV1::Value("create-signature".to_string()),
        create_signature_matched: matches!(
            selection_mode,
            DevBuySelectionModeV1::CreateSignatureMatch
        ),
        selection_mode,
        selected_signature: CanonicalNullableV1::Value(selected_signature.to_string()),
        selected_slot: CanonicalNullableV1::Value(CanonicalU64StringV1::new(42)),
        selected_transaction_index: CanonicalNullableV1::Value(3),
        eligible_buy_count: 1,
    }
}

fn timing_surface(
    surface: MetricSurfaceId,
    source: TxTimingSourceV1,
    population: TxTimingPopulationV1,
    window_ms: Option<u32>,
) -> TxTimingMeasurementEvidenceV1 {
    TxTimingMeasurementEvidenceV1 {
        envelope: measured_surface_envelope(surface),
        source,
        population,
        canonical_dedupe_applied: true,
        dust_filter_sol: CanonicalNullableV1::Null,
        window_ms: window_ms.into(),
        numerator: 1,
        denominator: 4,
        ratio: CanonicalNullableV1::Value(0.25),
    }
}

fn manipulation_field(
    field_id: ManipulationNumericFieldIdV2,
) -> ManipulationNumericFieldEvidenceV2 {
    ManipulationNumericFieldEvidenceV2 {
        field_id,
        value: CanonicalNullableV1::Value(0.0),
        availability: MetricAvailabilityV1::Available,
        measurement_quality: MetricMeasurementQualityV1::Measured,
        reason_codes: vec![],
    }
}

fn complete_contract_evidence() -> MetricContractsEvidenceSetV1 {
    let manipulation_fields = [
        ManipulationNumericFieldIdV2::SameMsTxRatio,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio,
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
        ManipulationNumericFieldIdV2::Hhi,
        ManipulationNumericFieldIdV2::MaxTxPerSigner,
        ManipulationNumericFieldIdV2::DevVolumeRatio,
        ManipulationNumericFieldIdV2::ContradictionScore,
    ]
    .into_iter()
    .map(manipulation_field)
    .collect::<Vec<_>>();
    let legacy_high_flags = [
        ManipulationNumericFieldIdV2::SameMsTxRatio,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio,
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
        ManipulationNumericFieldIdV2::Hhi,
        ManipulationNumericFieldIdV2::MaxTxPerSigner,
        ManipulationNumericFieldIdV2::DevVolumeRatio,
    ]
    .into_iter()
    .map(|field_id| ManipulationLegacyHighFlagEvidenceV1 {
        field_id,
        value: false,
        field_recorded: true,
    })
    .collect::<Vec<_>>();
    let derived_high_flags = legacy_high_flags
        .iter()
        .map(|legacy| ManipulationDerivedFlagEvidenceV2 {
            field_id: legacy.field_id,
            raw_value: CanonicalNullableV1::Value(0.0),
            raw_availability: MetricAvailabilityV1::Available,
            raw_measurement_quality: MetricMeasurementQualityV1::Measured,
            derived_value: CanonicalNullableV1::Value(false),
            comparator: ManipulationComparatorV1::GreaterThan,
            threshold: CanonicalNullableV1::Value(0.5),
            policy_stage: MANIPULATION_DERIVED_POLICY_STAGE_V1.to_string(),
            policy_version: MANIPULATION_DERIVED_POLICY_VERSION_V1.to_string(),
            config_hash: CanonicalHashV1::parse("2".repeat(64)).unwrap(),
        })
        .collect();

    MetricContractsEvidenceSetV1 {
        fee_topology_diversity_index: FtdiEvidenceV1 {
            legacy_value: FtdiValueMeasurementV1 {
                envelope: measured_surface_envelope(
                    MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy,
                ),
                value: CanonicalNullableV1::Value(0.5),
                unique_topology_count: 1,
                unique_buyer_sample_count: 2,
                buy_transaction_sample_count: 3,
            },
            value_v1: FtdiValueMeasurementV1 {
                envelope: measured_surface_envelope(MetricSurfaceId::FtdiValueEvidenceV1),
                value: CanonicalNullableV1::Value(0.5),
                unique_topology_count: 1,
                unique_buyer_sample_count: 2,
                buy_transaction_sample_count: 3,
            },
            legacy_actionability_envelope: measured_surface_envelope(
                MetricSurfaceId::FtdiLegacyBuyTxActionability,
            ),
            legacy_buy_tx_actionable: true,
            unique_buyer_actionability_v2_envelope: measured_surface_envelope(
                MetricSurfaceId::FtdiUniqueBuyerActionabilityV2,
            ),
            unique_buyer_actionable_v2: false,
            coordination_hhi_export_envelope: measured_surface_envelope(
                MetricSurfaceId::CoordinationFeeTopologyHhiExportV1,
            ),
            coordination_hhi: CanonicalNullableV1::Value(0.5),
        },
        dev_buy: DevBuyContractEvidenceV1 {
            tx_intel_first_observed: dev_surface(
                MetricSurfaceId::TxIntelDevFirstObservedBuySol,
                DevBuySelectionModeV1::LegacyFirstObserved,
            ),
            gatekeeper_buffer_primary: dev_surface(
                MetricSurfaceId::GatekeeperBufferDevPrimaryBuySol,
                DevBuySelectionModeV1::CreateSignatureMatch,
            ),
            mfs_first_observed: dev_surface(
                MetricSurfaceId::MfsDevFirstObservedBuySol,
                DevBuySelectionModeV1::LegacyFirstObserved,
            ),
            mfs_primary_v1: dev_surface(
                MetricSurfaceId::MfsDevPrimaryBuySolV1,
                DevBuySelectionModeV1::CreateSignatureMatch,
            ),
            effective_policy: dev_surface(
                MetricSurfaceId::EffectivePolicyDevBuySol,
                DevBuySelectionModeV1::LegacyFirstObserved,
            ),
        },
        same_ms_tx_ratio: TxTimingEvidenceV1 {
            legacy_exact: timing_surface(
                MetricSurfaceId::TxIntelSameMsCollisionRatioExact,
                TxTimingSourceV1::TxIntelFullObservationExactLegacy,
                TxTimingPopulationV1::AcceptedTransactions,
                None,
            ),
            exact_v1: timing_surface(
                MetricSurfaceId::TxTimingExactSameMsEvidenceV1,
                TxTimingSourceV1::TxTimingFullObservationExactV1,
                TxTimingPopulationV1::AcceptedTransactions,
                None,
            ),
            cluster_lt_50ms: timing_surface(
                MetricSurfaceId::TxIntelBundleClusterRatioLt50Ms,
                TxTimingSourceV1::PhaseDiversityClusterLt50Ms,
                TxTimingPopulationV1::AcceptedTransactions,
                None,
            ),
            recent_exact: timing_surface(
                MetricSurfaceId::RceSameMsCollisionRatioRecentExact,
                TxTimingSourceV1::RceRecentExact,
                TxTimingPopulationV1::SuccessfulTransactions,
                Some(10_000),
            ),
        },
        top3_signer_volume_ratio: Top3SignerVolumeEvidenceV1 {
            preferred_envelope: measured_surface_envelope(
                MetricSurfaceId::TxIntelTop3SignerVolumeRatioPreferred,
            ),
            preferred_ratio: CanonicalNullableV1::Value(0.4),
            compatibility_alias_envelope: measured_surface_envelope(
                MetricSurfaceId::TxIntelTop3VolumePctCompatibilityAlias,
            ),
            compatibility_alias_ratio: CanonicalNullableV1::Value(0.4),
            effective_selector_envelope: measured_surface_envelope(
                MetricSurfaceId::TxIntelTop3EffectiveSelector,
            ),
            effective_ratio: CanonicalNullableV1::Value(0.4),
            preferred_alias_bitwise_equal: CanonicalNullableV1::Value(true),
            used_compatibility_fallback: false,
        },
        flip_ratio: FlipRatioContractEvidenceV1 {
            legacy_envelope: measured_surface_envelope(
                MetricSurfaceId::EarlyFingerprintFlipRatioLegacySlotGap,
            ),
            legacy_slot_gap_ratio: CanonicalNullableV1::Value(0.0),
            hybrid_v2: FlipRatioEvidenceV2 {
                envelope: measured_surface_envelope(MetricSurfaceId::FlipRatioHybridEvidenceV2),
                ratio: CanonicalNullableV1::Value(0.0),
                eligible_buyer_count: 1,
                flipper_count: 0,
                wall_clock_window_ms: 10_000,
                max_slot_gap: 20,
                dump_ratio: 0.5,
                owners: vec![FlipOwnerEvidenceV2 {
                    owner_id: "owner-a".to_string(),
                    status: FlipOwnerStatusV2::Tracking,
                    anchor_event_identity: CanonicalNullableV1::Value(
                        StableEventIdentityV1::try_from_signature(
                            "yellowstone",
                            "anchor-signature",
                        )
                        .unwrap(),
                    ),
                    anchor_slot: CanonicalNullableV1::Value(CanonicalU64StringV1::new(40)),
                    anchor_timestamp_ms: CanonicalNullableV1::Value(CanonicalU64StringV1::new(
                        1_000,
                    )),
                    pre_anchor_sell_count: 0,
                    cumulative_eligible_buy_tokens: CanonicalU128StringV1::new(
                        u128::from(u64::MAX) + 1,
                    ),
                    cumulative_eligible_sell_tokens: CanonicalU128StringV1::new(0),
                    qualifying_sell_event_identity: CanonicalNullableV1::Null,
                    qualifying_sell_slot: CanonicalNullableV1::Null,
                    qualifying_sell_timestamp_ms: CanonicalNullableV1::Null,
                }],
            },
        },
        funding_source_concentration: FundingSourceContractEvidenceV1 {
            legacy_source: FundingSourceLegacyMeasurementV1 {
                envelope: measured_surface_envelope(
                    MetricSurfaceId::TxIntelFundingSourceConcentrationLegacy,
                ),
                ratio: CanonicalNullableV1::Value(0.5),
                distinct_known_source_count: 1,
                known_source_sample_count: 2,
            },
            legacy_v1: FundingSourceLegacyMeasurementV1 {
                envelope: measured_surface_envelope(
                    MetricSurfaceId::FundingSourceConcentrationLegacyEvidenceV1,
                ),
                ratio: CanonicalNullableV1::Value(0.5),
                distinct_known_source_count: 1,
                known_source_sample_count: 2,
            },
            v2_envelope: measured_surface_envelope(
                MetricSurfaceId::FundingSourceV2ReadinessEvidence,
            ),
            v2_status: FscEvidenceStatus::Clean,
            known_coverage: CanonicalNullableV1::Value(1.0),
            non_neutral_known_coverage: CanonicalNullableV1::Value(1.0),
            known_buyer_count: 2,
            known_non_neutral_buyer_count: 2,
            total_buyer_count: 2,
            provider: CanonicalNullableV1::Value("yellowstone".to_string()),
            config_hash: CanonicalNullableV1::Value(
                CanonicalHashV1::parse("1".repeat(64)).unwrap(),
            ),
            coordination_hhi_export_envelope: measured_surface_envelope(
                MetricSurfaceId::CoordinationFundingSourceHhiExportV1,
            ),
            coordination_hhi: CanonicalNullableV1::Value(0.5),
        },
        fsc_evidence_status: FscStatusEvidenceV1 {
            envelope: measured_surface_envelope(
                MetricSurfaceId::MaterializedFscStatusCompatibility,
            ),
            legacy_scalar_present: true,
            legacy_feature_status: EvidenceStatus::Clean,
            fsc_v2_status: CanonicalNullableV1::Value(FscEvidenceStatus::Clean),
            fsc_v2_coverage: CanonicalNullableV1::Value(1.0),
        },
        manipulation_contradiction: ManipulationNumericEvidenceV2 {
            legacy_numeric_envelope: measured_surface_envelope(
                MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
            ),
            numeric_v2_envelope: measured_surface_envelope(
                MetricSurfaceId::ManipulationNumericEvidenceV2,
            ),
            measured_fields_mask: 0b111_1111,
            legacy_fields: manipulation_fields.clone(),
            fields: manipulation_fields,
            legacy_high_flags_envelope: measured_surface_envelope(
                MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
            ),
            legacy_high_flags,
            derived_high_flags_envelope: measured_surface_envelope(
                MetricSurfaceId::PolicyDerivedManipulationHighFlagsV2,
            ),
            derived_high_flags,
        },
        reserve_velocity: ReserveVelocityEvidenceV1 {
            legacy_envelope: measured_surface_envelope(
                MetricSurfaceId::AccountStateReserveVelocityScalarLegacy,
            ),
            legacy_velocity_sol_per_sec: 0.0,
            v1_envelope: measured_surface_envelope(MetricSurfaceId::ReserveVelocityEvidenceV1),
            velocity_sol_per_sec: CanonicalNullableV1::Value(0.0),
            previous_real_sol_reserves_lamports: CanonicalNullableV1::Value(
                CanonicalU64StringV1::new(1_000_000_000),
            ),
            current_real_sol_reserves_lamports: CanonicalNullableV1::Value(
                CanonicalU64StringV1::new(1_000_000_000),
            ),
            interval_ms: CanonicalNullableV1::Value(100),
            accepted_update_count: 2,
            source_clock: ReserveVelocitySourceClockV1::ReceiveTime,
            status: ReserveVelocityStatusV1::Measured,
        },
        recent_buy_sell: RecentBuySellEvidenceV1 {
            legacy_envelope: measured_surface_envelope(
                MetricSurfaceId::RceBuySellRatioRecentLegacy,
            ),
            v1_envelope: measured_surface_envelope(MetricSurfaceId::RecentBuySellEvidenceV1),
            window_ms: 10_000,
            buy_count: 2,
            sell_count: 1,
            transaction_count: 3,
            legacy_buy_sell_scalar: CanonicalNullableV1::Value(2.0),
            buy_to_sell_ratio: CanonicalNullableV1::Value(2.0),
            buy_share: CanonicalNullableV1::Value(2.0 / 3.0),
        },
    }
}

#[test]
fn registry_contains_exactly_ten_unique_contracts_and_all_profile_surfaces() {
    assert_eq!(METRIC_CONTRACTS_V1_1.len(), 10);
    let ids = METRIC_CONTRACTS_V1_1
        .iter()
        .map(|contract| contract.id)
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 10);

    let registry_surfaces = METRIC_CONTRACTS_V1_1
        .iter()
        .flat_map(|contract| contract.surfaces.iter().copied())
        .collect::<BTreeSet<_>>();
    let profile = MetricContractProfileV1::profile_a().expect("valid Profile A");
    let profile_surfaces = profile
        .payload()
        .entries
        .iter()
        .map(|entry| entry.surface_id)
        .collect::<BTreeSet<_>>();
    assert_eq!(profile_surfaces, registry_surfaces);
    assert_eq!(profile_surfaces.len(), 32);
}

#[test]
fn top3_effective_selector_prefers_ratio_and_falls_back_bit_for_bit() {
    let mut features = ghost_core::tx_intelligence::types::TxIntelFeatures {
        top3_volume_pct: 0.25,
        ..ghost_core::tx_intelligence::types::TxIntelFeatures::default()
    };
    assert_eq!(
        features.effective_top3_signer_volume_ratio().to_bits(),
        0.25_f64.to_bits()
    );

    features.top3_signer_volume_ratio = Some(0.5);
    assert_eq!(
        features.effective_top3_signer_volume_ratio().to_bits(),
        0.5_f64.to_bits()
    );
}

#[test]
fn profile_a_keeps_counterfactual_and_evidence_surfaces_non_authoritative() {
    let profile = MetricContractProfileV1::profile_a().expect("valid Profile A");
    for entry in &profile.payload().entries {
        if matches!(
            entry.authority_class,
            MetricAuthorityClass::Compatibility
                | MetricAuthorityClass::Counterfactual
                | MetricAuthorityClass::EvidenceOnly
                | MetricAuthorityClass::LoggingOnly
                | MetricAuthorityClass::ExportOnly
        ) {
            for mode in [
                MetricContractRolloutMode::Legacy,
                MetricContractRolloutMode::DualCompute,
                MetricContractRolloutMode::V2,
            ] {
                assert_ne!(
                    entry.role_for(mode),
                    MetricRolloutRoleV1::PolicyAuthoritative,
                    "surface={:?} mode={mode:?}",
                    entry.surface_id
                );
            }
        }
    }

    let dev_primary = profile
        .entry_for(MetricSurfaceId::MfsDevPrimaryBuySolV1)
        .expect("dev primary assignment");
    assert_eq!(
        dev_primary.role_for(MetricContractRolloutMode::V2),
        MetricRolloutRoleV1::PolicyComparator
    );
    let ftdi_actionability = profile
        .entry_for(MetricSurfaceId::FtdiUniqueBuyerActionabilityV2)
        .expect("FTDI counterfactual assignment");
    assert_eq!(
        ftdi_actionability.role_for(MetricContractRolloutMode::V2),
        MetricRolloutRoleV1::PolicyComparator
    );

    let exact_same_ms_candidate = profile
        .entry_for(MetricSurfaceId::TxTimingExactSameMsEvidenceV1)
        .expect("typed exact same-ms assignment");
    assert_eq!(
        exact_same_ms_candidate.authority_class,
        MetricAuthorityClass::EquivalentCutover
    );
    assert_eq!(
        exact_same_ms_candidate.role_for(MetricContractRolloutMode::DualCompute),
        MetricRolloutRoleV1::PolicyComparator
    );
    assert_eq!(
        exact_same_ms_candidate.role_for(MetricContractRolloutMode::V2),
        MetricRolloutRoleV1::PolicyAuthoritative
    );
}

#[test]
fn profile_hash_is_deterministic_and_sensitive_to_every_authority_entry() {
    let profile = MetricContractProfileV1::profile_a().expect("valid Profile A");
    let baseline = profile.canonical_hash().expect("profile hash");
    assert_eq!(baseline, profile.canonical_hash().expect("repeat hash"));

    for index in 0..profile.payload().entries.len() {
        let mut changed = profile.payload().clone();
        changed.entries[index].authority_class =
            if changed.entries[index].authority_class == MetricAuthorityClass::ExportOnly {
                MetricAuthorityClass::LoggingOnly
            } else {
                MetricAuthorityClass::ExportOnly
            };
        assert_ne!(
            baseline,
            changed.canonical_hash().expect("changed profile hash"),
            "authority entry {index} must participate in profile hash"
        );
    }

    for index in 0..profile.payload().registry_contracts.len() {
        let mut changed = profile.payload().clone();
        changed.registry_contracts[index]
            .population_and_denominator
            .push_str("_changed");
        assert_ne!(
            baseline,
            changed.canonical_hash().expect("changed registry hash"),
            "registry semantic definition {index} must participate in profile hash"
        );
        assert!(matches!(
            MetricContractProfileV1::try_from_payload(changed),
            Err(MetricContractProfileErrorV1::RegistrySemanticMismatch)
        ));
    }
}

#[test]
fn profile_validation_rejects_schema_and_dual_compute_authority_drift() {
    let profile = MetricContractProfileV1::profile_a().expect("valid Profile A");

    let mut wrong_schema = profile.payload().clone();
    wrong_schema.schema_version += 1;
    assert!(matches!(
        MetricContractProfileV1::try_from_payload(wrong_schema),
        Err(MetricContractProfileErrorV1::UnsupportedSchema(_))
    ));

    let mut authority_drift = profile.payload().clone();
    let legacy_authority = authority_drift
        .entries
        .iter_mut()
        .find(|entry| entry.surface_id == MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy)
        .expect("legacy FTDI authority");
    legacy_authority.dual_compute_role = MetricRolloutRoleV1::NonPolicy;
    assert!(matches!(
        MetricContractProfileV1::try_from_payload(authority_drift),
        Err(MetricContractProfileErrorV1::InvalidDualComputeAuthority(
            MetricSurfaceId::TxIntelFeeTopologyDiversityLegacy
        ))
    ));

    let mut reordered = profile.payload().clone();
    reordered.entries.swap(0, 1);
    assert!(matches!(
        MetricContractProfileV1::try_from_payload(reordered),
        Err(MetricContractProfileErrorV1::NonCanonicalEntryOrder)
    ));
}

#[test]
fn canonical_hash_uses_rfc8785_key_unicode_and_number_rules() {
    #[derive(Serialize)]
    struct ReverseKeys {
        z: u32,
        a: u32,
    }
    let bytes = canonical_jcs_bytes_v1(&ReverseKeys { z: 2, a: 1 }).expect("JCS");
    assert_eq!(bytes, br#"{"a":1,"z":2}"#);

    let unicode = json!({
        "\u{fb33}": "Hebrew",
        "\r": "CR",
        "\u{1f600}": "Emoji",
        "1": "One",
        "\u{20ac}": "Euro",
        "\u{0080}": "Control",
        "\u{00f6}": "Latin"
    });
    let unicode_jcs =
        String::from_utf8(canonical_jcs_bytes_v1(&unicode).expect("Unicode JCS")).unwrap();
    assert_eq!(
        unicode_jcs,
        "{\"\\r\":\"CR\",\"1\":\"One\",\"\":\"Control\",\"ö\":\"Latin\",\"€\":\"Euro\",\"😀\":\"Emoji\",\"דּ\":\"Hebrew\"}"
    );

    assert_eq!(canonical_jcs_bytes_v1(&-0.0f64).unwrap(), b"0");
    assert_eq!(canonical_jcs_bytes_v1(&1e30f64).unwrap(), b"1e+30");
    assert_eq!(canonical_jcs_bytes_v1(&0.000_001f64).unwrap(), b"0.000001");
    assert_eq!(canonical_jcs_bytes_v1(&0.000_000_1f64).unwrap(), b"1e-7");
    for non_finite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(canonical_jcs_bytes_v1(&non_finite).is_err());
    }

    // The optimized production serializer must remain byte-for-byte equal to
    // the established RFC 8785 reference implementation. This fixture covers
    // nested objects, UTF-16 key ordering, escaping, arrays and every numeric
    // boundary used by metric-contract semantic payloads.
    let differential_fixture = json!({
        "z": [null, true, false, -0.0, 1e30, 0.000_001, 0.000_000_1],
        "a": {"escaped": "line\nquote\"slash/", "wide": u64::MAX.to_string()},
        "\u{fb33}": "Hebrew",
        "\u{1f600}": "Emoji",
        "\u{20ac}": "Euro"
    });
    assert_eq!(
        canonical_jcs_bytes_v1(&differential_fixture).unwrap(),
        serde_json_canonicalizer::to_vec(&differential_fixture).unwrap()
    );

    #[derive(Serialize)]
    struct RawIntegerBoundaries {
        max_u64: u64,
        min_i64: i64,
    }
    let integer_boundaries = RawIntegerBoundaries {
        max_u64: u64::MAX,
        min_i64: i64::MIN,
    };
    assert_eq!(
        canonical_jcs_bytes_v1(&integer_boundaries).unwrap(),
        serde_json_canonicalizer::to_vec(&integer_boundaries).unwrap()
    );
    assert_eq!(
        canonical_jcs_bytes_v1(&u128::MAX).unwrap(),
        serde_json_canonicalizer::to_vec(&u128::MAX).unwrap()
    );
}

#[test]
fn required_null_is_distinct_from_omission_and_wide_integer_is_canonical_string() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct RequiredNullablePayload {
        value: CanonicalNullableV1<u32>,
        wide: CanonicalU64StringV1,
        wider: CanonicalU128StringV1,
        signed: CanonicalI64StringV1,
    }

    let payload = RequiredNullablePayload {
        value: CanonicalNullableV1::Null,
        wide: CanonicalU64StringV1::new(u64::MAX),
        wider: CanonicalU128StringV1::new(u128::MAX),
        signed: CanonicalI64StringV1::new(i64::MIN),
    };
    assert_eq!(
        String::from_utf8(canonical_jcs_bytes_v1(&payload).unwrap()).unwrap(),
        format!(
            "{{\"signed\":\"{}\",\"value\":null,\"wide\":\"{}\",\"wider\":\"{}\"}}",
            i64::MIN,
            u64::MAX,
            u128::MAX
        )
    );
    assert!(serde_json::from_str::<RequiredNullablePayload>(&format!(
        "{{\"signed\":\"{}\",\"wide\":\"{}\",\"wider\":\"{}\"}}",
        i64::MIN,
        u64::MAX,
        u128::MAX
    ))
    .is_err());
    assert!(serde_json::from_str::<RequiredNullablePayload>(&format!(
        "{{\"signed\":\"{}\",\"value\":null,\"wide\":\"01\",\"wider\":\"{}\"}}",
        i64::MIN,
        u128::MAX
    ))
    .is_err());
    assert!(serde_json::from_str::<RequiredNullablePayload>(&format!(
        "{{\"signed\":\"{}\",\"value\":null,\"wide\":1,\"wider\":\"{}\"}}",
        i64::MIN,
        u128::MAX
    ))
    .is_err());
    assert!(serde_json::from_str::<RequiredNullablePayload>(&format!(
        "{{\"signed\":\"{}\",\"value\":null,\"wide\":\"{}\",\"wider\":\"01\"}}",
        i64::MIN,
        u64::MAX
    ))
    .is_err());
    assert!(serde_json::from_str::<RequiredNullablePayload>(&format!(
        "{{\"signed\":\"-0\",\"value\":null,\"wide\":\"{}\",\"wider\":\"{}\"}}",
        u64::MAX,
        u128::MAX
    ))
    .is_err());
}

#[test]
fn semantic_profile_payload_excludes_its_own_hash() {
    let payload = MetricContractProfileV1::profile_a()
        .expect("profile")
        .payload()
        .clone();
    let value = serde_json::to_value(payload).expect("profile JSON");
    assert!(value.get("profile_hash").is_none());
}

#[test]
fn effective_config_is_closed_complete_sorted_and_hash_validated() {
    assert_eq!(
        METRIC_EFFECTIVE_CONFIG_KEYS_V1
            .iter()
            .collect::<HashSet<_>>()
            .len(),
        METRIC_EFFECTIVE_CONFIG_KEYS_V1.len(),
        "required key registry itself must not contain duplicates"
    );
    let resolved = complete_effective_config();
    resolved.validate_hash().expect("hash parity");
    assert_eq!(
        resolved.payload.entries.len(),
        METRIC_EFFECTIVE_CONFIG_KEYS_V1.len()
    );
    assert!(resolved
        .payload
        .entries
        .windows(2)
        .all(|window| window[0].key < window[1].key));

    let mut builder =
        MetricContractEffectiveConfigBuilderV1::new(MetricContractFoundationConfigV1::default())
            .unwrap();
    for key in &METRIC_EFFECTIVE_CONFIG_KEYS_V1[..METRIC_EFFECTIVE_CONFIG_KEYS_V1.len() - 1] {
        builder.insert(*key, value_for_key(*key)).unwrap();
    }
    assert!(matches!(
        builder.build(),
        Err(MetricContractEffectiveConfigErrorV1::MissingKey(_))
    ));

    let encoded = serde_json::to_string(&resolved).expect("resolved config JSON");
    let decoded: ResolvedMetricContractEffectiveConfigV1 =
        serde_json::from_str(&encoded).expect("validated round-trip");
    assert_eq!(decoded, resolved);

    let mut wrong_profile = resolved.payload.clone();
    wrong_profile.profile_hash = CanonicalHashV1::parse("0".repeat(64)).unwrap();
    assert!(matches!(
        ResolvedMetricContractEffectiveConfigV1::try_from_payload(wrong_profile),
        Err(MetricContractEffectiveConfigErrorV1::ProfileHashMismatch)
    ));

    let mut tampered_transport = serde_json::to_value(&resolved).unwrap();
    tampered_transport["metric_contract_effective_config_hash"] = json!("0".repeat(64));
    assert!(
        serde_json::from_value::<ResolvedMetricContractEffectiveConfigV1>(tampered_transport)
            .is_err()
    );
}

fn collect_leaf_paths(
    value: &serde_json::Value,
    path: &mut Vec<String>,
    out: &mut Vec<Vec<String>>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                collect_leaf_paths(child, path, out);
                path.pop();
            }
        }
        serde_json::Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                path.push(index.to_string());
                collect_leaf_paths(child, path, out);
                path.pop();
            }
        }
        _ => out.push(path.clone()),
    }
}

fn mutate_leaf(value: &mut serde_json::Value, path: &[String]) {
    let mut current = value;
    for segment in &path[..path.len() - 1] {
        current = match current {
            serde_json::Value::Object(map) => map.get_mut(segment).unwrap(),
            serde_json::Value::Array(values) => &mut values[segment.parse::<usize>().unwrap()],
            _ => unreachable!("leaf path traverses container only"),
        };
    }
    let last = path.last().unwrap();
    let leaf = match current {
        serde_json::Value::Object(map) => map.get_mut(last).unwrap(),
        serde_json::Value::Array(values) => &mut values[last.parse::<usize>().unwrap()],
        _ => unreachable!("leaf parent is container"),
    };
    *leaf = match leaf {
        serde_json::Value::Null => json!("was_null"),
        serde_json::Value::Bool(value) => json!(!*value),
        serde_json::Value::Number(value) => {
            json!(value.as_f64().expect("JSON number") + 1.0)
        }
        serde_json::Value::String(value) => json!(format!("{value}_changed")),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => unreachable!(),
    };
}

#[test]
fn every_effective_config_semantic_leaf_changes_the_hash() {
    let resolved = complete_effective_config();
    let baseline = resolved.metric_contract_effective_config_hash.clone();
    let value = serde_json::to_value(&resolved.payload).expect("payload JSON");
    let mut paths = Vec::new();
    collect_leaf_paths(&value, &mut Vec::new(), &mut paths);
    assert!(paths.len() > METRIC_EFFECTIVE_CONFIG_KEYS_V1.len() * 2);

    for path in paths {
        let mut changed = value.clone();
        mutate_leaf(&mut changed, &path);
        let changed_hash = CanonicalHashV1::digest(&changed).expect("mutated JCS hash");
        assert_ne!(baseline, changed_hash, "semantic leaf path={path:?}");
    }
}

#[test]
fn record_identity_and_underlying_event_identity_are_separate_contracts() {
    let first = MetricEvidenceRecordIdentityV1::try_new("run-a", "join-1", "legacy_live")
        .expect("record identity");
    let second = MetricEvidenceRecordIdentityV1::try_new("run-b", "join-1", "legacy_live")
        .expect("record identity");
    assert_ne!(
        first, second,
        "same join key across runs is not a duplicate record"
    );

    let signature_event = StableEventIdentityV1::try_from_signature("yellowstone", "signature")
        .expect("signature identity");
    assert!(matches!(
        signature_event.key,
        StableEventKeyV1::Signature { .. }
    ));
    let ordered_event =
        StableEventIdentityV1::try_from_transaction_index("yellowstone", u64::MAX, 7)
            .expect("order-key fallback identity");
    assert!(matches!(
        ordered_event.key,
        StableEventKeyV1::SlotTransactionIndex { slot, transaction_index: 7 }
            if slot.get() == u64::MAX
    ));
    assert!(MetricEvidenceRecordIdentityV1::try_new("", "join", "plane").is_err());
    assert!(StableEventIdentityV1::try_from_signature("source", "").is_err());
}

fn complete_transport_payload() -> MetricContractEvidenceHashPayloadV1 {
    let foundation = MetricContractFoundationConfigV1::default();
    let profile = foundation.resolve_profile().expect("Profile A");
    let resolved_config = complete_effective_config();
    MetricContractEvidenceHashPayloadV1 {
        evidence_schema_version: METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
        record_identity: MetricEvidenceRecordIdentityV1::try_new("run-a", "join-a", "legacy_live")
            .unwrap(),
        stable_event_identity: CanonicalNullableV1::Value(
            StableEventIdentityV1::try_from_signature("yellowstone", "signature-a").unwrap(),
        ),
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(10_000, Some(100)).unwrap(),
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile_id: foundation.metric_contract_profile,
        profile_hash: profile.canonical_hash().unwrap(),
        metric_contract_effective_config_hash: resolved_config
            .metric_contract_effective_config_hash
            .clone(),
        policy_equivalence: MetricContractPolicyEquivalenceEvidenceV1 {
            policy_version: "v2.5".to_string(),
            gatekeeper_config_hash: CanonicalHashV1::parse(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap(),
            comparator_evaluable: true,
            authoritative: MetricContractPolicyEquivalenceSnapshotV1 {
                verdict: "Reject".to_string(),
                primary_reason_code: "TEST_REASON".to_string(),
                ordered_reason_chain: vec!["TEST_REASON".to_string()],
                phase_pass_vector: vec![false; 6],
                soft_points: 0,
                selector_soft_score_bits: 0,
                hard_fail_classification: "none".to_string(),
            },
            comparator: MetricContractPolicyEquivalenceSnapshotV1 {
                verdict: "Reject".to_string(),
                primary_reason_code: "TEST_REASON".to_string(),
                ordered_reason_chain: vec!["TEST_REASON".to_string()],
                phase_pass_vector: vec![false; 6],
                soft_points: 0,
                selector_soft_score_bits: 0,
                hard_fail_classification: "none".to_string(),
            },
        },
        contracts: complete_contract_evidence(),
    }
}

#[test]
fn full_evidence_transport_validates_every_surface_profile_schema_and_hash() {
    let foundation = MetricContractFoundationConfigV1::default();
    let profile = foundation.resolve_profile().expect("Profile A");
    let payload = complete_transport_payload();
    payload
        .contracts
        .validate_for_profile(&profile, MetricContractRolloutMode::Legacy)
        .expect("all 32 evidence surface slots match Profile A");
    let transport =
        MetricContractEvidenceTransportV1::try_new(payload, 123, 4).expect("valid transport");
    transport.validate_hash().expect("hash validation");

    let encoded = serde_json::to_value(&transport).unwrap();
    let decoded: MetricContractEvidenceTransportV1 =
        serde_json::from_value(encoded.clone()).expect("validated transport round-trip");
    assert_eq!(decoded, transport);

    let mut transport_only_change = encoded.clone();
    transport_only_change["writer_timestamp_ms"] = json!(999);
    let changed_transport: MetricContractEvidenceTransportV1 =
        serde_json::from_value(transport_only_change).expect("transport metadata is unhashed");
    assert_eq!(changed_transport.evidence_sha256, transport.evidence_sha256);

    let mut bad_hash = encoded;
    bad_hash["evidence_sha256"] = json!("0".repeat(64));
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(bad_hash).is_err());

    let mut wrong_surface_payload = transport.payload.clone();
    wrong_surface_payload
        .contracts
        .same_ms_tx_ratio
        .exact_v1
        .envelope = measured_surface_envelope(MetricSurfaceId::TxIntelSameMsCollisionRatioExact);
    assert!(matches!(
        MetricContractEvidenceTransportV1::try_new(wrong_surface_payload, 123, 4),
        Err(MetricContractEvidenceTransportErrorV1::Envelope(
            MetricEvidenceEnvelopeErrorV1::UnexpectedSurfaceForEvidenceField { .. }
        ))
    ));

    let mut wrong_schema_payload = transport.payload;
    wrong_schema_payload.evidence_schema_version += 1;
    assert!(matches!(
        MetricContractEvidenceTransportV1::try_new(wrong_schema_payload, 123, 4),
        Err(MetricContractEvidenceTransportErrorV1::UnsupportedEvidenceSchema(_))
    ));

    let mut invalid_recent = complete_contract_evidence();
    invalid_recent.recent_buy_sell.buy_count = 2;
    invalid_recent.recent_buy_sell.sell_count = 0;
    invalid_recent.recent_buy_sell.transaction_count = 2;
    invalid_recent.recent_buy_sell.buy_to_sell_ratio = CanonicalNullableV1::Value(2.0);
    invalid_recent.recent_buy_sell.buy_share = CanonicalNullableV1::Value(1.0);
    assert!(matches!(
        invalid_recent.validate_semantics(),
        Err(MetricContractEvidenceSemanticErrorV1::RecentBuySellInvariant)
    ));

    let mut invalid_reserve = complete_contract_evidence();
    invalid_reserve.reserve_velocity.status = ReserveVelocityStatusV1::FirstUpdate;
    invalid_reserve.reserve_velocity.accepted_update_count = 1;
    assert!(matches!(
        invalid_reserve.validate_semantics(),
        Err(MetricContractEvidenceSemanticErrorV1::ReserveVelocityInvariant)
    ));

    let mut invalid_presence = complete_contract_evidence();
    invalid_presence
        .manipulation_contradiction
        .measured_fields_mask = 0;
    assert!(matches!(
        invalid_presence.validate_semantics(),
        Err(MetricContractEvidenceSemanticErrorV1::ManipulationMeasuredMaskMismatch)
    ));

    let mut one_known_fsc = complete_contract_evidence();
    for legacy in [
        &mut one_known_fsc.funding_source_concentration.legacy_source,
        &mut one_known_fsc.funding_source_concentration.legacy_v1,
    ] {
        legacy.distinct_known_source_count = 1;
        legacy.known_source_sample_count = 1;
        legacy.ratio = CanonicalNullableV1::Null;
    }
    one_known_fsc.fsc_evidence_status.legacy_scalar_present = false;
    one_known_fsc
        .validate_semantics()
        .expect("one known FSC source is insufficient and must not become measured zero");

    one_known_fsc
        .funding_source_concentration
        .legacy_source
        .ratio = CanonicalNullableV1::Value(0.0);
    assert!(matches!(
        one_known_fsc.validate_semantics(),
        Err(MetricContractEvidenceSemanticErrorV1::DerivedRatioMismatch(
            "fsc_legacy_source"
        ))
    ));

    let mut incomplete_flip = complete_contract_evidence();
    incomplete_flip.flip_ratio.hybrid_v2.owners.clear();
    assert!(matches!(
        incomplete_flip.validate_semantics(),
        Err(MetricContractEvidenceSemanticErrorV1::FlipOwnerInvariant)
    ));
}

fn set_fsc_v2_unavailable(payload: &mut MetricContractEvidenceHashPayloadV1, total_buyers: u32) {
    let funding = &mut payload.contracts.funding_source_concentration;
    funding.v2_status = FscEvidenceStatus::Unavailable;
    funding.v2_envelope.availability = MetricAvailabilityV1::Unavailable;
    funding.v2_envelope.measurement_quality = MetricMeasurementQualityV1::NotApplicable;
    funding.known_coverage = CanonicalNullableV1::Null;
    funding.non_neutral_known_coverage = CanonicalNullableV1::Null;
    funding.known_buyer_count = 0;
    funding.known_non_neutral_buyer_count = 0;
    funding.total_buyer_count = total_buyers;
    payload.contracts.fsc_evidence_status.fsc_v2_status =
        CanonicalNullableV1::Value(FscEvidenceStatus::Unavailable);
    payload.contracts.fsc_evidence_status.fsc_v2_coverage = CanonicalNullableV1::Null;
}

fn assert_fsc_transport_semantic_rejected(
    payload: MetricContractEvidenceHashPayloadV1,
    invariant: &'static str,
) {
    assert!(matches!(
        MetricContractEvidenceTransportV1::try_new(payload.clone(), 123, 4),
        Err(MetricContractEvidenceTransportErrorV1::Semantic(
            MetricContractEvidenceSemanticErrorV1::FscV2Invariant(actual)
        )) if actual == invariant
    ));

    let correct_hash = payload.canonical_hash().unwrap();
    let encoded = json!({
        "payload": payload,
        "evidence_sha256": correct_hash,
        "writer_timestamp_ms": 123,
        "rotation_part_index": 4,
    });
    let error = serde_json::from_value::<MetricContractEvidenceTransportV1>(encoded).unwrap_err();
    assert!(
        error.to_string().contains(invariant),
        "serde must reject the same semantic invariant after a correct rehash: {error}"
    );
}

#[test]
fn full_evidence_transport_accepts_unavailable_fsc_with_non_empty_buyer_cohort() {
    let mut payload = complete_transport_payload();
    set_fsc_v2_unavailable(&mut payload, 2);
    payload.contracts.validate_semantics().unwrap();

    let transport = MetricContractEvidenceTransportV1::try_new(payload, 123, 4).unwrap();
    let encoded = serde_json::to_value(&transport).unwrap();
    let decoded: MetricContractEvidenceTransportV1 = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, transport);
    assert_eq!(
        decoded
            .payload
            .contracts
            .funding_source_concentration
            .total_buyer_count,
        2
    );
}

#[test]
fn full_evidence_transport_rejects_fsc_v2_count_ordering() {
    let mut non_neutral_exceeds_known = complete_transport_payload();
    non_neutral_exceeds_known
        .contracts
        .funding_source_concentration
        .known_buyer_count = 1;
    assert_fsc_transport_semantic_rejected(
        non_neutral_exceeds_known,
        "known non-neutral buyers exceed known buyers",
    );

    let mut known_exceeds_total = complete_transport_payload();
    known_exceeds_total
        .contracts
        .funding_source_concentration
        .total_buyer_count = 1;
    assert_fsc_transport_semantic_rejected(known_exceeds_total, "known buyers exceed total buyers");
}

#[test]
fn full_evidence_transport_rejects_fsc_v2_count_coverage_drift() {
    let mut non_neutral_drift = complete_transport_payload();
    non_neutral_drift
        .contracts
        .funding_source_concentration
        .known_non_neutral_buyer_count = 1;
    assert_fsc_transport_semantic_rejected(
        non_neutral_drift,
        "non-neutral known coverage does not match buyer counts",
    );

    let mut known_drift = complete_transport_payload();
    let funding = &mut known_drift.contracts.funding_source_concentration;
    funding.known_buyer_count = 1;
    funding.known_non_neutral_buyer_count = 1;
    funding.non_neutral_known_coverage = CanonicalNullableV1::Value(0.5);
    assert_fsc_transport_semantic_rejected(
        known_drift,
        "known coverage does not match buyer counts",
    );
}

#[test]
fn full_evidence_transport_rejects_fsc_v2_status_presence_drift() {
    let mut unavailable_coverage = complete_transport_payload();
    set_fsc_v2_unavailable(&mut unavailable_coverage, 2);
    unavailable_coverage
        .contracts
        .funding_source_concentration
        .known_coverage = CanonicalNullableV1::Value(0.0);
    assert_fsc_transport_semantic_rejected(
        unavailable_coverage,
        "unavailable status cannot expose known counts or coverage",
    );

    let mut unavailable_non_neutral_coverage = complete_transport_payload();
    set_fsc_v2_unavailable(&mut unavailable_non_neutral_coverage, 2);
    unavailable_non_neutral_coverage
        .contracts
        .funding_source_concentration
        .non_neutral_known_coverage = CanonicalNullableV1::Value(0.0);
    assert_fsc_transport_semantic_rejected(
        unavailable_non_neutral_coverage,
        "unavailable status cannot expose known counts or coverage",
    );

    let mut unavailable_known = complete_transport_payload();
    set_fsc_v2_unavailable(&mut unavailable_known, 2);
    unavailable_known
        .contracts
        .funding_source_concentration
        .known_buyer_count = 1;
    assert_fsc_transport_semantic_rejected(
        unavailable_known,
        "unavailable status cannot expose known counts or coverage",
    );

    for status in [FscEvidenceStatus::Clean, FscEvidenceStatus::Degraded] {
        let mut missing_coverage = complete_transport_payload();
        let funding = &mut missing_coverage.contracts.funding_source_concentration;
        funding.v2_status = status;
        funding.known_coverage = CanonicalNullableV1::Null;
        missing_coverage.contracts.fsc_evidence_status.fsc_v2_status =
            CanonicalNullableV1::Value(status);
        missing_coverage
            .contracts
            .fsc_evidence_status
            .fsc_v2_coverage = CanonicalNullableV1::Null;
        assert_fsc_transport_semantic_rejected(
            missing_coverage,
            "available status requires known coverage",
        );

        let mut missing_non_neutral_coverage = complete_transport_payload();
        let funding = &mut missing_non_neutral_coverage
            .contracts
            .funding_source_concentration;
        funding.v2_status = status;
        funding.non_neutral_known_coverage = CanonicalNullableV1::Null;
        missing_non_neutral_coverage
            .contracts
            .fsc_evidence_status
            .fsc_v2_status = CanonicalNullableV1::Value(status);
        assert_fsc_transport_semantic_rejected(
            missing_non_neutral_coverage,
            "available status requires non-neutral known coverage",
        );

        let mut zero_total = complete_transport_payload();
        let funding = &mut zero_total.contracts.funding_source_concentration;
        funding.v2_status = status;
        funding.known_buyer_count = 0;
        funding.known_non_neutral_buyer_count = 0;
        funding.total_buyer_count = 0;
        funding.known_coverage = CanonicalNullableV1::Value(0.0);
        funding.non_neutral_known_coverage = CanonicalNullableV1::Value(0.0);
        zero_total.contracts.fsc_evidence_status.fsc_v2_status = CanonicalNullableV1::Value(status);
        zero_total.contracts.fsc_evidence_status.fsc_v2_coverage = CanonicalNullableV1::Value(0.0);
        assert_fsc_transport_semantic_rejected(
            zero_total,
            "available status requires a non-empty buyer cohort",
        );
    }
}

#[test]
fn legacy_status_adapters_are_exhaustive_and_fail_closed() {
    for status in [
        EvidenceStatus::Clean,
        EvidenceStatus::Degraded,
        EvidenceStatus::Unavailable,
        EvidenceStatus::InsufficientSample,
        EvidenceStatus::Stale,
        EvidenceStatus::Fallback,
        EvidenceStatus::ShadowOnly,
        EvidenceStatus::NotConfigured,
    ] {
        adapt_evidence_status_v1(legacy_ftdi_context(), status)
            .expect("every EvidenceStatus variant maps");
    }
    for quality in [
        MetricEvidenceQuality::Clean,
        MetricEvidenceQuality::DegradedLowSample,
        MetricEvidenceQuality::CarriedForward,
        MetricEvidenceQuality::InsufficientSample,
        MetricEvidenceQuality::Stale,
        MetricEvidenceQuality::NotAllowed,
        MetricEvidenceQuality::UnavailableSource,
        MetricEvidenceQuality::Unavailable,
        MetricEvidenceQuality::NotConfigured,
    ] {
        adapt_metric_evidence_quality_v1(legacy_ftdi_context(), quality)
            .expect("every MetricEvidenceQuality variant maps");
    }
    for status in [
        FscEvidenceStatus::Clean,
        FscEvidenceStatus::Degraded,
        FscEvidenceStatus::Unavailable,
    ] {
        adapt_fsc_evidence_status_v1(legacy_ftdi_context(), status)
            .expect("every FscEvidenceStatus variant maps");
    }
    for reason in [
        FscExcludedReason::FundingLaneUnavailable,
        FscExcludedReason::IndexCold,
        FscExcludedReason::NoBuyerCohort,
        FscExcludedReason::InsufficientNonNeutralSupport,
        FscExcludedReason::LowCoverage,
        FscExcludedReason::NeutralOnly,
        FscExcludedReason::SameSlotOrderingUnavailable,
        FscExcludedReason::LowAttributionConfidence,
    ] {
        let _ = adapt_fsc_excluded_reason_v1(reason);
    }

    let clean_with_reason = FeatureEvidenceStatus {
        status: EvidenceStatus::Clean,
        degraded_reasons: vec![EvidenceDegradedReason::FscEvidencePartial],
        unavailable_reasons: vec![EvidenceUnavailableReason::FscMetricsMissing],
    };
    let adapted = adapt_feature_evidence_status_v1(legacy_ftdi_context(), &clean_with_reason)
        .expect("inconsistent legacy status maps conservatively");
    assert_eq!(
        adapted.measurement_quality,
        MetricMeasurementQualityV1::Degraded
    );
    assert!(!adapted.policy_actionable);
}

#[test]
fn envelope_rejects_invalid_combinations_during_construction_and_deserialization() {
    assert!(MetricEvidenceEnvelopeV1::<MetricEvidenceReasonV1>::try_new(
        MetricContractId::ReserveVelocity,
        1,
        MetricSurfaceId::ReserveVelocityEvidenceV1,
        MetricAuthorityClass::EvidenceOnly,
        MetricAvailabilityV1::Unavailable,
        MetricMeasurementQualityV1::Measured,
        false,
        vec![],
    )
    .is_err());

    let invalid = json!({
        "contract_id": "reserve_velocity",
        "contract_version": 1,
        "surface_id": "reserve_velocity_evidence_v1",
        "authority_class": "evidence_only",
        "availability": "unavailable",
        "measurement_quality": "measured",
        "policy_actionable": false,
        "reason_codes": []
    });
    assert!(serde_json::from_value::<CanonicalMetricEnvelopeV1>(invalid).is_err());

    assert!(matches!(
        MetricEvidenceEnvelopeV1::<MetricEvidenceReasonV1>::try_new(
            MetricContractId::ReserveVelocity,
            2,
            MetricSurfaceId::ReserveVelocityEvidenceV1,
            MetricAuthorityClass::EvidenceOnly,
            MetricAvailabilityV1::Available,
            MetricMeasurementQualityV1::Measured,
            false,
            vec![],
        ),
        Err(MetricEvidenceEnvelopeErrorV1::ContractVersionMismatch { .. })
    ));

    let wrong_reason_family = json!({
        "contract_id": "reserve_velocity",
        "contract_version": 1,
        "surface_id": "reserve_velocity_evidence_v1",
        "authority_class": "evidence_only",
        "availability": "available",
        "measurement_quality": "degraded",
        "policy_actionable": false,
        "reason_codes": [{
            "reason_family": "ftdi",
            "detail": "insufficient_buy_transactions"
        }]
    });
    assert!(serde_json::from_value::<CanonicalMetricEnvelopeV1>(wrong_reason_family).is_err());
}

#[test]
fn manipulation_presence_distinguishes_absent_from_measured_zero() {
    let numeric_v2_envelope = MetricEvidenceEnvelopeV1::try_new(
        MetricContractId::ManipulationContradiction,
        1,
        MetricSurfaceId::ManipulationNumericEvidenceV2,
        MetricAuthorityClass::EquivalentCutover,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::Degraded,
        false,
        vec![MetricEvidenceReasonV1::Manipulation(
            ManipulationEvidenceReasonV1::RawFieldAbsent,
        )],
    )
    .unwrap();
    let legacy_numeric_envelope = MetricEvidenceEnvelopeV1::try_new(
        MetricContractId::ManipulationContradiction,
        1,
        MetricSurfaceId::MfsManipulationNumericLegacyDefaults,
        MetricAuthorityClass::Authoritative,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::LegacyDefault,
        false,
        vec![MetricEvidenceReasonV1::Manipulation(
            ManipulationEvidenceReasonV1::LegacyDefaultZero,
        )],
    )
    .unwrap();
    let legacy_high_flags_envelope = MetricEvidenceEnvelopeV1::try_new(
        MetricContractId::ManipulationContradiction,
        1,
        MetricSurfaceId::MfsManipulationHighFlagsLegacyDefaults,
        MetricAuthorityClass::Authoritative,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::LegacyDefault,
        false,
        vec![MetricEvidenceReasonV1::Manipulation(
            ManipulationEvidenceReasonV1::LegacyDefaultFalse,
        )],
    )
    .unwrap();
    let derived_high_flags_envelope = MetricEvidenceEnvelopeV1::try_new(
        MetricContractId::ManipulationContradiction,
        1,
        MetricSurfaceId::PolicyDerivedManipulationHighFlagsV2,
        MetricAuthorityClass::EquivalentCutover,
        MetricAvailabilityV1::Available,
        MetricMeasurementQualityV1::Degraded,
        false,
        vec![MetricEvidenceReasonV1::Manipulation(
            ManipulationEvidenceReasonV1::RawFieldAbsent,
        )],
    )
    .unwrap();
    let evidence = ManipulationNumericEvidenceV2 {
        legacy_numeric_envelope,
        numeric_v2_envelope,
        measured_fields_mask: 0b10,
        legacy_fields: vec![],
        fields: vec![
            ManipulationNumericFieldEvidenceV2 {
                field_id: ManipulationNumericFieldIdV2::SameMsTxRatio,
                value: CanonicalNullableV1::Null,
                availability: MetricAvailabilityV1::NotRecordedLegacySchema,
                measurement_quality: MetricMeasurementQualityV1::NotApplicable,
                reason_codes: vec![MetricEvidenceReasonV1::Manipulation(
                    ManipulationEvidenceReasonV1::RawFieldAbsent,
                )],
            },
            ManipulationNumericFieldEvidenceV2 {
                field_id: ManipulationNumericFieldIdV2::BundleSuspicionRatio,
                value: CanonicalNullableV1::Value(0.0),
                availability: MetricAvailabilityV1::Available,
                measurement_quality: MetricMeasurementQualityV1::Measured,
                reason_codes: vec![],
            },
        ],
        legacy_high_flags_envelope,
        legacy_high_flags: vec![],
        derived_high_flags_envelope,
        derived_high_flags: vec![],
    };
    let encoded = serde_json::to_string(&evidence).unwrap();
    let decoded: ManipulationNumericEvidenceV2 = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.fields[0].value, CanonicalNullableV1::Null);
    assert_eq!(decoded.fields[1].value, CanonicalNullableV1::Value(0.0));
}
