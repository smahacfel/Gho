use ghost_brain::config::{FscV2Config, GatekeeperV2Config, MetricContractFoundationConfigV1};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::metric_contracts::*;
use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata};
use ghost_launcher::components::gatekeeper::GatekeeperDevPrimaryCompatibilitySnapshotV1;
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::metric_contracts::{
    build_dev_buy_evidence_v1, build_fsc_status_evidence_v1, build_ftdi_evidence_v1,
    build_funding_evidence_v1, build_top3_evidence_v1, build_tx_timing_evidence_v1,
    resolve_metric_contract_effective_config_v1, Pr2aEvidenceBuildContextV1, Pr2aProducerErrorV1,
};
use ghost_launcher::session::{OpenSessionRequest, SessionConfig, SessionManager};
use ghost_launcher::tx_intelligence::{
    compute_ftdi, FundingSourceConfig, FundingSourceIndex, FundingSourceProducerConfigSnapshotV1,
    TxIntelligenceConfig, TxIntelligenceEngine, TxTimingProducerSnapshotV1,
};
use seer::early_fingerprint::EarlyFingerprintConfig;
use seer::types::ToolchainFingerprintInput;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::sync::Arc;

fn tx(
    signer: Pubkey,
    signature: Signature,
    timestamp_ms: u64,
    volume_sol: f64,
    is_buy: bool,
    success: bool,
    topology: Option<(u32, u32)>,
) -> PoolTransaction {
    let mut toolchain_fingerprint = ToolchainFingerprintInput::default();
    if let Some((external, internal)) = topology {
        toolchain_fingerprint.external_fee_transfer_count = Some(external);
        toolchain_fingerprint.internal_fee_transfer_count = Some(internal);
    }
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: Pubkey::new_unique().to_string(),
        slot: Some(timestamp_ms / 10 + 1),
        event_ordinal: Some((timestamp_ms % 10) as u32),
        tx_index: Some((timestamp_ms % 10) as u32),
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
        signer: signer.to_string(),
        is_buy,
        volume_sol,
        sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: signature.to_string(),
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
        toolchain_fingerprint,
        curve_data_known: false,
        curve_finality: CurveFinality::Speculative,
    }
}

fn runtime_contract_context() -> (
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
    let resolved = resolve_metric_contract_effective_config_v1(
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
        resolved,
        gatekeeper,
        tx_config,
        funding,
    )
}

fn funding_producer_config(funding: &FundingSourceConfig) -> FundingSourceProducerConfigSnapshotV1 {
    funding
        .metric_contract_producer_config_snapshot(None)
        .unwrap()
}

fn fsc_runtime_contract_context(
    gatekeeper: &GatekeeperV2Config,
    fsc_v2: &FscV2Config,
) -> (
    MetricContractProfileV1,
    ResolvedMetricContractEffectiveConfigV1,
    FundingSourceConfig,
    FundingSourceProducerConfigSnapshotV1,
) {
    let fingerprint = EarlyFingerprintConfig::default();
    let tx_config = TxIntelligenceConfig::from_gatekeeper_config(gatekeeper, fingerprint.clone());
    let funding = FundingSourceConfig::from_configs(gatekeeper, Some(fsc_v2));
    let producer_config = funding
        .metric_contract_producer_config_snapshot(Some(fsc_v2))
        .unwrap();
    let resolved = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        gatekeeper,
        &tx_config,
        &fingerprint,
        &funding,
        Some(fsc_v2),
    )
    .unwrap();
    (
        MetricContractProfileV1::profile_a().unwrap(),
        resolved,
        funding,
        producer_config,
    )
}

fn rehashed_runtime_config_with_value(
    resolved: &ResolvedMetricContractEffectiveConfigV1,
    key: MetricEffectiveConfigKeyV1,
    value: MetricEffectiveConfigValueV1,
) -> ResolvedMetricContractEffectiveConfigV1 {
    let mut payload = resolved.payload.clone();
    payload
        .entries
        .iter_mut()
        .find(|entry| entry.key == key)
        .unwrap()
        .value = value;
    ResolvedMetricContractEffectiveConfigV1::try_from_payload(payload).unwrap()
}

fn recent_exact_snapshot_for_same_timestamp(successes: &[bool]) -> TxTimingProducerSnapshotV1 {
    let manager = SessionManager::new(SessionConfig {
        max_sessions: 1,
        ..SessionConfig::default()
    });
    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let timestamp_ms = 5_000;
    let mut candidate = EnhancedCandidate {
        pool_amm_id: pool,
        base_mint,
        bonding_curve,
        timestamp: timestamp_ms,
        ..EnhancedCandidate::default()
    };
    candidate.signature = Signature::new_unique().to_string();
    let gatekeeper = GatekeeperV2Config::default();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate,
            created_at_wall_ms: timestamp_ms,
            deadline_wall_ms: Some(timestamp_ms + 30_000),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&gatekeeper),
            gatekeeper_config: gatekeeper,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .unwrap();
    let session = manager.get_session(&pool).unwrap();
    let mut guard = session.write();
    for success in successes {
        let _outcome = guard.ingest_transaction(Arc::new(tx(
            Pubkey::new_unique(),
            Signature::new_unique(),
            timestamp_ms,
            1.0,
            true,
            *success,
            None,
        )));
    }
    guard.metric_contract_recent_exact_timing_snapshot()
}

#[test]
fn ftdi_preserves_value_population_and_splits_legacy_from_corrected_actionability() {
    let signer_a = Pubkey::new_unique();
    let signer_b = Pubkey::new_unique();
    let first_a = tx(
        signer_a,
        Signature::new_unique(),
        1_000,
        1.0,
        true,
        true,
        Some((1, 0)),
    );
    let second_a_different_topology = tx(
        signer_a,
        Signature::new_unique(),
        1_010,
        1.0,
        true,
        true,
        Some((9, 9)),
    );
    let first_b = tx(
        signer_b,
        Signature::new_unique(),
        1_020,
        1.0,
        true,
        true,
        Some((1, 0)),
    );
    let computation = compute_ftdi([&first_a, &second_a_different_topology, &first_b]);

    assert_eq!(computation.buy_sample_count, 3);
    assert_eq!(computation.signer_sample_count, 2);
    assert_eq!(computation.unique_topology_count, 1);
    assert_eq!(computation.fee_topology_diversity_index, Some(0.5));
    assert_eq!(computation.coordination_hhi, Some(1.0));
    assert!(computation.legacy_buy_tx_actionable);
    assert!(!computation.unique_buyer_actionable_v2);

    let (profile, resolved, ..) = runtime_contract_context();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let evidence = build_ftdi_evidence_v1(&computation, &context).unwrap();
    assert!(evidence.legacy_buy_tx_actionable);
    assert!(!evidence.unique_buyer_actionable_v2);
    assert_eq!(
        evidence
            .unique_buyer_actionability_v2_envelope
            .authority_class,
        MetricAuthorityClass::Counterfactual
    );
    assert!(
        !evidence
            .unique_buyer_actionability_v2_envelope
            .policy_actionable
    );
    assert_eq!(
        evidence.coordination_hhi_export_envelope.authority_class,
        MetricAuthorityClass::ExportOnly
    );
}

#[test]
fn dev_first_observed_policy_stays_legacy_while_primary_is_success_dust_dedupe_safe() {
    let dev = Pubkey::new_unique();
    let create_signature = Signature::new_unique();
    let candidate = EnhancedCandidate {
        signature: create_signature.to_string(),
        timestamp: 1_000,
        slot: Some(100),
        ..EnhancedCandidate::default()
    };
    let gatekeeper = GatekeeperV2Config {
        min_sol_threshold: 0.01,
        ..GatekeeperV2Config::default()
    };
    let fingerprint = EarlyFingerprintConfig::default();
    let config = TxIntelligenceConfig::from_gatekeeper_config(&gatekeeper, fingerprint.clone());
    let funding = FundingSourceConfig::from_gatekeeper_config(&gatekeeper);
    let resolved = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &gatekeeper,
        &config,
        &fingerprint,
        &funding,
        None,
    )
    .unwrap();
    let mut engine = TxIntelligenceEngine::new(config, &candidate, Some(dev));

    let failed_first = tx(dev, Signature::new_unique(), 1_000, 0.40, true, false, None);
    let dust_create = tx(dev, create_signature, 1_005, 0.001, true, true, None);
    let eligible_fallback = tx(dev, Signature::new_unique(), 1_010, 0.75, true, true, None);
    let eligible_create = tx(dev, create_signature, 1_020, 1.25, true, true, None);
    engine.on_transaction(&failed_first);
    engine.on_transaction(&dust_create);
    engine.on_transaction(&eligible_fallback);
    engine.on_transaction(&eligible_create);
    engine.on_transaction(&eligible_create);

    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    assert_eq!(features.dev_buy_sol, 0.40);
    assert_eq!(snapshot.dev_first_observed.amount_sol, Some(0.40));
    assert_eq!(snapshot.dev_first_observed.selected_success, Some(false));
    assert_eq!(snapshot.dev_primary_v1.amount_sol, Some(1.25));
    assert_eq!(snapshot.dev_primary_v1.eligible_buy_count, 2);
    assert!(snapshot.dev_primary_v1.create_signature_matched);
    assert_eq!(
        snapshot.dev_primary_v1.selection_mode,
        DevBuySelectionModeV1::CreateSignatureMatch
    );

    let profile = MetricContractProfileV1::profile_a().unwrap();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let compatibility = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
        amount_sol: Some(0.40),
        creator_known: true,
        create_signature: Some(create_signature.to_string()),
        create_signature_matched: false,
        selection_mode: DevBuySelectionModeV1::EarliestEligibleCreatorBuy,
        selected_signature: Some(failed_first.signature.clone()),
        selected_slot: failed_first.slot,
        selected_transaction_index: failed_first.tx_index,
        eligible_buy_count: 3,
        selected_success: Some(false),
    };
    let evidence = build_dev_buy_evidence_v1(&snapshot, &compatibility, &context).unwrap();
    assert_eq!(
        evidence.effective_policy.amount_sol,
        evidence.mfs_first_observed.amount_sol
    );
    assert_eq!(
        evidence.effective_policy.amount_sol,
        CanonicalNullableV1::Value(0.40)
    );
    assert_eq!(
        evidence.mfs_primary_v1.amount_sol,
        CanonicalNullableV1::Value(1.25)
    );
    assert_eq!(
        evidence.mfs_primary_v1.envelope.authority_class,
        MetricAuthorityClass::Counterfactual
    );
    assert!(!evidence.mfs_primary_v1.envelope.policy_actionable);
}

#[test]
fn dev_no_creator_and_no_eligible_buy_are_typed_null_not_measured_zero() {
    let candidate = EnhancedCandidate::default();
    let engine = TxIntelligenceEngine::new(TxIntelligenceConfig::default(), &candidate, None);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    assert!(!snapshot.dev_first_observed.creator_known);
    assert_eq!(snapshot.dev_first_observed.amount_sol, None);
    assert_eq!(snapshot.dev_primary_v1.amount_sol, None);

    let dev = Pubkey::new_unique();
    let mut known_engine = TxIntelligenceEngine::new(
        TxIntelligenceConfig::default(),
        &EnhancedCandidate::default(),
        Some(dev),
    );
    let failed = tx(dev, Signature::new_unique(), 1_000, 0.50, true, false, None);
    let dust = tx(dev, Signature::new_unique(), 1_010, 0.001, true, true, None);
    known_engine.on_transaction(&failed);
    known_engine.on_transaction(&dust);
    let known_features = known_engine.compute_features();
    let known_snapshot = known_engine.metric_contract_snapshot(&known_features);
    assert_eq!(known_snapshot.dev_first_observed.amount_sol, Some(0.50));
    assert_eq!(known_snapshot.dev_primary_v1.amount_sol, None);
    assert_eq!(
        known_snapshot.dev_primary_v1.selection_mode,
        DevBuySelectionModeV1::NoEligibleBuy
    );
    assert_eq!(known_snapshot.dev_primary_v1.eligible_buy_count, 0);
}

#[test]
fn dev_primary_fallback_uses_earliest_stable_key_not_delivery_order() {
    let dev = Pubkey::new_unique();
    let candidate = EnhancedCandidate {
        signature: Signature::new_unique().to_string(),
        timestamp: 1_000,
        ..EnhancedCandidate::default()
    };
    let mut engine =
        TxIntelligenceEngine::new(TxIntelligenceConfig::default(), &candidate, Some(dev));
    let delivered_first_but_later = tx(dev, Signature::new_unique(), 1_020, 1.20, true, true, None);
    let delivered_second_but_earlier =
        tx(dev, Signature::new_unique(), 1_010, 0.80, true, true, None);
    engine.on_transaction(&delivered_first_but_later);
    engine.on_transaction(&delivered_second_but_earlier);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);

    assert_eq!(snapshot.dev_first_observed.amount_sol, Some(1.20));
    assert_eq!(snapshot.dev_primary_v1.amount_sol, Some(0.80));
    assert_eq!(
        snapshot.dev_primary_v1.selection_mode,
        DevBuySelectionModeV1::EarliestEligibleCreatorBuy
    );
    assert!(!snapshot.dev_primary_v1.create_signature_matched);
    assert_eq!(snapshot.dev_primary_v1.eligible_buy_count, 2);
}

#[test]
fn timing_exact_cluster_recent_missing_timestamp_and_dedupe_are_distinct() {
    let signer = Pubkey::new_unique();
    let mut engine = TxIntelligenceEngine::new(
        TxIntelligenceConfig::default(),
        &EnhancedCandidate::default(),
        None,
    );
    let first = tx(
        signer,
        Signature::new_unique(),
        1_000,
        1.0,
        true,
        true,
        None,
    );
    let exact = tx(
        Pubkey::new_unique(),
        Signature::new_unique(),
        1_000,
        1.0,
        true,
        true,
        None,
    );
    let near = tx(
        Pubkey::new_unique(),
        Signature::new_unique(),
        1_010,
        1.0,
        true,
        true,
        None,
    );
    engine.on_transaction(&first);
    engine.on_transaction(&exact);
    engine.on_transaction(&near);
    engine.on_transaction(&near);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    assert_eq!(snapshot.exact_same_ms.numerator, 1);
    assert_eq!(snapshot.exact_same_ms.denominator, 3);
    assert_eq!(snapshot.cluster_lt_50ms.numerator, 2);
    assert_eq!(
        features.same_ms_tx_ratio.to_bits(),
        (1.0_f64 / 3.0).to_bits()
    );

    let recent = TxTimingProducerSnapshotV1 {
        numerator: 0,
        denominator: 2,
        ratio: Some(0.0),
        canonical_dedupe_applied: true,
        dust_filter_sol: None,
        window_ms: Some(10_000),
        fallback_timestamp_count: 0,
        fallback_ordering_count: 0,
        source_complete: true,
        source_state_capacity: Some(
            u64::try_from(GatekeeperV2Config::default().decision_time_series_tx_capacity).unwrap(),
        ),
    };
    let (profile, resolved, ..) = runtime_contract_context();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let evidence = build_tx_timing_evidence_v1(&snapshot, &recent, &context).unwrap();
    assert_eq!(evidence.legacy_exact.ratio, evidence.exact_v1.ratio);
    assert_ne!(evidence.legacy_exact.ratio, evidence.cluster_lt_50ms.ratio);
    assert_ne!(evidence.legacy_exact.ratio, evidence.recent_exact.ratio);
    assert!(evidence.legacy_exact.envelope.policy_actionable);
    assert!(!evidence.cluster_lt_50ms.envelope.policy_actionable);
    assert!(!evidence.recent_exact.envelope.policy_actionable);
    assert_eq!(
        evidence.recent_exact.dust_filter_sol,
        CanonicalNullableV1::Null
    );

    let mut missing_timestamp = tx(
        Pubkey::new_unique(),
        Signature::new_unique(),
        0,
        1.0,
        true,
        true,
        None,
    );
    missing_timestamp.event_time = EventTimeMetadata::default();
    missing_timestamp.arrival_ts_ms = 0;
    missing_timestamp.signature.clear();
    missing_timestamp.event_ordinal = None;
    missing_timestamp.tx_index = None;
    let mut missing_engine = TxIntelligenceEngine::new(
        TxIntelligenceConfig::default(),
        &EnhancedCandidate::default(),
        None,
    );
    missing_engine.on_transaction(&missing_timestamp);
    let missing_features = missing_engine.compute_features();
    let missing_snapshot = missing_engine.metric_contract_snapshot(&missing_features);
    assert_eq!(missing_snapshot.exact_same_ms.fallback_timestamp_count, 1);
    assert_eq!(missing_snapshot.exact_same_ms.fallback_ordering_count, 1);
}

#[test]
fn timing_cluster_boundary_is_strictly_below_fifty_milliseconds() {
    let mut engine = TxIntelligenceEngine::new(
        TxIntelligenceConfig::default(),
        &EnhancedCandidate::default(),
        None,
    );
    for timestamp_ms in [1_000, 1_000, 1_001, 1_050, 1_100] {
        engine.on_transaction(&tx(
            Pubkey::new_unique(),
            Signature::new_unique(),
            timestamp_ms,
            1.0,
            true,
            true,
            None,
        ));
    }
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    assert_eq!(snapshot.exact_same_ms.numerator, 1);
    assert_eq!(snapshot.cluster_lt_50ms.numerator, 3);
    assert_eq!(snapshot.exact_same_ms.denominator, 5);
    assert_eq!(snapshot.cluster_lt_50ms.denominator, 5);
}

#[test]
fn recent_exact_snapshot_uses_successful_ten_second_window_not_full_observation() {
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
    let gatekeeper = GatekeeperV2Config::default();
    let funding_source_config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper);
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate,
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(30_000),
            gatekeeper_config: gatekeeper,
            funding_source_config,
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .unwrap();
    let session = manager.get_session(&pool).unwrap();
    let mut guard = session.write();
    for timestamp_ms in [1_000, 1_000, 1_000, 20_000, 20_001] {
        let _outcome = guard.ingest_transaction(Arc::new(tx(
            Pubkey::new_unique(),
            Signature::new_unique(),
            timestamp_ms,
            1.0,
            true,
            true,
            None,
        )));
    }
    let full_features = guard.tx_intel_features.clone();
    let full_snapshot = guard
        .tx_intelligence
        .metric_contract_snapshot(&full_features);
    let recent_snapshot = guard.metric_contract_recent_exact_timing_snapshot();
    assert_eq!(full_snapshot.exact_same_ms.numerator, 2);
    assert_eq!(full_snapshot.exact_same_ms.denominator, 5);
    assert_eq!(recent_snapshot.numerator, 0);
    assert_eq!(recent_snapshot.denominator, 2);
    assert_eq!(recent_snapshot.ratio, Some(0.0));
    assert_eq!(recent_snapshot.window_ms, Some(10_000));
    assert_eq!(recent_snapshot.dust_filter_sol, None);
}

#[test]
fn recent_exact_zero_width_one_successful_tx_is_evaluable() {
    let snapshot = recent_exact_snapshot_for_same_timestamp(&[true]);
    assert_eq!(snapshot.denominator, 1);
    assert_eq!(snapshot.numerator, 0);
    assert_eq!(snapshot.ratio, Some(0.0));
}

#[test]
fn recent_exact_zero_width_two_successful_txs_count_same_ms_extra() {
    let snapshot = recent_exact_snapshot_for_same_timestamp(&[true, true]);
    assert_eq!(snapshot.denominator, 2);
    assert_eq!(snapshot.numerator, 1);
    assert_eq!(snapshot.ratio, Some(0.5));
}

#[test]
fn recent_exact_zero_width_three_successful_txs_count_all_extras() {
    let snapshot = recent_exact_snapshot_for_same_timestamp(&[true, true, true]);
    assert_eq!(snapshot.denominator, 3);
    assert_eq!(snapshot.numerator, 2);
    assert_eq!(snapshot.ratio, Some(2.0 / 3.0));
}

#[test]
fn recent_exact_zero_width_excludes_failed_tx_from_successful_population() {
    let snapshot = recent_exact_snapshot_for_same_timestamp(&[true, false, true]);
    assert_eq!(snapshot.denominator, 2);
    assert_eq!(snapshot.numerator, 1);
    assert_eq!(snapshot.ratio, Some(0.5));
}

#[test]
fn top3_preferred_fallback_mismatch_and_ratio_scale_preserve_existing_selector() {
    let engine = TxIntelligenceEngine::new(
        TxIntelligenceConfig::default(),
        &EnhancedCandidate::default(),
        None,
    );
    let mut features = engine.compute_features();
    features.tx_count = 4;
    features.total_volume_sol = 10.0;
    features.top3_signer_volume_ratio = Some(0.60);
    features.top3_volume_pct = 0.20;
    let mismatch = engine.metric_contract_snapshot(&features);
    assert_eq!(mismatch.top3.effective_ratio, Some(0.60));
    assert_eq!(mismatch.top3.preferred_alias_bitwise_equal, Some(false));
    assert!(!mismatch.top3.used_compatibility_fallback);

    let (profile, resolved, ..) = runtime_contract_context();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let evidence = build_top3_evidence_v1(&mismatch, &context).unwrap();
    assert_eq!(evidence.effective_ratio, CanonicalNullableV1::Value(0.60));

    features.top3_signer_volume_ratio = None;
    features.top3_volume_pct = 0.67;
    let fallback = engine.metric_contract_snapshot(&features);
    assert_eq!(fallback.top3.effective_ratio, Some(0.67));
    assert!(fallback.top3.used_compatibility_fallback);
    assert!((0.0..=1.0).contains(&fallback.top3.effective_ratio.unwrap()));
}

#[test]
fn fsc_compatibility_status_does_not_claim_v2_measurement_or_policy_authority() {
    let (_, resolved, gatekeeper, _, funding) = runtime_contract_context();
    let computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    let profile = MetricContractProfileV1::profile_a().unwrap();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let status = build_fsc_status_evidence_v1(
        &computation,
        &funding,
        &funding_producer_config(&funding),
        &context,
    )
    .unwrap();
    assert!(!status.legacy_scalar_present);
    assert_ne!(
        status.fsc_v2_status,
        CanonicalNullableV1::Value(ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean)
    );
    assert!(!status.envelope.policy_actionable);
    assert!(!gatekeeper.max_funding_source_concentration.is_nan());

    let mut one_known = computation.clone();
    one_known.distinct_known_source_count = 1;
    one_known.known_source_sample_count = 1;
    one_known.diagnostics.buyer_sample_count = 1;
    one_known.degraded_reasons =
        vec![ghost_core::tx_intelligence::types::FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()];
    let one_known_evidence = build_funding_evidence_v1(
        &one_known,
        &funding,
        &funding_producer_config(&funding),
        &context,
    )
    .unwrap();
    assert_eq!(
        one_known_evidence.legacy_v1.ratio,
        CanonicalNullableV1::Null
    );
    assert_eq!(
        one_known_evidence.legacy_v1.envelope.availability,
        MetricAvailabilityV1::Unavailable
    );
    assert_eq!(
        one_known_evidence.legacy_v1.envelope.measurement_quality,
        MetricMeasurementQualityV1::NotApplicable
    );

    let mut two_known_samples = one_known;
    two_known_samples.known_source_sample_count = 2;
    two_known_samples.funding_source_concentration = Some(0.5);
    two_known_samples.degraded_reasons.clear();
    let measured = build_funding_evidence_v1(
        &two_known_samples,
        &funding,
        &funding_producer_config(&funding),
        &context,
    )
    .unwrap();
    assert_eq!(measured.legacy_v1.ratio, CanonicalNullableV1::Value(0.5));
    assert_eq!(
        measured.legacy_v1.envelope.measurement_quality,
        MetricMeasurementQualityV1::Measured
    );
}

#[test]
fn resolved_effective_config_is_deterministic_sensitive_and_rejects_producer_mismatch() {
    let (_, first, gatekeeper, tx_config, funding) = runtime_contract_context();
    let fingerprint = EarlyFingerprintConfig::default();
    let second = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &gatekeeper,
        &tx_config,
        &fingerprint,
        &funding,
        None,
    )
    .unwrap();
    assert_eq!(
        first.metric_contract_effective_config_hash,
        second.metric_contract_effective_config_hash
    );

    let mut changed_gatekeeper = gatekeeper.clone();
    changed_gatekeeper.min_sol_threshold = 0.006;
    let changed_tx =
        TxIntelligenceConfig::from_gatekeeper_config(&changed_gatekeeper, fingerprint.clone());
    let changed_funding = FundingSourceConfig::from_gatekeeper_config(&changed_gatekeeper);
    let changed = resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &changed_gatekeeper,
        &changed_tx,
        &fingerprint,
        &changed_funding,
        None,
    )
    .unwrap();
    assert_ne!(
        first.metric_contract_effective_config_hash,
        changed.metric_contract_effective_config_hash
    );
    assert!(resolve_metric_contract_effective_config_v1(
        MetricContractFoundationConfigV1::default(),
        &changed_gatekeeper,
        &tx_config,
        &fingerprint,
        &changed_funding,
        None,
    )
    .is_err());
}

#[test]
fn runtime_resolved_config_keeps_legacy_window_and_fsc_minimum() {
    let (profile, resolved, gatekeeper, tx_config, funding) = runtime_contract_context();
    assert_eq!(
        resolved.value(MetricEffectiveConfigKeyV1::SameMsExactDeltaMs),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(0)
        ))
    );
    assert_eq!(
        resolved.value(MetricEffectiveConfigKeyV1::SameMsClusterUpperBoundExclusiveMs),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(50)
        ))
    );
    assert_eq!(
        resolved.value(MetricEffectiveConfigKeyV1::SameMsRecentWindowMs),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(10_000)
        ))
    );
    assert_eq!(
        resolved.value(MetricEffectiveConfigKeyV1::FscLegacyMinKnownSourceSamples),
        Some(&MetricEffectiveConfigValueV1::WideUnsigned(
            CanonicalU64StringV1::new(2)
        ))
    );

    let evidence_context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let candidate = EnhancedCandidate::default();
    let engine = TxIntelligenceEngine::new(tx_config, &candidate, None);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    let recent = TxTimingProducerSnapshotV1 {
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
            u64::try_from(gatekeeper.decision_time_series_tx_capacity.max(1)).unwrap(),
        ),
    };
    let timing = build_tx_timing_evidence_v1(&snapshot, &recent, &evidence_context).unwrap();
    let fsc = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    let producer_config = funding_producer_config(&funding);
    assert_eq!(
        fsc.funding_source_v2.config_hash,
        producer_config.producer_config_hash()
    );
    let funding_evidence =
        build_funding_evidence_v1(&fsc, &funding, &producer_config, &evidence_context).unwrap();
    let projection_context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(10_000, Some(1)).unwrap(),
    };
    TxTimingDecisionProjectionV1::try_from_evidence(&timing, &projection_context).unwrap();
    FundingDecisionProjectionV1::try_from_evidence(&funding_evidence, &projection_context).unwrap();
}

#[test]
fn rehashed_non_compact_config_drift_is_rejected_at_frozen_producer_boundaries() {
    let (profile, resolved, gatekeeper, tx_config, funding) = runtime_contract_context();
    let candidate = EnhancedCandidate::default();
    let engine = TxIntelligenceEngine::new(tx_config, &candidate, None);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    let compatibility = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
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
    let recent = TxTimingProducerSnapshotV1 {
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
            u64::try_from(gatekeeper.decision_time_series_tx_capacity.max(1)).unwrap(),
        ),
    };
    let ftdi = compute_ftdi(std::iter::empty::<&PoolTransaction>());
    let fsc = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);

    let assert_boundary_rejects =
        |config: &ResolvedMetricContractEffectiveConfigV1,
         result: Result<(), Pr2aProducerErrorV1>| {
            assert_ne!(
                config.metric_contract_effective_config_hash,
                resolved.metric_contract_effective_config_hash
            );
            assert!(matches!(
                result,
                Err(Pr2aProducerErrorV1::ProducerConfigMismatch(_))
            ));
        };

    let config = rehashed_runtime_config_with_value(
        &resolved,
        MetricEffectiveConfigKeyV1::FtdiMissingSignerBehavior,
        MetricEffectiveConfigValueV1::Enum("drop_missing_signer".to_string()),
    );
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
    };
    assert_boundary_rejects(&config, build_ftdi_evidence_v1(&ftdi, &context).map(|_| ()));

    let config = rehashed_runtime_config_with_value(
        &resolved,
        MetricEffectiveConfigKeyV1::DevTxIntelDedupeKey,
        MetricEffectiveConfigValueV1::Enum("signature_only".to_string()),
    );
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
    };
    assert_boundary_rejects(
        &config,
        build_dev_buy_evidence_v1(&snapshot, &compatibility, &context).map(|_| ()),
    );

    let config = rehashed_runtime_config_with_value(
        &resolved,
        MetricEffectiveConfigKeyV1::SameMsRecentDedupeKey,
        MetricEffectiveConfigValueV1::Enum("signature_only".to_string()),
    );
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
    };
    assert_boundary_rejects(
        &config,
        build_tx_timing_evidence_v1(&snapshot, &recent, &context).map(|_| ()),
    );

    let config = rehashed_runtime_config_with_value(
        &resolved,
        MetricEffectiveConfigKeyV1::Top3MismatchBehavior,
        MetricEffectiveConfigValueV1::Enum("alias_authoritative".to_string()),
    );
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
    };
    assert_boundary_rejects(
        &config,
        build_top3_evidence_v1(&snapshot, &context).map(|_| ()),
    );

    let config = rehashed_runtime_config_with_value(
        &resolved,
        MetricEffectiveConfigKeyV1::FscFundingLookbackWindowMs,
        MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(
            funding.lookback_window_ms + 1,
        )),
    );
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &config,
    };
    assert_boundary_rejects(
        &config,
        build_funding_evidence_v1(&fsc, &funding, &funding_producer_config(&funding), &context)
            .map(|_| ()),
    );
}

#[test]
fn evidence_builders_fail_closed_when_frozen_snapshot_settings_do_not_match_config_hash() {
    let (profile, resolved, gatekeeper, tx_config, funding) = runtime_contract_context();
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let candidate = EnhancedCandidate::default();
    let engine = TxIntelligenceEngine::new(tx_config, &candidate, None);
    let features = engine.compute_features();
    let snapshot = engine.metric_contract_snapshot(&features);
    let compatibility = GatekeeperDevPrimaryCompatibilitySnapshotV1 {
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

    let mut wrong_tx_snapshot = snapshot.clone();
    wrong_tx_snapshot.producer_dust_filter_sol += 0.001;
    assert!(build_dev_buy_evidence_v1(&wrong_tx_snapshot, &compatibility, &context).is_err());
    assert!(build_top3_evidence_v1(&wrong_tx_snapshot, &context).is_err());

    let mut wrong_recent = TxTimingProducerSnapshotV1 {
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
    wrong_recent.source_state_capacity = wrong_recent.source_state_capacity.map(|value| value + 1);
    assert!(build_tx_timing_evidence_v1(&snapshot, &wrong_recent, &context).is_err());

    let computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    let mut wrong_funding = funding;
    wrong_funding.lookback_window_ms += 1;
    let wrong_snapshot = funding_producer_config(&wrong_funding);
    assert!(
        build_funding_evidence_v1(&computation, &wrong_funding, &wrong_snapshot, &context).is_err()
    );
    assert!(
        build_fsc_status_evidence_v1(&computation, &wrong_funding, &wrong_snapshot, &context)
            .is_err()
    );
}

#[test]
fn stale_fsc_computation_is_rejected_by_both_builders_after_config_is_rehashed() {
    let gatekeeper = GatekeeperV2Config::default();
    let config_a = FscV2Config {
        min_known_coverage: 0.50,
        ..FscV2Config::default()
    };
    let (_, _, funding_a, _) = fsc_runtime_contract_context(&gatekeeper, &config_a);
    let mut computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding_a);
    computation.funding_source_v2.total_buyers = 5;
    computation.funding_source_v2.known_buyers = 3;
    computation.funding_source_v2.known_non_neutral_buyers = 2;
    computation.funding_source_v2.known_coverage = 0.60;
    computation.funding_source_v2.non_neutral_known_coverage = 0.40;
    computation.funding_source_v2.status =
        ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean;

    let mut config_b = config_a.clone();
    config_b.min_known_coverage = 0.80;
    let (profile, resolved_b, funding_b, producer_b) =
        fsc_runtime_contract_context(&gatekeeper, &config_b);
    assert_ne!(
        funding_a.metric_contract_producer_config_hash(),
        funding_b.metric_contract_producer_config_hash()
    );
    let context_b = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved_b,
    };

    for error in [
        build_funding_evidence_v1(&computation, &funding_b, &producer_b, &context_b).unwrap_err(),
        build_fsc_status_evidence_v1(&computation, &funding_b, &producer_b, &context_b)
            .unwrap_err(),
    ] {
        assert!(matches!(
            error,
            Pr2aProducerErrorV1::ProducerConfigMismatch("fsc.computation_config_hash")
        ));
    }
}

#[test]
fn non_neutral_buyer_count_is_full_evidence_only_and_not_compact_projection() {
    let gatekeeper = GatekeeperV2Config::default();
    let fsc_v2 = FscV2Config {
        min_known_non_neutral_buyers: 1,
        ..FscV2Config::default()
    };
    let (profile, resolved, funding, producer_config) =
        fsc_runtime_contract_context(&gatekeeper, &fsc_v2);
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let mut computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    computation.funding_source_v2.total_buyers = 2;
    computation.funding_source_v2.known_buyers = 2;
    computation.funding_source_v2.known_non_neutral_buyers = 1;
    computation.funding_source_v2.known_coverage = 1.0;
    computation.funding_source_v2.non_neutral_known_coverage = 0.5;
    computation.funding_source_v2.status =
        ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean;

    let evidence =
        build_funding_evidence_v1(&computation, &funding, &producer_config, &context).unwrap();
    assert_eq!(evidence.known_non_neutral_buyer_count, 1);

    let projection_context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(10_000, Some(1)).unwrap(),
    };
    let projection =
        FundingDecisionProjectionV1::try_from_evidence(&evidence, &projection_context).unwrap();
    let compact_json = serde_json::to_value(projection).unwrap();
    assert!(!compact_json
        .as_object()
        .unwrap()
        .contains_key("known_non_neutral_buyer_count"));

    let mut mismatched = computation;
    mismatched.funding_source_v2.known_non_neutral_buyers = 2;
    assert!(matches!(
        build_funding_evidence_v1(&mismatched, &funding, &producer_config, &context),
        Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_non_neutral_known_coverage"
        ))
    ));
}

#[test]
fn fsc_frozen_boundary_rejects_count_coverage_drift_and_clean_non_neutral_minimum() {
    let gatekeeper = GatekeeperV2Config::default();
    let fsc_v2 = FscV2Config::default();
    let (profile, resolved, funding, producer_config) =
        fsc_runtime_contract_context(&gatekeeper, &fsc_v2);
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let mut valid = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    valid.funding_source_v2.total_buyers = 2;
    valid.funding_source_v2.known_buyers = 2;
    valid.funding_source_v2.known_non_neutral_buyers = 2;
    valid.funding_source_v2.known_coverage = 1.0;
    valid.funding_source_v2.non_neutral_known_coverage = 1.0;
    valid.funding_source_v2.status = ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean;
    build_funding_evidence_v1(&valid, &funding, &producer_config, &context).unwrap();

    let mut wrong_counts = valid.clone();
    wrong_counts.funding_source_v2.known_non_neutral_buyers = 3;
    assert!(matches!(
        build_funding_evidence_v1(&wrong_counts, &funding, &producer_config, &context),
        Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_buyer_counts"
        ))
    ));

    let mut known_exceeds_total = valid.clone();
    known_exceeds_total.funding_source_v2.known_buyers = 3;
    assert!(matches!(
        build_funding_evidence_v1(&known_exceeds_total, &funding, &producer_config, &context),
        Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_buyer_counts"
        ))
    ));

    let mut wrong_known_coverage = valid.clone();
    wrong_known_coverage.funding_source_v2.known_buyers = 1;
    wrong_known_coverage
        .funding_source_v2
        .known_non_neutral_buyers = 1;
    assert!(matches!(
        build_funding_evidence_v1(&wrong_known_coverage, &funding, &producer_config, &context),
        Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_known_coverage"
        ))
    ));

    let mut wrong_non_neutral_coverage = valid.clone();
    wrong_non_neutral_coverage
        .funding_source_v2
        .known_non_neutral_buyers = 1;
    assert!(matches!(
        build_funding_evidence_v1(
            &wrong_non_neutral_coverage,
            &funding,
            &producer_config,
            &context
        ),
        Err(Pr2aProducerErrorV1::ProducerInvariant(
            "fsc.v2_non_neutral_known_coverage"
        ))
    ));

    let mut below_clean_minimum = valid;
    below_clean_minimum
        .funding_source_v2
        .known_non_neutral_buyers = 1;
    below_clean_minimum
        .funding_source_v2
        .non_neutral_known_coverage = 0.5;
    for error in [
        build_funding_evidence_v1(&below_clean_minimum, &funding, &producer_config, &context)
            .unwrap_err(),
        build_fsc_status_evidence_v1(&below_clean_minimum, &funding, &producer_config, &context)
            .unwrap_err(),
    ] {
        assert!(matches!(
            error,
            Pr2aProducerErrorV1::ProducerInvariant("fsc.v2_clean_effective_config_minimum")
        ));
    }
}

#[test]
fn fsc_computation_embedded_producer_settings_are_defensively_cross_checked() {
    let gatekeeper = GatekeeperV2Config {
        neutral_funding_sources: vec!["neutral-source-a".to_string()],
        ..GatekeeperV2Config::default()
    };
    let fsc_v2 = FscV2Config {
        neutral_funder_set_version: Some("v1".to_string()),
        ..FscV2Config::default()
    };
    let (profile, resolved, funding, producer_config) =
        fsc_runtime_contract_context(&gatekeeper, &fsc_v2);
    let computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    let context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let mut variants = Vec::new();
    let mut changed = computation.clone();
    changed.funding_source_v2.min_abs_store_lamports += 1;
    variants.push(changed);
    let mut changed = computation.clone();
    changed.funding_source_v2.min_abs_attribution_lamports += 1;
    variants.push(changed);
    let mut changed = computation.clone();
    changed.funding_source_v2.min_rel_to_buy = 0.25;
    variants.push(changed);
    let mut changed = computation.clone();
    changed.funding_source_v2.ttl_seconds += 1;
    variants.push(changed);
    let mut changed = computation.clone();
    changed.funding_source_v2.neutral_funder_set_version = Some("v2".to_string());
    variants.push(changed);
    let mut changed = computation;
    changed.funding_source_v2.neutral_funder_set_hash = Some("fnv64:bad".to_string());
    variants.push(changed);

    for changed in variants {
        assert!(matches!(
            build_funding_evidence_v1(&changed, &funding, &producer_config, &context),
            Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
                "fsc.computation_embedded_settings"
            ))
        ));
    }
}

#[test]
fn neutral_set_version_is_part_of_fsc_computation_provenance() {
    let gatekeeper = GatekeeperV2Config {
        neutral_funding_sources: vec!["neutral-source-a".to_string()],
        ..GatekeeperV2Config::default()
    };
    let config_a = FscV2Config {
        neutral_funder_set_version: Some("v1".to_string()),
        ..FscV2Config::default()
    };
    let (_, _, funding_a, _) = fsc_runtime_contract_context(&gatekeeper, &config_a);
    let computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding_a);

    let mut config_b = config_a.clone();
    config_b.neutral_funder_set_version = Some("v2".to_string());
    let (profile, resolved_b, funding_b, producer_b) =
        fsc_runtime_contract_context(&gatekeeper, &config_b);
    assert_eq!(
        funding_a.metric_contract_neutral_funder_set_hash().unwrap(),
        funding_b.metric_contract_neutral_funder_set_hash().unwrap()
    );
    assert_eq!(
        funding_a.metric_contract_neutral_funder_set_producer_hash(),
        funding_b.metric_contract_neutral_funder_set_producer_hash()
    );
    assert_ne!(
        funding_a.metric_contract_producer_config_hash(),
        funding_b.metric_contract_producer_config_hash()
    );
    let context_b = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved_b,
    };
    assert!(matches!(
        build_funding_evidence_v1(&computation, &funding_b, &producer_b, &context_b),
        Err(Pr2aProducerErrorV1::ProducerConfigMismatch(
            "fsc.computation_config_hash"
        ))
    ));
}

#[test]
fn warmup_and_same_slot_effective_config_drift_fail_at_frozen_fsc_boundary() {
    let gatekeeper = GatekeeperV2Config::default();
    let fsc_v2 = FscV2Config::default();
    let (profile, resolved, funding, producer_config) =
        fsc_runtime_contract_context(&gatekeeper, &fsc_v2);
    let computation = FundingSourceIndex::new()
        .compute_for_transactions(std::iter::empty::<&PoolTransaction>(), &funding);
    for (key, value, expected_field) in [
        (
            MetricEffectiveConfigKeyV1::FscWarmupWindowMs,
            MetricEffectiveConfigValueV1::WideUnsigned(CanonicalU64StringV1::new(
                producer_config.warmup_window_ms() - 1,
            )),
            "fsc.warmup_window_ms",
        ),
        (
            MetricEffectiveConfigKeyV1::FscSameSlotOrderingPolicy,
            MetricEffectiveConfigValueV1::Enum("arrival_order".to_string()),
            "fsc.same_slot_ordering_policy",
        ),
    ] {
        let changed = rehashed_runtime_config_with_value(&resolved, key, value);
        changed.validate_hash().unwrap();
        let context = Pr2aEvidenceBuildContextV1 {
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &profile,
            effective_config: &changed,
        };
        assert!(matches!(
            build_funding_evidence_v1(
                &computation,
                &funding,
                &producer_config,
                &context
            ),
            Err(Pr2aProducerErrorV1::ProducerConfigMismatch(field)) if field == expected_field
        ));
    }
}

#[test]
fn projection_builders_consume_one_frozen_producer_result_without_recompute() {
    let signer_a = Pubkey::new_unique();
    let signer_b = Pubkey::new_unique();
    let samples = [
        tx(
            signer_a,
            Signature::new_unique(),
            1_000,
            1.0,
            true,
            true,
            Some((1, 0)),
        ),
        tx(
            signer_b,
            Signature::new_unique(),
            1_010,
            1.0,
            true,
            true,
            Some((2, 0)),
        ),
    ];
    let mut calls = 0_u32;
    let mut producer = || {
        calls += 1;
        compute_ftdi(samples.iter())
    };
    let computation = producer();
    let (profile, resolved, ..) = runtime_contract_context();
    let evidence_context = Pr2aEvidenceBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
    };
    let evidence = build_ftdi_evidence_v1(&computation, &evidence_context).unwrap();
    let projection_context = MetricDecisionProjectionBuildContextV1 {
        rollout_mode: MetricContractRolloutMode::Legacy,
        profile: &profile,
        effective_config: &resolved,
        source_cutoff: MetricContractDecisionSourceCutoffV1::try_new(1_010, Some(101)).unwrap(),
    };
    let first =
        FtdiDecisionProjectionV1::try_from_evidence(&evidence, &projection_context).unwrap();
    let second =
        FtdiDecisionProjectionV1::try_from_evidence(&evidence, &projection_context).unwrap();
    assert_eq!(first, second);
    assert_eq!(calls, 1);
}
