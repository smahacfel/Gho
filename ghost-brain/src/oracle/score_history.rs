//! Score History - Aggregated Cycle Tracking for Patient Observer Strategy
//!
//! This module implements the "Patient Observer" approach to token evaluation.
//! Instead of making decisions based on a single snapshot, it tracks scores
//! across multiple observation cycles (38 cycles over 15 seconds) and uses
//! trend analysis and weighted averaging to make final decisions.
//!
//! # Key Features
//!
//! - **15-second observation window** with ~38 cycles at 400ms intervals
//! - **Trend detection** (Rising/Falling/Stable/SecondWave)
//! - **Weighted average scoring** giving more weight to recent observations
//! - **Second wave detection** for identifying organic activity after HFT exit
//! - **Early exit** when score is critically low
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_brain::oracle::score_history::{ScoreHistory, CycleScore};
//!
//! let mut history = ScoreHistory::new(38, "pool_id".to_string());
//!
//! // Add cycle results as they come in
//! history.push_cycle(CycleScore {
//!     cycle_num: 1,
//!     timestamp_ms: 0,
//!     survivor_score: 45,
//!     confidence: 0.65,
//!     // ... other fields
//! });
//!
//! // After observation, get final decision
//! let decision = history.compute_final_decision(50);
//! match decision.action {
//!     ObservationAction::Enter => println!("Buy!"),
//!     ObservationAction::Skip => println!("Skip"),
//!     ObservationAction::Wait => println!("Continue observing"),
//! }
//! ```

use crate::oracle::hyper_prediction::TcfResult;
use crate::oracle::second_wave_detector::SecondWaveAction;
use crate::oracle::tcf::{MarketObservation, TcfPhase, TrendCohesionField};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// =============================================================================
// Constants
// =============================================================================

/// Number of recent cycles to analyze for trend detection
const TREND_ANALYSIS_WINDOW: usize = 5;

/// Minimum delta between cycles to consider "Rising" trend
const RISING_TREND_MIN_DELTA: f32 = 3.0;

/// Minimum delta between cycles to consider "Falling" trend
const FALLING_TREND_MIN_DELTA: f32 = -3.0;

/// Minimum cycles before we can reliably detect a trend
const MIN_CYCLES_FOR_TREND: usize = 3;

/// Minimum cycles for second wave detection
const MIN_CYCLES_FOR_SECOND_WAVE: usize = 5;

/// Early exit threshold - if peak score is below this after MIN_CYCLES_FOR_DECISION, skip
const EARLY_EXIT_SCORE_THRESHOLD: u8 = 25;

/// Minimum cycles before allowing early exit decision
const MIN_CYCLES_FOR_DECISION: usize = 10;

/// Low confidence threshold
const LOW_CONFIDENCE_THRESHOLD: f32 = 0.4;

/// High confidence threshold for entering with slightly below threshold score
const HIGH_CONFIDENCE_THRESHOLD: f32 = 0.75;

/// Medium confidence threshold
const MEDIUM_CONFIDENCE_THRESHOLD: f32 = 0.6;

/// Score buffer for entering with rising trend
const RISING_TREND_SCORE_BUFFER: u8 = 5;

/// Score buffer for second wave detection
const SECOND_WAVE_SCORE_BUFFER: u8 = 10;

/// Critical low score threshold
const CRITICAL_LOW_SCORE: u8 = 30;

/// Minimum cycles without momentum before skip
const MIN_CYCLES_NO_MOMENTUM: usize = 20;

// =============================================================================
// Types
// =============================================================================

/// Result of a single observation cycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleScore {
    /// Cycle number (1-38)
    pub cycle_num: u32,
    /// Timestamp since observation start (ms)
    pub timestamp_ms: u64,
    /// SurvivorScore from this cycle
    pub survivor_score: u8,
    /// Confidence from this cycle (0.0-1.0)
    pub confidence: f32,
    /// Number of new transactions in this cycle
    pub tx_count_delta: usize,
    /// Total transaction count at this point
    pub tx_count_total: usize,
    /// SecondWaveDetector action for this cycle
    pub second_wave_action: SecondWaveAction,
    /// Momentum score from this cycle
    pub momentum: f32,
    /// TCF market observation for this cycle (optional)
    /// This is collected each cycle and used in Final Verdict
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcf_observation: Option<TcfObservationData>,
}

/// Serializable TCF observation data for cycle tracking
///
/// This is a simplified representation of MarketObservation
/// that can be serialized and stored in the cycle history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcfObservationData {
    /// Price change direction and magnitude [-1, 1]
    pub price_delta: f64,
    /// Volume change direction and magnitude [-1, 1]
    pub volume_delta: f64,
    /// Liquidity distribution entropy [0, 1]
    pub liquidity_entropy: f64,
    /// Order flow imbalance [-1, 1]
    pub order_flow_imbalance: f64,
    /// MPCF confidence [0, 1]
    pub mpcf: f64,
    /// Transaction timing jitter [0, 1]
    pub jitter: f64,
    /// ParadoxSensor phase sync [0, 1]
    pub phase_sync: f64,
}

/// Trend direction of scores over time
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoreTrend {
    /// Score is increasing - positive signal
    Rising,
    /// Score is decreasing - negative signal
    Falling,
    /// Score is stable - neutral
    Stable,
    /// Second wave pattern detected - strong positive signal
    SecondWave,
    /// Not enough data to determine trend
    Insufficient,
}

impl std::fmt::Display for ScoreTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreTrend::Rising => write!(f, "📈 Rising"),
            ScoreTrend::Falling => write!(f, "📉 Falling"),
            ScoreTrend::Stable => write!(f, "➡️ Stable"),
            ScoreTrend::SecondWave => write!(f, "🌊 SecondWave"),
            ScoreTrend::Insufficient => write!(f, "❓ Insufficient"),
        }
    }
}

/// Final action after observation period
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationAction {
    /// Enter position - conditions are favorable
    Enter,
    /// Skip this token - conditions are unfavorable
    Skip,
    /// Continue waiting (internal state)
    Wait,
}

impl std::fmt::Display for ObservationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObservationAction::Enter => write!(f, "✅ ENTER"),
            ObservationAction::Skip => write!(f, "❌ SKIP"),
            ObservationAction::Wait => write!(f, "⏳ WAIT"),
        }
    }
}

/// Final decision after observation period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationDecision {
    /// Recommended action
    pub action: ObservationAction,
    /// Final aggregated score (weighted average)
    pub final_score: u8,
    /// Aggregated confidence
    pub confidence: f32,
    /// Detected trend
    pub trend: ScoreTrend,
    /// Whether second wave was detected
    pub second_wave_detected: bool,
    /// Peak score during observation
    pub peak_score: u8,
    /// Lowest score during observation
    pub lowest_score: u8,
    /// Number of cycles with rising momentum
    pub rising_momentum_cycles: usize,
    /// Reason for the decision
    pub reason: String,
    /// Total cycles observed
    pub total_cycles: usize,
}

// =============================================================================
// ScoreHistory Implementation
// =============================================================================

/// History of scores during token observation
///
/// Tracks all cycle results and provides aggregation methods for
/// the Patient Observer strategy.
#[derive(Debug, Clone)]
pub struct ScoreHistory {
    /// Ring buffer of cycle scores
    cycles: VecDeque<CycleScore>,
    /// Maximum capacity
    capacity: usize,
    /// Timestamp when observation started (ms since epoch or start)
    observation_start_ms: u64,
    /// Pool identifier for logging
    pool_id: String,
}

impl ScoreHistory {
    /// Create a new ScoreHistory with given capacity
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of cycles to store (typically 38)
    /// * `pool_id` - Pool identifier for logging purposes
    pub fn new(capacity: usize, pool_id: String) -> Self {
        Self {
            cycles: VecDeque::with_capacity(capacity),
            capacity,
            observation_start_ms: 0,
            pool_id,
        }
    }

    /// Add a new cycle score to the history
    pub fn push_cycle(&mut self, cycle: CycleScore) {
        if self.cycles.is_empty() {
            self.observation_start_ms = cycle.timestamp_ms;
        }

        // If at capacity, remove oldest entry
        if self.cycles.len() >= self.capacity {
            self.cycles.pop_front();
        }

        self.cycles.push_back(cycle);
    }

    /// Get the number of recorded cycles
    pub fn cycle_count(&self) -> usize {
        self.cycles.len()
    }

    /// Get observation duration in milliseconds
    pub fn observation_duration_ms(&self) -> u64 {
        if let Some(last) = self.cycles.back() {
            last.timestamp_ms.saturating_sub(self.observation_start_ms)
        } else {
            0
        }
    }

    /// Get the peak (highest) score observed
    pub fn peak_score(&self) -> u8 {
        self.cycles
            .iter()
            .map(|c| c.survivor_score)
            .max()
            .unwrap_or(0)
    }

    /// Get the lowest score observed
    pub fn lowest_score(&self) -> u8 {
        self.cycles
            .iter()
            .map(|c| c.survivor_score)
            .min()
            .unwrap_or(0)
    }

    /// Get the most recent score
    pub fn latest_score(&self) -> Option<u8> {
        self.cycles.back().map(|c| c.survivor_score)
    }

    /// Count cycles with rising momentum (Prepare or Enter action)
    pub fn rising_momentum_cycles(&self) -> usize {
        self.cycles
            .iter()
            .filter(|c| {
                matches!(
                    c.second_wave_action,
                    SecondWaveAction::Prepare | SecondWaveAction::Enter
                )
            })
            .count()
    }

    /// Calculate average confidence across all cycles
    pub fn average_confidence(&self) -> f32 {
        if self.cycles.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.cycles.iter().map(|c| c.confidence).sum();
        sum / self.cycles.len() as f32
    }

    /// Calculate weighted average score (more recent cycles have higher weight)
    ///
    /// Uses linear weighting: oldest cycle gets weight 1.0, newest gets weight 2.0
    /// Confidence is also factored in as a multiplier.
    pub fn weighted_average_score(&self) -> u8 {
        if self.cycles.is_empty() {
            return 0;
        }

        let mut weighted_sum: f32 = 0.0;
        let mut weight_sum: f32 = 0.0;
        let len = self.cycles.len() as f32;

        for (i, cycle) in self.cycles.iter().enumerate() {
            // Weight increases linearly from 1.0 to 2.0
            let recency_weight = 1.0 + (i as f32 / len);
            let effective_weight = recency_weight * cycle.confidence;

            weighted_sum += cycle.survivor_score as f32 * effective_weight;
            weight_sum += effective_weight;
        }

        if weight_sum > 0.0 {
            (weighted_sum / weight_sum).round().min(100.0) as u8
        } else {
            0
        }
    }

    /// Compute trend based on recent cycles
    ///
    /// Analyzes the last `window` cycles to determine if scores are:
    /// - Rising: Average delta >= 3 points per cycle
    /// - Falling: Average delta <= -3 points per cycle
    /// - SecondWave: Detected second wave pattern
    /// - Stable: No significant change
    /// - Insufficient: Not enough data
    pub fn compute_trend(&self, window: usize) -> ScoreTrend {
        if self.cycles.len() < MIN_CYCLES_FOR_TREND {
            return ScoreTrend::Insufficient;
        }

        let window_size = window.min(self.cycles.len());
        let recent: Vec<_> = self.cycles.iter().rev().take(window_size).collect();

        // Check for second wave pattern first
        if self.detect_second_wave(&recent) {
            return ScoreTrend::SecondWave;
        }

        // Calculate average delta between consecutive cycles
        // Note: recent is in reverse order (newest first)
        let mut deltas: Vec<f32> = Vec::new();
        for i in 1..recent.len() {
            // recent[i-1] is newer than recent[i]
            let delta = recent[i - 1].survivor_score as f32 - recent[i].survivor_score as f32;
            deltas.push(delta);
        }

        if deltas.is_empty() {
            return ScoreTrend::Insufficient;
        }

        let avg_delta: f32 = deltas.iter().sum::<f32>() / deltas.len() as f32;

        if avg_delta >= RISING_TREND_MIN_DELTA {
            ScoreTrend::Rising
        } else if avg_delta <= FALLING_TREND_MIN_DELTA {
            ScoreTrend::Falling
        } else {
            ScoreTrend::Stable
        }
    }

    /// Detect "second wave" pattern
    ///
    /// Second wave is when:
    /// 1. Earlier cycles showed decline/stabilization
    /// 2. Recent cycles show rising momentum
    /// 3. SecondWaveDetector shows Prepare or Enter
    fn detect_second_wave(&self, recent: &[&CycleScore]) -> bool {
        if recent.len() < MIN_CYCLES_FOR_SECOND_WAVE {
            return false;
        }

        // Check if last 3 cycles show rising momentum (Prepare or Enter)
        let last_3_rising = recent[0..3.min(recent.len())].iter().all(|c| {
            matches!(
                c.second_wave_action,
                SecondWaveAction::Prepare | SecondWaveAction::Enter
            )
        });

        // Check if earlier cycles were Wait or Skip (stagnation/decline)
        let earlier_cycles = if recent.len() > 3 { &recent[3..] } else { &[] };
        let earlier_stagnant = earlier_cycles
            .iter()
            .filter(|c| {
                matches!(
                    c.second_wave_action,
                    SecondWaveAction::Wait | SecondWaveAction::Skip
                )
            })
            .count();

        // Need at least 2 stagnant cycles earlier and all last 3 rising
        last_3_rising && earlier_stagnant >= 2
    }

    /// Check if we should early exit due to consistently low scores
    ///
    /// Returns true if peak score is below threshold after minimum cycles
    pub fn should_early_exit(&self) -> bool {
        if self.cycles.len() < MIN_CYCLES_FOR_DECISION {
            return false;
        }
        self.peak_score() < EARLY_EXIT_SCORE_THRESHOLD
    }

    /// Compute the final decision after observation
    ///
    /// # Arguments
    /// * `threshold` - Minimum score threshold for entry
    pub fn compute_final_decision(&self, threshold: u8) -> ObservationDecision {
        let trend = self.compute_trend(TREND_ANALYSIS_WINDOW);
        let final_score = self.weighted_average_score();
        let peak = self.peak_score();
        let lowest = self.lowest_score();
        let second_wave = trend == ScoreTrend::SecondWave;
        let avg_confidence = self.average_confidence();
        let rising_cycles = self.rising_momentum_cycles();
        let total_cycles = self.cycle_count();

        let (action, reason) = self.determine_action(
            final_score,
            threshold,
            &trend,
            second_wave,
            avg_confidence,
            rising_cycles,
        );

        ObservationDecision {
            action,
            final_score,
            confidence: avg_confidence,
            trend,
            second_wave_detected: second_wave,
            peak_score: peak,
            lowest_score: lowest,
            rising_momentum_cycles: rising_cycles,
            reason,
            total_cycles,
        }
    }

    /// Determine action based on aggregated metrics
    fn determine_action(
        &self,
        score: u8,
        threshold: u8,
        trend: &ScoreTrend,
        second_wave: bool,
        confidence: f32,
        rising_cycles: usize,
    ) -> (ObservationAction, String) {
        // === ENTER CONDITIONS ===

        // 1. Second wave detected with score near threshold
        if second_wave && score >= threshold.saturating_sub(SECOND_WAVE_SCORE_BUFFER) {
            return (
                ObservationAction::Enter,
                format!("Second wave detected! Score {} with rising momentum", score),
            );
        }

        // 2. Score above threshold + Rising/Stable trend + good confidence
        if score >= threshold
            && matches!(trend, ScoreTrend::Rising | ScoreTrend::Stable)
            && confidence > MEDIUM_CONFIDENCE_THRESHOLD
        {
            return (
                ObservationAction::Enter,
                format!(
                    "Score {} >= threshold {}, trend {:?}, conf {:.0}%",
                    score,
                    threshold,
                    trend,
                    confidence * 100.0
                ),
            );
        }

        // 3. Rising trend with high confidence, score slightly below threshold
        if score >= threshold.saturating_sub(RISING_TREND_SCORE_BUFFER)
            && *trend == ScoreTrend::Rising
            && confidence > HIGH_CONFIDENCE_THRESHOLD
        {
            return (
                ObservationAction::Enter,
                format!(
                    "Rising trend with high confidence {:.0}%, score {}",
                    confidence * 100.0,
                    score
                ),
            );
        }

        // === SKIP CONDITIONS ===

        // 1. Score below threshold with falling trend
        if score < threshold && *trend == ScoreTrend::Falling {
            return (
                ObservationAction::Skip,
                format!("Score {} < threshold {}, falling trend", score, threshold),
            );
        }

        // 2. Critically low score
        if score < CRITICAL_LOW_SCORE {
            return (
                ObservationAction::Skip,
                format!("Critical low score: {}", score),
            );
        }

        // 3. No momentum detected after many cycles
        if rising_cycles == 0 && self.cycle_count() >= MIN_CYCLES_NO_MOMENTUM {
            return (
                ObservationAction::Skip,
                format!("No momentum detected after {} cycles", self.cycle_count()),
            );
        }

        // 4. Low confidence
        if confidence < LOW_CONFIDENCE_THRESHOLD {
            return (
                ObservationAction::Skip,
                format!("Low confidence: {:.0}%", confidence * 100.0),
            );
        }

        // === DEFAULT: Skip if criteria not met ===
        (
            ObservationAction::Skip,
            format!(
                "Score {} below threshold {}, trend {:?}",
                score, threshold, trend
            ),
        )
    }

    /// Get a summary string for logging
    pub fn summary(&self, threshold: u8) -> String {
        let decision = self.compute_final_decision(threshold);
        format!(
            "{} | Score: {} | Trend: {} | Peak: {} | Low: {} | Conf: {:.0}% | Cycles: {}",
            decision.action,
            decision.final_score,
            decision.trend,
            decision.peak_score,
            decision.lowest_score,
            decision.confidence * 100.0,
            decision.total_cycles
        )
    }

    // =========================================================================
    // TCF Integration Methods
    // =========================================================================

    /// Compute TCF result for Final Verdict from stored observations.
    ///
    /// This method processes all TCF observations collected during scoring cycles
    /// and produces a final TcfResult for use in the Final Verdict.
    ///
    /// # Arguments
    ///
    /// * `tcf_config` - TCF configuration with modulation parameters
    ///
    /// # Returns
    ///
    /// A `TcfResult` containing the computed TCF score and diagnostics.
    pub fn compute_tcf_result(&self, tcf_config: &crate::config::TcfConfig) -> TcfResult {
        let start = std::time::Instant::now();

        // Check if TCF is enabled
        if !tcf_config.enabled {
            return TcfResult {
                tcf_score: 0.5,
                is_primed: false,
                observation_count: 0,
                phase: TcfPhase::ColdStart,
                cliff_detected: false,
                latest_cohesion: 0.5,
                latest_cohesion_computed_this_cycle: false,
                latest_cohesion_is_fallback: true,
                latest_cohesion_fallback_reason: Some("neutral_default_tcf_disabled"),
                avg_cohesion: 0.5,
                trend_direction: 0,
                modulation_factor: tcf_config.tcf_min_modulation
                    + tcf_config.tcf_modulation_range * 0.5,
                analysis_time_us: start.elapsed().as_micros() as u64,
            };
        }

        // Collect cycles with TCF data so data_moved can be derived from cycle-level ts/tx.
        let cycles_with_tcf: Vec<&CycleScore> = self
            .cycles
            .iter()
            .filter(|c| c.tcf_observation.is_some())
            .collect();

        // Not enough observations for meaningful TCF
        if cycles_with_tcf.len() < tcf_config.min_updates_for_primed {
            return TcfResult {
                tcf_score: 0.5,
                is_primed: false,
                observation_count: cycles_with_tcf.len(),
                phase: TcfPhase::ColdStart,
                cliff_detected: false,
                latest_cohesion: 0.5,
                latest_cohesion_computed_this_cycle: false,
                latest_cohesion_is_fallback: true,
                latest_cohesion_fallback_reason: Some("neutral_default_insufficient_observations"),
                avg_cohesion: 0.5,
                trend_direction: 0,
                modulation_factor: tcf_config.tcf_min_modulation
                    + tcf_config.tcf_modulation_range * 0.5,
                analysis_time_us: start.elapsed().as_micros() as u64,
            };
        }

        // Build TCF instance and process observations
        let mut tcf = TrendCohesionField::new();

        let mut prev_ts_ms: Option<u64> = None;
        let mut prev_tx_count: Option<u64> = None;
        for cycle in &cycles_with_tcf {
            let Some(obs) = cycle.tcf_observation.as_ref() else {
                continue;
            };
            let observation = MarketObservation::new(
                obs.price_delta,
                obs.volume_delta,
                obs.liquidity_entropy,
                obs.order_flow_imbalance,
                obs.mpcf,
                obs.jitter,
                obs.phase_sync,
            );
            let current_ts = cycle.timestamp_ms;
            let current_tx = cycle.tx_count_total as u64;
            let data_moved = match (prev_ts_ms, prev_tx_count) {
                (Some(prev_ts), Some(prev_tx)) => (current_tx > prev_tx) || (current_ts != prev_ts),
                _ => true,
            };
            let _ = tcf.update_with_progress(&observation, data_moved);
            prev_ts_ms = Some(current_ts);
            prev_tx_count = Some(current_tx);
        }

        // Get diagnostics
        let diagnostics = tcf.get_diagnostics();
        let tcf_score = tcf.get_tcf_score();

        // Get cohesion values
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
        let avg_cohesion = if diagnostics.recent_cohesions.is_empty() {
            0.5
        } else {
            diagnostics.recent_cohesions.iter().sum::<f64>()
                / diagnostics.recent_cohesions.len() as f64
        };

        // Determine phase
        let phase = self.determine_tcf_phase(&diagnostics, avg_cohesion);

        // Apply phase-based modulation to TCF score.
        // When phase is Pump, Dump, or Chaos the raw tcf_score may remain
        // high because cohesion itself is high, but the phase classification
        // indicates a risky pattern. Scale the effective score by the phase's
        // modulation_factor so the downstream modulation_factor reflects
        // actual risk instead of being a diagnostic-only label.
        let phase_factor = phase.modulation_factor();
        let phase_modulated_tcf_score = (tcf_score * phase_factor).clamp(0.0, 1.0);

        // Recalculate modulation factor using phase-modulated TCF score
        let modulation_factor = tcf_config.tcf_min_modulation
            + tcf_config.tcf_modulation_range * phase_modulated_tcf_score;

        TcfResult {
            tcf_score: phase_modulated_tcf_score,
            is_primed: diagnostics.is_primed,
            observation_count: cycles_with_tcf.len(),
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

    /// Determine TCF phase from diagnostics and average cohesion
    fn determine_tcf_phase(
        &self,
        diagnostics: &crate::oracle::tcf::TcfDiagnostics,
        avg_cohesion: f64,
    ) -> TcfPhase {
        if !diagnostics.is_primed {
            return TcfPhase::ColdStart;
        }

        // Check cliff conditions first
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

        TcfPhase::Stable
    }

    /// Get the count of cycles with TCF observations
    pub fn tcf_observation_count(&self) -> usize {
        self.cycles
            .iter()
            .filter(|c| c.tcf_observation.is_some())
            .count()
    }

    /// Check if TCF has enough observations for meaningful analysis
    pub fn tcf_is_ready(&self, min_observations: usize) -> bool {
        self.tcf_observation_count() >= min_observations
    }
}

impl TcfObservationData {
    /// Create from a MarketObservation
    pub fn from_observation(obs: &MarketObservation) -> Self {
        Self {
            price_delta: obs.price_delta,
            volume_delta: obs.volume_delta,
            liquidity_entropy: obs.liquidity_entropy,
            order_flow_imbalance: obs.order_flow_imbalance,
            mpcf: obs.mpcf,
            jitter: obs.jitter,
            phase_sync: obs.phase_sync,
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cycle(
        cycle_num: u32,
        score: u8,
        confidence: f32,
        action: SecondWaveAction,
    ) -> CycleScore {
        CycleScore {
            cycle_num,
            timestamp_ms: cycle_num as u64 * 400,
            survivor_score: score,
            confidence,
            tx_count_delta: 1,
            tx_count_total: cycle_num as usize,
            second_wave_action: action,
            momentum: 1.0,
            tcf_observation: None,
        }
    }

    #[test]
    fn test_empty_history() {
        let history = ScoreHistory::new(38, "test".to_string());
        assert_eq!(history.cycle_count(), 0);
        assert_eq!(history.peak_score(), 0);
        assert_eq!(history.weighted_average_score(), 0);
        assert_eq!(history.compute_trend(5), ScoreTrend::Insufficient);
    }

    #[test]
    fn test_push_and_count() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        history.push_cycle(make_cycle(1, 50, 0.7, SecondWaveAction::Wait));
        history.push_cycle(make_cycle(2, 55, 0.7, SecondWaveAction::Wait));
        history.push_cycle(make_cycle(3, 60, 0.7, SecondWaveAction::Wait));

        assert_eq!(history.cycle_count(), 3);
        assert_eq!(history.peak_score(), 60);
        assert_eq!(history.lowest_score(), 50);
    }

    #[test]
    fn test_weighted_average() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        // Add cycles with increasing scores - newer should have more weight
        history.push_cycle(make_cycle(1, 40, 0.8, SecondWaveAction::Wait));
        history.push_cycle(make_cycle(2, 50, 0.8, SecondWaveAction::Wait));
        history.push_cycle(make_cycle(3, 60, 0.8, SecondWaveAction::Wait));

        let avg = history.weighted_average_score();
        // Should be weighted toward 60 (newer)
        assert!(avg > 50, "Weighted average {} should be > 50", avg);
    }

    #[test]
    fn test_rising_trend() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        // Consistent rising pattern
        for i in 1..=10 {
            history.push_cycle(make_cycle(
                i,
                40 + (i as u8 * 5),
                0.7,
                SecondWaveAction::Wait,
            ));
        }

        let trend = history.compute_trend(5);
        assert_eq!(trend, ScoreTrend::Rising);
    }

    #[test]
    fn test_falling_trend() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        // Consistent falling pattern
        for i in 1..=10 {
            history.push_cycle(make_cycle(
                i,
                80 - (i as u8 * 5),
                0.7,
                SecondWaveAction::Wait,
            ));
        }

        let trend = history.compute_trend(5);
        assert_eq!(trend, ScoreTrend::Falling);
    }

    #[test]
    fn test_stable_trend() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        // Stable scores with minor fluctuation
        for i in 1..=10 {
            let score = 50 + (i as u8 % 3); // 50, 51, 52, 50, 51, ...
            history.push_cycle(make_cycle(i, score, 0.7, SecondWaveAction::Wait));
        }

        let trend = history.compute_trend(5);
        assert_eq!(trend, ScoreTrend::Stable);
    }

    #[test]
    fn test_second_wave_detection() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // Earlier cycles: Wait/Skip (stagnation)
        for i in 1..=7 {
            history.push_cycle(make_cycle(i, 40, 0.6, SecondWaveAction::Wait));
        }

        // Last 3 cycles: Prepare/Enter (rising momentum)
        history.push_cycle(make_cycle(8, 50, 0.7, SecondWaveAction::Prepare));
        history.push_cycle(make_cycle(9, 55, 0.8, SecondWaveAction::Prepare));
        history.push_cycle(make_cycle(10, 60, 0.9, SecondWaveAction::Enter));

        let trend = history.compute_trend(10);
        assert_eq!(trend, ScoreTrend::SecondWave);
    }

    #[test]
    fn test_early_exit_condition() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // 10 cycles with low scores
        for i in 1..=10 {
            history.push_cycle(make_cycle(i, 20, 0.5, SecondWaveAction::Skip));
        }

        assert!(history.should_early_exit());
    }

    #[test]
    fn test_no_early_exit_with_good_peak() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // 10 cycles with one good score
        for i in 1..=9 {
            history.push_cycle(make_cycle(i, 20, 0.5, SecondWaveAction::Skip));
        }
        history.push_cycle(make_cycle(10, 40, 0.7, SecondWaveAction::Wait)); // Peak above threshold

        assert!(!history.should_early_exit());
    }

    #[test]
    fn test_decision_enter_above_threshold() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        for i in 1..=10 {
            history.push_cycle(make_cycle(
                i,
                65 + (i as u8 % 3),
                0.8,
                SecondWaveAction::Prepare,
            ));
        }

        let decision = history.compute_final_decision(60);
        assert_eq!(decision.action, ObservationAction::Enter);
        assert!(decision.final_score >= 60);
    }

    #[test]
    fn test_decision_skip_below_threshold_falling() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // Falling scores below threshold
        for i in 1..=10 {
            history.push_cycle(make_cycle(
                i,
                50 - (i as u8 * 3),
                0.6,
                SecondWaveAction::Skip,
            ));
        }

        let decision = history.compute_final_decision(60);
        assert_eq!(decision.action, ObservationAction::Skip);
    }

    #[test]
    fn test_decision_enter_on_second_wave() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // Build up second wave pattern
        for i in 1..=5 {
            history.push_cycle(make_cycle(i, 45, 0.6, SecondWaveAction::Wait));
        }
        for i in 6..=8 {
            history.push_cycle(make_cycle(i, 52, 0.8, SecondWaveAction::Prepare));
        }
        history.push_cycle(make_cycle(9, 55, 0.85, SecondWaveAction::Enter));
        history.push_cycle(make_cycle(10, 58, 0.9, SecondWaveAction::Enter));

        let decision = history.compute_final_decision(60);
        // Should enter due to second wave even though below 60
        assert_eq!(decision.action, ObservationAction::Enter);
        assert!(decision.second_wave_detected);
    }

    #[test]
    fn test_capacity_overflow() {
        let mut history = ScoreHistory::new(5, "test".to_string());

        // Add more than capacity
        for i in 1..=10 {
            history.push_cycle(make_cycle(i, 50, 0.7, SecondWaveAction::Wait));
        }

        // Should only keep last 5
        assert_eq!(history.cycle_count(), 5);
    }

    #[test]
    fn test_observation_duration() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        let mut c1 = make_cycle(1, 50, 0.7, SecondWaveAction::Wait);
        c1.timestamp_ms = 0;
        history.push_cycle(c1);

        let mut c2 = make_cycle(2, 55, 0.7, SecondWaveAction::Wait);
        c2.timestamp_ms = 400;
        history.push_cycle(c2);

        let mut c3 = make_cycle(3, 60, 0.7, SecondWaveAction::Wait);
        c3.timestamp_ms = 800;
        history.push_cycle(c3);

        assert_eq!(history.observation_duration_ms(), 800);
    }

    #[test]
    fn test_tcf_observation_count() {
        let mut history = ScoreHistory::new(38, "test".to_string());

        // Add cycles without TCF observations
        history.push_cycle(make_cycle(1, 50, 0.7, SecondWaveAction::Wait));
        history.push_cycle(make_cycle(2, 55, 0.7, SecondWaveAction::Wait));

        assert_eq!(history.tcf_observation_count(), 0);

        // Add cycle with TCF observation
        let mut cycle_with_tcf = make_cycle(3, 60, 0.7, SecondWaveAction::Wait);
        cycle_with_tcf.tcf_observation = Some(TcfObservationData {
            price_delta: 0.1,
            volume_delta: 0.2,
            liquidity_entropy: 0.6,
            order_flow_imbalance: 0.3,
            mpcf: 0.7,
            jitter: 0.5,
            phase_sync: 0.2,
        });
        history.push_cycle(cycle_with_tcf);

        assert_eq!(history.tcf_observation_count(), 1);
        assert!(!history.tcf_is_ready(3)); // Need at least 3
    }

    #[test]
    fn test_tcf_compute_result_cold_start() {
        let history = ScoreHistory::new(38, "test".to_string());
        let tcf_config = crate::config::TcfConfig::default();

        let result = history.compute_tcf_result(&tcf_config);

        // No observations - should be cold start
        assert!(!result.is_primed);
        assert_eq!(result.observation_count, 0);
        assert_eq!(result.phase, TcfPhase::ColdStart);
        assert_eq!(result.tcf_score, 0.5); // Neutral default
    }

    #[test]
    fn test_tcf_compute_result_with_observations() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        let tcf_config = crate::config::TcfConfig::default();

        // Add cycles with TCF observations simulating organic growth
        for i in 1..=5 {
            let mut cycle = make_cycle(i, 50 + i as u8 * 5, 0.7, SecondWaveAction::Wait);
            cycle.tcf_observation = Some(TcfObservationData {
                price_delta: 0.1 + 0.02 * i as f64,
                volume_delta: 0.1 + 0.01 * i as f64,
                liquidity_entropy: 0.6,
                order_flow_imbalance: 0.2,
                mpcf: 0.7,
                jitter: 0.5,
                phase_sync: 0.2,
            });
            history.push_cycle(cycle);
        }

        let result = history.compute_tcf_result(&tcf_config);

        // Should have processed all observations
        assert!(result.is_primed);
        assert_eq!(result.observation_count, 5);
        // Score should be reasonable (not 0 or 1)
        assert!(result.tcf_score > 0.0 && result.tcf_score <= 1.0);
        // Modulation factor should be calculated
        let expected_mod =
            tcf_config.tcf_min_modulation + tcf_config.tcf_modulation_range * result.tcf_score;
        assert!((result.modulation_factor - expected_mod).abs() < 0.01);
    }

    #[test]
    fn test_tcf_compute_result_never_pump_when_cycle_data_stale() {
        let mut history = ScoreHistory::new(38, "test".to_string());
        let tcf_config = crate::config::TcfConfig::default();

        // Repeated timestamp/tx_count simulates stale cycle-level input.
        for i in 1..=5 {
            let mut cycle = make_cycle(i, 55, 0.8, SecondWaveAction::Wait);
            cycle.timestamp_ms = 1_000;
            cycle.tx_count_total = 10;
            cycle.tcf_observation = Some(TcfObservationData {
                price_delta: 0.4,
                volume_delta: 0.4,
                liquidity_entropy: 0.7,
                order_flow_imbalance: 0.6,
                mpcf: 0.8,
                jitter: 0.2,
                phase_sync: 0.3,
            });
            history.push_cycle(cycle);
        }

        let result = history.compute_tcf_result(&tcf_config);
        assert_ne!(result.phase, TcfPhase::Pump);
    }

    #[test]
    fn test_tcf_phase_modulation_applied_to_score() {
        // Verify that phase modulation_factor is applied to tcf_score,
        // so Pump/Dump/Chaos phases reduce the effective score instead
        // of being diagnostic-only labels with zero score impact.
        let tcf_config = crate::config::TcfConfig::default();

        // OrganicGrowth has modulation_factor=1.0, so score should be unchanged.
        // Pump has modulation_factor=0.6, so score should be reduced.
        // Dump has modulation_factor=0.2, so score should be strongly reduced.

        // The modulation formula is: tcf_score * phase.modulation_factor()
        let organic_mod = TcfPhase::OrganicGrowth.modulation_factor();
        let pump_mod = TcfPhase::Pump.modulation_factor();
        let dump_mod = TcfPhase::Dump.modulation_factor();

        assert!(
            (organic_mod - 1.0).abs() < 0.01,
            "OrganicGrowth should have modulation_factor=1.0, got {}",
            organic_mod
        );
        assert!(
            pump_mod < 1.0,
            "Pump modulation_factor should be < 1.0, got {}",
            pump_mod
        );
        assert!(
            dump_mod < pump_mod,
            "Dump modulation_factor ({}) should be less than Pump ({})",
            dump_mod,
            pump_mod
        );

        // Verify that for a given raw_tcf_score, phase modulation reduces the effective score
        let raw_tcf = 0.8;
        let pump_effective = raw_tcf * pump_mod;
        let organic_effective = raw_tcf * organic_mod;

        assert!(
            pump_effective < organic_effective,
            "Pump-modulated score ({:.3}) should be less than organic ({:.3})",
            pump_effective,
            organic_effective
        );

        // Verify the resulting modulation_factor differs
        let pump_mf =
            tcf_config.tcf_min_modulation + tcf_config.tcf_modulation_range * pump_effective;
        let organic_mf =
            tcf_config.tcf_min_modulation + tcf_config.tcf_modulation_range * organic_effective;

        assert!(
            pump_mf < organic_mf,
            "Pump modulation_factor ({:.3}) should be less than organic ({:.3})",
            pump_mf,
            organic_mf
        );
    }
}
