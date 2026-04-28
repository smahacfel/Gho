//! Resonance Detector Usage Example
//!
//! This example demonstrates how to use the ResonanceDetector to identify
//! bot trading patterns vs. human trading patterns in real-time.
//!
//! Run with: cargo run --example resonance_detector_demo

use ghost_brain::signals::{ActivityClassification, ResonanceConfig, ResonanceDetector};

fn main() {
    println!("=== Resonance Detector Demo ===\n");

    // Example 1: Detecting Bot Trading Patterns
    println!("Example 1: Bot Trading Pattern Detection");
    println!("-----------------------------------------");

    let mut bot_detector = ResonanceDetector::new();

    // Simulate bot trades: highly periodic (every 500ms)
    println!("Adding periodic bot-like trades (every 500ms)...");
    for i in 0..20 {
        bot_detector.add_timestamp(i * 500);
    }

    let bot_result = bot_detector.analyze();
    println!("Analysis Results:");
    println!("  Classification: {:?}", bot_result.classification);
    println!(
        "  Resonance Score: {:.4} (1.0 = perfectly periodic)",
        bot_result.resonance_score
    );
    println!(
        "  Coefficient of Variation: {:.4} (lower = more periodic)",
        bot_result.coefficient_variation
    );
    println!("  Mean Interval: {:.2}ms", bot_result.mean_interval_ms);
    println!("  Std Deviation: {:.2}ms", bot_result.std_dev_ms);
    println!("  Is Bot Likely: {}", bot_result.is_bot_likely());
    println!();

    // Example 2: Detecting Human Trading Patterns
    println!("Example 2: Human Trading Pattern Detection");
    println!("------------------------------------------");

    let mut human_detector = ResonanceDetector::new();

    // Simulate human trades: random intervals
    let human_intervals = vec![
        250, 1500, 800, 3200, 450, 2100, 600, 4500, 1200, 350, 2800, 900, 1800, 500, 3500, 700,
        1000, 2500, 400, 1600,
    ];

    println!("Adding random human-like trades...");
    let mut timestamp = 0;
    for interval in human_intervals {
        timestamp += interval;
        human_detector.add_timestamp(timestamp);
    }

    let human_result = human_detector.analyze();
    println!("Analysis Results:");
    println!("  Classification: {:?}", human_result.classification);
    println!("  Resonance Score: {:.4}", human_result.resonance_score);
    println!(
        "  Coefficient of Variation: {:.4}",
        human_result.coefficient_variation
    );
    println!("  Mean Interval: {:.2}ms", human_result.mean_interval_ms);
    println!("  Std Deviation: {:.2}ms", human_result.std_dev_ms);
    println!("  Is Human Likely: {}", human_result.is_human_likely());
    println!();

    // Example 3: Suspicious Pattern (Semi-periodic with noise)
    println!("Example 3: Suspicious Pattern Detection");
    println!("---------------------------------------");

    let mut suspicious_detector = ResonanceDetector::new();

    // Simulate suspicious pattern: mostly periodic with some variation
    println!("Adding semi-periodic suspicious trades...");
    let base_interval = 1000;
    for i in 0..20 {
        let variation = (i % 4) * 150; // Some variation
        suspicious_detector.add_timestamp(i * base_interval + variation);
    }

    let suspicious_result = suspicious_detector.analyze();
    println!("Analysis Results:");
    println!("  Classification: {:?}", suspicious_result.classification);
    println!(
        "  Resonance Score: {:.4}",
        suspicious_result.resonance_score
    );
    println!(
        "  Coefficient of Variation: {:.4}",
        suspicious_result.coefficient_variation
    );
    println!(
        "  Mean Interval: {:.2}ms",
        suspicious_result.mean_interval_ms
    );
    println!("  Std Deviation: {:.2}ms", suspicious_result.std_dev_ms);
    println!("  Is Suspicious: {}", suspicious_result.is_suspicious());
    println!();

    // Example 4: Custom Configuration
    println!("Example 4: Custom Configuration");
    println!("-------------------------------");

    let custom_config = ResonanceConfig {
        buffer_size: 128,        // Larger buffer for more history
        bot_threshold_cv: 0.25,  // Stricter bot detection
        human_threshold_cv: 0.9, // More lenient human classification
        min_samples: 10,         // More samples required
    };

    let mut custom_detector = ResonanceDetector::with_config(custom_config);

    println!("Custom config: buffer=128, bot_cv<0.25, human_cv>0.9, min_samples=10");

    // Add bot-like pattern
    for i in 0..15 {
        custom_detector.add_timestamp(i * 750);
    }

    let custom_result = custom_detector.analyze();
    println!("Analysis Results:");
    println!("  Classification: {:?}", custom_result.classification);
    println!(
        "  Coefficient of Variation: {:.4}",
        custom_result.coefficient_variation
    );
    println!();

    // Example 5: Real-time Monitoring Simulation
    println!("Example 5: Real-time Monitoring Simulation");
    println!("------------------------------------------");

    let mut realtime_detector = ResonanceDetector::new();

    println!("Simulating real-time trade monitoring...");

    // Simulate trades coming in over time
    let incoming_trades = vec![
        (1000, "Initial trade"),
        (1500, "500ms later"),
        (2000, "500ms later"),
        (2500, "500ms later"),
        (3000, "500ms later"),
        (3500, "500ms later"),
        (4000, "500ms later"),
        (4500, "500ms later"),
        (5000, "500ms later"),
        (5500, "500ms later"),
    ];

    for (timestamp, description) in incoming_trades {
        realtime_detector.add_timestamp(timestamp);

        // Analyze after each trade
        let result = realtime_detector.analyze();

        match result.classification {
            ActivityClassification::Insufficient => {
                println!(
                    "  [{}ms] {}: Insufficient data (need {} samples)",
                    timestamp, description, result.sample_count
                );
            }
            ActivityClassification::BotLikely => {
                println!(
                    "  [{}ms] {}: ⚠️ BOT DETECTED! CV={:.4}",
                    timestamp, description, result.coefficient_variation
                );
            }
            ActivityClassification::Suspicious => {
                println!(
                    "  [{}ms] {}: ⚠️ Suspicious pattern. CV={:.4}",
                    timestamp, description, result.coefficient_variation
                );
            }
            ActivityClassification::HumanLikely => {
                println!(
                    "  [{}ms] {}: ✓ Human-like pattern. CV={:.4}",
                    timestamp, description, result.coefficient_variation
                );
            }
        }
    }

    println!();
    println!("=== Demo Complete ===");
    println!("\nKey Insights:");
    println!("• Low CV (< 0.3) = Bot-like behavior (highly periodic)");
    println!("• Medium CV (0.3-0.8) = Suspicious (semi-periodic)");
    println!("• High CV (> 0.8) = Human-like behavior (random)");
    println!("• Resonance Score = Inverse of CV (1.0 = perfect periodicity)");
}
