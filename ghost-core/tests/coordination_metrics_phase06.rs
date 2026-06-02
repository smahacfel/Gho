use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_core::features::coordination::{
    build_coordination_risk_evidence_unit, build_coordination_risk_evidence_unit_from_snapshot,
    compute_bse_v2, compute_cpv_v2, compute_cucd_v2, compute_dbia_v2, compute_des_v2,
    compute_ftdi_v2, compute_sfd_v2, funding_source_concentration_from_fsc_v2,
    CoordinationMetricBreakdowns, CoordinationMetricName, CoordinationRiskConfig,
    CoordinationRiskEvidenceInput, CoordinationRiskFeatures, CoordinationSnapshotMode,
    DegradedReason, DevFingerprintEvidence, DevFingerprintMode, EconomicSpend, EconomicSpendSource,
    ExecutionTemplateFingerprint, FeeTopologyFingerprint, FrozenCoordinationDecisionSnapshot,
    FundingVisibility, InfraFingerprint, MetricEvidenceStatus, MetricPolicyMode, ObservedBuyTx,
    SignerCrossPoolActivity, T0Source, TxTimeSource,
};
use ghost_core::tx_intelligence::types::{
    FscAttributionScope, FscEvidenceStatus, FscExcludedReason, FscSnapshotMode, FscV2Evidence,
    FscVersion,
};
use smallvec::smallvec;
use solana_sdk::{pubkey::Pubkey, signature::Signature};

fn pk(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn fee_fp(seed: u8) -> FeeTopologyFingerprint {
    FeeTopologyFingerprint {
        external_fee_count: seed,
        internal_fee_count: seed % 2,
        external_amount_pattern_hash: u16::from(seed) * 17,
        has_wsol_self_flow: seed % 2 == 0,
        has_create_ata_flow: seed % 3 == 0,
    }
}

fn exec_fp(seed: u8) -> ExecutionTemplateFingerprint {
    ExecutionTemplateFingerprint {
        compute_budget_shape: seed,
        outer_program_sequence_hash: u16::from(seed) * 10,
        inner_program_sequence_hash: u16::from(seed) * 11,
        inner_instruction_count_bucket: seed % 4,
        account_role_pattern_hash: u16::from(seed) * 12,
        fee_topology_hash: u16::from(seed) * 13,
        ata_wsol_shape: seed % 5,
    }
}

fn infra_fp(seed: u8) -> InfraFingerprint {
    let execution = exec_fp(seed);
    InfraFingerprint {
        account_role_pattern_hash: execution.account_role_pattern_hash,
        outer_program_sequence_hash: execution.outer_program_sequence_hash,
        inner_program_sequence_hash: execution.inner_program_sequence_hash,
        outer_ix_count_bucket: 2,
        inner_ix_group_count_bucket: 1,
        compute_budget_shape: execution.compute_budget_shape,
        fee_topology_hash: execution.fee_topology_hash,
        ata_wsol_shape: execution.ata_wsol_shape,
    }
}

fn observed_buy_tx(signer_seed: u8, slot: u64, slot_index: u64) -> ObservedBuyTx {
    ObservedBuyTx {
        signature: Signature::new_unique(),
        pool_id: pk(1),
        mint: pk(2),
        signer: pk(signer_seed),
        slot,
        slot_index: Some(slot_index),
        tx_elapsed_ms_from_pool_create: Some(100),
        t0_source: Some(T0Source::ReplayFixture),
        tx_time_source: Some(TxTimeSource::ReplayFixture),
        is_success: true,
        is_buy: true,
        is_sell: false,
        is_dev: false,
        is_create_or_init_tx: false,
        is_unknown_direction: false,
        account_keys_resolved: smallvec![pk(signer_seed), pk(3)],
        outer_ix_count: Some(2),
        inner_ix_group_count: Some(1),
        fee_lamports: Some(5_000),
        pre_balance_signer: Some(1_000_000),
        post_balance_signer: Some(900_000),
        decoded_buy_sol_lamports: Some(100_000),
        curve_sol_delta_lamports: Some(100_000),
        economic_spent_lamports: Some(EconomicSpend {
            lamports: 100_000,
            source: EconomicSpendSource::DecodedPumpInstruction,
            confidence: 0.95,
        }),
        tokens_received: Some(42),
        price_before: Some(1.0),
        price_after: Some(1.1),
        compute_units_consumed: Some(50_000),
        cost_units: Some(50_000),
        fee_topology_fp: Some(fee_fp(1)),
        execution_template_fp: Some(exec_fp(1)),
        capital_template_fp: None,
    }
}

fn fsc_v2_evidence(
    excluded_reason: Option<FscExcludedReason>,
    capture_ready: bool,
    index_warm: bool,
    gap_suspected: bool,
) -> FscV2Evidence {
    FscV2Evidence {
        version: FscVersion::V2,
        attribution_scope: FscAttributionScope::SingleHopNativeSol,
        snapshot_mode: FscSnapshotMode::DecisionTime,
        total_buyers: 4,
        known_buyers: 4,
        known_non_neutral_buyers: 2,
        unknown_count: 0,
        neutral_count: 2,
        low_confidence_count: 0,
        same_slot_unorderable_count: 0,
        known_coverage: 1.0,
        non_neutral_known_coverage: 0.5,
        neutral_share: 0.5,
        top1_share_count: None,
        top1_share_sol: None,
        hhi_norm_count: None,
        hhi_norm_sol_weighted_excess: None,
        raw_hhi_including_neutral: None,
        scoring_hhi_non_neutral: None,
        top_funder: None,
        top_funder_count: 0,
        top_funder_buy_sol: 0.0,
        source_counts: Vec::new(),
        attribution_confidence_mean: None,
        attribution_confidence_min: None,
        dust_filtered_count: 0,
        post_buy_filtered_count: 0,
        rel_too_small_count: 0,
        index_warm,
        capture_ready,
        status: if excluded_reason.is_none() {
            FscEvidenceStatus::Clean
        } else {
            FscEvidenceStatus::Degraded
        },
        excluded_reason,
        funding_lane_watermark_slot: Some(10),
        max_buy_slot: Some(10),
        funding_lane_lag_slots: Some(0),
        stream_epoch: 1,
        gap_suspected,
        last_transfer_recv_ts_ms: Some(1_000),
        last_reconnect_ts_ms: None,
        dropped_events: 0,
        min_abs_store_lamports: 1,
        min_abs_attribution_lamports: 1,
        min_rel_to_buy: 0.0,
        ttl_seconds: 60,
        neutral_funder_set_version: None,
        neutral_funder_set_hash: None,
        config_hash: "cfg".to_string(),
        provider: "test".to_string(),
        source_topics: Vec::new(),
    }
}

#[test]
fn ftdi_v2_uses_hhi_diversity_and_keeps_export_only_policy() {
    let config = CoordinationRiskConfig::default();
    let mut txs = Vec::new();
    for signer_seed in 10..14 {
        let mut tx = observed_buy_tx(signer_seed, u64::from(signer_seed), 0);
        tx.fee_topology_fp = Some(fee_fp(1));
        txs.push(tx);
    }
    let mut minority = observed_buy_tx(14, 14, 0);
    minority.fee_topology_fp = Some(fee_fp(2));
    txs.push(minority);

    let result = compute_ftdi_v2(&txs, &config);

    let value = result.value.expect("full-coverage FTDI should compute");
    assert_approx_eq(value.value, 0.4);
    assert_eq!(value.status, MetricEvidenceStatus::Clean);
    assert_eq!(result.evidence.evidence_status, MetricEvidenceStatus::Clean);
    assert_eq!(result.evidence.policy_mode, MetricPolicyMode::ExportOnly);
    assert!(!result.evidence.score_eligible);
    assert_eq!(result.evidence.breakdown.topology_counts.len(), 2);
}

#[test]
fn ftdi_v2_does_not_compute_clean_value_on_partial_fingerprint_sample() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(20, 20, 0),
        observed_buy_tx(21, 21, 0),
        observed_buy_tx(22, 22, 0),
    ];
    txs[2].fee_topology_fp = None;

    let result = compute_ftdi_v2(&txs, &config);

    assert!(result.value.is_none());
    assert_eq!(
        result.evidence.evidence_status,
        MetricEvidenceStatus::Degraded
    );
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::LowCoverage));
    assert_approx_eq(result.evidence.breakdown.fingerprint_coverage, 2.0 / 3.0);
}

#[test]
fn dbia_v2_requires_comparable_dev_reference_and_never_uses_create_tx_as_buy() {
    let config = CoordinationRiskConfig::default();
    let txs = vec![
        observed_buy_tx(30, 30, 0),
        observed_buy_tx(31, 31, 0),
        observed_buy_tx(32, 32, 0),
    ];
    let buyers: Vec<&ObservedBuyTx> = txs.iter().collect();

    let not_comparable = compute_dbia_v2(
        &buyers,
        Some(DevFingerprintEvidence {
            mode: DevFingerprintMode::NotComparable,
            fingerprint: None,
            explicit_swap_slice: false,
        }),
        &config,
    );
    assert!(not_comparable.value.is_none());
    assert!(not_comparable
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::DevTxNotComparable));

    let comparable = compute_dbia_v2(
        &buyers,
        Some(DevFingerprintEvidence {
            mode: DevFingerprintMode::ComparablePureBuy,
            fingerprint: Some(infra_fp(1)),
            explicit_swap_slice: false,
        }),
        &config,
    );
    let value = comparable
        .value
        .expect("matching buyer/dev fingerprints should compute");
    assert_approx_eq(value.value, 1.0);
    assert_eq!(
        comparable.evidence.evidence_status,
        MetricEvidenceStatus::Clean
    );
    assert_eq!(
        comparable.evidence.policy_mode,
        MetricPolicyMode::ExportOnly
    );
    assert_approx_eq(value.confidence, 1.0);
}

#[test]
fn dbia_v2_requires_explicit_swap_slice_and_downweights_its_confidence() {
    let config = CoordinationRiskConfig::default();
    let txs = vec![
        observed_buy_tx(34, 34, 0),
        observed_buy_tx(35, 35, 0),
        observed_buy_tx(36, 36, 0),
    ];
    let buyers: Vec<&ObservedBuyTx> = txs.iter().collect();

    let implicit_slice = compute_dbia_v2(
        &buyers,
        Some(DevFingerprintEvidence {
            mode: DevFingerprintMode::CreateTxSwapSliceOnly,
            fingerprint: Some(infra_fp(1)),
            explicit_swap_slice: false,
        }),
        &config,
    );
    assert!(implicit_slice.value.is_none());
    assert!(implicit_slice
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::DevTxNotComparable));

    let explicit_slice = compute_dbia_v2(
        &buyers,
        Some(DevFingerprintEvidence {
            mode: DevFingerprintMode::CreateTxSwapSliceOnly,
            fingerprint: Some(infra_fp(1)),
            explicit_swap_slice: true,
        }),
        &config,
    );
    let value = explicit_slice
        .value
        .expect("explicit swap-slice dev reference should compute");
    assert_approx_eq(value.value, 1.0);
    assert_approx_eq(value.confidence, 0.6);
}

#[test]
fn sfd_v2_uses_economic_spend_primary_and_marks_signer_delta_fallback_degraded() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(40, 40, 0),
        observed_buy_tx(41, 41, 0),
        observed_buy_tx(42, 42, 0),
    ];
    txs[0].economic_spent_lamports = Some(EconomicSpend {
        lamports: 100_000,
        source: EconomicSpendSource::DecodedPumpInstruction,
        confidence: 0.95,
    });
    txs[1].economic_spent_lamports = Some(EconomicSpend {
        lamports: 150_000,
        source: EconomicSpendSource::CurveRealSolDelta,
        confidence: 0.85,
    });
    txs[2].economic_spent_lamports = None;
    txs[2].decoded_buy_sol_lamports = None;
    txs[2].curve_sol_delta_lamports = None;
    txs[2].post_balance_signer = Some(875_000);

    let result = compute_sfd_v2(&txs, &config);

    let value = result
        .value
        .expect("fallback signer delta still provides degraded evidence");
    assert_eq!(value.status, MetricEvidenceStatus::Degraded);
    assert!(value
        .degraded_reasons
        .contains(&DegradedReason::MissingEconomicSpend));
    assert_eq!(
        result.evidence.evidence_status,
        MetricEvidenceStatus::Degraded
    );
    assert_eq!(result.evidence.breakdown.source_counts.len(), 3);
}

#[test]
fn sfd_v2_uses_decoded_then_curve_before_signer_delta_fallback() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(43, 43, 0),
        observed_buy_tx(44, 44, 0),
        observed_buy_tx(45, 45, 0),
    ];
    txs[0].economic_spent_lamports = None;
    txs[0].decoded_buy_sol_lamports = Some(120_000);
    txs[0].curve_sol_delta_lamports = Some(130_000);
    txs[1].economic_spent_lamports = None;
    txs[1].decoded_buy_sol_lamports = None;
    txs[1].curve_sol_delta_lamports = Some(140_000);
    txs[2].economic_spent_lamports = None;
    txs[2].decoded_buy_sol_lamports = None;
    txs[2].curve_sol_delta_lamports = None;
    txs[2].fee_lamports = Some(5_000);
    txs[2].post_balance_signer = Some(850_000);

    let result = compute_sfd_v2(&txs, &config);

    let value = result.value.expect("all spend sources should compute");
    assert_eq!(value.status, MetricEvidenceStatus::Degraded);
    assert!(value
        .degraded_reasons
        .contains(&DegradedReason::MissingEconomicSpend));
    assert_eq!(
        result.evidence.breakdown.spend_fractions.as_slice()[0],
        0.12
    );
    assert_eq!(
        result.evidence.breakdown.spend_fractions.as_slice()[1],
        0.14
    );
    assert_eq!(
        result.evidence.breakdown.spend_fractions.as_slice()[2],
        0.145
    );
}

#[test]
fn sfd_v2_fail_closes_zero_balance_and_outlier_fraction() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(46, 46, 0),
        observed_buy_tx(47, 47, 0),
        observed_buy_tx(48, 48, 0),
    ];
    txs[0].pre_balance_signer = Some(0);
    txs[1].economic_spent_lamports = Some(EconomicSpend {
        lamports: 2_000_000,
        source: EconomicSpendSource::DecodedPumpInstruction,
        confidence: 0.95,
    });
    txs[2].economic_spent_lamports = None;
    txs[2].decoded_buy_sol_lamports = None;
    txs[2].curve_sol_delta_lamports = None;

    let result = compute_sfd_v2(&txs, &config);

    assert!(result.value.is_none());
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::MissingPrePostBalances));
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::SpendFractionOutOfRange));
}

#[test]
fn cpv_v2_averages_intensity_so_one_active_signer_does_not_dominate() {
    let config = CoordinationRiskConfig::default();
    let txs = vec![
        observed_buy_tx(50, 50, 0),
        observed_buy_tx(51, 51, 0),
        observed_buy_tx(52, 52, 0),
    ];
    let activity = [SignerCrossPoolActivity {
        signer: pk(50),
        other_pool_count: 3,
        current_pool_excluded: true,
        feature_cutoff_slot: Some(52),
        observed_until_slot: Some(52),
    }];

    let result = compute_cpv_v2(&txs, &activity, true, &config);

    let value = result.value.expect("warm CPV index should compute");
    assert_approx_eq(value.value, 1.0 / 3.0);
    assert_eq!(result.evidence.policy_mode, MetricPolicyMode::ExportOnly);
    assert_eq!(result.evidence.breakdown.signer_intensities.len(), 3);
}

#[test]
fn cpv_v2_rejects_activity_without_cutoff_or_current_pool_exclusion_proof() {
    let config = CoordinationRiskConfig::default();
    let txs = vec![
        observed_buy_tx(53, 53, 0),
        observed_buy_tx(54, 54, 0),
        observed_buy_tx(55, 55, 0),
    ];
    let activity = [SignerCrossPoolActivity {
        signer: pk(53),
        other_pool_count: 3,
        current_pool_excluded: false,
        feature_cutoff_slot: Some(55),
        observed_until_slot: Some(56),
    }];

    let result = compute_cpv_v2(&txs, &activity, true, &config);

    assert!(result.value.is_none());
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::CurrentPoolNotExcluded));
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::ActivityAfterCutoff));
}

#[test]
fn des_and_bse_require_slot_index_sequence_and_minimum_tau_pairs() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(60, 10, 0),
        observed_buy_tx(61, 11, 0),
        observed_buy_tx(62, 13, 0),
        observed_buy_tx(63, 16, 0),
    ];
    for (idx, tx) in txs.iter_mut().enumerate() {
        tx.price_before = Some(1.0);
        tx.price_after = Some(1.0 + ((idx + 1) as f64 * 0.1));
        tx.economic_spent_lamports = Some(EconomicSpend {
            lamports: ((idx + 1) as u64) * 100_000,
            source: EconomicSpendSource::DecodedPumpInstruction,
            confidence: 0.95,
        });
    }

    let des = compute_des_v2(&txs, &config);
    let des_value = des.value.expect("DES should compute with 3 eligible pairs");
    assert_approx_eq(des_value.value, 1.0);
    assert_eq!(des.evidence.evidence_status, MetricEvidenceStatus::Clean);

    let bse = compute_bse_v2(&txs, &config);
    let bse_value = bse.value.expect("BSE should compute with 4 eligible txs");
    assert_approx_eq(bse_value.value, 1.0);
    assert_approx_eq(bse.evidence.breakdown.tau_b_raw.unwrap(), 1.0);
    assert_approx_eq(bse.evidence.breakdown.tau_b_abs.unwrap(), 1.0);

    txs[0].slot_index = None;
    let missing_slot = compute_des_v2(&txs, &config);
    assert!(missing_slot.value.is_none());
    assert!(missing_slot
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::MissingSlotIndex));
}

#[test]
fn bse_v2_uses_current_impact_against_next_buy_size_and_requires_three_pairs() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(64, 64, 0),
        observed_buy_tx(65, 65, 0),
        observed_buy_tx(66, 66, 0),
    ];
    let too_short = compute_bse_v2(&txs, &config);
    assert!(too_short.value.is_none());
    assert_eq!(
        too_short.evidence.evidence_status,
        MetricEvidenceStatus::InsufficientSample
    );

    txs.push(observed_buy_tx(67, 67, 0));
    let impacts = [0.1, 0.2, 0.3, 0.4];
    let spends = [10_000, 400_000, 300_000, 200_000];
    for (idx, tx) in txs.iter_mut().enumerate() {
        tx.price_before = Some(1.0);
        tx.price_after = Some(1.0 + impacts[idx]);
        tx.economic_spent_lamports = Some(EconomicSpend {
            lamports: spends[idx],
            source: EconomicSpendSource::DecodedPumpInstruction,
            confidence: 0.95,
        });
    }

    let result = compute_bse_v2(&txs, &config);

    let value = result
        .value
        .expect("BSE should compute from three j -> j+1 pairs");
    assert_approx_eq(value.value, 1.0);
    assert_approx_eq(result.evidence.breakdown.tau_b_raw.unwrap(), -1.0);
    assert_approx_eq(result.evidence.breakdown.tau_b_abs.unwrap(), 1.0);
}

#[test]
fn des_v2_marks_same_slot_dominated_and_invalid_price_as_degraded() {
    let config = CoordinationRiskConfig::default();
    let mut txs = vec![
        observed_buy_tx(68, 70, 0),
        observed_buy_tx(69, 70, 1),
        observed_buy_tx(70, 70, 2),
        observed_buy_tx(71, 71, 0),
    ];
    txs[1].price_before = Some(0.0);

    let result = compute_des_v2(&txs, &config);

    assert!(result.value.is_none());
    assert!(result
        .evidence
        .degraded_reasons
        .contains(&DegradedReason::SameSlotDominated));
}

#[test]
fn cucd_v2_reports_low_robust_cv_as_homogeneous_not_positive_organic_evidence() {
    let config = CoordinationRiskConfig::default();
    let txs = vec![
        observed_buy_tx(70, 70, 0),
        observed_buy_tx(71, 71, 0),
        observed_buy_tx(72, 72, 0),
    ];

    let result = compute_cucd_v2(&txs, &config);

    let value = result
        .value
        .expect("CUCD should compute for full CU sample");
    assert_approx_eq(value.value, 0.0);
    assert_approx_eq(value.severity, 1.0);
    assert_eq!(result.evidence.evidence_status, MetricEvidenceStatus::Clean);
    assert_approx_eq(
        result.evidence.breakdown.dominant_bucket_share.unwrap(),
        1.0,
    );
    assert_approx_eq(result.evidence.breakdown.cu_bucket_hhi_norm.unwrap(), 1.0);
}

#[test]
fn evidence_unit_requires_frozen_decision_time_snapshot_and_keeps_sidecar_join_schema() {
    let input = CoordinationRiskEvidenceInput {
        schema_version: 1,
        scope_id: "phase06-test".to_string(),
        run_id: Some("run-1".to_string()),
        candidate_id: Some("candidate-1".to_string()),
        pool_id: pk(1),
        mint: pk(2),
        decision_id: Some("decision-1".to_string()),
        decision_ts_ms: 1_000,
        decision_slot: Some(10),
        snapshot_mode: CoordinationSnapshotMode::EventualPostfill,
        snapshot_available: false,
        feature_cutoff_ts_ms: 999,
        feature_cutoff_slot: Some(9),
        source_buffer_watermark_slot: Some(9),
        computed_at_recv_ts_ns: 1_000_000,
        gatekeeper_version: Some("v2.5".to_string()),
        source_snapshot_hash: Some("hash".to_string()),
        sample_summary: Default::default(),
        funding_visibility: FundingVisibility::from_lane_health(true, true, false),
        features: CoordinationRiskFeatures {
            funding_visibility: FundingVisibility::Available,
            fee_topology_diversity_index: Some(
                ghost_core::features::coordination::MetricValue::new(
                    0.4,
                    0.0,
                    1.0,
                    5,
                    1.0,
                    MetricEvidenceStatus::Clean,
                ),
            ),
            ..CoordinationRiskFeatures::default()
        },
        metric_breakdowns: CoordinationMetricBreakdowns::default(),
    };

    let unit = build_coordination_risk_evidence_unit(input);

    assert_eq!(unit.scope_id, "phase06-test");
    assert_eq!(
        unit.snapshot_mode,
        CoordinationSnapshotMode::EventualPostfill
    );
    assert_eq!(unit.features.fee_topology_diversity_index, None);
    assert!(unit
        .degraded_reasons
        .contains(&DegradedReason::MissingDecisionSnapshot));
    assert!(unit
        .degraded_reasons
        .contains(&DegradedReason::MissingFrozenBuffer));
    let skipped: Vec<CoordinationMetricName> = unit
        .metric_breakdowns
        .skipped_metrics
        .iter()
        .map(|entry| entry.metric)
        .collect();
    assert_eq!(
        skipped,
        vec![
            CoordinationMetricName::CapitalTemplateConcentration,
            CoordinationMetricName::CrossPoolCohortRecurrence,
            CoordinationMetricName::ExecutionTemplateConcentration,
        ]
    );

    let encoded = serde_json::to_string(&unit).expect("evidence unit should serialize");
    assert!(encoded.contains("feature_cutoff_ts_ms"));
    assert!(encoded.contains("source_buffer_watermark_slot"));
    assert!(encoded.contains("source_snapshot_hash"));
    assert!(encoded.contains("metric_breakdowns"));
    assert!(encoded.contains("skipped_metrics"));
}

#[test]
fn evidence_unit_sanitizes_penalties_and_skips_proxy_stack_even_for_valid_snapshot() {
    let input = CoordinationRiskEvidenceInput {
        schema_version: 1,
        scope_id: "phase06-valid".to_string(),
        run_id: None,
        candidate_id: None,
        pool_id: pk(1),
        mint: pk(2),
        decision_id: Some("decision-2".to_string()),
        decision_ts_ms: 1_000,
        decision_slot: Some(10),
        snapshot_mode: CoordinationSnapshotMode::DecisionTime,
        snapshot_available: true,
        feature_cutoff_ts_ms: 999,
        feature_cutoff_slot: Some(9),
        source_buffer_watermark_slot: Some(9),
        computed_at_recv_ts_ns: 1_000_000,
        gatekeeper_version: Some("v2.5".to_string()),
        source_snapshot_hash: Some("hash".to_string()),
        sample_summary: Default::default(),
        funding_visibility: FundingVisibility::Available,
        features: CoordinationRiskFeatures {
            funding_visibility: FundingVisibility::Available,
            total_coordination_penalty: Some(1.0),
            interaction_penalty: Some(1.0),
            ..CoordinationRiskFeatures::default()
        },
        metric_breakdowns: CoordinationMetricBreakdowns::default(),
    };

    let unit = build_coordination_risk_evidence_unit(input);

    assert_eq!(unit.features.total_coordination_penalty, None);
    assert_eq!(unit.features.interaction_penalty, None);
    assert_eq!(unit.skipped_metrics.len(), 3);
    assert_eq!(unit.metric_breakdowns.skipped_metrics.len(), 3);
}

#[test]
fn evidence_unit_requires_source_snapshot_hash_for_frozen_snapshot_proof() {
    let input = CoordinationRiskEvidenceInput {
        schema_version: 1,
        scope_id: "phase06-missing-hash".to_string(),
        run_id: None,
        candidate_id: None,
        pool_id: pk(1),
        mint: pk(2),
        decision_id: Some("decision-3".to_string()),
        decision_ts_ms: 1_000,
        decision_slot: Some(10),
        snapshot_mode: CoordinationSnapshotMode::DecisionTime,
        snapshot_available: true,
        feature_cutoff_ts_ms: 999,
        feature_cutoff_slot: Some(9),
        source_buffer_watermark_slot: Some(9),
        computed_at_recv_ts_ns: 1_000_000,
        gatekeeper_version: Some("v2.5".to_string()),
        source_snapshot_hash: None,
        sample_summary: Default::default(),
        funding_visibility: FundingVisibility::Available,
        features: CoordinationRiskFeatures {
            funding_visibility: FundingVisibility::Available,
            fee_topology_diversity_index: Some(
                ghost_core::features::coordination::MetricValue::new(
                    0.4,
                    0.0,
                    1.0,
                    5,
                    1.0,
                    MetricEvidenceStatus::Clean,
                ),
            ),
            ..CoordinationRiskFeatures::default()
        },
        metric_breakdowns: CoordinationMetricBreakdowns::default(),
    };

    let unit = build_coordination_risk_evidence_unit(input);

    assert_eq!(unit.features.fee_topology_diversity_index, None);
    assert!(unit
        .degraded_reasons
        .contains(&DegradedReason::MissingFrozenBuffer));
}

#[test]
fn evidence_unit_rejects_cutoff_or_watermark_after_decision() {
    let base_input = CoordinationRiskEvidenceInput {
        schema_version: 1,
        scope_id: "phase06-time-bounds".to_string(),
        run_id: None,
        candidate_id: None,
        pool_id: pk(1),
        mint: pk(2),
        decision_id: Some("decision-4".to_string()),
        decision_ts_ms: 1_000,
        decision_slot: Some(10),
        snapshot_mode: CoordinationSnapshotMode::DecisionTime,
        snapshot_available: true,
        feature_cutoff_ts_ms: 1_001,
        feature_cutoff_slot: Some(10),
        source_buffer_watermark_slot: Some(10),
        computed_at_recv_ts_ns: 1_000_000,
        gatekeeper_version: Some("v2.5".to_string()),
        source_snapshot_hash: Some("hash".to_string()),
        sample_summary: Default::default(),
        funding_visibility: FundingVisibility::Available,
        features: CoordinationRiskFeatures {
            funding_visibility: FundingVisibility::Available,
            fee_topology_diversity_index: Some(
                ghost_core::features::coordination::MetricValue::new(
                    0.4,
                    0.0,
                    1.0,
                    5,
                    1.0,
                    MetricEvidenceStatus::Clean,
                ),
            ),
            ..CoordinationRiskFeatures::default()
        },
        metric_breakdowns: CoordinationMetricBreakdowns::default(),
    };

    let cutoff_after_decision = build_coordination_risk_evidence_unit(base_input.clone());
    assert_eq!(
        cutoff_after_decision.features.fee_topology_diversity_index,
        None
    );
    assert!(cutoff_after_decision
        .degraded_reasons
        .contains(&DegradedReason::FeatureCutoffAfterDecision));

    let watermark_after_decision =
        build_coordination_risk_evidence_unit(CoordinationRiskEvidenceInput {
            feature_cutoff_ts_ms: 999,
            feature_cutoff_slot: Some(9),
            source_buffer_watermark_slot: Some(11),
            ..base_input
        });
    assert_eq!(
        watermark_after_decision
            .features
            .fee_topology_diversity_index,
        None
    );
    assert!(watermark_after_decision
        .degraded_reasons
        .contains(&DegradedReason::SourceWatermarkAfterDecision));
}

#[test]
fn funding_visibility_maps_fsc_v2_lane_health_not_metric_quality() {
    let low_coverage_or_neutral_only = fsc_v2_evidence(
        Some(FscExcludedReason::InsufficientNonNeutralSupport),
        true,
        true,
        false,
    );
    assert_eq!(
        FundingVisibility::from_fsc_v2_lane_health(Some(&low_coverage_or_neutral_only)),
        FundingVisibility::Available
    );

    let index_cold = fsc_v2_evidence(Some(FscExcludedReason::IndexCold), false, false, false);
    assert_eq!(
        FundingVisibility::from_fsc_v2_lane_health(Some(&index_cold)),
        FundingVisibility::Warmup
    );

    let gap = fsc_v2_evidence(None, true, true, true);
    assert_eq!(
        FundingVisibility::from_fsc_v2_lane_health(Some(&gap)),
        FundingVisibility::GapSuspected
    );

    let lane_unavailable = fsc_v2_evidence(
        Some(FscExcludedReason::FundingLaneUnavailable),
        false,
        false,
        false,
    );
    assert_eq!(
        FundingVisibility::from_fsc_v2_lane_health(Some(&lane_unavailable)),
        FundingVisibility::Unavailable
    );
}

#[test]
fn fsc_v2_metric_exports_only_clean_decision_time_hhi() {
    let config = CoordinationRiskConfig::default();
    let mut clean = fsc_v2_evidence(None, true, true, false);
    clean.status = FscEvidenceStatus::Clean;
    clean.hhi_norm_count = Some(0.75);
    clean.non_neutral_known_coverage = 0.9;

    let value = funding_source_concentration_from_fsc_v2(Some(&clean), &config)
        .expect("clean decision-time FSC v2 should export metric");
    assert_approx_eq(value.value, 0.75);

    let mut neutral_only = clean.clone();
    neutral_only.status = FscEvidenceStatus::Degraded;
    neutral_only.excluded_reason = Some(FscExcludedReason::NeutralOnly);
    neutral_only.hhi_norm_count = None;
    assert_eq!(
        funding_source_concentration_from_fsc_v2(Some(&neutral_only), &config),
        None
    );

    let mut eventual = clean;
    eventual.snapshot_mode = FscSnapshotMode::EventualPostfill;
    assert_eq!(
        funding_source_concentration_from_fsc_v2(Some(&eventual), &config),
        None
    );
}

#[test]
fn evidence_unit_from_snapshot_computes_metrics_from_frozen_input() {
    let config = CoordinationRiskConfig::default();
    let txs = smallvec![
        observed_buy_tx(80, 80, 0),
        observed_buy_tx(81, 81, 0),
        observed_buy_tx(82, 82, 0),
        observed_buy_tx(83, 83, 0),
    ];
    let snapshot = FrozenCoordinationDecisionSnapshot {
        schema_version: 1,
        scope_id: "phase06-snapshot".to_string(),
        run_id: Some("run".to_string()),
        candidate_id: Some("candidate".to_string()),
        pool_id: pk(1),
        mint: pk(2),
        decision_id: Some("decision".to_string()),
        decision_ts_ms: 1_000,
        decision_slot: Some(80),
        snapshot_mode: CoordinationSnapshotMode::DecisionTime,
        feature_cutoff_ts_ms: 999,
        feature_cutoff_slot: Some(80),
        source_buffer_watermark_slot: Some(80),
        computed_at_recv_ts_ns: 1_000_000,
        gatekeeper_version: Some("v2.5".to_string()),
        source_snapshot_hash: Some("snapshot-hash".to_string()),
        txs,
        dev_reference: Some(DevFingerprintEvidence {
            mode: DevFingerprintMode::ComparablePureBuy,
            fingerprint: Some(infra_fp(1)),
            explicit_swap_slice: false,
        }),
        signer_activity: smallvec![SignerCrossPoolActivity {
            signer: pk(80),
            other_pool_count: 1,
            current_pool_excluded: true,
            feature_cutoff_slot: Some(80),
            observed_until_slot: Some(80),
        }],
        rolling_state_ready: true,
        fsc_v2: None,
    };

    let unit = build_coordination_risk_evidence_unit_from_snapshot(snapshot, &config);

    assert!(unit.degraded_reasons.is_empty());
    assert!(unit.features.fee_topology_diversity_index.is_some());
    assert!(unit
        .metric_breakdowns
        .fee_topology_diversity_index
        .is_some());
    assert!(unit
        .features
        .degraded_reasons
        .contains(&DegradedReason::FundingLaneUnavailable));
    assert_eq!(unit.skipped_metrics.len(), 3);
}

#[test]
fn phase06_guard_does_not_extend_materialized_feature_set_or_emit_penalty_payload() {
    let coordination_features = CoordinationRiskFeatures::default();
    assert_eq!(coordination_features.total_coordination_penalty, None);
    assert_eq!(coordination_features.interaction_penalty, None);

    let materialized =
        serde_json::to_value(MaterializedFeatureSet::default()).expect("MFS should serialize");
    assert!(materialized.get("coordination_risk").is_none());
    assert!(materialized.get("total_coordination_penalty").is_none());
    assert!(materialized.get("interaction_penalty").is_none());
}

fn assert_approx_eq(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual}, expected={expected}"
    );
}
