use crate::config::ghost_brain_config::BehavioralScoringConfig;
use crate::oracle::survivor_score::{SurvivorScoreCalculator, SurvivorScoreInput};

#[test]
fn test_good_volume_slow_rpc_scenario() {
    let calc = SurvivorScoreCalculator::new();

    // SCENARIUSZ: "The Good Volume / Slow RPC"
    // iwim_threat_score is None (RPC slow)
    // sobp_momentum is strong (Good Volume)

    let input_slow_rpc = SurvivorScoreInput {
        iwim_threat_score: None,    // RPC pending
        sobp_momentum: Some(1.5),   // Strong volume/momentum
        chaos_pump_prob: Some(0.8), // Good pump probability
        qman_score: Some(0.7),      // Good QMAN

        // Other components neutral/positive
        qedd_survival_60s: Some(0.8),
        mpcf_organic_ratio: Some(0.6),

        ..Default::default()
    };

    let result = calc.calculate(&input_slow_rpc);

    // DEBUG OUTPUT
    println!(
        "Score: {}, Breakdown: Survival={:.2} Momentum={:.2}",
        result.score, result.breakdown.survival, result.breakdown.momentum
    );

    // FAIL CONDITION from ticket:
    // If w 1. cyklu ocena wynosi < 50 punktów (przez S=0.25), test jest NIEZALICZONY.
    assert!(
        result.score >= 50,
        "Score should be >= 50 during RPC wait, got {}",
        result.score
    );

    // ACCEPTANCE CRITERIA:
    // Bot NATYCHMIAST wystawia ocenę > 85 punktów (dzięki S=1.0).
    // Note: With S=1.0 for IWIM, overall survival might not be exactly 1.0 depending on other factors (QEDD, Cluster),
    // but should be high enough.
    // Let's see what the current implementation gives.
    // Current impl: iwim_threat_score.map(|t| 1.0 - t).unwrap_or(0.5);
    // 0.5 is neutral/wait, not 1.0 trust.

    // Check survival breakdown specifically for IWIM
    // We expect it to be 1.0 after fix.
    assert_eq!(
        result.breakdown.survival_from_iwim, 1.0,
        "IWIM survival should be 1.0 when pending (Default Trust)"
    );
}

#[test]
fn test_dead_token_full_pipeline() {
    let calc = SurvivorScoreCalculator::new();
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.05),
        cluster_risk_score: Some(0.95),
        sobp_momentum: None,
        qman_score: None,
        chaos_pump_prob: None,
        mpcf_organic_ratio: Some(0.05),
        mesa_organic_likeness: Some(0.05),
        scr_bot_score: Some(0.95),
        unique_wallet_ratio: Some(0.05),
        bva_score: Some(0.0),
        ligma_tradability_score: Some(0.05),
        ..Default::default()
    };

    let result = calc.calculate(&input);
    assert!(
        result.score < 40,
        "dead token should stay below 40, got {}",
        result.score
    );
}

#[test]
fn test_behavioral_does_not_dominate() {
    let mut calc = SurvivorScoreCalculator::new();
    let mut cfg = BehavioralScoringConfig::default();
    cfg.enabled = true;
    cfg.use_additive_mode = true;
    cfg.max_adjustment_points = 15.0;
    cfg.neutral_point = 0.5;
    cfg.min_behavioral_floor = 0.0;
    cfg.w_ecto = 1.0;
    cfg.w_bva = 0.0;
    cfg.w_panic = 0.0;
    cfg.w_tcr = 0.0;
    cfg.w_cir = 0.0;
    calc.update_behavioral_config(cfg);

    let base_input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.7),
        sobp_momentum: Some(0.0),
        qman_score: Some(0.5),
        chaos_pump_prob: Some(0.3),
        mpcf_organic_ratio: Some(0.65),
        mesa_organic_likeness: Some(0.6),
        scr_bot_score: Some(0.3),
        unique_wallet_ratio: Some(0.7),
        ligma_tradability_score: Some(0.5),
        ecto_score: Some(0.3),
        ..Default::default()
    };

    let result = calc.calculate(&base_input);
    assert!(
        (45..=60).contains(&result.score),
        "behavioral should modulate but not dominate: score={}",
        result.score
    );
}
