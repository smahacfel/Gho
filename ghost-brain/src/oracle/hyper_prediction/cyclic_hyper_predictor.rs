//! Cyclic HyperPrediction Orchestrator
//!
//! Implements the 12-cycle "Ghost Predator" strategy for high-precision
//! candidate scoring with phase-aware analysis.
//!
//! # Strategy
//! - Runs 12 cycles (S1-S12) at 400ms intervals.
//! - **Early Stage Mode (S1-S6)**: Static analysis only (no SCR/ULVF/POVC)
//! - **Full Analysis Mode (S7-S12)**: All metrics active including trend-based
//! - Uses `weighted geometric mean` for final verdict.
//! - Implements "Gunshot" early exit logic.
//! - Integrates real SOBP, MPCF, IWIM, LIGMA modules.
//!
//! # Architecture
//! ```text
//! [ Gatekeeper ] -> [ CyclicHyperPredictor ]
//!                            |
//!                            v
//!                      [ S1 ... S12 ] -> Loop
//!                            |
//!                       Update State (Shadow Ledger)
//!                       Score Candidate (Phase-Aware)
//!                       Check Gunshot
//!                            |
//!                            v
//!                      Final Verdict
//! ```
//!
//! # Quality Formulas
//!
//! **Early Stage (S1-S6)**:
//! ```text
//! Quality = 0.44 * MPCF + 0.31 * MESA + 0.25 * wallet_ratio
//! ```
//!
//! **Full Analysis (S7-S12)**:
//! ```text
//! Quality = 0.35 * MPCF + 0.25 * MESA + 0.20 * (1-SCR) + 0.20 * wallet_ratio
//! ```

use crate::chaos::amm_math::AmmPool;
use crate::fast_pipeline::EnhancedCandidate;
use crate::oracle::hyper_prediction::orchestrator::score_candidate_impl;
use crate::oracle::hyper_prediction::HyperPredictionOracle;
use crate::oracle::predator_strategy::{
    calculate_weighted_geometric_mean, get_gunshot_threshold, is_early_stage_cycle, ScoringPhase,
    CYCLE_WEIGHTS,
};
use crate::oracle::scoring::RiskLevel;
use crate::oracle::tcf::{observation_from_ghost_signals, TrendCohesionField};
use crate::oracle::ultrafast::{iwim, mpcf, sobp};
use crate::pumpfun::PumpCurveStateCache;
use crate::signals::ligma;
use anyhow::{anyhow, Result};
use ghost_core::shadow_ledger::ShadowLedger;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Cycle loop interval (Heartbeat)
const CYCLE_INTERVAL: Duration = Duration::from_millis(400);

/// Maximum cycles (S1-S12)
const MAX_CYCLES: usize = 12;

/// Final verdict decision threshold
const FINAL_VERDICT_THRESHOLD: f32 = 82.0;

/// Result of a single cycle execution
#[derive(Debug, Clone)]
pub enum CycleResult {
    /// Score met Gunshot threshold, exit immediately with BUY
    Gunshot(f32),
    /// Cycle completed, continue to next
    Continue(f32),
    /// Veto triggered (LIGMA, Rug, etc.), exit immediately with REJECT
    Veto(String),
}

/// Detailed result from a single cycle for history tracking
#[derive(Debug, Clone)]
pub struct CycleScoreRecord {
    /// Cycle index (0-11)
    pub cycle_idx: usize,
    /// Raw score before TCF modulation
    pub raw_score: f32,
    /// Score after TCF modulation
    pub modulated_score: f32,
    /// TCF cohesion score for this cycle
    pub tcf_score: f32,
    /// Whether cliff was detected
    pub cliff_detected: bool,
    /// Scoring phase used
    pub phase: ScoringPhase,
    /// Cycle duration in milliseconds
    pub duration_ms: u64,
}

/// Orchestrates the cyclic scoring process
pub struct CyclicHyperPredictor {
    oracle: Arc<HyperPredictionOracle>,
    shadow_ledger: Arc<ShadowLedger>,
}

impl CyclicHyperPredictor {
    pub fn new(oracle: Arc<HyperPredictionOracle>, shadow_ledger: Arc<ShadowLedger>) -> Self {
        Self {
            oracle,
            shadow_ledger,
        }
    }

    /// Execute the full S1-S12 cycle loop for a candidate
    pub async fn execute_cycles(
        &self,
        initial_candidate: &EnhancedCandidate,
        pumpfun_cache: &PumpCurveStateCache,
    ) -> Result<crate::oracle::hyper_prediction::state::HyperPredictionResult> {
        let mut scores = Vec::with_capacity(MAX_CYCLES);
        let mut cycle_records = Vec::with_capacity(MAX_CYCLES);
        let start_time = Instant::now();

        // 1. Initialization (Cold Start)
        // Instantiate TCF for this candidate's lifecycle using Oracle's TCF config
        let mut tcf = self.oracle.create_tcf();
        // Single source-of-truth for progress deltas in this predictor session.
        let mut prev_cycle_ts_ms: Option<u64> = None;
        let mut prev_cycle_tx_count: Option<u64> = None;

        // We need a mutable candidate to update with new data each cycle
        let mut current_candidate = initial_candidate.clone();

        for cycle in 0..MAX_CYCLES {
            let cycle_start = Instant::now();
            let cycle_id = cycle + 1;
            let phase = ScoringPhase::from_cycle_idx(cycle);

            debug!(
                "⚡ CYCLE S{} START ({}) for {}",
                cycle_id,
                phase.display_name(),
                current_candidate.bonding_curve
            );

            // Shadow Ledger is diagnostic-only; TCF uses neutral observation inputs.
            let (price_change, volume_change, tx_interval_cv) = (0.0, 0.0, 0.5);

            // 2. Execute Modular Scoring (Phase-Aware)
            let base_result = self
                .evaluate_cycle(&current_candidate, pumpfun_cache, cycle)
                .await?;

            match base_result {
                CycleResult::Veto(reason) => {
                    warn!(
                        "🛑 VETO at S{} ({}): {}",
                        cycle_id,
                        phase.display_name(),
                        reason
                    );
                    // Return failure result immediately
                    let mut final_result = score_candidate_impl(
                        &self.oracle,
                        &current_candidate,
                        pumpfun_cache,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )?;
                    final_result.score = 0; // Force fail
                    final_result.passed = false;
                    return Ok(final_result);
                }
                CycleResult::Gunshot(_) | CycleResult::Continue(_) => {
                    // Handled below
                }
            }

            let raw_score = match base_result {
                CycleResult::Gunshot(s) => s,
                CycleResult::Continue(s) => s,
                _ => 0.0,
            };

            // 3. TCF Injection
            // Build market observation from real ShadowLedger data
            let observation = observation_from_ghost_signals(
                price_change,
                volume_change,
                0.5, // buy_pressure_ratio (TODO: Extract from snapshots if available)
                1.0, // mpcf_confidence (Assuming high confidence for now)
                tx_interval_cv,
                0.0, // paradox_sync (Requires Paradox sensor data)
            );

            let (current_ts, current_tx) = self
                .shadow_ledger
                .get_latest_snapshot(&current_candidate.base_mint)
                .map(|snapshot| (snapshot.timestamp_ms, snapshot.tx_count))
                .unwrap_or((0, 0));
            let data_moved = match (prev_cycle_ts_ms, prev_cycle_tx_count) {
                (Some(prev_ts), Some(prev_tx)) => (current_tx > prev_tx) || (current_ts != prev_ts),
                _ => true,
            };
            prev_cycle_ts_ms = Some(current_ts);
            prev_cycle_tx_count = Some(current_tx);
            let tcf_result = tcf.update_with_progress(&observation, data_moved);

            // 4. Score Modulation (The Stabilizer)
            // Phase-aware modulation: apply phase modulation_factor to tcf_score
            // so that Pump/Dump/Chaos phases reduce confidence in momentum.
            let phase_factor = tcf_result.phase.modulation_factor() as f32;
            let effective_tcf = (tcf_result.tcf_score as f32 * phase_factor).clamp(0.0, 1.0);
            // If TCF detects a 'Cliff' (trend breakdown), the score is aggressively penalized.
            let modulated_score = if tcf_result.cliff_detected {
                raw_score * 0.6 // Penalty Mode
            } else {
                raw_score * (0.6 + 0.4 * effective_tcf) // Phase-modulated boost
            };

            // Store cycle record for history
            let cycle_duration = cycle_start.elapsed();
            cycle_records.push(CycleScoreRecord {
                cycle_idx: cycle,
                raw_score,
                modulated_score,
                tcf_score: tcf_result.tcf_score as f32,
                cliff_detected: tcf_result.cliff_detected,
                phase,
                duration_ms: cycle_duration.as_millis() as u64,
            });

            // Store history
            scores.push(modulated_score);

            debug!(
                "📊 S{} ({}) | Raw: {:.2} | TCF: {:.2} | Mod: {:.2} | Cliff: {}",
                cycle_id,
                phase.display_name(),
                raw_score,
                tcf_result.tcf_score,
                modulated_score,
                tcf_result.cliff_detected
            );

            // Per-cycle TCF component diagnostics
            if tcf_result.cohesion_computed_this_cycle {
                if let Some(ref cr) = tcf_result.last_cohesion_result {
                    debug!(
                        "[TCF|diag] pool={} cycle=S{} cohesion_computed_this_cycle=true \
                         price_volume_divergent={} direction_contradiction={} \
                         direction_score={:.4} rhythm_score={:.4} stability_score={:.4} \
                         total_penalty={:.4} total_bonus={:.4} cohesion_final={:.4}",
                        current_candidate.bonding_curve,
                        cycle_id,
                        cr.breakdown.price_volume_divergent,
                        cr.breakdown.direction_contradiction,
                        cr.direction_score,
                        cr.rhythm_score,
                        cr.stability_score,
                        cr.total_penalty,
                        cr.total_bonus,
                        cr.cohesion,
                    );
                }
            } else {
                debug!(
                    "[TCF|diag] pool={} cycle=S{} cohesion_computed_this_cycle=false \
                     price_volume_divergent=n/a direction_contradiction=n/a \
                     direction_score=n/a rhythm_score=n/a stability_score=n/a \
                     total_penalty=n/a total_bonus=n/a cohesion_final=n/a",
                    current_candidate.bonding_curve, cycle_id,
                );
            }

            // 5. Gunshot check based on modulated_score
            let threshold = get_gunshot_threshold(cycle);
            if modulated_score >= threshold {
                info!(
                    "🚀 GUNSHOT TRIGGERED at S{} ({})! Modulated Score: {:.2} >= {:.2} (Raw: {:.2}, TCF: {:.2})",
                    cycle_id,
                    phase.display_name(),
                    modulated_score,
                    threshold,
                    raw_score,
                    tcf_result.tcf_score
                );

                // Return positive result immediately
                let mut final_result = score_candidate_impl(
                    &self.oracle,
                    &current_candidate,
                    pumpfun_cache,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                final_result.score = modulated_score as u8;
                final_result.passed = true;
                return Ok(final_result);
            }

            // 6. Wait for next cycle (unless it's the last one)
            if cycle < MAX_CYCLES - 1 {
                let elapsed = cycle_start.elapsed();
                if elapsed < CYCLE_INTERVAL {
                    tokio::time::sleep(CYCLE_INTERVAL - elapsed).await;
                }
            }
        }

        // Final Verdict Logic
        let final_weighted_score = calculate_weighted_geometric_mean(&scores);

        let is_buy = final_weighted_score >= FINAL_VERDICT_THRESHOLD;

        // Log cycle summary
        let early_stage_avg: f32 = scores.iter().take(6).sum::<f32>() / 6.0;
        let full_analysis_avg: f32 = scores.iter().skip(6).sum::<f32>() / 6.0;

        info!(
            "🏁 Final Verdict: Score {:.2} (Threshold {:.1}) -> {} | Early Stage Avg: {:.2} | Full Analysis Avg: {:.2} | Total Time: {}ms",
            final_weighted_score,
            FINAL_VERDICT_THRESHOLD,
            if is_buy { "BUY" } else { "PASS" },
            early_stage_avg,
            full_analysis_avg,
            start_time.elapsed().as_millis()
        );

        // Construct final result
        let final_result = score_candidate_impl(
            &self.oracle,
            &current_candidate,
            pumpfun_cache,
            None, // explicit_pool_state
            None, // tx_timestamps
            None, // tx_data
            None, // iwim
            None, // chaos
            None, // resonance
            None, // gene_safety
            None, // hunter
            None, // tx_metrics
            None, // cluster
            None, // paradox
            None, // tuned_weights
            None, // ligma_result
            None, // ecto
            None, // bva
            None, // panic
            None, // tcr
            None, // cir
        )?;

        let mut result = final_result;
        result.score = final_weighted_score as u8;
        result.passed = is_buy;

        Ok(result)
    }

    /// Single cycle evaluation with phase-aware scoring
    async fn evaluate_cycle(
        &self,
        candidate: &EnhancedCandidate,
        pumpfun_cache: &PumpCurveStateCache,
        cycle_idx: usize,
    ) -> Result<CycleResult> {
        let phase = ScoringPhase::from_cycle_idx(cycle_idx);

        // 1. LIGMA Veto Check (runs in ALL phases)
        let ligma_config = &self.oracle.ligma_config;
        let amm_type = ghost_core::init_pool_parser::AmmType::PumpFun;
        let ligma_result = ligma::compute_ligma(
            candidate,
            None, // explicit pool state
            amm_type,
            ligma_config,
        );

        if ligma_result.liquidity_trap_risk > ligma_config.veto_trap_threshold {
            return Ok(CycleResult::Veto(format!(
                "LIGMA Trap Risk {:.2} > {:.2}",
                ligma_result.liquidity_trap_risk, ligma_config.veto_trap_threshold
            )));
        }

        if ligma_result.psi_ligma < ligma_config.veto_psi_ligma_threshold {
            return Ok(CycleResult::Veto(format!(
                "LIGMA Psi {:.2} < {:.2}",
                ligma_result.psi_ligma, ligma_config.veto_psi_ligma_threshold
            )));
        }

        // 2. Call HyperPrediction Orchestrator
        // The orchestrator handles phase detection internally based on tx_count
        // In the cyclic predictor, we pass phase information through the cycle index
        let result = score_candidate_impl(
            &self.oracle,
            candidate,
            pumpfun_cache,
            None,               // explicit_pool_state
            None,               // tx_timestamps (Should come from candidate/ledger)
            None,               // tx_data (Should come from candidate/ledger)
            None,               // iwim_result (Should compute real IWIM)
            None,               // chaos_result
            None,               // resonance_result
            None,               // gene_safety
            None,               // hunter
            None,               // tx_metrics
            None,               // cluster
            None,               // paradox
            None,               // tuned_weights
            Some(ligma_result), // Pass pre-computed LIGMA result
            None,               // ecto
            None,               // bva
            None,               // panic
            None,               // tcr
            None,               // cir
        )?;

        let score = result.score as f32;

        // Log phase-specific metrics
        if phase.is_early_stage() {
            debug!(
                "🐣 Early Stage S{}: Score={:.2}, SCR/ULVF/POVC skipped (insufficient samples)",
                cycle_idx + 1,
                score
            );
        } else {
            debug!(
                "🔬 Full Analysis S{}: Score={:.2}, all metrics active",
                cycle_idx + 1,
                score
            );
        }

        // 3. Gunshot Check
        let threshold = get_gunshot_threshold(cycle_idx);
        if score >= threshold {
            return Ok(CycleResult::Gunshot(score));
        }

        Ok(CycleResult::Continue(score))
    }

    /// Get the current scoring phase for a cycle
    pub fn get_phase_for_cycle(cycle_idx: usize) -> ScoringPhase {
        ScoringPhase::from_cycle_idx(cycle_idx)
    }
}
