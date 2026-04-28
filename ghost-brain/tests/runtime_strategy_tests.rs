//! Integration tests for runtime configuration in strategy selection
//!
//! These tests verify that the StrategySelector correctly uses runtime
//! configuration overrides when generating swap plans.

#[cfg(test)]
mod tests {
    use ghost_brain::oracle::{RiskLevel, ScoredCandidate};
    use ghost_brain::strategy::StrategySelector;
    use seer::types::CandidatePool;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_candidate(score: u8, passed: bool) -> ScoredCandidate {
        let pool = CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(12345),
            event_ts_ms: Some(1_234_567_890_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "5".repeat(88),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(0.05),
            initial_liquidity_sol: Some(10.0),
            token_total_supply: Some(1_000_000_000),
            block_time: Some(1234567890),
        };

        ScoredCandidate {
            pool,
            score,
            passed,
            risk_level: RiskLevel::Medium,
            confidence: None,
        }
    }

    #[test]
    fn test_strategy_selector_without_runtime_override() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            100_000_000, // 0.1 SOL default
            0.01,        // 1% slippage
            3600,        // 1 hour timeout
        );

        let candidate = create_test_candidate(85, true);
        let plan = selector.generate_swap_plan(&candidate).unwrap().unwrap();

        // With medium risk and score 85, position should be scaled
        // Medium risk = 75% of base = 75_000_000
        assert_eq!(plan.amount_in, 75_000_000);
    }

    #[test]
    fn test_strategy_selector_with_runtime_position_override() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            100_000_000, // 0.1 SOL default (will be overridden)
            0.01,        // 1% slippage
            3600,
        );

        let candidate = create_test_candidate(85, true);

        // Override with 0.5 SOL position size
        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(500_000_000), None)
            .unwrap()
            .unwrap();

        // With medium risk and score 85, position should be scaled from 500_000_000
        // Medium risk = 75% of 500_000_000 = 375_000_000
        assert_eq!(plan.amount_in, 375_000_000);
    }

    #[test]
    fn test_strategy_selector_with_runtime_slippage_override() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            100_000_000,
            0.01, // 1% slippage (will be overridden)
            3600,
        );

        let candidate = create_test_candidate(85, true);

        // Override with 5% slippage
        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, None, Some(0.05))
            .unwrap()
            .unwrap();

        // Position: 75_000_000 (75% of 100M for medium risk)
        // Expected output: 75_000_000 * 1000 = 75_000_000_000
        // With 5% slippage: 95% of expected = 71_250_000_000
        assert_eq!(plan.amount_in, 75_000_000);
        assert_eq!(plan.min_amount_out, 71_250_000_000);
    }

    #[test]
    fn test_strategy_selector_with_both_overrides() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            100_000_000, // Will be overridden
            0.01,        // Will be overridden
            3600,
        );

        let candidate = create_test_candidate(90, true);

        // Override both position size and slippage
        let plan = selector
            .generate_swap_plan_with_overrides(
                &candidate,
                Some(1_000_000_000), // 1 SOL
                Some(0.10),          // 10% slippage
            )
            .unwrap()
            .unwrap();

        // Position: 750_000_000 (75% of 1B for medium risk)
        // Expected output: 750_000_000 * 1000 = 750_000_000_000
        // With 10% slippage: 90% of expected = 675_000_000_000
        assert_eq!(plan.amount_in, 750_000_000);
        assert_eq!(plan.min_amount_out, 675_000_000_000);
    }

    #[test]
    fn test_strategy_selector_low_risk_full_position() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            200_000_000, // 0.2 SOL
            0.02,
            3600,
        );

        let mut candidate = create_test_candidate(90, true);
        candidate.risk_level = RiskLevel::Low; // Low risk = 100% of position

        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(400_000_000), None)
            .unwrap()
            .unwrap();

        // Low risk = 100% of 400M = 400M
        assert_eq!(plan.amount_in, 400_000_000);
    }

    #[test]
    fn test_strategy_selector_high_risk_reduced_position() {
        let selector = StrategySelector::new(Pubkey::new_unique(), 200_000_000, 0.02, 3600);

        let mut candidate = create_test_candidate(90, true);
        candidate.risk_level = RiskLevel::High; // High risk = 50% of position

        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(400_000_000), None)
            .unwrap()
            .unwrap();

        // High risk = 50% of 400M = 200M
        assert_eq!(plan.amount_in, 200_000_000);
    }

    #[test]
    fn test_strategy_selector_very_high_risk_minimal_position() {
        let selector = StrategySelector::new(Pubkey::new_unique(), 200_000_000, 0.02, 3600);

        let mut candidate = create_test_candidate(90, true);
        candidate.risk_level = RiskLevel::VeryHigh; // Very high risk = 25% of position

        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(400_000_000), None)
            .unwrap()
            .unwrap();

        // Very high risk = 25% of 400M = 100M
        assert_eq!(plan.amount_in, 100_000_000);
    }

    #[test]
    fn test_strategy_selector_low_score_scaling() {
        let selector = StrategySelector::new(Pubkey::new_unique(), 100_000_000, 0.02, 3600);

        let candidate = create_test_candidate(70, true); // Low score

        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(200_000_000), None)
            .unwrap()
            .unwrap();

        // Medium risk = 75% of 200M = 150M
        // Score 70 < 80, so further scale: 150M * 70% = 105M
        assert_eq!(plan.amount_in, 105_000_000);
    }

    #[test]
    fn test_strategy_selector_minimum_position_enforcement() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            10_000, // Very small base position
            0.02,
            3600,
        );

        let mut candidate = create_test_candidate(60, true); // Low score
        candidate.risk_level = RiskLevel::VeryHigh; // Very high risk

        let plan = selector
            .generate_swap_plan_with_overrides(&candidate, Some(10_000), None)
            .unwrap()
            .unwrap();

        // Very high risk = 25% of 10K = 2.5K = 2500
        // Score 60 < 80, so scale: 2500 * 60% = 1500
        // This is above minimum 1000 lamports
        assert_eq!(plan.amount_in, 1_500);
    }
}
