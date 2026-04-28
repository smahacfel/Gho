//! MCI Smoke Tests
//!
//! Basic integration tests to verify MCI engine compiles and runs.

use ghost_brain::{MarketSignals, MciConfig, MciEngine};

#[test]
fn smoke_mci() {
    let config = MciConfig::default();
    let engine = MciEngine::new(config);
    let signals = MarketSignals::mock();
    let result = engine.compute_mci(&signals);

    // Basic validation - placeholder returns 0.0 values
    assert!(result.mci >= 0.0);
    assert!(result.dc >= 0.0);
    assert!(result.sc >= 0.0);
}

#[test]
fn test_mci_config_default() {
    let config = MciConfig::default();
    assert_eq!(config.version, 1);
    assert!(config.weight_dc > 0.0);
    assert!(config.weight_sc > 0.0);
}

#[test]
fn test_mci_with_custom_config() {
    let mut config = MciConfig::default();
    config.weight_dc = 0.7;
    config.weight_sc = 0.3;

    let engine = MciEngine::new(config);
    let signals = MarketSignals::mock();
    let result = engine.compute_mci(&signals);

    // Should still return valid result structure
    assert!(result.mci >= 0.0);
}

#[test]
fn test_mci_abort_threshold() {
    let config = MciConfig::default();
    let engine = MciEngine::new(config.clone());
    let signals = MarketSignals::mock();
    let result = engine.compute_mci(&signals);

    // Check abort threshold logic
    let should_abort = result.should_abort(config.coherence_abort_threshold);
    assert!(should_abort); // Placeholder returns 0.0, below threshold 0.3
}
