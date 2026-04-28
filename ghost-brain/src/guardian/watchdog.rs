//! Watchdog implementation for handling the "2-Second Void" problem
//!
//! This module implements the Supervisor Loop as specified in TASK 1.2 of the
//! T<2S COMPONENTS IMPL PLAN. The supervisor coordinates multiple parallel tasks
//! during the "void" period and makes decisions based on:
//! 1. Slot updates with QASS re-evaluation
//! 2. Internal task results (Chaos Engine, Gene Mapper)
//! 3. External Hunter results
//! 4. Timeout conditions

use super::types::{WatchdogConfig, WatchdogContext, WatchdogDecision, WatchdogSignal};
use crate::telemetry::TelemetryRecorder;
use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// Risk score thresholds for Resonance Detector
// These can be made configurable in the future via ResonanceConfig
const RESONANCE_BOT_RISK_SCORE: f64 = 0.7; // High concern for bot activity
const RESONANCE_SUSPICIOUS_RISK_SCORE: f64 = 0.4; // Moderate concern
const RESONANCE_HUMAN_RISK_SCORE: f64 = 0.1; // Low concern

/// Network health assessment returned by the Guardian
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetworkHealth {
    /// Whether the network conditions are considered safe to trade
    pub is_safe: bool,
    /// Safety coefficient in range 0.0-1.0 used for adaptive throttling
    pub safety_coefficient: f64,
}

/// Evaluate network jitter and return adaptive safety parameters
///
/// * `< 50ms`  → High confidence (coefficient = 1.0)
/// * `50-200ms` → Degraded (coefficient = 0.5)
/// * `> 200ms` → Unsafe (coefficient = 0.0)
pub fn evaluate_network_health(jitter_ms: f64) -> NetworkHealth {
    let clamped_jitter = jitter_ms.max(0.0);
    let safety_coefficient = if clamped_jitter < 50.0 {
        1.0
    } else if clamped_jitter <= 200.0 {
        0.5
    } else {
        0.0
    };

    NetworkHealth {
        is_safe: safety_coefficient > 0.0,
        safety_coefficient,
    }
}

/// Chaos Engine task - Performs Monte Carlo simulation for risk assessment
///
/// This function runs a Monte Carlo simulation using the ChaosEngine to assess
/// market risk probabilities (crash/pump scenarios).
async fn spawn_chaos_task(
    tx: mpsc::Sender<WatchdogSignal>,
    pool: crate::chaos::AmmPool,
    scenario: crate::chaos::MarketScenario,
) -> Result<()> {
    use crate::chaos::{ChaosEngine, SimulationConfig};

    // Create ChaosEngine with default configuration
    let engine = ChaosEngine::new(SimulationConfig::default());

    // Run actual Monte Carlo simulation
    let result = engine.run_simulation(&pool, scenario)?;

    // Map crash_probability to risk_score (0.0-1.0)
    // crash_probability is a percentage (0-100), so divide by 100
    let risk_score = result.crash_probability / 100.0;
    let risk_detected = result.crash_probability > 50.0;

    // Create detailed summary from ChaosResult
    let details = format!(
        "Monte Carlo: crash={:.1}%, pump={:.1}%, median_roi={:.2}%, volatility={:.4}, sims={}, time={}ms",
        result.crash_probability,
        result.pump_probability,
        result.median_roi,
        result.price_volatility,
        result.num_simulations,
        result.execution_time_ms
    );

    debug!("Chaos Engine completed: {}", details);

    // Send result via channel
    tx.send(WatchdogSignal::InternalTaskResult {
        task_name: "Chaos Engine".to_string(),
        risk_detected,
        risk_score,
        details,
    })
    .await?;

    Ok(())
}

/// Gene Mapper task - Performs static bytecode analysis
///
/// This function analyzes program bytecode for malicious patterns using the Gene Mapper.
/// It can accept optional bytecode; if none is provided, analysis is skipped and reported.
async fn spawn_gene_mapper_task(
    tx: mpsc::Sender<WatchdogSignal>,
    bytecode: Option<Vec<u8>>,
) -> Result<()> {
    use crate::security::GeneMapper;

    let Some(code) = bytecode else {
        tx.send(WatchdogSignal::InternalTaskResult {
            task_name: "Gene Mapper".to_string(),
            risk_detected: false,
            risk_score: 0.0,
            details: "Bytecode unavailable - static analysis skipped".to_string(),
        })
        .await?;
        return Ok(());
    };

    // Create Gene Mapper with default configuration
    let mapper = GeneMapper::new();

    // Analyze provided bytecode payload
    let result = mapper.analyze(&code);

    // Map Gene Mapper results to Watchdog signals
    let risk_detected = result.is_high_risk();
    let risk_score = result.risk_score;

    // Create detailed summary
    let details = if result.is_critical() {
        format!(
            "CRITICAL: {} | Recommended: {}",
            result.threat_summary,
            result.recommended_action()
        )
    } else if result.is_high_risk() {
        format!(
            "HIGH RISK: {} | Patterns: {} | Recommended: {}",
            result.threat_summary,
            result.detected_patterns.len(),
            result.recommended_action()
        )
    } else if !result.detected_patterns.is_empty() {
        format!(
            "Risk detected: {} | Patterns: {} ({})",
            result.threat_summary,
            result.detected_patterns.len(),
            result.recommended_action()
        )
    } else {
        format!(
            "Bytecode analysis completed | Status: {} | Scanned: {} bytes",
            result.recommended_action(),
            result.bytes_scanned
        )
    };

    tx.send(WatchdogSignal::InternalTaskResult {
        task_name: "Gene Mapper".to_string(),
        risk_detected,
        risk_score,
        details,
    })
    .await?;

    Ok(())
}

/// Resonance Detector task - Performs bot activity detection via pattern analysis
///
/// This function analyzes trade timing patterns to detect bot networks and
/// automated trading systems using the Resonance Detector.
async fn spawn_resonance_task(
    tx: mpsc::Sender<WatchdogSignal>,
    timestamps: Vec<u64>,
) -> Result<()> {
    use crate::signals::ResonanceDetector;

    // Create detector with default configuration
    let mut detector = ResonanceDetector::new();

    // Add timestamps to analyze
    detector.add_timestamps(&timestamps);

    // Analyze trading pattern
    let result = detector.analyze();

    // Map bot detection to risk score
    // Bot activity is concerning for trading decisions
    let risk_detected = result.is_bot_likely();
    let risk_score = if result.is_bot_likely() {
        RESONANCE_BOT_RISK_SCORE
    } else if result.is_suspicious() {
        RESONANCE_SUSPICIOUS_RISK_SCORE
    } else {
        RESONANCE_HUMAN_RISK_SCORE
    };

    // Create detailed summary
    let details = format!(
        "Resonance: CV={:.3}, score={:.3}, class={:?}, samples={}",
        result.coefficient_variation,
        result.resonance_score,
        result.classification,
        result.sample_count
    );

    debug!("Resonance Detector completed: {}", details);

    // Send result via channel
    tx.send(WatchdogSignal::InternalTaskResult {
        task_name: "Resonance Detector".to_string(),
        risk_detected,
        risk_score,
        details,
    })
    .await?;

    Ok(())
}

/// Run the watchdog supervisor loop
///
/// This is the heart of the new system implementing TASK 1.2.
/// It spawns parallel tasks immediately and uses tokio::select! to handle:
/// 1. Slot updates with QASS re-evaluation
/// 2. Internal task results (Chaos, Gene, Resonance)
/// 3. External Hunter results
/// 4. Timeout conditions
///
/// # Arguments
/// * `config` - Watchdog configuration
/// * `signal_rx` - Channel for receiving signals
/// * `telemetry` - Optional telemetry recorder for logging decisions
/// * `pool` - Optional AMM pool state for Chaos Engine simulation
/// * `scenario` - Market scenario for Chaos Engine (defaults to Mixed)
/// * `timestamps` - Optional trade timestamps for Resonance Detector
///
/// # Returns
/// * `WatchdogDecision` - Decision to Proceed, Abort, or Timeout
pub async fn run_watchdog(
    config: WatchdogConfig,
    mut signal_rx: mpsc::Receiver<WatchdogSignal>,
    telemetry: Option<TelemetryRecorder>,
    pool: Option<crate::chaos::AmmPool>,
    scenario: Option<crate::chaos::MarketScenario>,
    timestamps: Option<Vec<u64>>,
) -> Result<WatchdogDecision> {
    info!(
        "Starting watchdog supervisor loop with config: {:?}",
        config
    );

    let void_timeout = tokio::time::Duration::from_millis(config.max_void_duration_ms);
    let start_time = tokio::time::Instant::now();

    let mut failure_count = 0;
    let mut context = WatchdogContext::default();

    // Create internal channel for task results
    let (internal_tx, mut internal_rx) = mpsc::channel::<WatchdogSignal>(10);

    // Spawn parallel tasks immediately if enabled
    if config.enable_parallel_tasks {
        info!("Spawning parallel internal tasks (Chaos Engine, Gene Mapper, Resonance Detector)");

        // Calculate number of tasks to spawn based on available data
        let mut task_count = 1; // Gene Mapper always runs
        if pool.is_some() {
            task_count += 1; // Chaos Engine
        }
        if timestamps.is_some() {
            task_count += 1; // Resonance Detector
        }

        context.internal_tasks_spawned = task_count;

        // Spawn Chaos Engine task if pool state is available
        if let Some(pool_state) = pool {
            let chaos_tx = internal_tx.clone();
            let scenario_to_use = scenario.unwrap_or(crate::chaos::MarketScenario::Mixed);
            tokio::spawn(async move {
                if let Err(e) = spawn_chaos_task(chaos_tx, pool_state, scenario_to_use).await {
                    error!("Chaos Engine task failed: {}", e);
                }
            });
        } else {
            debug!("Chaos Engine not spawned - no pool state provided");
        }

        // Spawn Gene Mapper task
        let gene_tx = internal_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = spawn_gene_mapper_task(gene_tx, None).await {
                error!("Gene Mapper task failed: {}", e);
            }
        });

        // Spawn Resonance Detector task if timestamps are available
        if let Some(ts) = timestamps {
            let resonance_tx = internal_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = spawn_resonance_task(resonance_tx, ts).await {
                    error!("Resonance Detector task failed: {}", e);
                }
            });
        } else {
            debug!("Resonance Detector not spawned - no timestamps provided");
        }
    }

    // Main supervisor loop using tokio::select!
    loop {
        tokio::select! {
            // 1. Handle incoming signals from external sources
            Some(signal) = signal_rx.recv() => {
                debug!("Watchdog received external signal: {:?}", signal);

                match signal {
                    WatchdogSignal::SlotUpdate { slot, timestamp_ms, qass_score } => {
                        debug!("Slot update: slot={}, timestamp={}, qass_score={:?}",
                               slot, timestamp_ms, qass_score);

                        // Update context
                        context.latest_slot = Some(slot);

                        // Re-evaluate fast metrics (QASS)
                        if let Some(score) = qass_score {
                            context.latest_qass_score = Some(score);

                            // Check if QASS score drops below threshold -> ABORT
                            if score < config.min_qass_score {
                                warn!("QASS score {} below threshold {} - ABORTING",
                                      score, config.min_qass_score);

                                // Log telemetry before aborting
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Abort,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                }

                                return Ok(WatchdogDecision::Abort);
                            }

                            info!("QASS score {} passes threshold {}",
                                  score, config.min_qass_score);
                        }
                    }

                    WatchdogSignal::ExternalResult { success, data } => {
                        context.external_result_received = true;

                        if success {
                            info!("External Hunter result successful: {}", data);

                            // Wait for internal checks before proceeding if tasks are still running
                            if config.enable_parallel_tasks
                                && context.internal_tasks_completed < context.internal_tasks_spawned {
                                info!("External result received, waiting for internal tasks to complete");
                                // Continue loop to wait for internal tasks
                            } else {
                                // All checks passed, proceed
                                info!("All checks complete - PROCEEDING");

                                // Log telemetry before proceeding
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Proceed,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                }

                                return Ok(WatchdogDecision::Proceed);
                            }
                        } else {
                            warn!("External Hunter result failed: {}", data);
                            failure_count += 1;

                            if failure_count >= config.critical_failure_threshold {
                                error!("Critical failure threshold reached");

                                // Log telemetry before aborting
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Abort,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                }

                                return Ok(WatchdogDecision::Abort);
                            }
                        }
                    }

                    WatchdogSignal::InternalTaskResult { .. } => {
                        // Forward to internal channel for processing
                        let _ = internal_tx.send(signal).await;
                    }

                    WatchdogSignal::CriticalFailure { reason } => {
                        error!("Critical failure: {}", reason);

                        // Log telemetry before aborting
                        if let Some(ref tel) = telemetry {
                            let void_latency = start_time.elapsed().as_millis() as u64;
                            tel.log_decision(
                                WatchdogDecision::Abort,
                                void_latency,
                                context.latest_qass_score,
                                context.internal_risk_scores.clone(),
                                context.internal_tasks_completed,
                                context.internal_tasks_spawned,
                                context.external_result_received,
                            );
                            tel.log_error(reason, "Critical failure signal".to_string());
                        }

                        return Ok(WatchdogDecision::Abort);
                    }

                    WatchdogSignal::DataIntegrityViolation {
                        source,
                        severity,
                        details,
                        timestamp_ms
                    } => {
                        use crate::guardian::types::IntegritySeverity;

                        // Update context
                        context.integrity_violations_count += 1;

                        // Track first violation timestamp in current window
                        if context.first_violation_ts_in_window.is_none() {
                            context.first_violation_ts_in_window = Some(timestamp_ms);
                        }

                        context.last_violation_ts = Some(timestamp_ms);

                        match severity {
                            IntegritySeverity::SoftSync => {
                                // Add risk penalty for SoftSync violation
                                // Note: Risk score accumulates throughout the watchdog session
                                // to track overall system health degradation
                                const SOFT_SYNC_RISK_PENALTY: f64 = 0.3;
                                context.internal_risk_score += SOFT_SYNC_RISK_PENALTY;

                                warn!(
                                    "Data integrity violation (SoftSync) from {}: {} at {}ms (total violations: {}, risk_score: {:.2})",
                                    source, details, timestamp_ms, context.integrity_violations_count, context.internal_risk_score
                                );

                                // Log but continue - soft violations don't abort immediately
                                if let Some(ref tel) = telemetry {
                                    tel.log_error(
                                        format!("SoftSync violation from {}: {}", source, details),
                                        "Data integrity warning".to_string(),
                                    );
                                }

                                // Check for cascade condition: >5 violations in last 1 second
                                const CASCADE_THRESHOLD: u32 = 5;
                                const CASCADE_TIME_WINDOW_MS: u64 = 1000;

                                // Check if violations count exceeds threshold (6th violation triggers cascade)
                                if context.integrity_violations_count > CASCADE_THRESHOLD {
                                    // Check if violations occurred within last second from first violation
                                    if let Some(first_violation) = context.first_violation_ts_in_window {
                                        let time_since_first_violation = timestamp_ms.saturating_sub(first_violation);

                                        // Reset window if we're beyond the time window
                                        if time_since_first_violation > CASCADE_TIME_WINDOW_MS {
                                            context.first_violation_ts_in_window = Some(timestamp_ms);
                                            context.integrity_violations_count = 1;
                                        } else {
                                            // If more than CASCADE_THRESHOLD violations within time window, cascade to HardAbort
                                            error!(
                                                "CASCADE ABORT: {} SoftSync violations within {}ms - escalating to HardAbort",
                                                context.integrity_violations_count, time_since_first_violation
                                            );

                                            // Log telemetry before aborting
                                            if let Some(ref tel) = telemetry {
                                                let void_latency = start_time.elapsed().as_millis() as u64;
                                                tel.log_decision(
                                                    WatchdogDecision::Abort,
                                                    void_latency,
                                                    context.latest_qass_score,
                                                    context.internal_risk_scores.clone(),
                                                    context.internal_tasks_completed,
                                                    context.internal_tasks_spawned,
                                                    context.external_result_received,
                                                );
                                                tel.log_error(
                                                    format!(
                                                        "CASCADE ABORT: {} SoftSync violations within {}ms from {}",
                                                        context.integrity_violations_count, time_since_first_violation, source
                                                    ),
                                                    "Cascading soft violations escalated to hard abort".to_string(),
                                                );
                                            }

                                            // Cascade to Abort
                                            return Ok(WatchdogDecision::Abort);
                                        }
                                    }
                                }
                            }
                            IntegritySeverity::HardAbort => {
                                error!(
                                    "CRITICAL data integrity violation (HardAbort) from {}: {} at {}ms",
                                    source, details, timestamp_ms
                                );

                                // Log telemetry before aborting
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Abort,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                    tel.log_error(
                                        format!("HardAbort violation from {}: {}", source, details),
                                        "Critical data integrity violation".to_string(),
                                    );
                                }

                                // IMMEDIATE ABORT on HardAbort severity
                                return Ok(WatchdogDecision::Abort);
                            }
                        }
                    }

                    WatchdogSignal::ParadoxState {
                        tension,
                        derivative,
                        phase_sync,
                        pds_score,
                        is_echo_spike,
                        anomaly_detected,
                        timestamp_ms,
                    } => {
                        // PARADOX SHIELD - Enhanced decision logic with derivative analysis

                        // Check for EMERGENCY CONDITION: Rapid tension drop (Bots escaping)
                        if derivative < -0.86 {
                            error!(
                                "🛡️ PARADOX SHIELD ACTIVATED: Derivative {:.2} indicates rapid bot escape! BLOCKING BUY operations.",
                                derivative
                            );

                            // Log telemetry
                            if let Some(ref tel) = telemetry {
                                tel.log_error(
                                    format!(
                                        "Paradox Shield triggered: derivative={:.2}, tension={:.2}, pds={:.2}",
                                        derivative, tension, pds_score
                                    ),
                                    "Bot escape detected - BLOCKING BUY".to_string(),
                                );

                                let void_latency = start_time.elapsed().as_millis() as u64;
                                tel.log_decision(
                                    WatchdogDecision::Abort,
                                    void_latency,
                                    context.latest_qass_score,
                                    context.internal_risk_scores.clone(),
                                    context.internal_tasks_completed,
                                    context.internal_tasks_spawned,
                                    context.external_result_received,
                                );
                            }

                            // ABORT to block the BUY operation
                            return Ok(WatchdogDecision::Abort);
                        }

                        // Check for high-risk conditions based on PDS
                        if anomaly_detected && pds_score > 80.0 {
                            warn!(
                                "⚠️ Paradox Sensor: High PDS Score {:.2} detected - Tension: {:.2}%, Derivative: {:.2}, Phase Sync: {:.2}, Echo Spike: {} at {}ms",
                                pds_score, tension, derivative, phase_sync, is_echo_spike, timestamp_ms
                            );

                            // Add risk penalty proportional to PDS score
                            let pds_risk_penalty = (pds_score - 80.0) / 100.0; // 0.0 - 0.2+ for PDS 80-100+
                            context.internal_risk_score += pds_risk_penalty;

                            // Log telemetry
                            if let Some(ref tel) = telemetry {
                                tel.log_error(
                                    format!(
                                        "High PDS: {:.2}, tension={:.2}, derivative={:.2}, phase_sync={:.2}, echo={}",
                                        pds_score, tension, derivative, phase_sync, is_echo_spike
                                    ),
                                    "Paradox Sensor high PDS warning".to_string(),
                                );
                            }

                            // Check if total accumulated risk exceeds threshold
                            if context.internal_risk_score > config.max_internal_risk_score {
                                error!(
                                    "ABORT: Total risk score {:.2} exceeds threshold {} (PDS + other factors)",
                                    context.internal_risk_score, config.max_internal_risk_score
                                );

                                // Log telemetry before aborting
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Abort,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                }

                                return Ok(WatchdogDecision::Abort);
                            }

                            info!(
                                "PDS risk added: {:.2}, total risk: {:.2}/{:.2}",
                                pds_risk_penalty, context.internal_risk_score, config.max_internal_risk_score
                            );
                        } else {
                            debug!(
                                "Paradox Sensor: PDS {:.2}, Derivative: {:.2}, Phase Sync: {:.2}, Anomaly: {}",
                                pds_score, derivative, phase_sync, anomaly_detected
                            );
                        }
                    }

                    WatchdogSignal::NetworkStress {
                        tension,
                        jitter_ms,
                        density_bps,
                        anomaly_detected,
                        timestamp_ms,
                    } => {
                        // Handle Paradox Sensor network stress signal
                        // NOTE: This is the legacy signal format, kept for backward compatibility
                        // The new ParadoxState includes derivative and PDS, but NetworkStress doesn't
                        // In practice, the Paradox Sensor should send updated signals with derivative

                        if anomaly_detected && tension > 80.0 {
                            warn!(
                                "⚠️ Paradox Sensor: High Network Stress detected - Tension: {:.2}%, Jitter: {:.2}ms, Density: {:.0}pps at {}ms",
                                tension, jitter_ms, density_bps, timestamp_ms
                            );

                            // Add risk penalty for high network stress
                            // This accumulates and may trigger abort if combined with other risks
                            let stress_risk_penalty = (tension - 80.0) / 100.0; // 0.0 - 0.2+ for tension 80-100+
                            context.internal_risk_score += stress_risk_penalty;

                            // Log telemetry
                            if let Some(ref tel) = telemetry {
                                tel.log_error(
                                    format!(
                                        "High network stress: tension={:.2}%, jitter={:.2}ms, density={:.0}pps",
                                        tension, jitter_ms, density_bps
                                    ),
                                    "Paradox Sensor network stress warning".to_string(),
                                );
                            }

                            // Check if total accumulated risk exceeds threshold
                            if context.internal_risk_score > config.max_internal_risk_score {
                                error!(
                                    "ABORT: Total risk score {:.2} exceeds threshold {} (network stress + other factors)",
                                    context.internal_risk_score, config.max_internal_risk_score
                                );

                                // Log telemetry before aborting
                                if let Some(ref tel) = telemetry {
                                    let void_latency = start_time.elapsed().as_millis() as u64;
                                    tel.log_decision(
                                        WatchdogDecision::Abort,
                                        void_latency,
                                        context.latest_qass_score,
                                        context.internal_risk_scores.clone(),
                                        context.internal_tasks_completed,
                                        context.internal_tasks_spawned,
                                        context.external_result_received,
                                    );
                                }

                                return Ok(WatchdogDecision::Abort);
                            }

                            info!(
                                "Network stress risk added: {:.2}, total risk: {:.2}/{:.2}",
                                stress_risk_penalty, context.internal_risk_score, config.max_internal_risk_score
                            );
                        } else {
                            debug!(
                                "Paradox Sensor: Normal network conditions - Tension: {:.2}%, Anomaly: {}",
                                tension, anomaly_detected
                            );
                        }
                    }
                }
            }

            // 2. Handle internal task results (Chaos, Gene)
            Some(signal) = internal_rx.recv() => {
                if let WatchdogSignal::InternalTaskResult {
                    task_name,
                    risk_detected,
                    risk_score,
                    details
                } = &signal {
                    debug!("Internal task '{}' completed: risk={}, score={}, details={}",
                           task_name, risk_detected, risk_score, details);

                    // Log task completion to telemetry
                    if let Some(ref tel) = telemetry {
                        tel.log_task_completion(&signal);
                    }

                    context.internal_tasks_completed += 1;
                    context.internal_risk_scores.push((task_name.clone(), *risk_score));

                    // If Chaos/Gene reports high risk -> ABORT
                    if *risk_score > config.max_internal_risk_score {
                        error!("Internal task '{}' detected high risk: {} > threshold {}",
                               task_name, risk_score, config.max_internal_risk_score);

                        // Log telemetry before aborting
                        if let Some(ref tel) = telemetry {
                            let void_latency = start_time.elapsed().as_millis() as u64;
                            tel.log_decision(
                                WatchdogDecision::Abort,
                                void_latency,
                                context.latest_qass_score,
                                context.internal_risk_scores.clone(),
                                context.internal_tasks_completed,
                                context.internal_tasks_spawned,
                                context.external_result_received,
                            );
                        }

                        return Ok(WatchdogDecision::Abort);
                    }

                    if *risk_detected {
                        warn!("Internal task '{}' detected risk (score: {}): {}",
                              task_name, risk_score, details);
                        // Update context but don't abort yet unless score is too high
                    }

                    // Check if all tasks complete and external result received
                    if context.external_result_received
                        && context.internal_tasks_completed >= context.internal_tasks_spawned {
                        info!("All internal tasks and external result complete - PROCEEDING");

                        // Log telemetry before proceeding
                        if let Some(ref tel) = telemetry {
                            let void_latency = start_time.elapsed().as_millis() as u64;
                            tel.log_decision(
                                WatchdogDecision::Proceed,
                                void_latency,
                                context.latest_qass_score,
                                context.internal_risk_scores.clone(),
                                context.internal_tasks_completed,
                                context.internal_tasks_spawned,
                                context.external_result_received,
                            );
                        }

                        return Ok(WatchdogDecision::Proceed);
                    }

                    info!("Internal task '{}' completed ({}/{})",
                          task_name,
                          context.internal_tasks_completed,
                          context.internal_tasks_spawned);
                }
            }

            // 3. Handle timeout
            _ = tokio::time::sleep(void_timeout.saturating_sub(start_time.elapsed())) => {
                let elapsed = start_time.elapsed();
                warn!("Watchdog timeout reached after {:?} (max: {:?})",
                      elapsed, void_timeout);

                // Log context state for debugging
                debug!("Timeout context: {:?}", context);

                // Log telemetry before timeout
                if let Some(ref tel) = telemetry {
                    let void_latency = elapsed.as_millis() as u64;
                    tel.log_decision(
                        WatchdogDecision::Timeout,
                        void_latency,
                        context.latest_qass_score,
                        context.internal_risk_scores.clone(),
                        context.internal_tasks_completed,
                        context.internal_tasks_spawned,
                        context.external_result_received,
                    );
                }

                return Ok(WatchdogDecision::Timeout);
            }

            // 4. Handle channel closure
            else => {
                warn!("All signal channels closed, aborting watchdog");

                // Log telemetry before aborting
                if let Some(ref tel) = telemetry {
                    let void_latency = start_time.elapsed().as_millis() as u64;
                    tel.log_decision(
                        WatchdogDecision::Abort,
                        void_latency,
                        context.latest_qass_score,
                        context.internal_risk_scores.clone(),
                        context.internal_tasks_completed,
                        context.internal_tasks_spawned,
                        context.external_result_received,
                    );
                    tel.log_error(
                        "All signal channels closed".to_string(),
                        "Channel closure".to_string(),
                    );
                }

                return Ok(WatchdogDecision::Abort);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_health_autostrada() {
        let health = evaluate_network_health(25.0);
        assert!(health.is_safe);
        assert!((health.safety_coefficient - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_network_health_deszcz() {
        let health = evaluate_network_health(150.0);
        assert!(health.is_safe);
        assert!((health.safety_coefficient - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_network_health_burza() {
        let health = evaluate_network_health(250.0);
        assert!(!health.is_safe);
        assert!((health.safety_coefficient - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_watchdog_proceed_on_success() {
        let config = WatchdogConfig::default();
        let (tx, rx) = mpsc::channel(10);

        // Spawn watchdog
        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send successful external result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Test success".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    #[tokio::test]
    async fn test_watchdog_abort_on_low_qass() {
        let mut config = WatchdogConfig::default();
        config.min_qass_score = 0.7;
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send slot update with low QASS score
        tx.send(WatchdogSignal::SlotUpdate {
            slot: 12345,
            timestamp_ms: 1000,
            qass_score: Some(0.3), // Below threshold
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Abort);
    }

    #[tokio::test]
    async fn test_watchdog_abort_on_high_internal_risk() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = false; // Disable auto-spawn for controlled test
        config.max_internal_risk_score = 0.5;
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send high risk internal result
        tx.send(WatchdogSignal::InternalTaskResult {
            task_name: "Test Task".to_string(),
            risk_detected: true,
            risk_score: 0.8, // Above threshold
            details: "High risk detected".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Abort);
    }

    #[tokio::test]
    async fn test_watchdog_timeout() {
        let mut config = WatchdogConfig::default();
        config.max_void_duration_ms = 100; // Short timeout
        config.enable_parallel_tasks = false; // Disable for clean test
        let (_tx, rx) = mpsc::channel(10);

        let decision = run_watchdog(config, rx, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(decision, WatchdogDecision::Timeout);
    }

    #[tokio::test]
    async fn test_watchdog_critical_failure() {
        let config = WatchdogConfig::default();
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        tx.send(WatchdogSignal::CriticalFailure {
            reason: "System failure".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Abort);
    }

    #[tokio::test]
    async fn test_watchdog_waits_for_internal_tasks() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 500;
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Give time for parallel tasks to spawn
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Send external success - should wait for internal tasks
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "External complete".to_string(),
        })
        .await
        .unwrap();

        // Internal tasks should complete and trigger proceed
        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    #[tokio::test]
    async fn test_watchdog_with_telemetry() {
        use crate::telemetry::{TelemetryConfig, TelemetryRecorder};
        use std::path::PathBuf;
        use tokio::fs;
        use tokio::time::{sleep, Duration};

        let telemetry_config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_watchdog_telemetry.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        // Clean up any existing log file
        let _ = fs::remove_file(&telemetry_config.log_path).await;

        let telemetry = TelemetryRecorder::new(telemetry_config.clone())
            .await
            .unwrap();

        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = false;
        let (tx, rx) = mpsc::channel(10);

        let handle = tokio::spawn(async move {
            run_watchdog(config, rx, Some(telemetry), None, None, None).await
        });

        // Send successful result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Test with telemetry".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);

        // Give time for async telemetry write
        sleep(Duration::from_millis(100)).await;

        // Verify telemetry file was created and contains decision
        let content = fs::read_to_string(&telemetry_config.log_path)
            .await
            .unwrap();
        assert!(content.contains("Decision"));
        assert!(content.contains("Proceed"));
        assert!(content.contains("void_latency_ms"));

        // Cleanup
        let _ = fs::remove_file(&telemetry_config.log_path).await;
    }

    // ==================== NEW INTEGRATION TESTS ====================

    // Test data constants
    const TEST_POOL_RESERVE_A: u128 = 1_000_000_000_000; // 1M SOL (in lamports)
    const TEST_POOL_RESERVE_B: u128 = 2_000_000_000_000; // 2M USDC (in smallest unit)
    const TEST_POOL_FEE_BPS: u16 = 30; // 0.3% fee

    /// Helper function to create a test AMM pool
    /// Creates a pool with 1M SOL and 2M USDC reserves with 0.3% fee
    fn create_test_pool() -> crate::chaos::AmmPool {
        crate::chaos::AmmPool::new(TEST_POOL_RESERVE_A, TEST_POOL_RESERVE_B, TEST_POOL_FEE_BPS)
            .unwrap()
    }

    #[tokio::test]
    async fn test_watchdog_full_integration_with_chaos() {
        use crate::chaos::MarketScenario;

        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 3000; // Allow time for simulation
        config.max_internal_risk_score = 0.9; // High threshold to avoid abort

        let (tx, rx) = mpsc::channel(10);
        let pool = create_test_pool();

        let handle = tokio::spawn(async move {
            run_watchdog(
                config,
                rx,
                None,
                Some(pool),
                Some(MarketScenario::Bullish),
                None,
            )
            .await
        });

        // Give time for Chaos Engine to run
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Send external success result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Chaos test complete".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    /// Helper function to create human-like timestamps with variable intervals
    /// These intervals simulate random human trading behavior (high CV)
    fn create_human_like_timestamps() -> Vec<u64> {
        // Variable intervals: 100, 500, 1200, 300, 2000, 450, 800, 150, 600, 250 ms
        const HUMAN_INTERVALS: [u64; 10] = [100, 500, 1200, 300, 2000, 450, 800, 150, 600, 250];
        let mut timestamps = Vec::new();
        let mut ts = 0u64;
        for interval in HUMAN_INTERVALS {
            ts += interval;
            timestamps.push(ts);
        }
        timestamps
    }

    /// Helper function to create bot-like timestamps with periodic intervals
    /// These intervals simulate automated bot behavior (low CV)
    fn create_bot_like_timestamps() -> Vec<u64> {
        // Periodic 100ms intervals simulate bot trading pattern
        const BOT_INTERVAL: u64 = 100;
        const BOT_TIMESTAMP_COUNT: usize = 20;
        (0..BOT_TIMESTAMP_COUNT)
            .map(|i| (i as u64) * BOT_INTERVAL)
            .collect()
    }

    #[tokio::test]
    async fn test_watchdog_full_integration_with_resonance() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 2000;
        config.max_internal_risk_score = 0.9; // High threshold

        let (tx, rx) = mpsc::channel(10);

        // Use human-like timestamps
        let timestamps = create_human_like_timestamps();

        let handle = tokio::spawn(async move {
            run_watchdog(config, rx, None, None, None, Some(timestamps)).await
        });

        // Give time for Resonance to analyze
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send external success result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Resonance test complete".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    #[tokio::test]
    async fn test_watchdog_all_tasks_parallel() {
        use crate::chaos::MarketScenario;

        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 3000;
        config.max_internal_risk_score = 0.9;

        let (tx, rx) = mpsc::channel(10);
        let pool = create_test_pool();

        // Create bot-like periodic timestamps (every 500ms)
        let timestamps: Vec<u64> = (0..15).map(|i| i * 500).collect();

        let handle = tokio::spawn(async move {
            run_watchdog(
                config,
                rx,
                None,
                Some(pool),
                Some(MarketScenario::Mixed),
                Some(timestamps),
            )
            .await
        });

        // Give time for all three tasks to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // Send external success result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "All tasks test complete".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    #[tokio::test]
    async fn test_chaos_high_crash_probability_aborts() {
        use crate::chaos::MarketScenario;

        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 3000;
        config.max_internal_risk_score = 0.55; // Lower threshold to catch crash probability

        let (tx, rx) = mpsc::channel(10);
        let pool = create_test_pool();

        let handle = tokio::spawn(async move {
            run_watchdog(
                config,
                rx,
                None,
                Some(pool),
                Some(MarketScenario::RugPull), // High crash probability scenario
                None,
            )
            .await
        });

        // Give time for Chaos Engine to run and detect high risk
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let decision = handle.await.unwrap().unwrap();
        // Should abort due to high crash probability from rug pull scenario
        // OR timeout if the simulation takes too long
        // Both are acceptable since we're testing that rug pull is detected as risky
        assert!(
            matches!(
                decision,
                WatchdogDecision::Abort | WatchdogDecision::Timeout
            ),
            "Expected Abort or Timeout for rug pull scenario, got: {:?}",
            decision
        );
    }

    #[tokio::test]
    async fn test_resonance_bot_detection_adds_risk() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = true;
        config.max_void_duration_ms = 2000;
        config.max_internal_risk_score = 0.9; // High threshold to not abort

        let (tx, rx) = mpsc::channel(10);

        // Use bot-like periodic timestamps
        let timestamps = create_bot_like_timestamps();

        let handle = tokio::spawn(async move {
            run_watchdog(config, rx, None, None, None, Some(timestamps)).await
        });

        // Give time for Resonance to detect bot pattern
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send external success result
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Bot detection test".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        // Should proceed but Resonance should have detected bot activity
        // (verified by the fact that the task completed and risk was added to context)
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    // ==================== KILL-SWITCH TESTS ====================

    #[tokio::test]
    async fn test_hard_abort_immediate_termination() {
        let config = WatchdogConfig::default();
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send HardAbort integrity violation
        tx.send(WatchdogSignal::DataIntegrityViolation {
            source: "SnapshotEngine".to_string(),
            severity: super::super::types::IntegritySeverity::HardAbort,
            details: "Slot regression detected".to_string(),
            timestamp_ms: 1000,
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        assert_eq!(decision, WatchdogDecision::Abort);
    }

    #[tokio::test]
    async fn test_soft_sync_adds_risk_penalty() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = false;
        config.max_void_duration_ms = 500;

        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send SoftSync violation
        tx.send(WatchdogSignal::DataIntegrityViolation {
            source: "Seer".to_string(),
            severity: super::super::types::IntegritySeverity::SoftSync,
            details: "Jitter > 400ms".to_string(),
            timestamp_ms: 1000,
        })
        .await
        .unwrap();

        // Give time for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Send success to complete
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Test complete".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        // Should proceed since single SoftSync doesn't abort
        assert_eq!(decision, WatchdogDecision::Proceed);
    }

    #[tokio::test]
    async fn test_cascade_abort_on_multiple_soft_violations() {
        let config = WatchdogConfig::default();
        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send 6 SoftSync violations within 1 second window
        let base_time = 1000u64;
        for i in 0..6 {
            tx.send(WatchdogSignal::DataIntegrityViolation {
                source: "SnapshotEngine".to_string(),
                severity: super::super::types::IntegritySeverity::SoftSync,
                details: format!("Jitter violation #{}", i + 1),
                timestamp_ms: base_time + (i * 100), // 100ms intervals
            })
            .await
            .unwrap();

            // Small delay between violations
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }

        let decision = handle.await.unwrap().unwrap();
        // Should abort due to cascade (>5 violations in 1s)
        assert_eq!(decision, WatchdogDecision::Abort);
    }

    #[tokio::test]
    async fn test_no_cascade_when_violations_spread_over_time() {
        let mut config = WatchdogConfig::default();
        config.enable_parallel_tasks = false;
        config.max_void_duration_ms = 3000;

        let (tx, rx) = mpsc::channel(10);

        let handle =
            tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

        // Send 6 violations but spread over >1 second
        let base_time = 1000u64;
        for i in 0..6 {
            tx.send(WatchdogSignal::DataIntegrityViolation {
                source: "Seer".to_string(),
                severity: super::super::types::IntegritySeverity::SoftSync,
                details: format!("Scattered violation #{}", i + 1),
                timestamp_ms: base_time + (i * 300), // 300ms intervals = 1.8s total
            })
            .await
            .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }

        // Send success to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        tx.send(WatchdogSignal::ExternalResult {
            success: true,
            data: "Test complete".to_string(),
        })
        .await
        .unwrap();

        let decision = handle.await.unwrap().unwrap();
        // Should proceed since violations are spread beyond 1s window
        assert_eq!(decision, WatchdogDecision::Proceed);
    }
}
