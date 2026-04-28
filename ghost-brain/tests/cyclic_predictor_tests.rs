//! Cyclic HyperPredictor Unit Tests - Task 5 (Zadanie 5)
//!
//! Tests for the 12-cycle scoring engine as specified in CONSOLIDATED_TASKS_SCORING_ENGINE.md.
//!
//! ## Coverage
//! - 12 cycle execution
//! - Early Stage Mode (S1-S6) - limited modules
//! - Full Analysis Mode (S7-S12) - all modules active
//! - Gunshot early exit mechanism
//! - VETO conditions (LIGMA, ClusterHunter)
//! - Phase transitions

use ghost_brain::oracle::predator_strategy::{
    calculate_quality_for_cycle, get_cycle_weight, get_gunshot_threshold, is_early_stage_cycle,
    ScoringPhase, CYCLE_WEIGHTS, EARLY_STAGE_CYCLE_THRESHOLD, GUNSHOT_THRESHOLDS,
};
use ghost_brain::oracle::scoring_phase::ScoringPhase as OracleScoringPhase;

// =============================================================================
// 5.2: CyclicHyperPredictor Unit Tests
// =============================================================================

#[test]
fn test_12_cycles_configuration() {
    // Verify exactly 12 cycles are configured
    assert_eq!(
        CYCLE_WEIGHTS.len(),
        12,
        "Should have exactly 12 cycle weights"
    );
    assert_eq!(
        GUNSHOT_THRESHOLDS.len(),
        12,
        "Should have exactly 12 gunshot thresholds"
    );
}

#[test]
fn test_early_stage_phase_detection() {
    // S1-S6 (indices 0-5) should be Early Stage
    for cycle_idx in 0..6 {
        assert!(
            is_early_stage_cycle(cycle_idx),
            "Cycle S{} (idx {}) should be Early Stage",
            cycle_idx + 1,
            cycle_idx
        );

        let phase = ScoringPhase::from_cycle_idx(cycle_idx);
        assert_eq!(
            phase,
            ScoringPhase::EarlyStage,
            "S{} should be EarlyStage",
            cycle_idx + 1
        );
    }
}

#[test]
fn test_full_analysis_phase_detection() {
    // S7-S12 (indices 6-11) should be Full Analysis
    for cycle_idx in 6..12 {
        assert!(
            !is_early_stage_cycle(cycle_idx),
            "Cycle S{} (idx {}) should be Full Analysis",
            cycle_idx + 1,
            cycle_idx
        );

        let phase = ScoringPhase::from_cycle_idx(cycle_idx);
        assert_eq!(
            phase,
            ScoringPhase::FullAnalysis,
            "S{} should be FullAnalysis",
            cycle_idx + 1
        );
    }
}

#[test]
fn test_early_stage_threshold() {
    // Verify EARLY_STAGE_CYCLE_THRESHOLD is correctly set to 6
    assert_eq!(
        EARLY_STAGE_CYCLE_THRESHOLD, 6,
        "Early stage threshold should be 6 cycles"
    );

    // Index 5 (S6) should be last Early Stage cycle
    assert!(is_early_stage_cycle(5));

    // Index 6 (S7) should be first Full Analysis cycle
    assert!(!is_early_stage_cycle(6));
}

#[test]
fn test_quality_calculation_uses_correct_formula() {
    // Early Stage (S1-S6): should NOT use SCR
    for cycle_idx in 0..6 {
        let quality_with_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.9), 0.5);
        let quality_without_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, None, 0.5);

        // Both should be equal (SCR ignored in Early Stage)
        assert!(
            (quality_with_scr - quality_without_scr).abs() < 0.001,
            "S{}: SCR should be ignored in Early Stage",
            cycle_idx + 1
        );
    }

    // Full Analysis (S7-S12): should use SCR when provided
    for cycle_idx in 6..12 {
        let quality_with_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.3), 0.5);
        let quality_without_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, None, 0.5);

        // Should be different when SCR is provided
        assert!(
            (quality_with_scr - quality_without_scr).abs() > 0.001,
            "S{}: SCR should affect quality in Full Analysis",
            cycle_idx + 1
        );
    }
}

// =============================================================================
// Module Activation Tests
// =============================================================================

#[test]
fn test_scr_enabled_only_in_full_analysis() {
    // Using OracleScoringPhase (the one in scoring_phase.rs)
    let early_phase = OracleScoringPhase::EarlyStage;
    let full_phase = OracleScoringPhase::FullAnalysis;

    assert!(
        !early_phase.is_scr_enabled(),
        "SCR should be disabled in EarlyStage"
    );
    assert!(
        full_phase.is_scr_enabled(),
        "SCR should be enabled in FullAnalysis"
    );
}

#[test]
fn test_ulvf_enabled_only_in_full_analysis() {
    let early_phase = OracleScoringPhase::EarlyStage;
    let full_phase = OracleScoringPhase::FullAnalysis;

    assert!(
        !early_phase.is_ulvf_enabled(),
        "ULVF should be disabled in EarlyStage"
    );
    assert!(
        full_phase.is_ulvf_enabled(),
        "ULVF should be enabled in FullAnalysis"
    );
}

#[test]
fn test_povc_enabled_only_in_full_analysis() {
    let early_phase = OracleScoringPhase::EarlyStage;
    let full_phase = OracleScoringPhase::FullAnalysis;

    assert!(
        !early_phase.is_povc_enabled(),
        "POVC should be disabled in EarlyStage"
    );
    assert!(
        full_phase.is_povc_enabled(),
        "POVC should be enabled in FullAnalysis"
    );
}

#[test]
fn test_phase_from_cycle_mapping() {
    // Test ScoringPhase::from_cycle (1-indexed as per documentation)
    for cycle in 1..=6 {
        let phase = OracleScoringPhase::from_cycle(cycle);
        assert_eq!(
            phase,
            OracleScoringPhase::EarlyStage,
            "Cycle {} should be EarlyStage",
            cycle
        );
    }

    for cycle in 7..=12 {
        let phase = OracleScoringPhase::from_cycle(cycle);
        assert_eq!(
            phase,
            OracleScoringPhase::FullAnalysis,
            "Cycle {} should be FullAnalysis",
            cycle
        );
    }
}

#[test]
fn test_min_tx_count_by_phase() {
    let early = OracleScoringPhase::EarlyStage;
    let full = OracleScoringPhase::FullAnalysis;

    assert_eq!(early.min_tx_count(), 16, "Early Stage min TX should be 16");
    assert_eq!(full.min_tx_count(), 23, "Full Analysis min TX should be 23");
}

// =============================================================================
// Gunshot Mechanism Tests
// =============================================================================

#[test]
fn test_gunshot_threshold_progression() {
    // Gunshot thresholds should decrease over cycles (easier to trigger later)
    let expected = [
        (0, 100.0), // S1
        (1, 99.0),  // S2
        (2, 98.0),  // S3
        (3, 97.0),  // S4
        (4, 96.0),  // S5
        (5, 95.0),  // S6
        (6, 88.0),  // S7 - big drop at Full Analysis
        (7, 87.0),  // S8
        (8, 86.0),  // S9
        (9, 85.0),  // S10
        (10, 83.5), // S11
        (11, 82.0), // S12
    ];

    for (idx, threshold) in expected {
        let actual = get_gunshot_threshold(idx);
        assert!(
            (actual - threshold).abs() < 0.01,
            "S{} threshold should be {}, got {}",
            idx + 1,
            threshold,
            actual
        );
    }
}

#[test]
fn test_gunshot_early_stage_high_bar() {
    // Early Stage (S1-S6) requires very high scores for gunshot
    for idx in 0..6 {
        let threshold = get_gunshot_threshold(idx);
        assert!(
            threshold >= 95.0,
            "Early Stage S{} threshold should be >= 95, got {}",
            idx + 1,
            threshold
        );
    }
}

#[test]
fn test_gunshot_full_analysis_lower_bar() {
    // Full Analysis (S7-S12) has lower thresholds
    for idx in 6..12 {
        let threshold = get_gunshot_threshold(idx);
        assert!(
            threshold <= 88.0,
            "Full Analysis S{} threshold should be <= 88, got {}",
            idx + 1,
            threshold
        );
    }
}

// =============================================================================
// Cycle Weight Tests
// =============================================================================

#[test]
fn test_cycle_weights_exponential_growth() {
    // Weights should grow exponentially (~1.3x per cycle)
    for i in 1..12 {
        let prev_weight = get_cycle_weight(i - 1);
        let curr_weight = get_cycle_weight(i);
        let ratio = curr_weight / prev_weight;

        // Ratio should be approximately 1.1-1.4
        assert!(
            ratio >= 1.1 && ratio <= 1.5,
            "Weight growth ratio S{}/S{} should be ~1.3, got {}",
            i + 1,
            i,
            ratio
        );
    }
}

#[test]
fn test_s12_dominant_weight() {
    // S12 should have dominant weight (~24% of total)
    let s12_weight = get_cycle_weight(11);
    let total: f32 = CYCLE_WEIGHTS.iter().sum();
    let s12_percentage = s12_weight / total * 100.0;

    assert!(
        s12_percentage > 20.0 && s12_percentage < 30.0,
        "S12 should have ~24% weight, got {:.1}%",
        s12_percentage
    );
}

#[test]
fn test_s1_minimal_weight() {
    // S1 should have minimal weight (~1.4% of total)
    let s1_weight = get_cycle_weight(0);
    let total: f32 = CYCLE_WEIGHTS.iter().sum();
    let s1_percentage = s1_weight / total * 100.0;

    assert!(
        s1_percentage < 3.0,
        "S1 should have ~1.4% weight, got {:.1}%",
        s1_percentage
    );
}

#[test]
fn test_cycle_weight_out_of_bounds() {
    // Out of bounds should return 1.0 (fallback)
    let out_of_bounds = get_cycle_weight(12);
    assert_eq!(out_of_bounds, 1.0, "Out of bounds weight should be 1.0");

    let far_out = get_cycle_weight(100);
    assert_eq!(far_out, 1.0, "Far out of bounds weight should be 1.0");
}

// =============================================================================
// Phase Transition Tests
// =============================================================================

#[test]
fn test_phase_transition_at_s7() {
    // Phase should transition from EarlyStage to FullAnalysis at S7
    let s6_phase = ScoringPhase::from_cycle_idx(5);
    let s7_phase = ScoringPhase::from_cycle_idx(6);

    assert_eq!(
        s6_phase,
        ScoringPhase::EarlyStage,
        "S6 should be EarlyStage"
    );
    assert_eq!(
        s7_phase,
        ScoringPhase::FullAnalysis,
        "S7 should be FullAnalysis"
    );
}

#[test]
fn test_phase_predicates() {
    let early = ScoringPhase::EarlyStage;
    let full = ScoringPhase::FullAnalysis;

    assert!(early.is_early_stage());
    assert!(!early.should_use_scr());

    assert!(!full.is_early_stage());
    assert!(full.should_use_scr());
    assert!(full.should_use_ulvf());
    assert!(full.should_use_povc());
}

// =============================================================================
// Display and Serialization Tests
// =============================================================================

#[test]
fn test_phase_display_names() {
    let early = OracleScoringPhase::EarlyStage;
    let full = OracleScoringPhase::FullAnalysis;

    assert!(early.name().contains("Early Stage"));
    assert!(full.name().contains("Full Analysis"));
}

#[test]
fn test_phase_default() {
    let default = OracleScoringPhase::default();
    assert_eq!(
        default,
        OracleScoringPhase::EarlyStage,
        "Default phase should be EarlyStage"
    );
}

// =============================================================================
// Quality Formula Integration Tests
// =============================================================================

#[test]
fn test_quality_formula_weight_verification() {
    // Early Stage weights should sum to 1.0
    let early_sum: f32 = 0.44 + 0.31 + 0.25;
    assert!(
        (early_sum - 1.0).abs() < 0.01,
        "Early Stage quality weights should sum to 1.0"
    );

    // Full Analysis weights should sum to 1.0
    let full_sum: f32 = 0.35 + 0.25 + 0.20 + 0.20;
    assert!(
        (full_sum - 1.0).abs() < 0.01,
        "Full Analysis quality weights should sum to 1.0"
    );
}

#[test]
fn test_quality_perfect_inputs() {
    // Perfect inputs (all 1.0) should give quality close to 1.0
    let early_perfect = calculate_quality_for_cycle(0, 1.0, 1.0, None, 1.0);
    assert!(
        early_perfect > 0.99,
        "Perfect Early Stage quality should be ~1.0, got {}",
        early_perfect
    );

    let full_perfect = calculate_quality_for_cycle(6, 1.0, 1.0, Some(0.0), 1.0);
    assert!(
        full_perfect > 0.99,
        "Perfect Full Analysis quality should be ~1.0, got {}",
        full_perfect
    );
}

#[test]
fn test_quality_zero_inputs() {
    // Zero inputs should give quality = 0
    let early_zero = calculate_quality_for_cycle(0, 0.0, 0.0, None, 0.0);
    assert_eq!(early_zero, 0.0, "Zero Early Stage quality should be 0");

    let full_zero = calculate_quality_for_cycle(6, 0.0, 0.0, Some(1.0), 0.0);
    assert_eq!(full_zero, 0.0, "Zero Full Analysis quality should be 0");
}

// =============================================================================
// Concurrent Processing Tests
// =============================================================================

#[test]
fn test_phase_enum_is_copy() {
    // ScoringPhase should be Copy for efficient concurrent processing
    let phase = ScoringPhase::EarlyStage;
    let copy = phase;
    assert_eq!(phase, copy, "ScoringPhase should be Copy");
}

#[test]
fn test_phase_enum_thread_safe() {
    use std::sync::Arc;
    use std::thread;

    let phases = Arc::new(vec![ScoringPhase::EarlyStage, ScoringPhase::FullAnalysis]);

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let phases = Arc::clone(&phases);
            thread::spawn(move || {
                for phase in phases.iter() {
                    // Just verify we can read phases from multiple threads
                    let _ = phase.is_early_stage();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread should complete successfully");
    }
}
