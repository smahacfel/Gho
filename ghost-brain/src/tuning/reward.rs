//! Reward Calculation
//!
//! Computes reward signals for weight tuning based on trade outcomes.
//! Rewards are used by both online bandits and offline Bayesian optimization.

use crate::tuning::config::RewardConfig;
use serde::{Deserialize, Serialize};

/// Trade outcome for reward calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOutcome {
    /// Whether a BUY signal was generated
    pub signal_generated: bool,

    /// Whether the trade was executed
    pub trade_executed: bool,

    /// Profit/loss ratio (e.g., 1.05 for 5% profit)
    pub profit_ratio: Option<f32>,

    /// Whether the signal was a true positive
    pub was_true_positive: bool,

    /// Whether the signal was a false positive
    pub was_false_positive: bool,

    /// Whether we missed an opportunity (false negative)
    pub was_false_negative: bool,

    /// Time delay in seconds from signal to outcome
    pub time_delay_seconds: u64,

    /// Confidence score at time of signal
    pub confidence_score: f32,

    /// Pool age at time of signal
    pub pool_age_seconds: u64,
}

impl Default for TradeOutcome {
    fn default() -> Self {
        Self {
            signal_generated: false,
            trade_executed: false,
            profit_ratio: None,
            was_true_positive: false,
            was_false_positive: false,
            was_false_negative: false,
            time_delay_seconds: 0,
            confidence_score: 0.0,
            pool_age_seconds: 0,
        }
    }
}

impl TradeOutcome {
    /// Create a successful trade outcome
    pub fn success(profit_ratio: f32, confidence: f32) -> Self {
        Self {
            signal_generated: true,
            trade_executed: true,
            profit_ratio: Some(profit_ratio),
            was_true_positive: true,
            was_false_positive: false,
            was_false_negative: false,
            time_delay_seconds: 0,
            confidence_score: confidence,
            pool_age_seconds: 0,
        }
    }

    /// Create a failed trade outcome (loss)
    pub fn loss(profit_ratio: f32, confidence: f32) -> Self {
        Self {
            signal_generated: true,
            trade_executed: true,
            profit_ratio: Some(profit_ratio),
            was_true_positive: false,
            was_false_positive: true,
            was_false_negative: false,
            time_delay_seconds: 0,
            confidence_score: confidence,
            pool_age_seconds: 0,
        }
    }

    /// Create a missed opportunity outcome
    pub fn missed_opportunity() -> Self {
        Self {
            signal_generated: false,
            trade_executed: false,
            profit_ratio: None,
            was_true_positive: false,
            was_false_positive: false,
            was_false_negative: true,
            time_delay_seconds: 0,
            confidence_score: 0.0,
            pool_age_seconds: 0,
        }
    }

    /// Create a correct rejection outcome (no signal, no opportunity)
    pub fn correct_rejection() -> Self {
        Self {
            signal_generated: false,
            trade_executed: false,
            profit_ratio: None,
            was_true_positive: false,
            was_false_positive: false,
            was_false_negative: false,
            time_delay_seconds: 0,
            confidence_score: 0.0,
            pool_age_seconds: 0,
        }
    }
}

/// Computed reward signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardSignal {
    /// Total reward value
    pub total: f32,

    /// Profit component of reward
    pub profit_component: f32,

    /// Penalty component (negative)
    pub penalty_component: f32,

    /// Time discount factor applied
    pub time_discount_applied: f32,

    /// Original raw reward before clipping
    pub raw_reward: f32,
}

/// Reward calculator
pub struct RewardCalculator {
    config: RewardConfig,
}

impl RewardCalculator {
    /// Create a new reward calculator
    pub fn new(config: RewardConfig) -> Self {
        Self { config }
    }

    /// Calculate reward from trade outcome
    pub fn calculate(&self, outcome: &TradeOutcome) -> RewardSignal {
        let mut profit_component = 0.0;
        let mut penalty_component = 0.0;

        // Calculate profit/loss component
        if let Some(ratio) = outcome.profit_ratio {
            if ratio > 1.0 {
                // Profit
                let profit_pct = ratio - 1.0;
                profit_component =
                    self.config.success_reward + self.config.profit_scale * profit_pct;
            } else if ratio < 1.0 {
                // Loss
                let loss_pct = 1.0 - ratio;
                penalty_component = -self.config.loss_scale * loss_pct;
            }
        }

        // Apply penalties for false signals
        if outcome.was_false_positive {
            penalty_component += self.config.false_positive_penalty;
        }

        if outcome.was_false_negative {
            penalty_component += self.config.false_negative_penalty;
        }

        // Apply time discount
        let time_discount = self
            .config
            .time_discount
            .powf(outcome.time_delay_seconds as f32 / 60.0);

        let raw_reward = (profit_component + penalty_component) * time_discount;

        // Clip reward
        let total = raw_reward.clamp(self.config.min_reward, self.config.max_reward);

        RewardSignal {
            total,
            profit_component,
            penalty_component,
            time_discount_applied: time_discount,
            raw_reward,
        }
    }

    /// Calculate batch rewards
    pub fn calculate_batch(&self, outcomes: &[TradeOutcome]) -> Vec<RewardSignal> {
        outcomes.iter().map(|o| self.calculate(o)).collect()
    }

    /// Calculate cumulative reward
    pub fn cumulative_reward(&self, outcomes: &[TradeOutcome]) -> f32 {
        self.calculate_batch(outcomes).iter().map(|r| r.total).sum()
    }

    /// Calculate average reward
    pub fn average_reward(&self, outcomes: &[TradeOutcome]) -> f32 {
        if outcomes.is_empty() {
            return 0.0;
        }
        self.cumulative_reward(outcomes) / outcomes.len() as f32
    }
}

impl Default for RewardCalculator {
    fn default() -> Self {
        Self::new(RewardConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_calc() -> RewardCalculator {
        RewardCalculator::default()
    }

    #[test]
    fn test_trade_outcome_success() {
        let outcome = TradeOutcome::success(1.1, 0.8);
        assert!(outcome.was_true_positive);
        assert!(!outcome.was_false_positive);
        assert_eq!(outcome.profit_ratio, Some(1.1));
    }

    #[test]
    fn test_trade_outcome_loss() {
        let outcome = TradeOutcome::loss(0.9, 0.7);
        assert!(!outcome.was_true_positive);
        assert!(outcome.was_false_positive);
        assert_eq!(outcome.profit_ratio, Some(0.9));
    }

    #[test]
    fn test_reward_positive_profit() {
        let calc = default_calc();
        let outcome = TradeOutcome::success(1.2, 0.8); // 20% profit

        let reward = calc.calculate(&outcome);
        assert!(reward.total > 0.0, "Profit should give positive reward");
        assert!(reward.profit_component > 0.0);
    }

    #[test]
    fn test_reward_negative_loss() {
        let calc = default_calc();
        let outcome = TradeOutcome::loss(0.8, 0.6); // 20% loss

        let reward = calc.calculate(&outcome);
        assert!(reward.total < 0.0, "Loss should give negative reward");
        assert!(reward.penalty_component < 0.0);
    }

    #[test]
    fn test_reward_false_positive_penalty() {
        let calc = default_calc();
        let mut outcome = TradeOutcome::default();
        outcome.signal_generated = true;
        outcome.was_false_positive = true;

        let reward = calc.calculate(&outcome);
        assert!(reward.penalty_component < 0.0);
        assert_eq!(reward.penalty_component, calc.config.false_positive_penalty);
    }

    #[test]
    fn test_reward_false_negative_penalty() {
        let calc = default_calc();
        let outcome = TradeOutcome::missed_opportunity();

        let reward = calc.calculate(&outcome);
        assert!(reward.penalty_component < 0.0);
        assert_eq!(reward.penalty_component, calc.config.false_negative_penalty);
    }

    #[test]
    fn test_reward_time_discount() {
        let calc = default_calc();

        let mut outcome_immediate = TradeOutcome::success(1.1, 0.8);
        outcome_immediate.time_delay_seconds = 0;

        let mut outcome_delayed = TradeOutcome::success(1.1, 0.8);
        outcome_delayed.time_delay_seconds = 300; // 5 minutes

        let reward_immediate = calc.calculate(&outcome_immediate);
        let reward_delayed = calc.calculate(&outcome_delayed);

        assert!(
            reward_immediate.total >= reward_delayed.total,
            "Immediate reward should be >= delayed reward"
        );
    }

    #[test]
    fn test_reward_clipping() {
        let calc = default_calc();

        // Very high profit
        let outcome = TradeOutcome::success(5.0, 0.9); // 400% profit
        let reward = calc.calculate(&outcome);
        assert!(
            reward.total <= calc.config.max_reward,
            "Reward should be clipped to max"
        );

        // Very high loss
        let outcome = TradeOutcome::loss(0.1, 0.3); // 90% loss
        let reward = calc.calculate(&outcome);
        assert!(
            reward.total >= calc.config.min_reward,
            "Reward should be clipped to min"
        );
    }

    #[test]
    fn test_batch_rewards() {
        let calc = default_calc();
        let outcomes = vec![
            TradeOutcome::success(1.1, 0.8),
            TradeOutcome::loss(0.9, 0.6),
            TradeOutcome::correct_rejection(),
        ];

        let rewards = calc.calculate_batch(&outcomes);
        assert_eq!(rewards.len(), 3);
    }

    #[test]
    fn test_cumulative_reward() {
        let calc = default_calc();
        let outcomes = vec![
            TradeOutcome::success(1.1, 0.8),
            TradeOutcome::success(1.2, 0.7),
        ];

        let cumulative = calc.cumulative_reward(&outcomes);
        assert!(cumulative > 0.0);
    }

    #[test]
    fn test_average_reward() {
        let calc = default_calc();
        let outcomes = vec![
            TradeOutcome::success(1.1, 0.8),
            TradeOutcome::loss(0.9, 0.6),
        ];

        let avg = calc.average_reward(&outcomes);
        // Should be somewhere between the two rewards
        let rewards = calc.calculate_batch(&outcomes);
        let expected_avg = (rewards[0].total + rewards[1].total) / 2.0;
        assert!((avg - expected_avg).abs() < 0.001);
    }

    #[test]
    fn test_average_reward_empty() {
        let calc = default_calc();
        let avg = calc.average_reward(&[]);
        assert_eq!(avg, 0.0);
    }
}
