use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::{AccountStateUpdate, StatePhase, UpdateSource};
use ghost_core::session::types::SessionStatus;
use ghost_core::{CurveFinality, CurveFreshnessState, EventSemanticEnvelope};
use ghost_launcher::components::gatekeeper::{
    GatekeeperBuffer, GatekeeperVerdict, GatekeeperVerdictType,
};
use ghost_launcher::components::gatekeeper_policy::{
    build_assessment_from_features, build_timeout_decision_from_assessment, PolicyEvaluationContext,
};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, SessionManager};
use ghost_launcher::tx_intelligence::FundingSourceConfig;
use seer::early_fingerprint::EarlyFingerprintConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn pipeline_config() -> GatekeeperV2Config {
    GatekeeperV2Config {
        mode: GatekeeperMode::Standard,
        min_tx_count: 4,
        min_unique_signers: 3,
        min_buy_count: 3,
        max_wait_time_ms: 5_000,
        min_sol_threshold: 0.005,
        min_interval_cv: 0.05,
        max_interval_cv: 9999.0,
        max_burst_ratio: 0.95,
        min_avg_interval_ms: 1.0,
        max_avg_interval_ms: 60_000.0,
        min_timing_entropy: 0.1,
        max_timing_entropy: 9999.0,
        min_dust_filtered_count: 0,
        min_unique_ratio: 0.3,
        max_unique_ratio: 1.0,
        max_hhi: 0.8,
        max_tx_per_signer: 10,
        max_volume_gini: 0.95,
        min_volume_gini: 0.0,
        max_top3_volume_pct: 0.99,
        max_same_ms_tx_ratio: 1.0,
        min_buy_ratio: 0.3,
        max_buy_ratio: 1.0,
        min_avg_tx_sol: 0.01,
        max_avg_tx_sol: 100.0,
        min_volume_cv: 0.01,
        max_volume_cv: 9999.0,
        min_total_volume_sol: 0.1,
        max_total_volume_sol: 9999.0,
        min_sol_buy_ratio: 0.0,
        min_consecutive_buys: 0,
        max_dev_buy_sol: 8.0,
        max_dev_tx_ratio: 0.20,
        min_dev_tx_ratio: 0.0,
        max_dev_volume_ratio: 0.40,
        min_dev_volume_ratio: 0.0,
        reject_on_dev_sell: true,
        min_dev_buy_sol: 0.0,
        max_price_change_ratio: 100.0,
        max_single_tx_price_impact_pct: 100.0,
        max_bonding_progress_pct: 100.0,
        min_market_cap_sol: 0.01,
        max_single_sell_impact_pct: 100.0,
        min_phases_to_pass: 4,
        re_eval_tx_interval: 2,
        use_three_layer_decision: true,
        hard_fail_hhi: 0.50,
        hard_fail_same_ms_tx_ratio: 0.90,
        hard_fail_top3_volume_pct: 0.99,
        max_soft_points: 30,
        soft_weight_timing: 1,
        soft_weight_manipulation: 3,
        soft_weight_diversity: 2,
        soft_weight_ecosystem: 1,
        max_soft_score: 11,
        dev_unknown_min_market_cap_sol: 0.01,
        dev_unknown_min_sol_buy_ratio: 0.0,
        dev_unknown_max_soft_points: 30,
        dev_unknown_max_single_tx_price_impact_pct: 100.0,
        max_sell_buy_ratio: 9999.0,
        min_sell_buy_ratio: 0.0,
        max_compute_unit_cluster_dominance: 1.0,
        min_compute_unit_cluster_dominance: 0.0,
        max_static_fee_profile_ratio: 1.0,
        min_static_fee_profile_ratio: 0.0,
        max_fixed_size_buy_ratio: 1.0,
        min_fixed_size_buy_ratio: 0.0,
        max_fixed_size_buy_ratio_1e4: 1.0,
        max_flipper_presence_ratio: 1.0,
        max_jito_tip_intensity: 1.0,
        min_jito_tip_intensity: 0.0,
        max_early_slot_volume_dominance_buy: 1.0,
        max_early_top3_buy_volume_pct_3s: 1.0,
        min_avg_inner_ix_count_50tx: 0.0,
        max_avg_inner_ix_count_50tx: 9999.0,
        max_whale_reversal_ratio_top3: 9999.0,
        max_whale_reversal_ratio_top1: 9999.0,
        min_dev_paperhand_latency_ms: 0,
        min_fee_topology_diversity_index: 0.0,
        max_dev_buyer_infrastructure_affinity: 1.0,
        min_spend_fraction_divergence: 0.0,
        min_demand_elasticity_score: -1.0,
        max_signer_cross_pool_velocity: 1.0,
        max_funding_source_concentration: 1.0,
        soft_penalty_low_ftdi: 0,
        soft_penalty_high_dbia: 0,
        soft_penalty_low_sfd: 0,
        soft_penalty_inelastic_demand: 0,
        soft_penalty_high_cpv: 0,
        soft_penalty_high_fsc: 0,
        soft_penalty_high_dbia_low_ftdi_combo: 0,
        soft_penalty_low_des_low_sfd_combo: 0,
        soft_penalty_high_cpv_low_des_combo: 0,
        soft_penalty_high_fsc_high_cpv_combo: 0,
        enable_sybil_interference_layer: false,
        max_sybil_soft_points: 255,
        dev_unknown_max_sybil_soft_points: 255,
        enable_sybil_combo_veto: false,
        emit_sybil_meta_score: false,
        require_ready_fsc_for_combo_veto: true,
        cpv_lookback_window_s: 300,
        funding_lookback_window_s: 300,
        funding_dust_threshold_lamports: 10_000_000,
        cpv_per_signer_cap: 16,
        cpv_global_signer_cap: 50_000,
        fsc_per_recipient_cap: 4,
        fsc_global_recipient_cap: 75_000,
        neutral_funding_sources: vec![],
        hard_fail_bot_min_tx: 20,
        hard_fail_bot_min_observation_ms: 1_500,
        min_failed_tx_ratio_for_bot_flag: Some(0.30),
        use_slot_ordering: false,
        curve_wait_ms: 800,
        curve_require_for_buy: true,
        stale_fallback: ghost_core::shadow_ledger::ShadowLedgerStaleFallback::PendingCurve,
        iwim_veto_strong_margin: 3,
        iwim_veto_strong_max_manip_flags: 0,
        min_bonding_progress_pct: 0.0,
        enable_alpha_gate: false,
        min_momentum: 0.55,
        min_demand: 0.55,
        min_alpha_joint: 0.35,
        min_alpha_sample: 15,
        enable_prosperity_filter: false,
        prosperity_min_market_cap_sol: 35.0,
        prosperity_max_signer_cross_pool_velocity: 0.50,
        prosperity_branch1_min_block0_sniped_supply_pct: 0.28,
        prosperity_branch1_max_sell_buy_ratio: 0.16,
        prosperity_branch2_min_market_cap_sol: 50.0,
        prosperity_branch2_min_early_slot_volume_dominance_buy: 0.90,
        prosperity_branch3_max_hhi: 0.0416,
        prosperity_branch3_min_fee_topology_diversity_index: 0.0909,
        enable_prosperity_overlay: false,
        prosperity_overlay_max_price_change_ratio: 2.2,
        prosperity_overlay_max_bonding_progress_pct: 85.0,
        prosperity_overlay_min_fee_topology_diversity_index: 0.10,
        prosperity_overlay_branch23_max_sell_buy_ratio: 0.18,
        prosperity_overlay_branch2_max_price_change_ratio: 2.0,
        ..Default::default()
    }
}

fn candidate(pool_id: Pubkey, base_mint: Pubkey, bonding_curve: Pubkey) -> EnhancedCandidate {
    let mut candidate = EnhancedCandidate::default();
    candidate.pool_amm_id = pool_id;
    candidate.base_mint = base_mint;
    candidate.bonding_curve = bonding_curve;
    candidate.timestamp = 1_000;
    candidate
}

fn curve_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    volume_sol: f64,
    v_sol: f64,
    v_tokens: f64,
) -> Arc<PoolTransaction> {
    Arc::new(PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        signer: signer.to_string(),
        token_mint: Some(Pubkey::new_unique().to_string()),
        owner_token_deltas: vec![],
        is_buy: true,
        volume_sol,
        price_quote: Some(v_sol / v_tokens),
        slot: Some(100_000 + (timestamp_ms / 100)),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        signature: signature.to_string(),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        sol_amount_lamports: Some((volume_sol * 1e9) as u64),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        arrival_ts_ms: timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        v_tokens_in_bonding_curve: Some(v_tokens),
        v_sol_in_bonding_curve: Some(v_sol),
        market_cap_sol: Some((v_sol / v_tokens) * 1_000_000_000.0),
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
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: true,
        curve_finality: CurveFinality::Finalized,
    })
}

fn account_update(
    pool_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    receive_ts_ms: u64,
    slot: u64,
    receive_seq: u64,
    sol_reserves: u64,
    token_reserves: u64,
) -> AccountStateUpdate {
    AccountStateUpdate {
        pool_amm_id: pool_id,
        base_mint,
        bonding_curve,
        sol_reserves,
        token_reserves,
        is_complete: 0,
        slot,
        write_version: Some(receive_ts_ms),
        receive_ts_ms,
        receive_seq,
        curve_finality: CurveFinality::Finalized,
        source: UpdateSource::GeyserAccountUpdate,
    }
}

fn canonical_ready_terminal_verdict(config: GatekeeperV2Config) -> GatekeeperVerdict {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(6_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signers = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];

    let mut guard = session.write();
    guard.checkpoint_engine.config.interval_ms = 1;
    guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        1,
        1_010,
        28_000_000_000,
        980_000_000,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[0],
        "full-sig-0",
        1_100,
        0.6,
        28.0,
        980_000.0,
    ));
    guard.try_checkpoint(1_100);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_120,
        2,
        1_120,
        29_000_000_000,
        950_000_000,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[1],
        "full-sig-1",
        1_220,
        0.8,
        29.0,
        950_000.0,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[2],
        "full-sig-2",
        1_340,
        0.9,
        30.0,
        930_000.0,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[3],
        "full-sig-3",
        1_460,
        1.1,
        31.0,
        910_000.0,
    ));
    guard.try_checkpoint(1_460);

    let features = guard.materialize_features();
    assert_eq!(features.account_features.state_phase, StatePhase::Canonical);
    assert!(features.curve_readiness.is_ready);
    assert!(features.checkpoint_features.trajectory_checkpoint_count >= 2);

    guard
        .gatekeeper_buffer_mut()
        .evaluate_from_features(features, &config)
}

fn phase1_incomplete_feature_snapshot(
    config: &GatekeeperV2Config,
) -> ghost_core::checkpoint::types::MaterializedFeatureSet {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(6_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signer = Pubkey::new_unique();

    let mut guard = session.write();
    guard.checkpoint_engine.config.interval_ms = 1;
    guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);

    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signer,
        "timeout-sig-0",
        1_100,
        0.6,
        28.0,
        980_000.0,
    ));
    guard.try_checkpoint(1_100);

    let features = guard.materialize_features();
    assert!(features.tx_intel_features.tx_count < config.min_tx_count as u64);
    assert!(features.tx_intel_features.unique_signers < config.min_unique_signers as u64);
    assert!(features.tx_intel_features.buy_count < config.min_buy_count as u64);
    features
}

#[test]
fn full_pipeline_dual_ingest_reaches_buy_verdict() {
    let config = pipeline_config();
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(6_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signers = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];

    let mut guard = session.write();
    guard.checkpoint_engine.config.interval_ms = 1;
    guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        1,
        1_010,
        28_000_000_000,
        980_000_000,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[0],
        "full-sig-0",
        1_100,
        0.6,
        28.0,
        980_000.0,
    ));
    guard.try_checkpoint(1_100);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_120,
        2,
        1_120,
        29_000_000_000,
        950_000_000,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[1],
        "full-sig-1",
        1_220,
        0.8,
        29.0,
        950_000.0,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[2],
        "full-sig-2",
        1_340,
        0.9,
        30.0,
        930_000.0,
    ));
    let _ = guard.ingest_transaction(curve_tx(
        pool_id,
        signers[3],
        "full-sig-3",
        1_460,
        1.1,
        31.0,
        910_000.0,
    ));
    guard.try_checkpoint(1_460);

    let features = guard.materialize_features();
    assert_eq!(features.account_features.state_phase, StatePhase::Canonical);
    assert!(features.curve_readiness.is_ready);
    assert!(features.checkpoint_features.trajectory_checkpoint_count >= 2);

    let verdict = guard
        .gatekeeper_buffer_mut()
        .evaluate_from_features(features, &config);

    match verdict {
        GatekeeperVerdict::Buy { assessment, .. } => {
            let decision = assessment.decision.expect("decision should be attached");
            assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
            assert!(matches!(
                guard.get_status(),
                SessionStatus::Accumulating | SessionStatus::Evaluating
            ));
        }
        _ => panic!("expected Buy verdict"),
    }
}

#[test]
fn full_pipeline_canonical_buy_fixture_stays_feature_driven() {
    let verdict = canonical_ready_terminal_verdict(pipeline_config());

    match verdict {
        GatekeeperVerdict::Buy { assessment, .. } => {
            let decision = assessment
                .decision
                .expect("feature-driven verdict must attach decision");
            assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
            assert!(decision.verdict_buy);
            assert_eq!(
                assessment.feature_snapshot.account_features.state_phase,
                StatePhase::Canonical
            );
            assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 4);
        }
        _ => panic!("expected BUY on canonical-ready feature-driven fixture"),
    }
}

#[test]
fn full_pipeline_canonical_reject_fixture_stays_feature_driven() {
    let mut config = pipeline_config();
    config.min_buy_ratio = 1.1;
    let verdict = canonical_ready_terminal_verdict(config);

    match verdict {
        GatekeeperVerdict::Reject { assessment, reason } => {
            let decision = assessment
                .decision
                .expect("feature-driven reject must attach decision");
            assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
            assert!(!decision.verdict_buy);
            assert_eq!(reason, decision.reason_chain);
            assert_eq!(
                assessment.feature_snapshot.account_features.state_phase,
                StatePhase::Canonical
            );
            assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 4);
        }
        _ => panic!("expected REJECT on canonical-ready feature-driven fixture"),
    }
}

#[test]
fn full_pipeline_timeout_replay_matches_feature_timeout_builder() {
    let feature_config = pipeline_config();
    let features = phase1_incomplete_feature_snapshot(&feature_config);
    let feature_assessment = build_assessment_from_features(
        features.clone(),
        &feature_config,
        PolicyEvaluationContext::default(),
    );
    assert!(!feature_assessment.phase1_passed);
    assert!(
        feature_assessment.hard_reject_reason.is_none(),
        "timeout parity fixture must stay phase1-incomplete without becoming a hard reject"
    );

    let feature_decision =
        build_timeout_decision_from_assessment(&feature_assessment, &feature_config);

    assert_eq!(
        feature_decision.verdict_type,
        GatekeeperVerdictType::TimeoutPhase1
    );
    assert!(!feature_decision.verdict_buy);
    assert_eq!(
        feature_assessment
            .feature_snapshot
            .tx_intel_features
            .tx_count,
        features.tx_intel_features.tx_count
    );
    assert_eq!(
        feature_assessment
            .feature_snapshot
            .tx_intel_features
            .unique_signers,
        features.tx_intel_features.unique_signers
    );
    assert_eq!(
        feature_assessment
            .feature_snapshot
            .account_features
            .state_phase,
        features.account_features.state_phase
    );
}

#[test]
fn full_pipeline_feature_verdict_caches_v25_confidence_for_terminal_assessment() {
    let config = pipeline_config();
    let verdict = canonical_ready_terminal_verdict(config.clone());

    match verdict {
        GatekeeperVerdict::Buy { assessment, .. }
        | GatekeeperVerdict::Reject { assessment, .. } => {
            assert!(
                assessment.v25_confidence.is_some(),
                "feature-driven terminal verdict must cache v25 confidence"
            );
        }
        _ => panic!("expected terminal buy/reject verdict"),
    }
}

#[test]
fn full_pipeline_phase1_incomplete_feature_verdict_caches_v25_confidence() {
    let config = pipeline_config();
    let mut buffer = GatekeeperBuffer::new(Pubkey::new_unique(), &config);
    buffer.set_registered_wall_t0(1_000);
    let features = phase1_incomplete_feature_snapshot(&config);

    let verdict = buffer.evaluate_from_features(features, &config);
    match verdict {
        GatekeeperVerdict::Timeout { assessment }
        | GatekeeperVerdict::Reject { assessment, .. } => {
            assert!(
                assessment.v25_confidence.is_some(),
                "phase1-incomplete terminal verdict must cache v25 confidence"
            );
        }
        _ => panic!("expected terminal reject/timeout verdict"),
    }
}

#[test]
fn full_pipeline_bootstrap_state_stays_non_canonical_without_account_ingest() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let config = pipeline_config();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(6_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");

    let guard = session.read();
    let features = guard.materialize_features();
    assert_eq!(features.account_features.state_phase, StatePhase::Bootstrap);
    assert!(features.account_features.is_bootstrap);
    assert!(!features.curve_readiness.is_ready);
}

#[test]
fn full_pipeline_ignores_out_of_order_account_updates() {
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let config = pipeline_config();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: None,
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms: 1_000,
            deadline_wall_ms: Some(6_000),
            gatekeeper_config: config.clone(),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&config),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");

    {
        let mut guard = session.write();
        guard.on_account_update(&account_update(
            pool_id,
            base_mint,
            bonding_curve,
            1_500,
            10,
            10,
            30_000_000_000,
            900_000_000,
        ));
        guard.on_account_update(&account_update(
            pool_id,
            base_mint,
            bonding_curve,
            1_200,
            9,
            9,
            10_000_000_000,
            990_000_000,
        ));
    }

    let guard = session.read();
    let features = guard.materialize_features();
    assert_eq!(
        features.account_features.current_reserves,
        (30_000_000_000, 900_000_000)
    );
    assert_eq!(features.account_features.state_phase, StatePhase::Canonical);
    assert_eq!(features.account_features.update_count, 1);
}
