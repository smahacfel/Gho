//! Integration Test: Oracle Decision Logger
//!
//! This test demonstrates the complete decision logging flow:
//! 1. Initial scoring with component breakdowns
//! 2. Follow-up scores at 1s, 5s, 30s, 60s intervals
//! 3. Corrections with explicit reasons
//! 4. JSONL output validation
//!
//! Run with: cargo test -p ghost-brain --test oracle_decision_logger_integration -- --nocapture

use ghost_brain::oracle::{
    CorrectionReason, DecisionLogger, DecisionLoggerConfig, DecisionType, FollowupScore,
    InitialComponents, OracleDecisionLog, VetoType,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::fs;
use tokio::time::sleep;

#[tokio::test]
async fn test_oracle_decision_logger_complete_flow() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║           Oracle Decision Logger - Complete Flow Test                  ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    // Setup: Create temp directory for test
    let temp_dir = TempDir::new().unwrap();
    println!("📁 Test directory: {:?}", temp_dir.path());
    println!();

    // Setup: Create logger with test config
    let config = DecisionLoggerConfig {
        log_dir: temp_dir.path().to_path_buf(),
        gatekeeper_log_dir: temp_dir.path().to_path_buf(),
        channel_buffer_size: 100,
        enabled: true,
    };

    let logger = DecisionLogger::new(config);

    // Step 1: Create initial score
    println!("📋 Step 1: Creating initial decision log with components...");

    let mut extras = HashMap::new();
    extras.insert("ssmi_entropy".to_string(), 0.42);
    extras.insert("mpcf_confidence".to_string(), 0.85);
    extras.insert("sobp_score".to_string(), 0.68);

    let initial_components = InitialComponents {
        base_shadow: 60,
        qass_score: 78.5,
        qedd_survival_30s: Some(0.71),
        mci: Some(0.74),
        chaos_loss_prob: Some(0.12),
        gene_match_score: Some(0.03),
        confidence: Some(0.9),
        extras,
    };

    let mut decision_log = OracleDecisionLog::new(
        "pool_test_12345".to_string(),
        62,
        DecisionType::Buy,
        initial_components,
    );

    println!("✅ Initial score: {}", decision_log.initial_score);
    println!("   Decision: {:?}", decision_log.initial_decision);
    println!("   Components:");
    println!(
        "     - base_shadow: {}",
        decision_log.initial_components.base_shadow
    );
    println!(
        "     - qass_score: {:.2}",
        decision_log.initial_components.qass_score
    );
    println!(
        "     - qedd_survival_30s: {:.2}",
        decision_log
            .initial_components
            .qedd_survival_30s
            .unwrap_or(0.0)
    );
    println!(
        "     - mci: {:.2}",
        decision_log.initial_components.mci.unwrap_or(0.0)
    );
    println!();

    // Step 2: Add follow-up score at 1s (minimal change)
    println!("📋 Step 2: Adding follow-up score at 1s...");

    let followup_1s = FollowupScore {
        t_ms: 1000,
        score: 60,
        reason: "Small QASS fluctuation".to_string(),
        corrections: vec![CorrectionReason::QassScoreDrop {
            old_score: 78.5,
            new_score: 76.0,
            drop_pct: 3.2,
            impact: -2,
        }],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(followup_1s);
    println!("✅ 1s score: 60 (Hold) - Small QASS drop");
    println!();

    // Step 3: Add follow-up score at 5s (MCI drop detected)
    println!("📋 Step 3: Adding follow-up score at 5s with MCI drop...");

    let mci_correction = CorrectionReason::MciDrop {
        old_value: 0.74,
        new_value: 0.45,
        threshold: 0.50,
        impact: -15,
    };

    let qedd_correction = CorrectionReason::QeddSurvivalDrop {
        old_survival: 0.71,
        new_survival: 0.52,
        horizon_s: 30,
        impact: -8,
    };

    let followup_5s = FollowupScore {
        t_ms: 5000,
        score: 45,
        reason: "MCI drop below threshold + QEDD survival decline".to_string(),
        corrections: vec![mci_correction, qedd_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(followup_5s);
    println!("✅ 5s score: 45 (Sell) - MCI drop & QEDD decline");
    println!("   Corrections applied: 2");
    println!("     1. MCI drop: 0.74 → 0.45 (impact: -15)");
    println!("     2. QEDD survival: 0.71 → 0.52 (impact: -8)");
    println!();

    // Step 4: Add follow-up score at 30s (QEDD λ spike)
    println!("📋 Step 4: Adding follow-up score at 30s with QEDD λ spike...");

    let lambda_correction = CorrectionReason::QeddLambdaSpike {
        old_lambda: 0.5,
        new_lambda: 3.2,
        threshold: 2.0,
        impact: -25,
    };

    let followup_30s = FollowupScore {
        t_ms: 30000,
        score: 20,
        reason: "Critical: QEDD λ spike indicates rapid decay".to_string(),
        corrections: vec![lambda_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(followup_30s);
    println!("✅ 30s score: 20 (Sell) - Critical QEDD λ spike");
    println!("   λ: 0.5 → 3.2 (threshold: 2.0)");
    println!();

    // Step 5: Add follow-up score at 60s (recovery attempt, but still low)
    println!("📋 Step 5: Adding follow-up score at 60s...");

    let followup_60s = FollowupScore {
        t_ms: 60000,
        score: 25,
        reason: "Slight recovery, but still below threshold".to_string(),
        corrections: vec![],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(followup_60s);
    println!("✅ 60s score: 25 (Hold) - Monitoring for further changes");
    println!();

    // Step 6: Mark as completed
    decision_log.complete();
    println!("📋 Step 6: Marking decision log as completed");
    println!();

    // Step 7: Write to JSONL
    println!("📋 Step 7: Writing decision log to JSONL...");
    logger.log(decision_log.clone()).await;

    // Give time for async write
    sleep(Duration::from_millis(200)).await;

    // Step 8: Verify file exists and read content
    println!("📋 Step 8: Verifying JSONL output...");

    let log_path = temp_dir
        .path()
        .join("pool_test_12345")
        .join("decision.jsonl");

    assert!(log_path.exists(), "Decision log file should exist");

    let content = fs::read_to_string(&log_path).await.unwrap();
    println!("✅ Log file created at: {:?}", log_path);
    println!();

    // Verify JSON structure
    println!("📋 Step 9: Validating JSON structure...");
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["candidate_id"], "pool_test_12345");
    assert_eq!(parsed["initialScore"], 62);
    assert_eq!(parsed["initial_decision"], "BUY");

    // Verify follow-up scores
    let followups = parsed["followupScores"].as_array().unwrap();
    assert_eq!(followups.len(), 4, "Should have 4 follow-up scores");

    // Check 1s score
    assert_eq!(followups[0]["t_ms"], 1000);
    assert_eq!(followups[0]["score"], 60);
    assert_eq!(followups[0]["decision"], "HOLD");

    // Check 5s score with corrections
    assert_eq!(followups[1]["t_ms"], 5000);
    assert_eq!(followups[1]["score"], 45);
    assert_eq!(followups[1]["decision"], "SELL");
    let corrections_5s = followups[1]["corrections"].as_array().unwrap();
    assert_eq!(corrections_5s.len(), 2, "Should have 2 corrections at 5s");

    // Check 30s score
    assert_eq!(followups[2]["t_ms"], 30000);
    assert_eq!(followups[2]["score"], 20);

    // Check 60s score
    assert_eq!(followups[3]["t_ms"], 60000);
    assert_eq!(followups[3]["score"], 25);

    // Verify total corrections
    assert_eq!(parsed["total_corrections"], 3);
    assert_eq!(parsed["final_decision"], "HOLD");
    assert!(
        parsed["completed_at"].as_u64().is_some(),
        "Should have completion timestamp"
    );

    println!("✅ All validations passed!");
    println!();

    // Print sample of JSONL output
    println!("📄 Sample JSONL output (first 500 chars):");
    println!("{}", &content[..content.len().min(500)]);
    println!("...");
    println!();

    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                       Test Completed Successfully                       ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
}

#[tokio::test]
async fn test_veto_scenario() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║              Oracle Decision Logger - Veto Scenario                    ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    let temp_dir = TempDir::new().unwrap();
    let config = DecisionLoggerConfig {
        log_dir: temp_dir.path().to_path_buf(),
        gatekeeper_log_dir: temp_dir.path().to_path_buf(),
        channel_buffer_size: 100,
        enabled: true,
    };

    let logger = DecisionLogger::new(config);

    // Create decision with high initial score
    let initial_components = InitialComponents {
        base_shadow: 85,
        qass_score: 92.0,
        qedd_survival_30s: Some(0.89),
        mci: Some(0.91),
        chaos_loss_prob: Some(0.05),
        gene_match_score: Some(0.82), // HIGH - scam detected
        confidence: Some(0.4),
        extras: HashMap::new(),
    };

    let mut decision_log = OracleDecisionLog::new(
        "pool_scam_veto".to_string(),
        90,
        DecisionType::Buy,
        initial_components,
    );

    println!("📋 Initial score: 90 (BUY)");
    println!("   ⚠️  GeneMapper match score: 0.82 (HIGH)");
    println!();

    // GeneMapper veto triggered
    println!("🚨 GeneMapper veto triggered!");
    decision_log.set_veto(VetoType::Gene, DecisionType::Skip);

    let gene_correction = CorrectionReason::GeneMapperHit {
        match_score: 0.82,
        pattern_id: "pump_dump_pattern_v2".to_string(),
        impact: -100,
    };

    let veto_followup = FollowupScore {
        t_ms: 500,
        score: 0,
        reason: "VETO: GeneMapper detected known scam pattern".to_string(),
        corrections: vec![gene_correction],
        decision: DecisionType::Skip,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(veto_followup);
    decision_log.complete();

    println!("✅ Final decision: SKIP (Gene veto)");
    println!();

    logger.log(decision_log).await;
    sleep(Duration::from_millis(200)).await;

    let log_path = temp_dir
        .path()
        .join("pool_scam_veto")
        .join("decision.jsonl");
    let content = fs::read_to_string(&log_path).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["veto"], "gene");
    assert_eq!(parsed["final_decision"], "SKIP");

    println!("✅ Veto scenario validated!");
    println!();
}

#[tokio::test]
async fn test_guardian_abort_scenario() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║          Oracle Decision Logger - Guardian Abort Scenario              ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    let temp_dir = TempDir::new().unwrap();
    let config = DecisionLoggerConfig {
        log_dir: temp_dir.path().to_path_buf(),
        gatekeeper_log_dir: temp_dir.path().to_path_buf(),
        channel_buffer_size: 100,
        enabled: true,
    };

    let logger = DecisionLogger::new(config);

    let initial_components = InitialComponents {
        base_shadow: 70,
        qass_score: 75.0,
        qedd_survival_30s: Some(0.65),
        mci: Some(0.68),
        chaos_loss_prob: Some(0.15),
        gene_match_score: Some(0.05),
        confidence: Some(0.6),
        extras: HashMap::new(),
    };

    let mut decision_log = OracleDecisionLog::new(
        "pool_guardian_abort".to_string(),
        70,
        DecisionType::Buy,
        initial_components,
    );

    println!("📋 Initial score: 70 (BUY)");
    println!();

    // Guardian abort after 2s
    println!("🛡️  Guardian watchdog abort at 2s");
    decision_log.set_veto(VetoType::Guardian, DecisionType::Sell);

    let guardian_correction = CorrectionReason::GuardianAbort {
        reason: "Anomalous transaction pattern detected".to_string(),
        signal_name: "chaos_engine_critical".to_string(),
        impact: -100,
    };

    let abort_followup = FollowupScore {
        t_ms: 2000,
        score: 0,
        reason: "ABORT: Guardian watchdog detected anomaly".to_string(),
        corrections: vec![guardian_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };

    decision_log.add_followup(abort_followup);
    decision_log.complete();

    println!("✅ Emergency sell executed");
    println!();

    logger.log(decision_log).await;
    sleep(Duration::from_millis(200)).await;

    let log_path = temp_dir
        .path()
        .join("pool_guardian_abort")
        .join("decision.jsonl");
    let content = fs::read_to_string(&log_path).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["veto"], "guardian");
    assert_eq!(parsed["final_decision"], "SELL");

    println!("✅ Guardian abort scenario validated!");
    println!();
}

#[tokio::test]
async fn test_multiple_candidates_concurrent() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║       Oracle Decision Logger - Concurrent Candidates Test              ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    let temp_dir = TempDir::new().unwrap();
    let config = DecisionLoggerConfig {
        log_dir: temp_dir.path().to_path_buf(),
        gatekeeper_log_dir: temp_dir.path().to_path_buf(),
        channel_buffer_size: 100,
        enabled: true,
    };

    let logger = Arc::new(DecisionLogger::new(config));

    // Log 10 candidates concurrently with shared logger
    let mut handles = vec![];

    for i in 0..10 {
        let logger_clone = Arc::clone(&logger);

        let handle = tokio::spawn(async move {
            let initial_components = InitialComponents {
                base_shadow: 60 + i as u8,
                qass_score: 70.0 + i as f32,
                qedd_survival_30s: Some(0.6 + (i as f32 * 0.01)),
                mci: Some(0.7),
                chaos_loss_prob: Some(0.1),
                gene_match_score: Some(0.02),
                confidence: Some(0.7),
                extras: HashMap::new(),
            };

            let decision_log = OracleDecisionLog::new(
                format!("pool_concurrent_{}", i),
                65 + i as u8,
                DecisionType::Buy,
                initial_components,
            );

            logger_clone.log(decision_log).await;
        });

        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }

    println!("📋 Logged 10 candidates concurrently");

    // Give time for async writes
    sleep(Duration::from_millis(500)).await;

    // Verify all files exist
    for i in 0..10 {
        let log_path = temp_dir
            .path()
            .join(format!("pool_concurrent_{}", i))
            .join("decision.jsonl");
        assert!(log_path.exists(), "Log file {} should exist", i);
    }

    println!("✅ All 10 candidate logs created successfully");
    println!();
}
