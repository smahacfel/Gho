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
    assert_eq!(config.gatekeeper_v3.policy_version, 1);
    assert_eq!(config.gatekeeper_v3.materialization_version, 1);
    assert!(!config.gatekeeper_v3.promotion.enabled);
    assert_eq!(config.gatekeeper_v3.thresholds.min_tx_count, 12);
    assert_eq!(config.gatekeeper_v3.thresholds.min_unique_signers, 8);
    assert_eq!(config.gatekeeper_v3.thresholds.min_buy_count, 6);
    assert_eq!(
        config.gatekeeper_v3.thresholds.execution_not_run_confidence_cap,
        0.80
    );
}
