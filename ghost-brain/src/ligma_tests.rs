use crate::chaos::amm_math::AmmPool;
use crate::chaos::build_pumpfun_amm_pool;
use crate::config::ghost_brain_config::LigmaConfig;
use crate::fast_pipeline::EnhancedCandidate;
use crate::pumpfun::{
    CurveSnapshot, GENESIS_FEE_BPS, GENESIS_VIRTUAL_SOL_LAMPORTS, GENESIS_VIRTUAL_TOKEN_AMOUNT,
};
use crate::signals::compute_ligma;
use ghost_core::init_pool_parser::AmmType;
use std::time::Instant;

#[test]
fn ligma_pumpfun_genesis_is_fast_and_stable() {
    let candidate = EnhancedCandidate {
        initial_liquidity_sol: 30.0,
        slot: Some(1),
        ..Default::default()
    };
    let snapshot = CurveSnapshot::new(
        GENESIS_VIRTUAL_SOL_LAMPORTS,
        GENESIS_VIRTUAL_TOKEN_AMOUNT,
        GENESIS_FEE_BPS,
        Some(1),
    );
    let pool = build_pumpfun_amm_pool(&snapshot).expect("genesis pool should build");
    let config = LigmaConfig::default();

    let start = Instant::now();
    let result = compute_ligma(&candidate, Some(&pool), AmmType::PumpFun, &config);
    assert!(
        start.elapsed().as_micros() < 1_000,
        "LIGMA should remain sub-millisecond"
    );
    assert!(
        result.tradability_score > 0.3,
        "genesis should be somewhat tradable"
    );
    assert!(
        result.liquidity_trap_risk < 0.6,
        "genesis should not look like a hard trap"
    );

    let second = compute_ligma(&candidate, Some(&pool), AmmType::PumpFun, &config);
    assert_eq!(
        result.curve_fingerprint, second.curve_fingerprint,
        "fingerprint must be deterministic"
    );
}

#[test]
fn ligma_flags_trap_geometry() {
    // Extremely shallow pool with high fee emulates trap / honeypot geometry.
    let pool = AmmPool::new(50_000_000, 200_000, 400).expect("pool should build");
    let candidate = EnhancedCandidate {
        slot: Some(5),
        ..Default::default()
    };
    let config = LigmaConfig::default();

    let result = compute_ligma(&candidate, Some(&pool), AmmType::PumpFun, &config);
    assert!(
        result.liquidity_trap_risk > 0.6,
        "trap risk should be elevated"
    );
    assert!(
        result.tradability_score < 0.5,
        "trap should not be retail friendly"
    );
}

#[test]
fn ligma_rewards_retail_pool() {
    // Deep reserves, low fee → high tradability and positive psi.
    let pool =
        AmmPool::new(400_000_000_000u128, 4_000_000_000_000u128, 30).expect("pool should build");
    let candidate = EnhancedCandidate {
        initial_liquidity_sol: 400.0,
        ..Default::default()
    };
    let config = LigmaConfig::default();

    let result = compute_ligma(&candidate, Some(&pool), AmmType::PumpFun, &config);
    assert!(
        result.tradability_score > 0.7,
        "healthy pool should be retail friendly"
    );
    assert!(
        result.liquidity_trap_risk < 0.3,
        "healthy pool should not be trap-like"
    );
    assert!(
        result.psi_ligma > 0.0,
        "psi should tilt bullish for healthy pools"
    );
}

#[test]
fn ligma_handles_missing_pool_state() {
    let candidate = EnhancedCandidate {
        slot: Some(42),
        ..Default::default()
    };
    let config = LigmaConfig::default();

    let result = compute_ligma(&candidate, None, AmmType::PumpFun, &config);
    assert!(result.confidence < 1.0, "fallback should reduce confidence");
    assert_eq!(result.diagnostics.source, "genesis");
    assert!(result.min_tradeable_sol >= 0.0);
}
