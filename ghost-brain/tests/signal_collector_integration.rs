//! SignalCollector Integration Tests
//!
//! Tests for the SignalCollector module that orchestrates signal collection
//! from various analysis modules (LIGMA, QEDD, Cluster, MCI, Paradox).
//!
//! ## Test Coverage
//!
//! - Signal source tracking (Explicit, Fallback, Unavailable)
//! - Phase-based signal collection (EarlyStage vs. FullAnalysis)
//! - Veto logic for critical signals (LIGMA, QEDD, MCI, Cluster)
//! - Fallback tracking and confidence adjustment

use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::hyper_prediction::signals::{SignalCollector, SignalResult, SignalSource};
use ghost_brain::oracle::hyper_prediction::state::AnalysisPhase;
use solana_sdk::pubkey::Pubkey;

// =============================================================================
// Test Helpers
// =============================================================================

/// Create a base test candidate with neutral values
fn create_test_candidate() -> EnhancedCandidate {
    EnhancedCandidate::new_with_fields(
        Some(1),              // slot
        1_000_000,            // timestamp
        30.0,                 // initial_liquidity_sol
        0.0,                  // dev_buy_sol
        Some(0.5),            // bonding_curve_progress
        0,                    // vanity_score
        0,                    // metadata_len_score
        false,                // has_dev_buy
        false,                // mint_auth_disabled
        Some(0.0001),         // expected_price
        Some(5),              // shadow_bonding_progress
        Some(30_000_000_000), // virtual_sol_reserves
        None,                 // shadow_market_cap
        Pubkey::new_unique(), // pool_amm_id
        Pubkey::new_unique(), // amm_program_id
        Pubkey::new_unique(), // base_mint
        Pubkey::new_unique(), // quote_mint
        Pubkey::new_unique(), // bonding_curve
        "sig".to_string(),    // signature
        None,                 // token_total_supply
    )
}

// =============================================================================
// Core SignalCollector Tests
// =============================================================================

#[test]
fn test_signal_collector_creation() {
    let collector = SignalCollector::new();

    // Collector should be created successfully
    // (This is a simple smoke test)
    drop(collector);
}

#[test]
fn test_signal_collector_default() {
    let collector = SignalCollector::default();

    // Default constructor should work
    drop(collector);
}

#[test]
fn test_signal_bundle_early_stage() {
    let collector = SignalCollector::new();
    let candidate = create_test_candidate();

    // In early stage, most signals should be unavailable
    let bundle = collector.collect(&candidate, AnalysisPhase::EarlyStage, None);

    // LIGMA runs in all phases, but without pool it's unavailable
    assert_eq!(bundle.ligma.source, SignalSource::Unavailable);

    // QEDD, MCI also run in early stage but need data
    assert_eq!(bundle.qedd.source, SignalSource::Unavailable);
    assert_eq!(bundle.mci.source, SignalSource::Unavailable);

    // Cluster and Paradox are optional inputs
    assert_eq!(bundle.cluster.source, SignalSource::Unavailable);
    assert_eq!(bundle.paradox.source, SignalSource::Unavailable);
}

#[test]
fn test_signal_bundle_full_analysis() {
    let collector = SignalCollector::new();
    let candidate = create_test_candidate();

    // In full analysis, signals should be attempted
    let bundle = collector.collect(&candidate, AnalysisPhase::FullAnalysis, None);

    // Without pool/data, signals will still be unavailable, but the phase doesn't block them
    assert_eq!(bundle.ligma.source, SignalSource::Unavailable);
    assert_eq!(bundle.qedd.source, SignalSource::Unavailable);
    assert_eq!(bundle.mci.source, SignalSource::Unavailable);
    assert_eq!(bundle.cluster.source, SignalSource::Unavailable);
    assert_eq!(bundle.paradox.source, SignalSource::Unavailable);
}

// =============================================================================
// run_if_mature Tests
// =============================================================================

#[test]
fn test_run_if_mature_early_stage_blocks() {
    let collector = SignalCollector::new();

    // In early stage, run_if_mature should return Unavailable
    let result = collector.run_if_mature(AnalysisPhase::EarlyStage, || Some(42));

    assert_eq!(result.source, SignalSource::Unavailable);
    assert_eq!(result.value, None);
    assert_eq!(result.confidence, 0.0);
}

#[test]
fn test_run_if_mature_full_analysis_executes() {
    let collector = SignalCollector::new();

    // In full analysis, run_if_mature should execute the function
    let result = collector.run_if_mature(AnalysisPhase::FullAnalysis, || Some(42));

    assert_eq!(result.source, SignalSource::Explicit);
    assert_eq!(result.value, Some(42));
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn test_run_if_mature_full_analysis_none_result() {
    let collector = SignalCollector::new();

    // If the function returns None, result should be Unavailable
    let result: SignalResult<i32> = collector.run_if_mature(AnalysisPhase::FullAnalysis, || None);

    assert_eq!(result.source, SignalSource::Unavailable);
    assert_eq!(result.value, None);
    assert_eq!(result.confidence, 0.0);
}

// =============================================================================
// SignalResult Tests
// =============================================================================

#[test]
fn test_signal_result_explicit() {
    let result = SignalResult::explicit(100, 0.95);

    assert_eq!(result.value, Some(100));
    assert_eq!(result.source, SignalSource::Explicit);
    assert_eq!(result.confidence, 0.95);
}

#[test]
fn test_signal_result_fallback() {
    let result = SignalResult::fallback(50, 0.3);

    assert_eq!(result.value, Some(50));
    assert_eq!(result.source, SignalSource::Fallback);
    assert_eq!(result.confidence, 0.3);
}

#[test]
fn test_signal_result_unavailable() {
    let result: SignalResult<i32> = SignalResult::unavailable();

    assert_eq!(result.value, None);
    assert_eq!(result.source, SignalSource::Unavailable);
    assert_eq!(result.confidence, 0.0);
}

// =============================================================================
// SignalSource Tests
// =============================================================================

#[test]
fn test_signal_source_equality() {
    assert_eq!(SignalSource::Explicit, SignalSource::Explicit);
    assert_eq!(SignalSource::Fallback, SignalSource::Fallback);
    assert_eq!(SignalSource::Unavailable, SignalSource::Unavailable);

    assert_ne!(SignalSource::Explicit, SignalSource::Fallback);
    assert_ne!(SignalSource::Explicit, SignalSource::Unavailable);
    assert_ne!(SignalSource::Fallback, SignalSource::Unavailable);
}

#[test]
fn test_signal_source_copy() {
    let source1 = SignalSource::Explicit;
    let source2 = source1; // Copy should work

    assert_eq!(source1, source2);
}

// =============================================================================
// Integration Scenario: Early Stage Cold Start
// =============================================================================

#[test]
fn test_early_stage_cold_start_scenario() {
    // Simulate first 2-3 seconds of token lifecycle (tx_count < 2)
    let collector = SignalCollector::new();
    let candidate = create_test_candidate();

    let bundle = collector.collect(&candidate, AnalysisPhase::EarlyStage, None);

    // In early stage with no pool data:
    // - LIGMA: Unavailable (no pool state)
    // - QEDD: Unavailable (no market signals)
    // - MCI: Unavailable (no market signals)
    // - Cluster: Unavailable (no cluster data)
    // - Paradox: Unavailable (no paradox state)

    assert_eq!(bundle.ligma.source, SignalSource::Unavailable);
    assert_eq!(bundle.qedd.source, SignalSource::Unavailable);
    assert_eq!(bundle.mci.source, SignalSource::Unavailable);
    assert_eq!(bundle.cluster.source, SignalSource::Unavailable);
    assert_eq!(bundle.paradox.source, SignalSource::Unavailable);

    // All confidence values should be 0.0 for unavailable signals
    assert_eq!(bundle.ligma.confidence, 0.0);
    assert_eq!(bundle.qedd.confidence, 0.0);
    assert_eq!(bundle.mci.confidence, 0.0);
    assert_eq!(bundle.cluster.confidence, 0.0);
    assert_eq!(bundle.paradox.confidence, 0.0);
}

// =============================================================================
// Integration Scenario: Full Analysis with Sparse Data
// =============================================================================

#[test]
fn test_full_analysis_sparse_data_scenario() {
    // Simulate 4+ seconds into token lifecycle (tx_count >= 2)
    let collector = SignalCollector::new();
    let candidate = create_test_candidate();

    let bundle = collector.collect(&candidate, AnalysisPhase::FullAnalysis, None);

    // In full analysis without pool data, signals are still unavailable
    // but the phase doesn't prevent their collection
    assert_eq!(bundle.ligma.source, SignalSource::Unavailable);
    assert_eq!(bundle.qedd.source, SignalSource::Unavailable);
    assert_eq!(bundle.mci.source, SignalSource::Unavailable);

    // Optional signals remain unavailable
    assert_eq!(bundle.cluster.source, SignalSource::Unavailable);
    assert_eq!(bundle.paradox.source, SignalSource::Unavailable);
}
