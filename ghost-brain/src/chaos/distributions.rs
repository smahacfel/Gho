//! Buyer Profile Probability Distributions
//!
//! This module defines probability distributions for simulating different market participant
//! behaviors during Monte Carlo simulations. It models realistic trading patterns including
//! bullish accumulation, bearish exit, and rug-pull scenarios.
//!
//! ## Market Participant Profiles
//!
//! 1. **Bullish Whale** - Large buyers during uptrends
//! 2. **Bearish Whale** - Large sellers during downtrends
//! 3. **Rug Puller** - Malicious actors performing exit scams
//! 4. **Normal Trader** - Organic, smaller market participants
//!
//! ## Usage in Monte Carlo Simulations
//!
//! The distributions are used by the Chaos Engine to sample realistic trading actions
//! across thousands of parallel simulations to estimate risk probabilities.

use rand::distributions::{Distribution, WeightedIndex};
use rand_xoshiro::Xoshiro256PlusPlus;

/// Types of market actions that can be sampled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketAction {
    /// Large buy (whale accumulation)
    BuyLarge,
    /// Medium buy (retail FOMO)
    BuyMedium,
    /// Small buy (normal activity)
    BuySmall,
    /// Small sell (taking profits)
    SellSmall,
    /// Medium sell (exit position)
    SellMedium,
    /// Large sell (whale exit or rug pull)
    SellLarge,
    /// No action (hold)
    Hold,
}

/// Represents a market participant profile with action probabilities
#[derive(Debug, Clone)]
pub struct BuyerProfile {
    /// Name of the profile for debugging
    pub name: &'static str,
    /// Weighted distribution of actions
    weights: Vec<u32>,
    /// Corresponding actions for each weight
    actions: Vec<MarketAction>,
}

impl BuyerProfile {
    /// Creates a new buyer profile from action weights
    ///
    /// # Arguments
    /// * `name` - Profile identifier
    /// * `action_weights` - Tuples of (action, weight) pairs
    ///
    /// # Returns
    /// A new BuyerProfile configured with the given distribution
    pub fn new(name: &'static str, action_weights: &[(MarketAction, u32)]) -> Self {
        let (actions, weights): (Vec<_>, Vec<_>) = action_weights.iter().cloned().unzip();

        Self {
            name,
            weights,
            actions,
        }
    }

    /// Samples a random action from this profile's distribution
    ///
    /// # Arguments
    /// * `rng` - Random number generator (Xoshiro256PlusPlus for performance)
    ///
    /// # Returns
    /// A randomly sampled MarketAction according to the profile's weights
    pub fn sample_action(&self, rng: &mut Xoshiro256PlusPlus) -> MarketAction {
        let dist = WeightedIndex::new(&self.weights).expect("BuyerProfile weights must be valid");
        self.actions[dist.sample(rng)]
    }

    /// Predefined profile: Bullish Whale
    ///
    /// Characteristics:
    /// - 40% chance of large buys (accumulation)
    /// - 30% chance of medium buys (FOMO)
    /// - 20% chance of small buys (testing waters)
    /// - 5% chance of small sells (taking small profits)
    /// - 5% chance of hold
    pub fn bullish_whale() -> Self {
        Self::new(
            "BullishWhale",
            &[
                (MarketAction::BuyLarge, 40),
                (MarketAction::BuyMedium, 30),
                (MarketAction::BuySmall, 20),
                (MarketAction::SellSmall, 5),
                (MarketAction::Hold, 5),
            ],
        )
    }

    /// Predefined profile: Bearish Whale
    ///
    /// Characteristics:
    /// - 40% chance of large sells (exit)
    /// - 30% chance of medium sells (reducing position)
    /// - 20% chance of small sells (slow exit)
    /// - 5% chance of small buys (attempting to support price)
    /// - 5% chance of hold
    pub fn bearish_whale() -> Self {
        Self::new(
            "BearishWhale",
            &[
                (MarketAction::SellLarge, 40),
                (MarketAction::SellMedium, 30),
                (MarketAction::SellSmall, 20),
                (MarketAction::BuySmall, 5),
                (MarketAction::Hold, 5),
            ],
        )
    }

    /// Predefined profile: Rug Puller
    ///
    /// Characteristics:
    /// - 80% chance of large sells (dumping on holders)
    /// - 15% chance of medium sells (gradual dump)
    /// - 5% chance of small buys (fake buy walls to trap victims)
    pub fn rug_puller() -> Self {
        Self::new(
            "RugPuller",
            &[
                (MarketAction::SellLarge, 80),
                (MarketAction::SellMedium, 15),
                (MarketAction::BuySmall, 5),
            ],
        )
    }

    /// Predefined profile: Normal Trader (Organic Market Activity)
    ///
    /// Characteristics:
    /// - 50% chance of hold (most retail just holds)
    /// - 15% chance of small buys
    /// - 10% chance of medium buys
    /// - 5% chance of large buys (rare)
    /// - 15% chance of small sells
    /// - 5% chance of medium sells
    pub fn normal_trader() -> Self {
        Self::new(
            "NormalTrader",
            &[
                (MarketAction::Hold, 50),
                (MarketAction::BuySmall, 15),
                (MarketAction::BuyMedium, 10),
                (MarketAction::BuyLarge, 5),
                (MarketAction::SellSmall, 15),
                (MarketAction::SellMedium, 5),
            ],
        )
    }

    /// Predefined profile: Mixed Market (Balanced)
    ///
    /// Represents a healthy market with diverse participants.
    /// Equal probability of all action types except large sells (reduced to prevent panic).
    pub fn mixed_market() -> Self {
        Self::new(
            "MixedMarket",
            &[
                (MarketAction::BuyLarge, 10),
                (MarketAction::BuyMedium, 15),
                (MarketAction::BuySmall, 20),
                (MarketAction::SellSmall, 20),
                (MarketAction::SellMedium, 15),
                (MarketAction::SellLarge, 5),
                (MarketAction::Hold, 15),
            ],
        )
    }
}

/// Converts a MarketAction to a swap amount multiplier
///
/// These multipliers are applied to a base amount (e.g., 1% of pool reserves)
/// to generate realistic trade sizes for Monte Carlo simulations.
///
/// # Arguments
/// * `action` - The market action to convert
///
/// # Returns
/// A multiplier value (0.0 to 10.0) representing trade size
pub fn action_to_amount_multiplier(action: MarketAction) -> f64 {
    match action {
        MarketAction::BuyLarge => 5.0,   // 5x base amount (whale buy)
        MarketAction::BuyMedium => 2.0,  // 2x base amount
        MarketAction::BuySmall => 0.5,   // 0.5x base amount
        MarketAction::SellSmall => 0.5,  // 0.5x base amount
        MarketAction::SellMedium => 2.0, // 2x base amount
        MarketAction::SellLarge => 5.0,  // 5x base amount (whale sell or rug)
        MarketAction::Hold => 0.0,       // No trade
    }
}

/// Determines if a MarketAction is a buy
pub fn is_buy(action: MarketAction) -> bool {
    matches!(
        action,
        MarketAction::BuyLarge | MarketAction::BuyMedium | MarketAction::BuySmall
    )
}

/// Determines if a MarketAction is a sell
pub fn is_sell(action: MarketAction) -> bool {
    matches!(
        action,
        MarketAction::SellLarge | MarketAction::SellMedium | MarketAction::SellSmall
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn test_buyer_profile_creation() {
        let profile = BuyerProfile::new(
            "TestProfile",
            &[(MarketAction::BuyLarge, 50), (MarketAction::SellSmall, 50)],
        );

        assert_eq!(profile.name, "TestProfile");
        assert_eq!(profile.actions.len(), 2);
        assert_eq!(profile.weights.len(), 2);
    }

    #[test]
    fn test_sample_action_reproducible() {
        let profile = BuyerProfile::bullish_whale();
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(42);

        // Sample multiple times with same seed should give same sequence
        let action1 = profile.sample_action(&mut rng);

        // Reset RNG
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(42);
        let action2 = profile.sample_action(&mut rng);

        assert_eq!(action1, action2);
    }

    #[test]
    fn test_bullish_whale_profile() {
        let profile = BuyerProfile::bullish_whale();
        assert_eq!(profile.name, "BullishWhale");

        // Sample 1000 times and verify bias toward buys
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        let mut buy_count = 0;
        let mut sell_count = 0;

        for _ in 0..1000 {
            let action = profile.sample_action(&mut rng);
            if is_buy(action) {
                buy_count += 1;
            } else if is_sell(action) {
                sell_count += 1;
            }
        }

        // Bullish whale should buy more than sell
        assert!(buy_count > sell_count);
        // Should be heavily biased (>80% buy actions)
        assert!(buy_count > 800);
    }

    #[test]
    fn test_bearish_whale_profile() {
        let profile = BuyerProfile::bearish_whale();
        assert_eq!(profile.name, "BearishWhale");

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        let mut buy_count = 0;
        let mut sell_count = 0;

        for _ in 0..1000 {
            let action = profile.sample_action(&mut rng);
            if is_buy(action) {
                buy_count += 1;
            } else if is_sell(action) {
                sell_count += 1;
            }
        }

        // Bearish whale should sell more than buy
        assert!(sell_count > buy_count);
        // Should be heavily biased (>80% sell actions)
        assert!(sell_count > 800);
    }

    #[test]
    fn test_rug_puller_profile() {
        let profile = BuyerProfile::rug_puller();
        assert_eq!(profile.name, "RugPuller");

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        let mut large_sell_count = 0;

        for _ in 0..1000 {
            let action = profile.sample_action(&mut rng);
            if action == MarketAction::SellLarge {
                large_sell_count += 1;
            }
        }

        // Rug puller should do mostly large sells (80%)
        assert!(large_sell_count > 700);
    }

    #[test]
    fn test_normal_trader_profile() {
        let profile = BuyerProfile::normal_trader();
        assert_eq!(profile.name, "NormalTrader");

        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        let mut hold_count = 0;

        for _ in 0..1000 {
            let action = profile.sample_action(&mut rng);
            if action == MarketAction::Hold {
                hold_count += 1;
            }
        }

        // Normal trader should hold frequently (~50%)
        assert!(hold_count > 400);
        assert!(hold_count < 600);
    }

    #[test]
    fn test_action_to_amount_multiplier() {
        assert_eq!(action_to_amount_multiplier(MarketAction::BuyLarge), 5.0);
        assert_eq!(action_to_amount_multiplier(MarketAction::BuyMedium), 2.0);
        assert_eq!(action_to_amount_multiplier(MarketAction::BuySmall), 0.5);
        assert_eq!(action_to_amount_multiplier(MarketAction::SellSmall), 0.5);
        assert_eq!(action_to_amount_multiplier(MarketAction::SellMedium), 2.0);
        assert_eq!(action_to_amount_multiplier(MarketAction::SellLarge), 5.0);
        assert_eq!(action_to_amount_multiplier(MarketAction::Hold), 0.0);
    }

    #[test]
    fn test_is_buy() {
        assert!(is_buy(MarketAction::BuyLarge));
        assert!(is_buy(MarketAction::BuyMedium));
        assert!(is_buy(MarketAction::BuySmall));
        assert!(!is_buy(MarketAction::SellSmall));
        assert!(!is_buy(MarketAction::Hold));
    }

    #[test]
    fn test_is_sell() {
        assert!(is_sell(MarketAction::SellLarge));
        assert!(is_sell(MarketAction::SellMedium));
        assert!(is_sell(MarketAction::SellSmall));
        assert!(!is_sell(MarketAction::BuySmall));
        assert!(!is_sell(MarketAction::Hold));
    }

    #[test]
    fn test_mixed_market_profile() {
        let profile = BuyerProfile::mixed_market();
        assert_eq!(profile.name, "MixedMarket");

        // Just verify it samples without panicking
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        for _ in 0..100 {
            let _ = profile.sample_action(&mut rng);
        }
    }
}
