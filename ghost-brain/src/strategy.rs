//! Strategy selection and SwapPlan generation
//!
//! This module implements the Features layer that selects trading strategies
//! and generates SwapPlans based on Oracle scores and user configuration.
//!
//! P.A.N.I.C. × CIR × BVA Decision Map (DecisionEngine roadmap):
//! - Dead Launch: LDR < 0.2, CIR low, BVA low → PASS
//! - Fake Pump: LDR < 0.5, CIR high, BVA high → SHORT/PASS
//! - The Spring: LDR > 2.0, CIR low, BVA neutral → APE IN
//! - God Candle: LDR > 1.5, CIR high, BVA high → HOLD
//! - Rug Pull: LDR high, CIR falling, BVA falling → EXIT

use anyhow::{Context, Result};
// Używamy aliasu SwapRiskLevel, aby uniknąć konfliktu z RiskLevel z wyroczni
use ghost_core::swap_plan::{RiskLevel as SwapRiskLevel, SwapPlan, SwapPlanBuilder};
use solana_sdk::pubkey::Pubkey;
use std::time::{SystemTime, UNIX_EPOCH};

// Importujemy typy z poprawnego modułu oracle i tworzymy alias dla RiskLevel
use crate::oracle::{RiskLevel as OracleRiskLevel, ScoredCandidate};

/// Strategy selector for generating SwapPlans
pub struct StrategySelector {
    /// Default authority (user wallet)
    authority: Pubkey,

    /// Maximum position size in lamports
    max_position_size: u64,

    /// Maximum slippage tolerance (0.0 - 1.0)
    max_slippage: f64,

    /// Intent timeout in seconds
    intent_timeout_secs: u64,
}

impl StrategySelector {
    /// Create a new strategy selector
    pub fn new(
        authority: Pubkey,
        max_position_size: u64,
        max_slippage: f64,
        intent_timeout_secs: u64,
    ) -> Self {
        Self {
            authority,
            max_position_size,
            max_slippage,
            intent_timeout_secs,
        }
    }

    /// Generate a SwapPlan from a scored candidate
    ///
    /// Returns None if the candidate should not be traded.
    pub fn generate_swap_plan(&self, scored: &ScoredCandidate) -> Result<Option<SwapPlan>> {
        self.generate_swap_plan_with_overrides(scored, None, None)
    }

    /// Generate a SwapPlan with optional runtime overrides
    ///
    /// Allows runtime configuration to override position size and slippage.
    /// Returns None if the candidate should not be traded.
    pub fn generate_swap_plan_with_overrides(
        &self,
        scored: &ScoredCandidate,
        runtime_position_size: Option<u64>,
        runtime_max_slippage: Option<f64>,
    ) -> Result<Option<SwapPlan>> {
        // Don't trade if score is too low
        if !scored.passed {
            return Ok(None);
        }

        // Use runtime override if available, otherwise use configured value
        let base_position_size = runtime_position_size.unwrap_or(self.max_position_size);
        let max_slippage = runtime_max_slippage.unwrap_or(self.max_slippage);

        // Calculate position size based on score and risk level
        let amount_in = self.calculate_position_size_with_base(
            scored.score,
            scored.risk_level,
            base_position_size,
        );

        // Calculate minimum output with slippage protection
        let min_amount_out =
            self.calculate_min_amount_out_with_slippage(amount_in, max_slippage)?;

        // Calculate timeout
        let timeout = self.calculate_timeout();

        // Build the SwapPlan
        let plan = SwapPlanBuilder::new(self.authority, scored.pool.pool_amm_id)
            .amount_in(amount_in)
            .min_amount_out(min_amount_out)
            .timeout(timeout)
            .with_score(scored.score)
            .with_strategy("snipe_new_pool")
            .with_risk_level(self.convert_risk_level(scored.risk_level))
            .with_token_mint(scored.pool.base_mint)
            .build()
            .context("Failed to build SwapPlan")?;

        Ok(Some(plan))
    }

    /// Calculate position size based on score and risk level
    fn calculate_position_size(&self, score: u8, risk_level: OracleRiskLevel) -> u64 {
        self.calculate_position_size_with_base(score, risk_level, self.max_position_size)
    }

    /// Calculate position size with custom base size
    fn calculate_position_size_with_base(
        &self,
        score: u8,
        risk_level: OracleRiskLevel,
        base_size: u64,
    ) -> u64 {
        // Base position size
        let mut size = base_size;

        // Scale down based on risk level
        size = match risk_level {
            OracleRiskLevel::Low => size,                   // 100%
            OracleRiskLevel::Medium => (size * 75) / 100,   // 75%
            OracleRiskLevel::High => (size * 50) / 100,     // 50%
            OracleRiskLevel::VeryHigh => (size * 25) / 100, // 25%
        };

        // Further scale based on confidence (score)
        if score < 80 {
            size = (size * score as u64) / 100;
        }

        // Ensure minimum swap amount
        size.max(1000) // Minimum 1000 lamports
    }

    /// Calculate minimum output amount with slippage protection
    fn calculate_min_amount_out(&self, amount_in: u64) -> Result<u64> {
        self.calculate_min_amount_out_with_slippage(amount_in, self.max_slippage)
    }

    /// Calculate minimum output amount with custom slippage
    fn calculate_min_amount_out_with_slippage(
        &self,
        amount_in: u64,
        max_slippage: f64,
    ) -> Result<u64> {
        // This is a simplified calculation
        // In production, this would fetch current pool price and calculate expected output

        // For now, assume a simple 1:1000 ratio (1 SOL = 1000 tokens)
        let expected_output = amount_in * 1000;

        // Apply slippage tolerance
        let min_output = (expected_output as f64 * (1.0 - max_slippage)) as u64;

        // Ensure at least 1 token
        Ok(min_output.max(1))
    }

    /// Calculate timeout timestamp
    fn calculate_timeout(&self) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        now + self.intent_timeout_secs as i64
    }

    /// Convert oracle risk level to ghost-core risk level
    fn convert_risk_level(&self, risk: OracleRiskLevel) -> SwapRiskLevel {
        match risk {
            OracleRiskLevel::Low => SwapRiskLevel::Low,
            OracleRiskLevel::Medium => SwapRiskLevel::Medium,
            OracleRiskLevel::High => SwapRiskLevel::High,
            OracleRiskLevel::VeryHigh => SwapRiskLevel::VeryHigh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // W testach również używamy poprawnej ścieżki i aliasu
    use crate::oracle::{RiskLevel as OracleRiskLevel, ScoredCandidate};
    use seer::types::CandidatePool;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_scored_candidate() -> ScoredCandidate {
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
            score: 85,
            passed: true,
            risk_level: OracleRiskLevel::Medium,
            confidence: Some(0.85),
        }
    }

    #[test]
    fn test_swap_plan_generation() {
        let selector = StrategySelector::new(
            Pubkey::new_unique(),
            10_000_000, // 0.01 SOL
            0.05,       // 5% slippage
            3600,       // 1 hour timeout
        );

        let scored = create_test_scored_candidate();
        let plan = selector.generate_swap_plan(&scored).unwrap();

        assert!(plan.is_some());
        let plan = plan.unwrap();

        assert_eq!(plan.authority, selector.authority);
        assert!(plan.amount_in > 0);
        assert!(plan.min_amount_out > 0);
        assert!(plan.timeout > 0);
    }

    #[test]
    fn test_position_sizing() {
        let selector = StrategySelector::new(Pubkey::new_unique(), 10_000_000, 0.05, 3600);

        // High score, low risk
        let size1 = selector.calculate_position_size(95, OracleRiskLevel::Low);
        assert_eq!(size1, 10_000_000);

        // Medium score, medium risk
        let size2 = selector.calculate_position_size(75, OracleRiskLevel::Medium);
        assert!(size2 < size1);

        // Low score, high risk
        let size3 = selector.calculate_position_size(60, OracleRiskLevel::High);
        assert!(size3 < size2);
    }

    #[test]
    fn test_reject_low_score() {
        let selector = StrategySelector::new(Pubkey::new_unique(), 10_000_000, 0.05, 3600);

        let mut scored = create_test_scored_candidate();
        scored.passed = false;

        let plan = selector.generate_swap_plan(&scored).unwrap();
        assert!(plan.is_none());
    }
}
