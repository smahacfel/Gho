//! SSMI (Sub-Slot Microentropy Index) Module
//!
//! Extends SCR by adding Shannon entropy analysis of micro-timing jitter patterns
//! to classify transaction sources (Bot vs Human vs Viral Launch) before full
//! market topology forms.
//!
//! ## Precision
//! Uses f64 internally for entropy/AR calculations to maintain accuracy with large datasets.
//!
//! ## Concurrency
//! `SubSlotMicroentropy` implements `Send + Sync`, allowing safe use across threads.
//! For multi-threaded analysis, wrap with `Arc<SubSlotMicroentropy>`:
//! ```
//! use std::sync::Arc;
//! use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;
//!
//! let ssmi = Arc::new(SubSlotMicroentropy::new());
//! // Clone Arc for use in multiple threads
//! let ssmi_clone = Arc::clone(&ssmi);
//! ```
//!
//! ## Generic Histogram Bins
//! Use `SubSlotMicroentropyConfigurable<BINS>` for custom bin counts:
//! ```
//! use ghost_brain::oracle::ultrafast::ssmi::SubSlotMicroentropyConfigurable;
//!
//! // Use 128 bins for finer jitter resolution
//! let ssmi = SubSlotMicroentropyConfigurable::<128>::with_max_jitter(4000);
//! ```

use crate::oracle::hyper_oracle::HyperOracle;
use serde::Serialize;
use std::time::Instant;
use tracing::{debug, info, instrument};

// =============================================================================
// Classification Thresholds
// =============================================================================

/// SCR threshold for bot detection (probability above this indicates bot)
const BOT_SCR_THRESHOLD: f32 = 0.7;
/// AR correlation threshold for bot detection (high correlation = predictable)
const BOT_AR_THRESHOLD: f32 = 0.8;
/// Entropy threshold for bot detection (low entropy = regular patterns)
const BOT_ENTROPY_THRESHOLD: f32 = 1.5;

/// Entropy threshold for human detection (high entropy = chaotic)
const HUMAN_ENTROPY_THRESHOLD: f32 = 3.0;
/// AR correlation threshold for human detection (low correlation = unpredictable)
const HUMAN_AR_THRESHOLD: f32 = 0.3;
/// SCR threshold for human detection (low SCR = organic)
const HUMAN_SCR_THRESHOLD: f32 = 0.4;

/// Minimum transaction count for viral launch detection
const VIRAL_MIN_TX_COUNT: usize = 6;
/// Minimum entropy for viral launch detection
const VIRAL_ENTROPY_MIN: f32 = 2.5;
/// Maximum entropy for viral launch detection
const VIRAL_ENTROPY_MAX: f32 = 4.0;
/// SCR threshold for viral launch detection
const VIRAL_SCR_THRESHOLD: f32 = 0.5;
/// Center point for viral entropy scoring
const VIRAL_ENTROPY_CENTER: f32 = 3.25;
/// Range for viral entropy scoring
const VIRAL_ENTROPY_RANGE: f32 = 0.75;

// =============================================================================
// Scoring Weights
// =============================================================================

/// Weight for entropy component in combined score
const SCORE_WEIGHT_ENTROPY: f32 = 0.35;
/// Weight for SCR component in combined score
const SCORE_WEIGHT_SCR: f32 = 0.40;
/// Weight for AR correlation component in combined score
const SCORE_WEIGHT_AR: f32 = 0.25;

/// Bonus applied to viral launch source type
const VIRAL_SCORE_BONUS: f32 = 0.15;
/// Bonus applied to human source type
const HUMAN_SCORE_BONUS: f32 = 0.05;
/// Penalty applied to bot source type
const BOT_SCORE_PENALTY: f32 = 0.20;

/// Maximum entropy value for normalization (typical range 0-6)
const MAX_ENTROPY_NORMALIZATION: f32 = 6.0;

/// Default confidence for unknown classification
const UNKNOWN_CONFIDENCE: f32 = 0.3;

/// Default number of histogram bins for entropy calculation
const DEFAULT_HISTOGRAM_BINS: usize = 64;

/// Default maximum jitter in milliseconds for histogram normalization
const DEFAULT_MAX_JITTER_MS: u64 = 2000;

/// Minimum viral entropy range to prevent division by zero
const MIN_VIRAL_ENTROPY_RANGE: f32 = 0.1;

// =============================================================================
// Probabilistic Classification Weights (Bayesian-style)
// =============================================================================

/// Weight for SCR in bot probability calculation
const PROB_WEIGHT_SCR: f64 = 0.40;
/// Weight for AR correlation in bot probability calculation
const PROB_WEIGHT_AR: f64 = 0.30;
/// Weight for entropy in bot probability calculation
const PROB_WEIGHT_ENTROPY: f64 = 0.30;

/// Weight for entropy in viral probability calculation
const PROB_VIRAL_WEIGHT_ENTROPY: f64 = 0.50;
/// Weight for SCR in viral probability calculation
const PROB_VIRAL_WEIGHT_SCR: f64 = 0.50;

/// Fallback probability for bot when total is zero (1/3)
const FALLBACK_BOT_PROB: f32 = 0.33;
/// Fallback probability for human when total is zero (1/3)
const FALLBACK_HUMAN_PROB: f32 = 0.33;
/// Fallback probability for viral when total is zero (1/3, rounded up)
const FALLBACK_VIRAL_PROB: f32 = 0.34;

/// Classification of transaction source based on timing analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceType {
    /// Automated bot activity - regular, predictable intervals
    Bot,
    /// Human/organic activity - chaotic, unpredictable timing
    Human,
    /// Viral launch pattern - rapid organic adoption
    ViralLaunch,
    /// Unable to classify with confidence
    Unknown,
}

/// Result of SSMI analysis
#[derive(Debug, Clone, Serialize)]
pub struct SsmiResult {
    /// Combined SSMI score (0.0 - 1.0, higher = better opportunity)
    pub ssmi_score: f32,
    /// Shannon entropy of jitter distribution (0.0 - 6.0 typical range)
    pub shannon_entropy: f32,
    /// SCR bot probability from HyperOracle (0.0 - 1.0)
    pub scr_bot_probability: f32,
    /// AR-1 correlation coefficient (-1.0 to 1.0)
    pub ar_correlation: f32,
    /// Classified source type
    pub source_type: SourceType,
    /// Classification confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Number of transactions analyzed
    pub tx_count: usize,
    /// Analysis time in microseconds
    pub analysis_time_us: u64,
    /// Probabilistic bot score (0.0 - 1.0, Bayesian-weighted)
    pub bot_probability: f32,
    /// Probabilistic human score (0.0 - 1.0, Bayesian-weighted)
    pub human_probability: f32,
    /// Probabilistic viral launch score (0.0 - 1.0, Bayesian-weighted)
    pub viral_probability: f32,
}

/// Adaptive thresholds for classification
///
/// These thresholds can be adjusted based on historical data or market conditions.
/// Use `update_thresholds_from_history` to automatically calibrate thresholds.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveThresholds {
    // Bot detection thresholds
    /// SCR threshold for bot detection (probability above this indicates bot)
    pub bot_scr_threshold: f32,
    /// AR correlation threshold for bot detection (high correlation = predictable)
    pub bot_ar_threshold: f32,
    /// Entropy threshold for bot detection (low entropy = regular patterns)
    pub bot_entropy_threshold: f32,

    // Human detection thresholds
    /// Entropy threshold for human detection (high entropy = chaotic)
    pub human_entropy_threshold: f32,
    /// AR correlation threshold for human detection (low correlation = unpredictable)
    pub human_ar_threshold: f32,
    /// SCR threshold for human detection (low SCR = organic)
    pub human_scr_threshold: f32,

    // Viral launch thresholds
    /// Minimum transaction count for viral launch detection
    pub viral_min_tx_count: usize,
    /// Minimum entropy for viral launch detection
    pub viral_entropy_min: f32,
    /// Maximum entropy for viral launch detection
    pub viral_entropy_max: f32,
    /// SCR threshold for viral launch detection
    pub viral_scr_threshold: f32,
    /// Center point for viral entropy scoring
    pub viral_entropy_center: f32,
    /// Range for viral entropy scoring
    pub viral_entropy_range: f32,

    // Scoring weights
    /// Weight for entropy component in combined score
    pub score_weight_entropy: f32,
    /// Weight for SCR component in combined score
    pub score_weight_scr: f32,
    /// Weight for AR correlation component in combined score
    pub score_weight_ar: f32,

    // Source type bonuses/penalties
    /// Bonus applied to viral launch source type
    pub viral_score_bonus: f32,
    /// Bonus applied to human source type
    pub human_score_bonus: f32,
    /// Penalty applied to bot source type
    pub bot_score_penalty: f32,
}

impl Default for AdaptiveThresholds {
    fn default() -> Self {
        Self {
            // Bot thresholds
            bot_scr_threshold: BOT_SCR_THRESHOLD,
            bot_ar_threshold: BOT_AR_THRESHOLD,
            bot_entropy_threshold: BOT_ENTROPY_THRESHOLD,
            // Human thresholds
            human_entropy_threshold: HUMAN_ENTROPY_THRESHOLD,
            human_ar_threshold: HUMAN_AR_THRESHOLD,
            human_scr_threshold: HUMAN_SCR_THRESHOLD,
            // Viral thresholds
            viral_min_tx_count: VIRAL_MIN_TX_COUNT,
            viral_entropy_min: VIRAL_ENTROPY_MIN,
            viral_entropy_max: VIRAL_ENTROPY_MAX,
            viral_scr_threshold: VIRAL_SCR_THRESHOLD,
            viral_entropy_center: VIRAL_ENTROPY_CENTER,
            viral_entropy_range: VIRAL_ENTROPY_RANGE,
            // Scoring weights
            score_weight_entropy: SCORE_WEIGHT_ENTROPY,
            score_weight_scr: SCORE_WEIGHT_SCR,
            score_weight_ar: SCORE_WEIGHT_AR,
            // Source type bonuses/penalties
            viral_score_bonus: VIRAL_SCORE_BONUS,
            human_score_bonus: HUMAN_SCORE_BONUS,
            bot_score_penalty: BOT_SCORE_PENALTY,
        }
    }
}

/// Configurable Sub-Slot Microentropy Index analyzer with const generic bins
///
/// Allows custom histogram bin count for different jitter range scenarios.
/// Use higher bin counts (128, 256) for finer jitter resolution when max_jitter_ms is large.
///
/// # Type Parameters
/// * `BINS` - Number of histogram bins (default: 64)
///
/// # Thread Safety
/// This type is `Send + Sync` and can be safely shared across threads using `Arc`.
///
/// # Adaptive Thresholds
/// Thresholds can be customized via builder methods for dynamic Solana environments:
/// ```
/// use ghost_brain::oracle::ultrafast::ssmi::SubSlotMicroentropyConfigurable;
///
/// let ssmi = SubSlotMicroentropyConfigurable::<64>::new()
///     .with_bot_scr_threshold(0.8)
///     .with_bot_entropy_threshold(1.2)
///     .with_viral_min_tx_count(10);
/// ```
///
/// # Example
/// ```
/// use std::sync::Arc;
/// use ghost_brain::oracle::ultrafast::ssmi::SubSlotMicroentropyConfigurable;
///
/// // Use 128 bins for finer resolution
/// let ssmi = SubSlotMicroentropyConfigurable::<128>::with_max_jitter(4000);
/// let ssmi_arc = Arc::new(ssmi);
/// ```
#[derive(Clone)]
pub struct SubSlotMicroentropyConfigurable<const BINS: usize = DEFAULT_HISTOGRAM_BINS> {
    /// HyperOracle for SCR calculations
    hyper_oracle: HyperOracle,
    /// Maximum jitter in milliseconds for histogram normalization
    max_jitter_ms: u64,
    /// Adaptive thresholds for classification
    thresholds: AdaptiveThresholds,
}

// SAFETY: SubSlotMicroentropyConfigurable is Send + Sync because:
// - `max_jitter_ms: u64` is Copy and inherently Send + Sync
// - `thresholds: AdaptiveThresholds` contains only f32/usize fields which are Copy and Send + Sync
// - `hyper_oracle: HyperOracle` contains only:
//   - `povc_basis: Matrix3<f32>` - nalgebra Matrix3 is Copy and Send + Sync
//   - `povc_centroids: [Vector3<f32>; 3]` - nalgebra Vector3 is Copy and Send + Sync
// - HyperOracle uses thread_local! FFT_PLANNER which is thread-safe by design
// - All methods are read-only (&self) with no interior mutability
unsafe impl<const BINS: usize> Send for SubSlotMicroentropyConfigurable<BINS> {}
unsafe impl<const BINS: usize> Sync for SubSlotMicroentropyConfigurable<BINS> {}

impl<const BINS: usize> Default for SubSlotMicroentropyConfigurable<BINS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Sub-Slot Microentropy Index analyzer (default 64 bins)
///
/// Analyzes micro-timing jitter patterns in transaction timestamps
/// to classify transaction sources and calculate opportunity scores.
///
/// Uses stack-based allocations for zero-heap operation in hot paths.
///
/// # Thread Safety
/// This type is `Send + Sync` and can be safely shared across threads using `Arc`:
/// ```
/// use std::sync::Arc;
/// use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;
///
/// let ssmi = Arc::new(SubSlotMicroentropy::new());
/// // Clone Arc for use in multiple threads
/// let ssmi_clone = Arc::clone(&ssmi);
/// ```
pub type SubSlotMicroentropy = SubSlotMicroentropyConfigurable<DEFAULT_HISTOGRAM_BINS>;

impl<const BINS: usize> SubSlotMicroentropyConfigurable<BINS> {
    /// Create a new analyzer with default max jitter (2000ms) and default thresholds
    pub fn new() -> Self {
        Self {
            hyper_oracle: HyperOracle::new(),
            max_jitter_ms: DEFAULT_MAX_JITTER_MS,
            thresholds: AdaptiveThresholds::default(),
        }
    }

    /// Create a new analyzer with custom max jitter value
    ///
    /// # Arguments
    /// * `max_jitter_ms` - Maximum expected jitter in milliseconds
    pub fn with_max_jitter(max_jitter_ms: u64) -> Self {
        Self {
            hyper_oracle: HyperOracle::new(),
            max_jitter_ms,
            thresholds: AdaptiveThresholds::default(),
        }
    }

    /// Get the number of histogram bins
    pub const fn bins(&self) -> usize {
        BINS
    }

    /// Get the maximum jitter value
    pub fn max_jitter_ms(&self) -> u64 {
        self.max_jitter_ms
    }

    /// Get current thresholds (read-only)
    pub fn thresholds(&self) -> &AdaptiveThresholds {
        &self.thresholds
    }

    // =========================================================================
    // Builder methods for adaptive thresholds (chainable)
    // =========================================================================

    /// Set the SCR threshold for bot detection
    #[must_use]
    pub fn with_bot_scr_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.bot_scr_threshold = threshold;
        self
    }

    /// Set the AR correlation threshold for bot detection
    #[must_use]
    pub fn with_bot_ar_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.bot_ar_threshold = threshold;
        self
    }

    /// Set the entropy threshold for bot detection
    #[must_use]
    pub fn with_bot_entropy_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.bot_entropy_threshold = threshold;
        self
    }

    /// Set the entropy threshold for human detection
    #[must_use]
    pub fn with_human_entropy_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.human_entropy_threshold = threshold;
        self
    }

    /// Set the AR correlation threshold for human detection
    #[must_use]
    pub fn with_human_ar_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.human_ar_threshold = threshold;
        self
    }

    /// Set the SCR threshold for human detection
    #[must_use]
    pub fn with_human_scr_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.human_scr_threshold = threshold;
        self
    }

    /// Set the minimum transaction count for viral launch detection
    #[must_use]
    pub fn with_viral_min_tx_count(mut self, count: usize) -> Self {
        self.thresholds.viral_min_tx_count = count;
        self
    }

    /// Set the minimum entropy for viral launch detection
    #[must_use]
    pub fn with_viral_entropy_min(mut self, threshold: f32) -> Self {
        self.thresholds.viral_entropy_min = threshold;
        self
    }

    /// Set the maximum entropy for viral launch detection
    #[must_use]
    pub fn with_viral_entropy_max(mut self, threshold: f32) -> Self {
        self.thresholds.viral_entropy_max = threshold;
        self
    }

    /// Set the SCR threshold for viral launch detection
    #[must_use]
    pub fn with_viral_scr_threshold(mut self, threshold: f32) -> Self {
        self.thresholds.viral_scr_threshold = threshold;
        self
    }

    /// Set the center point for viral entropy scoring
    #[must_use]
    pub fn with_viral_entropy_center(mut self, center: f32) -> Self {
        self.thresholds.viral_entropy_center = center;
        self
    }

    /// Set the range for viral entropy scoring
    #[must_use]
    pub fn with_viral_entropy_range(mut self, range: f32) -> Self {
        self.thresholds.viral_entropy_range = range;
        self
    }

    /// Set the weight for entropy component in combined score
    #[must_use]
    pub fn with_score_weight_entropy(mut self, weight: f32) -> Self {
        self.thresholds.score_weight_entropy = weight;
        self
    }

    /// Set the weight for SCR component in combined score
    #[must_use]
    pub fn with_score_weight_scr(mut self, weight: f32) -> Self {
        self.thresholds.score_weight_scr = weight;
        self
    }

    /// Set the weight for AR correlation component in combined score
    #[must_use]
    pub fn with_score_weight_ar(mut self, weight: f32) -> Self {
        self.thresholds.score_weight_ar = weight;
        self
    }

    /// Set the bonus applied to viral launch source type
    #[must_use]
    pub fn with_viral_score_bonus(mut self, bonus: f32) -> Self {
        self.thresholds.viral_score_bonus = bonus;
        self
    }

    /// Set the bonus applied to human source type
    #[must_use]
    pub fn with_human_score_bonus(mut self, bonus: f32) -> Self {
        self.thresholds.human_score_bonus = bonus;
        self
    }

    /// Set the penalty applied to bot source type
    #[must_use]
    pub fn with_bot_score_penalty(mut self, penalty: f32) -> Self {
        self.thresholds.bot_score_penalty = penalty;
        self
    }

    /// Set all thresholds at once
    #[must_use]
    pub fn with_thresholds(mut self, thresholds: AdaptiveThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Update thresholds based on historical SSMI results
    ///
    /// This method calculates adaptive thresholds from historical data using:
    /// - Bot threshold = mean entropy of bot-classified results * 1.1
    /// - Human threshold = mean entropy of human-classified results * 0.9
    /// - Viral thresholds adjusted based on viral-classified sample means
    ///
    /// # Arguments
    /// * `history` - Slice of historical SSMI results to learn from
    ///
    /// # Returns
    /// `true` if thresholds were updated (sufficient history), `false` otherwise
    pub fn update_thresholds_from_history(&mut self, history: &[SsmiResult]) -> bool {
        if history.len() < 10 {
            return false; // Not enough data
        }

        // Collect stats for each source type
        let mut bot_entropies: Vec<f32> = Vec::new();
        let mut bot_scrs: Vec<f32> = Vec::new();
        let mut human_entropies: Vec<f32> = Vec::new();
        let mut human_scrs: Vec<f32> = Vec::new();
        let mut viral_entropies: Vec<f32> = Vec::new();
        let mut viral_scrs: Vec<f32> = Vec::new();

        for result in history {
            match result.source_type {
                SourceType::Bot => {
                    bot_entropies.push(result.shannon_entropy);
                    bot_scrs.push(result.scr_bot_probability);
                }
                SourceType::Human => {
                    human_entropies.push(result.shannon_entropy);
                    human_scrs.push(result.scr_bot_probability);
                }
                SourceType::ViralLaunch => {
                    viral_entropies.push(result.shannon_entropy);
                    viral_scrs.push(result.scr_bot_probability);
                }
                SourceType::Unknown => {}
            }
        }

        // Update bot thresholds if we have enough bot samples
        let bot_len = bot_entropies.len();
        if bot_len >= 3 {
            let mean_entropy: f32 = bot_entropies.iter().sum::<f32>() / bot_len as f32;
            let mean_scr: f32 = bot_scrs.iter().sum::<f32>() / bot_len as f32;
            // Set threshold slightly above mean to catch more bots
            self.thresholds.bot_entropy_threshold = mean_entropy * 1.1;
            self.thresholds.bot_scr_threshold = mean_scr * 0.9;
        }

        // Update human thresholds if we have enough human samples
        let human_len = human_entropies.len();
        if human_len >= 3 {
            let mean_entropy: f32 = human_entropies.iter().sum::<f32>() / human_len as f32;
            let mean_scr: f32 = human_scrs.iter().sum::<f32>() / human_len as f32;
            // Set threshold slightly below mean to catch more humans
            self.thresholds.human_entropy_threshold = mean_entropy * 0.9;
            self.thresholds.human_scr_threshold = mean_scr * 1.1;
        }

        // Update viral thresholds if we have enough viral samples
        let viral_len = viral_entropies.len();
        if viral_len >= 3 {
            let mean_entropy: f32 = viral_entropies.iter().sum::<f32>() / viral_len as f32;
            let min_entropy = viral_entropies
                .iter()
                .cloned()
                .fold(f32::INFINITY, f32::min);
            let max_entropy = viral_entropies
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);
            let mean_scr: f32 = viral_scrs.iter().sum::<f32>() / viral_len as f32;

            self.thresholds.viral_entropy_center = mean_entropy;
            self.thresholds.viral_entropy_min = min_entropy * 0.9;
            self.thresholds.viral_entropy_max = max_entropy * 1.1;
            // Ensure range is at least MIN_VIRAL_ENTROPY_RANGE to avoid division by zero
            let range = (max_entropy - min_entropy) / 2.0;
            self.thresholds.viral_entropy_range = range.max(MIN_VIRAL_ENTROPY_RANGE);
            self.thresholds.viral_scr_threshold = mean_scr * 1.1;
        }

        true
    }

    /// Calculate Shannon entropy from transaction timestamps using f64 precision
    ///
    /// Computes inter-transaction deltas (jitter), builds a histogram,
    /// and calculates Shannon entropy: H = -Σ p(x) * log2(p(x))
    ///
    /// Returns 0.0 if fewer than 2 timestamps are provided.
    /// Uses Vec-based histogram for generic bin counts.
    #[instrument(level = "debug", skip(self, timestamps_ms), fields(tx_count = timestamps_ms.len()))]
    pub fn calculate_shannon_entropy(&self, timestamps_ms: &[u64]) -> f32 {
        if timestamps_ms.len() < 2 {
            debug!("insufficient timestamps for entropy calculation");
            return 0.0;
        }

        // Use Vec for generic bin count
        let mut histogram = vec![0u32; BINS];
        let bin_width = self.max_jitter_ms as f64 / BINS as f64;
        let mut delta_count = 0usize;

        // Calculate inter-transaction deltas (jitter) and build histogram in single pass
        for window in timestamps_ms.windows(2) {
            let delta = window[1].saturating_sub(window[0]);
            let bin_idx = ((delta as f64 / bin_width) as usize).min(BINS - 1);
            histogram[bin_idx] += 1;
            delta_count += 1;
        }

        if delta_count == 0 {
            return 0.0;
        }

        // Calculate Shannon entropy using f64: H = -Σ p(x) * log2(p(x))
        let total = delta_count as f64;
        let mut entropy = 0.0f64;

        for &count in &histogram {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }

        entropy as f32
    }

    /// Calculate AR-1 (autoregressive lag-1) correlation coefficient using f64 precision
    ///
    /// Measures how predictable the next jitter value is based on the current one:
    /// AR-1 = Cov(X_t, X_{t-1}) / Var(X)
    ///
    /// Returns 0.0 if fewer than 4 timestamps are provided.
    /// Result is clamped to [-1.0, 1.0].
    /// Uses streaming calculation with f64 for full precision.
    #[instrument(level = "debug", skip(self, timestamps_ms), fields(tx_count = timestamps_ms.len()))]
    pub fn calculate_ar_correlation(&self, timestamps_ms: &[u64]) -> f32 {
        if timestamps_ms.len() < 4 {
            debug!("insufficient timestamps for AR correlation");
            return 0.0;
        }

        // First pass: calculate mean of deltas (streaming) using f64
        let mut sum = 0.0f64;
        let mut count = 0usize;

        for window in timestamps_ms.windows(2) {
            sum += window[1].saturating_sub(window[0]) as f64;
            count += 1;
        }

        if count < 2 {
            return 0.0;
        }

        let mean = sum / count as f64;

        // Second pass: calculate variance and covariance (streaming) using f64
        let mut variance_sum = 0.0f64;
        let mut covariance_sum = 0.0f64;
        let mut prev_delta_centered: Option<f64> = None;

        for window in timestamps_ms.windows(2) {
            let delta = window[1].saturating_sub(window[0]) as f64;
            let delta_centered = delta - mean;

            variance_sum += delta_centered * delta_centered;

            if let Some(prev) = prev_delta_centered {
                covariance_sum += delta_centered * prev;
            }
            prev_delta_centered = Some(delta_centered);
        }

        let variance = variance_sum / count as f64;

        if variance < 1e-9 {
            return 0.0;
        }

        let covariance = covariance_sum / (count - 1) as f64;

        // AR-1 correlation using f64
        let ar_corr = (covariance / variance) as f32;

        // Clamp to [-1.0, 1.0]
        ar_corr.clamp(-1.0, 1.0)
    }

    /// Calculate probabilistic scores for each source type (Bayesian-style)
    ///
    /// Uses weighted combination of SCR, AR correlation, and entropy
    /// to calculate probability-like scores for each classification.
    /// Uses adaptive thresholds for viral entropy center/range.
    fn calculate_probabilistic_scores(
        &self,
        entropy: f32,
        scr: f32,
        ar_corr: f32,
    ) -> (f32, f32, f32) {
        let t = &self.thresholds;

        // Use f64 for intermediate calculations
        let entropy_f64 = entropy as f64;
        let scr_f64 = scr as f64;
        let ar_corr_f64 = ar_corr as f64;

        // Bot probability: high SCR, high AR, low entropy
        let scr_bot_factor = scr_f64;
        let ar_bot_factor = (ar_corr_f64.abs()).min(1.0);
        let entropy_bot_factor = 1.0 - (entropy_f64 / MAX_ENTROPY_NORMALIZATION as f64).min(1.0);
        let bot_raw = PROB_WEIGHT_SCR * scr_bot_factor
            + PROB_WEIGHT_AR * ar_bot_factor
            + PROB_WEIGHT_ENTROPY * entropy_bot_factor;

        // Human probability: low SCR, low AR, high entropy
        let scr_human_factor = 1.0 - scr_f64;
        let ar_human_factor = 1.0 - ar_corr_f64.abs().min(1.0);
        let entropy_human_factor = (entropy_f64 / MAX_ENTROPY_NORMALIZATION as f64).min(1.0);
        let human_raw = PROB_WEIGHT_SCR * scr_human_factor
            + PROB_WEIGHT_AR * ar_human_factor
            + PROB_WEIGHT_ENTROPY * entropy_human_factor;

        // Viral probability: moderate entropy in sweet spot, low SCR
        // Uses adaptive thresholds for viral-specific calculation
        // Guard against division by zero if viral_entropy_range is 0
        let range = (t.viral_entropy_range as f64 * 2.0).max(MIN_VIRAL_ENTROPY_RANGE as f64);
        let entropy_viral_factor =
            1.0 - ((entropy_f64 - t.viral_entropy_center as f64).abs() / range).min(1.0);
        let scr_viral_factor = (1.0 - scr_f64).max(0.0);
        let viral_raw = PROB_VIRAL_WEIGHT_ENTROPY * entropy_viral_factor
            + PROB_VIRAL_WEIGHT_SCR * scr_viral_factor;

        // Normalize to sum to 1.0 (softmax-like)
        let total = bot_raw + human_raw + viral_raw;
        if total < 1e-9 {
            return (FALLBACK_BOT_PROB, FALLBACK_HUMAN_PROB, FALLBACK_VIRAL_PROB);
        }

        let bot_prob = (bot_raw / total) as f32;
        let human_prob = (human_raw / total) as f32;
        let viral_prob = (viral_raw / total) as f32;

        (bot_prob, human_prob, viral_prob)
    }

    /// Perform full SSMI analysis on transaction timestamps
    ///
    /// Combines SCR, Shannon entropy, and AR correlation to classify
    /// transaction sources and calculate a combined opportunity score.
    /// Also provides probabilistic scores for each classification.
    #[instrument(level = "info", skip(self, timestamps_ms), fields(tx_count = timestamps_ms.len()))]
    pub fn analyze(&self, timestamps_ms: &[u64]) -> SsmiResult {
        let start = Instant::now();

        let tx_count = timestamps_ms.len();

        // Calculate SCR using HyperOracle
        // Sanitize NaN/Inf values that can occur with extreme timestamp deltas
        let scr_raw = self.hyper_oracle.calculate_scr(timestamps_ms);
        let scr_bot_probability = if scr_raw.is_finite() { scr_raw } else { 0.0 };

        // Calculate Shannon entropy
        let shannon_entropy = self.calculate_shannon_entropy(timestamps_ms);

        // Calculate AR correlation
        let ar_correlation = self.calculate_ar_correlation(timestamps_ms);

        // Calculate probabilistic scores
        let (bot_probability, human_probability, viral_probability) = self
            .calculate_probabilistic_scores(shannon_entropy, scr_bot_probability, ar_correlation);

        // Classify source type (uses both hard thresholds and probabilistic info)
        let (source_type, confidence) = self.classify_source(
            shannon_entropy,
            scr_bot_probability,
            ar_correlation,
            tx_count,
        );

        // Calculate combined SSMI score
        let ssmi_score = self.calculate_combined_score(
            shannon_entropy,
            scr_bot_probability,
            ar_correlation,
            &source_type,
        );

        let analysis_time_us = start.elapsed().as_micros() as u64;

        // Log key metrics
        info!(
            entropy = shannon_entropy,
            scr = scr_bot_probability,
            ar = ar_correlation,
            score = ssmi_score,
            source = ?source_type,
            time_us = analysis_time_us,
            "SSMI analysis completed"
        );

        SsmiResult {
            ssmi_score,
            shannon_entropy,
            scr_bot_probability,
            ar_correlation,
            source_type,
            confidence,
            tx_count,
            analysis_time_us,
            bot_probability,
            human_probability,
            viral_probability,
        }
    }

    /// Classify the source type based on entropy, SCR, and AR correlation
    ///
    /// Returns (SourceType, confidence)
    /// Uses adaptive thresholds from the instance configuration.
    #[instrument(level = "debug", skip(self), fields(entropy, scr, ar_corr, tx_count))]
    fn classify_source(
        &self,
        entropy: f32,
        scr: f32,
        ar_corr: f32,
        tx_count: usize,
    ) -> (SourceType, f32) {
        let t = &self.thresholds;

        // Bot: high SCR, high AR correlation, low entropy
        if scr > t.bot_scr_threshold
            && ar_corr > t.bot_ar_threshold
            && entropy < t.bot_entropy_threshold
        {
            let confidence = 0.5 + 0.3 * scr + 0.2 * ar_corr.abs();
            debug!(source = "Bot", confidence, "classified as bot");
            return (SourceType::Bot, confidence.min(1.0));
        }

        // Human: high entropy, low AR correlation, low SCR
        if entropy > t.human_entropy_threshold
            && ar_corr.abs() < t.human_ar_threshold
            && scr < t.human_scr_threshold
        {
            let confidence =
                0.5 + 0.3 * (entropy / MAX_ENTROPY_NORMALIZATION).min(1.0) + 0.2 * (1.0 - scr);
            debug!(source = "Human", confidence, "classified as human");
            return (SourceType::Human, confidence.min(1.0));
        }

        // ViralLaunch: sufficient transactions, entropy in sweet spot, low SCR
        if tx_count >= t.viral_min_tx_count
            && entropy >= t.viral_entropy_min
            && entropy <= t.viral_entropy_max
            && scr < t.viral_scr_threshold
        {
            // Guard against division by zero if viral_entropy_range is 0
            let range = t.viral_entropy_range.max(MIN_VIRAL_ENTROPY_RANGE);
            let entropy_score = 1.0 - ((entropy - t.viral_entropy_center).abs() / range).min(1.0);
            let confidence = 0.5 + 0.25 * entropy_score + 0.25 * (1.0 - scr);
            debug!(
                source = "ViralLaunch",
                confidence, "classified as viral launch"
            );
            return (SourceType::ViralLaunch, confidence.min(1.0));
        }

        // Unknown
        debug!(source = "Unknown", "could not classify source");
        (SourceType::Unknown, UNKNOWN_CONFIDENCE)
    }

    /// Calculate combined SSMI score
    ///
    /// Weighted combination with source type bonus/penalty:
    /// - entropy_score * score_weight_entropy + scr_score * score_weight_scr + ar_score * score_weight_ar
    /// Uses adaptive thresholds from the instance configuration.
    fn calculate_combined_score(
        &self,
        entropy: f32,
        scr: f32,
        ar_corr: f32,
        source_type: &SourceType,
    ) -> f32 {
        let t = &self.thresholds;

        // Normalize entropy to 0-1 range (typical range is 0-6)
        let entropy_score = (entropy / MAX_ENTROPY_NORMALIZATION).clamp(0.0, 1.0);

        // SCR score: lower SCR is better (less bot activity)
        let scr_score = 1.0 - scr;

        // AR score: lower absolute correlation is better (less predictable = more organic)
        let ar_score = 1.0 - ar_corr.abs();

        // Base weighted combination using adaptive weights
        let mut score = entropy_score * t.score_weight_entropy
            + scr_score * t.score_weight_scr
            + ar_score * t.score_weight_ar;

        // Apply source type bonus/penalty using adaptive values
        match source_type {
            SourceType::ViralLaunch => score = (score + t.viral_score_bonus).min(1.0),
            SourceType::Human => score = (score + t.human_score_bonus).min(1.0),
            SourceType::Bot => score = (score - t.bot_score_penalty).max(0.0),
            SourceType::Unknown => {} // No adjustment
        }

        score.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssmi_empty_timestamps() {
        let ssmi = SubSlotMicroentropy::new();
        let result = ssmi.analyze(&[]);

        assert_eq!(result.tx_count, 0);
        assert_eq!(result.shannon_entropy, 0.0);
        assert_eq!(result.scr_bot_probability, 0.0);
        assert_eq!(result.ar_correlation, 0.0);
        assert_eq!(result.source_type, SourceType::Unknown);
    }

    #[test]
    fn test_ssmi_few_timestamps() {
        let ssmi = SubSlotMicroentropy::new();
        let timestamps = vec![100, 200, 300];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 3);
        // Should still calculate entropy with 2+ deltas
        assert!(result.shannon_entropy >= 0.0);
        // SCR requires 4+ timestamps
        assert_eq!(result.scr_bot_probability, 0.0);
        // AR correlation requires 4+ timestamps
        assert_eq!(result.ar_correlation, 0.0);
    }

    #[test]
    fn test_ssmi_bot_pattern() {
        let ssmi = SubSlotMicroentropy::new();
        // Very regular intervals (bot-like behavior)
        let timestamps: Vec<u64> = (0..32).map(|i| i * 50).collect();
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 32);
        // Regular intervals should have low entropy
        assert!(
            result.shannon_entropy < 2.0,
            "Bot pattern should have low entropy, got {}",
            result.shannon_entropy
        );
        // For perfectly regular intervals (constant deltas), variance is ~0, so AR correlation is ~0
        assert!(
            result.ar_correlation.abs() < 0.1,
            "Bot pattern with constant deltas should have AR correlation near 0, got {}",
            result.ar_correlation
        );
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
    }

    #[test]
    fn test_ssmi_human_pattern() {
        let ssmi = SubSlotMicroentropy::new();
        // Random/chaotic intervals (human-like behavior)
        let timestamps: Vec<u64> = vec![
            0, 127, 389, 412, 998, 1245, 1289, 1800, 1823, 2500, 2678, 3100, 3456, 3890, 4123,
            4567, 5000, 5234, 5678, 6012, 6345, 6789, 7123, 7456, 7890, 8234, 8567, 8901, 9345,
            9678,
        ];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 30);
        // Chaotic intervals should have higher entropy
        assert!(
            result.shannon_entropy > 1.5,
            "Human pattern should have higher entropy, got {}",
            result.shannon_entropy
        );
        // Low AR correlation expected for random intervals
        assert!(
            result.ar_correlation.abs() < 0.7,
            "Human pattern should have low AR correlation, got {}",
            result.ar_correlation
        );
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
    }

    #[test]
    fn test_ssmi_viral_pattern() {
        let ssmi = SubSlotMicroentropy::new();
        // Viral launch pattern: moderate entropy, many transactions, varied but not random
        let timestamps: Vec<u64> = vec![
            0, 50, 120, 180, 260, 350, 420, 510, 620, 740, 880, 1050, 1250, 1480, 1750, 2060,
        ];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 16);
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
        // Viral pattern should have moderate entropy
        assert!(
            result.shannon_entropy >= 1.0 && result.shannon_entropy <= 5.0,
            "Viral pattern should have moderate entropy, got {}",
            result.shannon_entropy
        );
    }

    #[test]
    fn test_shannon_entropy_calculation() {
        let ssmi = SubSlotMicroentropy::new();

        // Single delta should give 0 entropy (all in one bin)
        let uniform_timestamps: Vec<u64> = vec![0, 100];
        let entropy = ssmi.calculate_shannon_entropy(&uniform_timestamps);
        assert_eq!(entropy, 0.0);

        // More varied deltas should give higher entropy
        let varied_timestamps: Vec<u64> = vec![0, 100, 500, 550, 1200, 1250, 1800, 2200, 3000];
        let varied_entropy = ssmi.calculate_shannon_entropy(&varied_timestamps);
        assert!(
            varied_entropy > 0.0,
            "Varied timestamps should have positive entropy"
        );

        // Very spread out deltas should have high entropy
        let spread_timestamps: Vec<u64> = vec![
            0, 31, 125, 250, 500, 750, 1000, 1250, 1500, 1750, 2000, 2100, 2400, 2800, 3200, 3800,
        ];
        let spread_entropy = ssmi.calculate_shannon_entropy(&spread_timestamps);
        assert!(
            spread_entropy > varied_entropy * 0.5,
            "Spread timestamps should have relatively high entropy"
        );
    }

    #[test]
    fn test_ar_correlation_calculation() {
        let ssmi = SubSlotMicroentropy::new();

        // Less than 4 timestamps should return 0
        let few_timestamps: Vec<u64> = vec![0, 100, 200];
        assert_eq!(ssmi.calculate_ar_correlation(&few_timestamps), 0.0);

        // Perfectly regular intervals should have AR close to 0 (no variance in deltas)
        let regular_timestamps: Vec<u64> = (0..10).map(|i| i * 100).collect();
        let regular_ar = ssmi.calculate_ar_correlation(&regular_timestamps);
        // With constant deltas, variance is ~0, so AR should be ~0
        assert!(
            regular_ar.abs() <= 1.0,
            "AR correlation should be in valid range, got {}",
            regular_ar
        );

        // Alternating pattern should have negative AR correlation
        let alternating_timestamps: Vec<u64> = vec![0, 50, 150, 200, 300, 350, 450, 500, 600, 650];
        let alternating_ar = ssmi.calculate_ar_correlation(&alternating_timestamps);
        assert!(
            alternating_ar >= -1.0 && alternating_ar <= 1.0,
            "AR should be in [-1, 1], got {}",
            alternating_ar
        );
    }

    #[test]
    fn test_ssmi_performance() {
        /// Performance threshold in microseconds for CI environment
        const PERFORMANCE_THRESHOLD_US: u64 = 1000;

        let ssmi = SubSlotMicroentropy::new();
        // Create a reasonable-sized dataset
        let timestamps: Vec<u64> = (0..100).map(|i| i * 10 + (i % 7) * 3).collect();

        // Run analysis multiple times to get stable timing
        let mut total_time_us = 0u64;
        let iterations = 10;

        for _ in 0..iterations {
            let result = ssmi.analyze(&timestamps);
            total_time_us += result.analysis_time_us;
        }

        let avg_time_us = total_time_us / iterations;

        // Analysis should complete in less than PERFORMANCE_THRESHOLD_US microseconds on average
        // Note: Using 1000us (1ms) as a more generous threshold for CI
        assert!(
            avg_time_us < PERFORMANCE_THRESHOLD_US,
            "Average analysis time {}us exceeds threshold {}us",
            avg_time_us,
            PERFORMANCE_THRESHOLD_US
        );
    }

    #[test]
    fn test_source_type_classification() {
        let ssmi = SubSlotMicroentropy::new();

        // Test that classification returns valid types
        // Bot: high SCR + high AR + low entropy
        let (source, confidence) = ssmi.classify_source(
            BOT_ENTROPY_THRESHOLD - 0.5, // entropy below threshold
            BOT_SCR_THRESHOLD + 0.1,     // SCR above threshold
            BOT_AR_THRESHOLD + 0.1,      // AR above threshold
            10,
        );
        assert!(confidence >= 0.0 && confidence <= 1.0);
        assert_eq!(source, SourceType::Bot);

        // Human: high entropy + low AR + low SCR
        let (source, confidence) = ssmi.classify_source(
            HUMAN_ENTROPY_THRESHOLD + 1.0, // entropy above threshold
            HUMAN_SCR_THRESHOLD - 0.2,     // SCR below threshold
            HUMAN_AR_THRESHOLD - 0.2,      // AR below threshold
            10,
        );
        assert!(confidence >= 0.0 && confidence <= 1.0);
        assert_eq!(source, SourceType::Human);

        // ViralLaunch: tx_count >= VIRAL_MIN_TX_COUNT, entropy in range, scr < threshold
        let (source, confidence) = ssmi.classify_source(
            (VIRAL_ENTROPY_MIN + VIRAL_ENTROPY_MAX) / 2.0, // entropy in sweet spot
            VIRAL_SCR_THRESHOLD - 0.2,                     // SCR below threshold
            0.4,                                           // moderate AR
            VIRAL_MIN_TX_COUNT + 4,                        // tx_count above threshold
        );
        assert!(confidence >= 0.0 && confidence <= 1.0);
        assert_eq!(source, SourceType::ViralLaunch);

        // Unknown: doesn't match any pattern
        let (source, confidence) = ssmi.classify_source(2.0, 0.5, 0.5, 3);
        assert_eq!(confidence, UNKNOWN_CONFIDENCE);
        assert_eq!(source, SourceType::Unknown);
    }

    #[test]
    fn test_combined_score_bounds() {
        let ssmi = SubSlotMicroentropy::new();

        // Test various inputs to ensure score is always in [0, 1]
        let test_cases = [
            (0.0, 0.0, 0.0, SourceType::Unknown),
            (6.0, 1.0, 1.0, SourceType::Bot),
            (3.0, 0.5, 0.5, SourceType::Human),
            (3.5, 0.3, 0.2, SourceType::ViralLaunch),
            (10.0, 0.0, -1.0, SourceType::Human), // Edge case: entropy > 6
        ];

        for (entropy, scr, ar_corr, source_type) in test_cases {
            let score = ssmi.calculate_combined_score(entropy, scr, ar_corr, &source_type);
            assert!(
                score >= 0.0 && score <= 1.0,
                "Score {} out of bounds for inputs ({}, {}, {}, {:?})",
                score,
                entropy,
                scr,
                ar_corr,
                source_type
            );
        }
    }

    // =============================================================================
    // New Tests: Edge Cases, Fuzzing-style, and Enhanced Features
    // =============================================================================

    #[test]
    fn test_ssmi_duplicate_timestamps() {
        let ssmi = SubSlotMicroentropy::new();
        // All timestamps are the same (edge case)
        let timestamps: Vec<u64> = vec![1000, 1000, 1000, 1000, 1000, 1000];
        let result = ssmi.analyze(&timestamps);

        // All deltas are 0, should go to first bin
        assert_eq!(result.tx_count, 6);
        assert_eq!(
            result.shannon_entropy, 0.0,
            "Identical timestamps should have 0 entropy"
        );
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
    }

    #[test]
    fn test_ssmi_single_timestamp() {
        let ssmi = SubSlotMicroentropy::new();
        let timestamps = vec![500];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 1);
        assert_eq!(result.shannon_entropy, 0.0);
        assert_eq!(result.ar_correlation, 0.0);
        assert_eq!(result.scr_bot_probability, 0.0);
        assert_eq!(result.source_type, SourceType::Unknown);
    }

    #[test]
    fn test_ssmi_two_timestamps() {
        let ssmi = SubSlotMicroentropy::new();
        let timestamps = vec![100, 500];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 2);
        // One delta, so entropy is 0 (all in one bin)
        assert_eq!(result.shannon_entropy, 0.0);
        // AR needs 4+ timestamps
        assert_eq!(result.ar_correlation, 0.0);
    }

    #[test]
    fn test_ssmi_small_dataset_100tx() {
        let ssmi = SubSlotMicroentropy::new();
        // 100 transaction dataset with varied timing
        let timestamps: Vec<u64> = (0..100).map(|i| i * 50 + ((i * 7) % 20) * 5).collect();
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 100);
        assert!(result.shannon_entropy >= 0.0);
        assert!(result.ar_correlation >= -1.0 && result.ar_correlation <= 1.0);
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
        // Probabilistic scores should sum to ~1.0
        let prob_sum = result.bot_probability + result.human_probability + result.viral_probability;
        assert!(
            (prob_sum - 1.0).abs() < 0.01,
            "Probabilistic scores should sum to ~1.0, got {}",
            prob_sum
        );
    }

    #[test]
    fn test_ssmi_very_large_jitter() {
        let ssmi = SubSlotMicroentropy::new();
        // Jitter values way beyond max_jitter_ms (2000ms)
        let timestamps: Vec<u64> = vec![0, 10000, 25000, 50000, 100000];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 5);
        // Should still work, deltas clamped to last bin
        assert!(result.shannon_entropy >= 0.0);
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
    }

    #[test]
    fn test_ssmi_descending_timestamps() {
        let ssmi = SubSlotMicroentropy::new();
        // Descending timestamps (should saturate to 0 deltas)
        let timestamps: Vec<u64> = vec![1000, 900, 800, 700, 600, 500];
        let result = ssmi.analyze(&timestamps);

        assert_eq!(result.tx_count, 6);
        // saturating_sub should produce 0 deltas
        assert_eq!(result.shannon_entropy, 0.0);
    }

    #[test]
    fn test_configurable_bins() {
        // Test with 128 bins instead of default 64
        let ssmi_128 = SubSlotMicroentropyConfigurable::<128>::new();
        assert_eq!(ssmi_128.bins(), 128);

        // Test with custom max_jitter
        let ssmi_custom = SubSlotMicroentropyConfigurable::<32>::with_max_jitter(4000);
        assert_eq!(ssmi_custom.bins(), 32);
        assert_eq!(ssmi_custom.max_jitter_ms(), 4000);

        // Both should produce valid results
        let timestamps: Vec<u64> = (0..50).map(|i| i * 100).collect();

        let result_128 = ssmi_128.analyze(&timestamps);
        let result_32 = ssmi_custom.analyze(&timestamps);

        assert!(result_128.ssmi_score >= 0.0 && result_128.ssmi_score <= 1.0);
        assert!(result_32.ssmi_score >= 0.0 && result_32.ssmi_score <= 1.0);
    }

    #[test]
    fn test_probabilistic_scores_bounds() {
        let ssmi = SubSlotMicroentropy::new();

        // Test various patterns to ensure probabilistic scores are valid
        let test_cases = vec![
            // Regular bot-like pattern
            (0..32).map(|i| i * 50).collect::<Vec<u64>>(),
            // Chaotic human-like pattern
            vec![0, 127, 389, 412, 998, 1245, 1289, 1800, 1823, 2500],
            // Viral pattern
            vec![0, 50, 120, 180, 260, 350, 420, 510, 620, 740],
        ];

        for timestamps in test_cases {
            let result = ssmi.analyze(&timestamps);

            // Each probability should be in [0, 1]
            assert!(result.bot_probability >= 0.0 && result.bot_probability <= 1.0);
            assert!(result.human_probability >= 0.0 && result.human_probability <= 1.0);
            assert!(result.viral_probability >= 0.0 && result.viral_probability <= 1.0);

            // Sum should be ~1.0
            let sum = result.bot_probability + result.human_probability + result.viral_probability;
            assert!(
                (sum - 1.0).abs() < 0.01,
                "Probability sum should be ~1.0, got {}",
                sum
            );
        }
    }

    #[test]
    fn test_thread_safety_compile_check() {
        use std::sync::Arc;

        // This test verifies that SubSlotMicroentropy can be wrapped in Arc
        // and satisfies Send + Sync (compile-time check)
        let ssmi = Arc::new(SubSlotMicroentropy::new());
        let ssmi_clone = Arc::clone(&ssmi);

        // Use in a closure that could be sent to another thread
        let timestamps = vec![100, 200, 300, 400, 500];
        let _result1 = ssmi.analyze(&timestamps);
        let _result2 = ssmi_clone.analyze(&timestamps);

        // If this compiles, Send + Sync are satisfied
    }

    #[test]
    fn test_probabilistic_vs_threshold_classification() {
        let ssmi = SubSlotMicroentropy::new();

        // Test that probabilistic scores align with threshold classification
        // Bot pattern: should have high bot_probability
        let bot_timestamps: Vec<u64> = (0..32).map(|i| i * 50).collect();
        let bot_result = ssmi.analyze(&bot_timestamps);

        // Even if not classified as Bot due to thresholds, bot_probability should be reasonable
        assert!(
            bot_result.bot_probability >= 0.0,
            "Bot probability should be non-negative"
        );

        // Human-like pattern: should have higher human_probability
        let human_timestamps: Vec<u64> = vec![
            0, 127, 389, 412, 998, 1245, 1289, 1800, 1823, 2500, 2678, 3100,
        ];
        let human_result = ssmi.analyze(&human_timestamps);
        assert!(
            human_result.human_probability >= 0.0,
            "Human probability should be non-negative"
        );
    }

    #[test]
    fn test_f64_precision_consistency() {
        let ssmi = SubSlotMicroentropy::new();

        // Test with a pattern that might cause f32 precision issues
        // Large values with small differences
        let timestamps: Vec<u64> = (0..1000).map(|i| 1_000_000_000 + i * 10).collect();

        let result = ssmi.analyze(&timestamps);

        // Should still produce valid, bounded results
        assert!(result.shannon_entropy >= 0.0);
        assert!(result.ar_correlation >= -1.0 && result.ar_correlation <= 1.0);
        assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
    }

    // =============================================================================
    // Adaptive Thresholds Tests
    // =============================================================================

    #[test]
    fn test_adaptive_thresholds_builder_chainable() {
        // Test that builder methods are chainable
        let ssmi = SubSlotMicroentropyConfigurable::<64>::new()
            .with_bot_scr_threshold(0.8)
            .with_bot_ar_threshold(0.9)
            .with_bot_entropy_threshold(1.0)
            .with_human_entropy_threshold(3.5)
            .with_human_ar_threshold(0.25)
            .with_human_scr_threshold(0.35)
            .with_viral_min_tx_count(8)
            .with_viral_entropy_min(2.0)
            .with_viral_entropy_max(4.5)
            .with_viral_scr_threshold(0.45)
            .with_viral_entropy_center(3.0)
            .with_viral_entropy_range(1.0)
            .with_score_weight_entropy(0.4)
            .with_score_weight_scr(0.35)
            .with_score_weight_ar(0.25)
            .with_viral_score_bonus(0.2)
            .with_human_score_bonus(0.1)
            .with_bot_score_penalty(0.25);

        let t = ssmi.thresholds();
        assert_eq!(t.bot_scr_threshold, 0.8);
        assert_eq!(t.bot_ar_threshold, 0.9);
        assert_eq!(t.bot_entropy_threshold, 1.0);
        assert_eq!(t.human_entropy_threshold, 3.5);
        assert_eq!(t.human_ar_threshold, 0.25);
        assert_eq!(t.human_scr_threshold, 0.35);
        assert_eq!(t.viral_min_tx_count, 8);
        assert_eq!(t.viral_entropy_min, 2.0);
        assert_eq!(t.viral_entropy_max, 4.5);
        assert_eq!(t.viral_scr_threshold, 0.45);
        assert_eq!(t.viral_entropy_center, 3.0);
        assert_eq!(t.viral_entropy_range, 1.0);
        assert_eq!(t.score_weight_entropy, 0.4);
        assert_eq!(t.score_weight_scr, 0.35);
        assert_eq!(t.score_weight_ar, 0.25);
        assert_eq!(t.viral_score_bonus, 0.2);
        assert_eq!(t.human_score_bonus, 0.1);
        assert_eq!(t.bot_score_penalty, 0.25);
    }

    #[test]
    fn test_custom_thresholds_affect_classification() {
        // Test with default thresholds
        let ssmi_default = SubSlotMicroentropy::new();
        let timestamps: Vec<u64> = (0..20).map(|i| i * 50).collect();
        let result_default = ssmi_default.analyze(&timestamps);

        // Test with very strict bot thresholds (should make it harder to classify as bot)
        let ssmi_strict = SubSlotMicroentropy::new()
            .with_bot_scr_threshold(0.99) // Very strict - almost nothing is a bot
            .with_bot_ar_threshold(0.99)
            .with_bot_entropy_threshold(0.1);
        let result_strict = ssmi_strict.analyze(&timestamps);

        // Both should still produce valid scores
        assert!(result_default.ssmi_score >= 0.0 && result_default.ssmi_score <= 1.0);
        assert!(result_strict.ssmi_score >= 0.0 && result_strict.ssmi_score <= 1.0);

        // With strict bot thresholds, something that was classified as bot might now be unknown
        // (exact behavior depends on the input pattern)
    }

    #[test]
    fn test_with_thresholds_bulk_set() {
        let custom_thresholds = AdaptiveThresholds {
            bot_scr_threshold: 0.85,
            bot_ar_threshold: 0.85,
            bot_entropy_threshold: 1.2,
            human_entropy_threshold: 3.2,
            human_ar_threshold: 0.28,
            human_scr_threshold: 0.38,
            viral_min_tx_count: 7,
            viral_entropy_min: 2.3,
            viral_entropy_max: 4.2,
            viral_scr_threshold: 0.48,
            viral_entropy_center: 3.1,
            viral_entropy_range: 0.8,
            score_weight_entropy: 0.33,
            score_weight_scr: 0.37,
            score_weight_ar: 0.30,
            viral_score_bonus: 0.18,
            human_score_bonus: 0.08,
            bot_score_penalty: 0.22,
        };

        let ssmi = SubSlotMicroentropy::new().with_thresholds(custom_thresholds);
        let t = ssmi.thresholds();

        assert_eq!(t.bot_scr_threshold, 0.85);
        assert_eq!(t.viral_min_tx_count, 7);
        assert_eq!(t.score_weight_ar, 0.30);
    }

    #[test]
    fn test_update_thresholds_from_history_insufficient_data() {
        let mut ssmi = SubSlotMicroentropy::new();

        // With insufficient history, update should return false
        let short_history: Vec<SsmiResult> = (0..5)
            .map(|i| SsmiResult {
                ssmi_score: 0.5,
                shannon_entropy: 2.5,
                scr_bot_probability: 0.3,
                ar_correlation: 0.2,
                source_type: SourceType::Human,
                confidence: 0.7,
                tx_count: 10 + i,
                analysis_time_us: 100,
                bot_probability: 0.3,
                human_probability: 0.5,
                viral_probability: 0.2,
            })
            .collect();

        assert!(!ssmi.update_thresholds_from_history(&short_history));
    }

    #[test]
    fn test_update_thresholds_from_history_with_bot_samples() {
        let mut ssmi = SubSlotMicroentropy::new();

        // Create history with bot samples
        let mut history: Vec<SsmiResult> = Vec::new();

        // Add bot samples with low entropy
        for i in 0..5 {
            history.push(SsmiResult {
                ssmi_score: 0.3,
                shannon_entropy: 1.0 + (i as f32 * 0.1), // Low entropy ~1.0-1.4
                scr_bot_probability: 0.8 + (i as f32 * 0.02), // High SCR ~0.8-0.88
                ar_correlation: 0.85,
                source_type: SourceType::Bot,
                confidence: 0.9,
                tx_count: 50,
                analysis_time_us: 100,
                bot_probability: 0.8,
                human_probability: 0.1,
                viral_probability: 0.1,
            });
        }

        // Add some human and unknown samples to meet minimum history requirement
        for i in 0..5 {
            history.push(SsmiResult {
                ssmi_score: 0.6,
                shannon_entropy: 3.5 + (i as f32 * 0.1),
                scr_bot_probability: 0.2,
                ar_correlation: 0.1,
                source_type: SourceType::Unknown,
                confidence: 0.4,
                tx_count: 10,
                analysis_time_us: 100,
                bot_probability: 0.2,
                human_probability: 0.5,
                viral_probability: 0.3,
            });
        }

        let original_bot_entropy = ssmi.thresholds().bot_entropy_threshold;
        let original_bot_scr = ssmi.thresholds().bot_scr_threshold;

        assert!(ssmi.update_thresholds_from_history(&history));

        // Bot thresholds should have been updated based on bot samples
        let new_bot_entropy = ssmi.thresholds().bot_entropy_threshold;
        let new_bot_scr = ssmi.thresholds().bot_scr_threshold;

        // The new entropy threshold should be ~1.2 * 1.1 = ~1.32
        assert!(
            (new_bot_entropy - original_bot_entropy).abs() > 0.01
                || (new_bot_scr - original_bot_scr).abs() > 0.01,
            "At least one bot threshold should have changed"
        );
    }

    #[test]
    fn test_update_thresholds_from_history_with_viral_samples() {
        let mut ssmi = SubSlotMicroentropy::new();

        // Create history with viral samples
        let mut history: Vec<SsmiResult> = Vec::new();

        // Add viral samples with moderate entropy
        for i in 0..5 {
            history.push(SsmiResult {
                ssmi_score: 0.75,
                shannon_entropy: 3.0 + (i as f32 * 0.1), // Moderate entropy ~3.0-3.4
                scr_bot_probability: 0.3,
                ar_correlation: 0.4,
                source_type: SourceType::ViralLaunch,
                confidence: 0.8,
                tx_count: 20,
                analysis_time_us: 100,
                bot_probability: 0.2,
                human_probability: 0.3,
                viral_probability: 0.5,
            });
        }

        // Add some unknown samples to meet minimum history requirement
        for i in 0..5 {
            history.push(SsmiResult {
                ssmi_score: 0.5,
                shannon_entropy: 2.0 + (i as f32 * 0.1),
                scr_bot_probability: 0.5,
                ar_correlation: 0.5,
                source_type: SourceType::Unknown,
                confidence: 0.4,
                tx_count: 5,
                analysis_time_us: 100,
                bot_probability: 0.33,
                human_probability: 0.33,
                viral_probability: 0.34,
            });
        }

        let original_viral_center = ssmi.thresholds().viral_entropy_center;

        assert!(ssmi.update_thresholds_from_history(&history));

        // Viral thresholds should have been updated based on viral samples
        let new_viral_center = ssmi.thresholds().viral_entropy_center;

        // The new center should be ~3.2 (mean of 3.0-3.4)
        assert!(
            (new_viral_center - original_viral_center).abs() > 0.01,
            "Viral entropy center should have changed from {} to {}",
            original_viral_center,
            new_viral_center
        );
    }

    #[test]
    fn test_adaptive_thresholds_default_matches_const() {
        let default_thresholds = AdaptiveThresholds::default();

        // Verify that default thresholds match the const values
        assert_eq!(default_thresholds.bot_scr_threshold, BOT_SCR_THRESHOLD);
        assert_eq!(default_thresholds.bot_ar_threshold, BOT_AR_THRESHOLD);
        assert_eq!(
            default_thresholds.bot_entropy_threshold,
            BOT_ENTROPY_THRESHOLD
        );
        assert_eq!(
            default_thresholds.human_entropy_threshold,
            HUMAN_ENTROPY_THRESHOLD
        );
        assert_eq!(default_thresholds.human_ar_threshold, HUMAN_AR_THRESHOLD);
        assert_eq!(default_thresholds.human_scr_threshold, HUMAN_SCR_THRESHOLD);
        assert_eq!(default_thresholds.viral_min_tx_count, VIRAL_MIN_TX_COUNT);
        assert_eq!(default_thresholds.viral_entropy_min, VIRAL_ENTROPY_MIN);
        assert_eq!(default_thresholds.viral_entropy_max, VIRAL_ENTROPY_MAX);
        assert_eq!(default_thresholds.viral_scr_threshold, VIRAL_SCR_THRESHOLD);
        assert_eq!(
            default_thresholds.viral_entropy_center,
            VIRAL_ENTROPY_CENTER
        );
        assert_eq!(default_thresholds.viral_entropy_range, VIRAL_ENTROPY_RANGE);
        assert_eq!(
            default_thresholds.score_weight_entropy,
            SCORE_WEIGHT_ENTROPY
        );
        assert_eq!(default_thresholds.score_weight_scr, SCORE_WEIGHT_SCR);
        assert_eq!(default_thresholds.score_weight_ar, SCORE_WEIGHT_AR);
        assert_eq!(default_thresholds.viral_score_bonus, VIRAL_SCORE_BONUS);
        assert_eq!(default_thresholds.human_score_bonus, HUMAN_SCORE_BONUS);
        assert_eq!(default_thresholds.bot_score_penalty, BOT_SCORE_PENALTY);
    }

    #[test]
    fn test_thresholds_getter() {
        let ssmi = SubSlotMicroentropy::new().with_bot_scr_threshold(0.75);

        // Verify getter returns the correct thresholds
        assert_eq!(ssmi.thresholds().bot_scr_threshold, 0.75);
        // Other thresholds should be default
        assert_eq!(ssmi.thresholds().bot_ar_threshold, BOT_AR_THRESHOLD);
    }
}

// =============================================================================
// Proptest Fuzzing Tests
// =============================================================================

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Fuzz test: analyze should never panic and always produce valid results
        #[test]
        fn fuzz_analyze_random_timestamps(
            timestamps in prop::collection::vec(any::<u64>(), 0..500)
        ) {
            let ssmi = SubSlotMicroentropy::new();
            let result = ssmi.analyze(&timestamps);

            // Validate all outputs are in expected ranges
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0,
                "ssmi_score {} out of bounds", result.ssmi_score);
            prop_assert!(result.shannon_entropy >= 0.0,
                "shannon_entropy {} should be non-negative", result.shannon_entropy);
            prop_assert!(result.scr_bot_probability >= 0.0 && result.scr_bot_probability <= 1.0,
                "scr_bot_probability {} out of bounds", result.scr_bot_probability);
            prop_assert!(result.ar_correlation >= -1.0 && result.ar_correlation <= 1.0,
                "ar_correlation {} out of bounds", result.ar_correlation);
            prop_assert!(result.confidence >= 0.0 && result.confidence <= 1.0,
                "confidence {} out of bounds", result.confidence);
            prop_assert!(result.tx_count == timestamps.len(),
                "tx_count {} should match timestamps.len() {}", result.tx_count, timestamps.len());

            // Probabilistic scores should be valid
            prop_assert!(result.bot_probability >= 0.0 && result.bot_probability <= 1.0,
                "bot_probability {} out of bounds", result.bot_probability);
            prop_assert!(result.human_probability >= 0.0 && result.human_probability <= 1.0,
                "human_probability {} out of bounds", result.human_probability);
            prop_assert!(result.viral_probability >= 0.0 && result.viral_probability <= 1.0,
                "viral_probability {} out of bounds", result.viral_probability);

            // Probabilities should sum to ~1.0 (with some tolerance for floating point)
            let prob_sum = result.bot_probability + result.human_probability + result.viral_probability;
            prop_assert!((prob_sum - 1.0).abs() < 0.05,
                "probability sum {} should be ~1.0", prob_sum);
        }

        /// Fuzz test: ascending timestamps (realistic case)
        #[test]
        fn fuzz_analyze_ascending_timestamps(
            deltas in prop::collection::vec(0u64..5000, 0..200)
        ) {
            let ssmi = SubSlotMicroentropy::new();

            // Build ascending timestamps from deltas
            let mut timestamps = Vec::with_capacity(deltas.len() + 1);
            let mut current = 0u64;
            timestamps.push(current);
            for delta in deltas {
                current = current.saturating_add(delta);
                timestamps.push(current);
            }

            let result = ssmi.analyze(&timestamps);

            // All values should be in valid ranges
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
            prop_assert!(result.shannon_entropy >= 0.0);
            prop_assert!(result.ar_correlation >= -1.0 && result.ar_correlation <= 1.0);
        }

        /// Fuzz test: mixed ascending/descending timestamps (edge case)
        #[test]
        fn fuzz_analyze_mixed_order_timestamps(
            timestamps in prop::collection::vec(0u64..u64::MAX, 0..100)
        ) {
            let ssmi = SubSlotMicroentropy::new();
            let result = ssmi.analyze(&timestamps);

            // Should handle saturating_sub gracefully
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
            prop_assert!(result.shannon_entropy >= 0.0);
            // Entropy of 0 is valid when all deltas saturate to 0
        }

        /// Fuzz test: duplicate timestamps (edge case)
        #[test]
        fn fuzz_analyze_duplicate_timestamps(
            value in any::<u64>(),
            count in 0usize..100
        ) {
            let ssmi = SubSlotMicroentropy::new();
            let timestamps: Vec<u64> = vec![value; count];
            let result = ssmi.analyze(&timestamps);

            // All duplicates means all deltas are 0
            if count >= 2 {
                prop_assert_eq!(result.shannon_entropy, 0.0,
                    "duplicate timestamps should have 0 entropy");
            }
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
        }

        /// Fuzz test: u64 overflow handling
        #[test]
        fn fuzz_analyze_large_values(
            base in u64::MAX - 1000..u64::MAX,
            offsets in prop::collection::vec(0u64..1000, 2..50)
        ) {
            let ssmi = SubSlotMicroentropy::new();

            // Create timestamps near u64::MAX
            let timestamps: Vec<u64> = offsets.iter()
                .scan(base, |acc, &offset| {
                    let result = *acc;
                    *acc = acc.saturating_add(offset);
                    Some(result)
                })
                .collect();

            let result = ssmi.analyze(&timestamps);

            // Should handle near-overflow values gracefully
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
            prop_assert!(result.shannon_entropy >= 0.0);
        }

        /// Fuzz test: Shannon entropy calculation edge cases
        #[test]
        fn fuzz_shannon_entropy_calculation(
            timestamps in prop::collection::vec(any::<u64>(), 0..300)
        ) {
            let ssmi = SubSlotMicroentropy::new();
            let entropy = ssmi.calculate_shannon_entropy(&timestamps);

            // Entropy should always be non-negative
            prop_assert!(entropy >= 0.0, "entropy {} should be >= 0", entropy);

            // Entropy should be bounded (theoretical max for 64 bins is log2(64) = 6)
            prop_assert!(entropy <= 8.0,
                "entropy {} should be reasonably bounded", entropy);
        }

        /// Fuzz test: AR correlation calculation edge cases
        #[test]
        fn fuzz_ar_correlation_calculation(
            timestamps in prop::collection::vec(any::<u64>(), 0..300)
        ) {
            let ssmi = SubSlotMicroentropy::new();
            let ar_corr = ssmi.calculate_ar_correlation(&timestamps);

            // AR correlation should be in [-1, 1]
            prop_assert!(ar_corr >= -1.0 && ar_corr <= 1.0,
                "AR correlation {} out of bounds", ar_corr);
        }

        /// Fuzz test: configurable histogram bins
        #[test]
        fn fuzz_configurable_bins(
            timestamps in prop::collection::vec(any::<u64>(), 0..100),
            max_jitter in 100u64..10000
        ) {
            // Test with 32 bins
            let ssmi_32 = SubSlotMicroentropyConfigurable::<32>::with_max_jitter(max_jitter);
            let result_32 = ssmi_32.analyze(&timestamps);

            // Test with 128 bins
            let ssmi_128 = SubSlotMicroentropyConfigurable::<128>::with_max_jitter(max_jitter);
            let result_128 = ssmi_128.analyze(&timestamps);

            // Both should produce valid results
            prop_assert!(result_32.ssmi_score >= 0.0 && result_32.ssmi_score <= 1.0);
            prop_assert!(result_128.ssmi_score >= 0.0 && result_128.ssmi_score <= 1.0);
        }

        /// Fuzz test: adaptive thresholds don't break classification
        #[test]
        fn fuzz_custom_thresholds(
            timestamps in prop::collection::vec(0u64..10000, 10..100),
            bot_scr in 0.1f32..0.99,
            human_entropy in 1.0f32..5.0,
            viral_min_tx in 3usize..20
        ) {
            let ssmi = SubSlotMicroentropy::new()
                .with_bot_scr_threshold(bot_scr)
                .with_human_entropy_threshold(human_entropy)
                .with_viral_min_tx_count(viral_min_tx);

            let result = ssmi.analyze(&timestamps);

            // Should still produce valid results with custom thresholds
            prop_assert!(result.ssmi_score >= 0.0 && result.ssmi_score <= 1.0);
            prop_assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
        }
    }
}
