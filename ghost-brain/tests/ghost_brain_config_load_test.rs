use ghost_brain::config::GhostBrainConfig;

#[test]
fn test_production_toml_loads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ghost_brain_config.toml");
    let config = GhostBrainConfig::from_toml_file(&path).expect("production config should load");
    assert!(config.validate().is_ok());
}
