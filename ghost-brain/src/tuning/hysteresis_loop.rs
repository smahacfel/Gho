//! Hysteresis Learning Loop
//!
//! This module implements the feedback loop connecting ultrafast decisions (T<2s)
//! with followup scoring outcomes (T>2s) to continuously update weights via bandits.
//!
//! # Architecture
//!
//! ```text
//! Ultrafast Decision (T<2s)     →     Followup Scoring (1s, 5s, 30s, 60s)
//!       ↓ BUY/SKIP                           ↓ HOLD/SELL + corrections
//!       |                                     |
//!       └─────────→  Outcome Tracker  ←──────┘
//!                         ↓
//!                    Profit/Loss
//!                         ↓
//!                    Reward Calc
//!                         ↓
//!                   Bandits Update
//!                         ↓
//!                  New Ultrafast Weights (next mint)
//! ```
//!
//! # Key Features
//!
//! - **Outcome Tracking**: Links initial ultrafast decisions with final trading outcomes
//! - **Reward Calculation**: Converts profit/loss and followup corrections into reward signals
//! - **Weight Feedback**: Updates ultrafast weights through bandit algorithms
//! - **Hysteresis Control**: Dampens oscillations and ensures decision stability
//! - **Dry Run Support**: Full monitoring without actual weight updates
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_brain::tuning::hysteresis_loop::{HysteresisLoop, HysteresisConfig};
//!
//! // Create loop with configuration
//! let config = HysteresisConfig::default();
//! let mut loop_instance = HysteresisLoop::new(config);
//!
//! // Register ultrafast decision
//! loop_instance.register_decision(candidate_id, initial_score, weights_used);
//!
//! // Later: register outcome
//! loop_instance.register_outcome(candidate_id, outcome);
//!
//! // Get updated weights for next decision
//! let new_weights = loop_instance.suggest_weights(&context);
//! ```

use crate::oracle::{CorrectionReason, DecisionType, FollowupScore, InitialComponents};
use crate::tuning::{
    BanditAlgorithm, RewardCalculator, RewardSignal, TradeOutcome, TunableWeights, TuningContext,
    WeightBandit,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Configuration for hysteresis learning loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HysteresisConfig {
    /// Enable learning loop
    pub enabled: bool,
    /// Enable dry-run mode (monitoring only, no weight updates)
    pub dry_run: bool,
    /// Minimum time between weight updates (prevents oscillations)
    pub update_cooldown_ms: u64,
    /// Weight change dampening factor (0.0 to 1.0)
    pub dampening_factor: f32,
    /// Maximum number of pending decisions to track
    pub max_pending_decisions: usize,
    /// Outcome timeout - discard if no outcome after this duration
    pub outcome_timeout_seconds: u64,
    /// Enable decision repeatability monitoring
    pub monitor_repeatability: bool,
    /// Repeatability check window (number of recent decisions)
    pub repeatability_window: usize,
    /// Repeatability threshold (0.0 to 1.0) - higher = more stable
    pub repeatability_threshold: f32,
}

impl Default for HysteresisConfig {
    /// Create default hysteresis configuration with production-safe values
    ///
    /// # Default Values Rationale
    ///
    /// - `update_cooldown_ms = 5000` (5s): Prevents rapid oscillations in live trading
    ///   while still allowing responsive adaptation. Shorter would risk noise amplification.
    /// - `dampening_factor = 0.7`: Conservative blending (70% old + 30% new) provides
    ///   stability while allowing gradual weight evolution. Higher = more stable but slower.
    /// - `outcome_timeout_seconds = 300` (5min): Reasonable window for followup scoring
    ///   to complete. Most outcomes resolve within 60s, but allows for delayed processing.
    /// - `repeatability_threshold = 0.85`: High bar ensures weight stability. Below this
    ///   indicates potential oscillation or unstable market conditions.
    /// - `max_pending_decisions = 1000`: Bounds memory usage (~1MB) while accommodating
    ///   high-frequency trading scenarios.
    fn default() -> Self {
        Self {
            enabled: true,
            dry_run: false,
            update_cooldown_ms: 5000,
            dampening_factor: 0.7,
            max_pending_decisions: 1000,
            outcome_timeout_seconds: 300,
            monitor_repeatability: true,
            repeatability_window: 20,
            repeatability_threshold: 0.85,
        }
    }
}

impl HysteresisConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.dampening_factor < 0.0 || self.dampening_factor > 1.0 {
            anyhow::bail!(
                "dampening_factor must be in [0.0, 1.0], got {}",
                self.dampening_factor
            );
        }
        if self.repeatability_threshold < 0.0 || self.repeatability_threshold > 1.0 {
            anyhow::bail!(
                "repeatability_threshold must be in [0.0, 1.0], got {}",
                self.repeatability_threshold
            );
        }
        Ok(())
    }
}

/// Record of an ultrafast decision awaiting outcome
#[derive(Debug, Clone)]
struct PendingDecision {
    /// Candidate ID
    candidate_id: String,
    /// Initial score
    initial_score: u8,
    /// Initial decision (BUY/SKIP)
    initial_decision: DecisionType,
    /// Weights used for this decision
    weights_used: TunableWeights,
    /// Initial components
    initial_components: InitialComponents,
    /// Tuning context at decision time
    context: TuningContext,
    /// Decision timestamp
    timestamp: Instant,
}

/// Outcome of a decision after followup scoring
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    /// Candidate ID
    pub candidate_id: String,
    /// Final decision after followups
    pub final_decision: DecisionType,
    /// All followup scores
    pub followup_scores: Vec<FollowupScore>,
    /// Profit ratio (if trade executed)
    pub profit_ratio: Option<f32>,
    /// Total corrections applied
    pub total_corrections: usize,
    /// Whether this was a successful trade
    pub was_successful: bool,
    /// Time from initial decision to outcome
    pub elapsed_seconds: u64,
}

impl DecisionOutcome {
    /// Convert to TradeOutcome for reward calculation
    pub fn to_trade_outcome(&self, initial_decision: &DecisionType) -> TradeOutcome {
        let signal_generated = matches!(initial_decision, DecisionType::Buy);
        let trade_executed = signal_generated && self.profit_ratio.is_some();

        let was_true_positive = trade_executed && self.was_successful;
        let was_false_positive = trade_executed && !self.was_successful;
        let was_false_negative = !signal_generated && self.was_successful;

        TradeOutcome {
            signal_generated,
            trade_executed,
            profit_ratio: self.profit_ratio,
            was_true_positive,
            was_false_positive,
            was_false_negative,
            time_delay_seconds: self.elapsed_seconds,
            confidence_score: 0.0, // Could be extracted from followup scores
            pool_age_seconds: 0,   // Could be extracted from context
        }
    }
}

/// Statistics about the learning loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopStats {
    /// Total decisions processed
    pub total_decisions: u64,
    /// Total outcomes received
    pub total_outcomes: u64,
    /// Pending decisions count
    pub pending_count: usize,
    /// Total weight updates performed
    pub weight_updates: u64,
    /// Last weight update time (seconds ago)
    pub seconds_since_update: u64,
    /// Current weights
    pub current_weights: TunableWeights,
    /// Cumulative reward
    pub cumulative_reward: f64,
    /// Average reward per outcome
    pub average_reward: f64,
    /// Decision repeatability score (0.0 to 1.0)
    pub repeatability_score: f32,
    /// Whether in dry-run mode
    pub is_dry_run: bool,
}

/// Main hysteresis learning loop implementation
pub struct HysteresisLoop {
    /// Configuration
    config: HysteresisConfig,
    /// Current weights
    current_weights: Arc<Mutex<TunableWeights>>,
    /// Bandit algorithm for weight updates
    bandit: Arc<Mutex<Box<dyn WeightBandit + Send + Sync>>>,
    /// Reward calculator
    reward_calc: RewardCalculator,
    /// Pending decisions awaiting outcomes
    pending: Arc<Mutex<HashMap<String, PendingDecision>>>,
    /// Recent decisions for repeatability monitoring
    recent_decisions: Arc<Mutex<VecDeque<(TunableWeights, DecisionType)>>>,
    /// Last weight update timestamp
    last_update: Arc<Mutex<Instant>>,
    /// Statistics
    stats: Arc<Mutex<LoopStats>>,
}

impl HysteresisLoop {
    /// Create a new hysteresis learning loop
    pub fn new(
        config: HysteresisConfig,
        bandit_algorithm: BanditAlgorithm,
        initial_weights: TunableWeights,
    ) -> Result<Self> {
        config.validate()?;

        let bandit: Box<dyn WeightBandit + Send + Sync> = match bandit_algorithm {
            BanditAlgorithm::LinUCB => Box::new(crate::tuning::bandits::LinUCBBandit::new(
                crate::tuning::config::BanditConfig::default(),
            )),
            BanditAlgorithm::ThompsonSampling => {
                Box::new(crate::tuning::bandits::ThompsonSamplingBandit::new(
                    crate::tuning::config::BanditConfig::default(),
                ))
            }
        };

        let stats = LoopStats {
            total_decisions: 0,
            total_outcomes: 0,
            pending_count: 0,
            weight_updates: 0,
            seconds_since_update: 0,
            current_weights: initial_weights,
            cumulative_reward: 0.0,
            average_reward: 0.0,
            repeatability_score: 1.0,
            is_dry_run: config.dry_run,
        };

        Ok(Self {
            config,
            current_weights: Arc::new(Mutex::new(initial_weights)),
            bandit: Arc::new(Mutex::new(bandit)),
            reward_calc: RewardCalculator::default(),
            pending: Arc::new(Mutex::new(HashMap::new())),
            recent_decisions: Arc::new(Mutex::new(VecDeque::new())),
            last_update: Arc::new(Mutex::new(Instant::now())),
            stats: Arc::new(Mutex::new(stats)),
        })
    }

    /// Register an ultrafast decision
    pub fn register_decision(
        &self,
        candidate_id: String,
        initial_score: u8,
        initial_decision: DecisionType,
        initial_components: InitialComponents,
        context: TuningContext,
    ) {
        if !self.config.enabled {
            return;
        }

        let weights_used = *self.current_weights.lock().unwrap();

        let decision = PendingDecision {
            candidate_id: candidate_id.clone(),
            initial_score,
            initial_decision: initial_decision.clone(),
            weights_used,
            initial_components,
            context,
            timestamp: Instant::now(),
        };

        let mut pending = self.pending.lock().unwrap();

        // Enforce max pending limit
        // Note: Since HashMap has no ordering, we remove an arbitrary entry when full.
        // In practice, this is acceptable because:
        // 1. Hitting the limit indicates system overload or outcome processing delays
        // 2. The specific dropped decision is less important than preventing memory growth
        // 3. All pending decisions are equally likely to timeout anyway at this point
        // For deterministic oldest-first removal, consider using a BTreeMap or separate queue
        if pending.len() >= self.config.max_pending_decisions {
            warn!(
                "Max pending decisions reached ({}), dropping arbitrary entry to prevent memory growth",
                self.config.max_pending_decisions
            );
            if let Some(key) = pending.keys().next().cloned() {
                pending.remove(&key);
            }
        }

        pending.insert(candidate_id.clone(), decision);

        // Update statistics
        let mut stats = self.stats.lock().unwrap();
        stats.total_decisions += 1;
        stats.pending_count = pending.len();

        // Track for repeatability
        if self.config.monitor_repeatability {
            let mut recent = self.recent_decisions.lock().unwrap();
            recent.push_back((weights_used, initial_decision.clone()));
            if recent.len() > self.config.repeatability_window {
                recent.pop_front();
            }
        }

        debug!(
            "Registered decision for {}: score={}, decision={:?}, weights={:?}",
            candidate_id, initial_score, initial_decision, weights_used
        );
    }

    /// Register an outcome and trigger learning update
    pub fn register_outcome(&self, outcome: DecisionOutcome) {
        if !self.config.enabled {
            return;
        }

        let mut pending = self.pending.lock().unwrap();
        let decision = match pending.remove(&outcome.candidate_id) {
            Some(d) => d,
            None => {
                debug!(
                    "No pending decision found for outcome: {}",
                    outcome.candidate_id
                );
                return;
            }
        };

        // Check if outcome is too late
        if decision.timestamp.elapsed().as_secs() > self.config.outcome_timeout_seconds {
            warn!(
                "Outcome for {} arrived too late ({} seconds), discarding",
                outcome.candidate_id,
                decision.timestamp.elapsed().as_secs()
            );
            return;
        }

        // Convert to TradeOutcome
        let trade_outcome = outcome.to_trade_outcome(&decision.initial_decision);

        // Calculate reward
        let reward = self.reward_calc.calculate(&trade_outcome);

        info!(
            "Outcome for {}: final_decision={:?}, profit_ratio={:?}, reward={:.4}",
            outcome.candidate_id, outcome.final_decision, outcome.profit_ratio, reward.total
        );

        // Update statistics
        let mut stats = self.stats.lock().unwrap();
        stats.total_outcomes += 1;
        stats.pending_count = pending.len();
        stats.cumulative_reward += reward.total as f64;
        stats.average_reward = stats.cumulative_reward / stats.total_outcomes as f64;

        drop(stats); // Release lock before potentially updating weights

        // Check cooldown
        let last_update = *self.last_update.lock().unwrap();
        let cooldown = Duration::from_millis(self.config.update_cooldown_ms);
        if last_update.elapsed() < cooldown {
            debug!(
                "Update cooldown active ({} ms remaining)",
                cooldown.as_millis() - last_update.elapsed().as_millis()
            );
            return;
        }

        // Perform weight update
        self.update_weights(&decision.context, reward);
    }

    /// Update weights based on reward signal
    fn update_weights(&self, context: &TuningContext, reward: RewardSignal) {
        if self.config.dry_run {
            debug!(
                "DRY RUN: Would update weights with reward={:.4}",
                reward.total
            );
            return;
        }

        let features = context.to_features();

        // Update bandit
        let mut bandit = self.bandit.lock().unwrap();
        bandit.update(&features, reward.total);

        // Get new suggested weights
        let suggested = bandit.suggest_weights(&features);
        drop(bandit); // Release lock

        let mut current = self.current_weights.lock().unwrap();

        // Apply dampening to prevent oscillations
        let dampen = self.config.dampening_factor;
        let new_weights = TunableWeights {
            w_qass: dampen * current.w_qass + (1.0 - dampen) * suggested[0],
            w_mpcf: dampen * current.w_mpcf + (1.0 - dampen) * suggested[1],
            w_sobp: dampen * current.w_sobp + (1.0 - dampen) * suggested[2],
            w_iwim: dampen * current.w_iwim + (1.0 - dampen) * suggested[3],
        };

        *current = new_weights;

        // Update statistics
        let mut stats = self.stats.lock().unwrap();
        stats.weight_updates += 1;
        stats.current_weights = new_weights;

        // Update repeatability score
        if self.config.monitor_repeatability {
            stats.repeatability_score = self.calculate_repeatability();
        }

        *self.last_update.lock().unwrap() = Instant::now();

        info!(
            "Weight update #{}: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}, reward={:.4}",
            stats.weight_updates,
            new_weights.w_qass,
            new_weights.w_mpcf,
            new_weights.w_sobp,
            new_weights.w_iwim,
            reward.total
        );
    }

    /// Suggest weights for next decision
    pub fn suggest_weights(&self, context: &TuningContext) -> TunableWeights {
        let current = *self.current_weights.lock().unwrap();

        if self.config.dry_run {
            debug!(
                "DRY RUN: Suggesting weights: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}",
                current.w_qass, current.w_mpcf, current.w_sobp, current.w_iwim
            );
        }

        // Could use bandit to refine suggestion based on context
        // For now, return current weights
        current
    }

    /// Calculate decision repeatability score
    fn calculate_repeatability(&self) -> f32 {
        let recent = self.recent_decisions.lock().unwrap();
        if recent.len() < 2 {
            return 1.0;
        }

        // Count consistent decisions (same decision type for similar weights)
        let mut consistent = 0;
        let total = recent.len() - 1;

        for i in 0..total {
            let (w1, d1) = &recent[i];
            let (w2, d2) = &recent[i + 1];

            // Check if weights are similar using relative or absolute threshold
            // Use absolute threshold (1.0) to handle near-zero weights gracefully
            let threshold_qass = if w1.w_qass > 1.0 {
                w1.w_qass * 0.1
            } else {
                1.0
            };
            let threshold_mpcf = if w1.w_mpcf > 1.0 {
                w1.w_mpcf * 0.1
            } else {
                1.0
            };
            let threshold_sobp = if w1.w_sobp > 1.0 {
                w1.w_sobp * 0.1
            } else {
                1.0
            };
            let threshold_iwim = if w1.w_iwim > 1.0 {
                w1.w_iwim * 0.1
            } else {
                1.0
            };

            let w_similar = (w1.w_qass - w2.w_qass).abs() < threshold_qass
                && (w1.w_mpcf - w2.w_mpcf).abs() < threshold_mpcf
                && (w1.w_sobp - w2.w_sobp).abs() < threshold_sobp
                && (w1.w_iwim - w2.w_iwim).abs() < threshold_iwim;

            if w_similar && d1 == d2 {
                consistent += 1;
            }
        }

        consistent as f32 / total as f32
    }

    /// Get current statistics
    pub fn stats(&self) -> LoopStats {
        let mut stats = self.stats.lock().unwrap();
        stats.seconds_since_update = self.last_update.lock().unwrap().elapsed().as_secs();
        stats.pending_count = self.pending.lock().unwrap().len();
        stats.clone()
    }

    /// Clean up expired pending decisions
    pub fn cleanup_expired(&self) {
        let mut pending = self.pending.lock().unwrap();
        let timeout = Duration::from_secs(self.config.outcome_timeout_seconds);

        let expired: Vec<_> = pending
            .iter()
            .filter(|(_, d)| d.timestamp.elapsed() > timeout)
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired {
            pending.remove(&key);
            debug!("Cleaned up expired decision: {}", key);
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
            base_shadow: 60,
            qass_score: 75.0,
            qedd_survival_30s: Some(0.7),
            mci: Some(0.8),
            chaos_loss_prob: Some(0.1),
            gene_match_score: Some(0.05),
            confidence: Some(0.85),
            extras: HashMap::new(),
        }
    }

    #[test]
    fn test_hysteresis_config_default() {
        let config = HysteresisConfig::default();
        assert!(config.enabled);
        assert!(!config.dry_run);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_hysteresis_config_validation() {
        let mut config = HysteresisConfig::default();

        config.dampening_factor = 1.5;
        assert!(config.validate().is_err());
        config.dampening_factor = 0.7;

        config.repeatability_threshold = -0.1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_loop_creation() {
        let config = HysteresisConfig::default();
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let stats = loop_instance.stats();
        assert_eq!(stats.total_decisions, 0);
        assert_eq!(stats.total_outcomes, 0);
    }

    #[test]
    fn test_register_decision() {
        let config = HysteresisConfig::default();
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let context = create_test_context();
        let components = create_test_components();

        loop_instance.register_decision(
            "test_pool_1".to_string(),
            75,
            DecisionType::Buy,
            components,
            context,
        );

        let stats = loop_instance.stats();
        assert_eq!(stats.total_decisions, 1);
        assert_eq!(stats.pending_count, 1);
    }

    #[test]
    fn test_register_outcome_success() {
        let config = HysteresisConfig::default();
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let context = create_test_context();
        let components = create_test_components();

        // Register decision
        loop_instance.register_decision(
            "test_pool_2".to_string(),
            75,
            DecisionType::Buy,
            components,
            context,
        );

        // Register successful outcome
        let outcome = DecisionOutcome {
            candidate_id: "test_pool_2".to_string(),
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.2), // 20% profit
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 60,
        };

        loop_instance.register_outcome(outcome);

        let stats = loop_instance.stats();
        assert_eq!(stats.total_outcomes, 1);
        assert!(stats.cumulative_reward > 0.0);
    }

    #[test]
    fn test_dry_run_mode() {
        let mut config = HysteresisConfig::default();
        config.dry_run = true;
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let context = create_test_context();
        let components = create_test_components();

        loop_instance.register_decision(
            "test_pool_3".to_string(),
            75,
            DecisionType::Buy,
            components,
            context,
        );

        let outcome = DecisionOutcome {
            candidate_id: "test_pool_3".to_string(),
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.1),
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 30,
        };

        loop_instance.register_outcome(outcome);

        let stats = loop_instance.stats();
        assert!(stats.is_dry_run);
        assert_eq!(stats.weight_updates, 0); // No updates in dry-run
    }

    #[test]
    fn test_suggest_weights() {
        let config = HysteresisConfig::default();
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let context = create_test_context();
        let suggested = loop_instance.suggest_weights(&context);

        assert_eq!(suggested, TunableWeights::default());
    }

    #[test]
    fn test_repeatability_calculation() {
        let config = HysteresisConfig::default();
        let weights = TunableWeights::default();
        let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, weights).unwrap();

        let context = create_test_context();
        let components = create_test_components();

        // Register several consistent decisions
        for i in 0..5 {
            loop_instance.register_decision(
                format!("pool_{}", i),
                75,
                DecisionType::Buy,
                components.clone(),
                context.clone(),
            );
        }

        let stats = loop_instance.stats();
        assert!(stats.repeatability_score > 0.8);
    }
}
