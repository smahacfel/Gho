//! Follow-up Scoring Module
//!
//! This module implements the follow-up scoring loop that re-evaluates candidates
//! at fixed intervals (1s, 5s, 30s, 60s) after the initial buy decision.
//!
//! # Architecture
//!
//! ```text
//! Initial Score (T < 2s) --> BUY --> Spawn Follow-up Task
//!                                           |
//!                                           v
//!                                    +-------------+
//!                                    | t = 1s      |
//!                                    | Recompute   |
//!                                    | Score       |
//!                                    +-------------+
//!                                           |
//!                                           v
//!                                    +-------------+
//!                                    | t = 5s      |
//!                                    | Check MCI   |
//!                                    | Check QEDD  |
//!                                    +-------------+
//!                                           |
//!                                           v
//!                                    +-------------+
//!                                    | t = 30s     |
//!                                    | Full QEDD   |
//!                                    | Chaos Sim   |
//!                                    +-------------+
//!                                           |
//!                                           v
//!                                    +-------------+
//!                                    | t = 60s     |
//!                                    | Gene Check  |
//!                                    | Final       |
//!                                    +-------------+
//! ```
//!
//! # Decision Logic
//!
//! At each interval, the system:
//! 1. Recomputes scores with updated market data
//! 2. Checks for corrections (MCI drop, QEDD spike, etc.)
//! 3. Makes a decision (HOLD, SELL, SCALE_OUT)
//! 4. Logs everything to DecisionLogger

use crate::oracle::{
    CorrectionReason, DecisionLogger, DecisionType, FollowupScore, InitialComponents,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, info, warn};

/// Configuration for follow-up scoring
#[derive(Debug, Clone)]
pub struct FollowupConfig {
    /// Enable follow-up scoring
    pub enabled: bool,
    /// Intervals at which to rescore (milliseconds)
    pub intervals_ms: Vec<u64>,
    /// MCI drop threshold for triggering corrections
    pub mci_drop_threshold: f32,
    /// QEDD λ spike threshold
    pub qedd_lambda_spike_threshold: f32,
    /// QEDD survival drop threshold (percentage)
    pub qedd_survival_drop_pct: f32,
    /// Chaos loss probability threshold for high risk
    pub chaos_loss_prob_threshold: f32,
    /// GeneMapper match score threshold for veto
    pub gene_match_threshold: f32,
    /// Exit score threshold (below this = sell)
    pub exit_threshold: u8,
    /// Score drop percentage for triggering sell
    pub score_drop_pct_threshold: f32,
}

impl Default for FollowupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            intervals_ms: vec![1000, 5000, 30000, 60000], // 1s, 5s, 30s, 60s
            mci_drop_threshold: 0.35,                     // Updated from 0.50 - now configurable
            qedd_lambda_spike_threshold: 2.0,
            qedd_survival_drop_pct: 0.50, // Updated from 0.30 - now configurable
            chaos_loss_prob_threshold: 0.60,
            gene_match_threshold: 0.70,
            exit_threshold: 40,
            score_drop_pct_threshold: 0.30, // 30% drop from initial
        }
    }
}

impl FollowupConfig {
    /// Create FollowupConfig from HyperPredictionConfig
    ///
    /// Maps the followup_scoring section from HyperPredictionConfig to this module's
    /// FollowupConfig, preserving other fields with their defaults.
    pub fn from_hyper_prediction_config(
        hp_config: &crate::oracle::hyper_prediction::config::FollowupScoringConfig,
    ) -> Self {
        let mut config = Self::default();
        config.mci_drop_threshold = hp_config.mci_drop_threshold;
        config.qedd_survival_drop_pct = hp_config.qedd_survival_drop_pct;
        config.enabled = hp_config.enable_followup_penalties;
        config
    }
}

/// Context for a follow-up scoring task
#[derive(Debug, Clone)]
pub struct FollowupContext {
    /// Candidate ID
    pub candidate_id: String,
    /// Initial score
    pub initial_score: u8,
    /// Initial components
    pub initial_components: InitialComponents,
    /// Start time
    pub start_time: Instant,
    /// Configuration
    pub config: FollowupConfig,
}

/// Follow-up scoring manager
pub struct FollowupScoringManager {
    config: FollowupConfig,
    logger: Arc<DecisionLogger>,
}

impl FollowupScoringManager {
    /// Create a new follow-up scoring manager
    pub fn new(config: FollowupConfig, logger: Arc<DecisionLogger>) -> Self {
        Self { config, logger }
    }

    /// Spawn a follow-up task for a candidate
    ///
    /// This spawns an async task that will periodically rescore the candidate
    /// at the configured intervals and log all decisions.
    pub fn spawn_followup_task(&self, context: FollowupContext) {
        if !self.config.enabled {
            debug!("Follow-up scoring disabled, skipping task spawn");
            return;
        }

        let config = self.config.clone();
        let logger = Arc::clone(&self.logger);

        tokio::spawn(async move {
            if let Err(e) = run_followup_loop(context, config, logger).await {
                warn!("Follow-up scoring loop error: {}", e);
            }
        });
    }
}

/// Run the follow-up scoring loop
async fn run_followup_loop(
    context: FollowupContext,
    config: FollowupConfig,
    _logger: Arc<DecisionLogger>,
) -> Result<()> {
    info!(
        "Starting follow-up scoring for candidate: {}",
        context.candidate_id
    );

    let mut last_score = context.initial_score;
    let mut last_mci: Option<f32> = context.initial_components.mci;
    let mut last_qedd_survival: Option<f32> = context.initial_components.qedd_survival_30s;
    let mut last_qedd_lambda: Option<f32> = None;

    for &interval_ms in &config.intervals_ms {
        // Sleep until next interval
        let elapsed = context.start_time.elapsed();
        let target = Duration::from_millis(interval_ms);

        if elapsed < target {
            sleep(target - elapsed).await;
        }

        debug!(
            "Follow-up scoring at {}ms for candidate: {}",
            interval_ms, context.candidate_id
        );

        // TODO: Production Integration Points
        // In a real implementation, this would:
        // 1. Fetch updated market data from SnapshotEngine
        // 2. Recompute QASS with new waves
        // 3. Query QEDD for updated survival/lambda
        // 4. Check MCI for coherence
        // 5. Run Chaos Engine sims if needed (30s+)
        // 6. Check GeneMapper for new patterns
        //
        // For now, we use placeholder logic that demonstrates the structure
        // See compute_followup_score() for simulation logic

        let (new_score, corrections, decision) = compute_followup_score(
            &context,
            &config,
            last_score,
            last_mci,
            last_qedd_survival,
            last_qedd_lambda,
            interval_ms,
        );

        // Create follow-up score entry
        let followup = FollowupScore {
            t_ms: interval_ms,
            score: new_score,
            reason: generate_reason(&corrections, &decision),
            corrections: corrections.clone(),
            decision: decision.clone(),
            components: None, // Would include updated components in real impl
            confidence: None, // Would be calculated in real impl
        };

        // Update state for next iteration
        last_score = new_score;

        // Log this follow-up
        // TODO: Update the existing OracleDecisionLog with this followup score
        // In production, maintain a reference to the log and call log.add_followup(followup)
        // then persist with logger.log(log).await
        debug!(
            "Candidate {} at {}ms: score={}, decision={:?}, corrections={}",
            context.candidate_id,
            interval_ms,
            new_score,
            decision,
            corrections.len()
        );

        // Check if we should exit early
        if matches!(decision, DecisionType::Sell) {
            info!(
                "Early exit: Sell decision at {}ms for candidate {}",
                interval_ms, context.candidate_id
            );
            break;
        }
    }

    info!(
        "Follow-up scoring completed for candidate: {}",
        context.candidate_id
    );

    Ok(())
}

/// Compute follow-up score with corrections
///
/// This is a placeholder that demonstrates the structure.
/// In production, this would integrate with all Oracle modules.
fn compute_followup_score(
    context: &FollowupContext,
    config: &FollowupConfig,
    last_score: u8,
    last_mci: Option<f32>,
    last_qedd_survival: Option<f32>,
    last_qedd_lambda: Option<f32>,
    interval_ms: u64,
) -> (u8, Vec<CorrectionReason>, DecisionType) {
    let mut corrections = Vec::new();
    let mut score = last_score;
    let mut decision = DecisionType::Hold;

    // Simulate component updates (would be real queries in production)

    // At 1s: Check for quick QASS changes
    if interval_ms == 1000 {
        // Simulate small QASS fluctuation
        if context.initial_components.qass_score > 75.0 {
            let drop_pct = 3.0;
            corrections.push(CorrectionReason::QassScoreDrop {
                old_score: context.initial_components.qass_score,
                new_score: context.initial_components.qass_score * 0.97,
                drop_pct,
                impact: -2,
            });
            score = score.saturating_sub(2);
        }
    }

    // At 5s: Check MCI and QEDD
    if interval_ms == 5000 {
        if let Some(mci) = last_mci {
            // Simulate MCI drop
            let new_mci = mci * 0.85; // 15% drop
            if new_mci < config.mci_drop_threshold {
                corrections.push(CorrectionReason::MciDrop {
                    old_value: mci,
                    new_value: new_mci,
                    threshold: config.mci_drop_threshold,
                    impact: -15,
                });
                score = score.saturating_sub(15);
                decision = DecisionType::ScaleOut;
            }
        }

        if let Some(survival) = last_qedd_survival {
            // Simulate QEDD survival drop
            let new_survival = survival * 0.80; // 20% drop
            let drop_pct = (survival - new_survival) / survival;
            if drop_pct > config.qedd_survival_drop_pct {
                corrections.push(CorrectionReason::QeddSurvivalDrop {
                    old_survival: survival,
                    new_survival,
                    horizon_s: 30,
                    impact: -10,
                });
                score = score.saturating_sub(10);
            }
        }
    }

    // At 30s: Full QEDD analysis + Chaos Engine
    if interval_ms == 30000 {
        // Simulate QEDD λ spike
        let old_lambda = last_qedd_lambda.unwrap_or(0.5);
        let new_lambda = old_lambda * 2.5; // Spike

        if new_lambda > config.qedd_lambda_spike_threshold {
            corrections.push(CorrectionReason::QeddLambdaSpike {
                old_lambda,
                new_lambda,
                threshold: config.qedd_lambda_spike_threshold,
                impact: -25,
            });
            score = score.saturating_sub(25);
            decision = DecisionType::Sell;
        }

        // Simulate Chaos Engine result
        if context.initial_components.chaos_loss_prob.unwrap_or(0.0) > 0.10 {
            let loss_prob = 0.65; // High risk
            if loss_prob > config.chaos_loss_prob_threshold {
                corrections.push(CorrectionReason::ChaosHighRisk {
                    loss_prob,
                    threshold: config.chaos_loss_prob_threshold,
                    impact: -20,
                });
                score = score.saturating_sub(20);
                if decision != DecisionType::Sell {
                    decision = DecisionType::Sell;
                }
            }
        }
    }

    // At 60s: Final GeneMapper check
    if interval_ms == 60000 {
        if let Some(gene_score) = context.initial_components.gene_match_score {
            if gene_score > config.gene_match_threshold {
                corrections.push(CorrectionReason::GeneMapperHit {
                    match_score: gene_score,
                    pattern_id: "detected_pattern".to_string(),
                    impact: -100,
                });
                score = 0;
                decision = DecisionType::Sell;
            }
        }
    }

    // Check overall score drop
    let score_drop_pct =
        (context.initial_score as f32 - score as f32) / context.initial_score as f32;
    if score_drop_pct > config.score_drop_pct_threshold && decision == DecisionType::Hold {
        decision = DecisionType::Sell;
    }

    // Check exit threshold
    if score < config.exit_threshold && decision == DecisionType::Hold {
        decision = DecisionType::Sell;
    }

    (score, corrections, decision)
}

/// Generate human-readable reason from corrections and decision
fn generate_reason(corrections: &[CorrectionReason], decision: &DecisionType) -> String {
    if corrections.is_empty() {
        return match decision {
            DecisionType::Hold => "No significant changes".to_string(),
            DecisionType::Sell => "Score below exit threshold".to_string(),
            DecisionType::ScaleOut => "Reducing position size".to_string(),
            _ => "Updated evaluation".to_string(),
        };
    }

    let reasons: Vec<String> = corrections
        .iter()
        .map(|c| match c {
            CorrectionReason::MciDrop { .. } => "MCI drop".to_string(),
            CorrectionReason::QeddLambdaSpike { .. } => "QEDD λ spike".to_string(),
            CorrectionReason::QeddSurvivalDrop { .. } => "QEDD survival decline".to_string(),
            CorrectionReason::GeneMapperHit { .. } => "GeneMapper hit".to_string(),
            CorrectionReason::GuardianAbort { .. } => "Guardian abort".to_string(),
            CorrectionReason::ChaosHighRisk { .. } => "Chaos high risk".to_string(),
            CorrectionReason::QassScoreDrop { .. } => "QASS drop".to_string(),
            CorrectionReason::ResonanceDetected { .. } => "Bot activity".to_string(),
            CorrectionReason::ShadowMigrationRisk { .. } => "Migration risk".to_string(),
            CorrectionReason::Other { description, .. } => description.clone(),
        })
        .collect();

    reasons.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{DecisionLoggerConfig, InitialComponents};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_context() -> FollowupContext {
        let initial_components = InitialComponents {
            base_shadow: 60,
            qass_score: 78.5,
            qedd_survival_30s: Some(0.71),
            mci: Some(0.74),
            chaos_loss_prob: Some(0.12),
            gene_match_score: Some(0.03),
            confidence: Some(0.85),
            extras: HashMap::new(),
        };

        FollowupContext {
            candidate_id: "test_pool_123".to_string(),
            initial_score: 62,
            initial_components,
            start_time: Instant::now(),
            config: FollowupConfig::default(),
        }
    }

    #[test]
    fn test_followup_config_defaults() {
        let config = FollowupConfig::default();

        assert!(config.enabled);
        assert_eq!(config.intervals_ms, vec![1000, 5000, 30000, 60000]);
        assert_eq!(config.mci_drop_threshold, 0.35); // Updated from 0.50
        assert_eq!(config.qedd_survival_drop_pct, 0.50); // Updated from 0.30
        assert_eq!(config.qedd_lambda_spike_threshold, 2.0);
        assert_eq!(config.exit_threshold, 40);
    }

    #[test]
    fn test_compute_followup_score_1s() {
        let context = create_test_context();
        let config = FollowupConfig::default();

        let (score, corrections, decision) = compute_followup_score(
            &context,
            &config,
            62,
            Some(0.74),
            Some(0.71),
            Some(0.5),
            1000,
        );

        // At 1s, should have minor QASS drop
        assert_eq!(corrections.len(), 1);
        assert!(matches!(
            corrections[0],
            CorrectionReason::QassScoreDrop { .. }
        ));
        assert_eq!(score, 60);
        assert_eq!(decision, DecisionType::Hold);
    }

    #[test]
    fn test_compute_followup_score_5s_mci_drop() {
        let context = create_test_context();
        let config = FollowupConfig::default();

        let (score, corrections, decision) = compute_followup_score(
            &context,
            &config,
            60,
            Some(0.74),
            Some(0.71),
            Some(0.5),
            5000,
        );

        // At 5s, MCI drops below threshold and QEDD survival drops
        assert!(corrections.len() >= 1);
        assert!(score < 60);
        // Should trigger ScaleOut due to MCI drop
        assert!(matches!(
            decision,
            DecisionType::ScaleOut | DecisionType::Hold
        ));
    }

    #[test]
    fn test_compute_followup_score_30s_lambda_spike() {
        let context = create_test_context();
        let config = FollowupConfig::default();

        let (score, corrections, decision) = compute_followup_score(
            &context,
            &config,
            45,
            Some(0.45),
            Some(0.52),
            Some(0.5),
            30000,
        );

        // At 30s, λ spikes and chaos shows high risk
        assert!(corrections.len() >= 1);
        assert!(score < 45);
        // Should trigger Sell due to λ spike
        assert_eq!(decision, DecisionType::Sell);
    }

    #[test]
    fn test_generate_reason() {
        let corrections = vec![
            CorrectionReason::MciDrop {
                old_value: 0.74,
                new_value: 0.45,
                threshold: 0.50,
                impact: -15,
            },
            CorrectionReason::QeddSurvivalDrop {
                old_survival: 0.71,
                new_survival: 0.52,
                horizon_s: 30,
                impact: -10,
            },
        ];

        let reason = generate_reason(&corrections, &DecisionType::Sell);

        assert!(reason.contains("MCI drop"));
        assert!(reason.contains("QEDD survival decline"));
    }

    #[test]
    fn test_generate_reason_no_corrections() {
        let corrections = vec![];

        let reason_hold = generate_reason(&corrections, &DecisionType::Hold);
        assert_eq!(reason_hold, "No significant changes");

        let reason_sell = generate_reason(&corrections, &DecisionType::Sell);
        assert_eq!(reason_sell, "Score below exit threshold");
    }

    #[tokio::test]
    async fn test_followup_manager_spawn() {
        let temp_dir = TempDir::new().unwrap();
        let logger_config = DecisionLoggerConfig {
            log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 100,
            enabled: true,
        };

        let logger = Arc::new(DecisionLogger::new(logger_config));
        let followup_config = FollowupConfig {
            enabled: true,
            intervals_ms: vec![100, 200], // Short intervals for testing
            ..Default::default()
        };

        let manager = FollowupScoringManager::new(followup_config, logger);

        let context = create_test_context();
        manager.spawn_followup_task(context);

        // Give time for task to run
        sleep(Duration::from_millis(500)).await;

        // Task should complete without panicking
    }

    #[tokio::test]
    async fn test_followup_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let logger_config = DecisionLoggerConfig {
            log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 100,
            enabled: true,
        };

        let logger = Arc::new(DecisionLogger::new(logger_config));
        let followup_config = FollowupConfig {
            enabled: false, // Disabled
            ..Default::default()
        };

        let manager = FollowupScoringManager::new(followup_config, logger);

        let context = create_test_context();
        manager.spawn_followup_task(context);

        // Should not spawn task
        sleep(Duration::from_millis(100)).await;
    }
}
