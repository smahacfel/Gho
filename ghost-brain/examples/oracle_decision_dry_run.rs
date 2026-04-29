//! Oracle Decision Logging Dry Run Demo
//!
//! This example demonstrates the complete Oracle Brain decision logging system:
//! 1. Initial scoring with component breakdown
//! 2. Follow-up scoring at 1s, 5s, 30s, 60s intervals
//! 3. Corrections with explicit reasons  
//! 4. JSONL output for each candidate
//!
//! Run with: cargo run --example oracle_decision_dry_run

use ghost_brain::oracle::{
    CorrectionReason, DecisionLogger, DecisionLoggerConfig, DecisionType, FollowupConfig,
    FollowupContext, FollowupScore, FollowupScoringManager, InitialComponents, OracleDecisionLog,
    VetoType,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

#[tokio::main]
async fn main() {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║        Oracle Brain Decision Logging - Dry Run Demonstration           ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    // Create decision logger
    let logger_config = DecisionLoggerConfig {
        log_dir: "datasets/decisions".into(),
        gatekeeper_log_dir: "logs/decisions.json/rollout/shadow-burnin/decisions".into(),
        channel_buffer_size: 1000,
        enabled: true,
    };
    let logger = Arc::new(DecisionLogger::new(logger_config));

    // Create follow-up scoring manager
    let followup_config = FollowupConfig {
        enabled: true,
        intervals_ms: vec![1000, 5000, 30000, 60000],
        mci_drop_threshold: 0.50,
        qedd_lambda_spike_threshold: 2.0,
        qedd_survival_drop_pct: 0.30,
        chaos_loss_prob_threshold: 0.60,
        gene_match_threshold: 0.70,
        exit_threshold: 40,
        score_drop_pct_threshold: 0.30,
    };
    let _followup_manager = Arc::new(FollowupScoringManager::new(
        followup_config.clone(),
        Arc::clone(&logger),
    ));

    println!("📋 Configuration:");
    println!("   Log directory: datasets/decisions");
    println!("   Follow-up intervals: {:?}", followup_config.intervals_ms);
    println!(
        "   MCI drop threshold: {}",
        followup_config.mci_drop_threshold
    );
    println!(
        "   QEDD λ spike threshold: {}",
        followup_config.qedd_lambda_spike_threshold
    );
    println!();

    // ========================================================================
    // Scenario 1: Successful Trade (Good candidate)
    // ========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│ Scenario 1: Successful Trade - Good Candidate                         │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    let mut extras1 = HashMap::new();
    extras1.insert("ssmi_entropy".to_string(), 0.32);
    extras1.insert("mpcf_confidence".to_string(), 0.91);
    extras1.insert("sobp_score".to_string(), 0.78);

    let components1 = InitialComponents {
        base_shadow: 75,
        qass_score: 88.5,
        qedd_survival_30s: Some(0.85),
        mci: Some(0.88),
        chaos_loss_prob: Some(0.08),
        gene_match_score: Some(0.02),
        confidence: Some(0.92),
        extras: extras1,
    };

    let mut log1 = OracleDecisionLog::new(
        "pool_good_candidate_001".to_string(),
        85,
        DecisionType::Buy,
        components1.clone(),
    );

    println!("✅ Initial Score: 85 (BUY)");
    println!("   Components:");
    println!("     - base_shadow: {}", components1.base_shadow);
    println!("     - qass_score: {:.2}", components1.qass_score);
    println!(
        "     - qedd_survival_30s: {:.2}",
        components1.qedd_survival_30s.unwrap()
    );
    println!("     - mci: {:.2}", components1.mci.unwrap());
    println!();

    // Simulate stable follow-ups
    let followups1 = vec![
        (
            1000,
            84,
            "Minor QASS fluctuation",
            vec![],
            DecisionType::Hold,
        ),
        (5000, 86, "Slight improvement", vec![], DecisionType::Hold),
        (30000, 88, "Strong momentum", vec![], DecisionType::Hold),
        (60000, 90, "Continuing uptrend", vec![], DecisionType::Hold),
    ];

    for (t_ms, score, reason, corrections, decision) in followups1 {
        let followup = FollowupScore {
            t_ms,
            score,
            reason: reason.to_string(),
            corrections,
            decision: decision.clone(),
            components: None,
            confidence: None,
        };
        log1.add_followup(followup);
        println!(
            "   {}ms: score={} ({:?}) - {}",
            t_ms, score, decision, reason
        );
    }

    log1.complete();
    logger.log(log1).await;

    println!();
    println!("✅ Scenario 1 completed - Candidate held successfully");
    println!();

    // ========================================================================
    // Scenario 2: MCI Drop Detected (Sell triggered)
    // ========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│ Scenario 2: MCI Drop - Early Exit                                     │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    let mut extras2 = HashMap::new();
    extras2.insert("ssmi_entropy".to_string(), 0.45);

    let components2 = InitialComponents {
        base_shadow: 65,
        qass_score: 72.0,
        qedd_survival_30s: Some(0.68),
        mci: Some(0.72),
        chaos_loss_prob: Some(0.15),
        gene_match_score: Some(0.05),
        confidence: Some(0.75),
        extras: extras2,
    };

    let mut log2 = OracleDecisionLog::new(
        "pool_mci_drop_002".to_string(),
        68,
        DecisionType::Buy,
        components2.clone(),
    );

    println!("✅ Initial Score: 68 (BUY)");
    println!();

    // 1s: Normal
    let followup_1s = FollowupScore {
        t_ms: 1000,
        score: 67,
        reason: "Small QASS adjustment".to_string(),
        corrections: vec![],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    };
    log2.add_followup(followup_1s);
    println!("   1s: score=67 (HOLD) - Small QASS adjustment");

    // 5s: MCI drops significantly
    let mci_correction = CorrectionReason::MciDrop {
        old_value: 0.72,
        new_value: 0.42,
        threshold: 0.50,
        impact: -18,
    };

    let followup_5s = FollowupScore {
        t_ms: 5000,
        score: 49,
        reason: "MCI drop below threshold".to_string(),
        corrections: vec![mci_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };
    log2.add_followup(followup_5s);
    println!("   5s: score=49 (SELL) - 🚨 MCI drop: 0.72 → 0.42 (impact: -18)");

    log2.complete();
    logger.log(log2).await;

    println!();
    println!("✅ Scenario 2 completed - Exit triggered by MCI drop");
    println!();

    // ========================================================================
    // Scenario 3: QEDD λ Spike (Critical exit)
    // ========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│ Scenario 3: QEDD λ Spike - Critical Exit                              │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    let components3 = InitialComponents {
        base_shadow: 70,
        qass_score: 75.0,
        qedd_survival_30s: Some(0.70),
        mci: Some(0.75),
        chaos_loss_prob: Some(0.12),
        gene_match_score: Some(0.03),
        confidence: Some(0.8),
        extras: HashMap::new(),
    };

    let mut log3 = OracleDecisionLog::new(
        "pool_qedd_spike_003".to_string(),
        72,
        DecisionType::Buy,
        components3,
    );

    println!("✅ Initial Score: 72 (BUY)");
    println!();

    // Early followups stable
    log3.add_followup(FollowupScore {
        t_ms: 1000,
        score: 71,
        reason: "Stable".to_string(),
        corrections: vec![],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    });
    println!("   1s: score=71 (HOLD) - Stable");

    log3.add_followup(FollowupScore {
        t_ms: 5000,
        score: 70,
        reason: "Minor decline".to_string(),
        corrections: vec![],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    });
    println!("   5s: score=70 (HOLD) - Minor decline");

    // 30s: Critical QEDD λ spike
    let lambda_correction = CorrectionReason::QeddLambdaSpike {
        old_lambda: 0.5,
        new_lambda: 3.8,
        threshold: 2.0,
        impact: -30,
    };

    let followup_30s = FollowupScore {
        t_ms: 30000,
        score: 40,
        reason: "CRITICAL: QEDD λ spike indicates rapid decay".to_string(),
        corrections: vec![lambda_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };
    log3.add_followup(followup_30s);
    println!("   30s: score=40 (SELL) - 🚨 CRITICAL λ spike: 0.5 → 3.8");

    log3.complete();
    logger.log(log3).await;

    println!();
    println!("✅ Scenario 3 completed - Emergency exit due to λ spike");
    println!();

    // ========================================================================
    // Scenario 4: GeneMapper Veto (Scam detected)
    // ========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│ Scenario 4: GeneMapper Veto - Scam Pattern Detected                   │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    let components4 = InitialComponents {
        base_shadow: 80,
        qass_score: 85.0,
        qedd_survival_30s: Some(0.82),
        mci: Some(0.85),
        chaos_loss_prob: Some(0.06),
        gene_match_score: Some(0.88), // HIGH - scam pattern
        confidence: Some(0.55),
        extras: HashMap::new(),
    };

    let mut log4 = OracleDecisionLog::new(
        "pool_scam_veto_004".to_string(),
        82,
        DecisionType::Buy,
        components4,
    );

    println!("✅ Initial Score: 82 (BUY)");
    println!("   ⚠️  GeneMapper score: 0.88 (HIGH RISK)");
    println!();

    // GeneMapper veto triggered immediately
    log4.set_veto(VetoType::Gene, DecisionType::Skip);

    let gene_correction = CorrectionReason::GeneMapperHit {
        match_score: 0.88,
        pattern_id: "pump_dump_pattern_v3".to_string(),
        impact: -100,
    };

    let veto_followup = FollowupScore {
        t_ms: 100,
        score: 0,
        reason: "VETO: GeneMapper detected known scam pattern".to_string(),
        corrections: vec![gene_correction],
        decision: DecisionType::Skip,
        components: None,
        confidence: None,
    };
    log4.add_followup(veto_followup);

    println!("   100ms: score=0 (SKIP) - 🛑 VETO: Scam pattern detected");

    log4.complete();
    logger.log(log4).await;

    println!();
    println!("✅ Scenario 4 completed - Trade blocked by GeneMapper veto");
    println!();

    // ========================================================================
    // Scenario 5: Guardian Abort (Anomaly detected)
    // ========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│ Scenario 5: Guardian Abort - Anomaly Detected                         │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    let components5 = InitialComponents {
        base_shadow: 73,
        qass_score: 78.0,
        qedd_survival_30s: Some(0.72),
        mci: Some(0.76),
        chaos_loss_prob: Some(0.11),
        gene_match_score: Some(0.04),
        confidence: Some(0.82),
        extras: HashMap::new(),
    };

    let mut log5 = OracleDecisionLog::new(
        "pool_guardian_abort_005".to_string(),
        75,
        DecisionType::Buy,
        components5,
    );

    println!("✅ Initial Score: 75 (BUY)");
    println!();

    // Normal at 1s
    log5.add_followup(FollowupScore {
        t_ms: 1000,
        score: 74,
        reason: "Normal".to_string(),
        corrections: vec![],
        decision: DecisionType::Hold,
        components: None,
        confidence: None,
    });
    println!("   1s: score=74 (HOLD) - Normal");

    // Guardian abort at 2s
    log5.set_veto(VetoType::Guardian, DecisionType::Sell);

    let guardian_correction = CorrectionReason::GuardianAbort {
        reason: "Suspicious wash trading pattern detected".to_string(),
        signal_name: "resonance_detector_alert".to_string(),
        impact: -100,
    };

    let abort_followup = FollowupScore {
        t_ms: 2000,
        score: 0,
        reason: "ABORT: Guardian detected suspicious activity".to_string(),
        corrections: vec![guardian_correction],
        decision: DecisionType::Sell,
        components: None,
        confidence: None,
    };
    log5.add_followup(abort_followup);

    println!("   2s: score=0 (SELL) - 🛡️ Guardian abort: Wash trading detected");

    log5.complete();
    logger.log(log5).await;

    println!();
    println!("✅ Scenario 5 completed - Emergency exit by Guardian");
    println!();

    // Give time for all async writes to complete
    sleep(Duration::from_millis(500)).await;

    // Summary
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                          Dry Run Summary                               ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("📊 Scenarios Executed: 5");
    println!();
    println!("   1. ✅ Successful Trade - Held through all intervals");
    println!("   2. 🔴 MCI Drop - Exited at 5s due to coherence loss");
    println!("   3. 🔴 QEDD λ Spike - Emergency exit at 30s");
    println!("   4. 🛑 GeneMapper Veto - Trade blocked (scam pattern)");
    println!("   5. 🛡️  Guardian Abort - Emergency exit (anomaly)");
    println!();
    println!("📁 Logs written to: datasets/decisions/");
    println!();
    println!("   pool_good_candidate_001/decision.jsonl");
    println!("   pool_mci_drop_002/decision.jsonl");
    println!("   pool_qedd_spike_003/decision.jsonl");
    println!("   pool_scam_veto_004/decision.jsonl");
    println!("   pool_guardian_abort_005/decision.jsonl");
    println!();
    println!("💡 View logs with:");
    println!("   cat datasets/decisions/*/decision.jsonl | jq '.'");
    println!();
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                       Dry Run Completed Successfully                    ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
}
