//! Trend Cohesion Field (TCF) Module
//!
//! TCF is a signal processing module that measures the **coherence** of market
//! dynamics across scoring cycles. Unlike traditional trend indicators (EMA, RSI, MACD),
//! TCF does not predict price direction. Instead, it evaluates whether the
//! **mechanism** generating market changes remains consistent.
//!
//! ## Core Concept
//!
//! Traditional trend analysis asks: "Is price going up?"
//! TCF asks: "Is the WAY price is changing consistent with how it was changing before?"
//!
//! This distinction is crucial for detecting:
//! - **Pump-and-dump schemes**: Initial buying looks organic, then dump pattern emerges
//! - **Bot-driven pumps**: Artificial volume with inconsistent dynamics
//! - **Organic growth**: Genuine interest with stable market behavior
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Ghost Scoring Pipeline                        │
//! │                                                                   │
//! │  S1 → S2 → S3 → ... → S13  (Scoring Cycles, 420ms each)         │
//! │   │     │     │           │                                      │
//! │   ▼     ▼     ▼           ▼                                      │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │           MarketObservation                  │                │
//! │  │  • price_delta [-1,1]                        │                │
//! │  │  • volume_delta [-1,1]                       │                │
//! │  │  • liquidity_entropy [0,1]                   │                │
//! │  │  • order_flow_imbalance [-1,1]               │                │
//! │  │  • mpcf [0,1]                                │                │
//! │  │  • jitter [0,1]                              │                │
//! │  │  • phase_sync [0,1]                          │                │
//! │  └─────────────────────────────────────────────┘                │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │              Transition                      │                │
//! │  │  T_i = O_{i+1} - O_i                         │                │
//! │  │  • delta_vector                              │                │
//! │  │  • volatility (magnitude)                    │                │
//! │  │  • directional_consistency                   │                │
//! │  └─────────────────────────────────────────────┘                │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │      ExpectedTransitionModel                 │                │
//! │  │  • Online learning (no ML/NN)                │                │
//! │  │  • Exponential forgetting                    │                │
//! │  │  • Anomaly resistance                        │                │
//! │  └─────────────────────────────────────────────┘                │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │       Cohesion Function (CORE)               │                │
//! │  │  cohesion(expected, observed) → [0,1]        │                │
//! │  │  • Direction alignment (40%)                 │                │
//! │  │  • Rhythm similarity (30%)                   │                │
//! │  │  • Stability preservation (30%)              │                │
//! │  └─────────────────────────────────────────────┘                │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │        TrendCohesionField                    │                │
//! │  │  • Weighted integral of cohesions            │                │
//! │  │  • Cliff detection                           │                │
//! │  │  • Phase classification                      │                │
//! │  │                                              │                │
//! │  │  Output: tcf_score [0,1]                     │                │
//! │  └─────────────────────────────────────────────┘                │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────┐                │
//! │  │           Final Verdict Integration          │                │
//! │  │  effective_momentum = base * (0.6 + 0.4*tcf) │                │
//! │  └─────────────────────────────────────────────┘                │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Module Structure
//!
//! | Module | Description |
//! |--------|-------------|
//! | `observation` | Market state representation at a single cycle |
//! | `transition` | State change operator between cycles |
//! | `expected` | Adaptive model learning expected transitions |
//! | `cohesion` | Core cohesion function (most important) |
//! | `field` | Top-level accumulator and integration point |
//!
//! ## Performance Characteristics
//!
//! - **Zero heap allocation** in hot path (after initialization)
//! - **O(1)** per-cycle update
//! - **O(1)** memory (fixed-size history buffers)
//! - **No external crates** (std/core only)
//! - **Deterministic** (same input → same output)
//!
//! ## Integration Example
//!
//! ```rust,ignore
//! use ghost_brain::oracle::tcf::{TrendCohesionField, MarketObservation};
//!
//! // Create TCF instance (once per pool analysis)
//! let mut tcf = TrendCohesionField::new();
//!
//! // For each scoring cycle S(i):
//! let observation = MarketObservation::new(
//!     price_delta,           // Normalized price change
//!     volume_delta,          // Normalized volume change
//!     liquidity_entropy,     // Liquidity distribution entropy
//!     order_flow_imbalance,  // Buy/sell pressure balance
//!     mpcf_confidence,       // Actor classification confidence
//!     jitter,                // Timing irregularity
//!     phase_sync,            // Participant synchronization
//! );
//!
//! let result = tcf.update(&observation);
//!
//! // Use in Final Verdict
//! let effective_momentum = base_momentum * (0.6 + 0.4 * result.tcf_score);
//!
//! // Check for warnings
//! if result.cliff_detected {
//!     warn!("TCF CLIFF: Trend dynamics changed abruptly!");
//! }
//!
//! match result.phase {
//!     TcfPhase::OrganicGrowth => { /* High confidence in trend */ }
//!     TcfPhase::Pump => { /* Caution: possibly artificial */ }
//!     TcfPhase::Dump => { /* VETO: dynamics reversed */ }
//!     _ => {}
//! }
//! ```
//!
//! ## Design Principles
//!
//! 1. **No classical indicators**: TCF deliberately avoids EMA, RSI, MACD, VWAP.
//!    These are easy to manipulate and widely known to bots.
//!
//! 2. **Focus on transitions**: We analyze HOW things change, not WHAT they are.
//!    A pump-and-dump has a distinct transition signature.
//!
//! 3. **Online learning**: The model starts fresh for each pool and learns
//!    the specific dynamics of that token launch.
//!
//! 4. **Modulation, not veto**: TCF modulates existing signals rather than
//!    creating hard rejections. This prevents overfitting to specific patterns.
//!
//! 5. **Interpretable**: Every component has clear meaning and can be traced
//!    through the calculation.

pub mod cohesion;
pub mod expected;
pub mod field;
pub mod observation;
pub mod transition;

// Re-export primary types
pub use cohesion::{
    batch_cohesion, cohesion, cohesion_simple, cumulative_cohesion, detect_cohesion_cliff,
    CohesionBreakdown, CohesionConfig, CohesionResult,
};
pub use expected::{ExpectedTransition, ExpectedTransitionModel};
pub use field::{TcfDiagnostics, TcfPhase, TcfUpdateResult, TrendCohesionField};
pub use observation::{MarketObservation, OBSERVATION_DIM};
pub use transition::Transition;

/// Convenience function to create a MarketObservation from common Ghost signals.
///
/// Maps Ghost pipeline signals to TCF observation variables.
///
/// # Arguments
///
/// * `price_change_pct` - Price change as percentage (e.g., 0.05 for +5%)
/// * `volume_change_pct` - Volume change as percentage
/// * `buy_pressure_ratio` - Ratio of buys to total trades [0, 1]
/// * `mpcf_confidence` - MPCF actor classification confidence [0, 1]
/// * `interval_cv` - Coefficient of variation of trade intervals
/// * `paradox_sync` - ParadoxSensor phase_sync value [0, 1]
///
/// # Returns
///
/// Normalized `MarketObservation` ready for TCF processing.
pub fn observation_from_ghost_signals(
    price_change_pct: f64,
    volume_change_pct: f64,
    buy_pressure_ratio: f64,
    mpcf_confidence: f64,
    interval_cv: f64,
    paradox_sync: f64,
) -> MarketObservation {
    // Normalize price change (assuming typical range is -20% to +50% for memecoins)
    let price_delta = (price_change_pct / 0.30).clamp(-1.0, 1.0);

    // Normalize volume change (assuming typical range is -50% to +100%)
    let volume_delta = (volume_change_pct / 0.50).clamp(-1.0, 1.0);

    // Convert buy pressure ratio [0, 1] to order flow imbalance [-1, 1]
    let order_flow_imbalance = (buy_pressure_ratio - 0.5) * 2.0;

    // Liquidity entropy: estimate using continuous mapping based on interval CV
    // High CV (irregular intervals) correlates with diverse liquidity sources
    // Low CV (regular intervals) suggests concentrated/bot liquidity
    //
    // Mapping: CV in [0, 1] → entropy in [0.3, 0.8]
    // CV = 0.0 → entropy = 0.3 (very regular, likely bot, low diversity)
    // CV = 0.5 → entropy = 0.55 (mixed)
    // CV = 1.0+ → entropy = 0.8 (highly irregular, diverse sources)
    let liquidity_entropy = (0.3 + 0.5 * interval_cv.clamp(0.0, 1.0)).clamp(0.3, 0.8);

    // Jitter: high CV = irregular timing = more human-like
    let jitter = interval_cv.clamp(0.0, 1.0);

    MarketObservation::new(
        price_delta,
        volume_delta,
        liquidity_entropy,
        order_flow_imbalance,
        mpcf_confidence,
        jitter,
        paradox_sync,
    )
}

// =============================================================================
// Integration Tests
// =============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_pipeline_organic_growth() {
        let mut tcf = TrendCohesionField::new();

        // Simulate organic growth: consistent upward movement
        let observations: Vec<MarketObservation> = (0..13)
            .map(|i| {
                let base = 0.1 + 0.02 * i as f64;
                MarketObservation::new(
                    base,       // Gradually increasing price
                    base * 0.8, // Volume following price
                    0.6,        // Healthy entropy
                    0.3,        // Moderate buy pressure
                    0.7,        // High MPCF confidence
                    0.6,        // Human-like jitter
                    0.2,        // Low sync (independent actors)
                )
            })
            .collect();

        let mut final_result = None;
        for obs in &observations {
            final_result = Some(tcf.update(obs));
        }

        let result = final_result.unwrap();

        // Should show high cohesion and organic growth phase
        assert!(
            result.tcf_score > 0.5,
            "Organic growth should have high TCF: {}",
            result.tcf_score
        );
        assert!(result.is_primed);
    }

    #[test]
    fn test_full_pipeline_pump_and_dump() {
        let mut tcf = TrendCohesionField::new();

        // Phase 1: Pump (cycles 0-6)
        for i in 0..7 {
            let obs = MarketObservation::new(
                0.3 + 0.1 * i as f64, // Strong price increase
                0.4 + 0.1 * i as f64, // Strong volume
                0.5,
                0.6,
                0.5,
                0.3, // Low jitter (bot-like)
                0.7, // High sync (coordinated)
            );
            tcf.update(&obs);
        }

        // Phase 2: Dump (cycles 7-12) - sudden reversal
        for i in 0..6 {
            let obs = MarketObservation::new(
                -0.4 - 0.1 * i as f64, // Sharp price drop
                -0.2,                  // Declining volume
                0.3,
                -0.7, // Heavy selling
                0.5,
                0.4,
                0.5,
            );
            let result = tcf.update(&obs);

            // After a few cycles, should detect the pattern change
            if i >= 2 {
                // TCF score should decrease
                assert!(
                    result.tcf_score < 0.7,
                    "Dump should reduce TCF: {}",
                    result.tcf_score
                );
            }
        }

        let diag = tcf.get_diagnostics();
        // Should have detected issues
        assert!(diag.tcf_score < 0.6 || diag.cliff_detected || diag.trend_direction == -1);
    }

    #[test]
    fn test_observation_from_ghost_signals() {
        let obs = observation_from_ghost_signals(
            0.15, // +15% price
            0.30, // +30% volume
            0.65, // 65% buys
            0.8,  // High MPCF confidence
            0.45, // Moderate interval CV
            0.25, // Low sync
        );

        // Check normalization
        assert!(obs.price_delta > 0.0 && obs.price_delta <= 1.0);
        assert!(obs.volume_delta > 0.0 && obs.volume_delta <= 1.0);
        assert!(obs.order_flow_imbalance > 0.0); // More buys than sells
        assert!(obs.mpcf == 0.8);
    }

    #[test]
    fn test_tcf_modulation_calculation() {
        let tcf_score: f64 = 0.8;

        // Test the modulation formula from documentation
        let base_momentum: f64 = 50.0;
        let effective_momentum = base_momentum * (0.6 + 0.4 * tcf_score);

        // TCF = 0.8 → effective = base * 0.92
        assert!((effective_momentum - 46.0).abs() < 0.1);

        // TCF = 0.0 → effective = base * 0.6
        let low_tcf_score: f64 = 0.0;
        let low_effective = base_momentum * (0.6 + 0.4 * low_tcf_score);
        assert!((low_effective - 30.0).abs() < 0.1);

        // TCF = 1.0 → effective = base * 1.0
        let high_tcf_score: f64 = 1.0;
        let high_effective = base_momentum * (0.6 + 0.4 * high_tcf_score);
        assert!((high_effective - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_phase_based_modulation() {
        // Test that phase modulation factors are sensible
        let phases = [
            TcfPhase::ColdStart,
            TcfPhase::Stable,
            TcfPhase::OrganicGrowth,
            TcfPhase::Pump,
            TcfPhase::Dump,
            TcfPhase::Chaos,
        ];

        for phase in &phases {
            let factor = phase.modulation_factor();
            assert!(
                factor >= 0.0 && factor <= 1.0,
                "{:?} has invalid modulation factor: {}",
                phase,
                factor
            );
        }

        // OrganicGrowth should have highest factor
        assert!(
            TcfPhase::OrganicGrowth.modulation_factor() >= TcfPhase::Stable.modulation_factor()
        );

        // Dump should have lowest non-zero factor
        assert!(TcfPhase::Dump.modulation_factor() < TcfPhase::Chaos.modulation_factor());
    }

    #[test]
    fn test_concurrent_tcf_instances() {
        // Ensure different TCF instances are independent
        let mut tcf1 = TrendCohesionField::new();
        let mut tcf2 = TrendCohesionField::new();

        // Feed different patterns
        for i in 0..8 {
            let obs1 = MarketObservation::new(0.1, 0.1, 0.5, 0.2, 0.5, 0.5, 0.2);
            let obs2 = MarketObservation::new(-0.1 * i as f64, -0.1, 0.3, -0.3, 0.5, 0.3, 0.6);

            tcf1.update(&obs1);
            tcf2.update(&obs2);
        }

        // Scores should differ
        let score1 = tcf1.get_tcf_score();
        let score2 = tcf2.get_tcf_score();

        assert!(
            (score1 - score2).abs() > 0.1 || !tcf1.is_primed() || !tcf2.is_primed(),
            "Different patterns should produce different scores: {} vs {}",
            score1,
            score2
        );
    }
}
