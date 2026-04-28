//! LIGMA Signal Integration Module
//!
//! This module provides LIGMA (Liquidity Genesis Manifold Analyzer) signal
//! collection for the HyperPrediction Oracle.
//!
//! ## Key Features
//!
//! - **Always-On Protection**: LIGMA runs in EVERY scoring cycle, regardless of
//!   pool age or transaction count, to provide continuous protection against
//!   liquidity traps.
//!
//! - **Veto Logic**: LIGMA can veto a candidate if:
//!   1. Liquidity trap risk exceeds threshold
//!   2. Negative psi_ligma (< -0.5)
//!
//! - **Explicit Source Tracking**: Returns `SignalResult` with source tracking
//!   (Explicit when enabled, Unavailable when disabled).

use crate::chaos::amm_math::AmmPool;
use crate::config::ghost_brain_config::LigmaConfig;
use crate::fast_pipeline::EnhancedCandidate;
use crate::oracle::hyper_prediction::signals::{SignalResult, SignalSource};
use crate::signals::{compute_ligma, LigmaResult};
use ghost_core::init_pool_parser::AmmType;
use tracing::{debug, warn};

/// Check LIGMA liquidity safety for a candidate
///
/// This function is the signal collection entry point for LIGMA analysis.
/// It handles:
/// - Configuration-based enabling/disabling
/// - Veto checks (liquidity trap, negative psi_ligma)
/// - Source tracking (Explicit vs. Unavailable)
///
/// ## Veto Conditions
///
/// LIGMA can trigger a VETO (returning Err) if:
/// 1. `liquidity_trap_risk > veto_trap_threshold`
/// 2. `psi_ligma < veto_psi_ligma_threshold` (typically -0.5)
///
/// When a veto occurs, the caller should immediately reject the candidate
/// with score = 0 and risk_level = VeryHigh.
///
/// ## Arguments
///
/// * `candidate` - The enhanced candidate to analyze
/// * `pool` - Optional pool state (explicit or from cache)
/// * `config` - LIGMA configuration with thresholds and veto parameters
///
/// ## Returns
///
/// - `Ok(SignalResult::Explicit)` - LIGMA passed all checks
/// - `Ok(SignalResult::Unavailable)` - LIGMA disabled in config
/// - `Err(LigmaVeto)` - LIGMA triggered veto (trap or negative psi)
pub fn check_ligma_safety(
    candidate: &EnhancedCandidate,
    pool: Option<&AmmPool>,
    config: &LigmaConfig,
) -> Result<SignalResult<LigmaResult>, LigmaVeto> {
    if !config.enabled {
        debug!("LIGMA: Disabled in config, skipping analysis");
        return Ok(SignalResult::unavailable());
    }

    let amm_type = AmmType::PumpFun; // TODO: detect from candidate if needed
    let result = compute_ligma(candidate, pool, amm_type, config);

    // VETO Check #1: Liquidity Trap Risk
    if result.liquidity_trap_risk > config.veto_trap_threshold {
        warn!(
            "LIGMA VETO: Liquidity Trap detected (Risk: {:.2}, psi_ligma: {:.2})",
            result.liquidity_trap_risk, result.psi_ligma
        );
        return Err(LigmaVeto::LiquidityTrap {
            risk: result.liquidity_trap_risk,
            psi_ligma: result.psi_ligma,
            tradability: result.tradability_score,
            worst_loss_bps: result.worst_round_trip_loss_bps,
            source: result.diagnostics.source,
        });
    }

    // VETO Check #2: Strong negative psi_ligma
    if result.psi_ligma < config.veto_psi_ligma_threshold {
        warn!(
            "LIGMA VETO: Strong negative psi_ligma={:.2} (trap={:.2}, tradability={:.2})",
            result.psi_ligma, result.liquidity_trap_risk, result.tradability_score
        );
        return Err(LigmaVeto::NegativePsi {
            psi_ligma: result.psi_ligma,
            trap_risk: result.liquidity_trap_risk,
            sniper_attractiveness: result.sniper_attractiveness,
            tradability: result.tradability_score,
            source: result.diagnostics.source,
        });
    }

    // LIGMA passed all checks - log diagnostics
    debug!(
        "LIGMA: psi={:.2}, trap_risk={:.2}%, sniper_attr={:.2}%, tradability={:.2}%, \
        retail_fraction={:.1}%, min_tradeable={:.4} SOL, worst_loss={:.0} bps, \
        baseline_price={:.8}, convexity={:.3}, confidence={:.2}, source={}, time={}μs",
        result.psi_ligma,
        result.liquidity_trap_risk * 100.0,
        result.sniper_attractiveness * 100.0,
        result.tradability_score * 100.0,
        result.retail_fraction * 100.0,
        result.min_tradeable_sol,
        result.worst_round_trip_loss_bps,
        result.baseline_price,
        result.impact_convexity,
        result.confidence,
        result.diagnostics.source,
        result.diagnostics.analysis_time_us
    );

    // Log high trap risk warning (even if below veto threshold)
    if result.liquidity_trap_risk > config.veto_trap_threshold * 0.8 {
        debug!(
            "LIGMA: ⚠️  HIGH TRAP RISK detected: {:.1}% (threshold={:.1}%)",
            result.liquidity_trap_risk * 100.0,
            config.veto_trap_threshold * 100.0
        );
    }

    Ok(SignalResult::explicit(result, 1.0))
}

/// LIGMA veto reason
///
/// When LIGMA detects a critical liquidity issue, it triggers a veto
/// that should immediately reject the candidate. This enum describes
/// the specific veto reason with relevant metrics.
#[derive(Debug, Clone)]
pub enum LigmaVeto {
    /// Liquidity trap risk exceeds veto threshold
    LiquidityTrap {
        risk: f64,
        psi_ligma: f64,
        tradability: f64,
        worst_loss_bps: f64,
        source: &'static str,
    },

    /// Negative psi_ligma exceeds veto threshold
    NegativePsi {
        psi_ligma: f64,
        trap_risk: f64,
        sniper_attractiveness: f64,
        tradability: f64,
        source: &'static str,
    },
}

impl LigmaVeto {
    /// Generate a human-readable interpretation of the veto
    pub fn interpretation(&self) -> String {
        match self {
            LigmaVeto::LiquidityTrap {
                risk,
                psi_ligma,
                tradability,
                worst_loss_bps,
                source,
            } => format!(
                "🚫 LIGMA VETO: Liquidity Trap Risk={:.2}%, psi={:.2}, tradability={:.2}, worst_loss={:.0}bps | Source: {}",
                risk * 100.0,
                psi_ligma,
                tradability,
                worst_loss_bps,
                source
            ),
            LigmaVeto::NegativePsi {
                psi_ligma,
                trap_risk,
                sniper_attractiveness,
                tradability,
                source,
            } => format!(
                "🚫 LIGMA VETO: Negative psi_ligma={:.2} | trap={:.2}, sniper={:.2}, tradability={:.2} | Source: {}",
                psi_ligma,
                trap_risk,
                sniper_attractiveness,
                tradability,
                source
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ghost_brain_config::LigmaConfig;
    use crate::fast_pipeline::{CacheLinePadding, EnhancedCandidate};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_candidate() -> EnhancedCandidate {
        EnhancedCandidate {
            slot: Some(12345),
            timestamp: 1000000,
            initial_liquidity_sol: 30.0,
            dev_buy_sol: 0.0,
            bonding_curve_progress: Some(0.05),
            vanity_score: 50,
            metadata_len_score: 50,
            has_dev_buy: false,
            mint_auth_disabled: true,
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            expected_price: Some(0.0001),
            shadow_bonding_progress: Some(5),
            virtual_sol_reserves: Some(30_000_000_000),
            shadow_market_cap: None,
            _cache_barrier_2: CacheLinePadding::default(),
            pool_amm_id: Pubkey::new_unique(),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            signature: "test_signature".to_string(),
            token_total_supply: Some(1_000_000_000),
        }
    }

    #[test]
    fn test_ligma_disabled() {
        let candidate = create_test_candidate();
        let config = LigmaConfig {
            enabled: false,
            ..Default::default()
        };

        let result = check_ligma_safety(&candidate, None, &config).unwrap();
        assert_eq!(result.source, SignalSource::Unavailable);
        assert_eq!(result.value, None);
    }

    #[test]
    fn test_ligma_veto_interpretation() {
        let veto = LigmaVeto::LiquidityTrap {
            risk: 0.85,
            psi_ligma: -0.3,
            tradability: 0.2,
            worst_loss_bps: 500.0,
            source: "explicit",
        };

        let interpretation = veto.interpretation();
        assert!(interpretation.contains("LIGMA VETO"));
        assert!(interpretation.contains("Liquidity Trap"));
        assert!(interpretation.contains("85.00%"));
    }

    #[test]
    fn test_ligma_negative_psi_veto_interpretation() {
        let veto = LigmaVeto::NegativePsi {
            psi_ligma: -0.7,
            trap_risk: 0.4,
            sniper_attractiveness: 0.9,
            tradability: 0.3,
            source: "cache",
        };

        let interpretation = veto.interpretation();
        assert!(interpretation.contains("LIGMA VETO"));
        assert!(interpretation.contains("Negative psi_ligma"));
        assert!(interpretation.contains("-0.7"));
    }
}
