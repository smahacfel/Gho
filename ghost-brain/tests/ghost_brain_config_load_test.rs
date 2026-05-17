use ghost_brain::config::GhostBrainConfig;

#[test]
fn test_production_toml_loads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");
    assert!(config.validate().is_ok());
}

#[test]
fn gatekeeper_v3_config_loads_from_production_toml() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");

    assert!(!config.gatekeeper_v3.enabled);
    assert!(config.gatekeeper_v3.shadow_emit_enabled);
    assert!(!config.gatekeeper_v3.replay_payload_enabled);
    assert_eq!(config.gatekeeper_v3.policy_version, 1);
    assert_eq!(config.gatekeeper_v3.materialization_version, 1);
    assert!(!config.gatekeeper_v3.promotion.enabled);
    let gatekeeper_v2 = config
        .gatekeeper_v2
        .as_ref()
        .expect("production config should include gatekeeper_v2");
    assert_eq!(gatekeeper_v2.min_market_cap_sol, 41.0);
    assert_eq!(config.gatekeeper_v3.normal.min_tx_count, 12);
    assert_eq!(config.gatekeeper_v3.normal.min_unique_signers, 8);
    assert_eq!(config.gatekeeper_v3.normal.min_buy_count, 6);
    assert_eq!(config.gatekeeper_v3.extended.min_tx_count, 12);
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
