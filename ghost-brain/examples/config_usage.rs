//! Example: Loading and Using Ghost Brain Configuration
//!
//! This example demonstrates how to load, customize, and validate
//! the Ghost Brain unified configuration system.
//!
//! Run with:
//! ```bash
//! cargo run --example config_usage
//! ```

use ghost_brain::config::GhostBrainConfig;

fn main() -> anyhow::Result<()> {
    println!("=== Ghost Brain Configuration Example ===\n");

    // 1. Create default configuration
    println!("1. Loading default configuration...");
    let default_config = GhostBrainConfig::default();
    println!("   ✓ Default config loaded");
    println!(
        "   - MPCF bot_entropy_threshold: {}",
        default_config.mpcf.bot_entropy_threshold
    );
    println!(
        "   - SOBP hyper_pump_threshold: {}",
        default_config.sobp.hyper_pump_threshold
    );
    println!(
        "   - QASS collapse_threshold: {}\n",
        default_config.qass.collapse_threshold
    );

    // 2. Validate default configuration
    println!("2. Validating default configuration...");
    default_config.validate()?;
    println!("   ✓ Default configuration is valid\n");

    // 3. Create custom configuration
    println!("3. Creating custom configuration for aggressive trading...");
    let mut aggressive_config = GhostBrainConfig::default();

    // Adjust MPCF for more lenient bot detection
    aggressive_config.mpcf.bot_entropy_threshold = 4.0;
    aggressive_config.mpcf.human_entropy_threshold = 5.0;

    // Adjust SOBP for earlier pump detection
    aggressive_config.sobp.hyper_pump_threshold = 2.5;
    aggressive_config.sobp.growth_threshold = 1.3;
    aggressive_config.sobp.human_weight_multiplier = 1.5;

    // Adjust IWIM for less strict rug detection
    aggressive_config.iwim.iapp_rug_threshold = 3;
    aggressive_config.iwim.min_iapp_rug_score = 0.90;

    println!("   ✓ Aggressive config created");
    println!(
        "   - MPCF bot_entropy_threshold: {}",
        aggressive_config.mpcf.bot_entropy_threshold
    );
    println!(
        "   - SOBP hyper_pump_threshold: {}",
        aggressive_config.sobp.hyper_pump_threshold
    );
    println!(
        "   - IWIM iapp_rug_threshold: {}\n",
        aggressive_config.iwim.iapp_rug_threshold
    );

    // 4. Validate custom configuration
    println!("4. Validating aggressive configuration...");
    aggressive_config.validate()?;
    println!("   ✓ Aggressive configuration is valid\n");

    // 5. Save configurations to files
    println!("5. Saving configurations to files...");

    // Save default config
    default_config.to_json_file("/tmp/ghost_brain_default.json")?;
    default_config.to_toml_file("/tmp/ghost_brain_default.toml")?;
    println!("   ✓ Saved default config to:");
    println!("     - /tmp/ghost_brain_default.json");
    println!("     - /tmp/ghost_brain_default.toml");

    // Save aggressive config
    aggressive_config.to_json_file("/tmp/ghost_brain_aggressive.json")?;
    aggressive_config.to_toml_file("/tmp/ghost_brain_aggressive.toml")?;
    println!("   ✓ Saved aggressive config to:");
    println!("     - /tmp/ghost_brain_aggressive.json");
    println!("     - /tmp/ghost_brain_aggressive.toml\n");

    // 6. Load configuration from file
    println!("6. Loading configuration from file...");
    let loaded_config = GhostBrainConfig::from_json_file("/tmp/ghost_brain_aggressive.json")?;
    println!("   ✓ Loaded aggressive config from JSON");
    println!(
        "   - SOBP hyper_pump_threshold: {}",
        loaded_config.sobp.hyper_pump_threshold
    );

    let loaded_toml_config = GhostBrainConfig::from_toml_file("/tmp/ghost_brain_aggressive.toml")?;
    println!("   ✓ Loaded aggressive config from TOML");
    println!(
        "   - SOBP hyper_pump_threshold: {}\n",
        loaded_toml_config.sobp.hyper_pump_threshold
    );

    // 7. Create conservative configuration
    println!("7. Creating custom configuration for conservative trading...");
    let mut conservative_config = GhostBrainConfig::default();

    // Adjust MPCF for stricter bot detection
    conservative_config.mpcf.bot_entropy_threshold = 3.0;
    conservative_config.mpcf.human_entropy_threshold = 6.0;

    // Adjust SOBP for later pump detection
    conservative_config.sobp.hyper_pump_threshold = 3.5;
    conservative_config.sobp.growth_threshold = 1.8;
    conservative_config.sobp.human_weight_multiplier = 2.5;

    // Adjust IWIM for stricter rug detection
    conservative_config.iwim.iapp_rug_threshold = 1;
    conservative_config.iwim.min_iapp_rug_score = 0.98;

    // Adjust QASS for higher entry bars
    conservative_config.qass.score_threshold_moderate = 0.75;
    conservative_config.qass.score_threshold_viral = 0.90;

    println!("   ✓ Conservative config created");
    println!(
        "   - MPCF bot_entropy_threshold: {}",
        conservative_config.mpcf.bot_entropy_threshold
    );
    println!(
        "   - SOBP hyper_pump_threshold: {}",
        conservative_config.sobp.hyper_pump_threshold
    );
    println!(
        "   - IWIM iapp_rug_threshold: {}",
        conservative_config.iwim.iapp_rug_threshold
    );
    println!(
        "   - QASS score_threshold_moderate: {}\n",
        conservative_config.qass.score_threshold_moderate
    );

    // 8. Validate conservative configuration
    println!("8. Validating conservative configuration...");
    conservative_config.validate()?;
    println!("   ✓ Conservative configuration is valid\n");

    // 9. Demonstrate invalid configuration
    println!("9. Testing validation with invalid configuration...");
    let mut invalid_config = GhostBrainConfig::default();
    invalid_config.mpcf.bot_entropy_threshold = 15.0; // Invalid: > 10.0

    match invalid_config.validate() {
        Ok(_) => println!("   ✗ Validation should have failed!"),
        Err(e) => println!("   ✓ Validation correctly rejected invalid config: {}\n", e),
    }

    // 10. Summary
    println!("=== Summary ===");
    println!("✓ Successfully demonstrated Ghost Brain configuration system");
    println!("✓ Created and validated default, aggressive, and conservative configs");
    println!("✓ Saved and loaded configurations from JSON and TOML files");
    println!("✓ Validated invalid configurations are properly rejected");
    println!("\nConfiguration files created in /tmp/:");
    println!("  - ghost_brain_default.json");
    println!("  - ghost_brain_default.toml");
    println!("  - ghost_brain_aggressive.json");
    println!("  - ghost_brain_aggressive.toml");

    Ok(())
}
