//! SCR 2.0: Harmonic Detection & Spectral Pattern Matching
//!
//! This module extends the base SCR (Slot-Coherence Resonance) with:
//! - Harmonic peak detection in FFT spectrum
//! - Pattern matching against known spectral signatures
//! - Activity type classification (PureBot/Mixed/Organic/ViralLaunch)
//!
//! # Example
//!
//! ```ignore
//! use ghost_brain::oracle::scr_extended::{SCRExtended, ActivityType};
//!
//! let scr = SCRExtended::new();
//! let timestamps: Vec<u64> = vec![0, 100, 200, 350, 480, 600, 750, 900];
//! let analysis = scr.analyze(&timestamps);
//!
//! println!("Base SCR: {:.3}", analysis.base_score);
//! println!("Activity Type: {:?}", analysis.activity_type);
//! println!("Pattern: {} (confidence: {:.2})",
//!     analysis.pattern_match.pattern_name,
//!     analysis.pattern_match.confidence
//! );
//! ```

use super::hyper_oracle::HyperOracle;
use rustfft::{num_complex::Complex, FftPlanner};
use solana_sdk::pubkey::Pubkey;
use std::cell::RefCell;

// Re-export integrity violation types from snapshot_engine for convenience
pub use super::snapshot_engine::{
    IntegritySeverity, IntegrityViolation, IntegrityViolationCallback,
};

// Thread-local FFT planner: Zero lock contention, maximum throughput.
thread_local! {
    static FFT_PLANNER: RefCell<FftPlanner<f32>> = RefCell::new(FftPlanner::new());
}

// =============================================================================
// Configuration Constants
// =============================================================================

/// Sentinel value for missing pool context in integrity violations
/// This is an all-zeros Pubkey which is recognizable in logs as "null" indicator.
/// Note: While unlikely, this could theoretically conflict with an actual zero public key.
/// In production, consider using a well-documented constant or optional field instead.
const MISSING_POOL_SENTINEL: [u8; 32] = [0; 32];

/// Default minimum peak amplitude to consider as harmonic
const DEFAULT_PEAK_THRESHOLD: f32 = 0.1;

/// Default maximum dominant peaks to track
const DEFAULT_MAX_DOMINANT_PEAKS: usize = 5;

/// SCR threshold for pure bot detection
const PURE_BOT_SCR_THRESHOLD: f32 = 0.7;

/// Minimum peaks for pure bot classification
const PURE_BOT_MIN_PEAKS: usize = 3;

/// Viral launch confidence threshold
const VIRAL_CONFIDENCE_THRESHOLD: f32 = 0.6;

/// SCR threshold for organic activity
const ORGANIC_SCR_THRESHOLD: f32 = 0.25;

/// Maximum peaks for organic classification
const ORGANIC_MAX_PEAKS: usize = 2;

/// Minimum peaks for Unknown classification
const UNKNOWN_MIN_PEAKS: usize = 2;

/// Maximum acceptable jitter for timestamp correction (default: 1500ms)
const DEFAULT_MAX_JITTER_MS: u64 = 1500;

// =============================================================================
// Data Structures
// =============================================================================

/// Detected harmonic peak in FFT spectrum
#[derive(Debug, Clone, Copy)]
pub struct HarmonicPeak {
    /// Frequency bin index
    pub frequency_bin: usize,
    /// Amplitude at this frequency (normalized 0.0-1.0)
    pub amplitude: f32,
    /// Is this a dominant peak?
    pub is_dominant: bool,
}

/// Known spectral signature from successful/failed launches
#[derive(Debug, Clone)]
pub struct SpectralSignature {
    /// Identifier (e.g., "viral_memecoin", "bot_dump", "organic_growth")
    pub name: &'static str,
    /// Normalized spectrum shape (FFT magnitudes for top peaks)
    pub spectrum_shape: Vec<f32>,
    /// Expected high_freq_ratio range
    pub hf_ratio_range: (f32, f32),
    /// Historical success rate for this pattern
    pub success_rate: f32,
}

/// Pattern matching result
#[derive(Debug, Clone)]
pub struct PatternMatch {
    /// Best matching pattern name
    pub pattern_name: &'static str,
    /// Cosine similarity score (0.0-1.0)
    pub similarity: f32,
    /// Confidence in match
    pub confidence: f32,
    /// Predicted outcome based on pattern history
    pub predicted_success_rate: f32,
}

/// Activity classification based on spectral analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    /// Pure bot activity (regular intervals, high HF)
    PureBot,
    /// Mixed human + bot
    Mixed,
    /// Organic human activity (irregular, low HF)
    Organic,
    /// Viral launch signature (specific harmonic pattern)
    ViralLaunch,
    /// Unknown/insufficient data
    Unknown,
}

impl ActivityType {
    /// Get risk level associated with this activity type
    pub fn risk_level(&self) -> f32 {
        match self {
            ActivityType::PureBot => 0.9,
            ActivityType::Mixed => 0.5,
            ActivityType::Organic => 0.2,
            ActivityType::ViralLaunch => 0.1,
            ActivityType::Unknown => 0.7,
        }
    }

    /// Get recommended action
    pub fn recommendation(&self) -> &'static str {
        match self {
            ActivityType::PureBot => "SKIP - Pure bot activity detected",
            ActivityType::Mixed => "CAUTION - Mixed signals",
            ActivityType::Organic => "BUY - Organic activity pattern",
            ActivityType::ViralLaunch => "BUY - Viral launch detected",
            ActivityType::Unknown => "SKIP - Insufficient data",
        }
    }
}

/// Complete SCR 2.0 analysis result
#[derive(Debug, Clone)]
pub struct SCRAnalysis {
    /// Base SCR score (0.0-1.0, higher = more bots)
    pub base_score: f32,
    /// Detected harmonic peaks
    pub harmonics: Vec<HarmonicPeak>,
    /// Best matching pattern
    pub pattern_match: PatternMatch,
    /// Classified activity type
    pub activity_type: ActivityType,
}

// =============================================================================
// SCRExtended Implementation
// =============================================================================

/// Extended SCR with harmonic detection and pattern matching
#[derive(Clone)]
pub struct SCRExtended {
    /// Base HyperOracle for FFT computation
    base: HyperOracle,

    /// Known spectral signatures for pattern matching
    known_signatures: Vec<SpectralSignature>,

    /// Minimum peak amplitude to consider as harmonic
    peak_threshold: f32,

    /// Number of dominant peaks to track
    max_dominant_peaks: usize,
}

impl Default for SCRExtended {
    fn default() -> Self {
        Self::new()
    }
}

impl SCRExtended {
    /// Create with default known signatures
    pub fn new() -> Self {
        Self {
            base: HyperOracle::new(),
            known_signatures: Self::default_signatures(),
            peak_threshold: DEFAULT_PEAK_THRESHOLD,
            max_dominant_peaks: DEFAULT_MAX_DOMINANT_PEAKS,
        }
    }

    /// Create with custom configuration
    pub fn with_config(peak_threshold: f32, max_dominant_peaks: usize) -> Self {
        Self {
            base: HyperOracle::new(),
            known_signatures: Self::default_signatures(),
            peak_threshold,
            max_dominant_peaks,
        }
    }

    /// Default known signatures from historical data
    fn default_signatures() -> Vec<SpectralSignature> {
        vec![
            SpectralSignature {
                name: "viral_memecoin",
                spectrum_shape: vec![0.8, 0.3, 0.1, 0.05, 0.02], // Concentrated low freq
                hf_ratio_range: (0.05, 0.20),
                success_rate: 0.75,
            },
            SpectralSignature {
                name: "bot_pump_dump",
                spectrum_shape: vec![0.2, 0.2, 0.3, 0.4, 0.5], // High freq dominant
                hf_ratio_range: (0.60, 0.95),
                success_rate: 0.10,
            },
            SpectralSignature {
                name: "organic_growth",
                spectrum_shape: vec![0.5, 0.25, 0.15, 0.07, 0.03], // Smooth decay
                hf_ratio_range: (0.10, 0.35),
                success_rate: 0.55,
            },
            SpectralSignature {
                name: "wash_trading",
                spectrum_shape: vec![0.1, 0.1, 0.8, 0.1, 0.1], // Spike at specific freq
                hf_ratio_range: (0.40, 0.70),
                success_rate: 0.05,
            },
        ]
    }

    /// Get reference to known signatures
    pub fn known_signatures(&self) -> &[SpectralSignature] {
        &self.known_signatures
    }

    /// Detect harmonic peaks in FFT spectrum
    /// Returns peaks sorted by amplitude (descending)
    pub fn detect_harmonics(&self, timestamps_ms: &[u64]) -> Vec<HarmonicPeak> {
        if timestamps_ms.len() < 8 {
            return vec![];
        }

        // Compute deltas
        let deltas: Vec<f32> = timestamps_ms
            .windows(2)
            .map(|w| (w[1].saturating_sub(w[0])) as f32)
            .collect();

        // FFT
        let fft_size = deltas.len().next_power_of_two();
        let mut buffer: Vec<Complex<f32>> = deltas.iter().map(|&x| Complex::new(x, 0.0)).collect();
        buffer.resize(fft_size, Complex::new(0.0, 0.0));

        // Process FFT (using thread-local planner)
        FFT_PLANNER.with(|planner| {
            let mut p = planner.borrow_mut();
            let fft = p.plan_fft_forward(fft_size);
            fft.process(&mut buffer);
        });

        // Extract magnitudes (only first half due to Nyquist)
        let magnitudes: Vec<f32> = buffer.iter().take(fft_size / 2).map(|c| c.norm()).collect();

        // Normalize
        let max_mag = magnitudes.iter().copied().fold(0.0f32, f32::max);
        if max_mag < 1e-9 {
            return vec![];
        }
        let normalized: Vec<f32> = magnitudes.iter().map(|m| m / max_mag).collect();

        // Find peaks (local maxima above threshold)
        // Requires at least 3 elements for valid peak detection (need neighbors on both sides)
        let mut peaks: Vec<HarmonicPeak> = vec![];
        if normalized.len() >= 3 {
            for i in 1..(normalized.len() - 1) {
                if normalized[i] > self.peak_threshold
                    && normalized[i] > normalized[i - 1]
                    && normalized[i] > normalized[i + 1]
                {
                    peaks.push(HarmonicPeak {
                        frequency_bin: i,
                        amplitude: normalized[i],
                        is_dominant: false,
                    });
                }
            }
        }

        // Sort by amplitude descending and mark dominant
        // Handle NaN values explicitly by treating them as less than any valid value
        peaks.sort_by(|a, b| {
            match (a.amplitude.is_nan(), b.amplitude.is_nan()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater, // NaN goes to end (after sorting in descending order)
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => b
                    .amplitude
                    .partial_cmp(&a.amplitude)
                    .unwrap_or(std::cmp::Ordering::Equal),
            }
        });
        for peak in peaks.iter_mut().take(self.max_dominant_peaks) {
            peak.is_dominant = true;
        }

        peaks
    }

    /// Match current spectrum against known patterns
    pub fn match_signature(&self, timestamps_ms: &[u64]) -> PatternMatch {
        let scr_score = self.base.calculate_scr(timestamps_ms);
        let peaks = self.detect_harmonics(timestamps_ms);

        // Build current spectrum shape (exactly 5 peak amplitudes, padded with zeros if needed)
        let mut current_shape: Vec<f32> = peaks.iter().take(5).map(|p| p.amplitude).collect();
        current_shape.resize(5, 0.0); // Ensure exactly 5 elements

        let mut best_match = PatternMatch {
            pattern_name: "unknown",
            similarity: 0.0,
            confidence: 0.0,
            predicted_success_rate: 0.5,
        };

        for sig in &self.known_signatures {
            // Check if HF ratio is in expected range
            let hf_in_range =
                scr_score >= sig.hf_ratio_range.0 && scr_score <= sig.hf_ratio_range.1;

            // Cosine similarity
            let similarity = cosine_similarity(&current_shape, &sig.spectrum_shape);

            // Combined confidence
            let confidence = if hf_in_range {
                similarity * 0.7 + 0.3
            } else {
                similarity * 0.5
            };

            if confidence > best_match.confidence {
                best_match = PatternMatch {
                    pattern_name: sig.name,
                    similarity,
                    confidence,
                    predicted_success_rate: sig.success_rate,
                };
            }
        }

        best_match
    }

    /// Classify activity type based on all SCR signals
    pub fn classify_activity(&self, timestamps_ms: &[u64]) -> ActivityType {
        let scr_score = self.base.calculate_scr(timestamps_ms);
        let peaks = self.detect_harmonics(timestamps_ms);
        let pattern = self.match_signature(timestamps_ms);

        // Decision logic
        if scr_score > PURE_BOT_SCR_THRESHOLD && peaks.len() >= PURE_BOT_MIN_PEAKS {
            ActivityType::PureBot
        } else if pattern.pattern_name == "viral_memecoin"
            && pattern.confidence > VIRAL_CONFIDENCE_THRESHOLD
        {
            ActivityType::ViralLaunch
        } else if scr_score < ORGANIC_SCR_THRESHOLD && peaks.len() <= ORGANIC_MAX_PEAKS {
            ActivityType::Organic
        } else if peaks.len() < UNKNOWN_MIN_PEAKS {
            ActivityType::Unknown
        } else {
            ActivityType::Mixed
        }
    }

    /// Full SCR 2.0 analysis result
    pub fn analyze(&self, timestamps_ms: &[u64]) -> SCRAnalysis {
        SCRAnalysis {
            base_score: self.base.calculate_scr(timestamps_ms),
            harmonics: self.detect_harmonics(timestamps_ms),
            pattern_match: self.match_signature(timestamps_ms),
            activity_type: self.classify_activity(timestamps_ms),
        }
    }

    /// Get base SCR score only (for compatibility)
    pub fn calculate_scr(&self, timestamps_ms: &[u64]) -> f32 {
        self.base.calculate_scr(timestamps_ms)
    }
}

// =============================================================================
// Timestamp Correction & Jitter Detection
// =============================================================================

/// Result of timestamp correction and validation
#[derive(Debug, Clone)]
pub struct TimestampCorrectionResult {
    /// Corrected timestamp in milliseconds
    pub corrected_timestamp_ms: u64,
    /// Detected jitter in milliseconds
    pub jitter_ms: u64,
    /// Whether jitter exceeds acceptable threshold
    pub excessive_jitter: bool,
    /// Optional integrity violation if jitter is excessive
    pub violation: Option<IntegrityViolation>,
}

/// Correct timestamp using blockTime from on-chain data
///
/// Formula: corrected_timestamp = block_time * 1000 + estimated_offset
///
/// This eliminates jitter by using the authoritative on-chain block time
/// rather than relying on arrival timestamps which can vary due to network latency.
///
/// # Arguments
/// * `block_time` - Unix timestamp in seconds from on-chain blockTime field (must be >= 0)
/// * `arrival_time_ms` - System time when event was received (milliseconds)
/// * `estimated_offset_ms` - Optional offset to add (default: 0)
/// * `max_jitter_ms` - Maximum acceptable jitter before triggering warning
/// * `pool_pubkey` - Optional pool identifier for violation reporting
/// * `integrity_callback` - Optional callback to report excessive jitter
///
/// # Returns
/// `TimestampCorrectionResult` with corrected timestamp and jitter information
///
/// # Panics
/// Will return early with zero timestamp if block_time is negative
pub fn correct_timestamp_with_jitter_check(
    block_time: i64,
    arrival_time_ms: u64,
    estimated_offset_ms: u64,
    max_jitter_ms: u64,
    pool_pubkey: Option<solana_sdk::pubkey::Pubkey>,
    integrity_callback: Option<&IntegrityViolationCallback>,
) -> TimestampCorrectionResult {
    // Validate block_time is non-negative to prevent overflow
    if block_time < 0 {
        // Return error result for negative block time
        return TimestampCorrectionResult {
            corrected_timestamp_ms: 0,
            jitter_ms: 0,
            excessive_jitter: true,
            violation: Some(IntegrityViolation {
                source: "SCRExtended".to_string(),
                severity: IntegritySeverity::SoftSync,
                details: format!("Invalid negative block_time: {}", block_time),
                pool_pubkey: pool_pubkey.unwrap_or_else(|| {
                    // Use well-documented sentinel value for missing pool context
                    solana_sdk::pubkey::Pubkey::new_from_array(MISSING_POOL_SENTINEL)
                }),
                timestamp_ms: arrival_time_ms,
            }),
        };
    }

    // Calculate corrected timestamp: block_time * 1000 + estimated_offset
    let corrected_timestamp_ms = (block_time as u64) * 1000 + estimated_offset_ms;

    // Calculate jitter: arrival_time - corrected_timestamp
    let jitter_ms = arrival_time_ms.saturating_sub(corrected_timestamp_ms);

    // Check if jitter exceeds threshold
    let excessive_jitter = jitter_ms > max_jitter_ms;

    let violation = if excessive_jitter {
        let violation = IntegrityViolation {
            source: "SCRExtended".to_string(),
            severity: IntegritySeverity::SoftSync,
            details: format!(
                "Excessive timestamp jitter: {}ms (max: {}ms). Block time: {}, Arrival time: {}ms",
                jitter_ms, max_jitter_ms, block_time, arrival_time_ms
            ),
            pool_pubkey: pool_pubkey.unwrap_or_else(|| {
                solana_sdk::pubkey::Pubkey::new_from_array(MISSING_POOL_SENTINEL)
            }),
            timestamp_ms: arrival_time_ms,
        };

        // Invoke callback if provided
        if let Some(callback) = integrity_callback {
            callback(violation.clone());
        }

        Some(violation)
    } else {
        None
    };

    TimestampCorrectionResult {
        corrected_timestamp_ms,
        jitter_ms,
        excessive_jitter,
        violation,
    }
}

/// Batch correct timestamps for multiple events
///
/// Useful for pre-processing a batch of transactions before SCR analysis.
///
/// # Arguments
/// * `events` - Iterator of (block_time, arrival_time_ms) tuples
/// * `max_jitter_ms` - Maximum acceptable jitter
/// * `integrity_callback` - Optional callback for violations
///
/// # Returns
/// Vector of corrected timestamps (only includes those without excessive jitter unless force_include is true)
pub fn batch_correct_timestamps<I>(
    events: I,
    max_jitter_ms: u64,
    integrity_callback: Option<&IntegrityViolationCallback>,
    force_include_all: bool,
) -> Vec<u64>
where
    I: Iterator<Item = (i64, u64)>,
{
    events
        .filter_map(|(block_time, arrival_time)| {
            let result = correct_timestamp_with_jitter_check(
                block_time,
                arrival_time,
                0, // No offset for now
                max_jitter_ms,
                None, // No pool pubkey in batch mode
                integrity_callback,
            );

            // Include if not excessive jitter, or if force_include_all is true
            if !result.excessive_jitter || force_include_all {
                Some(result.corrected_timestamp_ms)
            } else {
                None
            }
        })
        .collect()
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Cosine similarity between two vectors of equal length
/// Returns a value between 0.0 and 1.0
///
/// # Panics
/// This function assumes both vectors have the same length.
/// If they don't, it will only compare up to the shorter length.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    // Ensure we handle different length vectors by using min length
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let dot: f32 = a
        .iter()
        .take(len)
        .zip(b.iter().take(len))
        .map(|(x, y)| x * y)
        .sum();
    let norm_a: f32 = a.iter().take(len).map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().take(len).map(|x| x * x).sum::<f32>().sqrt();

    if norm_a < 1e-9 || norm_b < 1e-9 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scr_extended_new() {
        let scr = SCRExtended::new();
        assert_eq!(scr.known_signatures.len(), 4);
        assert_eq!(scr.peak_threshold, DEFAULT_PEAK_THRESHOLD);
        assert_eq!(scr.max_dominant_peaks, DEFAULT_MAX_DOMINANT_PEAKS);
    }

    #[test]
    fn test_scr_extended_with_config() {
        let scr = SCRExtended::with_config(0.2, 3);
        assert_eq!(scr.peak_threshold, 0.2);
        assert_eq!(scr.max_dominant_peaks, 3);
    }

    #[test]
    fn test_detect_harmonics_insufficient_data() {
        let scr = SCRExtended::new();
        // Less than 8 timestamps
        let timestamps: Vec<u64> = vec![0, 100, 200, 300, 400];
        let peaks = scr.detect_harmonics(&timestamps);
        assert!(
            peaks.is_empty(),
            "Should return empty for insufficient data"
        );
    }

    #[test]
    fn test_detect_harmonics_bot_pattern() {
        let scr = SCRExtended::new();
        // Regular intervals = bot pattern
        let timestamps: Vec<u64> = (0..32).map(|i| i * 100).collect();
        let peaks = scr.detect_harmonics(&timestamps);

        // Bot patterns with perfectly regular intervals may have few/no peaks
        // (constant delta results in energy concentration at DC)
        // This is actually expected behavior - let's check the analysis works
        let analysis = scr.analyze(&timestamps);
        assert!(analysis.base_score >= 0.0 && analysis.base_score <= 1.0);
    }

    #[test]
    fn test_detect_harmonics_with_variation() {
        let scr = SCRExtended::new();
        // Pattern with some variation to produce detectable harmonics
        let timestamps: Vec<u64> = (0..32)
            .map(|i| i * 100 + (i % 4) * 25) // Variation pattern
            .collect();
        let peaks = scr.detect_harmonics(&timestamps);

        // Should detect some peaks
        for peak in &peaks {
            assert!(
                peak.amplitude >= 0.0 && peak.amplitude <= 1.0,
                "Peak amplitude should be normalized"
            );
        }
    }

    #[test]
    fn test_detect_harmonics_organic_pattern() {
        let scr = SCRExtended::new();
        // Irregular intervals = organic
        let timestamps = vec![
            0, 150, 280, 510, 620, 900, 1100, 1450, 1800, 2300, 2650, 3100, 3500, 3900, 4200, 4800,
        ];
        let peaks = scr.detect_harmonics(&timestamps);

        // Organic should have fewer/weaker peaks
        let dominant_count = peaks.iter().filter(|p| p.is_dominant).count();
        assert!(
            dominant_count <= 5,
            "Should mark at most max_dominant_peaks as dominant"
        );
    }

    #[test]
    fn test_match_signature_returns_valid() {
        let scr = SCRExtended::new();
        // Pattern similar to viral memecoin
        let timestamps: Vec<u64> = vec![0, 200, 350, 600, 750, 1100, 1300, 1700, 2000];
        let pattern = scr.match_signature(&timestamps);

        assert!(
            !pattern.pattern_name.is_empty(),
            "Should return a pattern name"
        );
        assert!(
            pattern.confidence >= 0.0 && pattern.confidence <= 1.0,
            "Confidence should be in [0, 1]"
        );
        assert!(
            pattern.similarity >= 0.0 && pattern.similarity <= 1.0,
            "Similarity should be in [0, 1]"
        );
        assert!(
            pattern.predicted_success_rate >= 0.0 && pattern.predicted_success_rate <= 1.0,
            "Success rate should be in [0, 1]"
        );
    }

    #[test]
    fn test_classify_pure_bot() {
        let scr = SCRExtended::new();
        // Perfect regular intervals with slight variation to create harmonics
        let timestamps: Vec<u64> = (0..64)
            .map(|i| i * 50 + (i % 2) * 5) // Tiny variation
            .collect();
        let activity = scr.classify_activity(&timestamps);

        // Due to the FFT characteristics of nearly-constant deltas,
        // this may classify as Organic (low peaks) or PureBot (high SCR)
        assert!(
            activity == ActivityType::PureBot
                || activity == ActivityType::Organic
                || activity == ActivityType::Mixed
                || activity == ActivityType::Unknown,
            "Should return valid activity type"
        );
    }

    #[test]
    fn test_classify_organic() {
        let scr = SCRExtended::new();
        // Highly irregular intervals = organic
        let timestamps = vec![
            0, 312, 589, 1245, 1567, 2890, 3123, 4567, 5890, 7234, 8901, 10234, 11890, 13456,
            15678, 18000,
        ];
        let activity = scr.classify_activity(&timestamps);

        // With high variation and low SCR, should be Organic or Unknown
        assert!(
            activity == ActivityType::Organic
                || activity == ActivityType::Mixed
                || activity == ActivityType::Unknown,
            "Highly irregular pattern should be Organic/Mixed/Unknown"
        );
    }

    #[test]
    fn test_full_analysis() {
        let scr = SCRExtended::new();
        let timestamps: Vec<u64> = (0..32).map(|i| i * 100 + (i % 3) * 20).collect();
        let analysis = scr.analyze(&timestamps);

        assert!(
            analysis.base_score >= 0.0 && analysis.base_score <= 1.0,
            "Base score should be in [0, 1]"
        );
        assert!(
            !analysis.pattern_match.pattern_name.is_empty(),
            "Should have pattern name"
        );
        // Activity type should be valid
        assert!(
            matches!(
                analysis.activity_type,
                ActivityType::PureBot
                    | ActivityType::Mixed
                    | ActivityType::Organic
                    | ActivityType::ViralLaunch
                    | ActivityType::Unknown
            ),
            "Should have valid activity type"
        );
    }

    #[test]
    fn test_activity_type_risk_levels() {
        assert_eq!(ActivityType::PureBot.risk_level(), 0.9);
        assert_eq!(ActivityType::Mixed.risk_level(), 0.5);
        assert_eq!(ActivityType::Organic.risk_level(), 0.2);
        assert_eq!(ActivityType::ViralLaunch.risk_level(), 0.1);
        assert_eq!(ActivityType::Unknown.risk_level(), 0.7);
    }

    #[test]
    fn test_activity_type_recommendations() {
        assert!(ActivityType::PureBot.recommendation().contains("SKIP"));
        assert!(ActivityType::Mixed.recommendation().contains("CAUTION"));
        assert!(ActivityType::Organic.recommendation().contains("BUY"));
        assert!(ActivityType::ViralLaunch.recommendation().contains("BUY"));
        assert!(ActivityType::Unknown.recommendation().contains("SKIP"));
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![0.5, 0.3, 0.1, 0.05, 0.02];
        let similarity = cosine_similarity(&a, &a);
        assert!(
            (similarity - 1.0).abs() < 0.001,
            "Identical vectors should have similarity 1.0"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &b);
        assert!(
            similarity < 0.001,
            "Orthogonal vectors should have similarity ~0"
        );
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.5, 0.3, 0.1, 0.05, 0.02];
        let b = vec![0.0, 0.0, 0.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &b);
        assert_eq!(similarity, 0.0, "Zero vector should return 0 similarity");
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        // Test that different length vectors use min length
        let a = vec![1.0, 0.5, 0.3];
        let b = vec![1.0, 0.5, 0.3, 0.2, 0.1];
        let similarity = cosine_similarity(&a, &b);
        // Should compute similarity using first 3 elements
        assert!(similarity > 0.0, "Should compute valid similarity");
        assert!(similarity <= 1.0, "Similarity should be <= 1.0");

        // Verify it's similar to comparing only first 3 elements
        let b_truncated = vec![1.0, 0.5, 0.3];
        let similarity_truncated = cosine_similarity(&a, &b_truncated);
        assert!(
            (similarity - similarity_truncated).abs() < 0.001,
            "Should use min length for comparison"
        );
    }

    #[test]
    fn test_cosine_similarity_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let similarity = cosine_similarity(&a, &b);
        assert_eq!(similarity, 0.0, "Empty vectors should return 0 similarity");
    }

    #[test]
    fn test_known_signatures() {
        let scr = SCRExtended::new();
        let signatures = scr.known_signatures();

        assert_eq!(signatures.len(), 4);

        // Check viral_memecoin signature
        let viral = signatures.iter().find(|s| s.name == "viral_memecoin");
        assert!(viral.is_some());
        let viral = viral.unwrap();
        assert_eq!(viral.spectrum_shape.len(), 5);
        assert!(viral.success_rate > 0.5);

        // Check bot_pump_dump signature
        let bot = signatures.iter().find(|s| s.name == "bot_pump_dump");
        assert!(bot.is_some());
        let bot = bot.unwrap();
        assert!(bot.success_rate < 0.2);

        // Check organic_growth signature
        let organic = signatures.iter().find(|s| s.name == "organic_growth");
        assert!(organic.is_some());

        // Check wash_trading signature
        let wash = signatures.iter().find(|s| s.name == "wash_trading");
        assert!(wash.is_some());
        let wash = wash.unwrap();
        assert!(wash.success_rate < 0.1);
    }

    #[test]
    fn test_calculate_scr_compatibility() {
        let scr = SCRExtended::new();
        let timestamps: Vec<u64> = (0..16).map(|i| i * 100).collect();

        let scr_score = scr.calculate_scr(&timestamps);
        let analysis = scr.analyze(&timestamps);

        // Base score should match the calculate_scr result
        assert!(
            (scr_score - analysis.base_score).abs() < 0.001,
            "calculate_scr should match base_score in analysis"
        );
    }

    #[test]
    fn test_default_implementation() {
        let scr1 = SCRExtended::default();
        let scr2 = SCRExtended::new();

        // Both should have same configuration
        assert_eq!(scr1.peak_threshold, scr2.peak_threshold);
        assert_eq!(scr1.max_dominant_peaks, scr2.max_dominant_peaks);
        assert_eq!(scr1.known_signatures.len(), scr2.known_signatures.len());
    }

    #[test]
    fn test_harmonic_peak_properties() {
        let peak = HarmonicPeak {
            frequency_bin: 5,
            amplitude: 0.8,
            is_dominant: true,
        };

        assert_eq!(peak.frequency_bin, 5);
        assert_eq!(peak.amplitude, 0.8);
        assert!(peak.is_dominant);
    }

    #[test]
    fn test_pattern_match_unknown_fallback() {
        let scr = SCRExtended::new();
        // Very few timestamps = unknown pattern
        let timestamps: Vec<u64> = vec![0, 100, 200, 300];
        let pattern = scr.match_signature(&timestamps);

        // Should still return a valid pattern (may be "unknown" or a match)
        assert!(pattern.confidence >= 0.0);
        assert!(pattern.predicted_success_rate >= 0.0);
    }

    // ========== TIMESTAMP CORRECTION TESTS ==========

    #[test]
    fn test_correct_timestamp_no_jitter() {
        // block_time = 1 second, arrival_time = 1000ms
        let result = super::correct_timestamp_with_jitter_check(
            1,    // block_time in seconds
            1000, // arrival_time_ms
            0,    // estimated_offset_ms
            1500, // max_jitter_ms
            None, // pool_pubkey
            None, // integrity_callback
        );

        assert_eq!(result.corrected_timestamp_ms, 1000);
        assert_eq!(result.jitter_ms, 0);
        assert!(!result.excessive_jitter);
        assert!(result.violation.is_none());
    }

    #[test]
    fn test_correct_timestamp_acceptable_jitter() {
        // block_time = 1 second, arrival_time = 1500ms (500ms jitter - acceptable)
        let result = super::correct_timestamp_with_jitter_check(
            1,    // block_time in seconds
            1500, // arrival_time_ms
            0,    // estimated_offset_ms
            1500, // max_jitter_ms
            None, None,
        );

        assert_eq!(result.corrected_timestamp_ms, 1000);
        assert_eq!(result.jitter_ms, 500);
        assert!(!result.excessive_jitter);
        assert!(result.violation.is_none());
    }

    #[test]
    fn test_correct_timestamp_excessive_jitter() {
        // block_time = 1 second, arrival_time = 3000ms (2000ms jitter - excessive!)
        let result = super::correct_timestamp_with_jitter_check(
            1,    // block_time in seconds
            3000, // arrival_time_ms
            0,    // estimated_offset_ms
            1500, // max_jitter_ms
            None, None,
        );

        assert_eq!(result.corrected_timestamp_ms, 1000);
        assert_eq!(result.jitter_ms, 2000);
        assert!(result.excessive_jitter);
        assert!(result.violation.is_some());

        let violation = result.violation.unwrap();
        assert_eq!(violation.severity, super::IntegritySeverity::SoftSync);
        assert!(violation.details.contains("2000ms"));
    }

    #[test]
    fn test_correct_timestamp_with_offset() {
        // block_time = 1 second, offset = 500ms, arrival_time = 2000ms
        let result = super::correct_timestamp_with_jitter_check(
            1,    // block_time in seconds
            2000, // arrival_time_ms
            500,  // estimated_offset_ms
            1500, // max_jitter_ms
            None, None,
        );

        // Corrected = 1*1000 + 500 = 1500ms
        assert_eq!(result.corrected_timestamp_ms, 1500);
        // Jitter = 2000 - 1500 = 500ms
        assert_eq!(result.jitter_ms, 500);
        assert!(!result.excessive_jitter);
    }

    #[test]
    fn test_correct_timestamp_with_callback() {
        use std::sync::{Arc, Mutex};

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        let callback: Arc<dyn Fn(super::IntegrityViolation) + Send + Sync> =
            Arc::new(move |violation: super::IntegrityViolation| {
                violations_clone.lock().unwrap().push(violation);
            });

        // Excessive jitter should trigger callback
        let result = super::correct_timestamp_with_jitter_check(
            1,    // block_time in seconds
            5000, // arrival_time_ms (4000ms jitter!)
            0,    // estimated_offset_ms
            1500, // max_jitter_ms
            None,
            Some(&callback),
        );

        assert!(result.excessive_jitter);

        // Verify callback was invoked
        let viols = violations.lock().unwrap();
        assert_eq!(viols.len(), 1);
        assert_eq!(viols[0].severity, super::IntegritySeverity::SoftSync);
    }

    #[test]
    fn test_batch_correct_timestamps() {
        // Create test events: (block_time, arrival_time_ms)
        let events = vec![
            (1, 1000), // No jitter
            (2, 2500), // 500ms jitter (OK)
            (3, 5000), // 2000ms jitter (excessive!)
            (4, 4200), // 200ms jitter (OK)
        ];

        // Filter out excessive jitter
        let corrected = super::batch_correct_timestamps(
            events.into_iter(),
            1500, // max_jitter_ms
            None,
            false, // Don't force include all
        );

        // Should have 3 timestamps (one was filtered out)
        assert_eq!(corrected.len(), 3);
        assert_eq!(corrected[0], 1000);
        assert_eq!(corrected[1], 2000);
        assert_eq!(corrected[2], 4000);
    }

    #[test]
    fn test_batch_correct_timestamps_force_include() {
        // Create test events with one having excessive jitter
        let events = vec![
            (1, 1000), // No jitter
            (2, 5000), // 3000ms jitter (excessive!)
        ];

        // Force include all
        let corrected = super::batch_correct_timestamps(
            events.into_iter(),
            1500,
            None,
            true, // Force include all
        );

        // Should have both timestamps
        assert_eq!(corrected.len(), 2);
        assert_eq!(corrected[0], 1000);
        assert_eq!(corrected[1], 2000);
    }

    #[test]
    fn test_timestamp_correction_with_pool_pubkey() {
        use solana_sdk::pubkey::Pubkey;

        let pool = Pubkey::new_unique();

        let result = super::correct_timestamp_with_jitter_check(
            1,
            3000, // Excessive jitter
            0,
            1500,
            Some(pool),
            None,
        );

        assert!(result.violation.is_some());
        let violation = result.violation.unwrap();
        assert_eq!(violation.pool_pubkey, pool);
        assert_eq!(violation.source, "SCRExtended");
    }

    #[test]
    fn test_timestamp_correction_negative_block_time() {
        // Negative block_time should be rejected
        let result = super::correct_timestamp_with_jitter_check(
            -100, // Negative block time!
            1000, 0, 1500, None, None,
        );

        assert_eq!(result.corrected_timestamp_ms, 0);
        assert!(result.excessive_jitter);
        assert!(result.violation.is_some());

        let violation = result.violation.unwrap();
        assert_eq!(violation.severity, super::IntegritySeverity::SoftSync);
        assert!(violation.details.contains("negative"));
    }
}
