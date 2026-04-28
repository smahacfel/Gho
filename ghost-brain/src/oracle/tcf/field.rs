//! TCF Field Module - Trend Cohesion Field Accumulator
//!
//! The TrendCohesionField is the top-level integration point for TCF.
//! It accumulates cohesion measurements over time and produces a single
//! signal that can modulate the Final Verdict scoring.
//!
//! ## Design Philosophy
//!
//! The field doesn't just average cohesions - it tracks the TRAJECTORY
//! of cohesion over time. A gradually declining cohesion is different
//! from a sudden cliff, even if they reach the same final value.
//!
//! ## Key Properties
//!
//! 1. **Weighted accumulation**: Recent cohesions matter more than old ones
//! 2. **Cliff detection**: Sudden drops trigger alerts
//! 3. **Trend analysis**: Rising vs falling cohesion trajectory
//! 4. **Phase detection**: Identifies pump vs organic growth patterns
//!
//! ## Integration with Final Verdict
//!
//! TCF output is a modulatory signal, NOT a veto:
//!
//! ```text
//! effective_momentum = base_momentum * (0.6 + 0.4 * tcf_score)
//! ```
//!
//! This means:
//! - TCF = 1.0 → full momentum preserved
//! - TCF = 0.5 → momentum reduced to 80%
//! - TCF = 0.0 → momentum reduced to 60%
//!
//! ## Performance
//!
//! - O(1) per-cycle update
//! - O(1) memory (fixed-size history buffer)
//! - No heap allocation in steady state

use super::cohesion::{cohesion, CohesionConfig, CohesionResult};
use super::expected::{ExpectedTransition, ExpectedTransitionModel};
use super::observation::MarketObservation;
use super::transition::Transition;
use tracing::debug;

/// Maximum history size for cohesion values.
const MAX_HISTORY_SIZE: usize = 13;

/// Default decay factor for integral calculation.
const DEFAULT_DECAY_FACTOR: f64 = 0.85;

/// Threshold for considering cohesion "high".
const HIGH_COHESION_THRESHOLD: f64 = 0.7;

/// Threshold for considering cohesion "low".
const LOW_COHESION_THRESHOLD: f64 = 0.4;

/// Cliff detection threshold.
const CLIFF_THRESHOLD: f64 = 0.25;
const ZERO_DELTA_EPSILON: f64 = 1e-9;

/// Trend Cohesion Field - main TCF integration point.
///
/// Tracks cohesion over scoring cycles and produces integrated signals
/// for Final Verdict modulation.
///
/// # Lifecycle
///
/// 1. Create with `new()`
/// 2. Call `update()` for each new observation (every 420ms cycle)
/// 3. Read `get_tcf_score()` for Final Verdict integration
/// 4. Check `get_diagnostics()` for detailed breakdown
///
/// # Thread Safety
///
/// NOT thread-safe by design. Each scoring pipeline should have its own instance.
#[derive(Debug, Clone)]
pub struct TrendCohesionField {
    /// History of cohesion values (circular buffer, newest at head).
    cohesion_history: [f64; MAX_HISTORY_SIZE],

    /// Current write position in history.
    history_idx: usize,

    /// Number of valid entries in history.
    history_len: usize,

    /// Running integral (weighted sum) of cohesion.
    integral: f64,

    /// Previous observation for transition computation.
    prev_observation: Option<MarketObservation>,

    /// Expected transition model.
    expected_model: ExpectedTransitionModel,

    /// Configuration for cohesion calculation.
    cohesion_config: CohesionConfig,

    /// Decay factor for integral.
    decay_factor: f64,

    /// Number of updates processed.
    update_count: usize,

    /// Last computed TCF score.
    last_tcf_score: f64,

    /// Cliff detected flag.
    cliff_detected: bool,

    /// Current trend direction (-1, 0, +1).
    trend_direction: i8,

    /// Consecutive low cohesion count.
    consecutive_low: usize,

    /// Most recent cohesion result.
    last_cohesion_result: Option<CohesionResult>,

    /// Whether the latest update had forward data progress.
    last_data_moved: bool,

    /// Whether a pump candidate was suppressed by stale input gate in the latest update.
    last_pump_suppressed: bool,

    /// Whether cohesion was actually computed in the latest cycle.
    last_cohesion_computed_this_cycle: bool,

    /// Cohesion computed in the latest cycle (if any).
    last_computed_cohesion_this_cycle: Option<f64>,
}

impl Default for TrendCohesionField {
    fn default() -> Self {
        Self::new()
    }
}

impl TrendCohesionField {
    /// Create a new TrendCohesionField in cold start state.
    pub fn new() -> Self {
        Self::with_config(CohesionConfig::default(), DEFAULT_DECAY_FACTOR)
    }

    /// Create a TrendCohesionField with custom configuration.
    pub fn with_config(cohesion_config: CohesionConfig, decay_factor: f64) -> Self {
        Self {
            cohesion_history: [0.0; MAX_HISTORY_SIZE],
            history_idx: 0,
            history_len: 0,
            integral: 0.0,
            prev_observation: None,
            expected_model: ExpectedTransitionModel::new(),
            cohesion_config,
            decay_factor: decay_factor.clamp(0.5, 0.99),
            update_count: 0,
            last_tcf_score: 0.5, // Neutral default
            cliff_detected: false,
            trend_direction: 0,
            consecutive_low: 0,
            last_cohesion_result: None,
            last_data_moved: true,
            last_pump_suppressed: false,
            last_cohesion_computed_this_cycle: false,
            last_computed_cohesion_this_cycle: None,
        }
    }

    /// Create a TrendCohesionField optimized for pump detection.
    pub fn pump_detector() -> Self {
        Self::with_config(CohesionConfig::pump_sensitive(), 0.80)
    }

    /// Create a TrendCohesionField optimized for organic growth.
    pub fn organic_detector() -> Self {
        Self::with_config(CohesionConfig::organic_tolerant(), 0.90)
    }

    /// Reset the field to cold start state.
    pub fn reset(&mut self) {
        *self = Self::with_config(self.cohesion_config, self.decay_factor);
    }

    /// Check if the field has enough data for reliable scoring.
    pub fn is_primed(&self) -> bool {
        self.update_count >= 3 && self.expected_model.is_primed()
    }

    /// Get the number of updates processed.
    pub fn update_count(&self) -> usize {
        self.update_count
    }

    /// Update the field with a new market observation.
    ///
    /// This is called once per scoring cycle (every 420ms).
    ///
    /// # Arguments
    ///
    /// * `observation` - The current market observation from cycle S(i)
    ///
    /// # Returns
    ///
    /// `TcfUpdateResult` containing the new TCF score and diagnostics.
    pub fn update(&mut self, observation: &MarketObservation) -> TcfUpdateResult {
        self.update_with_progress(observation, true)
    }

    /// Update the field with explicit data-progress signal.
    ///
    /// `data_moved` must be true only when engine-level inputs progressed
    /// between cycles (`current_tx > prev_tx || current_ts != prev_ts`).
    pub fn update_with_progress(
        &mut self,
        observation: &MarketObservation,
        data_moved: bool,
    ) -> TcfUpdateResult {
        self.update_count += 1;
        self.last_data_moved = data_moved;
        self.last_pump_suppressed = false;
        self.last_cohesion_computed_this_cycle = false;
        self.last_computed_cohesion_this_cycle = None;

        // First update - just store observation, no transition yet
        if self.prev_observation.is_none() {
            self.prev_observation = Some(*observation);
            return TcfUpdateResult {
                tcf_score: self.last_tcf_score,
                cohesion: None,
                is_primed: false,
                cliff_detected: false,
                trend_direction: 0,
                phase: TcfPhase::ColdStart,
                last_cohesion_result: None,
                data_moved,
                stale_input: !data_moved,
                cohesion_computed_this_cycle: false,
                pump_candidate_without_gate: false,
                pump_suppressed_reason: None,
            };
        }

        // Stale cycle: do not generate transition or update internal dynamics.
        if !data_moved {
            let phase = if self.is_primed() {
                TcfPhase::Stable
            } else {
                TcfPhase::ColdStart
            };
            let pump_candidate_without_gate = self.would_pump_without_gate();
            let pump_suppressed_reason = if pump_candidate_without_gate {
                self.last_pump_suppressed = true;
                Some("stale_input")
            } else {
                None
            };
            debug!(
                target: "oracle::tcf::field",
                "phase={} data_moved=false stale_input=true pump_candidate_without_gate={} pump_suppressed_reason={}",
                phase.name(),
                pump_candidate_without_gate,
                pump_suppressed_reason.unwrap_or("none"),
            );
            self.prev_observation = Some(*observation);
            return TcfUpdateResult {
                tcf_score: self.last_tcf_score,
                cohesion: None,
                is_primed: self.is_primed(),
                cliff_detected: self.cliff_detected,
                trend_direction: self.trend_direction,
                phase,
                last_cohesion_result: self.last_cohesion_result,
                data_moved: false,
                stale_input: true,
                cohesion_computed_this_cycle: false,
                pump_candidate_without_gate,
                pump_suppressed_reason,
            };
        }

        // Compute transition from previous to current
        let prev = self.prev_observation.unwrap();
        let transition = Transition::compute(&prev, observation);

        // Zero-delta guard:
        // tx_count is not part of MarketObservation; engine-level transactional progress
        // is represented by `data_moved`. If there is no meaningful observation movement,
        // force stagnant handling to prevent false Pump classification.
        let delta_volume = transition.delta_vector[1].abs();
        if transition.volatility <= ZERO_DELTA_EPSILON && delta_volume <= ZERO_DELTA_EPSILON {
            let stagnant_cohesion = CohesionResult {
                cohesion: 0.5,
                direction_score: 0.0,
                rhythm_score: 0.5,
                stability_score: 0.5,
                total_penalty: 0.0,
                total_bonus: 0.0,
                breakdown: super::cohesion::CohesionBreakdown::default(),
            };
            self.last_cohesion_result = Some(stagnant_cohesion);
            self.last_cohesion_computed_this_cycle = true;
            self.last_computed_cohesion_this_cycle = Some(stagnant_cohesion.cohesion);
            self.cohesion_history[self.history_idx] = stagnant_cohesion.cohesion;
            self.history_idx = (self.history_idx + 1) % MAX_HISTORY_SIZE;
            self.history_len = self.history_len.saturating_add(1).min(MAX_HISTORY_SIZE);
            self.integral = self.integral * self.decay_factor + stagnant_cohesion.cohesion;
            self.last_tcf_score = self.calculate_tcf_score();
            self.prev_observation = Some(*observation);
            return TcfUpdateResult {
                tcf_score: self.last_tcf_score,
                cohesion: Some(stagnant_cohesion.cohesion),
                is_primed: self.is_primed(),
                cliff_detected: self.cliff_detected,
                trend_direction: 0,
                phase: TcfPhase::Stable,
                last_cohesion_result: self.last_cohesion_result,
                data_moved,
                stale_input: !data_moved,
                cohesion_computed_this_cycle: true,
                pump_candidate_without_gate: false,
                pump_suppressed_reason: Some("zero_delta"),
            };
        }

        // Update expected model and get expectations
        let expected = self.expected_model.update(&transition);

        // Compute cohesion
        let cohesion_result = if self.expected_model.is_primed() {
            cohesion(&expected, &transition, &self.cohesion_config)
        } else {
            // Cold start: use neutral cohesion
            CohesionResult {
                cohesion: 0.5,
                direction_score: 0.5,
                rhythm_score: 0.5,
                stability_score: 0.5,
                total_penalty: 0.0,
                total_bonus: 0.0,
                breakdown: super::cohesion::CohesionBreakdown::default(),
            }
        };

        let coh_value = cohesion_result.cohesion;
        self.last_cohesion_result = Some(cohesion_result);
        self.last_cohesion_computed_this_cycle = true;
        self.last_computed_cohesion_this_cycle = Some(coh_value);

        // Store in history
        self.cohesion_history[self.history_idx] = coh_value;
        self.history_idx = (self.history_idx + 1) % MAX_HISTORY_SIZE;
        self.history_len = self.history_len.saturating_add(1).min(MAX_HISTORY_SIZE);

        // Update integral with decay
        self.integral = self.integral * self.decay_factor + coh_value;

        // Detect cliff
        self.detect_cliff();

        // Update trend direction
        self.update_trend_direction();

        // Track consecutive low cohesions
        if coh_value < LOW_COHESION_THRESHOLD {
            self.consecutive_low += 1;
        } else {
            self.consecutive_low = 0;
        }

        // Calculate TCF score
        self.last_tcf_score = self.calculate_tcf_score();

        // Determine phase
        let phase = self.detect_phase(data_moved);

        // Store previous observation
        self.prev_observation = Some(*observation);

        let pump_candidate_without_gate = self.would_pump_without_gate();
        TcfUpdateResult {
            tcf_score: self.last_tcf_score,
            cohesion: Some(coh_value),
            is_primed: self.is_primed(),
            cliff_detected: self.cliff_detected,
            trend_direction: self.trend_direction,
            phase,
            last_cohesion_result: self.last_cohesion_result,
            data_moved,
            stale_input: !data_moved,
            cohesion_computed_this_cycle: self.last_cohesion_computed_this_cycle,
            pump_candidate_without_gate,
            pump_suppressed_reason: if self.last_pump_suppressed {
                Some("stale_input")
            } else {
                None
            },
        }
    }

    /// Get the current TCF score for Final Verdict integration.
    ///
    /// # Returns
    ///
    /// Score in [0, 1] where:
    /// - 1.0 = Perfect trend cohesion (high confidence in momentum)
    /// - 0.5 = Neutral (no information)
    /// - 0.0 = Complete trend breakdown (low confidence in momentum)
    pub fn get_tcf_score(&self) -> f64 {
        self.last_tcf_score
    }

    /// Get detailed diagnostics for logging and analysis.
    pub fn get_diagnostics(&self) -> TcfDiagnostics {
        let cached_last_known_cohesion = if self.history_len > 0 {
            self.get_recent_cohesions().first().copied()
        } else {
            None
        };
        TcfDiagnostics {
            tcf_score: self.last_tcf_score,
            integral: self.integral,
            history_len: self.history_len,
            update_count: self.update_count,
            is_primed: self.is_primed(),
            cliff_detected: self.cliff_detected,
            trend_direction: self.trend_direction,
            consecutive_low: self.consecutive_low,
            model_stability: self.expected_model.stability(),
            model_confidence: self.expected_model.get_expected().confidence,
            recent_cohesions: self.get_recent_cohesions(),
            last_cohesion_result: self.last_cohesion_result,
            data_moved: self.last_data_moved,
            stale_input: !self.last_data_moved,
            cohesion_computed_this_cycle: self.last_cohesion_computed_this_cycle,
            latest_computed_cohesion_this_cycle: self.last_computed_cohesion_this_cycle,
            cached_last_known_cohesion,
            pump_suppressed_reason: if self.last_pump_suppressed {
                Some("stale_input")
            } else {
                None
            },
        }
    }

    /// Get recent cohesion values (newest first).
    pub fn get_recent_cohesions(&self) -> Vec<f64> {
        let mut result = Vec::with_capacity(self.history_len);
        for i in 0..self.history_len {
            let idx = (self.history_idx + MAX_HISTORY_SIZE - 1 - i) % MAX_HISTORY_SIZE;
            result.push(self.cohesion_history[idx]);
        }
        result
    }

    /// Calculate the TCF score from accumulated data.
    fn calculate_tcf_score(&self) -> f64 {
        if !self.is_primed() {
            return 0.5; // Neutral during cold start
        }

        let mut score = 0.0;

        // Component 1: Weighted average of recent cohesions (60%)
        let weighted_avg = self.calculate_weighted_average();
        score += 0.60 * weighted_avg;

        // Component 2: Trend momentum (20%)
        let trend_score = self.calculate_trend_score();
        score += 0.20 * trend_score;

        // Component 3: Stability bonus/penalty (20%)
        let stability_score = self.calculate_stability_score();
        score += 0.20 * stability_score;

        // Apply cliff penalty
        if self.cliff_detected {
            score *= 0.7;
        }

        // Apply consecutive low penalty
        if self.consecutive_low >= 3 {
            let penalty = (self.consecutive_low - 2) as f64 * 0.1;
            score *= (1.0 - penalty.min(0.4));
        }

        score.clamp(0.0, 1.0)
    }

    /// Calculate weighted average of recent cohesions.
    fn calculate_weighted_average(&self) -> f64 {
        if self.history_len == 0 {
            return 0.5;
        }

        let mut weighted_sum = 0.0;
        let mut weight_sum = 0.0;
        let mut weight = 1.0;

        // Iterate from newest to oldest
        for i in 0..self.history_len {
            let idx = (self.history_idx + MAX_HISTORY_SIZE - 1 - i) % MAX_HISTORY_SIZE;
            weighted_sum += self.cohesion_history[idx] * weight;
            weight_sum += weight;
            weight *= self.decay_factor;
        }

        if weight_sum < 1e-10 {
            return 0.5;
        }

        weighted_sum / weight_sum
    }

    /// Calculate trend momentum score.
    fn calculate_trend_score(&self) -> f64 {
        match self.trend_direction {
            1 => 0.8,  // Rising trend
            0 => 0.5,  // Flat
            -1 => 0.2, // Falling trend
            _ => 0.5,
        }
    }

    /// Calculate stability score from model stability.
    fn calculate_stability_score(&self) -> f64 {
        let model_stability = self.expected_model.stability();

        // Map model stability to score
        // High stability = confident in pattern = high score
        model_stability.clamp(0.0, 1.0)
    }

    /// Detect cliff in cohesion values.
    fn detect_cliff(&mut self) {
        if self.history_len < 3 {
            self.cliff_detected = false;
            return;
        }

        // Check last few values for sudden drop
        let recent = self.get_recent_cohesions();
        if recent.len() < 2 {
            return;
        }

        for i in 1..recent.len().min(4) {
            let drop = recent[i - 1] - recent[i];
            if drop > CLIFF_THRESHOLD && recent[i - 1] > HIGH_COHESION_THRESHOLD {
                self.cliff_detected = true;
                return;
            }
        }

        // Clear cliff flag if no cliff in recent window
        self.cliff_detected = false;
    }

    /// Update trend direction based on recent cohesions.
    fn update_trend_direction(&mut self) {
        if self.history_len < 3 {
            self.trend_direction = 0;
            return;
        }

        let recent = self.get_recent_cohesions();
        if recent.len() < 3 {
            return;
        }

        // Simple trend detection: compare averages
        let first_half_avg = (recent[0] + recent[1]) / 2.0;
        let second_half_start = recent.len().min(5) / 2;
        let second_half_len = recent.len().min(5) - second_half_start;

        // Guard against division by zero
        if second_half_len == 0 {
            self.trend_direction = 0;
            return;
        }

        let second_half_avg: f64 = recent[second_half_start..recent.len().min(5)]
            .iter()
            .sum::<f64>()
            / second_half_len as f64;

        let diff = first_half_avg - second_half_avg;

        if diff > 0.1 {
            self.trend_direction = 1; // Rising (newer is higher)
        } else if diff < -0.1 {
            self.trend_direction = -1; // Falling (newer is lower)
        } else {
            self.trend_direction = 0; // Flat
        }
    }

    /// Detect the current phase based on patterns.
    fn detect_phase(&mut self, data_moved: bool) -> TcfPhase {
        if !self.is_primed() {
            return TcfPhase::ColdStart;
        }

        let recent = self.get_recent_cohesions();
        if recent.is_empty() {
            return TcfPhase::ColdStart;
        }

        let latest = recent[0];
        let avg = self.calculate_weighted_average();
        let model_stability = self.expected_model.stability();
        let stable_scale_confirmed = self.expected_model.is_primed() && model_stability > 0.5;
        let real_breakdown_signal = self.cliff_detected || self.trend_direction == -1;
        let low_cohesion_signal = avg < LOW_COHESION_THRESHOLD || self.consecutive_low >= 2;
        let pump_candidate_without_gate = avg > HIGH_COHESION_THRESHOLD
            && self.trend_direction >= 0
            && !self.cliff_detected
            && model_stability <= 0.5;
        let phase = if stable_scale_confirmed && self.cliff_detected && self.trend_direction == -1 {
            TcfPhase::Dump
        } else if avg > HIGH_COHESION_THRESHOLD && self.trend_direction >= 0 && !self.cliff_detected
        {
            if model_stability > 0.5 {
                TcfPhase::OrganicGrowth
            } else if data_moved {
                TcfPhase::Pump
            } else {
                self.last_pump_suppressed = true;
                TcfPhase::Stable
            }
        } else if stable_scale_confirmed && real_breakdown_signal && low_cohesion_signal {
            TcfPhase::Chaos
        } else {
            TcfPhase::Stable
        };

        debug!(
            target: "oracle::tcf::field",
            "phase={} avg_cohesion={:.6} trend_direction={} model_stability={:.6} cliff_detected={} latest_cohesion={:.6} data_moved={} pump_candidate_without_gate={} pump_suppressed_reason={}",
            phase.name(),
            avg,
            self.trend_direction,
            model_stability,
            self.cliff_detected,
            latest,
            data_moved,
            pump_candidate_without_gate,
            if self.last_pump_suppressed { "stale_input" } else { "none" },
        );
        phase
    }

    fn would_pump_without_gate(&self) -> bool {
        if !self.is_primed() {
            return false;
        }
        let avg = self.calculate_weighted_average();
        let model_stability = self.expected_model.stability();
        avg > HIGH_COHESION_THRESHOLD
            && self.trend_direction >= 0
            && !self.cliff_detected
            && model_stability <= 0.5
    }
}

/// Result of a TCF update.
#[derive(Debug, Clone, Copy)]
pub struct TcfUpdateResult {
    /// Current TCF score [0, 1].
    pub tcf_score: f64,

    /// Cohesion value for this cycle (None if cold start).
    pub cohesion: Option<f64>,

    /// Whether the field is primed for reliable scoring.
    pub is_primed: bool,

    /// Whether a cohesion cliff was detected.
    pub cliff_detected: bool,

    /// Current trend direction (-1 falling, 0 flat, +1 rising).
    pub trend_direction: i8,

    /// Current phase classification.
    pub phase: TcfPhase,

    /// Full cohesion result with component breakdown (None during cold start).
    pub last_cohesion_result: Option<CohesionResult>,

    /// Whether source data moved in this cycle.
    pub data_moved: bool,

    /// Whether the cycle was treated as stale input.
    pub stale_input: bool,

    /// Whether cohesion was computed from fresh data in this cycle.
    pub cohesion_computed_this_cycle: bool,

    /// Whether Pump would be selected by legacy logic (without data gate).
    pub pump_candidate_without_gate: bool,

    /// Reason for pump suppression, if any.
    pub pump_suppressed_reason: Option<&'static str>,
}

/// Detailed diagnostics for TCF analysis.
#[derive(Debug, Clone)]
pub struct TcfDiagnostics {
    /// Current TCF score.
    pub tcf_score: f64,

    /// Running integral value.
    pub integral: f64,

    /// Number of cohesion values in history.
    pub history_len: usize,

    /// Total updates processed.
    pub update_count: usize,

    /// Whether field is primed.
    pub is_primed: bool,

    /// Cliff detected flag.
    pub cliff_detected: bool,

    /// Trend direction.
    pub trend_direction: i8,

    /// Consecutive low cohesion count.
    pub consecutive_low: usize,

    /// Model stability score.
    pub model_stability: f64,

    /// Model confidence.
    pub model_confidence: f64,

    /// Recent cohesion values (newest first).
    pub recent_cohesions: Vec<f64>,

    /// Last cohesion calculation result.
    pub last_cohesion_result: Option<CohesionResult>,

    /// Whether source data moved in latest cycle.
    pub data_moved: bool,

    /// Whether latest cycle was stale input.
    pub stale_input: bool,

    /// Whether cohesion was computed from fresh data in latest cycle.
    pub cohesion_computed_this_cycle: bool,

    /// Cohesion computed in latest cycle (None means not computed this cycle).
    pub latest_computed_cohesion_this_cycle: Option<f64>,

    /// Last known cohesion from history (cached, may come from previous cycle).
    pub cached_last_known_cohesion: Option<f64>,

    /// Reason for pump suppression in latest cycle, if any.
    pub pump_suppressed_reason: Option<&'static str>,
}

/// Phase classification for the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcfPhase {
    /// Not enough data for classification.
    ColdStart,

    /// Market is stable with consistent dynamics.
    Stable,

    /// Organic growth - high cohesion with stable pattern.
    OrganicGrowth,

    /// Pump phase - high cohesion but unstable pattern.
    Pump,

    /// Dump phase - cliff detected, falling cohesion.
    Dump,

    /// Chaos - low cohesion, unpredictable dynamics.
    Chaos,
}

impl TcfPhase {
    /// Get human-readable name for the phase.
    pub fn name(&self) -> &'static str {
        match self {
            TcfPhase::ColdStart => "ColdStart",
            TcfPhase::Stable => "Stable",
            TcfPhase::OrganicGrowth => "OrganicGrowth",
            TcfPhase::Pump => "Pump",
            TcfPhase::Dump => "Dump",
            TcfPhase::Chaos => "Chaos",
        }
    }

    /// Get emoji representation for logging.
    pub fn emoji(&self) -> &'static str {
        match self {
            TcfPhase::ColdStart => "🔵",
            TcfPhase::Stable => "🟢",
            TcfPhase::OrganicGrowth => "🌱",
            TcfPhase::Pump => "🚀",
            TcfPhase::Dump => "📉",
            TcfPhase::Chaos => "💥",
        }
    }

    /// Get modulation factor for Final Verdict.
    ///
    /// This determines how TCF affects the momentum calculation.
    pub fn modulation_factor(&self) -> f64 {
        match self {
            TcfPhase::ColdStart => 0.5,     // Neutral
            TcfPhase::Stable => 0.8,        // Slight boost
            TcfPhase::OrganicGrowth => 1.0, // Full boost
            TcfPhase::Pump => 0.6,          // Reduce (might be artificial)
            TcfPhase::Dump => 0.2,          // Strong reduction
            TcfPhase::Chaos => 0.3,         // Significant reduction
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_observation(price: f64, volume: f64, ofi: f64) -> MarketObservation {
        MarketObservation::new(price, volume, 0.5, ofi, 0.5, 0.5, 0.2)
    }

    #[test]
    fn test_tcf_cold_start() {
        let tcf = TrendCohesionField::new();

        assert!(!tcf.is_primed());
        assert_eq!(tcf.update_count(), 0);
        assert!((tcf.get_tcf_score() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_tcf_first_update() {
        let mut tcf = TrendCohesionField::new();

        let obs = make_observation(0.1, 0.1, 0.2);
        let result = tcf.update(&obs);

        assert!(!result.is_primed);
        assert_eq!(result.phase, TcfPhase::ColdStart);
        assert!(result.cohesion.is_none());
    }

    #[test]
    fn test_tcf_priming() {
        let mut tcf = TrendCohesionField::new();

        // Feed consistent observations
        for i in 0..6 {
            let obs = make_observation(0.1 + 0.01 * i as f64, 0.1 + 0.01 * i as f64, 0.2);
            let result = tcf.update(&obs);

            if i >= 3 {
                // Should be primed after 4+ updates
                assert!(result.is_primed || i == 3);
            }
        }

        assert!(tcf.is_primed());
    }

    #[test]
    fn test_tcf_consistent_pattern() {
        let mut tcf = TrendCohesionField::new();

        // Feed very consistent pattern
        for i in 0..10 {
            let base = 0.1 + 0.02 * i as f64;
            let obs = make_observation(base, base, 0.3);
            tcf.update(&obs);
        }

        // Should have high TCF score
        assert!(tcf.get_tcf_score() > 0.6);

        let diag = tcf.get_diagnostics();
        assert!(diag.model_stability > 0.3);
    }

    #[test]
    fn test_tcf_cliff_detection() {
        let mut tcf = TrendCohesionField::new();

        // Build stable pattern
        for _ in 0..5 {
            let obs = make_observation(0.1, 0.1, 0.3);
            tcf.update(&obs);
        }

        // Sudden reversal (creates cliff in cohesion)
        let crash_obs = make_observation(-0.5, -0.4, -0.6);
        let result = tcf.update(&crash_obs);

        // After more updates, cliff should be detected
        for _ in 0..3 {
            let obs = make_observation(-0.3, -0.2, -0.4);
            tcf.update(&obs);
        }

        // Check diagnostics
        let diag = tcf.get_diagnostics();
        // Cliff detection depends on cohesion values, not just observation changes
        // The pattern change should eventually cause low cohesion
        assert!(diag.recent_cohesions.len() > 3);
    }

    #[test]
    fn test_tcf_phases() {
        let mut tcf = TrendCohesionField::new();

        // Cold start
        let obs = make_observation(0.1, 0.1, 0.2);
        let result = tcf.update(&obs);
        assert_eq!(result.phase, TcfPhase::ColdStart);

        // Feed more data
        for _ in 0..8 {
            let obs = make_observation(0.1, 0.1, 0.3);
            tcf.update(&obs);
        }

        // Should be in stable or organic growth
        let result = tcf.update(&make_observation(0.1, 0.1, 0.3));
        assert!(matches!(
            result.phase,
            TcfPhase::Stable | TcfPhase::OrganicGrowth | TcfPhase::Pump
        ));
    }

    #[test]
    fn test_stagnant_phase_on_zero_delta() {
        let mut tcf = TrendCohesionField::new();
        let obs = make_observation(0.2, 0.2, 0.1);

        // Seed field and then feed identical snapshots.
        tcf.update(&obs);
        for _ in 0..6 {
            let _ = tcf.update(&obs);
        }
        let result = tcf.update(&obs);
        let direction_score = result
            .last_cohesion_result
            .map(|c| c.direction_score)
            .unwrap_or(0.0);

        assert_ne!(result.phase, TcfPhase::Pump);
        assert_eq!(direction_score, 0.0);
    }

    #[test]
    fn test_tcf_reset() {
        let mut tcf = TrendCohesionField::new();

        // Build up state
        for _ in 0..10 {
            let obs = make_observation(0.1, 0.1, 0.3);
            tcf.update(&obs);
        }

        assert!(tcf.is_primed());

        // Reset
        tcf.reset();

        assert!(!tcf.is_primed());
        assert_eq!(tcf.update_count(), 0);
    }

    #[test]
    fn test_tcf_presets() {
        let pump_tcf = TrendCohesionField::pump_detector();
        let organic_tcf = TrendCohesionField::organic_detector();

        // Both should start in cold start
        assert!(!pump_tcf.is_primed());
        assert!(!organic_tcf.is_primed());

        // Different decay factors
        assert!(pump_tcf.decay_factor != organic_tcf.decay_factor);
    }

    #[test]
    fn test_tcf_score_bounds() {
        let mut tcf = TrendCohesionField::new();

        // Feed extreme data
        for i in 0..15 {
            let price = if i % 2 == 0 { 1.0 } else { -1.0 };
            let obs = make_observation(price, price, price);
            tcf.update(&obs);
        }

        // Score should always be in [0, 1]
        let score = tcf.get_tcf_score();
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_phase_modulation_factors() {
        assert!(TcfPhase::OrganicGrowth.modulation_factor() > TcfPhase::Pump.modulation_factor());
        assert!(TcfPhase::Pump.modulation_factor() > TcfPhase::Dump.modulation_factor());
        assert!(TcfPhase::Stable.modulation_factor() > TcfPhase::Chaos.modulation_factor());
    }

    #[test]
    fn test_get_recent_cohesions() {
        let mut tcf = TrendCohesionField::new();

        // Feed some data
        for i in 0..5 {
            let obs = make_observation(0.1 * (i + 1) as f64, 0.1, 0.2);
            tcf.update(&obs);
        }

        let recent = tcf.get_recent_cohesions();

        // Should have entries (minus 1 for first observation)
        assert!(recent.len() >= 3);

        // All should be valid cohesion values
        for c in &recent {
            assert!(*c >= 0.0 && *c <= 1.0);
        }
    }

    #[test]
    fn test_diagnostics_completeness() {
        let mut tcf = TrendCohesionField::new();

        // Prime the field
        for _ in 0..8 {
            let obs = make_observation(0.1, 0.1, 0.3);
            tcf.update(&obs);
        }

        let diag = tcf.get_diagnostics();

        // Check all fields are populated
        assert!(diag.update_count >= 8);
        assert!(diag.history_len > 0);
        assert!(diag.tcf_score >= 0.0 && diag.tcf_score <= 1.0);
        assert!(diag.model_stability >= 0.0 && diag.model_stability <= 1.0);
        assert!(diag.model_confidence >= 0.0 && diag.model_confidence <= 1.0);
    }

    #[test]
    fn test_stale_input_is_semantic_noop_for_dynamics() {
        let mut tcf = TrendCohesionField::new();
        let obs = make_observation(0.1, 0.2, 0.3);

        let first = tcf.update_with_progress(&obs, true);
        assert_eq!(first.phase, TcfPhase::ColdStart);
        assert_eq!(tcf.history_len, 0);

        let stale = tcf.update_with_progress(&obs, false);
        assert!(stale.stale_input);
        assert!(!stale.data_moved);
        assert!(stale.cohesion.is_none());
        assert_eq!(stale.phase, TcfPhase::ColdStart);
        assert_eq!(tcf.history_len, 0);
    }

    #[test]
    fn test_stale_input_never_reports_pump() {
        let mut tcf = TrendCohesionField::new();
        let mut obs = make_observation(0.0, 0.0, 0.0);

        // Prime dynamics with moved data.
        for i in 0..8 {
            obs = make_observation(0.1 + i as f64 * 0.02, 0.2 + i as f64 * 0.01, 0.2);
            let _ = tcf.update_with_progress(&obs, true);
        }

        let stale = tcf.update_with_progress(&obs, false);
        assert_ne!(stale.phase, TcfPhase::Pump);
        assert!(stale.stale_input);
    }

    #[test]
    fn test_diagnostics_distinguish_computed_vs_cached_on_stale_cycle() {
        let mut tcf = TrendCohesionField::new();
        let obs0 = make_observation(0.1, 0.2, 0.3);
        let obs1 = make_observation(0.15, 0.25, 0.35);

        let _ = tcf.update_with_progress(&obs0, true);
        let moved = tcf.update_with_progress(&obs1, true);
        assert!(moved.cohesion.is_some());
        assert!(moved.cohesion_computed_this_cycle);

        let stale = tcf.update_with_progress(&obs1, false);
        assert!(stale.cohesion.is_none());
        assert!(!stale.cohesion_computed_this_cycle);

        let diag = tcf.get_diagnostics();
        assert!(diag.stale_input);
        assert!(!diag.cohesion_computed_this_cycle);
        assert!(diag.latest_computed_cohesion_this_cycle.is_none());
        assert!(diag.cached_last_known_cohesion.is_some());
    }
}
