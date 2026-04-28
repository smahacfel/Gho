//! Tuning Service - Background Weight Optimization
//!
//! This service runs as a background task, periodically updating weights
//! based on accumulated trading outcomes. It coordinates between:
//! - HysteresisLoop (immediate feedback from trades)
//! - WeightTuner (periodic optimization with bandit algorithms)
//! - Bayesian Optimizer (long-term optimization on 12h cycles)

use crate::tuning::{
    BanditAlgorithm, FrozenParameters, TradeOutcome, TunableWeights, TuningConfig, TuningContext,
    WeightTuner,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio::time::interval;
use tracing::{debug, info, warn};

/// Message types for tuning service
#[derive(Debug)]
pub enum TuningMessage {
    /// New trade outcome to process
    TradeOutcome {
        context: TuningContext,
        outcome: TradeOutcome,
    },
    /// Request current weights
    GetWeights {
        response_tx: tokio::sync::oneshot::Sender<TunableWeights>,
    },
    /// Force weight update (for testing/manual intervention)
    ForceUpdate,
    /// Freeze weights with given values
    FreezeWeights {
        weights: TunableWeights,
        reason: String,
    },
    /// Unfreeze weights
    UnfreezeWeights,
    /// Shutdown service
    Shutdown,
}

/// Tuning Service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningServiceConfig {
    /// How often to run bandit updates (seconds)
    pub bandit_update_interval_secs: u64,

    /// How often to check for Bayesian optimization (seconds)
    pub bayesian_check_interval_secs: u64,

    /// Minimum outcomes before running bandit update
    pub min_outcomes_for_update: usize,

    /// Maximum historical outcomes to keep for Bayesian optimization
    pub max_historical_outcomes: usize,

    /// Minimum historical outcomes required to run Bayesian optimization
    pub min_historical_for_bayesian: usize,

    /// Algorithm to use for online learning
    pub bandit_algorithm: BanditAlgorithm,

    /// Enable Bayesian optimization
    pub enable_bayesian: bool,

    /// Start in dry-run mode (no actual weight changes)
    pub dry_run: bool,
}

impl Default for TuningServiceConfig {
    fn default() -> Self {
        Self {
            bandit_update_interval_secs: 180,    // 3 minutes
            bayesian_check_interval_secs: 43200, // 12 hours
            min_outcomes_for_update: 5,
            max_historical_outcomes: 10000,
            min_historical_for_bayesian: 100,
            bandit_algorithm: BanditAlgorithm::LinUCB,
            enable_bayesian: true,
            dry_run: false,
        }
    }
}

/// Statistics about the tuning service
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuningServiceStats {
    /// Total bandit updates performed
    pub bandit_updates: u64,

    /// Total Bayesian optimizations performed
    pub bayesian_optimizations: u64,

    /// Total trade outcomes processed
    pub outcomes_processed: u64,

    /// Current cumulative reward
    pub cumulative_reward: f64,

    /// Average reward per outcome
    pub average_reward: f64,

    /// Whether weights are currently frozen
    pub is_frozen: bool,

    /// Seconds since last update
    pub seconds_since_update: u64,

    /// Current weights
    pub current_weights: TunableWeights,
}

/// Tuning Service - runs as background task
pub struct TuningService {
    /// Configuration
    config: TuningServiceConfig,

    /// Weight tuner (owns bandit algorithm)
    tuner: Arc<RwLock<WeightTuner>>,

    /// Accumulated outcomes waiting for batch processing
    pending_outcomes: Arc<RwLock<Vec<(TuningContext, TradeOutcome)>>>,

    /// Historical outcomes for Bayesian optimization
    historical_outcomes: Arc<RwLock<Vec<(TuningContext, TradeOutcome)>>>,

    /// Current weights (published via watch channel)
    weights_tx: watch::Sender<TunableWeights>,

    /// Statistics
    stats: Arc<RwLock<TuningServiceStats>>,

    /// Last update timestamp
    last_update: Arc<RwLock<Instant>>,
}

impl TuningService {
    /// Create a new TuningService
    pub fn new(config: TuningServiceConfig) -> (Self, watch::Receiver<TunableWeights>) {
        let tuning_config = TuningConfig::default();
        let tuner = WeightTuner::new(tuning_config, config.bandit_algorithm);

        let initial_weights = tuner.current_weights();
        let (weights_tx, weights_rx) = watch::channel(initial_weights);

        let stats = TuningServiceStats {
            current_weights: initial_weights,
            ..Default::default()
        };

        (
            Self {
                config,
                tuner: Arc::new(RwLock::new(tuner)),
                pending_outcomes: Arc::new(RwLock::new(Vec::new())),
                historical_outcomes: Arc::new(RwLock::new(Vec::new())),
                weights_tx,
                stats: Arc::new(RwLock::new(stats)),
                last_update: Arc::new(RwLock::new(Instant::now())),
            },
            weights_rx,
        )
    }

    /// Run the tuning service as an async task
    pub async fn run(self, mut message_rx: mpsc::Receiver<TuningMessage>) {
        info!(
            "TuningService started: algorithm={:?}, bandit_interval={}s, bayesian={}, dry_run={}",
            self.config.bandit_algorithm,
            self.config.bandit_update_interval_secs,
            self.config.enable_bayesian,
            self.config.dry_run
        );

        let mut bandit_interval =
            interval(Duration::from_secs(self.config.bandit_update_interval_secs));
        let mut bayesian_interval = interval(Duration::from_secs(
            self.config.bayesian_check_interval_secs,
        ));

        loop {
            tokio::select! {
                // Handle incoming messages
                Some(msg) = message_rx.recv() => {
                    match msg {
                        TuningMessage::TradeOutcome { context, outcome } => {
                            self.handle_outcome(context, outcome);
                        }
                        TuningMessage::GetWeights { response_tx } => {
                            let weights = self.get_current_weights();
                            let _ = response_tx.send(weights);
                        }
                        TuningMessage::ForceUpdate => {
                            self.run_bandit_update();
                        }
                        TuningMessage::FreezeWeights { weights, reason } => {
                            self.freeze_weights(weights, &reason);
                        }
                        TuningMessage::UnfreezeWeights => {
                            self.unfreeze_weights();
                        }
                        TuningMessage::Shutdown => {
                            info!("TuningService shutting down");
                            break;
                        }
                    }
                }

                // Periodic bandit update
                _ = bandit_interval.tick() => {
                    self.run_bandit_update();
                }

                // Periodic Bayesian optimization check
                _ = bayesian_interval.tick() => {
                    if self.config.enable_bayesian {
                        self.check_bayesian_optimization();
                    }
                }
            }
        }

        info!("TuningService stopped");
    }

    /// Handle a new trade outcome
    fn handle_outcome(&self, context: TuningContext, outcome: TradeOutcome) {
        // Add to pending outcomes
        {
            let mut pending = self.pending_outcomes.write().unwrap();
            pending.push((context.clone(), outcome.clone()));
        }

        // Add to historical for Bayesian (with limit)
        {
            let mut historical = self.historical_outcomes.write().unwrap();
            historical.push((context, outcome));

            // Keep limited history for Bayesian optimization
            if historical.len() > self.config.max_historical_outcomes {
                historical.remove(0);
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.outcomes_processed += 1;
        }

        debug!(
            "TuningService: outcome recorded, pending={}",
            self.pending_outcomes.read().unwrap().len()
        );
    }

    /// Run bandit update on pending outcomes
    fn run_bandit_update(&self) {
        let outcomes: Vec<(TuningContext, TradeOutcome)> = {
            let mut pending = self.pending_outcomes.write().unwrap();
            std::mem::take(&mut *pending)
        };

        if outcomes.len() < self.config.min_outcomes_for_update {
            debug!(
                "TuningService: skipping bandit update, insufficient outcomes ({} < {})",
                outcomes.len(),
                self.config.min_outcomes_for_update
            );
            return;
        }

        if self.config.dry_run {
            debug!(
                "TuningService: DRY RUN - would update with {} outcomes",
                outcomes.len()
            );
            return;
        }

        // Run updates through tuner
        let mut tuner = self.tuner.write().unwrap();

        for (context, outcome) in outcomes.iter() {
            tuner.update(context, outcome);
        }

        // Get new weights and publish
        let new_weights = tuner.current_weights();
        let _ = self.weights_tx.send(new_weights);

        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.bandit_updates += 1;
            stats.current_weights = new_weights;
            stats.cumulative_reward = tuner.stats().cumulative_reward;
            stats.average_reward = tuner.stats().average_reward;
            stats.is_frozen = tuner.stats().is_frozen;
        }

        *self.last_update.write().unwrap() = Instant::now();

        info!(
            "TuningService: bandit update #{} complete. weights: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}",
            self.stats.read().unwrap().bandit_updates,
            new_weights.w_qass,
            new_weights.w_mpcf,
            new_weights.w_sobp,
            new_weights.w_iwim
        );
    }

    /// Check if Bayesian optimization should run
    fn check_bayesian_optimization(&self) {
        let mut tuner = self.tuner.write().unwrap();

        if !tuner.should_run_bayesian() {
            debug!("TuningService: Bayesian optimization not yet due");
            return;
        }

        let historical = self.historical_outcomes.read().unwrap();

        if historical.len() < self.config.min_historical_for_bayesian {
            debug!(
                "TuningService: insufficient historical data for Bayesian ({} < {})",
                historical.len(),
                self.config.min_historical_for_bayesian
            );
            return;
        }

        if self.config.dry_run {
            info!(
                "TuningService: DRY RUN - would run Bayesian optimization with {} outcomes",
                historical.len()
            );
            return;
        }

        info!(
            "TuningService: starting Bayesian optimization with {} historical outcomes",
            historical.len()
        );

        if let Some(result) = tuner.run_bayesian_optimization(&historical) {
            let new_weights = tuner.current_weights();
            let _ = self.weights_tx.send(new_weights);

            let mut stats = self.stats.write().unwrap();
            stats.bayesian_optimizations += 1;
            stats.current_weights = new_weights;

            info!(
                "TuningService: Bayesian optimization #{} complete. EI={:.4}, weights: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}",
                stats.bayesian_optimizations,
                result.expected_improvement,
                new_weights.w_qass,
                new_weights.w_mpcf,
                new_weights.w_sobp,
                new_weights.w_iwim
            );
        } else {
            warn!("TuningService: Bayesian optimization returned no result");
        }
    }

    /// Get current weights
    fn get_current_weights(&self) -> TunableWeights {
        self.tuner.read().unwrap().current_weights()
    }

    /// Freeze weights
    fn freeze_weights(&self, weights: TunableWeights, reason: &str) {
        let mut tuner = self.tuner.write().unwrap();
        let params = FrozenParameters {
            enabled: true,
            weights,
            reason: reason.to_string(),
            frozen_at: chrono::Utc::now(),
        };
        tuner.freeze_parameters(params);

        let _ = self.weights_tx.send(weights);

        let mut stats = self.stats.write().unwrap();
        stats.is_frozen = true;
        stats.current_weights = weights;

        warn!("TuningService: weights frozen. Reason: {}", reason);
    }

    /// Unfreeze weights
    fn unfreeze_weights(&self) {
        let mut tuner = self.tuner.write().unwrap();
        tuner.unfreeze_parameters();

        let mut stats = self.stats.write().unwrap();
        stats.is_frozen = false;

        info!("TuningService: weights unfrozen");
    }

    /// Get service statistics
    pub fn stats(&self) -> TuningServiceStats {
        let mut stats = self.stats.read().unwrap().clone();
        stats.seconds_since_update = self.last_update.read().unwrap().elapsed().as_secs();
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tuning_service_creation() {
        let config = TuningServiceConfig::default();
        let (service, weights_rx) = TuningService::new(config);

        let initial_weights = *weights_rx.borrow();
        assert_eq!(initial_weights, TunableWeights::default());

        // Check stats
        let stats = service.stats();
        assert_eq!(stats.outcomes_processed, 0);
        assert_eq!(stats.bandit_updates, 0);
    }

    #[tokio::test]
    async fn test_outcome_recording() {
        let config = TuningServiceConfig {
            dry_run: true,
            ..Default::default()
        };
        let (service, _) = TuningService::new(config);

        let context = TuningContext::default();
        let outcome = TradeOutcome::success(1.1, 0.8);

        service.handle_outcome(context, outcome);

        let stats = service.stats();
        assert_eq!(stats.outcomes_processed, 1);
    }

    #[tokio::test]
    async fn test_bandit_update_skipped_with_insufficient_outcomes() {
        let config = TuningServiceConfig {
            min_outcomes_for_update: 5,
            dry_run: false,
            ..Default::default()
        };
        let (service, _) = TuningService::new(config);

        // Add only 2 outcomes (less than minimum of 5)
        for _ in 0..2 {
            let context = TuningContext::default();
            let outcome = TradeOutcome::success(1.1, 0.8);
            service.handle_outcome(context, outcome);
        }

        // Run bandit update - should skip due to insufficient outcomes
        service.run_bandit_update();

        let stats = service.stats();
        assert_eq!(stats.bandit_updates, 0);
    }

    #[tokio::test]
    async fn test_bandit_update_with_sufficient_outcomes() {
        let config = TuningServiceConfig {
            min_outcomes_for_update: 3,
            dry_run: false,
            ..Default::default()
        };
        let (service, _) = TuningService::new(config);

        // Add 5 outcomes (more than minimum of 3)
        for _ in 0..5 {
            let context = TuningContext::default();
            let outcome = TradeOutcome::success(1.1, 0.8);
            service.handle_outcome(context, outcome);
        }

        // Run bandit update - should execute
        service.run_bandit_update();

        let stats = service.stats();
        assert_eq!(stats.bandit_updates, 1);
    }

    #[tokio::test]
    async fn test_dry_run_mode() {
        let config = TuningServiceConfig {
            min_outcomes_for_update: 1,
            dry_run: true,
            ..Default::default()
        };
        let (service, _) = TuningService::new(config);

        // Add outcomes
        let context = TuningContext::default();
        let outcome = TradeOutcome::success(1.1, 0.8);
        service.handle_outcome(context, outcome);

        // Run bandit update - should not update due to dry run
        service.run_bandit_update();

        let stats = service.stats();
        assert_eq!(stats.bandit_updates, 0);
    }

    #[tokio::test]
    async fn test_freeze_unfreeze_weights() {
        let config = TuningServiceConfig::default();
        let (service, weights_rx) = TuningService::new(config);

        let frozen_weights = TunableWeights {
            w_qass: 20.0,
            w_mpcf: 15.0,
            w_sobp: 10.0,
            w_iwim: 5.0,
        };

        service.freeze_weights(frozen_weights, "Test freeze");

        // Check that weights are frozen
        let stats = service.stats();
        assert!(stats.is_frozen);
        assert_eq!(stats.current_weights, frozen_weights);

        // Check watch channel received frozen weights
        let received = *weights_rx.borrow();
        assert_eq!(received, frozen_weights);

        // Unfreeze
        service.unfreeze_weights();
        let stats = service.stats();
        assert!(!stats.is_frozen);
    }

    #[tokio::test]
    async fn test_service_run_and_shutdown() {
        let config = TuningServiceConfig {
            bandit_update_interval_secs: 1, // Short interval for test
            bayesian_check_interval_secs: 1,
            dry_run: true,
            ..Default::default()
        };
        let (service, _weights_rx) = TuningService::new(config);
        let (tx, rx) = mpsc::channel(100);

        // Spawn service task
        let service_handle = tokio::spawn(async move {
            service.run(rx).await;
        });

        // Wait a bit then shutdown
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.send(TuningMessage::Shutdown).await.unwrap();

        // Wait for service to stop
        let result = tokio::time::timeout(Duration::from_secs(2), service_handle).await;
        assert!(result.is_ok(), "Service should shutdown within timeout");
    }

    #[tokio::test]
    async fn test_get_weights_message() {
        let config = TuningServiceConfig::default();
        let (service, _) = TuningService::new(config);
        let (tx, rx) = mpsc::channel(100);

        // Spawn service
        let service_handle = tokio::spawn(async move {
            service.run(rx).await;
        });

        // Request weights
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        tx.send(TuningMessage::GetWeights { response_tx })
            .await
            .unwrap();

        let weights = response_rx.await.unwrap();
        assert_eq!(weights, TunableWeights::default());

        // Shutdown
        tx.send(TuningMessage::Shutdown).await.unwrap();
        let _ = service_handle.await;
    }
}
