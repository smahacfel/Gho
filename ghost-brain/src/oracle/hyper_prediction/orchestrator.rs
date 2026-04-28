//! Orchestrator Module - Core Scoring Logic for HyperPrediction Oracle
//!
//! This module contains the main scoring implementation extracted from `mod.rs`.
//! It orchestrates all analysis modules and combines their results into a final score.
//!
//! ## Architecture
//!
//! The orchestrator follows the "Patient Observer" strategy:
//!
//! 1. **Phase Detection**: Determines EarlyStage vs FullAnalysis based on tx_count
//! 2. **Veto Gates**: Cluster Hunter, LIGMA, FRE checks that can reject early
//! 3. **Signal Collection**: Runs all analysis modules (SSMI, MPCF, SCR, ULVF, etc.)
//! 4. **Score Combination**: Uses SurvivorScore + QASS + modifiers
//! 5. **Interpretation**: Generates human-readable result explanation
//!
//! ## Early Stage Detection Strategy
//!
//! The orchestrator uses a two-phase gating system:
//!
//! **Phase 1: Gatekeeper (oracle_runtime.rs)**
//! - Hard filter at T=1780ms
//! - Threshold: `min_tx_count_for_scoring` from config.toml (default: 15 TX)
//! - Action: REJECT pools with insufficient activity
//!
//! **Phase 2: Adaptive Analysis (orchestrator.rs)**
//! - Threshold: Gatekeeper × 1.5 (default: 22 TX)
//! - Early Stage (15-22 TX): Static analysis only (LIGMA, IWIM, Chaos, MESA)
//! - Full Analysis (23+ TX): All metrics including trend-based (SCR, ULVF, POVC)
//!
//! This prevents trend-based metrics from producing false negatives when
//! transaction history is insufficient for statistical significance.

use crate::analyzers::mesa::MesaResult;
use crate::chaos::{amm_math::AmmPool, build_pumpfun_amm_pool};
use crate::config::GhostBrainConfig;
use crate::fast_pipeline::EnhancedCandidate;
use crate::models::mci_result::MciResult;
use crate::models::qedd_result::QeddResult;
use crate::oracle::bva::BvaOutput;
use crate::oracle::{
    cluster_hunter::ClusterAnalysis,
    qman::{
        SignalDetector, SignalResult as QmanSignalResult, TradingSignal, TransitionMatrix,
        UnitaryEvolution,
    },
    scr_extended::SCRExtended,
    second_wave_detector::{SecondWaveAction, SecondWaveDetector, SecondWaveResult},
    survivor_score::{SurvivorScoreCalculator, SurvivorScoreInput, SurvivorScoreResult},
    tcf::{MarketObservation, TcfDiagnostics, TcfPhase, TrendCohesionField},
    tx_metrics::TransactionMetrics,
    ultrafast::{
        build_cluster_wave, build_iwim_wave, build_ligma_wave, build_povc_wave, build_praecog_wave,
        build_profiler_wave, build_scr_wave, build_ssmi_wave, build_ulvf_wave, build_vision_wave,
        fre::{FractalAction, FractalEngine, FractalVerdict},
        mpcf_infer, praecog_analyze, ActorInference, ActorType, DataSource, EctoSignal,
        HeuristicWave, PanicOutput, PraecogInput, PraecogParams, QASSResult,
        QuantumAmplitudeScorer, SourceType, SsmiResult, SubSlotMicroentropy, TcrScore,
    },
    ulvf_extended::ULVFExtended,
    wallet_energy_tracker::WalletEnergyTracker,
    HyperOracle, MarketSnapshot, ScoredCandidate,
};
use crate::pumpfun::PumpCurveStateCache;
use crate::signals::{compute_ligma, LigmaResult, MarketSignals};
use crate::tuning::TunableWeights;
use seer::paradox_sensor::ParadoxState;

use super::{
    scoring,
    signals::builders::{build_dev_buy_wave_scaled, build_liquidity_wave_scaled},
    state::{AnalysisPhase, HyperPredictionResult, QmanResult, TcfResult},
    utils::{calculate_recommended_delay, convert_enhanced_to_candidate_pool, is_pool_genesis},
    verdict::{FinalVerdict, OracleDecision, RiskLevel, RiskThresholds},
    HyperPredictionConfig, HyperPredictionOracle,
};

use anyhow::Result;
use std::env;
use std::f64::consts::PI;
use std::fs;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, instrument, warn};

struct SurvivorCalculatorCache {
    mtime: Option<SystemTime>,
    calculator: SurvivorScoreCalculator,
}

static SURVIVOR_CALCULATOR_CACHE: OnceLock<Mutex<SurvivorCalculatorCache>> = OnceLock::new();

fn load_survivor_calculator(oracle: &HyperPredictionOracle) -> SurvivorScoreCalculator {
    let config_path = env::var("GHOST_BRAIN_CONFIG_PATH")
        .ok()
        .unwrap_or_else(|| "ghost-brain/ghost_brain_config.toml".to_string());

    let cache = SURVIVOR_CALCULATOR_CACHE.get_or_init(|| {
        Mutex::new(SurvivorCalculatorCache {
            mtime: None,
            calculator: oracle.survivor_calculator.clone(),
        })
    });

    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let current_mtime = fs::metadata(&config_path)
        .and_then(|metadata| metadata.modified())
        .ok();

    let should_reload = current_mtime
        .map(|mtime| guard.mtime.map(|cached| mtime > cached).unwrap_or(true))
        .unwrap_or(false);

    if should_reload {
        match GhostBrainConfig::from_toml_file(&config_path) {
            Ok(config) => {
                let calculator = SurvivorScoreCalculator::from_ghost_brain_config(&config)
                    .with_thresholds(oracle.hyper_prediction_config.survivor_thresholds.clone())
                    .with_risk_multipliers(oracle.hyper_prediction_config.risk_multipliers.clone());
                guard.calculator = calculator;
                guard.mtime = current_mtime;
            }
            Err(err) => {
                warn!(
                    "⚠️ [SURVIVOR] Config reload failed, keeping last good config: {} ({})",
                    config_path, err
                );
            }
        }
    }

    guard.calculator.clone()
}

// =============================================================================
// Constants (from mod.rs)
// =============================================================================

/// Pump.fun virtual SOL reserves in lamports (30 SOL)
const PUMP_VIRTUAL_SOL_LAMPORTS: f64 = 30_000_000_000.0;
const PUMP_VIRTUAL_TOKEN_RESERVES: f64 = 1_073_000_000_000_000.0;
const PUMP_INITIAL_PRICE: f64 = PUMP_VIRTUAL_SOL_LAMPORTS / PUMP_VIRTUAL_TOKEN_RESERVES;

// NOTE: The following constants have been moved to HyperPredictionConfig
// and are now configurable via ghost_brain_config.toml:
// - CABAL_RISK_THRESHOLD → orchestrator_thresholds.cabal_risk_threshold
// - MESA_INTERPRETATION_WASH_THRESHOLD → survivor_thresholds.mesa_wash_elevated
// - MESA_INTERPRETATION_BOT_THRESHOLD → orchestrator_thresholds.mesa_interpretation_bot_threshold
// - MESA_INTERPRETATION_ORGANIC_THRESHOLD → orchestrator_thresholds.mesa_interpretation_organic_threshold

/// Default fallback POVC cluster: 2 = Bot Noise
const DEFAULT_POVC_CLUSTER: usize = 2;

/// Default fallback values
const DEFAULT_SCR_SCORE: f32 = 0.0;
const DEFAULT_ULVF_DIVERGENCE: f32 = 0.0;
const DEFAULT_ULVF_CURL: f32 = 0.0;

// NOTE: MESA interpretation thresholds moved to config (see note above)

/// Default confidence when SurvivorScore is unavailable
const SURVIVOR_FALLBACK_CONFIDENCE: f32 = 0.5;

// =============================================================================
// Main Scoring Implementation
// =============================================================================

pub(super) fn score_candidate_impl(
    oracle: &HyperPredictionOracle,

    candidate: &EnhancedCandidate,
    pumpfun_cache: &PumpCurveStateCache,
    explicit_pool_state: Option<&AmmPool>,
    tx_timestamps: Option<&[u64]>,
    tx_data: Option<&[u8]>,
    iwim_result: Option<crate::oracle::ultrafast::IwimResult>,
    chaos_result: Option<crate::chaos::engine::ChaosResult>,
    resonance_result: Option<crate::signals::resonance::ResonanceResult>,
    gene_safety_result: Option<crate::security::gene_mapper::GeneAnalysisResult>,
    hunter_score: Option<u8>,
    tx_metrics: Option<&TransactionMetrics>,
    cluster_result: Option<ClusterAnalysis>,
    paradox_state: Option<ParadoxState>,
    tuned_weights: Option<TunableWeights>,
    // New optional parameter for pre-computed LIGMA result
    ligma_result_override: Option<LigmaResult>,
    // Behavioral signals (optional)
    ecto_signal: Option<EctoSignal>,
    bva_output: Option<BvaOutput>,
    panic_output: Option<PanicOutput>,
    tcr_score: Option<TcrScore>,
    cir_strength: Option<f32>,
) -> Result<HyperPredictionResult> {
    let start = Instant::now();

    let survivor_calculator = load_survivor_calculator(oracle);

    let snapshot_price = explicit_pool_state.map(|pool| pool.price_b_in_a());

    // Initialize fallback tracker for this prediction cycle
    let mut fallback_tracker = crate::config::FallbackTracker::new();

    // =================================================================
    // PHASE DETECTION: Early Stage vs Full Analysis
    // =================================================================
    // Strategy: Pools passing Gatekeeper (≥15 TX) enter orchestrator.
    // Early Stage mode activates for pools still building momentum (15-22 TX).
    // This prevents false negatives from trend-based metrics requiring deeper history.
    //
    // Threshold Calculation:
    // - Gatekeeper minimum: from config (gatekeeper_min_tx_count)
    // - Early Stage threshold: gatekeeper_min_tx_count * early_stage_multiplier
    // - Rationale: Trend metrics (SCR FFT, ULVF divergence) need ~20+ samples
    //              for statistical significance in 400ms cycles.

    let gatekeeper_threshold = oracle.hyper_prediction_config.gatekeeper_min_tx_count;
    let early_stage_multiplier = oracle.hyper_prediction_config.early_stage_multiplier;
    let early_stage_threshold = (gatekeeper_threshold as f32 * early_stage_multiplier) as usize;

    let is_early_stage = tx_metrics
        .map(|m| m.tx_count < early_stage_threshold)
        .unwrap_or(true); // Default to early stage if no metrics available

    // Cold start detection for QEDD/MCI: treat any window <10 tx as sparse
    let cold_start_active = tx_metrics.map(|m| m.tx_count < 10).unwrap_or(true);

    if is_early_stage {
        debug!(
            "📊 PATIENT OBSERVER: Early stage (tx < {}), skipping trend-based metrics (SCR, ULVF, POVC)",
            early_stage_threshold
        );
    } else {
        debug!(
            "📊 PATIENT OBSERVER: Full analysis mode (tx >= {})",
            early_stage_threshold
        );
    }

    // Step 0.5: CLUSTER HUNTER - Early Cabal Detection (FAIL-FAST GATE)
    //
    // This runs BEFORE other modules as a fail-fast gate.
    // If we detect coordinated wallet clusters (cabal), reject immediately.
    //
    // Why first? Because if wallets are sybils controlled by one entity,
    // all other analysis (organic detection, dev intent, etc.) is meaningless.
    //
    // For pump.fun: This catches coordinated pump-and-dump schemes where
    // multiple wallets buy in block 0-1, then dump after graduation.
    // Uses configurable threshold from orchestrator_thresholds

    if let Some(ref cluster) = cluster_result {
        let cabal_threshold = oracle
            .hyper_prediction_config
            .orchestrator_thresholds
            .cabal_risk_threshold;
        if cluster.risk_score > cabal_threshold {
            warn!(
                "CLUSTER_HUNTER VETO: Cabal detected! risk={:.2} > threshold={:.2}, \
                max_cluster={}, controlled_supply={:.1}%",
                cluster.risk_score,
                cabal_threshold,
                cluster.metrics.max_cluster_size,
                cluster.metrics.controlled_supply_pct
            );

            return Ok(HyperPredictionResult {
                score: 0,
                passed: false,
                risk_level: RiskLevel::VeryHigh,
                analysis_phase: if is_early_stage {
                    AnalysisPhase::EarlyStage
                } else {
                    AnalysisPhase::FullAnalysis
                },
                analysis_started_at: start,
                ssmi_result: None,
                mpcf_result: None,
                iwim_result: None,
                praecog_result: None,
                mesa_result: None,
                scr_score: None,
                ulvf_divergence: None,
                ulvf_curl: None,
                povc_cluster: None,
                shadow_progress: candidate.shadow_bonding_progress,
                shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                base_score: 0,
                processing_time_us: start.elapsed().as_micros() as u64,
                interpretation: format!(
                    "🚫 CLUSTER VETO: Cabal Risk={:.0}% | {} holders in largest cluster, \
                    controlling {:.1}% supply | Likely coordinated pump-and-dump scheme",
                    cluster.risk_score * 100.0,
                    cluster.metrics.max_cluster_size,
                    cluster.metrics.controlled_supply_pct
                ),
                chaos_result: None,
                resonance_result: None,
                gene_safety_result: None,
                hunter_score: None,
                qedd_result: None,
                mci_result: None,
                qman_result: None,
                ligma_result: None,
                cluster_result: cluster_result.clone(),
                paradox_state: paradox_state.clone(),
                should_delay_entry: false,
                recommended_delay_ms: 0,
                second_wave_result: None,
                survivor_score_result: None,
                fractal_verdict: None,
                tcf_result: None,
                fallback_tracker: fallback_tracker.clone(),
            });
        } else {
            // DEBUG LOGGING: Cluster Hunter analysis
            debug!(
                "CLUSTER_HUNTER: analyzed {} holders, risk_score={:.2}%, max_cluster_size={}, \
                controlled_supply={:.1}%, cluster_count={}, total_clustered={}, \
                cabal_threshold={:.2}%",
                cluster.holders.len(),
                cluster.risk_score * 100.0,
                cluster.metrics.max_cluster_size,
                cluster.metrics.controlled_supply_pct,
                cluster.metrics.cluster_count,
                cluster.metrics.total_clustered_holders,
                oracle
                    .hyper_prediction_config
                    .orchestrator_thresholds
                    .cabal_risk_threshold
                    * 100.0
            );
        }
    } else {
        debug!("CLUSTER_HUNTER: No cluster analysis provided");
    }

    // DEBUG LOGGING: Chaos Engine Monte Carlo simulation (if provided)
    if let Some(ref chaos) = chaos_result {
        debug!(
            "CHAOS_ENGINE: pump_probability={:.1}%, crash_probability={:.1}%, \
            price_volatility={:.3}, simulations_run={}, median_roi={:.2}%, \
            p5_roi={:.2}%, p95_roi={:.2}%",
            chaos.pump_probability,
            chaos.crash_probability,
            chaos.price_volatility,
            chaos.num_simulations,
            chaos.median_roi,
            chaos.p5_roi,
            chaos.p95_roi
        );
    }

    // Step 1: Calculate base enhanced score (includes Shadow Ledger data)
    let base_scored = crate::oracle::scoring::score_enhanced(candidate, oracle.threshold);
    let base_score = base_scored.score;
    let risk_level = base_scored.risk_level;

    // Prepare pool state for microstructure analysis (prefer explicit, fallback to cache)
    let pool_state_for_mesa: Option<AmmPool> = if let Some(p) = explicit_pool_state {
        Some(*p)
    } else if let Some(snapshot) = pumpfun_cache.get_snapshot(&candidate.bonding_curve) {
        build_pumpfun_amm_pool(&snapshot).ok()
    } else {
        None
    };

    // =================================================================
    // [GLOBAL GUARD] LIGMA LIQUIDITY TRAP PROTECTION
    // =================================================================
    // CRITICAL: This check runs ALWAYS (when enabled), regardless of:
    // - Pool age (early stage vs. mature)
    // - Transaction count (tx_count < 2 or >= 2)
    // - Time elapsed (T < 2s or T >= 2s)
    //
    // LIGMA provides continuous protection against sudden liquidity drops
    // (High Slippage Trap) that can occur at ANY point during analysis
    // (e.g., at 5s, 6s, 7s into the scoring window).
    //
    // This is decoupled from "Patient Observer" early_stage logic below,
    // which controls trend-based metrics (SSMI, SCR, ULVF, POVC, QEDD, MCI).
    let ligma_result = if let Some(result) = ligma_result_override {
        // Use pre-computed result if provided (e.g. from Cyclic Engine)
        Some(result)
    } else if oracle.ligma_config.enabled {
        let amm_type =
            ghost_core::init_pool_parser::AmmType::from_program_id(&candidate.amm_program_id)
                .unwrap_or_else(|| {
                    warn!(
                        "Unknown AMM program ID: {}, defaulting to PumpFun",
                        candidate.amm_program_id
                    );
                    ghost_core::init_pool_parser::AmmType::PumpFun
                });
        Some(compute_ligma(
            candidate,
            pool_state_for_mesa.as_ref(),
            amm_type,
            &oracle.ligma_config,
        ))
    } else {
        debug!("LIGMA: Disabled in config, skipping analysis");
        None
    };

    if let Some(ref result) = ligma_result {
        // VETO Check #1: Liquidity Trap Risk
        if result.liquidity_trap_risk > oracle.ligma_config.veto_trap_threshold {
            warn!(
                "LIGMA VETO: Liquidity Trap detected (Risk: {:.2}, psi_ligma: {:.2})",
                result.liquidity_trap_risk, result.psi_ligma
            );
            return Ok(HyperPredictionResult {
                score: 0,
                passed: false,
                risk_level: RiskLevel::VeryHigh,
                analysis_phase: if is_early_stage { AnalysisPhase::EarlyStage } else { AnalysisPhase::FullAnalysis },
                analysis_started_at: start,
                ssmi_result: None,
                mpcf_result: None,
                iwim_result: None,
                praecog_result: None,
                mesa_result: None,
                scr_score: None,
                ulvf_divergence: None,
                ulvf_curl: None,
                povc_cluster: None,
                shadow_progress: candidate.shadow_bonding_progress,
                shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                base_score: 0,
                processing_time_us: start.elapsed().as_micros() as u64,
                interpretation: format!(
                    "🚫 LIGMA VETO: Liquidity Trap Risk={:.2}%, psi={:.2}, tradability={:.2}, worst_loss={:.0}bps | Source: {}",
                    result.liquidity_trap_risk * 100.0,
                    result.psi_ligma,
                    result.tradability_score,
                    result.worst_round_trip_loss_bps,
                    result.diagnostics.source
                ),
                chaos_result: None,
                resonance_result: None,
                gene_safety_result: None,
                hunter_score: None,
                qedd_result: None,
                mci_result: None,
                qman_result: None,
                ligma_result: Some(result.clone()),
                cluster_result: cluster_result.clone(),
                paradox_state: paradox_state.clone(),
                should_delay_entry: false,
                recommended_delay_ms: 0,
                second_wave_result: None,
                survivor_score_result: None,
                fractal_verdict: None,
                tcf_result: None,
                fallback_tracker: fallback_tracker.clone(),
            });
        }

        // VETO Check #2: Strong negative psi_ligma (per spec: psi < -0.5)
        if result.psi_ligma < oracle.ligma_config.veto_psi_ligma_threshold {
            warn!(
                "LIGMA VETO: Strong negative psi_ligma={:.2} (trap={:.2}, tradability={:.2})",
                result.psi_ligma, result.liquidity_trap_risk, result.tradability_score
            );
            return Ok(HyperPredictionResult {
                score: 0,
                passed: false,
                risk_level: RiskLevel::VeryHigh,
                analysis_phase: if is_early_stage { AnalysisPhase::EarlyStage } else { AnalysisPhase::FullAnalysis },
                analysis_started_at: start,
                ssmi_result: None,
                mpcf_result: None,
                iwim_result: None,
                praecog_result: None,
                mesa_result: None,
                scr_score: None,
                ulvf_divergence: None,
                ulvf_curl: None,
                povc_cluster: None,
                shadow_progress: candidate.shadow_bonding_progress,
                shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                base_score: 0,
                processing_time_us: start.elapsed().as_micros() as u64,
                interpretation: format!(
                    "🚫 LIGMA VETO: Negative psi_ligma={:.2} | trap={:.2}, sniper={:.2}, tradability={:.2} | Source: {}",
                    result.psi_ligma,
                    result.liquidity_trap_risk,
                    result.sniper_attractiveness,
                    result.tradability_score,
                    result.diagnostics.source
                ),
                chaos_result: None,
                resonance_result: None,
                gene_safety_result: None,
                hunter_score: None,
                qedd_result: None,
                mci_result: None,
                qman_result: None,
                ligma_result: Some(result.clone()),
                cluster_result: cluster_result.clone(),
                paradox_state: paradox_state.clone(),
                should_delay_entry: false,
                recommended_delay_ms: 0,
                second_wave_result: None,
                survivor_score_result: None,
                fractal_verdict: None,
                tcf_result: None,
                fallback_tracker: fallback_tracker.clone(),
            });
        }

        // LIGMA passed all checks - log diagnostics in DEBUG mode
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

        // VETO status logging
        if result.liquidity_trap_risk > oracle.ligma_config.veto_trap_threshold {
            debug!(
                "LIGMA: ⚠️  HIGH TRAP RISK detected: {:.1}% (threshold={:.1}%)",
                result.liquidity_trap_risk * 100.0,
                oracle.ligma_config.veto_trap_threshold * 100.0
            );
        }
    }

    // Step 2: Run parallel analysis modules
    let mut waves: Vec<HeuristicWave> = Vec::with_capacity(8);

    // 2a. Shadow Ledger diagnostics only (no scoring impact)
    if let Some(shadow_progress) = candidate.shadow_bonding_progress {
        let price_ratio = candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE);
        debug!(
            "SHADOW_LEDGER|diag: bonding_progress={}%, price_ratio={:.4}, expected_price={:.8}",
            shadow_progress,
            price_ratio.unwrap_or(1.0),
            candidate.expected_price.unwrap_or(PUMP_INITIAL_PRICE)
        );
    }

    // 2a-bis. LIGMA wave injection (if enabled)
    // Injects psi_ligma into QASS superposition for wave interference
    if let Some(ref result) = ligma_result {
        let ligma_wave = build_ligma_wave(result);
        debug!(
            "LIGMA WAVE: ψ_ligma injected (amplitude={:.3}, phase={:.3}, confidence={:.3})",
            ligma_wave.amplitude, ligma_wave.phase, ligma_wave.confidence
        );
        waves.push(ligma_wave);
    }

    // 2b. SSMI Analysis (transaction timing patterns)
    // Early stage: SKIP - requires transaction history
    let ssmi_result = if is_early_stage {
        debug!("SSMI: Skipped (early stage, tx_count < 2)");
        None
    } else if let Some(timestamps) = tx_timestamps {
        if timestamps.len() >= 4 {
            let result = oracle.ssmi.analyze(timestamps);
            let wave = build_ssmi_wave(&result);
            waves.push(wave);

            // DEBUG LOGGING: SSMI temporal patterns
            debug!(
                "SSMI: shannon_entropy={:.3}, source_type={:?}, confidence={:.2}, \
                ssmi_score={:.3}, scr_bot_probability={:.3}, ar_correlation={:.3}",
                result.shannon_entropy,
                result.source_type,
                result.confidence,
                result.ssmi_score,
                result.scr_bot_probability,
                result.ar_correlation
            );

            Some(result)
        } else {
            debug!(
                "SSMI: Insufficient timestamps (has {}, need ≥4)",
                timestamps.len()
            );
            None
        }
    } else {
        debug!("SSMI: No timestamp data provided");
        None
    };

    // 2c. MPCF Analysis (byte-level fingerprinting)
    // EXCEPTION: MPCF runs in early stage if tx_data is present (Early Warning System)
    let mpcf_result = if let Some(tx_bytes) = tx_data {
        let inference = mpcf_infer(tx_bytes);

        // DEBUG LOGGING: MPCF actor classification
        debug!(
            "MPCF: actor={:?}, confidence={:.2}, entropy={:.3}, fingerprint={:?}",
            inference.actor, inference.confidence, inference.entropy, inference.fingerprint
        );

        // Convert MPCF to wave with phase mapping:
        // Humans: 0.7 (bullish), high amplitude 0.9
        // Bots: -0.6 to -0.8 (bearish), low amplitude
        // Unknown: 0.0 (neutral), medium amplitude 0.5
        let (amplitude, phase, confidence) = match inference.actor {
            ActorType::HumanMobile | ActorType::HumanDesktop => {
                (0.9, 0.7, inference.confidence as f64)
            }
            ActorType::SniperScript | ActorType::MEVArb => (0.3, -0.6, inference.confidence as f64),
            ActorType::SybilBot => (0.2, -0.8, inference.confidence as f64),
            _ => (0.5, 0.0, 0.3),
        };
        waves.push(HeuristicWave::new("ψ_mpcf", amplitude, phase, confidence));

        Some(inference)
    } else {
        debug!("MPCF: No transaction bytes provided (Cold Start)");
        None
    };

    // 2d. IWIM Analysis (dev-wallet intent mapping)
    // Runs in both modes - provides valuable dev-wallet intent data
    if let Some(ref iwim) = iwim_result {
        let wave = build_iwim_wave(iwim);
        waves.push(wave);

        // DEBUG LOGGING: IWIM dev safety analysis
        // Differentiate between early stage (S1-S7, default trust) and late stage (S8-S12, real data)
        let stage_info = if cold_start_active {
            "S1-S7 (default trust, Safety=1.0)"
        } else {
            "S8-S12 (RPC data available)"
        };

        debug!(
            "IWIM: organic_score={:.2}, sybil_score={:.2}, rug_threat_score={:.2}, \
            confidence={:.2}, stage={}, execution_time={}us",
            iwim.organic_score,
            iwim.sybil_score,
            iwim.rug_threat_score,
            iwim.confidence,
            stage_info,
            iwim.execution_time_us
        );
    } else {
        debug!("IWIM: No dev wallet trace available");
    }

    // 2d2. PRAECOG Analysis (adversarial exploitability simulation)
    // Runs in both modes - static pool analysis independent of history
    //
    // CRITICAL: explicit_pool_state MUST take priority over cache to prevent
    // PRAECOG from always seeing genesis pool (30 SOL / 1.073T tokens).
    // See issue: "PRAECOG MUST TAKE REAL POOL STATE"
    let praecog_result = {
        // Determine pool source with explicit priority:
        // 1. explicit_pool_state (from ShadowLedger/runtime) - ALWAYS preferred
        // 2. pumpfun_cache snapshot - fallback only
        let (pool, pool_source): (Option<AmmPool>, &str) = if let Some(p) = explicit_pool_state {
            info!(
                "PRAECOG_INPUT_SOURCE=explicit, sol_reserve={}, token_reserve={}, fee_bps={}",
                p.reserve_a, p.reserve_b, p.fee_bps
            );
            (Some(*p), "explicit_pool_state")
        } else if let Some(snapshot) = pumpfun_cache.get_snapshot(&candidate.bonding_curve) {
            match build_pumpfun_amm_pool(&snapshot) {
                Ok(pool) => {
                    info!(
                        "PRAECOG_INPUT_SOURCE=cache, sol_reserve={}, token_reserve={}, fee_bps={}, slot={:?}",
                        pool.reserve_a, pool.reserve_b, pool.fee_bps, snapshot.last_update_slot
                    );
                    (Some(pool), "pumpfun_cache")
                }
                Err(e) => {
                    debug!(
                        "PRAECOG SKIPPED: Failed to build AMM pool from snapshot: {}",
                        e
                    );
                    (None, "none")
                }
            }
        } else {
            (None, "none")
        };

        if let Some(pool) = pool {
            // PART 3: Check for genesis pool when live data exists
            // Genesis pool is: 30 SOL (30_000_000_000 lamports), 1.073T tokens, 1% fee
            let is_genesis_pool = is_pool_genesis(&pool);

            // Check if we have live transaction data
            let has_live_data = tx_metrics
                .map(|m| m.tx_count > 0 || m.total_volume_sol > 0.0)
                .unwrap_or(false);

            // WARN if live data exists but PRAECOG still sees genesis pool
            if has_live_data && is_genesis_pool {
                let (tx_count, volume) = tx_metrics
                    .map(|m| (m.tx_count, m.total_volume_sol))
                    .unwrap_or((0, 0.0));
                warn!(
                    "PRAECOG_LIVE_TX_BUT_GENESIS_POOL: Real transaction data exists (tx_count={}, volume={:.4} SOL), \
                    but PRAECOG still sees genesis pool ({} SOL / {} tokens). \
                    Pool source: {}. THIS IS A PIPELINE BUG - explicit_pool_state should be provided.",
                    tx_count, volume,
                    pool.reserve_a as f64 / 1_000_000_000.0,
                    pool.reserve_b,
                    pool_source
                );
            }

            // Choose parameters based on mode
            let params = if is_early_stage {
                PraecogParams::fast() // 64 paths, 2 steps for speed
            } else {
                PraecogParams::default() // 256 paths, 4 steps for thorough analysis
            };

            // Get early swaps from cache (heapless, within TTL) and convert to Vec<SwapInfo>
            let early_swap_events = pumpfun_cache.get_early_swaps(&candidate.bonding_curve);
            let initial_swaps: Vec<crate::oracle::ultrafast::SwapInfo> = early_swap_events
                .events
                .iter()
                .take(early_swap_events.len as usize)
                .filter_map(|opt_event| {
                    opt_event
                        .as_ref()
                        .map(|event| crate::oracle::ultrafast::SwapInfo {
                            amount_in: event.amount_in as u128,
                            is_buy: event.is_buy,
                            timestamp_ms: event.timestamp_ms,
                        })
                })
                .collect();

            let input = PraecogInput {
                pool,
                initial_swaps,
                params,
            };

            let result = praecog_analyze(&input);

            // Build PRAECOG wave for QASS
            let wave = build_praecog_wave(&result);
            waves.push(wave);

            debug!(
                "PRAECOG: adversarial_score={:.3}, crash_feasibility={:.3}, sandwich_feasibility={:.3}, \
                min_capital_sol={:.2}, time={}μs, swaps_simulated={}, \
                pool_source={}, pool_sol={:.2}, pool_tokens={:.0}",
                result.adversarial_score,
                result.crash_feasibility,
                result.sandwich_feasibility,
                result.min_capital_to_crash_sol,
                result.analysis_time_us,
                early_swap_events.len,
                pool_source,
                pool.reserve_a as f64 / 1e9,
                pool.reserve_b as f64
            );

            Some(result)
        } else {
            debug!(
                "PRAECOG SKIPPED: No valid pool state available for bonding_curve={}",
                candidate.bonding_curve
            );
            None
        }
    };

    // 2d2. FRE Analysis (Fractal Resonance Engine)
    // Runs in both phases - analyzes swap patterns for botnet/chaos/organic signals
    // Requires minimum 10 swaps for meaningful analysis
    let fractal_verdict = {
        // Reuse early_swap_events from PRAECOG analysis
        let early_swap_events = pumpfun_cache.get_early_swaps(&candidate.bonding_curve);
        let swaps: Vec<crate::oracle::ultrafast::SwapInfo> = early_swap_events
            .events
            .iter()
            .take(early_swap_events.len as usize)
            .filter_map(|opt_event| {
                opt_event
                    .as_ref()
                    .map(|event| crate::oracle::ultrafast::SwapInfo {
                        amount_in: event.amount_in as u128,
                        is_buy: event.is_buy,
                        timestamp_ms: event.timestamp_ms,
                    })
            })
            .collect();

        if swaps.len() >= 10 {
            let verdict = oracle.fractal_engine.lock().analyze(&swaps);

            // DEBUG LOGGING: FRE metrics
            debug!(
                "[FRE] H={:.2} | Coh={:.2} | Sig={:.3} | Verdict={:?} | Organic={}",
                verdict.hurst_global,
                verdict.coherence,
                verdict.stability_sigma,
                verdict.action,
                verdict.organic_score
            );

            // GUARDIAN VETO: Skip if botnet/chaos detected
            if let FractalAction::Skip(ref reason) = verdict.action {
                warn!("[FRE] 🛑 VETO: {}", reason);

                // Return immediately with veto result
                return Ok(HyperPredictionResult {
                    score: 0,
                    passed: false,
                    risk_level: RiskLevel::VeryHigh,
                    analysis_phase: if is_early_stage {
                        AnalysisPhase::EarlyStage
                    } else {
                        AnalysisPhase::FullAnalysis
                    },
                    analysis_started_at: start,
                    ssmi_result: None,
                    mpcf_result: None,
                    iwim_result,
                    praecog_result: None,
                    mesa_result: None,
                    scr_score: None,
                    ulvf_divergence: None,
                    ulvf_curl: None,
                    povc_cluster: None,
                    shadow_progress: candidate.shadow_bonding_progress,
                    shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                    base_score: 0,
                    processing_time_us: start.elapsed().as_micros() as u64,
                    interpretation: format!("🚫 FRE VETO: {}", reason),
                    chaos_result: None,
                    resonance_result: None,
                    gene_safety_result: None,
                    hunter_score: None,
                    qedd_result: None,
                    mci_result: None,
                    qman_result: None,
                    ligma_result: None,
                    cluster_result: None,
                    paradox_state: None,
                    should_delay_entry: false,
                    recommended_delay_ms: 0,
                    second_wave_result: None,
                    survivor_score_result: None,
                    fractal_verdict: Some(verdict),
                    tcf_result: None,
                    fallback_tracker: crate::config::FallbackTracker::new(),
                });
            }

            Some(verdict)
        } else {
            debug!(
                "[FRE] Skipped (insufficient swaps: {}, need >= 10)",
                swaps.len()
            );
            None
        }
    };

    // 2e. SCR Analysis (bot detection via FFT)
    // Early stage: SKIP - requires transaction history
    let scr_score = if is_early_stage {
        debug!("SCR: Skipped (early stage, tx_count < 2)");
        None
    } else if let Some(timestamps) = tx_timestamps {
        if timestamps.len() >= 4 {
            let scr = oracle.hyper.calculate_scr(timestamps);
            let wave = build_scr_wave(scr as f64, timestamps.len());
            waves.push(wave);

            // DEBUG LOGGING: SCR harmonic detection
            debug!(
                "SCR: score={:.3}, timestamps_analyzed={}, harmonic_strength={:.2}, bot_detection={}",
                scr,
                timestamps.len(),
                if scr > 0.7 { "HIGH" } else if scr > 0.4 { "MEDIUM" } else { "LOW" },
                if scr > 0.65 { "LIKELY_BOT" } else { "ORGANIC" }
            );

            Some(scr)
        } else {
            debug!(
                "SCR: Insufficient timestamps (has {}, need ≥4)",
                timestamps.len()
            );
            None
        }
    } else {
        debug!("SCR: No timestamp data");
        None
    };

    // 2f. ULVF Analysis (liquidity vector field)
    // Early stage: SKIP - requires transaction history for meaningful analysis
    let (ulvf_divergence, ulvf_curl) = if is_early_stage {
        debug!("ULVF: Skipped (early stage, tx_count < 2)");
        (None, None)
    } else {
        let (t0_tx_count, t0_unique, t1_tx_count, t1_unique) = if let Some(metrics) = tx_metrics {
            // Use real data
            let half_tx = (metrics.tx_count / 2).max(1);
            let half_unique = (metrics.unique_addrs / 2).max(1);
            (
                half_tx,
                half_unique,
                metrics.tx_count.saturating_sub(half_tx).max(1),
                metrics.unique_addrs.saturating_sub(half_unique).max(1),
            )
        } else {
            // FALLBACK: Conservative defaults (full analysis mode without metrics)
            fallback_tracker.record(
                crate::config::FallbackType::UlvfTxCounts,
                oracle.fallback_config.ulvf.confidence_penalty,
                oracle.fallback_config.max_cumulative_penalty,
            );
            debug!(
                "ULVF: Using fallback (penalty={:.0}%)",
                oracle.fallback_config.ulvf.confidence_penalty * 100.0
            );
            (
                oracle.fallback_config.ulvf.default_tx_count_t0,
                oracle.fallback_config.ulvf.default_unique_t0,
                oracle.fallback_config.ulvf.default_tx_count_t1,
                oracle.fallback_config.ulvf.default_unique_t1,
            )
        };

        let t0 = MarketSnapshot {
            tx_key: None,
            timestamp_ms: candidate.timestamp,
            volume_sol: candidate.initial_liquidity_sol,
            tx_count: t0_tx_count,
            unique_addrs: t0_unique,
        };
        let t1 = MarketSnapshot {
            tx_key: None,
            timestamp_ms: candidate.timestamp + 1000,
            volume_sol: candidate
                .virtual_sol_reserves
                .map(|v| v as f64 / 1_000_000_000.0)
                .unwrap_or(candidate.initial_liquidity_sol),
            tx_count: t1_tx_count,
            unique_addrs: t1_unique,
        };

        let (div, curl) = oracle.hyper.calculate_ulvf(&t0, &t1);
        let wave = build_ulvf_wave(div as f64, curl as f64);
        waves.push(wave);

        // DEBUG LOGGING: ULVF momentum classification
        let momentum_class = if div.abs() < 0.3 && curl.abs() < 0.3 {
            "STAGNANT"
        } else if div > 0.5 {
            "EXPANDING"
        } else if div < -0.5 {
            "CONTRACTING"
        } else if curl.abs() > 0.5 {
            "ROTATING"
        } else {
            "NEUTRAL"
        };

        debug!(
            "ULVF: divergence={:.3}, curl={:.3}, momentum_class={}, \
            t0_metrics=(tx={}, unique={}), t1_metrics=(tx={}, unique={})",
            div, curl, momentum_class, t0_tx_count, t0_unique, t1_tx_count, t1_unique
        );

        (Some(div), Some(curl))
    };

    // 2f. MESA Analysis (microstructure execution-shape)
    let mesa_result =
        if let (Some(pool), Some(metrics)) = (pool_state_for_mesa.as_ref(), tx_metrics) {
            let res = oracle
                .mesa_analyzer
                .analyze_microstructure(pool, std::slice::from_ref(metrics));
            let net_buying_pressure = metrics.buy_pressure_ratio();
            let amplitude = if res.wash_likeness > 0.8 {
                0.0
            } else {
                res.organic_likeness as f64
            };
            let phase = if net_buying_pressure > 0.5 { 0.0 } else { PI };
            let confidence = res.entropy_score as f64;
            waves.push(HeuristicWave::new("ψ_mesa", amplitude, phase, confidence));

            // DEBUG LOGGING: MESA microstructure analysis
            let market_quality = if res.wash_likeness > 0.70 {
                "WASH_TRADING_DETECTED"
            } else if res.bot_likeness > 0.75 {
                "BOT_DOMINATED"
            } else if res.organic_likeness > 0.70 {
                "ORGANIC_ACTIVITY"
            } else {
                "MIXED"
            };

            debug!(
                "MESA: wash_likeness={:.2}%, bot_likeness={:.2}%, organic_likeness={:.2}%, \
            entropy={:.3}, impact_efficiency={:.3}, market_quality={}, \
            buy_pressure_ratio={:.2}",
                res.wash_likeness * 100.0,
                res.bot_likeness * 100.0,
                res.organic_likeness * 100.0,
                res.entropy_score,
                res.impact_efficiency,
                market_quality,
                net_buying_pressure
            );

            Some(res)
        } else {
            debug!("MESA: Skipped (no pool state or tx metrics available)");
            None
        };

    // 2g. POVC Analysis (cluster prediction)
    // Early stage: SKIP - requires transaction history for meaningful clustering
    let povc_cluster = if is_early_stage {
        debug!("POVC: Skipped (early stage, tx_count < 2)");
        None
    } else {
        // Use real transaction metrics when available
        let (tx_count, unique_addrs) = if let Some(metrics) = tx_metrics {
            (metrics.tx_count, metrics.unique_addrs)
        } else {
            fallback_tracker.record(
                crate::config::FallbackType::PovcCluster,
                oracle.fallback_config.povc.confidence_penalty,
                oracle.fallback_config.max_cumulative_penalty,
            );
            debug!(
                "POVC: Using fallback (penalty={:.0}%)",
                oracle.fallback_config.povc.confidence_penalty * 100.0
            );
            (
                oracle.fallback_config.povc.default_tx_count,
                oracle.fallback_config.povc.default_unique_addrs,
            )
        };

        let snapshot = MarketSnapshot {
            tx_key: None,
            timestamp_ms: candidate.timestamp,
            volume_sol: candidate.initial_liquidity_sol,
            tx_count,
            unique_addrs,
        };
        let cluster = oracle.hyper.calculate_povc(&snapshot);
        let wave = build_povc_wave(cluster);
        waves.push(wave);

        // DEBUG LOGGING: POVC cluster prediction
        let cluster_interpretation = match cluster {
            0 => "ULTRA_ORGANIC (whales/real traders)",
            1 => "ORGANIC (small genuine traders)",
            2 => "BOT_NOISE (sniper bots)",
            3 => "SYBIL_ATTACK (coordinated wallets)",
            _ => "UNKNOWN",
        };

        debug!(
            "POVC: predicted_cluster={}, interpretation={}, \
            tx_count={}, unique_addrs={}, volume_sol={:.2}",
            cluster,
            cluster_interpretation,
            tx_count,
            unique_addrs,
            candidate.initial_liquidity_sol
        );

        Some(cluster)
    };

    // 2h. Add liquidity wave (using config-controlled scale)
    // Always runs - essential static analysis for both modes
    let liquidity_wave = build_liquidity_wave_scaled(
        candidate.initial_liquidity_sol,
        oracle.normalization_config.liquidity_scale,
    );
    waves.push(liquidity_wave);

    // 2i. Add dev buy wave (using config-controlled scale)
    // Always runs - essential static analysis for both modes
    let dev_buy_wave = build_dev_buy_wave_scaled(
        candidate.has_dev_buy,
        candidate.dev_buy_sol,
        oracle.normalization_config.volume_scale,
    );
    waves.push(dev_buy_wave);

    // 2j. QMAN Analysis (Capital Flow Prediction)
    //
    // QMAN (Quantum Money-flow Amplitude Network) tracks wallet "energy" and
    // predicts capital flow patterns. This is valuable in BOTH modes:
    // - Early stage: Detect early smart money accumulation
    // - Full analysis: Track ongoing capital flow dynamics
    //
    // QMAN answers: "Is smart money entering or exiting?"
    let qman_result: Option<QmanResult> = {
        let qman_start = Instant::now();

        // Get current state from wallet energy tracker
        let state = oracle.wallet_energy_tracker.get_state_vector();

        // Only run QMAN if we have some tracked data
        if state.active_wallets >= 2 && state.total_energy > 0.1 {
            // Update transition matrix with latest observations
            oracle.transition_matrix.update();

            // Get the sparse transition matrix
            let sparse_matrix = oracle.transition_matrix.get_matrix();

            // Run unitary evolution prediction
            let prediction = oracle.unitary_evolution.predict(&state, &sparse_matrix);

            if let Some(pred) = prediction {
                // Detect trading signals
                let signal_result = oracle.qman_signal_detector.analyze(&state, &pred);

                // Calculate overall QMAN score
                let qman_score = calculate_qman_score_impl(oracle, &signal_result, &state, &pred);

                // Calculate confidence based on data quality
                let confidence = calculate_qman_confidence_impl(oracle, &state);

                // Calculate net energy flow
                let net_energy_flow = pred
                    .top_flows
                    .iter()
                    .map(|(_, _, change)| *change as f32)
                    .sum::<f32>();

                // Count high-energy wallets (above average energy)
                let avg_energy = state.total_energy / (state.active_wallets as f64).max(1.0);
                let wallet_snapshot = oracle.wallet_energy_tracker.get_wallet_cache_snapshot();
                let high_energy_wallets = wallet_snapshot
                    .values()
                    .filter(|w| w.energy as f64 > avg_energy * 1.5)
                    .count();

                let analysis_time_us = qman_start.elapsed().as_micros() as u64;

                // DEBUG LOGGING: QMAN capital flow prediction
                debug!(
                    "QMAN: signal={:?}, score={:.2}, confidence={:.2}, high_energy_wallets={}, \
                    net_energy_flow={:.2}, active_wallets={}, total_energy={:.2}, \
                    avg_energy={:.3}, time={}μs",
                    signal_result.signal,
                    qman_score,
                    confidence,
                    high_energy_wallets,
                    net_energy_flow,
                    state.active_wallets,
                    state.total_energy,
                    avg_energy,
                    analysis_time_us
                );

                // Build QMAN wave for QASS superposition
                let amplitude = qman_score as f64;
                let phase = match signal_result.signal {
                    TradingSignal::AllInMainTrend => 0.9, // Very bullish - hyper-bubble
                    TradingSignal::PrepareSecondWave => 0.6, // Bullish - re-accumulation
                    TradingSignal::Hold => 0.0,           // Neutral
                    TradingSignal::ExitNow => -0.7,       // Bearish - capital drain
                };
                let wave_confidence = confidence as f64;
                waves.push(HeuristicWave::new(
                    "ψ_qman",
                    amplitude,
                    phase,
                    wave_confidence,
                ));

                Some(QmanResult {
                    signal: signal_result.signal,
                    qman_score,
                    confidence,
                    net_energy_flow,
                    high_energy_wallets,
                    analysis_time_us,
                    reason: signal_result.reason.clone(),
                })
            } else {
                debug!("QMAN: Skipped - insufficient confidence for prediction");
                None
            }
        } else {
            debug!(
                "QMAN: Skipped - insufficient data (wallets={}, energy={:.2})",
                state.active_wallets, state.total_energy
            );
            None
        }
    };

    // Step 3: QASS scoring is deprecated - using SurvivorScore as main scoring system
    // Create a default QASS result for backward compatibility (returns neutral values)
    let qass_scorer = QuantumAmplitudeScorer::default();
    let qass_result = qass_scorer.score(&waves);

    // Step 3b: Compute QEDD and MCI (cold-start aware)
    let market_signals = build_market_signals_impl(
        oracle,
        candidate,
        &ssmi_result,
        &mpcf_result,
        scr_score,
        ulvf_divergence,
        ulvf_curl,
        &resonance_result,
        snapshot_price,
        tx_metrics,
        cold_start_active,
    );

    if is_early_stage && cold_start_active {
        debug!("QEDD/MCI COLD START: tx_count<10, using MarketSignals extrapolation instead of neutral defaults");
    } else if is_early_stage {
        debug!("QEDD/MCI early stage: computing with limited history (tx_count < 2) using available signals");
    }

    let qedd_result = oracle.qedd.compute_qedd_sync(&market_signals);
    let mci_result = oracle.mci.compute_mci(&market_signals);

    // DEBUG LOGGING: QEDD survival probability
    debug!(
        "QEDD: lambda_now={:.3}, survival_1s={:.2}%, survival_5s={:.2}%, \
        survival_30s={:.2}%, survival_60s={:.2}%, veto_threshold={:.3}",
        qedd_result.lambda_now,
        qedd_result.survival_1s * 100.0,
        qedd_result.survival_5s * 100.0,
        qedd_result.survival_30s * 100.0,
        qedd_result.survival_60s * 100.0,
        oracle.qedd.config.lambda_abort_threshold
    );

    // DEBUG LOGGING: MCI market coherence
    debug!(
        "MCI: mci={:.3}, dc={:.3}, sc={:.3}, veto_threshold={:.3}",
        mci_result.mci, mci_result.dc, mci_result.sc, oracle.mci.config.coherence_abort_threshold
    );

    // Check for veto conditions (ONLY in full analysis mode)
    // In early stage mode, we still compute QEDD/MCI but avoid veto to prevent aborts on sparse data
    if !is_early_stage {
        let lambda_abort = oracle.qedd.config.lambda_abort_threshold;
        let coherence_abort = oracle.mci.config.coherence_abort_threshold;

        if qedd_result.lambda_now > lambda_abort {
            debug!(
                "QEDD VETO: lambda_now={:.3} > threshold={:.3}",
                qedd_result.lambda_now, lambda_abort
            );
            return Ok(HyperPredictionResult {
                score: 0,
                passed: false,
                risk_level: RiskLevel::VeryHigh,
                analysis_phase: if is_early_stage {
                    AnalysisPhase::EarlyStage
                } else {
                    AnalysisPhase::FullAnalysis
                },
                analysis_started_at: start,
                ssmi_result,
                mpcf_result,
                iwim_result,
                praecog_result,
                mesa_result: mesa_result.clone(),
                scr_score,
                ulvf_divergence,
                ulvf_curl,
                povc_cluster,
                shadow_progress: candidate.shadow_bonding_progress,
                shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                base_score: 0,
                processing_time_us: start.elapsed().as_micros() as u64,
                interpretation: format!(
                    "📊 PATIENT | 🔴 VETO: QEDD lambda={:.3} exceeds threshold={:.3}",
                    qedd_result.lambda_now, lambda_abort
                ),
                chaos_result,
                resonance_result,
                gene_safety_result,
                hunter_score,
                qedd_result: Some(qedd_result),
                mci_result: Some(mci_result),
                qman_result: qman_result.clone(),
                ligma_result: ligma_result.clone(),
                cluster_result: cluster_result.clone(),
                paradox_state: paradox_state.clone(),
                should_delay_entry: false,
                recommended_delay_ms: 0,
                second_wave_result: None,
                survivor_score_result: None,
                fractal_verdict: None,
                tcf_result: None,
                fallback_tracker: fallback_tracker.clone(),
            });
        }

        if mci_result.mci < coherence_abort {
            debug!(
                "MCI VETO: mci={:.3} < threshold={:.3}",
                mci_result.mci, coherence_abort
            );
            return Ok(HyperPredictionResult {
                score: 0,
                passed: false,
                risk_level: RiskLevel::VeryHigh,
                analysis_phase: if is_early_stage {
                    AnalysisPhase::EarlyStage
                } else {
                    AnalysisPhase::FullAnalysis
                },
                analysis_started_at: start,
                ssmi_result,
                mpcf_result,
                iwim_result,
                praecog_result,
                mesa_result: mesa_result.clone(),
                scr_score,
                ulvf_divergence,
                ulvf_curl,
                povc_cluster,
                shadow_progress: candidate.shadow_bonding_progress,
                shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
                base_score: 0,
                processing_time_us: start.elapsed().as_micros() as u64,
                interpretation: format!(
                    "📊 PATIENT | 🔴 VETO: MCI={:.3} below threshold={:.3}",
                    mci_result.mci, coherence_abort
                ),
                chaos_result,
                resonance_result,
                gene_safety_result,
                hunter_score,
                qedd_result: Some(qedd_result),
                mci_result: Some(mci_result),
                qman_result: qman_result.clone(),
                ligma_result: ligma_result.clone(),
                cluster_result: cluster_result.clone(),
                paradox_state: paradox_state.clone(),
                should_delay_entry: false,
                recommended_delay_ms: 0,
                second_wave_result: None,
                survivor_score_result: None,
                fractal_verdict: None,
                tcf_result: None,
                fallback_tracker: fallback_tracker.clone(),
            });
        }
    }

    // Step 4.5: PARADOX SENSOR - HFT Activity Detection
    //
    // The ParadoxSensor analyzes network-level packet timing to detect
    // synchronized HFT bot activity. Unlike on-chain analysis, this works
    // at T=0 (before any transactions are confirmed).
    //
    // Strategy "Cierpliwy Obserwator" (Patient Observer):
    // - If phase_sync > 0.7 (high bot synchronization), recommend delaying entry
    // - If pds_score > 80 (extreme tension), consider this a warning signal
    // - We DO NOT reject based on ParadoxSensor - we DELAY
    //
    // The idea is to wait for HFT bots to extract their profits and exit,
    // then enter during the "second wave" of organic buying.

    let (should_delay_entry, recommended_delay_ms) = if let Some(ref paradox) = paradox_state {
        // High phase synchronization = coordinated bot activity
        // This is normal in block 0-2, but should decrease after
        let high_bot_sync = paradox.phase_sync > 0.70;

        // High tension = extreme network activity
        let high_tension = paradox.tension > 75.0;

        // Echo spike = potential pump incoming (positive signal, but wait for confirmation)
        let echo_spike_detected = paradox.is_echo_spike;

        if high_bot_sync && high_tension {
            // Strong HFT activity detected - recommend significant delay
            let delay = calculate_recommended_delay(paradox);
            debug!(
                "PARADOX: High HFT activity detected! phase_sync={:.2}, tension={:.1}, pds={:.1} → delay {}ms",
                paradox.phase_sync, paradox.tension, paradox.pds_score, delay
            );
            (true, delay)
        } else if high_bot_sync || echo_spike_detected {
            // Moderate bot activity or echo spike - shorter delay
            let delay = 3000; // 3 seconds
            debug!(
                "PARADOX: Moderate bot activity. phase_sync={:.2}, echo_spike={} → delay {}ms",
                paradox.phase_sync, echo_spike_detected, delay
            );
            (true, delay)
        } else {
            // Low bot activity - safe to proceed
            debug!(
                "PARADOX: Low bot activity (phase_sync={:.2}, tension={:.1}). Safe to proceed.",
                paradox.phase_sync, paradox.tension
            );
            (false, 0)
        }
    } else {
        // No ParadoxSensor data available - proceed without delay recommendation
        debug!("PARADOX: No sensor data available, proceeding without delay");
        (false, 0)
    };

    // Step 4.6: SECOND WAVE DETECTOR
    // If ParadoxSensor suggested delay, check if second wave is now active
    // This helps answer: "Have HFT bots exited and is organic buying starting?"
    let second_wave_result = if should_delay_entry || !is_early_stage {
        // Get current price ratio from shadow ledger or use 1.0 as baseline
        let current_price_ratio = snapshot_price
            .map(|p| (p / PUMP_INITIAL_PRICE) as f32)
            .unwrap_or(1.0);

        // Convert single MPCF result to slice for analyze method
        // Using std::slice::from_ref avoids allocation when we have a single result
        let mpcf_slice: Option<&[ActorInference]> = mpcf_result.as_ref().map(std::slice::from_ref);

        let result = oracle.second_wave_detector.analyze(
            candidate.timestamp,
            tx_metrics.unwrap_or(&TransactionMetrics::new()),
            mpcf_slice,
            current_price_ratio,
        );

        debug!(
            "SECOND_WAVE: action={:?}, score={:.2}, hft_exit={:.2}, blocks={}",
            result.recommended_action,
            result.second_wave_score,
            result.hft_exit_confidence,
            result.blocks_since_launch
        );

        Some(result)
    } else {
        debug!("SECOND_WAVE: Skipped (early stage mode without delay recommendation)");
        None
    };

    // Step 4.7: SURVIVOR SCORE - Interpretable final scoring
    //
    // SurvivorScore replaces QASS with economically meaningful scoring.
    // Each component has clear interpretation and can be calibrated on historical data.
    //
    // Components:
    // - Survival: QEDD + IWIM + ClusterHunter (rug probability)
    // - Momentum: SOBP + QMAN + Chaos (buying pressure)
    // - Quality: MPCF + MESA + SCR (organic activity)
    // - Risk Discount: Wash trading + Smart money exit + Price crash + Anomaly
    let age_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|now| {
            if candidate.timestamp > 1_000_000_000_000 {
                let now_ms = now.as_millis() as u64;
                now_ms
                    .checked_sub(candidate.timestamp)
                    .map(|delta| delta as f32 / 1000.0)
            } else {
                now.as_secs()
                    .checked_sub(candidate.timestamp)
                    .map(|delta| delta as f32)
            }
        });

    let mut survivor_input = SurvivorScoreInput {
        qedd_survival_60s: Some(qedd_result.survival_30s as f32),
        iwim_threat_score: iwim_result.as_ref().map(|i| i.rug_threat_score),
        cluster_risk_score: cluster_result.as_ref().map(|c| c.risk_score),
        sobp_momentum: tx_metrics.map(|m| m.buy_pressure_ratio() as f32),
        qman_score: qman_result.as_ref().map(|q| q.qman_score),
        chaos_pump_prob: chaos_result
            .as_ref()
            .map(|c| c.pump_probability as f32 / 100.0),
        mpcf_organic_ratio: mpcf_result.as_ref().map(|m| {
            if matches!(m.actor, ActorType::HumanDesktop | ActorType::HumanMobile) {
                m.confidence
            } else {
                1.0 - m.confidence
            }
        }),
        mesa_organic_likeness: mesa_result.as_ref().map(|m| m.organic_likeness),
        scr_bot_score: scr_score,
        unique_wallet_ratio: tx_metrics.map(|m| m.unique_ratio() as f32),
        mesa_wash_likeness: mesa_result.as_ref().map(|m| m.wash_likeness),
        qman_exit_signal: qman_result
            .as_ref()
            .map(|q| matches!(q.signal, TradingSignal::ExitNow))
            .unwrap_or(false),
        price_crash_detected: second_wave_result
            .as_ref()
            .map(|s| s.price_vs_peak_ratio < 0.3)
            .unwrap_or(false),
        paradox_anomaly: paradox_state
            .as_ref()
            .map(|p| p.anomaly_detected)
            .unwrap_or(false),
        // LIGMA integration
        ligma_tradability_score: ligma_result.as_ref().map(|r| r.tradability_score as f32),
        ligma_psi: ligma_result.as_ref().map(|r| r.psi_ligma as f32),
        ligma_liquidity_trap_risk: ligma_result.as_ref().map(|r| r.liquidity_trap_risk as f32),
        // Transaction count for dynamic threshold selection
        tx_count: tx_metrics.map(|m| m.tx_count as u64),
        age_secs,
        ..Default::default()
    };

    if let Some(signal) = ecto_signal {
        survivor_input.ecto_score = Some(signal.score as f32);
        survivor_input.ecto_verdict = Some(signal.verdict);
    }

    if let Some(bva) = bva_output {
        let chaos = survivor_input.chaos_pump_prob.unwrap_or(0.0);
        let denom = (1.0 - chaos).max(0.05);
        survivor_input.bva_score = Some((bva.score as f32 / denom).clamp(0.0, 1.0));
    }

    if let Some(panic) = panic_output {
        survivor_input.panic_pressure =
            Some((panic.pressure.min(3.0) / 3.0).clamp(0.0, 1.0) as f32);
        if survivor_input.tcr_causality.is_none() {
            survivor_input.tcr_causality = panic.tcr_value.map(|v| v.clamp(0.0, 1.0) as f32);
        }
    }

    if let Some(score) = tcr_score {
        survivor_input.tcr_causality = Some(score.tcr_value.clamp(0.0, 1.0) as f32);
    }

    if let Some(cir) = cir_strength {
        survivor_input.cir_strength = Some(cir.clamp(0.0, 1.0));
    }

    let survivor_score_result = survivor_calculator.calculate(&survivor_input);

    // DEBUG LOGGING: SurvivorScore comprehensive breakdown
    debug!(
        "SURVIVOR_SCORE: final_score={} ({}), raw_score={:.3}, confidence={:.2}, \
        survival={:.2} (qedd={:.2}, iwim={:.2}, cluster={:.2}), \
        momentum={:.2} (sobp={:.2}, qman={:.2}, chaos={:.2}), \
        quality={:.2} (mpcf={:.2}, mesa={:.2}, scr={:.2}, wallets={:.2}, ligma={:.2}), \
        risk_discount={:.2} (wash={:.2}, exit={}, crash={}, anomaly={}), \
        interpretation={}",
        survivor_score_result.score,
        if survivor_score_result.passed {
            "PASS"
        } else {
            "FAIL"
        },
        survivor_score_result.raw_score,
        survivor_score_result.confidence,
        survivor_score_result.breakdown.survival,
        survivor_score_result.breakdown.survival_from_qedd,
        survivor_score_result.breakdown.survival_from_iwim,
        survivor_score_result.breakdown.survival_from_cluster,
        survivor_score_result.breakdown.momentum,
        survivor_score_result.breakdown.momentum_from_sobp,
        survivor_score_result.breakdown.momentum_from_qman,
        survivor_score_result.breakdown.momentum_from_chaos,
        survivor_score_result.breakdown.quality,
        survivor_score_result.breakdown.quality_from_mpcf,
        survivor_score_result.breakdown.quality_from_mesa,
        survivor_score_result.breakdown.quality_from_scr,
        survivor_score_result.breakdown.quality_from_wallets,
        survivor_score_result.breakdown.quality_from_ligma,
        survivor_score_result.breakdown.risk_discount,
        survivor_score_result.breakdown.risk_from_wash,
        survivor_score_result.breakdown.risk_from_exit,
        survivor_score_result.breakdown.risk_from_crash,
        survivor_score_result.breakdown.risk_from_anomaly,
        survivor_score_result.interpretation
    );

    // =================================================================
    // EARLY EXIT: SurvivorScore below critical threshold
    // =================================================================
    // Tokens with very low SurvivorScore are rejected immediately without
    // further processing. This saves computation and prevents bad tokens
    // from passing due to modifiers.
    if survivor_score_result.score < oracle.hyper_prediction_config.survivor_critical_threshold {
        info!(
            "🚫 EARLY SKIP: SurvivorScore {} < {} (critical threshold). Components: S={:.2} M={:.2} Q={:.2} R={:.2}",
            survivor_score_result.score,
            oracle.hyper_prediction_config.survivor_critical_threshold,
            survivor_score_result.breakdown.survival,
            survivor_score_result.breakdown.momentum,
            survivor_score_result.breakdown.quality,
            survivor_score_result.breakdown.risk_discount
        );

        return Ok(HyperPredictionResult {
            score: survivor_score_result.score,
            passed: false,
            risk_level: RiskLevel::VeryHigh,
            analysis_phase: if is_early_stage { AnalysisPhase::EarlyStage } else { AnalysisPhase::FullAnalysis },
            analysis_started_at: start,
            ssmi_result,
            mpcf_result,
            iwim_result,
            praecog_result,
            mesa_result,
            scr_score,
            ulvf_divergence,
            ulvf_curl,
            povc_cluster,
            shadow_progress: candidate.shadow_bonding_progress,
            shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
            base_score,
            processing_time_us: start.elapsed().as_micros() as u64,
            interpretation: format!(
                "🚫 SKIP | 📊 PATIENT | SurvivorScore: {} (CRITICAL FAIL) | S={:.0}% M={:.0}% Q={:.0}% | conf={:.0}%",
                survivor_score_result.score,
                survivor_score_result.breakdown.survival * 100.0,
                survivor_score_result.breakdown.momentum * 100.0,
                survivor_score_result.breakdown.quality * 100.0,
                survivor_score_result.confidence * 100.0
            ),
            chaos_result,
            resonance_result,
            gene_safety_result,
            hunter_score,
            qedd_result: Some(qedd_result),
            mci_result: Some(mci_result),
            qman_result: qman_result.clone(),
            ligma_result: ligma_result.clone(),
            cluster_result: cluster_result.clone(),
            paradox_state: paradox_state.clone(),
            should_delay_entry: false,
            recommended_delay_ms: 0,
            second_wave_result,
            survivor_score_result: Some(survivor_score_result),
            fractal_verdict: None,
            tcf_result: None,
            fallback_tracker: fallback_tracker.clone(),
        });
    }

    // Step 5: Combine scores (SurvivorScore is now the primary decision system)
    let survivor_result_ref = Some(survivor_score_result.clone());

    // Convert scoring::RiskLevel to verdict::RiskLevel for combine_scores
    let risk_level_verdict = match risk_level {
        crate::oracle::scoring::RiskLevel::Low => RiskLevel::Low,
        crate::oracle::scoring::RiskLevel::Medium => RiskLevel::Medium,
        crate::oracle::scoring::RiskLevel::High => RiskLevel::High,
        crate::oracle::scoring::RiskLevel::VeryHigh => RiskLevel::VeryHigh,
    };

    let (final_score, combined_risk, passed) = combine_scores_impl(
        oracle,
        base_score,
        &qass_result,
        &qedd_result,
        &mci_result,
        &ssmi_result,
        &mpcf_result,
        &iwim_result,
        scr_score,
        ulvf_divergence,
        ulvf_curl,
        povc_cluster,
        risk_level_verdict,
        &chaos_result,
        &resonance_result,
        &gene_safety_result,
        hunter_score,
        is_early_stage,
        cold_start_active,
        candidate,
        &cluster_result,
        &qman_result,
        tuned_weights.as_ref(),
        &mesa_result,
        &survivor_result_ref,
        &fallback_tracker,
    );

    // Step 5.5: Apply FRE Modifier
    // FRE provides independent organic quality scoring that can boost or penalize final score
    let (final_score, passed) = if let Some(ref verdict) = fractal_verdict {
        let mut modified_score = final_score as f32;

        match &verdict.action {
            FractalAction::Buy => {
                // Boost for organic quality (max +40 pts from 1.0 * 100 organic_score scaled to 0.4)
                let boost = (verdict.organic_score as f32 * 0.4);
                modified_score += boost;
                debug!(
                    "[FRE] BOOST: +{:.1} pts (organic_score={})",
                    boost, verdict.organic_score
                );
            }
            FractalAction::Watch(_) => {
                // Penalty for instability (20% reduction)
                modified_score *= 0.8;
                debug!("[FRE] PENALTY: -20% (unstable FSW)");
            }
            FractalAction::Skip(_) => {
                // Should have been caught by veto, but double-check
                modified_score = 0.0;
                warn!("[FRE] Late SKIP caught in scoring (should have been vetoed earlier)");
            }
        }

        // Clamp score to 0-100 range
        let clamped_score = modified_score.max(0.0).min(100.0) as u8;

        // Recalculate passed based on modified score
        let new_passed = passed && clamped_score >= oracle.threshold;

        debug!(
            "[FRE] Score adjustment: {} -> {} (passed: {} -> {})",
            final_score, clamped_score, passed, new_passed
        );

        (clamped_score, new_passed)
    } else {
        (final_score, passed)
    };

    // Step 6: Generate interpretation (with SurvivorScore prominently shown)
    let interpretation = generate_interpretation_impl(
        oracle,
        final_score,
        &qass_result,
        &qedd_result,
        &mci_result,
        &ssmi_result,
        &mpcf_result,
        &iwim_result,
        scr_score,
        povc_cluster,
        combined_risk,
        &chaos_result,
        &resonance_result,
        &gene_safety_result,
        hunter_score,
        is_early_stage,
        &paradox_state,
        should_delay_entry,
        recommended_delay_ms,
        &qman_result,
        &mesa_result,
        &survivor_result_ref,
        &fallback_tracker,
        passed,
        &fractal_verdict,
    );

    let processing_time_us = start.elapsed().as_micros() as u64;

    let mode_label = "📊 PATIENT";
    debug!(
        "{}: score={}, base={}, qass={:.2}, qedd_λ={:.3}, mci={:.3}, time={}μs",
        mode_label,
        final_score,
        base_score,
        qass_result.score,
        qedd_result.lambda_now,
        mci_result.mci,
        processing_time_us
    );

    Ok(HyperPredictionResult {
        score: final_score,
        passed, // Now determined by combine_scores based on SurvivorScore
        risk_level: combined_risk,
        analysis_phase: if is_early_stage {
            AnalysisPhase::EarlyStage
        } else {
            AnalysisPhase::FullAnalysis
        },
        analysis_started_at: start,
        ssmi_result,
        mpcf_result,
        iwim_result,
        praecog_result,
        mesa_result,
        scr_score,
        ulvf_divergence,
        ulvf_curl,
        povc_cluster,
        shadow_progress: candidate.shadow_bonding_progress,
        shadow_price_ratio: candidate.expected_price.map(|p| p / PUMP_INITIAL_PRICE),
        base_score,
        processing_time_us,
        interpretation,
        chaos_result,
        resonance_result,
        gene_safety_result,
        hunter_score,
        qedd_result: Some(qedd_result),
        mci_result: Some(mci_result),
        qman_result,
        ligma_result,
        cluster_result,
        paradox_state,
        should_delay_entry,
        recommended_delay_ms,
        second_wave_result,
        survivor_score_result: Some(survivor_score_result),
        fractal_verdict,
        tcf_result: None, // TCF is applied via ScoreHistory during multi-cycle observation
        fallback_tracker,
    })
}

/// Combine all scores into final result
///
/// ## SurvivorScore as Primary Decision System (PHASE 4.5)
///
/// SurvivorScore is now the **main scoring system**.
///
/// **Formula:**
/// ```text
/// 1. Start with SurvivorScore (or fallback to base_score if unavailable)
/// 3. Apply FallbackTracker confidence_multiplier
/// 4. Apply additional modifiers (Gene safety, Resonance, etc.)
/// 5. passed = survivor_result.passed AND final_score >= threshold
/// ```
///
/// **Risk Level** is based on SurvivorScore confidence:
/// - confidence < 0.5 → VeryHigh
/// - confidence < 0.7 → High
/// - score < 60 → Medium
/// - else → Low
pub(super) fn combine_scores_impl(
    oracle: &HyperPredictionOracle,

    base_score: u8,
    qass: &QASSResult,
    qedd: &QeddResult,
    mci: &MciResult,
    ssmi: &Option<SsmiResult>,
    mpcf: &Option<ActorInference>,
    iwim: &Option<crate::oracle::ultrafast::IwimResult>,
    scr: Option<f32>,
    ulvf_div: Option<f32>,
    ulvf_curl: Option<f32>,
    povc: Option<usize>,
    base_risk: RiskLevel,
    chaos: &Option<crate::chaos::engine::ChaosResult>,
    resonance: &Option<crate::signals::resonance::ResonanceResult>,
    gene_safety: &Option<crate::security::gene_mapper::GeneAnalysisResult>,
    hunter_score: Option<u8>,
    is_early_stage: bool,
    cold_start_active: bool,
    candidate: &EnhancedCandidate,
    cluster_result: &Option<ClusterAnalysis>,
    qman_result: &Option<QmanResult>,
    tuned_weights: Option<&TunableWeights>,
    mesa_result: &Option<MesaResult>,
    survivor_result: &Option<SurvivorScoreResult>,
    fallback_tracker: &crate::config::FallbackTracker,
) -> (u8, RiskLevel, bool) {
    // =================================================================
    // NEW: Delegate to scoring module
    // =================================================================
    // The new scoring module handles all penalties, boosters, and
    // uncapped scoring logic. This function remains as a thin wrapper
    // to maintain compatibility with existing code.

    // Use scoring weights from oracle (loaded from config)
    let scoring_weights = &oracle.scoring_weights;

    // Use new scoring module
    scoring::calculate_final_score(
        survivor_result,
        qass,
        ssmi,
        mpcf,
        iwim,
        scr,
        ulvf_div,
        ulvf_curl,
        povc,
        mesa_result,
        cluster_result,
        chaos,
        resonance,
        gene_safety,
        scoring_weights,
        &oracle.risk_thresholds,
        fallback_tracker,
        oracle.threshold,
        base_score,
        is_early_stage,
    )
}
/// Generate human-readable interpretation
///
/// ## SurvivorScore-First Interpretation (PHASE 4.5)
///
/// Shows SurvivorScore prominently as the primary decision indicator.
/// Other signals (QASS, MESA, etc.) are shown as secondary information.
///
/// Uses unified "PATIENT OBSERVER" mode indicator: `"📊 PATIENT | ..."`
pub(super) fn generate_interpretation_impl(
    oracle: &HyperPredictionOracle,

    score: u8,
    qass: &QASSResult,
    qedd: &QeddResult,
    mci: &MciResult,
    ssmi: &Option<SsmiResult>,
    mpcf: &Option<ActorInference>,
    iwim: &Option<crate::oracle::ultrafast::IwimResult>,
    scr: Option<f32>,
    povc: Option<usize>,
    risk: RiskLevel,
    chaos: &Option<crate::chaos::engine::ChaosResult>,
    resonance: &Option<crate::signals::resonance::ResonanceResult>,
    gene_safety: &Option<crate::security::gene_mapper::GeneAnalysisResult>,
    hunter_score: Option<u8>,
    is_early_stage: bool,
    paradox_state: &Option<ParadoxState>,
    should_delay_entry: bool,
    recommended_delay_ms: u64,
    qman_result: &Option<QmanResult>,
    mesa_result: &Option<MesaResult>,
    survivor_result: &Option<SurvivorScoreResult>,
    fallback_tracker: &crate::config::FallbackTracker,
    passed: bool,
    fractal_verdict: &Option<FractalVerdict>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Mode indicator - includes analysis phase for better observability
    // This makes the implicit phase (is_early_stage flag) explicit in the output
    let phase_emoji = if is_early_stage {
        AnalysisPhase::EarlyStage.emoji()
    } else {
        AnalysisPhase::FullAnalysis.emoji()
    };
    let phase_name = if is_early_stage {
        AnalysisPhase::EarlyStage.display_name()
    } else {
        AnalysisPhase::FullAnalysis.display_name()
    };
    let mode_prefix = format!("📊 PATIENT {} {}", phase_emoji, phase_name);

    // Main decision based on passed (which is now SurvivorScore-based)
    let (decision_icon, decision_text) = if passed {
        ("✅", "BUY")
    } else {
        ("❌", "SKIP")
    };

    // First: Decision + Mode + Final Score
    parts.push(format!(
        "{} {} | {} | Final: {}",
        decision_icon, decision_text, mode_prefix, score
    ));

    // === SURVIVOR SCORE (PRIMARY) - Show first ===
    if let Some(ref survivor) = survivor_result {
        let survivor_status = if survivor.passed { "PASS" } else { "FAIL" };
        parts.push(format!(
            "SURVIVOR: {} ({}) | S={:.0}% M={:.0}% Q={:.0}% R={:.0}% | conf={:.0}%",
            survivor.score,
            survivor_status,
            survivor.breakdown.survival * 100.0,
            survivor.breakdown.momentum * 100.0,
            survivor.breakdown.quality * 100.0,
            survivor.breakdown.risk_discount * 100.0,
            survivor.confidence * 100.0
        ));
    }

    // === FALLBACK PENALTY (if used) ===
    if fallback_tracker.any_used() {
        let penalty_pct = fallback_tracker.cumulative_penalty * 100.0;
        let types: Vec<&str> = fallback_tracker
            .used_fallbacks
            .iter()
            .map(|f| match f {
                crate::config::FallbackType::UlvfTxCounts => "ULVF",
                crate::config::FallbackType::UlvfDivergence => "ULVF_D",
                crate::config::FallbackType::PovcCluster => "POVC",
                crate::config::FallbackType::ScrScore => "SCR",
                crate::config::FallbackType::SsmiScore => "SSMI",
            })
            .collect();
        parts.push(format!(
            "⚠️ Fallback: -{:.0}% [{}]",
            penalty_pct,
            types.join(",")
        ));
    }

    // === QASS (SECONDARY) ===
    if qass.is_valid {
        parts.push(format!(
            "QASS: {:.0}% (conf={:.0}%)",
            qass.score * 100.0,
            qass.confidence * 100.0
        ));
    }

    // === MESA (if in full analysis mode and notable) ===
    if !is_early_stage {
        if let Some(ref mesa) = mesa_result {
            let wash_threshold = oracle
                .hyper_prediction_config
                .survivor_thresholds
                .mesa_wash_elevated;
            let bot_threshold = oracle
                .hyper_prediction_config
                .orchestrator_thresholds
                .mesa_interpretation_bot_threshold;
            let organic_threshold = oracle
                .hyper_prediction_config
                .orchestrator_thresholds
                .mesa_interpretation_organic_threshold;

            if mesa.wash_likeness > wash_threshold {
                parts.push(format!("🔄 Wash:{:.0}%", mesa.wash_likeness * 100.0));
            }
            if mesa.bot_likeness > bot_threshold {
                parts.push(format!("🤖 Bot:{:.0}%", mesa.bot_likeness * 100.0));
            }
            if mesa.organic_likeness > organic_threshold {
                parts.push(format!("👥 Organic:{:.0}%", mesa.organic_likeness * 100.0));
            }
        }
    }

    // IWIM dev-wallet intent analysis (BOTH MODES)
    if let Some(iwim_res) = iwim {
        if iwim_res.rug_threat_score > 0.6 {
            parts.push(format!("⚠️ IWIM Rug: {:.2}", iwim_res.rug_threat_score));
        }
        if iwim_res.organic_score > 0.5 {
            parts.push(format!("✓ IWIM Organic: {:.2}", iwim_res.organic_score));
        }
        if iwim_res.sybil_score > 0.5 {
            parts.push(format!("⚠️ IWIM Sybil: {:.2}", iwim_res.sybil_score));
        }
    }

    // SCR bot detection (Full analysis ONLY - hide in early stage to avoid confusion)
    if !is_early_stage {
        if let Some(scr_val) = scr {
            if scr_val > 0.7 {
                parts.push(format!("⚠️ SCR: {:.2} (high bot)", scr_val));
            }
        }
    }

    // POVC cluster (Full analysis ONLY - hide in early stage to avoid confusion)
    if !is_early_stage {
        if let Some(cluster) = povc {
            let cluster_name = match cluster {
                0 => "Dump",
                1 => "Hype",
                2 => "Noise",
                _ => "Unknown",
            };
            parts.push(format!("POVC: {}", cluster_name));
        }
    }

    // === QEDD and MCI interpretations (Full analysis ONLY - hide in early stage) ===
    if !is_early_stage {
        // QEDD lambda and survival
        if qedd.lambda_now > 0.8 {
            parts.push(format!("⚠️ QEDD λ={:.3} (high decay)", qedd.lambda_now));
        } else if qedd.lambda_now > 0.6 {
            parts.push(format!("QEDD λ={:.3}", qedd.lambda_now));
        }

        // Show survival probabilities if interesting
        if qedd.survival_1s < 0.5 {
            parts.push(format!("S(1s)={:.2}", qedd.survival_1s));
        }

        // MCI coherence
        if mci.mci > 0.7 {
            parts.push(format!("✓ MCI={:.2} (high coherence)", mci.mci));
        } else if mci.mci < 0.4 {
            parts.push(format!("⚠️ MCI={:.2} (low coherence)", mci.mci));
        } else {
            parts.push(format!("MCI={:.2}", mci.mci));
        }
    }

    // === TASK 4.1: Add new signal interpretations ===

    // Chaos Engine results
    if let Some(chaos_res) = chaos {
        if chaos_res.crash_probability > 30.0 {
            parts.push(format!(
                "⚠️ Chaos Crash: {:.1}%",
                chaos_res.crash_probability
            ));
        }
        if chaos_res.pump_probability > 40.0 {
            parts.push(format!("📈 Chaos Pump: {:.1}%", chaos_res.pump_probability));
        }
        if chaos_res.median_roi.abs() > 5.0 {
            let emoji = if chaos_res.median_roi > 0.0 { "+" } else { "" };
            parts.push(format!("Chaos ROI: {}{:.1}%", emoji, chaos_res.median_roi));
        }
    }

    // Resonance Detector results
    if let Some(res_res) = resonance {
        match res_res.classification {
            crate::signals::resonance::ActivityClassification::BotLikely => {
                parts.push(format!(
                    "🤖 Resonance: BOT (CV={:.2})",
                    res_res.coefficient_variation
                ));
            }
            crate::signals::resonance::ActivityClassification::Suspicious => {
                parts.push(format!(
                    "⚠️ Resonance: Suspicious (CV={:.2})",
                    res_res.coefficient_variation
                ));
            }
            crate::signals::resonance::ActivityClassification::HumanLikely => {
                parts.push(format!(
                    "✓ Resonance: Human (CV={:.2})",
                    res_res.coefficient_variation
                ));
            }
            _ => {}
        }
    }

    // Gene Mapper security analysis
    if let Some(gene_res) = gene_safety {
        match gene_res.risk_level {
            crate::security::gene_mapper::RiskLevel::Critical => {
                parts.push(format!(
                    "🚨 Gene: CRITICAL ({:.2}) - {}",
                    gene_res.risk_score, gene_res.threat_summary
                ));
            }
            crate::security::gene_mapper::RiskLevel::High => {
                parts.push(format!("⛔ Gene: HIGH RISK ({:.2})", gene_res.risk_score));
            }
            crate::security::gene_mapper::RiskLevel::Medium => {
                parts.push(format!("⚠️ Gene: Medium ({:.2})", gene_res.risk_score));
            }
            crate::security::gene_mapper::RiskLevel::Low => {
                parts.push(format!("Gene: Low Risk ({:.2})", gene_res.risk_score));
            }
            crate::security::gene_mapper::RiskLevel::Safe => {
                parts.push("✓ Gene: Safe".to_string());
            }
        }
    }

    // Hunter score (external oracle)
    if let Some(hunter) = hunter_score {
        if hunter >= 80 {
            parts.push(format!("🎯 Hunter: {}", hunter));
        } else if hunter >= 60 {
            parts.push(format!("Hunter: {}", hunter));
        } else if hunter < 40 {
            parts.push(format!("⚠️ Hunter: {} (low)", hunter));
        }
    }

    // === QMAN capital flow analysis ===
    if let Some(ref qman) = qman_result {
        // Show trading signal
        match qman.signal {
            TradingSignal::AllInMainTrend => {
                parts.push(format!(
                    "💰 QMAN: HyperBubble ({:.0}%)",
                    qman.qman_score * 100.0
                ));
            }
            TradingSignal::PrepareSecondWave => {
                parts.push(format!(
                    "📈 QMAN: SecondWave ({:.0}%)",
                    qman.qman_score * 100.0
                ));
            }
            TradingSignal::ExitNow => {
                parts.push(format!(
                    "⚠️ QMAN: Smart$ Exit ({:.0}%)",
                    qman.qman_score * 100.0
                ));
            }
            TradingSignal::Hold => {
                // Only show if score is notable
                if qman.qman_score > 0.7 || qman.qman_score < 0.3 {
                    let emoji = if qman.qman_score > 0.7 {
                        "📈"
                    } else {
                        "📉"
                    };
                    parts.push(format!("{} QMAN: {:.0}%", emoji, qman.qman_score * 100.0));
                }
            }
        }

        // Show high energy wallets if significant
        if qman.high_energy_wallets >= 2 {
            let flow_dir = if qman.net_energy_flow > 0.0 {
                "↑"
            } else {
                "↓"
            };
            parts.push(format!("💎 {}HEW {}", qman.high_energy_wallets, flow_dir));
        }
    }

    // === FRE (Fractal Resonance Engine) ===
    if let Some(ref fre) = fractal_verdict {
        match &fre.action {
            FractalAction::Buy => {
                parts.push(format!(
                    "🟢 FRE: Organic {} | H={:.2} Coh={:.2}",
                    fre.organic_score, fre.hurst_global, fre.coherence
                ));
            }
            FractalAction::Watch(reason) => {
                parts.push(format!(
                    "🟡 FRE: Watch ({}) | σ={:.3}",
                    reason, fre.stability_sigma
                ));
            }
            FractalAction::Skip(reason) => {
                parts.push(format!("🔴 FRE: SKIP ({})", reason));
            }
        }
    }

    // === ParadoxSensor network telemetry ===
    if let Some(paradox) = paradox_state {
        if paradox.phase_sync > 0.7 {
            parts.push(format!(
                "🤖 HFT: sync={:.0}% tension={:.0}",
                paradox.phase_sync * 100.0,
                paradox.tension
            ));
        }
        if paradox.is_echo_spike {
            parts.push("⚡ Echo Spike".to_string());
        }
    }

    // Delay recommendation
    if should_delay_entry {
        parts.push(format!("⏳ DELAY: {}ms", recommended_delay_ms));
    }

    parts.join(" | ")
}
/// Build MarketSignals from available data for QEDD and MCI computation
///
/// TASK 3: Helper function to construct market signals
///
/// Now uses real transaction metrics when available to compute accurate
/// SOBP, flow, and resonance signals instead of placeholder values.
pub(super) fn build_market_signals_impl(
    oracle: &HyperPredictionOracle,

    candidate: &EnhancedCandidate,
    ssmi_result: &Option<SsmiResult>,
    mpcf_result: &Option<ActorInference>,
    _scr_score: Option<f32>,
    ulvf_divergence: Option<f32>,
    ulvf_curl: Option<f32>,
    resonance_result: &Option<crate::signals::resonance::ResonanceResult>,
    snapshot_price: Option<f64>,
    tx_metrics: Option<&TransactionMetrics>,
    cold_start: bool,
) -> MarketSignals {
    use crate::signals::market_signals::*;

    // Minimum SOBP current to avoid zero division in calculations
    const MIN_SOBP_CURRENT: f64 = 0.5;
    // Minimum SOBP MA to ensure meaningful comparisons
    const MIN_SOBP_MA: f64 = 0.4;

    // Extract SOBP signals from candidate bonding curve progress AND real tx metrics
    let sobp_current = candidate.bonding_curve_progress.unwrap_or(0.0) as f64;

    // Calculate SOBP drop from real metrics if available
    // High sell pressure (low buy_pressure_ratio) indicates potential drop
    let sobp_drop = if let Some(metrics) = tx_metrics {
        let buy_pressure = metrics.buy_pressure_ratio();
        // If more sells than buys, this indicates dropping pressure
        // buy_pressure 0.3 (30% buys) → drop = 0.7
        // buy_pressure 0.7 (70% buys) → drop = 0.0
        (1.0 - buy_pressure * 1.43).clamp(0.0, 1.0)
    } else {
        0.3 // Conservative default: assume some drop risk when no data
    };

    // Build SOBP signals with safe minimum values
    let sobp = SobpSignals {
        current: sobp_current.max(MIN_SOBP_CURRENT),
        drop: sobp_drop,
        ma: sobp_current.max(MIN_SOBP_MA),
    };

    // Build flow signals from ULVF data and real metrics (QASS alignment removed)
    // Use neutral alignment since QASS is deprecated
    let flow_alignment = 0.0; // Neutral alignment

    // Calculate outflow from real metrics if available
    let outflow = if let Some(metrics) = tx_metrics {
        // High sell volume relative to buy volume indicates outflow
        metrics.sell_volume_sol / (metrics.total_volume_sol.max(0.001))
    } else {
        // Fall back to ULVF curl-based estimate
        // BUGFIX: Clamp curl before conversion to prevent QEDD lambda explosion
        // When ULVF curl overflows (e.g., 9.9B), it causes gigantic outflow values
        ulvf_curl
            .map(|c| {
                let clamped_curl = c.clamp(-10.0, 10.0); // Reasonable curl range
                (clamped_curl / 20.0).clamp(0.0, 1.0) as f64 // Normalize to [0, 1] and convert to f64
            })
            .unwrap_or(0.4)
    };

    // BUGFIX: Additional defensive clamp on outflow itself
    // This protects QEDD from any upstream calculation errors
    let outflow = outflow.clamp(0.0, 1.0);

    // Flow magnitude: derive from early volume footprint (QASS confidence removed)
    let mut flow_magnitude: f64 = 0.5; // Start with neutral
    if let Some(metrics) = tx_metrics {
        let volume_scale = oracle
            .normalization_config
            .volume_scale
            .max(oracle.hyper_prediction_config.min_volume_scale);
        let volume_factor = (metrics.total_volume_sol / volume_scale).clamp(0.0, 1.2);
        let liquidity_ref = candidate.initial_liquidity_sol.max(0.1);
        let compute_burst_factor = |metrics: &TransactionMetrics,
                                    liquidity_ref: f64,
                                    volume_scale: f64,
                                    relative_cap: f64,
                                    burst_norm: f64| {
            let base_burst = (metrics.max_tx_sol / volume_scale).clamp(0.0, 1.0);
            let relative_burst =
                (metrics.max_tx_sol / liquidity_ref).clamp(0.0, relative_cap) / burst_norm;
            let relative_volume =
                (metrics.total_volume_sol / liquidity_ref).clamp(0.0, relative_cap) / burst_norm;
            base_burst.max(relative_burst).max(relative_volume)
        };
        let burst_factor = compute_burst_factor(
            metrics,
            liquidity_ref,
            volume_scale,
            oracle.hyper_prediction_config.relative_factor_cap,
            oracle.hyper_prediction_config.burst_normalization,
        );
        let wallet_factor = if metrics.tx_count > 0 {
            (metrics.unique_addrs as f64 / metrics.tx_count as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        flow_magnitude = flow_magnitude.max(volume_factor).max(wallet_factor);

        if cold_start {
            flow_magnitude = flow_magnitude.max(burst_factor);
        }

        flow_magnitude = flow_magnitude.clamp(0.0, 1.0);
    }

    let flow = FlowSignals {
        outflow,
        qass_alignment: flow_alignment, // Use neutral alignment since QASS is deprecated
        magnitude: flow_magnitude,
    };

    // Build resonance signals from real metrics or resonance_result
    let resonance = if let Some(metrics) = tx_metrics {
        // Use real interval CV for bot detection
        let cv = metrics.interval_cv();
        // Low CV suggests bot activity (regular intervals)
        let risk = if cv < 0.25 {
            0.8 // High risk - very regular, bot-like
        } else if cv < 0.4 {
            0.5 // Medium risk - somewhat regular
        } else {
            0.2 // Low risk - irregular, human-like
        };
        ResonanceSignals {
            risk,
            cv,
            sample_count: metrics.tx_count,
        }
    } else if let Some(res) = resonance_result {
        ResonanceSignals {
            risk: res.resonance_score.clamp(0.0, 1.0),
            cv: res.coefficient_variation.clamp(0.0, 1.0),
            sample_count: res.sample_count,
        }
    } else {
        ResonanceSignals {
            risk: 0.5,
            cv: 0.5,
            sample_count: 0,
        }
    };

    // Build deviation signals (use ULVF divergence as proxy)
    let deviation_risk = ulvf_divergence
        .map(|d| 1.0 - d.clamp(0.0, 1.0))
        .unwrap_or(0.5);
    let deviation = DeviationSignals {
        risk: deviation_risk as f64,
        coherence_loss: deviation_risk as f64 * 0.8,
        anomaly_magnitude: deviation_risk as f64 * 0.6,
    };

    // Build entropy signals
    let ssmi_entropy = ssmi_result
        .as_ref()
        .map(|s| s.ssmi_score.clamp(0.0, 1.0) as f64)
        .unwrap_or(0.5);
    let mpcf_entropy = mpcf_result
        .as_ref()
        .map(|m| m.confidence as f64)
        .unwrap_or(0.5);

    let entropy = EntropySignals {
        ssmi: ssmi_entropy,
        mpcf: mpcf_entropy,
        combined: (ssmi_entropy + mpcf_entropy) / 2.0,
    };

    // Volume signals: prefer live metrics for cold start inference
    let volume_signals = if let Some(metrics) = tx_metrics {
        let current = metrics.total_volume_sol.max(metrics.max_tx_sol);
        let ma = (metrics.total_volume_sol * 0.8).max(current * 0.5);
        let std_dev = if metrics.volumes_sol.is_empty() {
            metrics.max_tx_sol
        } else {
            let volume_len = metrics.volumes_sol.len().max(1) as f64;
            let sum: f64 = metrics.volumes_sol.iter().sum();
            let mean = sum / volume_len;
            let variance = metrics
                .volumes_sol
                .iter()
                .map(|v| (v - mean).powi(2))
                .sum::<f64>()
                / volume_len;
            variance.sqrt()
        };
        VolumeSignals {
            current,
            ma,
            std_dev,
        }
    } else {
        VolumeSignals {
            current: candidate.initial_liquidity_sol * 10000.0,
            ma: candidate.initial_liquidity_sol * 9000.0,
            std_dev: candidate.initial_liquidity_sol * 1000.0,
        }
    };

    // Create placeholder values for price, orderbook, time
    MarketSignals {
        volume: volume_signals,
        price: PriceSignals {
            current: snapshot_price.unwrap_or(PUMP_INITIAL_PRICE),
            momentum: if let Some(metrics) = tx_metrics {
                let buy_pressure = metrics.volume_buy_pressure();
                ((buy_pressure - 0.5) * 1.2).clamp(-1.0, 1.0)
            } else if sobp_current > 0.5 {
                0.3
            } else {
                -0.1
            },
            volatility: 0.2,
            valid: true,
        },
        orderbook: OrderbookSignals {
            spread: 0.01,
            depth: candidate.initial_liquidity_sol * 5000.0,
            imbalance: 0.5 + (flow_alignment * 0.3), // Use neutral alignment since QASS is deprecated
        },
        time: TimeSignals {
            timestamp_ms: candidate.timestamp,
            time_since_last_trade_ms: tx_metrics.map(|m| m.avg_interval_ms as u64).unwrap_or(100),
        },
        sobp,
        flow,
        resonance,
        deviation,
        entropy,
    }
}

/// Calculate overall QMAN score from signal result and state
///
/// The QMAN score indicates smart money flow direction:
/// - 0.0-0.3: Smart money exiting (bearish)
/// - 0.3-0.6: Neutral / unclear
/// - 0.6-1.0: Smart money accumulating (bullish)
/// Calculate overall QMAN score from signal result and state
///
/// The QMAN score indicates smart money flow direction:
/// - 0.0-0.3: Smart money exiting (bearish)
/// - 0.3-0.6: Neutral / unclear
/// - 0.6-1.0: Smart money accumulating (bullish)
pub(super) fn calculate_qman_score_impl(
    oracle: &HyperPredictionOracle,

    signal_result: &QmanSignalResult,
    state: &crate::oracle::wallet_energy_tracker::StateVector,
    prediction: &crate::oracle::qman::PredictionResult,
) -> f32 {
    let mut score = 0.5; // Neutral baseline

    // Signal contribution
    match signal_result.signal {
        TradingSignal::AllInMainTrend => score += 0.3, // Hyper-bubble = very bullish
        TradingSignal::PrepareSecondWave => score += 0.2, // Re-accumulation = bullish
        TradingSignal::Hold => {}                      // Neutral
        TradingSignal::ExitNow => score -= 0.25,       // Capital drain = bearish
    }

    // Energy flow contribution
    // Positive net flow = capital entering = bullish
    let net_flow: f64 = prediction
        .top_flows
        .iter()
        .map(|(_, _, change)| *change)
        .sum();

    if state.total_energy > 0.1 {
        let flow_ratio = net_flow / state.total_energy;
        if flow_ratio > 0.1 {
            score += 0.15; // Significant inflow
        } else if flow_ratio < -0.1 {
            score -= 0.15; // Significant outflow
        }
    }

    // Confidence from prediction quality affects score magnitude
    let confidence_factor = signal_result.confidence.clamp(0.3, 1.0);
    score = 0.5 + (score - 0.5) * confidence_factor;

    score.clamp(0.0, 1.0)
}

/// Calculate QMAN confidence based on data quality
///
/// Higher confidence when:
/// - More active wallets are being tracked
/// - Higher total energy (more capital observed)
/// - More transitions recorded
pub(super) fn calculate_qman_confidence_impl(
    oracle: &HyperPredictionOracle,

    state: &crate::oracle::wallet_energy_tracker::StateVector,
) -> f32 {
    let mut confidence = 0.3; // Base confidence

    // More wallets = higher confidence (up to +0.3 at 20 wallets)
    let wallet_factor = (state.active_wallets as f32 / 20.0).min(1.0) * 0.3;
    confidence += wallet_factor;

    // More energy = more data to analyze (up to +0.2 at 10 SOL equivalent)
    let energy_factor = (state.total_energy as f32 / 10.0).min(1.0) * 0.2;
    confidence += energy_factor;

    // More transitions = better prediction (up to +0.2 at 10 transitions)
    let transition_count = oracle.transition_matrix.transition_count();
    let transition_factor = (transition_count as f32 / 10.0).min(1.0) * 0.2;
    confidence += transition_factor;

    confidence.clamp(0.0, 1.0)
}

/// Convert HyperPredictionResult to ScoredCandidate for compatibility
pub(super) fn to_scored_candidate_impl(
    _oracle: &HyperPredictionOracle,
    result: &HyperPredictionResult,
    candidate: &EnhancedCandidate,
) -> ScoredCandidate {
    // Convert verdict::RiskLevel back to scoring::RiskLevel for ScoredCandidate
    let risk_level_scoring = match result.risk_level {
        RiskLevel::Low => crate::oracle::scoring::RiskLevel::Low,
        RiskLevel::Medium => crate::oracle::scoring::RiskLevel::Medium,
        RiskLevel::High => crate::oracle::scoring::RiskLevel::High,
        RiskLevel::VeryHigh => crate::oracle::scoring::RiskLevel::VeryHigh,
    };

    ScoredCandidate {
        pool: convert_enhanced_to_candidate_pool(candidate),
        score: result.score,
        passed: result.passed,
        risk_level: risk_level_scoring,
        confidence: None,
    }
}

// =============================================================================
// TCF Integration Helpers
// =============================================================================

/// Build a TCF MarketObservation from available signals in the current scoring cycle.
///
/// This function extracts relevant signals from the HyperPrediction pipeline and
/// normalizes them into the TCF observation format.
///
/// # Arguments
///
/// * `survivor_result` - SurvivorScore result (quality metrics)
/// * `mpcf_result` - MPCF actor inference (confidence, actor type for order flow)
/// * `resonance_result` - Resonance detection (interval CV for jitter)
/// * `paradox_state` - ParadoxSensor state (phase_sync)
/// * `tx_metrics` - Transaction metrics (tx_count for volume proxy)
/// * `ligma_result` - LIGMA result (tradability, psi_ligma for order flow)
/// * `prev_price` - Previous price for delta calculation (optional)
/// * `curr_price` - Current price for delta calculation (optional)
///
/// # Returns
///
/// A normalized `MarketObservation` for TCF processing.
pub fn build_tcf_observation(
    survivor_result: Option<&SurvivorScoreResult>,
    mpcf_result: Option<&ActorInference>,
    resonance_result: Option<&crate::signals::resonance::ResonanceResult>,
    paradox_state: Option<&ParadoxState>,
    tx_metrics: Option<&TransactionMetrics>,
    ligma_result: Option<&LigmaResult>,
    prev_price: Option<f64>,
    curr_price: Option<f64>,
) -> MarketObservation {
    // Price delta: normalized price change [-1, 1]
    let price_delta = match (prev_price, curr_price) {
        (Some(prev), Some(curr)) if prev > 0.0 => {
            let change_pct = (curr - prev) / prev;
            // Normalize assuming max expected change is ±30% per cycle
            (change_pct / 0.30).clamp(-1.0, 1.0)
        }
        _ => 0.0, // No price change data
    };

    // Volume delta: from tx_metrics tx_count relative to expected baseline
    // Normalize tx_count assuming 15-50 txs is typical per cycle
    let volume_delta = tx_metrics
        .map(|m| {
            // Center around 25 txs, scale to [-1, 1]
            // < 10 txs → -1.0, 25 txs → 0.0, > 40 txs → 1.0
            let centered = (m.tx_count as f64 - 25.0) / 15.0;
            centered.clamp(-1.0, 1.0)
        })
        .or_else(|| {
            // Fallback: use momentum from survivor if no tx_metrics
            survivor_result.map(|s| ((s.breakdown.momentum - 1.0) as f64).clamp(-1.0, 1.0))
        })
        .unwrap_or(0.0);

    // Liquidity entropy: from LIGMA tradability_score
    let liquidity_entropy = ligma_result
        .map(|l| l.tradability_score.clamp(0.0, 1.0))
        .unwrap_or(0.5);

    // Order flow imbalance: from LIGMA psi_ligma (tradability - trap - sniper)
    // psi_ligma is already in [-1, 1] range where positive = healthy order flow
    let order_flow_imbalance = ligma_result
        .map(|l| l.psi_ligma.clamp(-1.0, 1.0))
        .or_else(|| {
            // Fallback: derive from MPCF - human actors suggest organic buy flow
            mpcf_result.map(|m| {
                match m.actor {
                    ActorType::HumanMobile | ActorType::HumanDesktop => 0.3, // Positive for human activity
                    ActorType::SniperScript | ActorType::MEVArb => -0.2, // Negative for aggressive bots
                    ActorType::SybilBot => -0.5, // Strong negative for sybil
                    ActorType::LiquidityBot | ActorType::RPCFiller => 0.0, // Neutral for market makers
                    ActorType::Unknown => 0.0,                             // Neutral for unknown
                }
            })
        })
        .unwrap_or(0.0);

    // MPCF confidence [0, 1]
    let mpcf = mpcf_result.map(|m| m.confidence as f64).unwrap_or(0.5);

    // Jitter: from Resonance coefficient_variation (higher CV = more human-like)
    let jitter = resonance_result
        .map(|r| r.coefficient_variation.clamp(0.0, 1.0))
        .unwrap_or(0.5);

    // Phase sync: from ParadoxSensor (HFT synchronization)
    let phase_sync = paradox_state
        .map(|p| p.phase_sync.clamp(0.0, 1.0))
        .unwrap_or(0.2);

    MarketObservation::new(
        price_delta,
        volume_delta,
        liquidity_entropy,
        order_flow_imbalance,
        mpcf,
        jitter,
        phase_sync,
    )
}

/// Compute TCF result for Final Verdict from a TrendCohesionField instance.
///
/// This function extracts the TCF diagnostics and converts them to a `TcfResult`
/// that can be included in `HyperPredictionResult`.
///
/// # Arguments
///
/// * `tcf` - The TrendCohesionField instance that has been updated with observations
/// * `tcf_config` - TCF configuration for modulation calculation
///
/// # Returns
///
/// A `TcfResult` containing the final TCF score and diagnostics.
pub fn compute_tcf_result(
    tcf: &TrendCohesionField,
    tcf_config: &crate::config::TcfConfig,
) -> TcfResult {
    let start = std::time::Instant::now();

    let diagnostics = tcf.get_diagnostics();
    let tcf_score = tcf.get_tcf_score();

    // Calculate modulation factor: min + range * tcf_score
    let modulation_factor =
        tcf_config.tcf_min_modulation + tcf_config.tcf_modulation_range * tcf_score;

    // Keep semantics explicit: fresh compute vs cached/default fallback.
    let (
        latest_cohesion,
        latest_cohesion_computed_this_cycle,
        latest_cohesion_is_fallback,
        latest_cohesion_fallback_reason,
    ) = if let Some(computed) = diagnostics.latest_computed_cohesion_this_cycle {
        (computed, true, false, None)
    } else if let Some(cached) = diagnostics.cached_last_known_cohesion {
        (cached, false, true, Some("cached_previous_cycle"))
    } else {
        (0.5, false, true, Some("neutral_default_no_history"))
    };

    // Calculate average cohesion from recent values
    let avg_cohesion = if diagnostics.recent_cohesions.is_empty() {
        0.5
    } else {
        diagnostics.recent_cohesions.iter().sum::<f64>() / diagnostics.recent_cohesions.len() as f64
    };

    // Determine phase from trend direction and cohesion patterns
    let phase = determine_tcf_phase(&diagnostics);

    TcfResult {
        tcf_score,
        is_primed: diagnostics.is_primed,
        observation_count: diagnostics.update_count,
        phase,
        cliff_detected: diagnostics.cliff_detected,
        latest_cohesion,
        latest_cohesion_computed_this_cycle,
        latest_cohesion_is_fallback,
        latest_cohesion_fallback_reason,
        avg_cohesion,
        trend_direction: diagnostics.trend_direction,
        modulation_factor,
        analysis_time_us: start.elapsed().as_micros() as u64,
    }
}

/// Determine TCF phase from diagnostics
fn determine_tcf_phase(diagnostics: &TcfDiagnostics) -> TcfPhase {
    // Cold start if not primed
    if !diagnostics.is_primed {
        return TcfPhase::ColdStart;
    }

    // Get average cohesion for phase determination
    let avg_cohesion = if diagnostics.recent_cohesions.is_empty() {
        0.5
    } else {
        diagnostics.recent_cohesions.iter().sum::<f64>() / diagnostics.recent_cohesions.len() as f64
    };

    // Determine phase based on trend direction, cohesion, and cliff detection
    // First check cliff conditions
    if diagnostics.cliff_detected {
        if diagnostics.trend_direction == -1 {
            return TcfPhase::Dump;
        }
        return TcfPhase::Chaos;
    }

    // Then check cohesion-based conditions
    if avg_cohesion < 0.2 {
        return TcfPhase::Chaos;
    }

    if diagnostics.trend_direction == -1 && avg_cohesion < 0.3 {
        return TcfPhase::Dump;
    }

    if diagnostics.trend_direction == 1 && avg_cohesion > 0.6 {
        return TcfPhase::OrganicGrowth;
    }

    if diagnostics.data_moved && diagnostics.trend_direction == 1 && avg_cohesion > 0.3 {
        return TcfPhase::Pump;
    }

    if diagnostics.trend_direction == 0 && avg_cohesion > 0.4 {
        return TcfPhase::Stable;
    }

    // Default to stable if nothing else matches
    TcfPhase::Stable
}

/// Apply TCF modulation to a momentum score during Final Verdict.
///
/// This function applies the TCF modulation formula to the momentum component
/// of the SurvivorScore. It should be called ONLY during Final Verdict (after S13).
///
/// Formula: `effective_momentum = base_momentum * modulation_factor`
/// Where: `modulation_factor = tcf_min_modulation + tcf_modulation_range * tcf_score`
///
/// # Arguments
///
/// * `base_momentum` - The base momentum score from SurvivorScore
/// * `tcf_result` - The TCF result containing modulation factor
/// * `tcf_config` - TCF configuration (for logging)
///
/// # Returns
///
/// The modulated momentum score.
pub fn apply_tcf_modulation(
    base_momentum: f64,
    tcf_result: &TcfResult,
    tcf_config: &crate::config::TcfConfig,
) -> f64 {
    if !tcf_config.enabled {
        return base_momentum;
    }

    let effective_momentum = base_momentum * tcf_result.modulation_factor;

    debug!(
        "TCF MODULATION: base={:.1} × factor={:.3} (tcf={:.3}) → effective={:.1}",
        base_momentum, tcf_result.modulation_factor, tcf_result.tcf_score, effective_momentum
    );

    effective_momentum
}

/// Generate TCF interpretation string for logging.
///
/// Creates a human-readable description of the TCF state for inclusion in
/// the overall interpretation.
///
/// # Arguments
///
/// * `tcf_result` - The TCF result to interpret
///
/// # Returns
///
/// A formatted string describing the TCF state.
pub fn interpret_tcf_result(tcf_result: &TcfResult) -> String {
    if !tcf_result.is_primed {
        return format!(
            "🌀 TCF: Cold start ({} obs), building baseline",
            tcf_result.observation_count
        );
    }

    let phase_emoji = match tcf_result.phase {
        TcfPhase::ColdStart => "❄️",
        TcfPhase::Stable => "⚖️",
        TcfPhase::OrganicGrowth => "🌱",
        TcfPhase::Pump => "🚀",
        TcfPhase::Dump => "💥",
        TcfPhase::Chaos => "🌪️",
    };

    let trend_emoji = match tcf_result.trend_direction {
        1 => "📈",
        -1 => "📉",
        _ => "➡️",
    };

    let cliff_warning = if tcf_result.cliff_detected {
        " ⚠️CLIFF"
    } else {
        ""
    };

    format!(
        "🌀 TCF: {:.0}% {}{} {} | cohesion={:.0}% | mod={:.2}{}",
        tcf_result.tcf_score * 100.0,
        phase_emoji,
        tcf_result.phase.name(),
        trend_emoji,
        tcf_result.avg_cohesion * 100.0,
        tcf_result.modulation_factor,
        cliff_warning,
    )
}
