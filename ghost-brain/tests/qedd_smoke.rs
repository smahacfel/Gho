//! QEDD Smoke Tests
//!
//! Basic integration tests to verify QEDD engine compiles and runs.

use ghost_brain::{MarketSignals, QeddConfig, QeddEngine};

#[test]
fn smoke_qedd() {
    let config = QeddConfig::default();
    let engine = QeddEngine::new(config);
    let signals = MarketSignals::mock();
    let result = engine.compute_qedd(&signals, None);

    // Basic validation - placeholder returns 0.0 values
    let result = result.expect("QEDD should return Some result");
    assert!(result.lambda_now >= 0.0);
    assert!(result.survival_1s >= 0.0);
    assert!(result.survival_5s >= 0.0);
    assert!(result.survival_30s >= 0.0);
    assert!(result.survival_60s >= 0.0);
}

#[test]
fn test_qedd_config_default() {
    let config = QeddConfig::default();
    assert_eq!(config.version, 1);
    assert!(config.lambda_base > 0.0);
    assert!(config.lambda_sensitivity >= 0.0);
}

#[test]
fn test_qedd_with_custom_config() {
    let mut config = QeddConfig::default();
    config.lambda_base = 0.8;
    config.lambda_sensitivity = 0.2;

    let engine = QeddEngine::new(config);
    let signals = MarketSignals::mock();
    let result = engine.compute_qedd(&signals, None);

    // Should still return valid result structure
    let result = result.expect("QEDD should return Some result");
    assert!(result.lambda_now >= 0.0);
}

#[test]
fn test_qedd_abort_threshold() {
    let config = QeddConfig::default();
    let engine = QeddEngine::new(config.clone());
    let signals = MarketSignals::mock();
    let result = engine.compute_qedd(&signals, None);

    // Check abort threshold logic
    let result = result.expect("QEDD should return Some result");
    let should_abort = result.should_abort(config.lambda_abort_threshold);
    assert!(!should_abort); // Placeholder returns 0.0, should not abort
}
