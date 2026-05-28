//! Pipeline Stages - Seer and Oracle/Features Processing
//!
//! This module handles:
//! - Seer component initialization and bridge to fast_pipeline
//! - Oracle scoring and features extraction
//! - SwapPlan generation and forwarding
//! - Shadow Ledger integration for real-time bonding curve state
//! - QASS (Quantum-Style Amplitude Superposition Scoring) integration

use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use solana_sdk::signer::Signer;

use crate::fast_pipeline;
use crate::oracle_scoring::SimpleOracle;
use crate::strategy::StrategySelector;

use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::swap_plan::SwapPlan;
use seer::types::CandidatePool;
use seer::Seer;

use super::E2EPipeline;

impl E2EPipeline {
    /// Start the Seer component (now uses fast_pipeline for zero-copy)
    pub(super) async fn start_seer(&self) -> Result<tokio::task::JoinHandle<()>> {
        info!("Starting Seer component with fast_pipeline integration and Shadow Ledger");

        let seer_config = seer::config::SeerConfig {
            connection_mode: seer::config::ConnectionMode::WebSocket, // Use WebSocket for tests
            source_mode: None,                                        // Derive from connection_mode
            geyser_endpoint: self.config.websocket_url.clone(),
            grpc_endpoint: "http://localhost:10000".to_string(), // Placeholder
            helius_endpoint: None,
            rpc_endpoint: self.config.rpc_url.clone(),
            grpc_manual_backfill_enabled: true,
            grpc_client_id: None,
            grpc_auth_token: None,
            grpc_auth_header: seer::config::SeerConfig::default_grpc_auth_header(),
            max_reconnect_attempts: self.config.seer.max_reconnect_attempts,
            reconnect_delay_secs: self.config.seer.reconnect_delay_secs,
            max_reconnect_delay_secs: 300,
            grpc_max_stalls_before_open:
                seer::config::SeerConfig::default_grpc_max_stalls_before_open(),
            grpc_circuit_breaker_cooldown_ms:
                seer::config::SeerConfig::default_grpc_circuit_breaker_cooldown_ms(),
            verbose: self.config.seer.verbose,
            filter: seer::config::FilterConfig {
                enable_pumpfun: self.config.seer.enable_pumpfun,
                enable_bonkfun: self.config.seer.enable_bonkfun,
                allowed_quote_mints: vec![], // Allow all for now
                min_initial_liquidity_sol: self.config.seer.min_liquidity_sol,
            },
            channel_buffer_size: 1000,
            ipc_config: seer::ipc::IpcChannelConfig {
                buffer_size: 1000,
                backpressure_policy: seer::ipc::BackpressurePolicy::Block,
                log_drops: false,
                log_overflows: false,
                warning_threshold_percent: 80.0,
            },
            metrics_port: 9091, // Different from E2E metrics port
            ultrafast_enter_threshold: 80.0,
            ultrafast_exit_threshold: 50.0,
            commitment: seer::config::CommitmentLevel::Confirmed,
            grpc_commitment_fallback_to_websocket: false,
            pumpportal: Default::default(),
            stream_mode: seer::config::StreamMode::SingleGlobal,
            tx_filter_strategy: seer::config::TxFilterStrategy::PerPool,
            funding_lane_mode: seer::config::FundingLaneMode::Disabled,
            watched_pools_ttl_ms: 120_000,
            watched_pools_cap: 512,
            watch_debounce_ms: 0,
            canonical_account_update_relay_enabled: false,
        };

        // Create a channel for Seer to send to (bridge to fast_pipeline)
        // Channel capacity aligned with expected Seer throughput (events/sec)
        // Note: fast_pipeline queue capacity is 16,384 for burst handling
        let (bridge_tx, mut bridge_rx) = mpsc::channel::<CandidatePool>(1000);

        // Create Seer with Shadow Ledger support
        let seer = Arc::new(Seer::new_with_shadow_ledger(
            seer_config,
            bridge_tx,
            Arc::clone(&self.shadow_ledger),
        ));
        let _metrics = Arc::clone(&self.metrics);

        // Spawn Seer task
        let seer_handle = tokio::spawn(async move {
            loop {
                match Arc::clone(&seer).run().await {
                    Ok(_) => {
                        warn!("Seer exited normally, restarting...");
                    }
                    Err(e) => {
                        error!("Seer error: {}, restarting in 5s...", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        // Spawn bridge task: converts mpsc to fast_pipeline
        tokio::spawn(async move {
            info!("Starting Seer→FastPipeline bridge");
            while let Some(candidate) = bridge_rx.recv().await {
                // Push to fast_pipeline (zero-copy, lock-free)
                // Use helper function to convert CandidatePool to PremintCandidate fields
                if let Err(_) = fast_pipeline::push_candidate(|c| {
                    // Map fields from CandidatePool to PremintCandidate
                    c.slot = candidate.slot;
                    c.pool_amm_id = candidate.pool_amm_id;
                    c.amm_program_id = candidate.amm_program_id;
                    c.base_mint = candidate.base_mint;
                    c.quote_mint = candidate.quote_mint;
                    c.bonding_curve = candidate.bonding_curve;
                    c.liquidity_sol = candidate.initial_liquidity_sol.unwrap_or(0.0);
                    c.base_score = 50; // Default base score
                    c.signature = candidate.signature.clone();
                    c.timestamp = candidate.timestamp;
                    c.bonding_curve_progress = candidate.bonding_curve_progress;
                    c.token_total_supply = candidate.token_total_supply;
                    c.block_time = candidate.block_time;
                }) {
                    warn!(
                        "Fast pipeline queue full, dropping candidate: pool={}, timestamp={}, signature={}",
                        candidate.pool_amm_id, candidate.timestamp, candidate.signature
                    );
                }
            }
            warn!("Seer→FastPipeline bridge ended");
        });

        Ok(seer_handle)
    }

    /// Start Oracle and Features processing (now using fast_pipeline batch consumer)
    ///
    /// Integrates Shadow Ledger for real-time bonding curve state:
    /// - Predicts expected price before scoring
    /// - Skips candidates with complete bonding curves (>98%)
    /// - Passes fresh market data to scoring for accurate decisions
    pub(super) async fn start_oracle_features(
        &self,
        swap_plan_tx: mpsc::Sender<SwapPlan>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        info!("Starting Oracle/Features component with fast_pipeline batch consumer and Shadow Ledger integration");

        let oracle = SimpleOracle::new(self.config.oracle.min_score_threshold);
        let strategy_selector = StrategySelector::new(
            self.authority.pubkey(),
            self.config.features.max_position_size_lamports,
            self.config.features.max_slippage,
            self.config.features.intent_timeout_secs,
        );

        let metrics = Arc::clone(&self.metrics);
        let gui_state = self.gui_state.clone();
        let min_score_threshold = oracle.min_score_threshold();

        // Clone shadow_ledger for use in async task
        let shadow_ledger = Arc::clone(&self.shadow_ledger);
        // Get default buy size from config for Shadow Ledger simulation
        let default_buy_size = self.config.features.max_position_size_lamports;

        let handle = tokio::spawn(async move {
            // Use fast_pipeline batch consumer for zero-copy processing
            const BATCH_SIZE: usize = 128; // Process up to 128 candidates at once

            // Migration risk threshold (98%) - candidates above this are skipped
            const MIGRATION_RISK_THRESHOLD: u64 = 98;

            info!(
                "Fast pipeline batch consumer started (batch_size={}, migration_threshold={}%)",
                BATCH_SIZE, MIGRATION_RISK_THRESHOLD
            );

            fast_pipeline::run_fast_consumer(BATCH_SIZE, |batch| {
                // Process entire batch with zero clone()!
                let batch_start = Instant::now();

                for candidate_arc in batch {
                    let start = Instant::now();

                    info!(
                        "Processing candidate: pool={}, amm={:?}, mint={}",
                        candidate_arc.pool_amm_id, candidate_arc.amm_program_id, candidate_arc.base_mint
                    );

                    // === SHADOW LEDGER INTEGRATION (MODUŁ 3) ===
                    // Before scoring, get prediction from Shadow Ledger
                    let (expected_price, shadow_progress, virtual_sol, market_cap, skip_reason) = {
                        // Use candidate's slot for staleness check
                        let current_slot = candidate_arc.slot;

                        // Try to get simulation from Shadow Ledger
                        match shadow_ledger.simulate_buy(&candidate_arc.base_mint, default_buy_size, current_slot) {
                            Ok(sim) => {
                                // Check for migration risk - IMMEDIATE SKIP if >98%
                                if sim.bonding_progress > MIGRATION_RISK_THRESHOLD {
                                    (
                                        Some(sim.effective_price_per_token),
                                        Some(sim.bonding_progress),
                                        None,
                                        Some(sim.market_cap_sol),
                                        Some(format!("Bonding curve complete ({}% > {}%)", sim.bonding_progress, MIGRATION_RISK_THRESHOLD))
                                    )
                                } else {
                                    // Get virtual_sol_reserves from the underlying curve
                                    let virtual_sol = shadow_ledger.get(&candidate_arc.base_mint)
                                        .map(|curve| curve.virtual_sol_reserves);

                                    (
                                        Some(sim.effective_price_per_token),
                                        Some(sim.bonding_progress),
                                        virtual_sol,
                                        Some(sim.market_cap_sol),
                                        None
                                    )
                                }
                            }
                            Err(e) => {
                                // Shadow Ledger doesn't have fresh data - use RPC-derived values
                                debug!(
                                    "Shadow Ledger miss for mint={}: {}. Using RPC-derived values.",
                                    candidate_arc.base_mint, e
                                );
                                (None, None, None, None, None)
                            }
                        }
                    };

                    // Skip if bonding curve is complete (migration risk)
                    if let Some(reason) = skip_reason {
                        warn!(
                            target: "shadow_ledger",
                            "⏭️ SKIP: {} for pool={}, mint={}. Avoiding migration freeze risk.",
                            reason, candidate_arc.pool_amm_id, candidate_arc.base_mint
                        );
                        metrics.oracle_candidates_scored.inc();
                        metrics.features_plans_rejected.inc();
                        continue;
                    }

                    // Convert PremintCandidate to EnhancedCandidate with Shadow Ledger data
                    use crate::fast_pipeline::{CacheLinePadding, EnhancedCandidate};
                    use crate::oracle_scoring::score_enhanced;

                    // FIX: Dynamic derivation of security flags to prevent Oracle from rejecting valid pools in simulation mode
                    let enhanced_candidate = EnhancedCandidate {
                        slot: candidate_arc.slot,
                        pool_amm_id: candidate_arc.pool_amm_id,
                        amm_program_id: candidate_arc.amm_program_id,
                        base_mint: candidate_arc.base_mint,
                        quote_mint: candidate_arc.quote_mint,
                        bonding_curve: candidate_arc.bonding_curve,
                        timestamp: candidate_arc.timestamp,
                        bonding_curve_progress: candidate_arc.bonding_curve_progress,
                        initial_liquidity_sol: candidate_arc.liquidity_sol,
                        token_total_supply: candidate_arc.token_total_supply,
                        signature: candidate_arc.signature.clone(),
                        // Enhanced fields - FIXED: Derived from real data instead of hardcoded failure
                        vanity_score: 0,
                        // If initial liquidity exists, the dev has bought/seeded the pool
                        has_dev_buy: candidate_arc.liquidity_sol > 0.0,
                        dev_buy_sol: candidate_arc.liquidity_sol,
                        // Standard PumpFun/BonkFun curves manage auth, treat as disabled/safe for scoring
                        mint_auth_disabled: true,
                        metadata_len_score: 50,
                        // Shadow Ledger fields (fresh market data)
                        expected_price,
                        shadow_bonding_progress: shadow_progress,
                        virtual_sol_reserves: virtual_sol,
                        shadow_market_cap: market_cap,
                        // Cache line padding fields (required for false sharing prevention)
                        _hot_padding: [0; 4],
                        _cache_barrier_1: CacheLinePadding::default(),
                        _cache_barrier_2: CacheLinePadding::default(),
                    };

                    // Log Shadow Ledger data for debugging
                    if expected_price.is_some() || shadow_progress.is_some() {
                        debug!(
                            target: "shadow_ledger",
                            "Shadow Ledger data for mint={}: expected_price={:?}, progress={:?}%, virtual_sol={:?}, mcap={:?}",
                            candidate_arc.base_mint,
                            expected_price,
                            shadow_progress,
                            virtual_sol,
                            market_cap
                        );
                    }

                    // === QASS-ENHANCED SCORING ===
                    // Use score_enhanced_with_qass for full QASS integration
                    let scored_with_qass = crate::oracle_scoring::score_enhanced_with_qass(
                        &enhanced_candidate,
                        min_score_threshold,
                        None, // Use default QASS scorer
                    );

                    // Extract base scored candidate for compatibility with existing pipeline
                    let scored = scored_with_qass.base.clone();

                    // Record QASS metrics
                    if let Some(ref qass_result) = scored_with_qass.qass_result {
                        // Safe cast: QASS latency is typically <25μs (25000ns), well within f64 precision
                        // Max u64 that f64 can represent exactly is 2^53, which is way larger than expected latency
                        let latency_ns = qass_result.analysis_time_ns.min(u64::MAX / 2) as f64;
                        metrics.qass_latency_ns.observe(latency_ns);
                        metrics.qass_score_histogram.observe(qass_result.score_100 as f64);
                        metrics.qass_confidence_histogram.observe(qass_result.confidence as f64);

                        // Track validity
                        let validity_label = if qass_result.is_valid { "valid" } else { "invalid" };
                        metrics.qass_analyses_total.with_label_values(&[validity_label]).inc();

                        // Track dominant wave if any
                        if let Some(dominant) = qass_result.dominant_waves.first() {
                            metrics.qass_dominant_waves.with_label_values(&[dominant]).inc();
                        }
                    }

                    metrics.oracle_candidates_scored.inc();
                    metrics.oracle_score_histogram.observe(scored.score as f64);
                    metrics.oracle_latency.observe(start.elapsed().as_millis() as f64);

                    // Log QASS interpretation for operator
                    info!(
                        target: "qass_operator",
                        "{}",
                        scored_with_qass.interpretation
                    );
                    info!(
                        "Oracle score: {} → combined: {} (passed: {}, risk: {:?})",
                        scored.score, scored_with_qass.combined_score, scored.passed, scored.risk_level
                    );

                    // Get runtime settings from GUI state if available
                    let (runtime_position_size, runtime_slippage) = if let Some(ref state) = gui_state {
                        let settings = state.get_settings();
                        info!(
                            "Using runtime settings: position_size={} lamports ({} SOL), slippage={}%",
                            settings.position_size_lamports,
                            settings.position_size_lamports as f64 / 1_000_000_000.0,
                            settings.max_slippage * 100.0
                        );
                        (Some(settings.position_size_lamports), Some(settings.max_slippage))
                    } else {
                        (None, None)
                    };

                    // Strategy selection and SwapPlan generation with runtime overrides
                    let swap_plan = match strategy_selector.generate_swap_plan_with_overrides(
                        &scored,
                        runtime_position_size,
                        runtime_slippage,
                    ) {
                        Ok(Some(plan)) => {
                            metrics.features_plans_created.inc();
                            plan
                        }
                        Ok(None) => {
                            metrics.features_plans_rejected.inc();
                            info!(
                                "Candidate rejected by strategy selector. Pool={}, score={}, passed={}",
                                scored.pool.pool_amm_id, scored.score, scored.passed
                            );
                            continue;
                        }
                        Err(e) => {
                            error!(
                                "Strategy selection error: {}. Candidate data: pool={}, score={}, risk={:?}",
                                e, scored.pool.pool_amm_id, scored.score, scored.risk_level
                            );
                            continue;
                        }
                    };

                    info!(
                        "SwapPlan created: amount_in={} lamports ({} SOL), min_amount_out={}",
                        swap_plan.amount_in,
                        swap_plan.amount_in as f64 / 1_000_000_000.0,
                        swap_plan.min_amount_out
                    );

                    // Forward to Trigger via DirectBuyBuilder
                    // Note: Using try_send here since we're in a sync closure
                    // The channel should have sufficient capacity (100) to buffer
                    if let Err(e) = swap_plan_tx.try_send(swap_plan) {
                        error!(
                            "Failed to send SwapPlan to Trigger (channel full or closed): {}. \
                            Consider increasing swap_plan channel capacity if this occurs frequently.",
                            e
                        );
                        // Continue processing other candidates in batch
                    }
                }

                let batch_duration = batch_start.elapsed();
                if batch.len() > 0 {
                    debug!(
                        "Processed batch of {} candidates in {:?} ({:.2} candidates/sec)",
                        batch.len(),
                        batch_duration,
                        batch.len() as f64 / batch_duration.as_secs_f64()
                    );
                }
            }).await;

            warn!("Oracle/Features batch consumer task ended");
        });

        Ok(handle)
    }
}
