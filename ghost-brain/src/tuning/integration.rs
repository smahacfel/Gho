//! Integration module for HysteresisLoop with trading pipeline
//!
//! This module provides the global HysteresisLoop instance and helper functions
//! to connect ultrafast decisions with followup outcomes.
//!
//! # Architecture
//!
//! The HysteresisLoop connects two phases of the trading pipeline:
//!
//! 1. **Ultrafast Decision (T<2s)**: Initial BUY/SKIP decision
//! 2. **Followup Scoring (1s, 5s, 30s, 60s)**: HOLD/SELL with corrections
//!
//! The loop tracks outcomes and uses bandit algorithms to optimize weights
//! for future decisions.
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_brain::tuning::integration::{
//!     register_decision, register_outcome, get_current_weights, get_loop_stats
//! };
//!
//! // Before scoring: get current optimized weights
//! let context = TuningContext::default();
//! let weights = get_current_weights(&context);
//!
//! // After scoring: register the decision
//! register_decision(
//!     candidate_id,
//!     score,
//!     decision,
//!     components,
//!     context,
//! );
//!
//! // After followup: register the outcome
//! register_outcome(outcome);
//! ```

use crate::oracle::{DecisionType, InitialComponents};
use crate::tuning::{
    BanditAlgorithm, DecisionOutcome, HysteresisConfig, HysteresisLoop, LoopStats, TunableWeights,
    TuningContext,
};
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

/// Global HysteresisLoop instance
///
/// This is initialized once at startup and shared across the trading pipeline.
/// Uses Arc<RwLock<>> for thread-safe access from multiple tasks.
///
/// # Configuration
/// - Algorithm: LinUCB (better for cold start, adapts quickly)
/// - Dry Run: false (live weight updates)
/// - Dampening: 0.7 (conservative, prevents oscillations)
/// - Cooldown: 5000ms (prevents rapid weight changes)
pub static HYSTERESIS_LOOP: Lazy<Arc<RwLock<HysteresisLoop>>> = Lazy::new(|| {
    let config = HysteresisConfig {
        enabled: true,
        dry_run: false, // Set to true for initial testing
        update_cooldown_ms: 5000,
        dampening_factor: 0.7,
        max_pending_decisions: 1000,
        outcome_timeout_seconds: 300, // 5 minutes
        monitor_repeatability: true,
        repeatability_window: 20,
        repeatability_threshold: 0.85,
    };

    let initial_weights = TunableWeights::default();

    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create HysteresisLoop");

    info!("HysteresisLoop initialized: algorithm=LinUCB, dry_run=false, dampening=0.7");

    Arc::new(RwLock::new(loop_instance))
});

/// Register an ultrafast decision with the HysteresisLoop
///
/// Call this immediately after `score_candidate()` returns a decision.
///
/// # Arguments
/// * `candidate_id` - Unique identifier (e.g., pool_amm_id.to_string())
/// * `score` - The final score from HyperPredictionResult
/// * `decision` - BUY or SKIP
/// * `components` - InitialComponents from the scoring
/// * `context` - TuningContext derived from market signals
pub fn register_decision(
    candidate_id: String,
    score: u8,
    decision: DecisionType,
    components: InitialComponents,
    context: TuningContext,
) {
    match HYSTERESIS_LOOP.write() {
        Ok(mut loop_guard) => {
            loop_guard.register_decision(candidate_id, score, decision, components, context);
        }
        Err(e) => {
            warn!(
                "Failed to acquire HysteresisLoop lock for decision registration: {}",
                e
            );
        }
    }
}

/// Register a trade outcome with the HysteresisLoop
///
/// Call this after followup scoring determines the final outcome.
///
/// # Arguments
/// * `outcome` - DecisionOutcome containing profit/loss and corrections
pub fn register_outcome(outcome: DecisionOutcome) {
    match HYSTERESIS_LOOP.write() {
        Ok(mut loop_guard) => {
            loop_guard.register_outcome(outcome);
        }
        Err(e) => {
            warn!(
                "Failed to acquire HysteresisLoop lock for outcome registration: {}",
                e
            );
        }
    }
}

/// Get current optimized weights for ultrafast scoring
///
/// Call this before `score_candidate()` to get the latest weights.
///
/// # Arguments
/// * `context` - TuningContext for context-aware weight suggestion
///
/// # Returns
/// TunableWeights - the current optimized weights
pub fn get_current_weights(context: &TuningContext) -> TunableWeights {
    match HYSTERESIS_LOOP.read() {
        Ok(loop_guard) => loop_guard.suggest_weights(context),
        Err(e) => {
            warn!(
                "Failed to acquire HysteresisLoop lock, using default weights: {}",
                e
            );
            TunableWeights::default()
        }
    }
}

/// Get current HysteresisLoop statistics
///
/// # Returns
/// Some(LoopStats) if the lock is acquired, None otherwise
pub fn get_loop_stats() -> Option<LoopStats> {
    HYSTERESIS_LOOP.read().ok().map(|guard| guard.stats())
}

/// Check if the HysteresisLoop is enabled
///
/// # Returns
/// true if the loop is enabled and ready to track decisions
pub fn is_enabled() -> bool {
    match HYSTERESIS_LOOP.read() {
        Ok(guard) => guard.stats().is_dry_run == false,
        Err(_) => false,
    }
}

/// Cleanup expired pending decisions
///
/// This should be called periodically to free memory from decisions
/// that never received outcomes (e.g., due to network issues).
pub fn cleanup_expired() {
    match HYSTERESIS_LOOP.write() {
        Ok(loop_guard) => {
            loop_guard.cleanup_expired();
            debug!("HysteresisLoop: expired decisions cleaned up");
        }
        Err(e) => {
            warn!("Failed to acquire HysteresisLoop lock for cleanup: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_context() -> TuningContext {
        TuningContext {
            volatility: 0.5,
            volume: 0.7,
            bot_activity: 0.3,
            mci: 0.8,
            time_factor: 0.5,
            pool_age_seconds: 60,
        }
    }

    fn create_test_components() -> InitialComponents {
        InitialComponents {
            base_shadow: 75,
            qass_score: 80.0,
            qedd_survival_30s: Some(0.7),
            mci: Some(0.8),
            chaos_loss_prob: Some(0.1),
            gene_match_score: Some(0.05),
            confidence: Some(0.85),
            extras: HashMap::new(),
        }
    }

    #[test]
    fn test_global_hysteresis_loop_initialization() {
        // Access the global loop - this should initialize it
        let stats = get_loop_stats();
        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.total_decisions, 0);
        assert_eq!(stats.total_outcomes, 0);
    }

    #[test]
    fn test_get_current_weights() {
        let context = create_test_context();
        let weights = get_current_weights(&context);

        // Should return default weights initially
        assert_eq!(weights, TunableWeights::default());
    }

    #[test]
    fn test_register_decision() {
        let context = create_test_context();
        let components = create_test_components();

        // Register a decision
        register_decision(
            "test_pool_integration_1".to_string(),
            75,
            DecisionType::Buy,
            components,
            context,
        );

        // Verify the decision was registered
        let stats = get_loop_stats().unwrap();
        // Note: We can't assert exact counts here because tests run in parallel
        // and may share the global state
        assert!(stats.total_decisions >= 1);
    }

    #[test]
    fn test_register_outcome() {
        let context = create_test_context();
        let components = create_test_components();
        let pool_id = "test_pool_integration_2".to_string();

        // Register a decision first
        register_decision(pool_id.clone(), 75, DecisionType::Buy, components, context);

        // Register the outcome
        let outcome = DecisionOutcome {
            candidate_id: pool_id,
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.20), // 20% profit
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 60,
        };

        register_outcome(outcome);

        // Verify outcome was processed
        let stats = get_loop_stats().unwrap();
        assert!(stats.total_outcomes >= 1);
    }

    #[test]
    fn test_cleanup_expired() {
        // This should not panic even if no decisions are pending
        cleanup_expired();
    }

    #[test]
    fn test_is_enabled() {
        // The global loop is configured with dry_run=false by default
        let enabled = is_enabled();
        assert!(enabled);
    }
}
