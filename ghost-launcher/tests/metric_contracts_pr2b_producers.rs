use ghost_brain::config::{GatekeeperV2Config, MetricContractFoundationConfigV1};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::AccountStateReserveVelocitySnapshotV1;
use ghost_core::checkpoint::{
    EvidenceStatus, ManipulationContradictionFeatures, MaterializedFeatureSet,
};
use ghost_core::metric_contracts::*;
use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata, SlotQuality};
use ghost_launcher::components::gatekeeper::GatekeeperDevPrimaryCompatibilitySnapshotV1;
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::metric_contracts::{
    build_flip_evidence_v2, build_manipulation_evidence_v2,
    build_pr2b_complete_metric_contract_snapshot_v1, build_recent_buy_sell_evidence_v1,
    build_reserve_velocity_evidence_v1, freeze_manipulation_producer_snapshot_v2,
    resolve_metric_contract_effective_config_v1, ManipulationFrozenSnapshotV2,
    ManipulationProducerFieldV2, ManipulationProducerSnapshotV2, Pr2aFrozenProducerInputsV1,
    Pr2bBuildContextV1, Pr2bFrozenProducerInputsV1, RecentBuySellProducerSnapshotV1,
};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::{
    compute_ftdi, FlipV2StateMachineV1, FundingSourceConfig, FundingSourceIndex,
    TxIntelligenceConfig, TxIntelligenceEngine, TxTimingProducerSnapshotV1,
};
use seer::early_fingerprint::EarlyFingerprintConfig;
use seer::types::ToolchainFingerprintInput;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::sync::Arc;
use std::time::Instant;

fn runtime_context() -> (
    MetricContractProfileV1,
    ResolvedMetricContractEffectiveConfigV1,
    GatekeeperV2Config,
    TxIntelligenceConfig,
    FundingSourceConfig,
) {
    let gatekeeper = GatekeeperV2Config::default();
    let fingerprint = EarlyFingerprintConfig::default();
    let tx_config = TxIntelligenceConfig::from_gatekeeper_config(&gatekeeper, fingerprint.clone());
    let funding = FundingSourceConfig::from_gatekeeper_config(&gatekeeper);
    let effective = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &gatekeeper,
        &tx_config,
        &fingerprint,
        &funding,
        None,
    )
    .unwrap();
    (
        MetricContractProfileV1::profile_a().unwrap(),
        effective,
        gatekeeper,
        tx_config,
        funding,
    )
}

fn build_context<'a>(
    profile: &'a MetricContractProfileV1,
    effective: &'a ResolvedMetricContractEffectiveConfigV1,
) -> Pr2bBuildContextV1<'a> {
    Pr2bBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile,
        effective_config: effective,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(10_000, Some(100)).unwrap(),
    }
}

fn projection_context<'a>(
    profile: &'a MetricContractProfileV1,
    effective: &'a ResolvedMetricContractEffectiveConfigV1,
    source_cutoff: MetricContractDecisionSourceCutoffV1,
) -> MetricDecisionProjectionBuildContextV1<'a> {
    MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile,
        effective_config: effective,
        source_cutoff,
    }
}

fn measured_manipulation() -> ManipulationContradictionFeatures {
    ManipulationContradictionFeatures {
        same_ms_tx_ratio: 0.0,
        bundle_suspicion_ratio: 0.0,
        top3_volume_pct: 0.0,
        hhi: 0.0,
        max_tx_per_signer: 0,
        dev_volume_ratio: 0.0,
        contradiction_score: 0.0,
        status: EvidenceStatus::Clean,
        ..ManipulationContradictionFeatures::default()
    }
}

fn frozen_manipulation(
    features: ManipulationContradictionFeatures,
) -> ManipulationFrozenSnapshotV2 {
    let available = matches!(
        features.status,
        EvidenceStatus::Clean | EvidenceStatus::Degraded
    );
    let quality = if features.status == EvidenceStatus::Clean {
        MetricMeasurementQualityV1::Measured
    } else if features.status == EvidenceStatus::Degraded {
        MetricMeasurementQualityV1::Degraded
    } else {
        MetricMeasurementQualityV1::NotApplicable
    };
    let field = |value| ManipulationProducerFieldV2 {
        value: available.then_some(value),
        availability: if available {
            MetricAvailabilityV1::Available
        } else {
            MetricAvailabilityV1::Unavailable
        },
        measurement_quality: quality,
        reasons: if available {
            Vec::new()
        } else {
            vec![MetricEvidenceReasonV1::Manipulation(
                ManipulationEvidenceReasonV1::RawFieldAbsent,
            )]
        },
    };
    ManipulationFrozenSnapshotV2 {
        typed: ManipulationProducerSnapshotV2 {
            same_ms_tx_ratio: field(features.same_ms_tx_ratio),
            bundle_suspicion_ratio: field(features.bundle_suspicion_ratio),
            top3_signer_volume_ratio: field(features.top3_volume_pct),
            hhi: field(features.hhi),
            max_tx_per_signer: ManipulationProducerFieldV2 {
                value: available.then_some(features.max_tx_per_signer),
                availability: if available {
                    MetricAvailabilityV1::Available
                } else {
                    MetricAvailabilityV1::Unavailable
                },
                measurement_quality: quality,
                reasons: if available {
                    Vec::new()
                } else {
                    vec![MetricEvidenceReasonV1::Manipulation(
                        ManipulationEvidenceReasonV1::RawFieldAbsent,
                    )]
                },
            },
            dev_volume_ratio: field(features.dev_volume_ratio),
            contradiction_score: field(features.contradiction_score),
            group_status: features.status,
            group_reasons: features.reasons.clone(),
        },
        legacy: features,
    }
}

fn nullable_f64(value: &CanonicalNullableV1<f64>) -> Option<f64> {
    match value {
        CanonicalNullableV1::Value(value) => Some(*value),
        CanonicalNullableV1::Null => None,
    }
}

fn recent_tx(timestamp_ms: u64, is_buy: bool, success: bool) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope {
            slot_quality: SlotQuality::Present,
            ..EventSemanticEnvelope::default()
        },
        pool_amm_id: Pubkey::new_unique().to_string(),
        slot: Some(timestamp_ms / 10 + 1),
        event_ordinal: Some((timestamp_ms % 1_000) as u32),
        tx_index: Some((timestamp_ms % 1_000) as u32),
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        event_time: EventTimeMetadata {
            ingress_wall_ts_ms: Some(timestamp_ms),
            ..EventTimeMetadata::default()
        },
        arrival_ts_ms: timestamp_ms,
        signer: Pubkey::new_unique().to_string(),
        is_buy,
        volume_sol: 1.0,
        sol_amount_lamports: Some(1_000_000_000),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Signature::new_unique().to_string(),
        success,
        error_code: (!success).then(|| "failed".to_string()),
        compute_units_consumed: None,
        owner_token_deltas: Vec::new(),
        mpcf_payload: Vec::new(),
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        token_mint: None,
        v_tokens_in_bonding_curve: None,
        v_sol_in_bonding_curve: None,
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
        creator_vault: None,
        bonding_curve_v2: None,
        bonding_curve_v2_provenance: None,
        buy_remaining_accounts: Vec::new(),
        is_mayhem_mode: None,
        cu_price_micro_lamports: None,
        compute_unit_limit: None,
        inner_ix_count: None,
        cpi_depth: None,
        ata_create_count: None,
        signer_pre_balance_lamports: None,
        signer_post_balance_lamports: None,
        jito_tip_detected: None,
        toolchain_fingerprint: ToolchainFingerprintInput::default(),
        curve_data_known: false,
        curve_finality: CurveFinality::Speculative,
    }
}

fn recent_snapshot(events: Vec<PoolTransaction>) -> RecentBuySellProducerSnapshotV1 {
    let (.., gatekeeper, _, funding) = runtime_context();
    let manager = SessionManager::new(SessionConfig {
        max_sessions: 1,
        ..SessionConfig::default()
    });
    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let mut candidate = EnhancedCandidate {
        pool_amm_id: pool,
        base_mint,
        bonding_curve,
        timestamp: 1_000,
        ..EnhancedCandidate::default()
    };
    candidate.signature = Signature::new_unique().to_string();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate,
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(31_000),
            funding_source_config: funding,
            gatekeeper_config: gatekeeper,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .unwrap();
    let session = manager.get_session(&pool).unwrap();
    let mut session = session.write();
    for event in events {
        session.ingest_transaction(Arc::new(event));
    }
    session.metric_contract_recent_buy_sell_snapshot().unwrap()
}

fn threshold(
    effective: &ResolvedMetricContractEffectiveConfigV1,
    id: ManipulationNumericFieldIdV2,
) -> f64 {
    let key = match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => {
            MetricEffectiveConfigKeyV1::ManipulationHighSameMsThreshold
        }
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => {
            MetricEffectiveConfigKeyV1::ManipulationHighBundleThreshold
        }
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => {
            MetricEffectiveConfigKeyV1::ManipulationHighTop3Threshold
        }
        ManipulationNumericFieldIdV2::Hhi => {
            MetricEffectiveConfigKeyV1::ManipulationHighHhiThreshold
        }
        ManipulationNumericFieldIdV2::MaxTxPerSigner => {
            MetricEffectiveConfigKeyV1::ManipulationHighSignerCountThreshold
        }
        ManipulationNumericFieldIdV2::DevVolumeRatio => {
            MetricEffectiveConfigKeyV1::ManipulationHighDevConcentrationThreshold
        }
        ManipulationNumericFieldIdV2::ContradictionScore => {
            panic!("contradiction score has no high flag")
        }
    };
    match effective.value(key).unwrap() {
        MetricEffectiveConfigValueV1::Ratio(value) => *value,
        MetricEffectiveConfigValueV1::WideUnsigned(value) => value.get() as f64,
        value => panic!("unexpected threshold kind: {value:?}"),
    }
}

fn set_manipulation_value(
    features: &mut ManipulationContradictionFeatures,
    id: ManipulationNumericFieldIdV2,
    value: f64,
) {
    match id {
        ManipulationNumericFieldIdV2::SameMsTxRatio => features.same_ms_tx_ratio = value,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio => {
            features.bundle_suspicion_ratio = value
        }
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio => features.top3_volume_pct = value,
        ManipulationNumericFieldIdV2::Hhi => features.hhi = value,
        ManipulationNumericFieldIdV2::MaxTxPerSigner => features.max_tx_per_signer = value as u64,
        ManipulationNumericFieldIdV2::DevVolumeRatio => features.dev_volume_ratio = value,
        ManipulationNumericFieldIdV2::ContradictionScore => features.contradiction_score = value,
    }
}

#[test]
fn flip_builder_uses_owner_snapshot_once_and_keeps_legacy_isolated() {
    let (profile, effective, _, tx_config, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let machine = FlipV2StateMachineV1::new(
        &tx_config.fingerprint,
        tx_config.min_sol_threshold,
        tx_config.tx_key_capacity,
        1_000,
    );
    let snapshot = machine.snapshot(10_000, Some(100));
    let evidence = build_flip_evidence_v2(Some(0.25), &snapshot, &context).unwrap();
    assert_eq!(
        evidence.legacy_slot_gap_ratio,
        CanonicalNullableV1::Value(0.25)
    );
    assert_eq!(evidence.hybrid_v2.ratio, CanonicalNullableV1::Null);
    let projected = FlipDecisionProjectionV1::try_from_evidence(
        &evidence,
        &projection_context(&profile, &effective, context.source_cutoff.clone()),
    )
    .unwrap();
    assert_eq!(
        projected.legacy_slot_gap_ratio.value,
        CanonicalNullableV1::Value(0.25)
    );
    assert_eq!(projected.hybrid_v2_ratio.value, CanonicalNullableV1::Null);
}

#[test]
fn manipulation_absent_is_not_zero_and_explicit_measured_zero_remains_zero() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let absent = build_manipulation_evidence_v2(
        &frozen_manipulation(ManipulationContradictionFeatures::default()),
        &context,
    )
    .unwrap();
    assert!(absent.fields.iter().all(|field| field.value.is_null()));
    assert!(absent.legacy_fields.iter().all(|field| {
        field.value == CanonicalNullableV1::Value(0.0)
            && field.measurement_quality == MetricMeasurementQualityV1::LegacyDefault
    }));
    assert_eq!(absent.measured_fields_mask, 0);
    assert!(absent
        .derived_high_flags
        .iter()
        .all(|flag| flag.derived_value.is_null()));

    let measured =
        build_manipulation_evidence_v2(&frozen_manipulation(measured_manipulation()), &context)
            .unwrap();
    assert!(measured.fields.iter().all(|field| {
        field.value == CanonicalNullableV1::Value(0.0)
            && field.measurement_quality == MetricMeasurementQualityV1::Measured
    }));
    assert_eq!(measured.measured_fields_mask, 0x7f);
}

#[test]
fn manipulation_owner_snapshot_preserves_mixed_presence_and_explicit_zero() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let mut materialized = MaterializedFeatureSet::default();
    materialized.tx_intel_features.tx_count = 2;
    materialized.tx_intel_features.unique_signers = 2;
    materialized.tx_intel_features.same_ms_tx_ratio = 0.0;
    materialized.tx_intel_features.bundle_suspicion_ratio = 0.2;
    materialized.tx_intel_features.top3_signer_volume_ratio = None;
    materialized.tx_intel_features.top3_volume_pct = 0.0;
    materialized.tx_intel_features.hhi = 0.5;
    materialized.tx_intel_features.max_tx_per_signer = 2;
    materialized.tx_intel_features.total_volume_sol = 1.0;
    materialized.tx_intel_features.dev_wallet_known = true;
    materialized.tx_intel_features.dev_volume_ratio = 0.0;
    let legacy = ManipulationContradictionFeatures {
        same_ms_tx_ratio: 0.0,
        bundle_suspicion_ratio: 0.2,
        top3_volume_pct: 0.0,
        hhi: 0.5,
        max_tx_per_signer: 2,
        dev_volume_ratio: 0.0,
        contradiction_score: 0.0,
        status: EvidenceStatus::Degraded,
        ..ManipulationContradictionFeatures::default()
    };
    let frozen = freeze_manipulation_producer_snapshot_v2(&materialized, legacy);
    assert_eq!(frozen.typed.group_status, EvidenceStatus::Degraded);
    assert_eq!(frozen.typed.same_ms_tx_ratio.value, Some(0.0));
    assert_eq!(frozen.typed.bundle_suspicion_ratio.value, Some(0.2));
    assert_eq!(frozen.typed.top3_signer_volume_ratio.value, None);
    assert_eq!(frozen.typed.hhi.value, Some(0.5));
    assert_eq!(frozen.typed.max_tx_per_signer.value, Some(2));
    assert_eq!(frozen.typed.dev_volume_ratio.value, Some(0.0));
    assert_eq!(frozen.typed.contradiction_score.value, None);

    let evidence = build_manipulation_evidence_v2(&frozen, &context).unwrap();
    let expected_mask = ManipulationNumericFieldIdV2::SameMsTxRatio.measured_mask_bit()
        | ManipulationNumericFieldIdV2::BundleSuspicionRatio.measured_mask_bit()
        | ManipulationNumericFieldIdV2::Hhi.measured_mask_bit()
        | ManipulationNumericFieldIdV2::MaxTxPerSigner.measured_mask_bit()
        | ManipulationNumericFieldIdV2::DevVolumeRatio.measured_mask_bit();
    assert_eq!(evidence.measured_fields_mask, expected_mask);
    let top3_flag = evidence
        .derived_high_flags
        .iter()
        .find(|flag| flag.field_id == ManipulationNumericFieldIdV2::Top3SignerVolumeRatio)
        .unwrap();
    assert_eq!(top3_flag.raw_value, CanonicalNullableV1::Null);
    assert_eq!(top3_flag.derived_value, CanonicalNullableV1::Null);
    assert_eq!(
        evidence
            .legacy_fields
            .iter()
            .find(|field| field.field_id == ManipulationNumericFieldIdV2::Top3SignerVolumeRatio)
            .unwrap()
            .value,
        CanonicalNullableV1::Value(0.0)
    );

    let round_trip: ManipulationNumericEvidenceV2 =
        serde_json::from_slice(&serde_json::to_vec(&evidence).unwrap()).unwrap();
    assert_eq!(round_trip, evidence);
    let projected = ManipulationDecisionProjectionV1::try_from_evidence(
        &evidence,
        &projection_context(&profile, &effective, context.source_cutoff.clone()),
    )
    .unwrap();
    assert_eq!(projected.measured_fields_mask, expected_mask);
    assert_eq!(
        projected.same_ms_tx_ratio.value,
        CanonicalNullableV1::Value(0.0)
    );
    assert_eq!(
        projected.top3_signer_volume_ratio.value,
        CanonicalNullableV1::Null
    );
    assert_eq!(
        projected.contradiction_score.value,
        CanonicalNullableV1::Null
    );
}

#[test]
fn clean_manipulation_group_with_missing_required_field_downgrades_or_fails_closed() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let mut materialized = MaterializedFeatureSet::default();
    materialized.tx_intel_features.tx_count = 1;
    materialized.tx_intel_features.unique_signers = 1;
    materialized.tx_intel_features.total_volume_sol = 1.0;
    materialized.tx_intel_features.dev_wallet_known = true;
    let legacy = measured_manipulation();
    let frozen = freeze_manipulation_producer_snapshot_v2(&materialized, legacy);
    assert_eq!(frozen.typed.group_status, EvidenceStatus::Degraded);
    assert!(frozen.typed.top3_signer_volume_ratio.value.is_none());
    build_manipulation_evidence_v2(&frozen, &context).unwrap();

    let mut invalid = frozen_manipulation(measured_manipulation());
    invalid.typed.top3_signer_volume_ratio = ManipulationProducerFieldV2 {
        value: None,
        availability: MetricAvailabilityV1::Unavailable,
        measurement_quality: MetricMeasurementQualityV1::NotApplicable,
        reasons: vec![MetricEvidenceReasonV1::Manipulation(
            ManipulationEvidenceReasonV1::RawFieldAbsent,
        )],
    };
    assert!(matches!(
        build_manipulation_evidence_v2(&invalid, &context),
        Err(
            ghost_launcher::metric_contracts::Pr2bProducerErrorV1::ProducerInvariant(
                "clean manipulation group requires every field measured"
            )
        )
    ));
}

#[test]
fn manipulation_threshold_truth_table_is_strict_for_all_six_flags() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let ids = [
        ManipulationNumericFieldIdV2::SameMsTxRatio,
        ManipulationNumericFieldIdV2::BundleSuspicionRatio,
        ManipulationNumericFieldIdV2::Top3SignerVolumeRatio,
        ManipulationNumericFieldIdV2::Hhi,
        ManipulationNumericFieldIdV2::MaxTxPerSigner,
        ManipulationNumericFieldIdV2::DevVolumeRatio,
    ];
    for id in ids {
        let boundary = threshold(&effective, id);
        let mut equal = measured_manipulation();
        set_manipulation_value(&mut equal, id, boundary);
        let evidence =
            build_manipulation_evidence_v2(&frozen_manipulation(equal), &context).unwrap();
        let flag = evidence
            .derived_high_flags
            .iter()
            .find(|flag| flag.field_id == id)
            .unwrap();
        assert_eq!(flag.comparator, ManipulationComparatorV1::GreaterThan);
        assert_eq!(
            flag.derived_value,
            CanonicalNullableV1::Value(false),
            "{id:?} equality"
        );

        let mut above = measured_manipulation();
        let above_value = if id == ManipulationNumericFieldIdV2::MaxTxPerSigner {
            boundary + 1.0
        } else {
            f64::from_bits(boundary.to_bits() + 1)
        };
        set_manipulation_value(&mut above, id, above_value);
        let evidence =
            build_manipulation_evidence_v2(&frozen_manipulation(above), &context).unwrap();
        let flag = evidence
            .derived_high_flags
            .iter()
            .find(|flag| flag.field_id == id)
            .unwrap();
        assert_eq!(
            flag.derived_value,
            CanonicalNullableV1::Value(true),
            "{id:?} above"
        );
    }
}

#[test]
fn manipulation_projection_rejects_provenance_drift() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let projection_context =
        projection_context(&profile, &effective, context.source_cutoff.clone());
    for mutation in 0..3 {
        let mut evidence =
            build_manipulation_evidence_v2(&frozen_manipulation(measured_manipulation()), &context)
                .unwrap();
        match mutation {
            0 => evidence.derived_high_flags[0].policy_stage = "wrong".to_string(),
            1 => evidence.derived_high_flags[0].policy_version = "wrong".to_string(),
            _ => {
                evidence.derived_high_flags[0].config_hash =
                    CanonicalHashV1::digest(&"wrong").unwrap()
            }
        }
        assert!(matches!(
            ManipulationDecisionProjectionV1::try_from_evidence(&evidence, &projection_context),
            Err(MetricContractProjectionErrorV1::FamilyInvariant(
                "manipulation derived provenance"
            ))
        ));
    }
}

#[test]
fn reserve_builder_preserves_all_typed_nonmeasured_states_and_exact_measured_formula() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let projection_context =
        projection_context(&profile, &effective, context.source_cutoff.clone());
    let cases = [
        AccountStateReserveVelocitySnapshotV1 {
            legacy_velocity_sol_per_sec: 0.0,
            previous_real_sol_reserves_lamports: None,
            current_real_sol_reserves_lamports: Some(1_000_000_000),
            interval_ms: None,
            accepted_update_count: 1,
            status: ReserveVelocityStatusV1::FirstUpdate,
        },
        AccountStateReserveVelocitySnapshotV1 {
            legacy_velocity_sol_per_sec: 1.0,
            previous_real_sol_reserves_lamports: Some(1_000_000_000),
            current_real_sol_reserves_lamports: Some(2_000_000_000),
            interval_ms: Some(1_000),
            accepted_update_count: 2,
            status: ReserveVelocityStatusV1::Measured,
        },
        AccountStateReserveVelocitySnapshotV1 {
            legacy_velocity_sol_per_sec: 0.0,
            previous_real_sol_reserves_lamports: Some(2_000_000_000),
            current_real_sol_reserves_lamports: Some(2_000_000_000),
            interval_ms: Some(1_000),
            accepted_update_count: 3,
            status: ReserveVelocityStatusV1::Measured,
        },
        AccountStateReserveVelocitySnapshotV1 {
            legacy_velocity_sol_per_sec: 0.0,
            previous_real_sol_reserves_lamports: Some(2_000_000_000),
            current_real_sol_reserves_lamports: Some(3_000_000_000),
            interval_ms: Some(0),
            accepted_update_count: 4,
            status: ReserveVelocityStatusV1::ZeroDeltaTime,
        },
        AccountStateReserveVelocitySnapshotV1 {
            status: ReserveVelocityStatusV1::BootstrapFallback,
            ..AccountStateReserveVelocitySnapshotV1::default()
        },
        AccountStateReserveVelocitySnapshotV1::default(),
    ];
    for snapshot in cases {
        let evidence = build_reserve_velocity_evidence_v1(&snapshot, &context).unwrap();
        let projection =
            ReserveVelocityDecisionProjectionV1::try_from_evidence(&evidence, &projection_context)
                .unwrap();
        assert_eq!(projection.status, snapshot.status);
        assert_eq!(
            projection.velocity_v1.value.is_null(),
            snapshot.status != ReserveVelocityStatusV1::Measured
        );
    }
    let invalid = AccountStateReserveVelocitySnapshotV1 {
        legacy_velocity_sol_per_sec: f64::NAN,
        ..AccountStateReserveVelocitySnapshotV1::default()
    };
    assert!(build_reserve_velocity_evidence_v1(&invalid, &context).is_err());
}

#[test]
fn recent_buy_sell_reconstructs_legacy_unbounded_bounded_and_zero_denominators() {
    let (profile, effective, _, _, _) = runtime_context();
    let context = build_context(&profile, &effective);
    let projection_context =
        projection_context(&profile, &effective, context.source_cutoff.clone());
    for (buy, sell, legacy, unbounded, share) in [
        (6, 0, Some(6.0), None, Some(1.0)),
        (1, 1, Some(1.0), Some(1.0), Some(0.5)),
        (0, 0, None, None, None),
    ] {
        let snapshot = RecentBuySellProducerSnapshotV1 {
            window_ms: 10_000,
            buy_count: buy,
            sell_count: sell,
            transaction_count: buy + sell,
            failed_transaction_count: 1,
            source_complete: true,
        };
        let evidence = build_recent_buy_sell_evidence_v1(&snapshot, &context).unwrap();
        assert_eq!(nullable_f64(&evidence.legacy_buy_sell_scalar), legacy);
        assert_eq!(nullable_f64(&evidence.buy_to_sell_ratio), unbounded);
        assert_eq!(nullable_f64(&evidence.buy_share), share);
        let projection =
            RecentBuySellDecisionProjectionV1::try_from_evidence(&evidence, &projection_context)
                .unwrap();
        assert_eq!(
            projection.transaction_count,
            u32::try_from(buy + sell).unwrap()
        );
    }
}

#[test]
fn recent_owner_is_successful_only_and_uses_inclusive_window_boundaries() {
    let snapshot = recent_snapshot(vec![
        recent_tx(1_000, true, true),
        recent_tx(1_000, false, false),
        recent_tx(11_000, false, true),
    ]);
    assert_eq!(snapshot.window_ms, 10_000);
    assert_eq!(snapshot.buy_count, 1, "inclusive start BUY");
    assert_eq!(snapshot.sell_count, 1, "inclusive end SELL");
    assert_eq!(snapshot.transaction_count, 2);
    assert_eq!(snapshot.failed_transaction_count, 1);
    assert!(snapshot.source_complete);
}

fn current_materialized_projection() -> (
    ghost_core::checkpoint::MaterializedFeatureSet,
    MetricContractProfileV1,
    ResolvedMetricContractEffectiveConfigV1,
) {
    let (profile, effective, gatekeeper, _, funding) = runtime_context();
    let manager = SessionManager::new(SessionConfig {
        max_sessions: 1,
        ..SessionConfig::default()
    });
    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let mut candidate = EnhancedCandidate {
        pool_amm_id: pool,
        base_mint,
        bonding_curve,
        timestamp: 1_000,
        ..EnhancedCandidate::default()
    };
    candidate.signature = Signature::new_unique().to_string();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate,
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(31_000),
            funding_source_config: funding,
            gatekeeper_config: gatekeeper,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .unwrap();
    let session = manager.get_session(&pool).unwrap();
    let materialized = session.read().try_materialize_features().unwrap();
    (materialized, profile, effective)
}

#[test]
fn mfs_historical_absence_is_none_but_current_materialization_is_atomic_some() {
    let (materialized, profile, effective) = current_materialized_projection();
    let projection = materialized
        .metric_contract_decision_projection_v1
        .as_ref()
        .expect("current terminal snapshot must contain the complete projection");
    let context = projection_context(
        &profile,
        &effective,
        projection
            .flip_ratio
            .legacy_slot_gap_ratio
            .source_cutoff
            .clone(),
    );
    projection.validate_context(&context).unwrap();
    projection.validated_canonical_hash(&context).unwrap();
    let mut json = serde_json::to_value(&materialized).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("metric_contract_decision_projection_v1");
    let historical: ghost_core::checkpoint::MaterializedFeatureSet =
        serde_json::from_value(json).unwrap();
    assert!(historical.metric_contract_decision_projection_v1.is_none());
    let current_json = serde_json::to_string(&materialized).unwrap();
    assert!(!current_json.contains("metric_contracts_evidence_set_v1"));
}

#[test]
fn projection_resource_gate_is_deterministic_bounded_and_rejects_hard_max() {
    let (complete, profile, effective, source_cutoff) = complete_snapshot_fixture();
    let projection = complete.compact_projection;
    let context = projection_context(&profile, &effective, source_cutoff);
    let size = projection.authoritative_serialized_size_bytes().unwrap();
    assert!(size <= METRIC_CONTRACT_PROJECTION_SERIALIZED_P95_TARGET_BYTES_V1);
    assert_eq!(
        size,
        projection.authoritative_serialized_size_bytes().unwrap()
    );
    projection.validated_canonical_hash(&context).unwrap();

    let mut large_allowed = projection.clone();
    large_allowed
        .flip_ratio
        .hybrid_v2_ratio
        .envelope
        .reasons
        .codes = vec![MetricEvidenceReasonV1::UnmappedLegacyString {
        contract_id: MetricContractId::FlipRatio,
        raw: "x".repeat(6 * 1_024),
    }];
    let large_allowed_size = large_allowed.authoritative_serialized_size_bytes().unwrap();
    assert!(large_allowed_size > size);
    assert!(large_allowed_size <= METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1);
    large_allowed.validated_canonical_hash(&context).unwrap();

    let mut oversized = projection.clone();
    oversized.flip_ratio.hybrid_v2_ratio.envelope.reasons.codes =
        vec![MetricEvidenceReasonV1::UnmappedLegacyString {
            contract_id: MetricContractId::FlipRatio,
            raw: "x".repeat(METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1 * 2),
        }];
    let oversized_bytes = oversized.authoritative_serialized_size_bytes().unwrap();
    assert!(oversized_bytes > METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1);
    assert!(matches!(
        oversized.validated_canonical_hash(&context),
        Err(MetricContractProjectionErrorV1::ProjectionTooLarge {
            actual_bytes,
            hard_max_bytes: METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1,
        }) if actual_bytes == oversized_bytes
    ));

    if !cfg!(debug_assertions) {
        let profile_started = Instant::now();
        for _ in 0..32 {
            context.profile.canonical_hash().unwrap();
        }
        let profile_hash_us = profile_started.elapsed().as_micros() / 32;
        let config_started = Instant::now();
        for _ in 0..32 {
            context.effective_config.validate_hash().unwrap();
        }
        let config_hash_us = config_started.elapsed().as_micros() / 32;
        let evidence_started = Instant::now();
        for _ in 0..32 {
            complete.full_evidence.validate_semantics().unwrap();
        }
        let evidence_validate_us = evidence_started.elapsed().as_micros() / 32;
        let projection_started = Instant::now();
        for _ in 0..32 {
            projection.validate_context(&context).unwrap();
        }
        let projection_validate_us = projection_started.elapsed().as_micros() / 32;
        eprintln!(
            "PR2B release diagnostic mean_us: profile_hash={} config_hash={} evidence_validate={} projection_validate={}",
            profile_hash_us, config_hash_us, evidence_validate_us, projection_validate_us,
        );
    }

    // Exclude allocator/code-page warm-up from the steady-state release
    // distribution; acceptance samples below still execute the complete path.
    for _ in 0..32 {
        let rebuilt = MetricContractDecisionEvidenceProjectionV1::try_from_evidence(
            &complete.full_evidence,
            &context,
        )
        .unwrap();
        rebuilt.authoritative_serialized_bytes().unwrap();
    }

    let mut build_validate_samples = Vec::with_capacity(256);
    let mut wire_serialize_samples = Vec::with_capacity(256);
    let mut combined_samples = Vec::with_capacity(256);
    for _ in 0..256 {
        let started = Instant::now();
        let rebuilt = MetricContractDecisionEvidenceProjectionV1::try_from_evidence(
            &complete.full_evidence,
            &context,
        )
        .unwrap();
        build_validate_samples.push(started.elapsed().as_micros());
        assert_eq!(rebuilt, projection);

        let started = Instant::now();
        let wire_bytes = rebuilt.authoritative_serialized_bytes().unwrap();
        wire_serialize_samples.push(started.elapsed().as_micros());
        assert_eq!(wire_bytes.len(), size);

        let started = Instant::now();
        let rebuilt = MetricContractDecisionEvidenceProjectionV1::try_from_evidence(
            &complete.full_evidence,
            &context,
        )
        .unwrap();
        rebuilt.authoritative_serialized_bytes().unwrap();
        combined_samples.push(started.elapsed().as_micros());
    }
    let percentiles = |mut samples: Vec<u128>| {
        samples.sort_unstable();
        (
            samples[samples.len() * 50 / 100],
            samples[samples.len() * 95 / 100],
            samples[samples.len() * 99 / 100],
        )
    };
    let (build_p50, build_p95, build_p99) = percentiles(build_validate_samples);
    let (wire_p50, wire_p95, wire_p99) = percentiles(wire_serialize_samples);
    let (combined_p50, combined_p95, combined_p99) = percentiles(combined_samples);
    let verbose_bytes = projection
        .verbose_domain_json_diagnostic_size_bytes()
        .unwrap();
    let bincode_bytes = projection.bincode_diagnostic_size_bytes().unwrap();
    eprintln!(
        "PR2B projection resource gate: projection_wire_json_bytes={size} large_allowed_wire_json_bytes={large_allowed_size} projection_verbose_domain_json_diagnostic_bytes={verbose_bytes} projection_bincode_diagnostic_bytes={bincode_bytes} projection_build_and_validate_us_p50={build_p50} projection_build_and_validate_us_p95={build_p95} projection_build_and_validate_us_p99={build_p99} projection_wire_serialize_us_p50={wire_p50} projection_wire_serialize_us_p95={wire_p95} projection_wire_serialize_us_p99={wire_p99} projection_build_validate_serialize_us_p50={combined_p50} projection_build_validate_serialize_us_p95={combined_p95} projection_build_validate_serialize_us_p99={combined_p99}"
    );
    if !cfg!(debug_assertions) {
        assert!(
            build_p99 <= 1_000,
            "release build/validate p99 {build_p99}us exceeds 1000us"
        );
    }
}

#[test]
fn compact_json_wire_v1_roundtrips_all_families_and_has_a_frozen_schema() {
    let (complete, profile, effective, source_cutoff) = complete_snapshot_fixture();
    let projection = complete.compact_projection;
    let context = projection_context(&profile, &effective, source_cutoff);
    let semantic_hash_before = projection.validated_canonical_hash(&context).unwrap();
    assert_eq!(
        semantic_hash_before.as_str(),
        "61cf0429a8dd042070f18cf426f37f27983d055b91d4033df3a8311a78e5a09e"
    );

    let wire = MetricContractDecisionProjectionWireV1::try_from_domain(&projection).unwrap();
    assert_eq!(wire.w, METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1);
    assert_eq!(wire.d.len(), 15);
    let expected_family_lengths = [7, 8, 4, 5, 7, 9, 5, 14, 8, 8];
    for (value, expected) in wire.d[5..].iter().zip(expected_family_lengths) {
        assert_eq!(value.as_array().unwrap().len(), expected);
    }
    assert!(wire.json_bytes().unwrap().contains(&b'n'));

    let roundtripped = wire.clone().try_into_domain().unwrap();
    assert_eq!(roundtripped, projection);
    assert_eq!(
        roundtripped.validated_canonical_hash(&context).unwrap(),
        semantic_hash_before
    );

    let tuple_layouts = metric_contract_projection_wire_v1_tuple_layouts();
    assert_eq!(tuple_layouts.len(), 18);
    let enum_tables = metric_contract_projection_wire_v1_mapping_tables();
    assert_eq!(enum_tables.len(), 28);
    for tables in [tuple_layouts, enum_tables] {
        let mut names = std::collections::BTreeSet::new();
        for (name, values) in tables {
            assert!(
                names.insert(*name),
                "duplicate Wire V1 mapping table {name}"
            );
            assert!(!values.is_empty(), "empty Wire V1 mapping table {name}");
            let mut entries = std::collections::BTreeSet::new();
            for value in *values {
                assert!(
                    entries.insert(*value),
                    "duplicate Wire V1 code in {name}: {value}"
                );
            }
        }
    }

    let bytes = wire.json_bytes().unwrap();
    let golden = blake3::hash(&bytes).to_hex().to_string();
    assert_eq!(
        golden,
        "be965cdbfabffc8690a256574334ddd628414d2423a24cd5e81900ec32f4b566"
    );
    let text = String::from_utf8(bytes).unwrap();
    for forbidden in ["owner_states", "events", "eligible_events", "wallet_states"] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn compact_json_wire_v1_rejects_version_shape_and_enum_drift() {
    let (complete, ..) = complete_snapshot_fixture();
    let projection = complete.compact_projection;
    let wire = MetricContractDecisionProjectionWireV1::try_from_domain(&projection).unwrap();

    let mut unsupported = wire.clone();
    unsupported.w = 2;
    let unsupported_value = serde_json::to_value(&unsupported).unwrap();
    assert!(matches!(
        unsupported.try_into_domain(),
        Err(MetricContractProjectionWireErrorV1::UnsupportedVersion(2))
    ));
    let mut unsupported_mfs = serde_json::to_value(MaterializedFeatureSet::default()).unwrap();
    unsupported_mfs.as_object_mut().unwrap().insert(
        "metric_contract_decision_projection_v1".to_string(),
        unsupported_value,
    );
    assert!(serde_json::from_value::<MaterializedFeatureSet>(unsupported_mfs).is_err());

    let mut missing_root_slot = wire.clone();
    missing_root_slot.d.pop();
    assert!(matches!(
        missing_root_slot.try_into_domain(),
        Err(MetricContractProjectionWireErrorV1::TupleLength {
            path: "projection root",
            expected: 15,
            actual: 14,
        })
    ));
    let mut extra_root_slot = wire.clone();
    extra_root_slot.d.push(serde_json::Value::Null);
    assert!(matches!(
        extra_root_slot.try_into_domain(),
        Err(MetricContractProjectionWireErrorV1::TupleLength {
            path: "projection root",
            expected: 15,
            actual: 16,
        })
    ));
    let mut wrong_family_length = wire.clone();
    wrong_family_length.d[5].as_array_mut().unwrap().pop();
    assert!(matches!(
        wrong_family_length.try_into_domain(),
        Err(MetricContractProjectionWireErrorV1::TupleLength {
            path: "family.ftdi",
            expected: 7,
            actual: 6,
        })
    ));
    let mut invalid_enum = wire.clone();
    invalid_enum.d[1] = serde_json::Value::from(255_u64);
    let invalid_enum_value = serde_json::to_value(&invalid_enum).unwrap();
    assert!(matches!(
        invalid_enum.try_into_domain(),
        Err(MetricContractProjectionWireErrorV1::InvalidEnumCode {
            kind: "rollout mode",
            code: 255,
        })
    ));
    let mut invalid_enum_mfs = serde_json::to_value(MaterializedFeatureSet::default()).unwrap();
    invalid_enum_mfs.as_object_mut().unwrap().insert(
        "metric_contract_decision_projection_v1".to_string(),
        invalid_enum_value,
    );
    assert!(serde_json::from_value::<MaterializedFeatureSet>(invalid_enum_mfs).is_err());

    let mut wire_value = serde_json::to_value(&wire).unwrap();
    wire_value
        .as_object_mut()
        .unwrap()
        .insert("extra".to_string(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<MetricContractDecisionProjectionWireV1>(wire_value).is_err());
    let mut missing_key = serde_json::to_value(&wire).unwrap();
    missing_key.as_object_mut().unwrap().remove("d");
    assert!(serde_json::from_value::<MetricContractDecisionProjectionWireV1>(missing_key).is_err());
}

#[test]
fn compact_json_wire_v1_roundtrips_every_typed_reason_code() {
    let (complete, ..) = complete_snapshot_fixture();
    let base = complete.compact_projection;
    for (table_name, details) in metric_contract_projection_wire_v1_mapping_tables()
        .iter()
        .filter(|(name, _)| name.starts_with("reason.") && *name != "reason_family")
    {
        let family = table_name.strip_prefix("reason.").unwrap();
        for detail in *details {
            let reason: MetricEvidenceReasonV1 = serde_json::from_value(serde_json::json!({
                "reason_family": family,
                "detail": detail,
            }))
            .unwrap_or_else(|error| panic!("invalid mapping {table_name}/{detail}: {error}"));
            let mut projection = base.clone();
            projection.flip_ratio.hybrid_v2_ratio.envelope.reasons.codes = vec![reason];
            let roundtripped = MetricContractDecisionProjectionWireV1::try_from_domain(&projection)
                .unwrap()
                .try_into_domain()
                .unwrap();
            assert_eq!(roundtripped, projection, "{table_name}/{detail}");
        }
    }

    let mut projection = base;
    projection.flip_ratio.hybrid_v2_ratio.envelope.reasons.codes =
        vec![MetricEvidenceReasonV1::UnmappedLegacyString {
            contract_id: MetricContractId::FlipRatio,
            raw: "full-unmapped-reason-text".to_string(),
        }];
    let roundtripped = MetricContractDecisionProjectionWireV1::try_from_domain(&projection)
        .unwrap()
        .try_into_domain()
        .unwrap();
    assert_eq!(roundtripped, projection);
}

#[test]
fn mfs_field_uses_only_wire_v1_and_preserves_nulls_reasons_and_semantic_hash() {
    let (complete, profile, effective, source_cutoff) = complete_snapshot_fixture();
    let projection = complete.compact_projection;
    let context = projection_context(&profile, &effective, source_cutoff);
    let semantic_hash = projection.validated_canonical_hash(&context).unwrap();
    let semantic_roundtrip = MetricContractDecisionProjectionWireV1::try_from_domain(&projection)
        .unwrap()
        .try_into_domain()
        .unwrap();
    assert_eq!(
        semantic_roundtrip
            .validated_canonical_hash(&context)
            .unwrap(),
        semantic_hash
    );

    let mut projection = projection;
    projection
        .flip_ratio
        .hybrid_v2_ratio
        .envelope
        .reasons
        .omitted_count = 7;
    let wire = MetricContractDecisionProjectionWireV1::try_from_domain(&projection).unwrap();
    let wire_bytes = wire.json_bytes().unwrap();
    assert!(wire_bytes.windows(4).any(|window| window == b"null"));

    let mfs = MaterializedFeatureSet {
        metric_contract_decision_projection_v1: Some(projection.clone()),
        ..MaterializedFeatureSet::default()
    };
    let mfs_json = serde_json::to_string(&mfs).unwrap();
    let marker = "\"metric_contract_decision_projection_v1\":";
    let field_start = mfs_json.rfind(marker).unwrap() + marker.len();
    let field_end = mfs_json.len() - 1;
    assert_eq!(&mfs_json.as_bytes()[field_start..field_end], wire_bytes);
    assert_eq!(
        wire_bytes.len(),
        projection.authoritative_serialized_size_bytes().unwrap()
    );
    let mfs_value = serde_json::to_value(&mfs).unwrap();
    let decoded: MaterializedFeatureSet = serde_json::from_value(mfs_value.clone()).unwrap();
    assert_eq!(
        decoded.metric_contract_decision_projection_v1.as_ref(),
        Some(&projection)
    );
    assert_eq!(
        decoded
            .metric_contract_decision_projection_v1
            .as_ref()
            .unwrap()
            .flip_ratio
            .hybrid_v2_ratio
            .envelope
            .reasons
            .omitted_count,
        7
    );

    let historical: MaterializedFeatureSet =
        serde_json::from_value(serde_json::to_value(MaterializedFeatureSet::default()).unwrap())
            .unwrap();
    assert!(historical.metric_contract_decision_projection_v1.is_none());

    let mut verbose_mfs = serde_json::to_value(MaterializedFeatureSet::default()).unwrap();
    verbose_mfs.as_object_mut().unwrap().insert(
        "metric_contract_decision_projection_v1".to_string(),
        serde_json::to_value(&projection).unwrap(),
    );
    assert!(serde_json::from_value::<MaterializedFeatureSet>(verbose_mfs).is_err());

    let mut null_mfs = serde_json::to_value(MaterializedFeatureSet::default()).unwrap();
    null_mfs.as_object_mut().unwrap().insert(
        "metric_contract_decision_projection_v1".to_string(),
        serde_json::Value::Null,
    );
    assert!(serde_json::from_value::<MaterializedFeatureSet>(null_mfs).is_err());
}

fn complete_snapshot_fixture() -> (
    ghost_launcher::metric_contracts::Pr2bCompleteMetricContractSnapshotV1,
    MetricContractProfileV1,
    ResolvedMetricContractEffectiveConfigV1,
    MetricContractDecisionSourceCutoffV1,
) {
    let (profile, effective, gatekeeper, tx_config, funding) = runtime_context();
    let context = build_context(&profile, &effective);
    let candidate = EnhancedCandidate {
        timestamp: 1_000,
        ..EnhancedCandidate::default()
    };
    let engine = TxIntelligenceEngine::new(tx_config.clone(), &candidate, None);
    let tx_snapshot = engine
        .metric_contract_snapshot(&ghost_core::tx_intelligence::types::TxIntelFeatures::default());
    let flip = engine.flip_v2_snapshot(10_000, Some(100));
    let ftdi = compute_ftdi(std::iter::empty::<&ghost_launcher::events::PoolTransaction>());
    let gatekeeper_dev = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
        amount_sol: None,
        creator_known: false,
        create_signature: None,
        create_signature_matched: false,
        selection_mode: DevBuySelectionModeV1::NoEligibleBuy,
        selected_signature: None,
        selected_slot: None,
        selected_transaction_index: None,
        eligible_buy_count: 0,
        selected_success: None,
    };
    let recent_exact = TxTimingProducerSnapshotV1 {
        numerator: 0,
        denominator: 0,
        ratio: None,
        canonical_dedupe_applied: true,
        dust_filter_sol: None,
        window_ms: Some(10_000),
        fallback_timestamp_count: 0,
        fallback_ordering_count: 0,
        source_complete: true,
        source_state_capacity: Some(
            u64::try_from(gatekeeper.decision_time_series_tx_capacity).unwrap(),
        ),
    };
    let index = FundingSourceIndex::new();
    let fsc = index.compute_for_transactions(
        std::iter::empty::<&ghost_launcher::events::PoolTransaction>(),
        &funding,
    );
    let funding_producer_config = funding
        .metric_contract_producer_config_snapshot(None)
        .unwrap();
    let reserve = AccountStateReserveVelocitySnapshotV1::default();
    let recent = RecentBuySellProducerSnapshotV1 {
        window_ms: 10_000,
        buy_count: 0,
        sell_count: 0,
        transaction_count: 0,
        failed_transaction_count: 0,
        source_complete: true,
    };
    let manipulation = frozen_manipulation(ManipulationContradictionFeatures::default());
    let complete = build_pr2b_complete_metric_contract_snapshot_v1(
        Pr2bFrozenProducerInputsV1 {
            pr2a: Pr2aFrozenProducerInputsV1 {
                ftdi: &ftdi,
                tx_intelligence: &tx_snapshot,
                gatekeeper_dev_primary: &gatekeeper_dev,
                recent_exact_timing: &recent_exact,
                fsc: &fsc,
                funding_source_config: &funding,
                funding_source_producer_config: &funding_producer_config,
            },
            legacy_flip_ratio: None,
            flip_v2: &flip,
            manipulation: &manipulation,
            reserve_velocity: &reserve,
            recent_buy_sell: &recent,
        },
        &context,
    )
    .unwrap();
    let source_cutoff = context.source_cutoff.clone();
    (complete, profile, effective, source_cutoff)
}

#[test]
fn full_evidence_and_projection_are_deterministic_views_of_one_frozen_input_set() {
    let (complete, profile, effective, source_cutoff) = complete_snapshot_fixture();
    let projection_context = projection_context(&profile, &effective, source_cutoff);
    let rebuilt = MetricContractDecisionEvidenceProjectionV1::try_from_evidence(
        &complete.full_evidence,
        &projection_context,
    )
    .unwrap();
    assert_eq!(rebuilt, complete.compact_projection);
    assert_eq!(
        rebuilt
            .validated_canonical_hash(&projection_context)
            .unwrap(),
        complete
            .compact_projection
            .validated_canonical_hash(&projection_context)
            .unwrap()
    );
}
