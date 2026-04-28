//! Second Wave Detector - Identifies optimal entry timing after HFT exit
//!
//! Analyzes on-chain data to detect transition from HFT-dominated trading
//! to organic "second wave" buying. Works in conjunction with ParadoxSensor
//! (network-level) to provide full picture.
//!
//! # Detection Signals
//! 1. Bot Activity Decay: MPCF bot ratio decreasing over time
//! 2. Organic Ratio Rise: Human transactions increasing
//! 3. Price Stabilization: Price recovered from initial dump
//! 4. Wallet Diversity: New unique wallets entering
//! 5. Volume Distribution: Varied tx sizes (not uniform bot sizes)

use crate::oracle::tx_metrics::TransactionMetrics;
use crate::oracle::ultrafast::{ActorInference, ActorType};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, info};

/// Weight for current metrics when blending with historical data
const CURRENT_WEIGHT: f32 = 0.7;
/// Weight for historical metrics when blending with current data  
const HISTORICAL_WEIGHT: f32 = 0.3;

/// Configuration for SecondWaveDetector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondWaveConfig {
    /// Minimum blocks after launch before considering second wave (default: 15 = ~6s)
    pub min_blocks_after_launch: u32,

    /// Maximum blocks to wait for second wave (default: 75 = ~30s)
    pub max_blocks_to_wait: u32,

    /// Bot ratio threshold - below this indicates HFT exit (default: 0.30)
    pub bot_ratio_exit_threshold: f32,

    /// Organic ratio threshold - above this indicates second wave (default: 0.50)
    pub organic_ratio_threshold: f32,

    /// Minimum unique wallets for confidence (default: 8)
    pub min_unique_wallets: u32,

    /// Price recovery threshold - price should be at least this % of peak (default: 0.70)
    pub price_recovery_threshold: f32,

    /// Score threshold for triggering ENTER signal (default: 0.65)
    pub entry_score_threshold: f32,

    /// Price crash threshold - below this % of peak triggers Skip action (default: 0.30)
    pub price_crash_threshold: f32,

    /// Component weights (must sum to 1.0)
    pub weight_bot_decay: f32, // default: 0.25
    pub weight_organic_growth: f32,      // default: 0.25
    pub weight_price_stability: f32,     // default: 0.20
    pub weight_wallet_diversity: f32,    // default: 0.15
    pub weight_volume_distribution: f32, // default: 0.15
}

impl Default for SecondWaveConfig {
    fn default() -> Self {
        Self {
            min_blocks_after_launch: 15,
            max_blocks_to_wait: 75,
            bot_ratio_exit_threshold: 0.30,
            organic_ratio_threshold: 0.50,
            min_unique_wallets: 8,
            price_recovery_threshold: 0.70,
            entry_score_threshold: 0.65,
            price_crash_threshold: 0.30,
            weight_bot_decay: 0.25,
            weight_organic_growth: 0.25,
            weight_price_stability: 0.20,
            weight_wallet_diversity: 0.15,
            weight_volume_distribution: 0.15,
        }
    }
}

/// Recommended action based on analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecondWaveAction {
    /// Too early - HFT still active
    Wait,
    /// HFT exiting - prepare for entry
    Prepare,
    /// Second wave detected - optimal entry window
    Enter,
    /// Too late or failed - momentum lost
    Skip,
}

/// Result from SecondWaveDetector analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondWaveResult {
    /// Overall second wave confidence score (0.0-1.0)
    pub second_wave_score: f32,

    /// Whether second wave is active (score > threshold)
    pub is_second_wave_active: bool,

    /// Confidence that HFT bots have exited (0.0-1.0)
    pub hft_exit_confidence: f32,

    /// Organic activity growth confidence (0.0-1.0)
    pub organic_growth_confidence: f32,

    /// Recommended action
    pub recommended_action: SecondWaveAction,

    /// Current bot ratio from MPCF
    pub current_bot_ratio: f32,

    /// Current organic ratio
    pub current_organic_ratio: f32,

    /// Blocks since token launch
    pub blocks_since_launch: u32,

    /// Unique wallet count
    pub unique_wallet_count: u32,

    /// Price relative to peak (0.0-1.0+)
    pub price_vs_peak_ratio: f32,

    /// Individual component scores
    pub components: SecondWaveComponents,

    /// Analysis time in microseconds
    pub analysis_time_us: u64,
}

/// Individual component scores for transparency
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecondWaveComponents {
    pub bot_decay_score: f32,
    pub organic_growth_score: f32,
    pub price_stability_score: f32,
    pub wallet_diversity_score: f32,
    pub volume_distribution_score: f32,
}

/// SecondWaveDetector - Main analysis engine
#[derive(Debug, Clone)]
pub struct SecondWaveDetector {
    config: SecondWaveConfig,
    peak_price_ratio: f32,
    launch_ts_ms: Option<u64>,
}

impl SecondWaveDetector {
    pub fn new() -> Self {
        Self::with_config(SecondWaveConfig::default())
    }

    pub fn with_config(config: SecondWaveConfig) -> Self {
        Self {
            config,
            peak_price_ratio: 1.0,
            launch_ts_ms: None,
        }
    }

    /// Set launch slot for tracking time since launch
    pub fn set_launch_ts_ms(&mut self, ts_ms: u64) {
        if self.launch_ts_ms.is_none() {
            self.launch_ts_ms = Some(ts_ms);
        }
    }

    /// Update peak price if current is higher
    pub fn update_peak_price(&mut self, current_price_ratio: f32) {
        if current_price_ratio > self.peak_price_ratio {
            self.peak_price_ratio = current_price_ratio;
        }
    }

    /// Analyze current state and detect second wave
    ///
    /// # Arguments
    /// * `current_ts_ms` - Current event timestamp (ms)
    /// * `metrics` - Transaction metrics from recent window
    /// * `mpcf_results` - MPCF actor inference results (if available)
    /// * `current_price_ratio` - Current price relative to initial (1.0 = initial)
    pub fn analyze(
        &self,
        current_ts_ms: u64,
        metrics: &TransactionMetrics,
        mpcf_results: Option<&[ActorInference]>,
        current_price_ratio: f32,
    ) -> SecondWaveResult {
        let start = Instant::now();

        // Calculate blocks since launch
        let blocks_since_launch = self
            .launch_ts_ms
            .map(|ls| current_ts_ms.saturating_sub(ls).saturating_div(400) as u32)
            .unwrap_or(0);

        // Extract bot/organic ratios
        let (bot_ratio, organic_ratio) = self.calculate_actor_ratios(mpcf_results, metrics);

        // Calculate component scores
        let mut components = SecondWaveComponents::default();

        // 1. Bot Decay Score
        components.bot_decay_score = self.calc_bot_decay_score(bot_ratio);

        // 2. Organic Growth Score
        components.organic_growth_score = self.calc_organic_growth_score(organic_ratio, metrics);

        // 3. Price Stability Score
        // Handle edge case where peak_price_ratio is 0 (should not happen in normal operation)
        let price_vs_peak = if self.peak_price_ratio > 0.0 {
            current_price_ratio / self.peak_price_ratio
        } else {
            // Peak not yet recorded or reset, treat current as baseline
            current_price_ratio
        };
        components.price_stability_score = self.calc_price_stability_score(price_vs_peak);

        // 4. Wallet Diversity Score
        components.wallet_diversity_score = self.calc_wallet_diversity_score(metrics);

        // 5. Volume Distribution Score
        components.volume_distribution_score = self.calc_volume_distribution_score(metrics);

        // Calculate weighted second wave score
        let second_wave_score = self.config.weight_bot_decay * components.bot_decay_score
            + self.config.weight_organic_growth * components.organic_growth_score
            + self.config.weight_price_stability * components.price_stability_score
            + self.config.weight_wallet_diversity * components.wallet_diversity_score
            + self.config.weight_volume_distribution * components.volume_distribution_score;

        let second_wave_score = second_wave_score.clamp(0.0, 1.0);

        // Calculate HFT exit confidence
        let hft_exit_confidence =
            self.calc_hft_exit_confidence(bot_ratio, &components, blocks_since_launch);

        // Determine action
        let recommended_action = self.determine_action(
            second_wave_score,
            hft_exit_confidence,
            blocks_since_launch,
            price_vs_peak,
        );

        let is_second_wave_active = second_wave_score >= self.config.entry_score_threshold;

        let result = SecondWaveResult {
            second_wave_score,
            is_second_wave_active,
            hft_exit_confidence,
            organic_growth_confidence: components.organic_growth_score,
            recommended_action,
            current_bot_ratio: bot_ratio,
            current_organic_ratio: organic_ratio,
            blocks_since_launch,
            unique_wallet_count: metrics.unique_addrs as u32,
            price_vs_peak_ratio: price_vs_peak,
            components,
            analysis_time_us: start.elapsed().as_micros() as u64,
        };

        if recommended_action == SecondWaveAction::Enter {
            info!(
                "SECOND_WAVE: ENTER! score={:.2}, hft_exit={:.2}, organic={:.2}, blocks={}",
                second_wave_score, hft_exit_confidence, organic_ratio, blocks_since_launch
            );
        } else {
            debug!(
                "SECOND_WAVE: {:?} score={:.2}, bot={:.2}, blocks={}",
                recommended_action, second_wave_score, bot_ratio, blocks_since_launch
            );
        }

        result
    }

    /// Analyze current state with block history for enhanced trend detection
    ///
    /// This method uses historical block data to improve second wave detection
    /// by analyzing trends in bot activity and wallet diversity.
    ///
    /// # Arguments
    /// * `current_ts_ms` - Current event timestamp (ms)
    /// * `metrics` - Transaction metrics from recent window
    /// * `block_history` - Historical block snapshots for trend analysis
    /// * `mpcf_results` - MPCF actor inference results (if available)
    /// * `current_price_ratio` - Current price relative to initial (1.0 = initial)
    pub fn analyze_with_history(
        &self,
        current_ts_ms: u64,
        metrics: &TransactionMetrics,
        block_history: &crate::oracle::block_metrics::BlockMetricsBuffer,
        mpcf_results: Option<&[ActorInference]>,
        current_price_ratio: f32,
    ) -> SecondWaveResult {
        let start = Instant::now();

        // Calculate blocks since launch
        let blocks_since_launch = self
            .launch_ts_ms
            .map(|ls| current_ts_ms.saturating_sub(ls).saturating_div(400) as u32)
            .unwrap_or(0);

        // Get trend data from block history
        let bot_trend = block_history.bot_ratio_trend(10);
        let wallet_trend = block_history.unique_wallets_trend(10);
        let historical_bot_ratio = block_history.avg_bot_ratio(10);
        let historical_organic_ratio = block_history.avg_organic_ratio(10);

        // Extract current bot/organic ratios
        let (current_bot_ratio, current_organic_ratio) =
            self.calculate_actor_ratios(mpcf_results, metrics);

        // Use historical average if available, otherwise fall back to current
        let bot_ratio = if !block_history.is_empty() {
            // Blend current and historical for more stable signal
            CURRENT_WEIGHT * current_bot_ratio + HISTORICAL_WEIGHT * historical_bot_ratio
        } else {
            current_bot_ratio
        };

        let organic_ratio = if !block_history.is_empty() {
            CURRENT_WEIGHT * current_organic_ratio + HISTORICAL_WEIGHT * historical_organic_ratio
        } else {
            current_organic_ratio
        };

        // Calculate component scores
        let mut components = SecondWaveComponents::default();

        // 1. Bot Decay Score - enhanced with trend data
        components.bot_decay_score = self.calc_bot_decay_score_with_trend(bot_ratio, bot_trend);

        // 2. Organic Growth Score - enhanced with trend data
        components.organic_growth_score =
            self.calc_organic_growth_score_with_trend(organic_ratio, metrics, wallet_trend);

        // 3. Price Stability Score
        let price_vs_peak = if self.peak_price_ratio > 0.0 {
            current_price_ratio / self.peak_price_ratio
        } else {
            current_price_ratio
        };
        components.price_stability_score = self.calc_price_stability_score(price_vs_peak);

        // 4. Wallet Diversity Score - enhanced with trend data
        components.wallet_diversity_score =
            self.calc_wallet_diversity_score_with_trend(metrics, wallet_trend);

        // 5. Volume Distribution Score
        components.volume_distribution_score = self.calc_volume_distribution_score(metrics);

        // Calculate weighted second wave score
        let second_wave_score = self.config.weight_bot_decay * components.bot_decay_score
            + self.config.weight_organic_growth * components.organic_growth_score
            + self.config.weight_price_stability * components.price_stability_score
            + self.config.weight_wallet_diversity * components.wallet_diversity_score
            + self.config.weight_volume_distribution * components.volume_distribution_score;

        let second_wave_score = second_wave_score.clamp(0.0, 1.0);

        // Calculate HFT exit confidence with trend data
        let hft_exit_confidence = self.calc_hft_exit_confidence_with_trend(
            bot_ratio,
            &components,
            blocks_since_launch,
            bot_trend,
        );

        // Determine action
        let recommended_action = self.determine_action(
            second_wave_score,
            hft_exit_confidence,
            blocks_since_launch,
            price_vs_peak,
        );

        let is_second_wave_active = second_wave_score >= self.config.entry_score_threshold;

        let result = SecondWaveResult {
            second_wave_score,
            is_second_wave_active,
            hft_exit_confidence,
            organic_growth_confidence: components.organic_growth_score,
            recommended_action,
            current_bot_ratio: bot_ratio,
            current_organic_ratio: organic_ratio,
            blocks_since_launch,
            unique_wallet_count: metrics.unique_addrs as u32,
            price_vs_peak_ratio: price_vs_peak,
            components,
            analysis_time_us: start.elapsed().as_micros() as u64,
        };

        if recommended_action == SecondWaveAction::Enter {
            info!(
                "SECOND_WAVE_HISTORY: ENTER! score={:.2}, hft_exit={:.2}, bot_trend={:.3}, wallet_trend={:.1}",
                second_wave_score, hft_exit_confidence, bot_trend, wallet_trend
            );
        } else {
            debug!(
                "SECOND_WAVE_HISTORY: {:?} score={:.2}, bot_trend={:.3}, wallet_trend={:.1}",
                recommended_action, second_wave_score, bot_trend, wallet_trend
            );
        }

        result
    }

    /// Calculate bot decay score with trend enhancement
    fn calc_bot_decay_score_with_trend(&self, bot_ratio: f32, bot_trend: f32) -> f32 {
        let base_score = self.calc_bot_decay_score(bot_ratio);

        // Boost score if bot ratio is trending down (negative trend)
        let trend_bonus = if bot_trend < -0.1 {
            // Strong downward trend: up to +0.2 bonus
            (bot_trend.abs() * 0.5).min(0.2)
        } else if bot_trend > 0.1 {
            // Upward trend: penalty
            -(bot_trend * 0.3).min(0.15)
        } else {
            0.0
        };

        (base_score + trend_bonus).clamp(0.0, 1.0)
    }

    /// Calculate organic growth score with trend enhancement
    fn calc_organic_growth_score_with_trend(
        &self,
        organic_ratio: f32,
        metrics: &TransactionMetrics,
        wallet_trend: f32,
    ) -> f32 {
        let base_score = self.calc_organic_growth_score(organic_ratio, metrics);

        // Boost score if wallet count is trending up (positive trend)
        let trend_bonus = if wallet_trend > 1.0 {
            // Strong upward trend: up to +0.15 bonus
            (wallet_trend / 10.0).min(0.15)
        } else if wallet_trend < -1.0 {
            // Downward trend: penalty
            (wallet_trend / 10.0).max(-0.1)
        } else {
            0.0
        };

        (base_score + trend_bonus).clamp(0.0, 1.0)
    }

    /// Calculate wallet diversity score with trend enhancement
    fn calc_wallet_diversity_score_with_trend(
        &self,
        metrics: &TransactionMetrics,
        wallet_trend: f32,
    ) -> f32 {
        let base_score = self.calc_wallet_diversity_score(metrics);

        // Boost score if wallet diversity is increasing
        let trend_bonus = if wallet_trend > 2.0 {
            0.1
        } else if wallet_trend > 0.5 {
            0.05
        } else if wallet_trend < -2.0 {
            -0.1
        } else {
            0.0
        };

        (base_score + trend_bonus).clamp(0.0, 1.0)
    }

    /// Calculate HFT exit confidence with trend data
    fn calc_hft_exit_confidence_with_trend(
        &self,
        bot_ratio: f32,
        components: &SecondWaveComponents,
        blocks_since_launch: u32,
        bot_trend: f32,
    ) -> f32 {
        let base_confidence =
            self.calc_hft_exit_confidence(bot_ratio, components, blocks_since_launch);

        // Enhance confidence if bot trend is strongly negative (bots leaving)
        let trend_bonus = if bot_trend < -0.15 {
            0.1
        } else if bot_trend < -0.05 {
            0.05
        } else if bot_trend > 0.1 {
            -0.1
        } else {
            0.0
        };

        (base_confidence + trend_bonus).clamp(0.0, 1.0)
    }

    fn calculate_actor_ratios(
        &self,
        mpcf: Option<&[ActorInference]>,
        metrics: &TransactionMetrics,
    ) -> (f32, f32) {
        if let Some(results) = mpcf {
            if !results.is_empty() {
                let bot_count = results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.actor,
                            ActorType::SniperScript | ActorType::MEVArb | ActorType::SybilBot
                        )
                    })
                    .count();
                let human_count = results
                    .iter()
                    .filter(|r| matches!(r.actor, ActorType::HumanMobile | ActorType::HumanDesktop))
                    .count();
                let total = results.len();

                return (
                    bot_count as f32 / total as f32,
                    human_count as f32 / total as f32,
                );
            }
        }

        // Fallback: infer from unique ratio
        let unique_ratio = metrics.unique_ratio() as f32;
        (1.0 - unique_ratio, unique_ratio)
    }

    fn calc_bot_decay_score(&self, bot_ratio: f32) -> f32 {
        if bot_ratio < self.config.bot_ratio_exit_threshold {
            1.0
        } else {
            1.0 - (bot_ratio / self.config.bot_ratio_exit_threshold).min(1.0)
        }
    }

    fn calc_organic_growth_score(&self, organic_ratio: f32, metrics: &TransactionMetrics) -> f32 {
        let ratio_score = if organic_ratio >= self.config.organic_ratio_threshold {
            1.0
        } else {
            organic_ratio / self.config.organic_ratio_threshold
        };

        let wallet_bonus = (metrics.unique_ratio() as f32 * 0.3).min(0.3);
        (ratio_score + wallet_bonus).min(1.0)
    }

    fn calc_price_stability_score(&self, price_vs_peak: f32) -> f32 {
        if price_vs_peak >= 1.0 {
            1.0
        } else if price_vs_peak >= self.config.price_recovery_threshold {
            0.7 + 0.3
                * ((price_vs_peak - self.config.price_recovery_threshold)
                    / (1.0 - self.config.price_recovery_threshold))
        } else if price_vs_peak >= 0.5 {
            0.3 + 0.4 * ((price_vs_peak - 0.5) / (self.config.price_recovery_threshold - 0.5))
        } else {
            price_vs_peak * 0.6
        }
    }

    fn calc_wallet_diversity_score(&self, metrics: &TransactionMetrics) -> f32 {
        let unique = metrics.unique_addrs as u32;
        let threshold = self.config.min_unique_wallets;

        if unique >= threshold * 2 {
            1.0
        } else if unique >= threshold {
            0.7 + 0.3 * ((unique - threshold) as f32 / threshold as f32)
        } else if unique >= 3 {
            0.3 + 0.4 * ((unique - 3) as f32 / (threshold - 3) as f32)
        } else {
            unique as f32 * 0.1
        }
    }

    fn calc_volume_distribution_score(&self, metrics: &TransactionMetrics) -> f32 {
        if metrics.volumes_sol.is_empty() {
            return 0.5;
        }

        let mean = metrics.total_volume_sol / metrics.volumes_sol.len() as f64;
        if mean <= 0.0 {
            return 0.5;
        }

        let variance: f64 = metrics
            .volumes_sol
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / metrics.volumes_sol.len() as f64;
        let cv = (variance.sqrt() / mean) as f32;

        // High CV = varied volumes = organic
        if cv > 1.5 {
            1.0
        } else if cv > 0.8 {
            0.6 + 0.4 * ((cv - 0.8) / 0.7)
        } else if cv > 0.3 {
            0.3 + 0.3 * ((cv - 0.3) / 0.5)
        } else {
            cv / 0.3 * 0.3
        }
    }

    fn calc_hft_exit_confidence(
        &self,
        bot_ratio: f32,
        components: &SecondWaveComponents,
        blocks_since_launch: u32,
    ) -> f32 {
        let time_factor = if blocks_since_launch < 10 {
            0.1
        } else if blocks_since_launch < 15 {
            0.3
        } else if blocks_since_launch < 25 {
            0.6 + 0.3 * ((blocks_since_launch - 15) as f32 / 10.0)
        } else {
            0.9
        };

        let ratio_factor = 1.0 - bot_ratio;

        (0.4 * time_factor + 0.3 * ratio_factor + 0.3 * components.bot_decay_score).clamp(0.0, 1.0)
    }

    fn determine_action(
        &self,
        score: f32,
        hft_exit: f32,
        blocks: u32,
        price_vs_peak: f32,
    ) -> SecondWaveAction {
        if blocks < self.config.min_blocks_after_launch {
            return SecondWaveAction::Wait;
        }
        if blocks > self.config.max_blocks_to_wait {
            return SecondWaveAction::Skip;
        }
        if price_vs_peak < self.config.price_crash_threshold {
            return SecondWaveAction::Skip;
        }
        if score >= self.config.entry_score_threshold && hft_exit > 0.6 {
            return SecondWaveAction::Enter;
        }
        if hft_exit > 0.5 && score >= 0.4 {
            return SecondWaveAction::Prepare;
        }
        SecondWaveAction::Wait
    }

    pub fn reset(&mut self) {
        self.peak_price_ratio = 1.0;
        self.launch_ts_ms = None;
    }
}

impl Default for SecondWaveDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = SecondWaveDetector::new();
        assert_eq!(detector.config.min_blocks_after_launch, 15);
    }

    #[test]
    fn test_default_config() {
        let config = SecondWaveConfig::default();
        assert_eq!(config.min_blocks_after_launch, 15);
        assert_eq!(config.max_blocks_to_wait, 75);
        assert!((config.bot_ratio_exit_threshold - 0.30).abs() < 0.001);
        assert!((config.organic_ratio_threshold - 0.50).abs() < 0.001);
        assert_eq!(config.min_unique_wallets, 8);
        assert!((config.price_recovery_threshold - 0.70).abs() < 0.001);
        assert!((config.entry_score_threshold - 0.65).abs() < 0.001);
        assert!((config.price_crash_threshold - 0.30).abs() < 0.001);

        // Check weights sum to 1.0
        let weight_sum = config.weight_bot_decay
            + config.weight_organic_growth
            + config.weight_price_stability
            + config.weight_wallet_diversity
            + config.weight_volume_distribution;
        assert!(
            (weight_sum - 1.0).abs() < 0.001,
            "Weights should sum to 1.0"
        );
    }

    #[test]
    fn test_component_scores_in_range() {
        let detector = SecondWaveDetector::new();
        let metrics = TransactionMetrics::new()
            .with_tx_count(30)
            .with_unique_addrs(20);

        let result = detector.analyze(25, &metrics, None, 0.8);

        assert!(
            result.second_wave_score >= 0.0 && result.second_wave_score <= 1.0,
            "second_wave_score should be in [0,1]"
        );
        assert!(
            result.components.bot_decay_score >= 0.0 && result.components.bot_decay_score <= 1.0,
            "bot_decay_score should be in [0,1]"
        );
        assert!(
            result.components.organic_growth_score >= 0.0
                && result.components.organic_growth_score <= 1.0,
            "organic_growth_score should be in [0,1]"
        );
        assert!(
            result.components.price_stability_score >= 0.0
                && result.components.price_stability_score <= 1.0,
            "price_stability_score should be in [0,1]"
        );
        assert!(
            result.components.wallet_diversity_score >= 0.0
                && result.components.wallet_diversity_score <= 1.0,
            "wallet_diversity_score should be in [0,1]"
        );
        assert!(
            result.components.volume_distribution_score >= 0.0
                && result.components.volume_distribution_score <= 1.0,
            "volume_distribution_score should be in [0,1]"
        );
    }

    #[test]
    fn test_wait_action_before_min_blocks() {
        let detector = SecondWaveDetector::new();
        let metrics = TransactionMetrics::new()
            .with_tx_count(30)
            .with_unique_addrs(20);

        // Blocks < min_blocks_after_launch (15) should result in Wait
        let result = detector.analyze(10 * 400, &metrics, None, 1.0);
        assert_eq!(
            result.recommended_action,
            SecondWaveAction::Wait,
            "Should Wait when blocks < min_blocks_after_launch"
        );
    }

    #[test]
    fn test_skip_action_after_max_blocks() {
        let mut detector = SecondWaveDetector::new();
        detector.set_launch_ts_ms(0);
        let metrics = TransactionMetrics::new()
            .with_tx_count(30)
            .with_unique_addrs(20);

        // Blocks > max_blocks_to_wait (75) should result in Skip
        let result = detector.analyze(100 * 400, &metrics, None, 1.0);
        assert_eq!(
            result.recommended_action,
            SecondWaveAction::Skip,
            "Should Skip when blocks > max_blocks_to_wait"
        );
    }

    #[test]
    fn test_skip_action_on_price_crash() {
        let mut detector = SecondWaveDetector::new();
        detector.set_launch_ts_ms(0);
        detector.update_peak_price(1.0);
        let metrics = TransactionMetrics::new()
            .with_tx_count(30)
            .with_unique_addrs(20);

        // Price < price_crash_threshold (0.30 by default) of peak should result in Skip
        let result = detector.analyze(30 * 400, &metrics, None, 0.2);
        assert_eq!(
            result.recommended_action,
            SecondWaveAction::Skip,
            "Should Skip when price crashes below price_crash_threshold (30% of peak)"
        );
    }

    #[test]
    fn test_enter_action_on_good_conditions() {
        let mut detector = SecondWaveDetector::new();
        detector.set_launch_ts_ms(0);

        // Create favorable conditions: high organic ratio, good price, many unique wallets
        let metrics = TransactionMetrics::new()
            .with_tx_count(50)
            .with_unique_addrs(40) // High unique ratio
            .with_volumes(vec![0.5, 1.2, 0.8, 3.5, 0.1, 2.0, 0.3, 1.5, 0.7, 2.5]); // Varied volumes

        // Good price recovery, enough blocks passed
        let result = detector.analyze(30 * 400, &metrics, None, 0.85);

        // With high unique ratio (fallback for MPCF), should have high organic score
        assert!(
            result.recommended_action == SecondWaveAction::Enter
                || result.recommended_action == SecondWaveAction::Prepare,
            "Should Enter or Prepare with good conditions, got {:?}",
            result.recommended_action
        );
    }

    #[test]
    fn test_mpcf_actor_ratios() {
        let detector = SecondWaveDetector::new();

        // Create MPCF results with mixed actors
        let mpcf_results = vec![
            ActorInference {
                actor: ActorType::HumanMobile,
                confidence: 0.9,
                entropy: 6.0,
                fingerprint: [0u8; 16],
            },
            ActorInference {
                actor: ActorType::HumanDesktop,
                confidence: 0.85,
                entropy: 5.5,
                fingerprint: [0u8; 16],
            },
            ActorInference {
                actor: ActorType::SniperScript,
                confidence: 0.8,
                entropy: 3.0,
                fingerprint: [0u8; 16],
            },
            ActorInference {
                actor: ActorType::Unknown,
                confidence: 0.3,
                entropy: 4.0,
                fingerprint: [0u8; 16],
            },
        ];

        let metrics = TransactionMetrics::new();
        let (bot_ratio, organic_ratio) =
            detector.calculate_actor_ratios(Some(&mpcf_results), &metrics);

        // 1 bot (SniperScript), 2 humans (Mobile + Desktop) out of 4
        assert!((bot_ratio - 0.25).abs() < 0.01, "Bot ratio should be 0.25");
        assert!(
            (organic_ratio - 0.5).abs() < 0.01,
            "Organic ratio should be 0.5"
        );
    }

    #[test]
    fn test_launch_ts_ms_tracking() {
        let mut detector = SecondWaveDetector::new();
        assert!(detector.launch_ts_ms.is_none());

        detector.set_launch_ts_ms(1000);
        assert_eq!(detector.launch_ts_ms, Some(1000));

        // Second call should not change the launch slot
        detector.set_launch_ts_ms(2000);
        assert_eq!(detector.launch_ts_ms, Some(1000));
    }

    #[test]
    fn test_peak_price_tracking() {
        let mut detector = SecondWaveDetector::new();
        assert!((detector.peak_price_ratio - 1.0).abs() < 0.001);

        detector.update_peak_price(1.5);
        assert!((detector.peak_price_ratio - 1.5).abs() < 0.001);

        // Lower price should not update peak
        detector.update_peak_price(1.2);
        assert!((detector.peak_price_ratio - 1.5).abs() < 0.001);

        // Higher price should update
        detector.update_peak_price(2.0);
        assert!((detector.peak_price_ratio - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_reset() {
        let mut detector = SecondWaveDetector::new();
        detector.set_launch_ts_ms(1000);
        detector.update_peak_price(2.0);

        detector.reset();

        assert!(detector.launch_ts_ms.is_none());
        assert!((detector.peak_price_ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_volume_distribution_score_varied() {
        let detector = SecondWaveDetector::new();

        // Highly varied volumes (organic-like) should score high
        let metrics_varied =
            TransactionMetrics::new().with_volumes(vec![0.1, 5.0, 0.5, 10.0, 0.2, 3.0]);
        let score_varied = detector.calc_volume_distribution_score(&metrics_varied);

        // Uniform volumes (bot-like) should score lower
        let metrics_uniform =
            TransactionMetrics::new().with_volumes(vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let score_uniform = detector.calc_volume_distribution_score(&metrics_uniform);

        assert!(
            score_varied > score_uniform,
            "Varied volumes ({}) should score higher than uniform ({}) volumes",
            score_varied,
            score_uniform
        );
    }

    #[test]
    fn test_hft_exit_confidence_increases_with_time() {
        let detector = SecondWaveDetector::new();
        let components = SecondWaveComponents::default();

        let conf_early = detector.calc_hft_exit_confidence(0.5, &components, 5);
        let conf_mid = detector.calc_hft_exit_confidence(0.5, &components, 20);
        let conf_late = detector.calc_hft_exit_confidence(0.5, &components, 30);

        assert!(
            conf_early < conf_mid,
            "HFT exit confidence should increase with time"
        );
        assert!(
            conf_mid < conf_late,
            "HFT exit confidence should increase with time"
        );
    }

    #[test]
    fn test_analysis_time_is_recorded() {
        let detector = SecondWaveDetector::new();
        let metrics = TransactionMetrics::new()
            .with_tx_count(10)
            .with_unique_addrs(5);

        let result = detector.analyze(20, &metrics, None, 0.9);

        assert!(
            result.analysis_time_us > 0,
            "Analysis time should be recorded"
        );
    }

    #[test]
    fn test_second_wave_action_serialization() {
        // Test that SecondWaveAction can be serialized and deserialized
        let actions = vec![
            SecondWaveAction::Wait,
            SecondWaveAction::Prepare,
            SecondWaveAction::Enter,
            SecondWaveAction::Skip,
        ];

        for action in actions {
            let serialized = serde_json::to_string(&action).expect("Should serialize");
            let deserialized: SecondWaveAction =
                serde_json::from_str(&serialized).expect("Should deserialize");
            assert_eq!(action, deserialized);
        }
    }

    #[test]
    fn test_second_wave_result_serialization() {
        let result = SecondWaveResult {
            second_wave_score: 0.75,
            is_second_wave_active: true,
            hft_exit_confidence: 0.8,
            organic_growth_confidence: 0.7,
            recommended_action: SecondWaveAction::Enter,
            current_bot_ratio: 0.2,
            current_organic_ratio: 0.6,
            blocks_since_launch: 25,
            unique_wallet_count: 15,
            price_vs_peak_ratio: 0.9,
            components: SecondWaveComponents {
                bot_decay_score: 0.8,
                organic_growth_score: 0.7,
                price_stability_score: 0.85,
                wallet_diversity_score: 0.75,
                volume_distribution_score: 0.6,
            },
            analysis_time_us: 100,
        };

        let serialized = serde_json::to_string(&result).expect("Should serialize");
        let deserialized: SecondWaveResult =
            serde_json::from_str(&serialized).expect("Should deserialize");

        assert!((deserialized.second_wave_score - 0.75).abs() < 0.001);
        assert!(deserialized.is_second_wave_active);
        assert_eq!(deserialized.recommended_action, SecondWaveAction::Enter);
    }
}
