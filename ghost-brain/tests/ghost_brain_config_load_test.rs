use ghost_brain::{
    config::GhostBrainConfig,
    guardian::post_buy::{validate_het_pm_v2_config, CrashGuardMode, HetPmV2Mode},
};

#[test]
fn test_production_toml_loads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");
    assert!(config.validate().is_ok());
}

#[test]
fn post_buy_guardian_lifecycle_thresholds_load_from_production_toml() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");

    assert_eq!(config.post_buy_guardian.target_threshold, Some(50.0));
    assert_eq!(config.post_buy_guardian.stoploss_threshold, Some(50.0));
    assert_eq!(config.post_buy_guardian.wait_for_timestop, Some(30_000));
    assert_eq!(config.post_buy_guardian.wait_for_timestop_ms(), 30_000);
    assert_eq!(
        config.post_buy_guardian.exit_policy_v1.quote_recovery_ms,
        5_000
    );
    let policy = &config.post_buy_guardian.exit_policy_v1;
    assert!(policy.absolute_max_hold_enabled);
    assert_eq!(policy.absolute_max_hold_ms, 120_000);
    assert_eq!(policy.crash_guard_mode, CrashGuardMode::ObserveOnly);
    assert_eq!(policy.crash_window_ms, 1_500);
    assert_eq!(policy.crash_min_short_window_drop_pct, 25.0);
    assert_eq!(policy.crash_min_peak_drawdown_pct, 30.0);
    assert_eq!(policy.crash_min_distinct_slots, 2);
    assert_eq!(policy.crash_max_sample_age_ms, 1_500);
    assert_eq!(policy.crash_max_executable_return_pct, -20.0);
    let het = &config.post_buy_guardian.het_pm_v2;
    assert!(het.enabled);
    assert_eq!(het.mode, HetPmV2Mode::ObserveOnly);
    assert_eq!(het.trajectory_short_ms, 1_500);
    assert_eq!(het.trajectory_medium_ms, 5_000);
    assert_eq!(het.trajectory_long_ms, 15_000);
    assert_eq!(het.max_newest_sample_age_ms, 1_500);
    assert_eq!(het.trailing_arm_mark_return_bps, 2_500);
    assert_eq!(het.trailing_mark_candidate_drawdown_bps, 1_500);
    assert_eq!(het.trailing_executable_breach_bps, 1_800);
    assert_eq!(het.peak_anchor_min_step_bps, 500);
    assert_eq!(het.peak_anchor_force_refresh_on_new_peak_after_ms, 5_000);
    assert_eq!(het.vitality_min_age_ms, 11_000);
    assert_eq!(het.vitality_required_non_alive_windows, 3);
    assert_eq!(het.vitality_min_time_since_peak_ms, 5_000);
    assert_eq!(het.vitality_recovery_return_bps, 300);
    let status = validate_het_pm_v2_config(&config.post_buy_guardian)
        .expect("production HET-PM V2 config should validate");
    assert!(status.v1_shadow_authority);
    assert!(!status.v2_shadow_authority);
    assert!(!status.live_authority);
    assert_eq!(status.crash_guard_mode, CrashGuardMode::ObserveOnly);
    assert!(!config.post_buy_guardian.aem.enabled);
}

#[test]
fn post_buy_guardian_lifecycle_thresholds_load_from_r41_selector_config() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("R41 config should load");

    assert_eq!(config.post_buy_guardian.target_threshold, Some(50.0));
    assert_eq!(config.post_buy_guardian.stoploss_threshold, Some(50.0));
    assert_eq!(config.post_buy_guardian.wait_for_timestop, Some(30_000));
}

#[test]
fn gatekeeper_v3_config_loads_from_production_toml() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");

    assert!(config.gatekeeper_v3.enabled);
    assert!(config.gatekeeper_v3.shadow_emit_enabled);
    assert!(config.gatekeeper_v3.replay_payload_enabled);
    assert_eq!(config.gatekeeper_v3.policy_version, 1);
    assert_eq!(config.gatekeeper_v3.materialization_version, 1);
    assert!(!config.gatekeeper_v3.promotion.enabled);
    let gatekeeper_v2 = config
        .gatekeeper_v2
        .as_ref()
        .expect("production config should include gatekeeper_v2");
    assert_eq!(gatekeeper_v2.min_market_cap_sol, 115.0);
    assert_eq!(config.gatekeeper_v3.normal.min_tx_count, 4);
    assert_eq!(config.gatekeeper_v3.normal.min_unique_signers, 3);
    assert_eq!(config.gatekeeper_v3.normal.min_buy_count, 2);
    assert_eq!(config.gatekeeper_v3.extended.min_tx_count, 4);
    assert!(!config.gatekeeper_v3.evidence_requirements.tx_segments);
    assert!(!config.gatekeeper_v3.evidence_requirements.fsc);
    assert!(!config.gatekeeper_v3.evidence_requirements.execution);
    assert_eq!(config.gatekeeper_v3.confidence_caps.execution_not_run, 0.80);
    assert_eq!(
        config.gatekeeper_v3.component_weights.max_risk_penalty,
        0.85
    );
}

#[test]
fn gatekeeper_v3_replay_payload_enabled_in_p32_replay_config() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../configs/rollout/ghost_brain_v3_p32_replay.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("P3.2 replay config should load");

    assert!(!config.gatekeeper_v3.enabled);
    assert!(config.gatekeeper_v3.shadow_emit_enabled);
    assert!(config.gatekeeper_v3.replay_payload_enabled);
    assert!(!config.gatekeeper_v3.promotion.enabled);
}

#[test]
fn gatekeeper_v3_p36_primary_only_descopes_fsc_forward_only() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../configs/rollout/ghost_brain_v3_p36_primary_only.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("P3.6 config should load");

    assert!(!config.gatekeeper_v3.enabled);
    assert!(config.gatekeeper_v3.shadow_emit_enabled);
    assert!(config.gatekeeper_v3.replay_payload_enabled);
    assert!(!config.gatekeeper_v3.promotion.enabled);
    assert!(!config.gatekeeper_v3.evidence_requirements.fsc);
}

#[test]
fn gatekeeper_v3_p37_mfs_lifecycle_collection_descopes_fsc_forward_only() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../configs/rollout/ghost_brain_v3_p37_mfs_lifecycle.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("P3.7-J config should load");

    assert!(!config.gatekeeper_v3.enabled);
    assert!(config.gatekeeper_v3.shadow_emit_enabled);
    assert!(config.gatekeeper_v3.replay_payload_enabled);
    assert!(!config.gatekeeper_v3.promotion.enabled);
    assert!(!config.gatekeeper_v3.evidence_requirements.fsc);
    assert!(!config.gatekeeper_v3.evidence_requirements.execution);
}
