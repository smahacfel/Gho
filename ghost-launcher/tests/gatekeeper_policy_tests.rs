use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::types::{
    AccountStateFeatures, AccountStateUpdate, StatePhase, UpdateSource,
};
use ghost_core::checkpoint::{
    AlphaFingerprintFeatures, CheckpointDerivedFeatures, CurveReadinessFeatures,
    MaterializedFeatureSet, SybilResistanceFeatures, TrendDirection,
};
use ghost_core::session::types::{SessionId, SessionMetadata};
use ghost_core::tx_intelligence::types::TxIntelFeatures;
use ghost_core::tx_intelligence::types::{
    CPV_ROLLING_STATE_UNAVAILABLE_REASON, FTDI_INSUFFICIENT_BUYS_REASON,
    SFD_PARTIAL_BALANCE_COVERAGE_REASON,
};
use ghost_core::{CurveFinality, CurveFreshnessState, EventSemanticEnvelope};
use ghost_launcher::components::gatekeeper::{
    GatekeeperIngressOutcome, GatekeeperVerdict, GatekeeperVerdictType, ProsperityRejectTrigger,
    SybilInterferencePattern, SybilLeadSignal,
};
use ghost_launcher::components::gatekeeper_policy::{
    build_assessment_from_features, build_timeout_decision_from_assessment, evaluate_hard_filters,
    evaluate_policy, evaluate_policy_from_assessment, refresh_assessment_thresholds,
    PolicyEvaluationContext,
};
use ghost_launcher::events::{PoolTransaction, RawBytesMissingReason};
use ghost_launcher::session::{OpenSessionRequest, PoolObservationSession, SessionManager};
use seer::early_fingerprint::EarlyFingerprintConfig;
use seer::early_fingerprint::EarlyFingerprintMetrics;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn policy_test_config() -> GatekeeperV2Config {
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

fn stage_b_policy_config() -> GatekeeperV2Config {
    let mut config = policy_test_config();
    config.max_soft_points = 255;
    config.dev_unknown_max_soft_points = 255;
    config.min_fee_topology_diversity_index = 0.25;
    config.max_dev_buyer_infrastructure_affinity = 0.60;
    config.min_spend_fraction_divergence = 0.08;
    config.min_demand_elasticity_score = 0.15;
    config.soft_penalty_low_ftdi = 1;
    config.soft_penalty_high_dbia = 1;
    config.soft_penalty_low_sfd = 2;
    config.soft_penalty_inelastic_demand = 3;
    config.soft_penalty_high_dbia_low_ftdi_combo = 2;
    config.soft_penalty_low_des_low_sfd_combo = 2;
    config.max_sybil_soft_points = 6;
    config.dev_unknown_max_sybil_soft_points = 5;
    config.enable_sybil_interference_layer = true;
    config
}

fn stage_c_policy_config() -> GatekeeperV2Config {
    let mut config = stage_b_policy_config();
    config.max_signer_cross_pool_velocity = 0.50;
    config.soft_penalty_high_cpv = 1;
    config.soft_penalty_high_cpv_low_des_combo = 0;
    config
}

fn current_wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_millis() as u64
}

fn balanced_prosperity_config() -> GatekeeperV2Config {
    let mut config = policy_test_config();
    config.enable_prosperity_filter = true;
    config
}

fn strict_prosperity_overlay_config() -> GatekeeperV2Config {
    let mut config = balanced_prosperity_config();
    config.enable_prosperity_overlay = true;
    config
}

fn base_feature_set() -> MaterializedFeatureSet {
    let pool_id = Pubkey::new_unique();
    MaterializedFeatureSet {
        account_features: AccountStateFeatures {
            current_reserves: (30_000_000_000, 900_000_000),
            price_sol: 35.0 / 900_000.0,
            market_cap_sol: 42.0,
            bonding_progress: 0.22,
            price_change_since_t0_pct: 12.0,
            reserve_velocity_sol_per_sec: 1.5,
            is_bootstrap: false,
            curve_finality: CurveFinality::Finalized,
            state_phase: StatePhase::Canonical,
            update_count: 3,
        },
        tx_intel_features: TxIntelFeatures {
            tx_count: 24,
            buy_count: 20,
            sell_count: 4,
            unique_signers: 18,
            buy_ratio: 0.83,
            sol_buy_ratio: 0.86,
            avg_tx_sol: 1.1,
            volume_cv: 0.25,
            hhi: 0.18,
            volume_gini: 0.22,
            unique_signer_ratio: 0.75,
            avg_tx_per_signer: 1.33,
            same_ms_tx_ratio: 0.05,
            bundle_suspicion_ratio: 0.02,
            top3_volume_pct: 0.44,
            dev_buy_sol: 0.5,
            dev_volume_ratio: 0.08,
            dev_tx_ratio: 0.04,
            dev_has_sold: false,
            interval_cv: 0.40,
            timing_entropy: 1.6,
            avg_interval_ms: 140.0,
            burst_ratio: 0.10,
            dust_ratio: 0.02,
            max_tx_per_signer: 3,
            total_volume_sol: 26.4,
            min_tx_sol: 0.2,
            max_tx_sol: 2.5,
            max_consecutive_buys: 6,
            dev_wallet_known: true,
            dev_initial_buy_tokens: Some(100_000.0),
            dev_tx_count: 1,
            dev_is_first_buyer: true,
            dust_tx_count: 1,
            failed_tx_count: 0,
        },
        checkpoint_features: CheckpointDerivedFeatures {
            price_trajectory: vec![0.000031, 0.000035, 0.000039],
            reserve_trajectory: vec![
                (28_000_000_000, 980_000_000),
                (29_000_000_000, 940_000_000),
                (30_000_000_000, 900_000_000),
            ],
            buy_pressure_trend: TrendDirection::Rising,
            signer_diversity_trend: TrendDirection::Stable,
            risk_flag_count_trend: TrendDirection::Stable,
            trajectory_checkpoint_count: 3,
            price_change_from_first_checkpoint_pct: 25.0,
            single_tx_max_price_impact_pct: 18.0,
            max_single_sell_impact_pct: 12.0,
            bonding_progress: 0.22,
            trajectory_assessment: None,
        },
        risk_flags: vec![],
        session_metadata: SessionMetadata {
            session_id: SessionId(1),
            pool_amm_id: pool_id,
            base_mint: Pubkey::new_unique(),
            observation_duration_ms: 2_000,
            is_dev_known: true,
        },
        curve_readiness: CurveReadinessFeatures {
            is_ready: true,
            freshness: CurveFreshnessState::Fresh,
            finality: CurveFinality::Finalized,
            curve_data_known: true,
            price_sample_count: 3,
            t0_event_ts_ms: Some(1_000),
            wait_elapsed_ms: Some(1_200),
        },
        sybil_resistance: SybilResistanceFeatures::default(),
        alpha_fingerprint: Default::default(),
        ..Default::default()
    }
}

fn evaluate(
    features: MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> ghost_launcher::components::gatekeeper::GatekeeperDecision {
    evaluate_policy(&features, config)
}

#[test]
fn neutral_sybil_defaults_preserve_buy_and_reject_verdicts() {
    let config = policy_test_config();
    assert_eq!(config.min_fee_topology_diversity_index, 0.0);
    assert_eq!(config.max_dev_buyer_infrastructure_affinity, 1.0);
    assert_eq!(config.min_spend_fraction_divergence, 0.0);
    assert_eq!(config.min_demand_elasticity_score, -1.0);
    assert_eq!(config.max_signer_cross_pool_velocity, 1.0);
    assert_eq!(config.max_funding_source_concentration, 1.0);
    assert_eq!(config.soft_penalty_low_ftdi, 0);
    assert_eq!(config.soft_penalty_high_dbia, 0);
    assert_eq!(config.soft_penalty_low_sfd, 0);
    assert_eq!(config.soft_penalty_inelastic_demand, 0);
    assert_eq!(config.soft_penalty_high_cpv, 0);
    assert_eq!(config.soft_penalty_high_fsc, 0);
    assert_eq!(config.soft_penalty_high_dbia_low_ftdi_combo, 0);
    assert_eq!(config.soft_penalty_low_des_low_sfd_combo, 0);
    assert_eq!(config.soft_penalty_high_cpv_low_des_combo, 0);
    assert_eq!(config.soft_penalty_high_fsc_high_cpv_combo, 0);
    assert_eq!(config.max_sybil_soft_points, 255);
    assert_eq!(config.dev_unknown_max_sybil_soft_points, 255);
    assert!(!config.enable_sybil_interference_layer);
    assert!(!config.enable_sybil_combo_veto);

    let neutral_sybil = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.0),
        dev_buyer_infrastructure_affinity: Some(1.0),
        spend_fraction_divergence: Some(0.0),
        demand_elasticity_score: Some(-1.0),
        signer_cross_pool_velocity: Some(1.0),
        funding_source_concentration: Some(1.0),
        funding_source_diagnostics: None,
        degraded_reasons: vec!["FTDI_INSUFFICIENT_BUYS".to_string()],
        buy_sample_count: 24,
        signer_sample_count: 18,
    };

    let buy_baseline = evaluate(base_feature_set(), &config);
    assert!(
        buy_baseline.verdict_buy,
        "control fixture should remain BUY"
    );

    let mut buy_with_sybil = base_feature_set();
    buy_with_sybil.sybil_resistance = neutral_sybil.clone();
    let buy_neutral = evaluate(buy_with_sybil, &config);
    assert_eq!(buy_baseline.verdict_buy, buy_neutral.verdict_buy);
    assert_eq!(buy_baseline.reason_chain, buy_neutral.reason_chain);
    assert_eq!(buy_neutral.sybil_policy.soft_points, 0);
    assert_eq!(buy_neutral.sybil_policy.soft_signals.format_flags(), "none");

    let mut reject_features = base_feature_set();
    reject_features.tx_intel_features.dev_has_sold = true;
    reject_features.tx_intel_features.dev_volume_ratio = 0.75;
    let reject_baseline = evaluate(reject_features.clone(), &config);
    assert!(
        !reject_baseline.verdict_buy,
        "control fixture should remain REJECT"
    );

    reject_features.sybil_resistance = neutral_sybil;
    let reject_neutral = evaluate(reject_features, &config);
    assert_eq!(reject_baseline.verdict_buy, reject_neutral.verdict_buy);
    assert_eq!(
        reject_baseline.hard_fail_reason,
        reject_neutral.hard_fail_reason
    );
    assert_eq!(reject_neutral.sybil_policy.soft_points, 0);
}

#[test]
fn high_dbia_with_high_ftdi_does_not_change_policy_verdict() {
    let mut config = policy_test_config();
    config.min_fee_topology_diversity_index = 0.25;
    config.max_dev_buyer_infrastructure_affinity = 0.60;
    config.soft_penalty_low_ftdi = 4;
    config.soft_penalty_high_dbia = 7;
    config.soft_penalty_high_dbia_low_ftdi_combo = 11;

    let baseline = evaluate(base_feature_set(), &config);
    assert!(baseline.verdict_buy, "control fixture should remain BUY");

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(1.0),
        dev_buyer_infrastructure_affinity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let evaluated = evaluate(features, &config);
    assert_eq!(baseline.verdict_buy, evaluated.verdict_buy);
    assert_eq!(baseline.reason_chain, evaluated.reason_chain);
    assert_eq!(evaluated.sybil_policy.soft_points, 7);
    assert_eq!(
        evaluated.sybil_policy.lead_signal,
        Some(SybilLeadSignal::HighDbia)
    );
    assert!(!evaluated
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::HighDbiaLowFtdi));
}

#[test]
fn sybil_bucket_requires_explicit_enable_to_affect_verdict() {
    let mut config = policy_test_config();
    config.min_spend_fraction_divergence = 0.30;
    config.min_demand_elasticity_score = 0.10;
    config.soft_penalty_low_sfd = 2;
    config.soft_penalty_inelastic_demand = 3;
    config.soft_penalty_low_des_low_sfd_combo = 2;
    config.max_sybil_soft_points = 1;

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        spend_fraction_divergence: Some(0.05),
        demand_elasticity_score: Some(-0.20),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let disabled = evaluate(features.clone(), &config);
    assert!(disabled.verdict_buy);
    assert_eq!(disabled.verdict_type, GatekeeperVerdictType::Buy);
    assert_eq!(disabled.sybil_policy.soft_points, 7);
    assert_eq!(
        disabled.sybil_policy.lead_signal,
        Some(SybilLeadSignal::LowDes)
    );
    assert!(disabled
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::LowDesLowSfd));

    config.enable_sybil_interference_layer = true;
    let enabled = evaluate(features, &config);
    assert!(!enabled.verdict_buy);
    assert_eq!(
        enabled.verdict_type,
        GatekeeperVerdictType::RejectSybilSoftExcess
    );
    assert!(enabled.reason_chain.contains("SYBIL_SOFT_FAIL"));
}

#[test]
fn sybil_combo_veto_requires_sybil_layer_enable() {
    let mut config = policy_test_config();
    config.min_fee_topology_diversity_index = 0.25;
    config.max_dev_buyer_infrastructure_affinity = 0.60;
    config.min_spend_fraction_divergence = 0.08;
    config.enable_sybil_combo_veto = true;

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.01),
        dev_buyer_infrastructure_affinity: Some(0.95),
        spend_fraction_divergence: Some(0.05),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let disabled = evaluate(features.clone(), &config);
    assert!(disabled.verdict_buy);
    assert_eq!(disabled.verdict_type, GatekeeperVerdictType::Buy);
    assert!(disabled
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::HighDbiaLowFtdiLowSfd));

    config.enable_sybil_interference_layer = true;
    let enabled = evaluate(features, &config);
    assert!(!enabled.verdict_buy);
    assert_eq!(
        enabled.verdict_type,
        GatekeeperVerdictType::RejectSybilInterference
    );
    assert!(enabled
        .reason_chain
        .contains("SYBIL_INTERFERENCE: pattern=HIGH_DBIA_LOW_FTDI_LOW_SFD"));
}

#[test]
fn degraded_sybil_metrics_do_not_score_even_with_active_penalties() {
    let mut config = policy_test_config();
    config.min_fee_topology_diversity_index = 0.25;
    config.max_signer_cross_pool_velocity = 0.25;
    config.soft_penalty_low_ftdi = 4;
    config.soft_penalty_high_cpv = 5;
    config.enable_sybil_interference_layer = true;
    config.max_sybil_soft_points = 1;

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.01),
        signer_cross_pool_velocity: Some(0.90),
        degraded_reasons: vec![
            FTDI_INSUFFICIENT_BUYS_REASON.to_string(),
            CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string(),
        ],
        buy_sample_count: 2,
        signer_sample_count: 2,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(decision.verdict_buy);
    assert_eq!(decision.sybil_policy.soft_points, 0);
    assert_eq!(decision.sybil_policy.soft_signals.format_flags(), "none");
    assert_eq!(decision.sybil_policy.lead_signal, None);
}

#[test]
fn zero_penalty_sybil_patterns_remain_telemetry_only_without_lead_signal() {
    let mut config = policy_test_config();
    config.min_spend_fraction_divergence = 0.30;
    config.min_demand_elasticity_score = 0.10;
    config.enable_sybil_interference_layer = true;

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        spend_fraction_divergence: Some(0.05),
        demand_elasticity_score: Some(-0.20),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(decision.verdict_buy);
    assert!(decision.sybil_policy.soft_signals.low_sfd);
    assert!(decision.sybil_policy.soft_signals.low_des);
    assert_eq!(decision.sybil_policy.soft_points, 0);
    assert_eq!(decision.sybil_policy.lead_signal, None);
    assert!(decision
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::LowDesLowSfd));
}

#[test]
fn partial_sfd_coverage_remains_actionable_when_value_is_present() {
    let mut config = policy_test_config();
    config.min_spend_fraction_divergence = 0.30;
    config.soft_penalty_low_sfd = 2;
    config.enable_sybil_interference_layer = true;
    config.max_sybil_soft_points = 1;

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        spend_fraction_divergence: Some(0.05),
        degraded_reasons: vec![SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectSybilSoftExcess
    );
    assert!(decision.sybil_policy.soft_signals.low_sfd);
    assert_eq!(decision.sybil_policy.soft_points, 2);
}

#[test]
fn stage_b_config_rejects_local_des_sfd_combo_with_legacy_soft_frozen() {
    let config = stage_b_policy_config();

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        spend_fraction_divergence: Some(0.05),
        demand_elasticity_score: Some(-0.20),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert_eq!(config.max_soft_points, 255);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectSybilSoftExcess
    );
    assert!(!decision.verdict_buy);
    assert_eq!(decision.sybil_policy.soft_points, 7);
    assert_eq!(decision.soft_points, 0);
    assert_eq!(
        decision.sybil_policy.lead_signal,
        Some(SybilLeadSignal::LowDes)
    );
    assert!(decision
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::LowDesLowSfd));
}

#[test]
fn stage_b_config_distinguishes_high_dbia_low_ftdi_from_high_dbia_high_ftdi() {
    let config = stage_b_policy_config();

    let mut high_ftdi_features = base_feature_set();
    high_ftdi_features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(1.0),
        dev_buyer_infrastructure_affinity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let mut low_ftdi_features = base_feature_set();
    low_ftdi_features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.01),
        dev_buyer_infrastructure_affinity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let high_ftdi = evaluate(high_ftdi_features, &config);
    let low_ftdi = evaluate(low_ftdi_features, &config);

    assert!(high_ftdi.verdict_buy);
    assert!(low_ftdi.verdict_buy);
    assert_eq!(high_ftdi.sybil_policy.soft_points, 1);
    assert_eq!(low_ftdi.sybil_policy.soft_points, 4);
    assert_eq!(
        high_ftdi.sybil_policy.lead_signal,
        Some(SybilLeadSignal::HighDbia)
    );
    assert_eq!(
        low_ftdi.sybil_policy.lead_signal,
        Some(SybilLeadSignal::HighDbiaLowFtdi)
    );
    assert!(!high_ftdi
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::HighDbiaLowFtdi));
    assert!(low_ftdi
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::HighDbiaLowFtdi));
}

#[test]
fn sybil_bucket_rejection_keeps_priority_over_legacy_soft_excess() {
    let mut config = stage_b_policy_config();
    config.max_soft_points = 1;

    let mut features = base_feature_set();
    features.tx_intel_features.interval_cv = 0.01;
    features.tx_intel_features.timing_entropy = 0.01;
    features.tx_intel_features.burst_ratio = 0.99;
    features.sybil_resistance = SybilResistanceFeatures {
        spend_fraction_divergence: Some(0.05),
        demand_elasticity_score: Some(-0.20),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectSybilSoftExcess
    );
    assert!(decision.soft_points > decision.effective_max_soft_points);
    assert!(
        decision.sybil_policy.soft_points
            > u16::from(decision.sybil_policy.effective_max_soft_points)
    );
}

#[test]
fn stage_c_config_treats_cpv_as_helper_without_combo_bonus() {
    let config = stage_c_policy_config();
    assert_eq!(config.soft_penalty_high_cpv, 1);
    assert_eq!(config.soft_penalty_high_cpv_low_des_combo, 0);

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        demand_elasticity_score: Some(-0.20),
        signer_cross_pool_velocity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(decision.verdict_buy);
    assert_eq!(decision.sybil_policy.soft_points, 4);
    assert!(decision.sybil_policy.soft_signals.high_cpv);
    assert_eq!(
        decision.sybil_policy.lead_signal,
        Some(SybilLeadSignal::LowDes)
    );
    assert!(decision
        .sybil_policy
        .interference_patterns
        .contains(&SybilInterferencePattern::HighCpvLowDes));
}

#[test]
fn stage_c_config_high_cpv_solo_stays_buy() {
    let config = stage_c_policy_config();

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        signer_cross_pool_velocity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(decision.verdict_buy);
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
    assert_eq!(decision.sybil_policy.soft_points, 1);
    assert_eq!(
        decision.sybil_policy.lead_signal,
        Some(SybilLeadSignal::HighCpv)
    );
    assert!(decision.sybil_policy.soft_signals.high_cpv);
    assert!(decision.sybil_policy.interference_patterns.is_empty());
}

#[test]
fn stage_c_config_cpv_can_tip_borderline_structural_case() {
    let stage_b = stage_b_policy_config();
    let stage_c = stage_c_policy_config();

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.01),
        dev_buyer_infrastructure_affinity: Some(0.95),
        spend_fraction_divergence: Some(0.05),
        signer_cross_pool_velocity: Some(0.95),
        degraded_reasons: vec![],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision_stage_b = evaluate(features.clone(), &stage_b);
    let decision_stage_c = evaluate(features, &stage_c);

    assert!(decision_stage_b.verdict_buy);
    assert_eq!(decision_stage_b.sybil_policy.soft_points, 6);
    assert!(!decision_stage_c.verdict_buy);
    assert_eq!(
        decision_stage_c.verdict_type,
        GatekeeperVerdictType::RejectSybilSoftExcess
    );
    assert_eq!(decision_stage_c.sybil_policy.soft_points, 7);
    assert!(decision_stage_c.reason_chain.contains("SYBIL_SOFT_FAIL"));
}

#[test]
fn stage_c_config_cpv_unavailable_stays_non_penalizing() {
    let config = stage_c_policy_config();

    let mut features = base_feature_set();
    features.sybil_resistance = SybilResistanceFeatures {
        fee_topology_diversity_index: Some(0.01),
        dev_buyer_infrastructure_affinity: Some(0.95),
        spend_fraction_divergence: Some(0.05),
        signer_cross_pool_velocity: Some(0.95),
        degraded_reasons: vec![CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string()],
        buy_sample_count: 24,
        signer_sample_count: 18,
        ..SybilResistanceFeatures::default()
    };

    let decision = evaluate(features, &config);
    assert!(decision.verdict_buy);
    assert_eq!(decision.sybil_policy.soft_points, 6);
    assert!(!decision.sybil_policy.soft_signals.high_cpv);
    assert!(!decision.sybil_policy.metric_degraded_reasons.is_empty());
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
        slot: 1,
        write_version: Some(receive_ts_ms),
        receive_ts_ms,
        receive_seq: receive_ts_ms,
        curve_finality: CurveFinality::Finalized,
        source: UpdateSource::GeyserAccountUpdate,
    }
}

fn seed_session_tx(session: &mut PoolObservationSession, tx: Arc<PoolTransaction>) {
    let ingress = session.ingest_transaction(tx);
    assert!(
        !matches!(ingress, GatekeeperIngressOutcome::DeadlineElapsed),
        "policy test seed unexpectedly hit deadline"
    );
}

fn evaluate_feature_policy(
    session: &mut PoolObservationSession,
    config: &GatekeeperV2Config,
) -> GatekeeperVerdict {
    session.begin_evaluation();
    let features = session.materialize_features();
    let verdict = {
        let buffer = session.gatekeeper_buffer_mut();
        buffer.prepare_feature_evaluation();
        buffer.evaluate_from_features(features, config)
    };

    if matches!(verdict, GatekeeperVerdict::PendingCurve) {
        session
            .gatekeeper_buffer_mut()
            .rollback_feature_evaluation();
        session.resume_accumulation();
    }

    verdict
}

#[test]
fn hard_filters_reject_every_supported_condition() {
    let config = policy_test_config();
    let cases: Vec<(
        &str,
        Box<dyn Fn(MaterializedFeatureSet) -> MaterializedFeatureSet>,
    )> = vec![
        (
            "dev_sold",
            Box::new(|mut features| {
                features.tx_intel_features.dev_has_sold = true;
                features
            }),
        ),
        (
            "sell_impact",
            Box::new(|mut features| {
                features.checkpoint_features.max_single_sell_impact_pct =
                    config.max_single_sell_impact_pct + 1.0;
                features
            }),
        ),
        (
            "tx_impact",
            Box::new(|mut features| {
                features.checkpoint_features.single_tx_max_price_impact_pct =
                    config.max_single_tx_price_impact_pct + 1.0;
                features
            }),
        ),
        (
            "price_change",
            Box::new(|mut features| {
                features
                    .checkpoint_features
                    .price_change_from_first_checkpoint_pct = 10_100.0;
                features
            }),
        ),
        (
            "market_cap",
            Box::new(|mut features| {
                features.account_features.market_cap_sol = 0.001;
                features
            }),
        ),
        (
            "hhi",
            Box::new(|mut features| {
                features.tx_intel_features.hhi = config.hard_fail_hhi + 0.05;
                features
            }),
        ),
        (
            "same_ms",
            Box::new(|mut features| {
                features.tx_intel_features.same_ms_tx_ratio =
                    config.hard_fail_same_ms_tx_ratio + 0.01;
                features
            }),
        ),
        (
            "top3",
            Box::new(|mut features| {
                features.tx_intel_features.top3_volume_pct =
                    config.hard_fail_top3_volume_pct + 0.01;
                features
            }),
        ),
        (
            "bot_timing",
            Box::new(|mut features| {
                features.session_metadata.observation_duration_ms =
                    config.hard_fail_bot_min_observation_ms + 100;
                features.tx_intel_features.interval_cv = 0.01;
                features.tx_intel_features.avg_interval_ms = 10.0;
                features
            }),
        ),
        (
            "failed_ratio",
            Box::new(|mut features| {
                features.tx_intel_features.failed_tx_count = 12;
                features
            }),
        ),
        (
            "slow_pool",
            Box::new(|mut features| {
                features.tx_intel_features.avg_interval_ms = config.max_avg_interval_ms + 1_000.0;
                features
            }),
        ),
    ];

    for (label, build_case) in cases {
        let decision = evaluate(build_case(base_feature_set()), &config);
        assert_eq!(
            decision.verdict_type,
            GatekeeperVerdictType::RejectHardFail,
            "case={label} should hard-fail"
        );
        assert!(!decision.verdict_buy, "case={label} should reject");
    }
}

#[test]
fn hard_fail_decision_preserves_phase_diagnostics() {
    let mut config = policy_test_config();
    config.min_market_cap_sol = 50.0;

    let assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectHardFail);
    assert!(decision.core1_passed);
    assert!(decision.core2_passed);
    assert!(!decision.core3_passed);
    assert_eq!(decision.soft_points, 0);
    assert!(decision.max_soft_points_possible > 0);
    assert_eq!(decision.effective_max_soft_points, config.max_soft_points);
}

#[test]
fn timeout_decision_uses_explicit_timeout_verdict() {
    let config = policy_test_config();
    let mut features = base_feature_set();
    features.tx_intel_features.tx_count = 2;
    features.tx_intel_features.unique_signers = 2;
    features.tx_intel_features.buy_count = 2;

    let assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let decision = build_timeout_decision_from_assessment(&assessment, &config);

    assert_eq!(decision.verdict_type, GatekeeperVerdictType::TimeoutPhase1);
    assert!(!decision.verdict_buy);
    assert!(!decision.core1_passed);
    assert!(!decision.core2_passed);
    assert!(!decision.core3_passed);
    assert!(decision.reason_chain.contains("tx=2/4"));
}

#[test]
fn timeout_decision_does_not_claim_phase1_timeout_after_phase1_passed() {
    let config = policy_test_config();
    let assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assert!(assessment.phase1_passed);

    let decision = build_timeout_decision_from_assessment(&assessment, &config);

    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::TimeoutDeadlineLowPhases
    );
    assert!(!decision.verdict_buy);
    assert!(decision.core1_passed);
    assert!(decision
        .reason_chain
        .starts_with("TIMEOUT_DEADLINE_LOW_PHASES:"));
    assert!(!decision
        .reason_chain
        .contains("TIMEOUT_PHASE1_INSUFFICIENT:"));
}

#[test]
fn fingerprint_thresholds_can_downgrade_preliminary_buy() {
    let mut config = policy_test_config();
    config.use_three_layer_decision = true;
    config.min_volume_gini = 0.20;
    config.min_avg_inner_ix_count_50tx = 11.5;
    config.min_fixed_size_buy_ratio = 0.216;
    config.max_early_slot_volume_dominance_buy = 0.301;
    config.max_early_top3_buy_volume_pct_3s = 0.71;

    let mut assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: None,
        flip_ratio_10s: None,
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: Some(9.0),
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: None,
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: Some(0.10),
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.45),
        early_top3_buy_volume_pct_3s: Some(0.72),
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    refresh_assessment_thresholds(&mut assessment, &config);
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(
        !assessment.phase4_passed,
        "fingerprint thresholds should fail phase 4"
    );
    assert!(
        !decision.verdict_buy,
        "fingerprint thresholds should reject BUY"
    );
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
}

#[test]
fn early_top3_fingerprint_threshold_can_downgrade_preliminary_buy() {
    let mut config = policy_test_config();
    config.use_three_layer_decision = true;
    config.max_early_top3_buy_volume_pct_3s = 0.71;

    let mut assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: None,
        flip_ratio_10s: None,
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: None,
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: None,
        early_top3_buy_volume_pct_3s: Some(0.72),
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    refresh_assessment_thresholds(&mut assessment, &config);
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!assessment.phase4_passed);
    assert!(!decision.verdict_buy);
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
}

#[test]
fn feature_snapshot_alpha_fingerprint_can_fail_phase4_without_post_attach() {
    let mut config = policy_test_config();
    config.use_three_layer_decision = true;
    config.max_early_top3_buy_volume_pct_3s = 0.71;

    let mut features = base_feature_set();
    features.alpha_fingerprint = AlphaFingerprintFeatures {
        avg_inner_ix_count_50tx: None,
        sell_buy_ratio: None,
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: None,
        early_top3_buy_volume_pct_3s: Some(0.72),
        fixed_size_buy_ratio: None,
        flipper_presence_ratio: None,
    };

    let assessment = build_assessment_from_features(
        features.clone(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let decision = evaluate(features, &config);

    assert!(assessment.early_fingerprint.is_none());
    assert!(!assessment.phase4_passed);
    assert!(!decision.verdict_buy);
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
}

#[test]
fn prosperity_filter_accepts_branch_b1_conviction_clean_sells() {
    let config = balanced_prosperity_config();
    let mut features = base_feature_set();
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.20);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.32),
        flip_ratio_10s: Some(0.18),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.10),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.70),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.verdict_buy);
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
    assert_eq!(decision.prosperity_filter.pass, Some(true));
    assert_eq!(decision.prosperity_filter.branch1_pass, Some(true));
    assert!(decision
        .prosperity_filter
        .matched_branches
        .contains(&"conviction_clean_sells"));
}

#[test]
fn prosperity_filter_accepts_branch_b2_large_cap_buy_dominance() {
    let config = balanced_prosperity_config();
    let mut features = base_feature_set();
    features.account_features.market_cap_sol = 58.0;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.18);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.10),
        flip_ratio_10s: Some(0.12),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.25),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.94),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.verdict_buy);
    assert_eq!(decision.prosperity_filter.branch2_pass, Some(true));
    assert!(decision
        .prosperity_filter
        .matched_branches
        .contains(&"large_cap_buy_dominance"));
}

#[test]
fn prosperity_filter_accepts_branch_b3_organic_structure() {
    let config = balanced_prosperity_config();
    let mut features = base_feature_set();
    features.tx_intel_features.hhi = 0.03;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.12);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.12);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.08),
        flip_ratio_10s: Some(0.11),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.22),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.60),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.verdict_buy);
    assert_eq!(decision.prosperity_filter.branch3_pass, Some(true));
    assert!(decision
        .prosperity_filter
        .matched_branches
        .contains(&"organic_structure"));
}

#[test]
fn prosperity_filter_rejects_when_no_balanced_branch_matches() {
    let config = balanced_prosperity_config();
    let mut features = base_feature_set();
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.20);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.07);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.12),
        flip_ratio_10s: Some(0.15),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.22),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.80),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectLowProsperity
    );
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::NoBalancedBranch)
    );
}

#[test]
fn prosperity_filter_rejects_high_cpv_before_branch_match() {
    let config = balanced_prosperity_config();
    let mut features = base_feature_set();
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.72);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.35),
        flip_ratio_10s: Some(0.10),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.08),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.95),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectLowProsperity
    );
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::HighSignerCrossPoolVelocity)
    );
}

#[test]
fn prosperity_overlay_accepts_large_cap_branch_when_overlay_passes() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.market_cap_sol = 58.0;
    features.account_features.bonding_progress = 0.72;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 85.0;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.18);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.14);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.10),
        flip_ratio_10s: Some(0.12),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.14),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.94),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.verdict_buy);
    assert_eq!(decision.prosperity_filter.overlay_pass, Some(true));
    assert_eq!(
        decision.prosperity_filter.overlay_branch2_price_change_pass,
        Some(true)
    );
    assert_eq!(
        decision.prosperity_filter.overlay_branch23_sell_buy_pass,
        Some(true)
    );
}

#[test]
fn prosperity_overlay_accepts_organic_branch_when_overlay_passes() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.bonding_progress = 0.60;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 110.0;
    features.tx_intel_features.hhi = 0.03;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.12);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.14);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.08),
        flip_ratio_10s: Some(0.11),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.15),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.60),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.verdict_buy);
    assert_eq!(decision.prosperity_filter.overlay_pass, Some(true));
    assert_eq!(
        decision.prosperity_filter.overlay_price_change_pass,
        Some(true)
    );
}

#[test]
fn prosperity_overlay_rejects_large_cap_branch_on_branch2_price_extension() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.market_cap_sol = 58.0;
    features.account_features.bonding_progress = 0.72;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 110.0;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.18);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.14);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.10),
        flip_ratio_10s: Some(0.12),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.14),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.94),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::AboveOverlayBranch2MaxPriceChange)
    );
}

#[test]
fn prosperity_overlay_rejects_matched_branch_on_high_bonding_progress() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.market_cap_sol = 58.0;
    features.account_features.bonding_progress = 0.88;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 85.0;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.18);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.14);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.10),
        flip_ratio_10s: Some(0.12),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.14),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.94),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::AboveOverlayMaxBondingProgress)
    );
}

#[test]
fn prosperity_overlay_rejects_matched_branch_on_low_fee_topology_diversity() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.market_cap_sol = 58.0;
    features.account_features.bonding_progress = 0.72;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 85.0;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.18);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.09);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.10),
        flip_ratio_10s: Some(0.12),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.14),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.94),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::BelowOverlayMinFeeTopologyDiversityIndex)
    );
}

#[test]
fn prosperity_overlay_rejects_organic_branch_on_high_sell_buy_ratio() {
    let config = strict_prosperity_overlay_config();
    let mut features = base_feature_set();
    features.account_features.bonding_progress = 0.60;
    features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct = 110.0;
    features.tx_intel_features.hhi = 0.03;
    features.sybil_resistance.signer_cross_pool_velocity = Some(0.12);
    features.sybil_resistance.fee_topology_diversity_index = Some(0.14);

    let mut assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: Some(0.08),
        flip_ratio_10s: Some(0.11),
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: None,
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.22),
        compute_unit_cluster_dominance: None,
        static_fee_profile_ratio: None,
        fixed_size_buy_ratio: None,
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: None,
        early_slot_volume_dominance_buy: Some(0.60),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(!decision.verdict_buy);
    assert_eq!(
        decision.prosperity_filter.reject_trigger,
        Some(ProsperityRejectTrigger::AboveOverlayMaxSellBuyRatio)
    );
}

#[test]
fn phase2_upper_bounds_can_reject_via_soft_signals() {
    let mut config = policy_test_config();
    config.max_interval_cv = 0.35;
    config.max_timing_entropy = 1.5;
    config.max_soft_points = 0;
    config.dev_unknown_max_soft_points = 0;

    let decision = evaluate(base_feature_set(), &config);

    assert!(
        !decision.verdict_buy,
        "upper timing bounds should reject BUY"
    );
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectSoftExcess
    );
    assert!(decision.soft_signals.high_interval_cv);
    assert!(decision.soft_signals.high_timing_entropy);
}

#[test]
fn bidirectional_fingerprint_bounds_can_fail_core2() {
    let mut config = policy_test_config();
    config.max_avg_inner_ix_count_50tx = 8.5;
    config.min_sell_buy_ratio = 0.20;
    config.max_sell_buy_ratio = 0.50;
    config.min_compute_unit_cluster_dominance = 0.10;
    config.max_compute_unit_cluster_dominance = 0.70;
    config.min_static_fee_profile_ratio = 0.10;
    config.max_static_fee_profile_ratio = 0.70;
    config.min_jito_tip_intensity = 0.10;
    config.max_jito_tip_intensity = 0.40;

    let mut assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    assessment.early_fingerprint = Some(EarlyFingerprintMetrics {
        block0_sniped_supply_pct: None,
        flip_ratio_10s: None,
        cu_price_p90_1s: None,
        cu_price_p90_10s: None,
        priority_fee_surge_slope: None,
        buyer_pre_balance_cv: None,
        avg_inner_ix_count_50tx: Some(9.0),
        avg_cpi_depth_50tx: None,
        sell_buy_ratio: Some(0.05),
        compute_unit_cluster_dominance: Some(0.75),
        static_fee_profile_ratio: Some(0.05),
        fixed_size_buy_ratio: Some(0.10),
        fixed_size_buy_ratio_1e4: None,
        flipper_presence_ratio: None,
        jito_tip_intensity: Some(0.50),
        early_slot_volume_dominance_buy: Some(0.20),
        early_top3_buy_volume_pct_3s: None,
        whale_reversal_ratio_top3: None,
        whale_reversal_ratio_top1: None,
        dev_paperhand_latency_ms: None,
        dev_sold_within_3s: None,
        dev_sold_within_5s: None,
        fingerprint_degraded: false,
        fingerprint_reason: None,
    });

    refresh_assessment_thresholds(&mut assessment, &config);
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(
        !assessment.phase4_passed,
        "fingerprint bounds should fail phase 4"
    );
    assert!(
        !decision.verdict_buy,
        "fingerprint bounds should reject BUY"
    );
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
}

#[test]
fn min_dev_tx_ratio_can_fail_core3() {
    let mut config = policy_test_config();
    config.min_dev_tx_ratio = 0.10;

    let decision = evaluate(base_feature_set(), &config);

    assert!(
        !decision.verdict_buy,
        "dev tx lower bound should reject BUY"
    );
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectCoreFail);
    assert!(!decision.core3_passed);
}

#[test]
fn policy_replay_is_deterministic() {
    let config = policy_test_config();
    let features = base_feature_set();
    let left = evaluate(features.clone(), &config);
    let right = evaluate(features, &config);

    assert_eq!(left.verdict_type, right.verdict_type);
    assert_eq!(left.verdict_buy, right.verdict_buy);
    assert_eq!(left.soft_points, right.soft_points);
    assert_eq!(left.reason_chain, right.reason_chain);
    assert_eq!(left.dev_unknown, right.dev_unknown);
}

#[test]
fn hard_filter_features_api_matches_assessment_snapshot() {
    let config = policy_test_config();
    let mut features = base_feature_set();
    features.tx_intel_features.dev_has_sold = true;

    let assessment = build_assessment_from_features(
        features.clone(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let hard_filter = evaluate_hard_filters(&features, &config);

    assert_eq!(
        hard_filter.as_ref().map(|(reason, _)| *reason),
        Some(ghost_launcher::components::gatekeeper_policy::HardFailReason::DevSold)
    );
    assert_eq!(
        hard_filter.as_ref().map(|(_, reason)| reason.as_str()),
        assessment.hard_reject_reason.as_deref()
    );
}

#[test]
fn verdict_engine_buys_when_core_pass_holds_and_soft_signals_stay_within_limit() {
    let config = policy_test_config();
    let assessment = build_assessment_from_features(
        base_feature_set(),
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.core1_passed);
    assert!(decision.core2_passed);
    assert!(decision.core3_passed);
    assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
    assert!(decision.verdict_buy);
    assert!(decision.soft_points <= decision.effective_max_soft_points);
}

#[test]
fn verdict_engine_rejects_soft_excess_while_core_pass_still_holds() {
    let mut config = policy_test_config();
    config.max_soft_points = 1;

    let mut features = base_feature_set();
    features.tx_intel_features.interval_cv = 0.01;
    features.tx_intel_features.timing_entropy = 0.01;
    features.tx_intel_features.burst_ratio = 0.99;

    let assessment = build_assessment_from_features(
        features,
        &config,
        PolicyEvaluationContext {
            finalize_lag_ms: 0,
            eval_count: 1,
        },
    );
    let decision = evaluate_policy_from_assessment(&assessment, &config);

    assert!(decision.core1_passed);
    assert!(decision.core2_passed);
    assert!(decision.core3_passed);
    assert_eq!(
        decision.verdict_type,
        GatekeeperVerdictType::RejectSoftExcess
    );
    assert!(!decision.verdict_buy);
    assert!(decision.soft_points > decision.effective_max_soft_points);
}

#[test]
fn session_features_drive_policy_buy() {
    let config = policy_test_config();
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let created_at_wall_ms = current_wall_ms();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(Pubkey::new_unique()),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(created_at_wall_ms.saturating_add(5_000)),
            gatekeeper_config: config.clone(),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");

    let signers: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
    let mut guard = session.write();
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);
    guard.checkpoint_engine.config.interval_ms = 1;
    guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        28_000_000_000,
        980_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(pool_id, signers[0], "sig-1", 1_020, 0.5, 28.0, 980_000.0),
    );
    guard.try_checkpoint(1_020);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_120,
        29_000_000_000,
        940_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(pool_id, signers[1], "sig-2", 1_120, 0.8, 29.0, 940_000.0),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(pool_id, signers[2], "sig-3", 1_240, 0.9, 30.0, 920_000.0),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(pool_id, signers[3], "sig-4", 1_360, 1.1, 31.0, 900_000.0),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(pool_id, signers[4], "sig-5", 1_520, 1.2, 32.0, 880_000.0),
    );
    guard.try_checkpoint(1_520);

    let features = guard.materialize_features();
    guard.begin_evaluation();
    let verdict = {
        let buffer = guard.gatekeeper_buffer_mut();
        buffer.prepare_feature_evaluation();
        buffer.evaluate_from_features(features.clone(), &config)
    };
    match verdict {
        GatekeeperVerdict::Buy { assessment, .. } => {
            assert_eq!(
                assessment.feature_snapshot.tx_intel_features.tx_count,
                features.tx_intel_features.tx_count
            );
            assert_eq!(assessment.checkpoint_count, guard.checkpoints.len() as u32);
            assert_eq!(
                assessment.trajectory_available,
                features.checkpoint_features.trajectory_assessment.is_some()
            );
            assert_eq!(
                assessment.trajectory.is_some(),
                features.checkpoint_features.trajectory_assessment.is_some()
            );
            assert!(assessment.v25_confidence.is_some());
        }
        _ => panic!("expected Buy verdict"),
    }
}

#[test]
fn feature_policy_buys_seeded_flow_without_account_updates_when_curve_data_is_known() {
    let config = policy_test_config();
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();
    let created_at_wall_ms = current_wall_ms();

    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(created_at_wall_ms.saturating_add(5_000)),
            gatekeeper_config: config.clone(),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signers: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();

    let mut guard = session.write();
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[0],
            "bootstrap-buy-sig-1",
            1_020,
            0.5,
            28.0,
            980_000.0,
        ),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[1],
            "bootstrap-buy-sig-2",
            1_120,
            0.8,
            29.0,
            940_000.0,
        ),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[2],
            "bootstrap-buy-sig-3",
            1_240,
            0.9,
            30.0,
            920_000.0,
        ),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[3],
            "bootstrap-buy-sig-4",
            1_360,
            1.1,
            31.0,
            900_000.0,
        ),
    );
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[4],
            "bootstrap-buy-sig-5",
            1_520,
            1.2,
            32.0,
            880_000.0,
        ),
    );

    let features = guard.materialize_features();
    assert_eq!(features.account_features.update_count, 0);
    assert!(features.curve_readiness.curve_data_known);
    assert!(
        features.account_features.market_cap_sol > config.min_market_cap_sol,
        "bootstrap fallback should expose positive market cap, got {}",
        features.account_features.market_cap_sol
    );

    let policy_verdict = evaluate_feature_policy(&mut guard, &config);
    match policy_verdict {
        GatekeeperVerdict::Buy { assessment, .. } => {
            let decision = assessment
                .decision
                .expect("feature policy should attach decision");
            assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
            assert!(assessment.feature_snapshot.account_features.market_cap_sol > 0.0);
        }
        GatekeeperVerdict::Reject { reason, .. } => {
            panic!("unexpected reject without account updates: {reason}")
        }
        GatekeeperVerdict::Timeout { .. } => {
            panic!("unexpected timeout without account updates")
        }
        GatekeeperVerdict::Wait
        | GatekeeperVerdict::PendingCurve
        | GatekeeperVerdict::ApprovedTx { .. } => {
            panic!("unexpected non-terminal verdict")
        }
    }
}

#[test]
fn feature_policy_buys_seeded_organic_flow() {
    let config = policy_test_config();
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();
    let created_at_wall_ms = current_wall_ms();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(created_at_wall_ms.saturating_add(5_000)),
            gatekeeper_config: config.clone(),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signers: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();

    let mut guard = session.write();
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);
    guard.checkpoint_engine.config.interval_ms = 1;
    guard.checkpoint_engine.config.min_tx_between_checkpoints = 1;

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        28_000_000_000,
        980_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[0],
            "legacy-buy-sig-1",
            1_020,
            0.5,
            28.0,
            980_000.0,
        ),
    );
    guard.try_checkpoint(1_020);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_120,
        29_000_000_000,
        940_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[1],
            "legacy-buy-sig-2",
            1_120,
            0.8,
            29.0,
            940_000.0,
        ),
    );
    guard.try_checkpoint(1_120);
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[2],
            "legacy-buy-sig-3",
            1_240,
            0.9,
            30.0,
            920_000.0,
        ),
    );
    guard.try_checkpoint(1_240);
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[3],
            "legacy-buy-sig-4",
            1_360,
            1.1,
            31.0,
            900_000.0,
        ),
    );
    guard.try_checkpoint(1_360);
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signers[4],
            "legacy-buy-sig-5",
            1_520,
            1.2,
            32.0,
            880_000.0,
        ),
    );
    guard.try_checkpoint(1_520);

    let policy_verdict = evaluate_feature_policy(&mut guard, &config);

    match policy_verdict {
        GatekeeperVerdict::Buy { assessment, .. } => {
            assert_eq!(
                assessment
                    .decision
                    .expect("feature policy should attach decision")
                    .verdict_type,
                GatekeeperVerdictType::Buy
            );
        }
        _ => panic!("feature path should BUY on the same seeded flow"),
    }
}

#[test]
fn feature_policy_rejects_dev_sell_after_seeded_flow() {
    let config = policy_test_config();
    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();
    let created_at_wall_ms = current_wall_ms();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(created_at_wall_ms.saturating_add(5_000)),
            gatekeeper_config: config.clone(),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signer_two = Pubkey::new_unique();
    let signer_three = Pubkey::new_unique();

    let mut guard = session.write();
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);
    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        28_000_000_000,
        980_000_000,
    ));

    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            dev_wallet,
            "legacy-sig-0",
            1_100,
            0.8,
            28.0,
            980_000.0,
        ),
    );
    guard.try_checkpoint(1_100);

    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signer_two,
            "legacy-sig-1",
            1_220,
            0.9,
            29.0,
            960_000.0,
        ),
    );
    guard.try_checkpoint(1_220);

    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signer_three,
            "legacy-sig-2",
            1_340,
            1.1,
            30.0,
            940_000.0,
        ),
    );
    guard.try_checkpoint(1_340);

    let sell_tx = Arc::new(PoolTransaction {
        is_buy: false,
        ..(*curve_tx(
            pool_id,
            dev_wallet,
            "legacy-sig-3",
            1_460,
            0.7,
            29.5,
            930_000.0,
        ))
        .clone()
    });
    seed_session_tx(&mut guard, sell_tx);
    guard.try_checkpoint(1_460);

    let policy_verdict = evaluate_feature_policy(&mut guard, &config);

    assert!(matches!(policy_verdict, GatekeeperVerdict::Reject { .. }));
}

#[test]
fn feature_policy_rejects_sell_impact_after_seeded_flow() {
    let mut config = policy_test_config();
    config.max_single_sell_impact_pct = 15.0;

    let manager = SessionManager::default();
    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let dev_wallet = Pubkey::new_unique();
    let created_at_wall_ms = current_wall_ms();
    manager
        .open_session(OpenSessionRequest {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            dev_wallet: Some(dev_wallet),
            candidate_snapshot: candidate(pool_id, base_mint, bonding_curve),
            created_at_wall_ms,
            deadline_wall_ms: Some(created_at_wall_ms.saturating_add(5_000)),
            gatekeeper_config: config.clone(),
            fingerprint_config: EarlyFingerprintConfig::default(),
        })
        .expect("session should open");
    let session = manager.get_session(&pool_id).expect("session should exist");
    let signer_two = Pubkey::new_unique();
    let signer_three = Pubkey::new_unique();
    let signer_four = Pubkey::new_unique();

    let mut guard = session.write();
    guard
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_010,
        28_000_000_000,
        980_000_000,
    ));

    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            dev_wallet,
            "impact-sig-0",
            1_100,
            0.8,
            28.0,
            980_000.0,
        ),
    );
    guard.try_checkpoint(1_100);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_180,
        30_000_000_000,
        940_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signer_two,
            "impact-sig-1",
            1_220,
            0.9,
            30.0,
            940_000.0,
        ),
    );
    guard.try_checkpoint(1_220);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_300,
        31_000_000_000,
        920_000_000,
    ));
    seed_session_tx(
        &mut guard,
        curve_tx(
            pool_id,
            signer_three,
            "impact-sig-2",
            1_340,
            1.1,
            31.0,
            920_000.0,
        ),
    );
    guard.try_checkpoint(1_340);

    guard.on_account_update(&account_update(
        pool_id,
        base_mint,
        bonding_curve,
        1_420,
        10_000_000_000,
        1_020_000_000,
    ));
    let sell_tx = Arc::new(PoolTransaction {
        is_buy: false,
        ..(*curve_tx(
            pool_id,
            signer_four,
            "impact-sig-3",
            1_460,
            2.5,
            10.0,
            1_020_000.0,
        ))
        .clone()
    });
    seed_session_tx(&mut guard, sell_tx);
    guard.try_checkpoint(1_460);

    let features = guard.materialize_features();
    guard.begin_evaluation();
    let policy_verdict = {
        let buffer = guard.gatekeeper_buffer_mut();
        buffer.prepare_feature_evaluation();
        buffer.evaluate_from_features(features, &config)
    };
    assert!(
        matches!(policy_verdict, GatekeeperVerdict::Reject { .. }),
        "expected policy reject"
    );
}
