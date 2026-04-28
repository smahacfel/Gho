//! Test for configurable scoring system
//!
//! Validates that ScoringWeights can be loaded from GhostBrainConfig
//! and that modifying weights in the config actually affects the final score.

use ghost_brain::config::{GhostBrainConfig, ScoringWeightsConfig};
use ghost_brain::oracle::hyper_prediction::scoring::ScoringWeights;

#[test]
fn test_scoring_weights_default() {
    // Default config should have None for scoring section
    let config = GhostBrainConfig::default();
    assert!(
        config.scoring.is_none(),
        "Default config should have no scoring section"
    );

    // Loading from default config should return default weights
    let weights = ScoringWeights::from_config(&config);

    // All multipliers should be 1.0 by default
    assert_eq!(weights.wash_penalty_mult, 1.0);
    assert_eq!(weights.bot_penalty_mult, 1.0);
    assert_eq!(weights.organic_boost_mult, 1.0);
    assert_eq!(weights.ssmi_viral_boost_mult, 1.0);
}

#[test]
fn test_scoring_weights_from_config() {
    // Create a config with custom scoring weights
    let mut config = GhostBrainConfig::default();
    let mut scoring_config = ScoringWeightsConfig::default();

    // Modify specific weights to test values
    scoring_config.wash_penalty_mult = 1.5;
    scoring_config.organic_boost_mult = 0.8;
    scoring_config.ssmi_viral_boost_mult = 2.0;
    scoring_config.bot_penalty_mult = 0.5;

    config.scoring = Some(scoring_config);

    // Load weights from config
    let weights = ScoringWeights::from_config(&config);

    // Verify the custom values are loaded
    assert_eq!(
        weights.wash_penalty_mult, 1.5,
        "wash_penalty_mult should be loaded from config"
    );
    assert_eq!(
        weights.organic_boost_mult, 0.8,
        "organic_boost_mult should be loaded from config"
    );
    assert_eq!(
        weights.ssmi_viral_boost_mult, 2.0,
        "ssmi_viral_boost_mult should be loaded from config"
    );
    assert_eq!(
        weights.bot_penalty_mult, 0.5,
        "bot_penalty_mult should be loaded from config"
    );

    // Verify unmodified values remain at default
    assert_eq!(
        weights.rug_penalty_mult, 1.0,
        "unmodified values should remain at default"
    );
    assert_eq!(
        weights.cluster_penalty_mult, 1.0,
        "unmodified values should remain at default"
    );
}

#[test]
fn test_scoring_weights_validation() {
    let mut scoring = ScoringWeightsConfig::default();

    // Valid config should pass
    assert!(scoring.validate().is_ok(), "Default config should be valid");

    // Negative penalty mult should fail
    scoring.wash_penalty_mult = -0.5;
    assert!(
        scoring.validate().is_err(),
        "Negative wash_penalty_mult should fail validation"
    );

    // Reset and test negative boost mult
    scoring.wash_penalty_mult = 1.0;
    scoring.organic_boost_mult = -1.0;
    assert!(
        scoring.validate().is_err(),
        "Negative organic_boost_mult should fail validation"
    );

    // Reset and test infinite value
    scoring.organic_boost_mult = 1.0;
    scoring.bot_penalty_mult = f32::INFINITY;
    assert!(
        scoring.validate().is_err(),
        "Infinite bot_penalty_mult should fail validation"
    );
}

#[test]
fn test_scoring_weights_zero_disable() {
    // Setting a multiplier to 0.0 should effectively disable that penalty/boost
    let mut config = GhostBrainConfig::default();
    let mut scoring_config = ScoringWeightsConfig::default();

    // Disable wash trading penalty
    scoring_config.wash_penalty_mult = 0.0;
    scoring_config.ssmi_bot_penalty_mult = 0.0;

    config.scoring = Some(scoring_config);

    let weights = ScoringWeights::from_config(&config);

    assert_eq!(
        weights.wash_penalty_mult, 0.0,
        "wash penalty should be disabled"
    );
    assert_eq!(
        weights.ssmi_bot_penalty_mult, 0.0,
        "ssmi bot penalty should be disabled"
    );

    // Validation should still pass (0.0 is non-negative)
    assert!(config.scoring.as_ref().unwrap().validate().is_ok());
}

#[test]
fn test_ghost_brain_config_includes_scoring() {
    // Test that GhostBrainConfig properly includes and validates scoring section
    let mut config = GhostBrainConfig::default();

    // Add valid scoring config
    let scoring = ScoringWeightsConfig::default();
    config.scoring = Some(scoring);

    // Should validate successfully
    assert!(
        config.validate().is_ok(),
        "Config with valid scoring should validate"
    );

    // Test with invalid scoring config
    let mut invalid_scoring = ScoringWeightsConfig::default();
    invalid_scoring.wash_penalty_mult = -1.0;
    config.scoring = Some(invalid_scoring);

    // Should fail validation
    assert!(
        config.validate().is_err(),
        "Config with invalid scoring should fail validation"
    );
}

#[test]
fn test_scoring_config_toml_serialization() {
    // Test that ScoringWeightsConfig can be serialized to/from TOML
    let scoring = ScoringWeightsConfig {
        wash_penalty_mult: 1.5,
        bot_penalty_mult: 0.8,
        organic_boost_mult: 1.2,
        ssmi_viral_boost_mult: 2.0,
        ..Default::default()
    };

    // Serialize to TOML
    let toml_str = toml::to_string(&scoring).expect("Should serialize to TOML");

    // Should contain our custom values
    assert!(
        toml_str.contains("wash_penalty_mult = 1.5"),
        "TOML should contain wash_penalty_mult"
    );
    assert!(
        toml_str.contains("bot_penalty_mult = 0.8"),
        "TOML should contain bot_penalty_mult"
    );

    // Deserialize back
    let deserialized: ScoringWeightsConfig =
        toml::from_str(&toml_str).expect("Should deserialize from TOML");

    // Values should match
    assert_eq!(deserialized.wash_penalty_mult, 1.5);
    assert_eq!(deserialized.bot_penalty_mult, 0.8);
    assert_eq!(deserialized.organic_boost_mult, 1.2);
}
