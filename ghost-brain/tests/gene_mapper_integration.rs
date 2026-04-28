//! Gene Mapper Integration Tests
//!
//! This module tests the complete integration of the Gene Mapper with the Watchdog
//! supervisor loop, demonstrating TASK 3.2 completion.

use ghost_brain::guardian::{WatchdogConfig, WatchdogDecision, WatchdogSignal};
use ghost_brain::security::{GeneMapper, GeneMapperConfig, RiskLevel};

#[test]
fn test_gene_mapper_standalone_clean_code() {
    // Test Gene Mapper with clean bytecode
    let mapper = GeneMapper::new();
    let clean_code = vec![0x01, 0x02, 0x03, 0x04, 0x05];

    let result = mapper.analyze(&clean_code);

    assert_eq!(result.risk_level, RiskLevel::Safe);
    assert_eq!(result.risk_score, 0.0);
    assert!(!result.is_high_risk());
    assert!(result.detected_patterns.is_empty());
    assert_eq!(result.recommended_action(), "PROCEED");
}

#[test]
fn test_gene_mapper_standalone_malicious_freeze() {
    // Test Gene Mapper with FreezeAccount instruction
    let mapper = GeneMapper::new();
    let malicious_code = vec![0x0e, 0x01, 0x02]; // FreezeAccount + padding

    let result = mapper.analyze(&malicious_code);

    assert!(result.risk_score > 0.0);
    assert!(!result.detected_patterns.is_empty());
    assert!(result
        .detected_patterns
        .iter()
        .any(|p| p.name == "FreezeAccount"));
    println!("Detected: {}", result.threat_summary);
}

#[test]
fn test_gene_mapper_standalone_rug_pull_pattern() {
    // Test Gene Mapper with classic rug pull pattern
    let mapper = GeneMapper::new();
    let rug_pull_code = vec![0x0e, 0x06]; // FreezeAccount + SetAuthority

    let result = mapper.analyze(&rug_pull_code);

    assert!(result.is_high_risk());
    assert!(result
        .detected_patterns
        .iter()
        .any(|p| p.name == "FreezeAndSeize"));
    assert_eq!(result.recommended_action(), "ABORT");
    println!("Rug pull detected: {}", result.threat_summary);
}

#[test]
fn test_gene_mapper_custom_config() {
    // Test Gene Mapper with custom configuration
    let mut config = GeneMapperConfig::default();
    config.max_scan_depth = 100;
    config.min_severity_threshold = 0.5;

    let mapper = GeneMapper::with_config(config);

    // Low severity pattern (should be filtered)
    let low_severity = vec![0x08]; // Burn (severity 0.5)
    let result = mapper.analyze(&low_severity);

    // Should still detect patterns at exactly threshold
    assert!(!result.detected_patterns.is_empty() || result.risk_score >= 0.5);
}

#[test]
fn test_gene_mapper_large_bytecode() {
    // Test with larger bytecode
    let mapper = GeneMapper::new();

    let mut large_code = vec![0x00; 1000];
    // Insert dangerous pattern in the middle
    large_code[500] = 0x06; // SetAuthority
    large_code[501] = 0x03; // Transfer

    let result = mapper.analyze(&large_code);

    assert!(result.risk_score > 0.0);
    assert!(result
        .detected_patterns
        .iter()
        .any(|p| p.name == "AuthorityHijack"));
}

#[tokio::test]
async fn test_gene_mapper_with_watchdog_clean() {
    // Test Gene Mapper integration with Watchdog - clean bytecode
    use ghost_brain::guardian::watchdog::run_watchdog;
    use tokio::sync::mpsc;

    let mut config = WatchdogConfig::default();
    config.enable_parallel_tasks = true;
    config.max_void_duration_ms = 500;

    let (tx, rx) = mpsc::channel(10);

    let handle =
        tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

    // Give time for Gene Mapper task to spawn and complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Send external success
    tx.send(WatchdogSignal::ExternalResult {
        success: true,
        data: "External check passed".to_string(),
    })
    .await
    .unwrap();

    let decision = handle.await.unwrap().unwrap();

    // With clean bytecode, Gene Mapper should not abort
    assert_eq!(decision, WatchdogDecision::Proceed);
}

#[tokio::test]
async fn test_gene_mapper_with_watchdog_timeout() {
    // Test that watchdog times out properly
    use ghost_brain::guardian::watchdog::run_watchdog;
    use tokio::sync::mpsc;

    let mut config = WatchdogConfig::default();
    config.enable_parallel_tasks = false; // Disable for predictable timeout
    config.max_void_duration_ms = 100;

    let (_tx, rx) = mpsc::channel(10);

    let decision = run_watchdog(config, rx, None, None, None, None)
        .await
        .unwrap();

    assert_eq!(decision, WatchdogDecision::Timeout);
}

#[test]
fn test_gene_mapper_performance() {
    // Benchmark Gene Mapper performance
    use std::time::Instant;

    let mapper = GeneMapper::new();
    let test_code = vec![0x06, 0x03, 0x07, 0x08, 0x09]; // Multiple patterns

    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = mapper.analyze(&test_code);
    }

    let elapsed = start.elapsed();
    let avg_us = elapsed.as_micros() / iterations;

    println!(
        "Gene Mapper performance: {} iterations in {:?}",
        iterations, elapsed
    );
    println!("Average time per analysis: {}μs", avg_us);

    // Should be fast (< 500μs per analysis)
    assert!(avg_us < 500, "Gene Mapper too slow: {}μs > 500μs", avg_us);
}

#[test]
fn test_gene_mapper_quick_check_performance() {
    // Test quick_check performance
    use std::time::Instant;

    let mapper = GeneMapper::new();
    let test_code = vec![0xff; 1000]; // Large bytecode

    let start = Instant::now();
    let iterations = 10000;

    for _ in 0..iterations {
        let _ = mapper.quick_check(&test_code);
    }

    let elapsed = start.elapsed();
    let avg_us = elapsed.as_micros() / iterations;

    println!(
        "Quick check performance: {} iterations in {:?}",
        iterations, elapsed
    );
    println!("Average time per check: {}μs", avg_us);

    // Quick check should be very fast (< 100μs)
    assert!(avg_us < 100, "Quick check too slow: {}μs > 100μs", avg_us);
}

#[test]
fn test_all_risk_levels() {
    // Test all risk level classifications
    let mapper = GeneMapper::new();

    // Safe
    let safe = mapper.analyze(&[0x01, 0x02, 0x03]);
    assert_eq!(safe.risk_level, RiskLevel::Safe);

    // Low (single low-severity pattern)
    let low = mapper.analyze(&[0x08]); // Burn (0.5 severity)
    assert!(matches!(low.risk_level, RiskLevel::Low | RiskLevel::Medium));

    // High (multiple patterns or high-severity pattern)
    let high = mapper.analyze(&[0x0e, 0x06]); // FreezeAndSeize (1.0 severity)
    assert_eq!(high.risk_level, RiskLevel::High);
}

#[test]
fn test_gene_mapper_empty_bytecode() {
    // Test with empty bytecode
    let mapper = GeneMapper::new();
    let empty = vec![];

    let result = mapper.analyze(&empty);

    assert_eq!(result.risk_level, RiskLevel::Safe);
    assert_eq!(result.bytes_scanned, 0);
}

#[test]
fn test_gene_mapper_pattern_count_bonus() {
    // Test that multiple patterns increase risk score
    let mapper = GeneMapper::new();

    // Single pattern
    let single = vec![0x06]; // SetAuthority
    let single_result = mapper.analyze(&single);

    // Multiple patterns
    let multiple = vec![0x06, 0x07, 0x08, 0x09]; // SetAuthority, MintTo, Burn, CloseAccount
    let multiple_result = mapper.analyze(&multiple);

    // Multiple patterns should have higher risk
    assert!(
        multiple_result.risk_score > single_result.risk_score,
        "Multiple patterns ({}) should have higher risk than single ({})",
        multiple_result.risk_score,
        single_result.risk_score
    );

    println!("Single pattern risk: {}", single_result.risk_score);
    println!("Multiple patterns risk: {}", multiple_result.risk_score);
}

#[test]
fn test_gene_mapper_hash_calculation() {
    // Test that different bytecode produces different hashes
    let mapper = GeneMapper::new();

    let code1 = vec![0x01, 0x02, 0x03];
    let code2 = vec![0x04, 0x05, 0x06];

    let result1 = mapper.analyze(&code1);
    let result2 = mapper.analyze(&code2);

    assert_ne!(result1.program_hash, result2.program_hash);

    // Same bytecode should produce same hash
    let result1_again = mapper.analyze(&code1);
    assert_eq!(result1.program_hash, result1_again.program_hash);
}
