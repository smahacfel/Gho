//! Oracle Pipeline - Async Multi-Worker Scoring System
//!
//! This module implements the Oracle scoring layer that filters and scores candidates
//! before they reach the Trigger component. Each Oracle component runs as a separate
//! async task to prevent CPU-heavy scoring (FFT, PCA, etc.) from blocking the event loop.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Oracle Pipeline                                  │
//! │                                                                          │
//! │  DetectedPool ──► ┌─────────────────────────────────────────────┐       │
//! │                   │              Scoring Workers                 │       │
//! │                   │                                              │       │
//! │                   │  ┌──────────────┐  ┌──────────────────────┐ │       │
//! │                   │  │ SimpleOracle │  │ score_enhanced()     │ │       │
//! │                   │  │   (async)    │  │ + Shadow Ledger      │ │       │
//! │                   │  └──────────────┘  │      (async)         │ │       │
//! │                   │                    └──────────────────────┘ │       │
//! │                   │  ┌──────────────┐  ┌──────────────────────┐ │       │
//! │                   │  │    QASS      │  │    HyperOracle       │ │       │
//! │                   │  │   (async)    │  │  SCR/ULVF/POVC       │ │       │
//! │                   │  └──────────────┘  │  (spawn_blocking)    │ │       │
//! │                   │                    └──────────────────────┘ │       │
//! │                   │  ┌──────────────┐  ┌──────────────────────┐ │       │
//! │                   │  │ VisionCritic │  │   ClusterHunter      │ │       │
//! │                   │  │ AI meme      │  │   Cabal detection    │ │       │
//! │                   │  │   (async)    │  │      (async)         │ │       │
//! │                   │  └──────────────┘  └──────────────────────┘ │       │
//! │                   │  ┌──────────────┐                          │       │
//! │                   │  │ DevProfiler  │                          │       │
//! │                   │  │ Creator risk │                          │       │
//! │                   │  │   (async)    │                          │       │
//! │                   │  └──────────────┘                          │       │
//! │                   └─────────────────────────────────────────────┘       │
//! │                                    │                                     │
//! │                                    ▼                                     │
//! │                          ┌─────────────────┐                            │
//! │                          │   Aggregator    │                            │
//! │                          │  join! + merge  │                            │
//! │                          └─────────────────┘                            │
//! │                                    │                                     │
//! │                                    ▼                                     │
//! │                      EnhancedScoringResult ──► Trigger (if passed)      │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Async Workers**: Each Oracle component runs in its own tokio task
//! - **7 Parallel Workers**: SimpleOracle, Enhanced/ShadowLedger, QASS, HyperOracle, VisionCritic, ClusterHunter, DevProfiler
//! - **Timeout Handling**: Workers have configurable timeouts; on timeout, candidate is skipped
//! - **Fault Tolerance**: Worker panics don't crash the pipeline
//! - **Telemetry**: Per-worker metrics (latency, throughput, timeouts)
//! - **Configuration**: All thresholds and timeouts configurable via config.toml

use crate::config::OracleConfig;
use crate::events::{DetectedPool, PoolTransaction};
use anyhow::Result;
use ghost_brain::telemetry::TelemetryRecorder;
use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger, LAMPORTS_PER_SOL};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, instrument, warn};

// Re-exports from ghost_e2e for Oracle components
use ghost_brain::oracle::{
    ClusterAnalysis,
    DevProfile,
    HyperOracle,
    HyperPredictionOracle,
    HyperPredictionResult,
    QASSResult,
    QuantumAmplitudeScorer,
    RiskLevel,
    ScoredCandidate,
    // Ghost Intelligence modules - results only since we use default() for now
    VisionCriticResult,
};

use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::pumpfun::PumpCurveStateCache;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Enhanced Scoring Result
// ============================================================================

/// Result of the Oracle scoring pipeline
///
/// Contains scores from all Oracle components and the final decision.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EnhancedScoringResult {
    /// Original pool data
    pub pool: Arc<DetectedPool>,

    /// HyperPrediction Oracle result (replaces SimpleOracle)
    pub hyper_prediction_result: Option<HyperPredictionResult>,

    /// Enhanced scoring result (with Shadow Ledger)
    pub enhanced_result: Option<ScoredCandidate>,

    /// QASS result (quantum-style amplitude superposition)
    pub qass_result: Option<QASSResult>,

    /// HyperOracle scores
    pub hyper_oracle_result: Option<HyperOracleResult>,

    /// VisionCritic result (AI meme quality analysis)
    pub vision_critic_result: Option<VisionCriticResult>,

    /// ClusterHunter result (Cabal detection)
    pub cluster_hunter_result: Option<ClusterAnalysis>,

    /// DevProfiler result (Creator behavioral analysis)
    pub dev_profiler_result: Option<DevProfile>,

    /// Combined final score (0-100)
    pub combined_score: u8,

    /// Final decision: true if candidate should proceed to Trigger
    pub passed: bool,

    /// Risk level assessment
    pub risk_level: RiskLevel,

    /// Processing time for entire pipeline (microseconds)
    pub processing_time_us: u64,

    /// Per-worker timings (microseconds)
    pub worker_timings: WorkerTimings,

    /// Human-readable interpretation
    pub interpretation: String,
}

/// HyperOracle analysis result
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HyperOracleResult {
    /// SCR (Slot-Coherence Resonance) score for bot detection
    pub scr_score: f32,
    /// ULVF (Ultra-Early Liquidity Vector Field) divergence
    pub ulvf_divergence: f32,
    /// ULVF curl (wash trading indicator)
    pub ulvf_curl: f32,
    /// POVC cluster (0=Dump, 1=Organic Hype, 2=Bot Noise)
    pub povc_cluster: usize,
    /// Whether the analysis indicates risk
    pub is_risky: bool,
}

/// Per-worker timing metrics
#[derive(Debug, Clone, Default)]
pub struct WorkerTimings {
    pub hyper_prediction_us: Option<u64>,
    pub enhanced_scoring_us: Option<u64>,
    pub qass_us: Option<u64>,
    pub hyper_oracle_us: Option<u64>,
    pub vision_critic_us: Option<u64>,
    pub cluster_hunter_us: Option<u64>,
    pub dev_profiler_us: Option<u64>,
}

// ============================================================================
// Oracle Pipeline Metrics
// ============================================================================

/// Telemetry metrics for the Oracle pipeline
#[derive(Debug, Default)]
pub struct OraclePipelineMetrics {
    /// Total candidates processed
    pub candidates_processed: u64,
    /// Candidates that passed scoring
    pub candidates_passed: u64,
    /// Candidates that failed scoring
    pub candidates_failed: u64,
    /// Candidates skipped due to timeout
    pub candidates_timeout: u64,
    /// Total processing time (microseconds)
    pub total_processing_time_us: u64,
    /// Average processing time (microseconds)
    pub avg_processing_time_us: u64,
    /// Per-worker metrics
    pub worker_metrics: WorkerMetrics,
}

/// Per-worker metrics
#[derive(Debug, Default)]
pub struct WorkerMetrics {
    pub hyper_prediction: WorkerStats,
    pub enhanced_scoring: WorkerStats,
    pub qass: WorkerStats,
    pub hyper_oracle: WorkerStats,
    pub vision_critic: WorkerStats,
    pub cluster_hunter: WorkerStats,
    pub dev_profiler: WorkerStats,
}

/// Statistics for a single worker
#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct WorkerStats {
    pub invocations: u64,
    pub successes: u64,
    pub failures: u64,
    pub timeouts: u64,
    pub total_time_us: u64,
    pub avg_time_us: u64,
}

// ============================================================================
// Oracle Pipeline
// ============================================================================

/// Main Oracle Pipeline coordinator
///
/// Orchestrates async workers for each Oracle component and aggregates results.
#[allow(dead_code)]
pub struct OraclePipeline {
    config: OracleConfig,
    hyper_prediction: HyperPredictionOracle,
    qass_scorer: QuantumAmplitudeScorer,
    hyper_oracle: HyperOracle,
    pumpfun_cache: Arc<PumpCurveStateCache>,
    shadow_ledger: Arc<ShadowLedger>,
    metrics: std::sync::Mutex<OraclePipelineMetrics>,
    telemetry: Option<Arc<TelemetryRecorder>>,
}

impl OraclePipeline {
    #[inline]
    fn now_wall_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[inline]
    fn detected_pool_event_ts_ms(pool: &DetectedPool) -> u64 {
        pool.effective_event_ts_ms()
            .or(pool.detected_wall_ts_ms)
            .unwrap_or_else(Self::now_wall_ms)
    }

    /// Create a new Oracle Pipeline with the given configuration
    pub fn new(config: OracleConfig, shadow_ledger: Arc<ShadowLedger>) -> Self {
        let threshold = config.simple_oracle.min_score_threshold;
        let hyper_prediction = if let Some(cfg) = config.ghost_brain_config.clone() {
            HyperPredictionOracle::new_with_config(threshold, &cfg)
        } else {
            HyperPredictionOracle::new(threshold)
        };

        // NOTE: QASS (Quantum Amplitude Superposition Scorer) is DEPRECATED as of Phase 4.5.
        // The primary scoring system is now SurvivorScore. QASS is retained only for:
        // - Backward compatibility with existing pipeline consumers
        // - HyperPredictionResult structure (returns neutral values)
        // This instance will be removed in a future version after full migration.
        let qass_scorer = QuantumAmplitudeScorer::new();

        let hyper_oracle = HyperOracle::new();
        let pumpfun_cache = Arc::new(PumpCurveStateCache::new());

        Self {
            config,
            hyper_prediction,
            qass_scorer,
            hyper_oracle,
            pumpfun_cache,
            shadow_ledger,
            metrics: std::sync::Mutex::new(OraclePipelineMetrics::default()),
            telemetry: None,
        }
    }

    /// Create a new Oracle Pipeline with telemetry recording
    pub fn with_telemetry(
        config: OracleConfig,
        shadow_ledger: Arc<ShadowLedger>,
        telemetry: Arc<TelemetryRecorder>,
    ) -> Self {
        let mut pipeline = Self::new(config, shadow_ledger);
        pipeline.telemetry = Some(telemetry);
        pipeline
    }

    /// Check if the pipeline is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get current metrics snapshot
    pub fn get_metrics(&self) -> OraclePipelineMetrics {
        let guard = self.metrics.lock().unwrap();
        OraclePipelineMetrics {
            candidates_processed: guard.candidates_processed,
            candidates_passed: guard.candidates_passed,
            candidates_failed: guard.candidates_failed,
            candidates_timeout: guard.candidates_timeout,
            total_processing_time_us: guard.total_processing_time_us,
            avg_processing_time_us: guard.avg_processing_time_us,
            worker_metrics: WorkerMetrics {
                hyper_prediction: guard.worker_metrics.hyper_prediction.clone(),
                enhanced_scoring: guard.worker_metrics.enhanced_scoring.clone(),
                qass: guard.worker_metrics.qass.clone(),
                hyper_oracle: guard.worker_metrics.hyper_oracle.clone(),
                vision_critic: guard.worker_metrics.vision_critic.clone(),
                cluster_hunter: guard.worker_metrics.cluster_hunter.clone(),
                dev_profiler: guard.worker_metrics.dev_profiler.clone(),
            },
        }
    }

    /// Score a detected pool using HyperPrediction Oracle
    ///
    /// This method now uses the unified HyperPrediction Oracle which integrates:
    /// 1. Shadow Ledger RAM simulations
    /// 2. SSMI (Sub-Slot Microentropy Index)
    /// 3. MPCF (Micro-Payload Cognitive Fingerprint)
    /// 4. QASS (Quantum Amplitude Superposition Scoring)
    /// 5. SCR/ULVF/POVC from HyperOracle
    /// 6. Enhanced contextual scoring
    ///
    /// Target: < 2s decision time with varied scoring output
    #[instrument(skip(self, pool), fields(pool_id = %pool.pool_amm_id))]
    pub async fn score_candidate(&self, pool: Arc<DetectedPool>) -> Result<EnhancedScoringResult> {
        let pipeline_start = Instant::now();
        let mut worker_timings = WorkerTimings::default();

        // Convert DetectedPool to EnhancedCandidate for scoring functions
        // No early transactions available at pipeline scoring time; dev_buy enriched via GatekeeperVerdict
        let enhanced_candidate = self.convert_to_enhanced_candidate(&pool, &[]);

        // Clone config values
        let hyper_timeout = Duration::from_millis(self.config.simple_oracle.timeout_ms);
        let vision_timeout = Duration::from_millis(self.config.vision_critic.timeout_ms);
        let cluster_timeout = Duration::from_millis(self.config.cluster_hunter.timeout_ms);
        let profiler_timeout = Duration::from_millis(self.config.dev_profiler.timeout_ms);
        let pipeline_timeout = Duration::from_millis(self.config.pipeline.pipeline_timeout_ms);

        let combined_threshold = self.config.pipeline.combined_score_threshold;

        // Vision/Cluster/Profiler config flags
        let vision_enabled = self.config.vision_critic.enabled;
        let cluster_enabled = self.config.cluster_hunter.enabled;
        let profiler_enabled = self.config.dev_profiler.enabled;

        // Clone pool data for new workers
        let pool_for_vision = Arc::clone(&pool);
        let pool_for_cluster = Arc::clone(&pool);
        let pool_for_profiler = Arc::clone(&pool);

        // Run all workers in parallel with global pipeline timeout
        let pipeline_result = timeout(pipeline_timeout, async {
            // Main HyperPrediction Oracle task
            let hyper_prediction_task = {
                let candidate = enhanced_candidate.clone();
                let hyper = self.hyper_prediction.clone();
                let pumpfun_cache = Arc::clone(&self.pumpfun_cache);
                tokio::spawn(async move {
                    let start = Instant::now();
                    let result = timeout(hyper_timeout, async move {
                        // Run HyperPrediction analysis
                        // Note: tx_timestamps and tx_data would be extracted from real transaction data
                        // tx_metrics = None here since this is called before OracleRuntime collects data
                        // tuned_weights = None since we don't have HysteresisLoop context here yet
                        hyper.score_candidate(
                            &candidate,
                            pumpfun_cache.as_ref(),
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
                        )
                    })
                    .await;
                    let elapsed = start.elapsed().as_micros() as u64;
                    (result, elapsed)
                })
            };

            // VisionCritic worker (AI meme quality - async with HTTP calls)
            let vision_critic_task = {
                let enabled = vision_enabled;
                let timeout_dur = vision_timeout;
                let _pool = pool_for_vision;

                tokio::spawn(async move {
                    let start = Instant::now();
                    if !enabled {
                        let elapsed = start.elapsed().as_micros() as u64;
                        return (Ok(VisionCriticResult::default()), elapsed);
                    }

                    let result =
                        timeout(timeout_dur, async move { VisionCriticResult::default() }).await;
                    let elapsed = start.elapsed().as_micros() as u64;
                    (result.map_err(|_| ()), elapsed)
                })
            };

            // ClusterHunter worker (Cabal detection - async with RPC calls)
            let cluster_hunter_task = {
                let enabled = cluster_enabled;
                let timeout_dur = cluster_timeout;
                let _pool = pool_for_cluster;

                tokio::spawn(async move {
                    let start = Instant::now();
                    if !enabled {
                        let elapsed = start.elapsed().as_micros() as u64;
                        return (Ok(ClusterAnalysis::default()), elapsed);
                    }

                    let result =
                        timeout(timeout_dur, async move { ClusterAnalysis::default() }).await;
                    let elapsed = start.elapsed().as_micros() as u64;
                    (result.map_err(|_| ()), elapsed)
                })
            };

            // DevProfiler worker (Creator analysis - async with RPC calls)
            let dev_profiler_task = {
                let enabled = profiler_enabled;
                let timeout_dur = profiler_timeout;
                let _pool = pool_for_profiler;

                tokio::spawn(async move {
                    let start = Instant::now();
                    if !enabled {
                        let elapsed = start.elapsed().as_micros() as u64;
                        return (Ok(DevProfile::default()), elapsed);
                    }

                    let result = timeout(timeout_dur, async move { DevProfile::default() }).await;
                    let elapsed = start.elapsed().as_micros() as u64;
                    (result.map_err(|_| ()), elapsed)
                })
            };

            // Wait for all workers to complete
            let (hyper_result, vision_result, cluster_result, profiler_result) = tokio::join!(
                hyper_prediction_task,
                vision_critic_task,
                cluster_hunter_task,
                dev_profiler_task
            );

            (hyper_result, vision_result, cluster_result, profiler_result)
        })
        .await;

        // Process results
        let (
            hyper_prediction_result,
            vision_critic_result,
            cluster_hunter_result,
            dev_profiler_result,
        ) = match pipeline_result {
            Ok((hyper, vision, cluster, profiler)) => {
                // Extract HyperPrediction result
                let hyper_prediction_result = match hyper {
                    Ok((Ok(Ok(result)), time)) => {
                        worker_timings.hyper_prediction_us = Some(time);
                        Some(result)
                    }
                    Ok((Ok(Err(e)), time)) => {
                        worker_timings.hyper_prediction_us = Some(time);
                        warn!("HyperPrediction scoring error: {:?}", e);
                        None
                    }
                    Ok((Err(_), time)) => {
                        worker_timings.hyper_prediction_us = Some(time);
                        warn!("HyperPrediction timeout");
                        None
                    }
                    Err(e) => {
                        warn!("HyperPrediction task error: {:?}", e);
                        None
                    }
                };

                // Extract VisionCritic result
                let vision_critic_result = match vision {
                    Ok((Ok(result), time)) => {
                        worker_timings.vision_critic_us = Some(time);
                        Some(result)
                    }
                    Ok((Err(_), time)) => {
                        worker_timings.vision_critic_us = Some(time);
                        warn!("VisionCritic timeout");
                        None
                    }
                    Err(e) => {
                        warn!("VisionCritic task error: {:?}", e);
                        None
                    }
                };

                // Extract ClusterHunter result
                let cluster_hunter_result = match cluster {
                    Ok((Ok(result), time)) => {
                        worker_timings.cluster_hunter_us = Some(time);
                        Some(result)
                    }
                    Ok((Err(_), time)) => {
                        worker_timings.cluster_hunter_us = Some(time);
                        warn!("ClusterHunter timeout");
                        None
                    }
                    Err(e) => {
                        warn!("ClusterHunter task error: {:?}", e);
                        None
                    }
                };

                // Extract DevProfiler result
                let dev_profiler_result = match profiler {
                    Ok((Ok(result), time)) => {
                        worker_timings.dev_profiler_us = Some(time);
                        Some(result)
                    }
                    Ok((Err(_), time)) => {
                        worker_timings.dev_profiler_us = Some(time);
                        warn!("DevProfiler timeout");
                        None
                    }
                    Err(e) => {
                        warn!("DevProfiler task error: {:?}", e);
                        None
                    }
                };

                (
                    hyper_prediction_result,
                    vision_critic_result,
                    cluster_hunter_result,
                    dev_profiler_result,
                )
            }
            Err(_) => {
                warn!("Pipeline timeout - skipping candidate");
                self.update_metrics_timeout();
                return Ok(EnhancedScoringResult {
                    pool,
                    hyper_prediction_result: None,
                    enhanced_result: None,
                    qass_result: None,
                    hyper_oracle_result: None,
                    vision_critic_result: None,
                    cluster_hunter_result: None,
                    dev_profiler_result: None,
                    combined_score: 0,
                    passed: false,
                    risk_level: RiskLevel::VeryHigh,
                    processing_time_us: pipeline_start.elapsed().as_micros() as u64,
                    worker_timings,
                    interpretation: "Pipeline timeout - candidate skipped".to_string(),
                });
            }
        };

        // Calculate combined score from HyperPrediction result
        let (combined_score, risk_level, passed, interpretation) =
            if let Some(hyper_res) = &hyper_prediction_result {
                let mut final_score = hyper_res.score;
                let mut final_risk = hyper_res.risk_level;

                // Apply additional modifiers from Ghost Intelligence modules
                if let Some(vision) = &vision_critic_result {
                    if vision.ai_analyzed && vision.viral_score > 7 {
                        final_score = final_score.saturating_add(5);
                    } else if vision.viral_score < 3 {
                        final_score = final_score.saturating_sub(5);
                    }
                }

                if let Some(cluster) = &cluster_hunter_result {
                    if cluster.is_high_risk {
                        final_score = final_score.saturating_sub(20);
                        final_risk = RiskLevel::VeryHigh;
                    }
                }

                if let Some(profiler) = &dev_profiler_result {
                    if profiler.mixer_interaction || profiler.rug_association {
                        final_score = final_score.saturating_sub(30);
                        final_risk = RiskLevel::VeryHigh;
                    }
                }

                let final_passed =
                    final_score >= combined_threshold && final_risk != RiskLevel::VeryHigh;
                (
                    final_score,
                    final_risk,
                    final_passed,
                    hyper_res.interpretation.clone(),
                )
            } else {
                (
                    0,
                    RiskLevel::VeryHigh,
                    false,
                    "HyperPrediction Oracle failed".to_string(),
                )
            };

        let processing_time_us = pipeline_start.elapsed().as_micros() as u64;

        // Update metrics
        self.update_metrics(passed, processing_time_us, &worker_timings);

        debug!(
            "Oracle scoring complete: score={}, passed={}, time={}μs",
            combined_score, passed, processing_time_us
        );

        // Log full scoring telemetry if telemetry recorder is available
        if let Some(ref telemetry) = self.telemetry {
            if let Some(ref hyper_res) = hyper_prediction_result {
                // Extract transaction data from pool (if available)
                // TODO(#zad-4-telemetry-full-jsonl): Extract tx data from canonical runtime/session
                // telemetry sources.
                // The transaction data should be passed from the caller (oracle_runtime)
                // using session / checkpoint / buffered history artifacts instead of any
                // legacy local per-pool compat wrapper.
                // For now, telemetry logs all subcomponent results without raw tx data.
                let txs = vec![];

                telemetry.log_hyper_prediction_scoring(&pool.pool_amm_id, hyper_res, txs);
            }
        }

        Ok(EnhancedScoringResult {
            pool,
            hyper_prediction_result: hyper_prediction_result.clone(),
            enhanced_result: None, // Now included in HyperPrediction
            // NOTE: qass_result is DEPRECATED - QASS has been replaced by SurvivorScore.
            // Downstream consumers should migrate to using:
            // - hyper_prediction_result.survivor_score_result (primary scoring)
            // - hyper_prediction_result.interpretation (human-readable summary)
            // This field returns None for all new scoring runs. Will be removed in future version.
            qass_result: None,
            hyper_oracle_result: hyper_prediction_result.as_ref().map(|h| HyperOracleResult {
                scr_score: h.scr_score.unwrap_or(0.0), // Default: no bot activity detected
                ulvf_divergence: h.ulvf_divergence.unwrap_or(0.0), // Default: no divergence
                ulvf_curl: h.ulvf_curl.unwrap_or(0.0), // Default: no wash trading
                povc_cluster: h.povc_cluster.unwrap_or(2), // Default: Bot Noise cluster (safest assumption)
                is_risky: risk_level == RiskLevel::High || risk_level == RiskLevel::VeryHigh,
            }),
            vision_critic_result,
            cluster_hunter_result,
            dev_profiler_result,
            combined_score,
            passed,
            risk_level,
            processing_time_us,
            worker_timings,
            interpretation,
        })
    }

    /// Convert DetectedPool to EnhancedCandidate
    ///
    /// Note: Invalid pubkeys are logged as warnings and default to Pubkey::default().
    /// In production, these would typically be valid Solana addresses from Seer.
    fn convert_to_enhanced_candidate(
        &self,
        pool: &DetectedPool,
        early_txs: &[PoolTransaction],
    ) -> EnhancedCandidate {
        // Helper function to parse pubkey with warning on failure
        fn parse_pubkey(s: &str, field_name: &str) -> solana_sdk::pubkey::Pubkey {
            s.parse().unwrap_or_else(|_| {
                tracing::warn!(
                    "Failed to parse {} as Pubkey: '{}', using default",
                    field_name,
                    s
                );
                solana_sdk::pubkey::Pubkey::default()
            })
        }

        let pool_amm_id = parse_pubkey(&pool.pool_amm_id, "pool_amm_id");
        let base_mint = parse_pubkey(&pool.base_mint, "base_mint");
        let shadow_snap = self
            .shadow_ledger
            .get_snapshots(&base_mint)
            .and_then(|snaps| snaps.last().cloned());

        let (shadow_progress, shadow_market_cap, shadow_price) = shadow_snap
            .map(|snap: MarketSnapshot| {
                let progress = snap.bonding_progress_pct.clamp(0.0, 100.0).round() as u64;
                let market_cap = (snap.market_cap_sol * LAMPORTS_PER_SOL).max(0.0) as u64;
                let price = (snap.price_sol_per_token * LAMPORTS_PER_SOL).max(0.0);
                (Some(progress), Some(market_cap), Some(price))
            })
            .unwrap_or((None, None, None));

        let (dev_buy_sol, has_dev_buy) =
            crate::oracle_runtime::extract_dev_buy_from_pool_txs(&pool.creator, early_txs);

        EnhancedCandidate {
            slot: pool.slot,
            timestamp: Self::detected_pool_event_ts_ms(pool),
            initial_liquidity_sol: pool.initial_liquidity_sol.unwrap_or(0.0),
            dev_buy_sol,
            bonding_curve_progress: None,
            vanity_score: 0,
            metadata_len_score: 50,
            has_dev_buy,
            mint_auth_disabled: false,
            _hot_padding: [0u8; 4],
            _cache_barrier_1: Default::default(),
            expected_price: shadow_price,
            shadow_bonding_progress: shadow_progress,
            virtual_sol_reserves: None,
            shadow_market_cap: shadow_market_cap,
            _cache_barrier_2: Default::default(),
            pool_amm_id,
            amm_program_id: parse_pubkey(&pool.amm_program, "amm_program"),
            base_mint: parse_pubkey(&pool.base_mint, "base_mint"),
            quote_mint: parse_pubkey(&pool.quote_mint, "quote_mint"),
            bonding_curve: parse_pubkey(&pool.bonding_curve, "bonding_curve"),
            signature: pool.signature.clone(),
            token_total_supply: None,
        }
    }

    /*
    /// OLD METHOD - Calculate combined score from all Oracle components
    /// REPLACED BY: HyperPredictionOracle.combine_scores()
    fn calculate_combined_score(
        &self,
        simple: &Option<ScoredCandidate>,
        enhanced: &Option<ScoredCandidate>,
        qass: &Option<QASSResult>,
        hyper: &Option<HyperOracleResult>,
        vision: &Option<VisionCriticResult>,
        cluster: &Option<ClusterAnalysis>,
        profiler: &Option<DevProfile>,
        threshold: u8,
    ) -> (u8, RiskLevel, bool) {
        let mut scores: Vec<f64> = Vec::new();
        let mut risk_level = RiskLevel::Medium;

        // Get configurable weight for enhanced scoring
        let enhanced_weight = self.config.pipeline.enhanced_score_weight;

        // Add SimpleOracle score
        if let Some(s) = simple {
            scores.push(s.score as f64);
            if s.risk_level == RiskLevel::VeryHigh {
                risk_level = RiskLevel::VeryHigh;
            } else if s.risk_level == RiskLevel::High && risk_level != RiskLevel::VeryHigh {
                risk_level = RiskLevel::High;
            }
        }

        // Add Enhanced score (weighted - fresher data from Shadow Ledger)
        if let Some(e) = enhanced {
            scores.push(e.score as f64 * enhanced_weight);
            if e.risk_level == RiskLevel::VeryHigh {
                risk_level = RiskLevel::VeryHigh;
            } else if e.risk_level == RiskLevel::High && risk_level != RiskLevel::VeryHigh {
                risk_level = RiskLevel::High;
            }
        }

        // Add QASS score (scaled to 0-100)
        if let Some(q) = qass {
            if q.is_valid {
                scores.push(q.score as f64 * 100.0);
            }
        }

        // Apply HyperOracle penalty if risky
        let hyper_penalty = if let Some(h) = hyper {
            if h.is_risky {
                if h.povc_cluster == 0 { // Dump trajectory
                    risk_level = RiskLevel::VeryHigh;
                    20.0
                } else if h.scr_score > 0.8 { // Strong bot signal
                    risk_level = RiskLevel::High;
                    15.0
                } else {
                    10.0
                }
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Apply VisionCritic score bonus/penalty
        let vision_modifier = if let Some(v) = vision {
            if v.ai_analyzed {
                // Scale viral score (0-10) to modifier (-10 to +10)
                (v.viral_score as f64 - 5.0) * 2.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Apply ClusterHunter penalty if high risk cabal detected
        let cluster_penalty = if let Some(c) = cluster {
            if c.is_high_risk {
                risk_level = RiskLevel::VeryHigh;
                25.0 // Heavy penalty for cabal detection
            } else if c.risk_score > 0.5 {
                risk_level = RiskLevel::High;
                15.0
            } else if c.risk_score > 0.3 {
                10.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Apply DevProfiler penalty if risky creator
        let profiler_penalty = if let Some(p) = profiler {
            if p.mixer_interaction || p.rug_association {
                risk_level = RiskLevel::VeryHigh;
                30.0 // Critical penalty for mixer/rug association
            } else if p.is_serial_minter {
                risk_level = RiskLevel::High;
                20.0
            } else if p.risk_score > 0.7 {
                15.0
            } else if p.risk_score > 0.4 {
                10.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Calculate weighted average with all modifiers
        let combined = if scores.is_empty() {
            0.0
        } else {
            let avg = scores.iter().sum::<f64>() / scores.len() as f64;
            (avg - hyper_penalty - cluster_penalty - profiler_penalty + vision_modifier).max(0.0)
        };

        let combined_score = (combined.min(100.0)) as u8;

        // Determine final risk level from score if not already VeryHigh
        if risk_level != RiskLevel::VeryHigh {
            risk_level = match combined_score {
                90..=100 => RiskLevel::Low,
                70..=89 => RiskLevel::Medium,
                50..=69 => RiskLevel::High,
                _ => RiskLevel::VeryHigh,
            };
        }

        let passed = combined_score >= threshold && risk_level != RiskLevel::VeryHigh;

        (combined_score, risk_level, passed)
    }
    */

    /*
    /// OLD METHOD - Generate human-readable interpretation
    /// REPLACED BY: HyperPredictionOracle.generate_interpretation()
    fn generate_interpretation(
        &self,
        simple: &Option<ScoredCandidate>,
        enhanced: &Option<ScoredCandidate>,
        qass: &Option<QASSResult>,
        hyper: &Option<HyperOracleResult>,
        vision: &Option<VisionCriticResult>,
        cluster: &Option<ClusterAnalysis>,
        profiler: &Option<DevProfile>,
        combined_score: u8,
        passed: bool,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        let action = if passed {
            if combined_score >= 80 {
                "🟢 STRONG BUY"
            } else if combined_score >= 70 {
                "🟡 BUY"
            } else {
                "⚠️ CONSIDER"
            }
        } else {
            if combined_score >= 50 {
                "🔶 SKIP (threshold)"
            } else {
                "🔴 REJECT"
            }
        };

        parts.push(format!("{} | Score: {}", action, combined_score));

        if let Some(s) = simple {
            parts.push(format!("Simple: {} ({:?})", s.score, s.risk_level));
        }

        if let Some(e) = enhanced {
            parts.push(format!("Enhanced: {} ({:?})", e.score, e.risk_level));
        }

        if let Some(q) = qass {
            if q.is_valid {
                parts.push(format!("QASS: {:.0}% conf={:.0}%", q.score * 100.0, q.confidence * 100.0));
            }
        }

        if let Some(h) = hyper {
            let cluster_name = match h.povc_cluster {
                0 => "Dump",
                1 => "Hype",
                2 => "Noise",
                _ => "Unknown",
            };
            parts.push(format!("Hyper: SCR={:.2} POVC={}", h.scr_score, cluster_name));
        }

        if let Some(v) = vision {
            if v.ai_analyzed {
                parts.push(format!("Vision: {}/10 ({:?})", v.viral_score, v.signal_strength));
            }
        }

        if let Some(c) = cluster {
            if c.is_high_risk {
                parts.push(format!("⚠️ CABAL: {}% supply", c.metrics.controlled_supply_pct));
            }
        }

        if let Some(p) = profiler {
            if p.mixer_interaction {
                parts.push("🚨 MIXER".to_string());
            } else if p.is_serial_minter {
                parts.push("⚠️ SerialMinter".to_string());
            } else if p.risk_score > 0.5 {
                parts.push(format!("Dev: risk={:.1}", p.risk_score));
            }
        }

        parts.join(" | ")
    }
    */

    /// Update metrics after successful scoring
    fn update_metrics(&self, passed: bool, processing_time_us: u64, timings: &WorkerTimings) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.candidates_processed += 1;
        if passed {
            metrics.candidates_passed += 1;
        } else {
            metrics.candidates_failed += 1;
        }
        metrics.total_processing_time_us += processing_time_us;
        metrics.avg_processing_time_us =
            metrics.total_processing_time_us / metrics.candidates_processed;

        // Update worker stats
        if let Some(time) = timings.hyper_prediction_us {
            metrics.worker_metrics.hyper_prediction.invocations += 1;
            metrics.worker_metrics.hyper_prediction.successes += 1;
            metrics.worker_metrics.hyper_prediction.total_time_us += time;
            metrics.worker_metrics.hyper_prediction.avg_time_us =
                metrics.worker_metrics.hyper_prediction.total_time_us
                    / metrics.worker_metrics.hyper_prediction.invocations;
        }

        if let Some(time) = timings.enhanced_scoring_us {
            metrics.worker_metrics.enhanced_scoring.invocations += 1;
            metrics.worker_metrics.enhanced_scoring.successes += 1;
            metrics.worker_metrics.enhanced_scoring.total_time_us += time;
            metrics.worker_metrics.enhanced_scoring.avg_time_us =
                metrics.worker_metrics.enhanced_scoring.total_time_us
                    / metrics.worker_metrics.enhanced_scoring.invocations;
        }

        if let Some(time) = timings.qass_us {
            metrics.worker_metrics.qass.invocations += 1;
            metrics.worker_metrics.qass.successes += 1;
            metrics.worker_metrics.qass.total_time_us += time;
            metrics.worker_metrics.qass.avg_time_us =
                metrics.worker_metrics.qass.total_time_us / metrics.worker_metrics.qass.invocations;
        }

        if let Some(time) = timings.hyper_oracle_us {
            metrics.worker_metrics.hyper_oracle.invocations += 1;
            metrics.worker_metrics.hyper_oracle.successes += 1;
            metrics.worker_metrics.hyper_oracle.total_time_us += time;
            metrics.worker_metrics.hyper_oracle.avg_time_us =
                metrics.worker_metrics.hyper_oracle.total_time_us
                    / metrics.worker_metrics.hyper_oracle.invocations;
        }

        if let Some(time) = timings.vision_critic_us {
            metrics.worker_metrics.vision_critic.invocations += 1;
            metrics.worker_metrics.vision_critic.successes += 1;
            metrics.worker_metrics.vision_critic.total_time_us += time;
            metrics.worker_metrics.vision_critic.avg_time_us =
                metrics.worker_metrics.vision_critic.total_time_us
                    / metrics.worker_metrics.vision_critic.invocations;
        }

        if let Some(time) = timings.cluster_hunter_us {
            metrics.worker_metrics.cluster_hunter.invocations += 1;
            metrics.worker_metrics.cluster_hunter.successes += 1;
            metrics.worker_metrics.cluster_hunter.total_time_us += time;
            metrics.worker_metrics.cluster_hunter.avg_time_us =
                metrics.worker_metrics.cluster_hunter.total_time_us
                    / metrics.worker_metrics.cluster_hunter.invocations;
        }

        if let Some(time) = timings.dev_profiler_us {
            metrics.worker_metrics.dev_profiler.invocations += 1;
            metrics.worker_metrics.dev_profiler.successes += 1;
            metrics.worker_metrics.dev_profiler.total_time_us += time;
            metrics.worker_metrics.dev_profiler.avg_time_us =
                metrics.worker_metrics.dev_profiler.total_time_us
                    / metrics.worker_metrics.dev_profiler.invocations;
        }
    }

    /// Update metrics for timeout
    fn update_metrics_timeout(&self) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.candidates_processed += 1;
        metrics.candidates_timeout += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OracleConfig;

    fn create_test_pool() -> Arc<DetectedPool> {
        Arc::new(DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "test_pool".to_string(),
            base_mint: "test_mint".to_string(),
            quote_mint: "So11111111111111111111111111111111111111112".to_string(),
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: "test_curve".to_string(),
            creator: "test_creator".to_string(),
            slot: Some(12345),
            tx_index: None,
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123),
            initial_liquidity_sol: Some(10.0),
            signature: "test_sig".to_string(),
        })
    }

    #[tokio::test]
    async fn test_oracle_pipeline_creation() {
        let config = OracleConfig::default();
        let pipeline = OraclePipeline::new(config, Arc::new(ShadowLedger::new()));
        assert!(pipeline.is_enabled());
    }

    #[tokio::test]
    async fn test_oracle_pipeline_scoring() {
        let config = OracleConfig::default();
        let pipeline = OraclePipeline::new(config, Arc::new(ShadowLedger::new()));
        let pool = create_test_pool();

        let result = pipeline.score_candidate(pool).await.unwrap();

        assert!(result.combined_score <= 100);
        assert!(result.processing_time_us > 0);
    }

    #[tokio::test]
    async fn test_oracle_pipeline_metrics() {
        let config = OracleConfig::default();
        let pipeline = OraclePipeline::new(config, Arc::new(ShadowLedger::new()));
        let pool = create_test_pool();

        let _ = pipeline.score_candidate(pool).await.unwrap();

        let metrics = pipeline.get_metrics();
        assert_eq!(metrics.candidates_processed, 1);
    }

    #[test]
    fn test_enhanced_scoring_result_defaults() {
        let pool = create_test_pool();
        let result = EnhancedScoringResult {
            pool,
            hyper_prediction_result: None,
            enhanced_result: None,
            qass_result: None,
            hyper_oracle_result: None,
            vision_critic_result: None,
            cluster_hunter_result: None,
            dev_profiler_result: None,
            combined_score: 50,
            passed: false,
            risk_level: RiskLevel::Medium,
            processing_time_us: 100,
            worker_timings: WorkerTimings::default(),
            interpretation: "Test".to_string(),
        };

        assert_eq!(result.combined_score, 50);
        assert!(!result.passed);
    }

    #[test]
    fn test_worker_timings_default() {
        let timings = WorkerTimings::default();
        assert!(timings.hyper_prediction_us.is_none());
        assert!(timings.enhanced_scoring_us.is_none());
        assert!(timings.qass_us.is_none());
        assert!(timings.hyper_oracle_us.is_none());
        assert!(timings.vision_critic_us.is_none());
        assert!(timings.cluster_hunter_us.is_none());
        assert!(timings.dev_profiler_us.is_none());
    }

    #[test]
    fn test_hyper_oracle_result() {
        let result = HyperOracleResult {
            scr_score: 0.5,
            ulvf_divergence: 0.4,
            ulvf_curl: 10.0,
            povc_cluster: 1,
            is_risky: false,
        };

        assert!(!result.is_risky);
        assert_eq!(result.povc_cluster, 1); // Organic Hype
    }

    // Z0.1 — dev_buy extraction via convert_to_enhanced_candidate

    fn make_pool_tx(
        signer: &str,
        is_buy: bool,
        is_dev_buy: bool,
        volume_sol: f64,
    ) -> PoolTransaction {
        use crate::events::RawBytesMissingReason;
        PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "test_pool".to_string(),
            slot: None,
            event_ordinal: None,
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 0,
            signer: signer.to_string(),
            is_buy,
            volume_sol,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy,
            dev_buy_lamports: 0,
            signature: "sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
        }
    }

    #[test]
    fn test_convert_to_enhanced_candidate_with_dev_buy() {
        let pipeline = OraclePipeline::new(OracleConfig::default(), Arc::new(ShadowLedger::new()));
        let mut pool = (*create_test_pool()).clone();
        pool.creator = "creator111".to_string();

        let txs = vec![make_pool_tx("creator111", true, false, 1.5)];
        let candidate = pipeline.convert_to_enhanced_candidate(&pool, &txs);

        assert!(candidate.has_dev_buy);
        assert!((candidate.dev_buy_sol - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_convert_to_enhanced_candidate_without_dev_buy() {
        let pipeline = OraclePipeline::new(OracleConfig::default(), Arc::new(ShadowLedger::new()));
        let pool = (*create_test_pool()).clone();

        let candidate = pipeline.convert_to_enhanced_candidate(&pool, &[]);

        assert!(!candidate.has_dev_buy);
        assert_eq!(candidate.dev_buy_sol, 0.0);
        assert_eq!(candidate.timestamp, 1_700_000_000_123);
    }

    #[test]
    fn test_convert_to_enhanced_candidate_is_dev_buy_flag() {
        let pipeline = OraclePipeline::new(OracleConfig::default(), Arc::new(ShadowLedger::new()));
        let mut pool = (*create_test_pool()).clone();
        pool.creator = "creator111".to_string();

        // Dev buys require creator-signed activity, not just the raw is_dev_buy flag.
        let txs = vec![make_pool_tx("other-signer", true, true, 2.0)];
        let candidate = pipeline.convert_to_enhanced_candidate(&pool, &txs);

        assert!(!candidate.has_dev_buy);
        assert_eq!(candidate.dev_buy_sol, 0.0);
    }

    #[test]
    fn test_convert_to_enhanced_candidate_ignores_legacy_only_timestamp() {
        let pipeline = OraclePipeline::new(OracleConfig::default(), Arc::new(ShadowLedger::new()));
        let mut pool = (*create_test_pool()).clone();
        pool.timestamp_ms = 1_700_000_000_000;
        pool.detected_wall_ts_ms = None;
        pool.event_time = ghost_core::EventTimeMetadata::default();

        let before = OraclePipeline::now_wall_ms();
        let candidate = pipeline.convert_to_enhanced_candidate(&pool, &[]);
        let after = OraclePipeline::now_wall_ms();

        assert_ne!(candidate.timestamp, pool.timestamp_ms);
        assert!(candidate.timestamp >= before && candidate.timestamp <= after);
    }
}
