//! Stub module for deprecated QASS types
//!
//! **DEPRECATED**: This module contains stub types for backward compatibility.
//! QASS (Quantum-Style Amplitude Superposition Scoring) has been replaced by
//! SurvivorScore as the main scoring system.
//!
//! These types are kept temporarily to allow gradual migration of dependent code.
//! New code should use SurvivorScore directly.

use serde::{Deserialize, Serialize};

/// Maximum number of waves (deprecated)
pub const MAX_WAVES: usize = 16;

/// Number of dominant waves to track (deprecated)
pub const NUM_DOMINANT_WAVES: usize = 3;

/// HeuristicWave - Deprecated, kept for backward compatibility
///
/// **DEPRECATED**: Use SurvivorScore components instead.
/// This type represents a signal wave for QASS superposition which has been replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeuristicWave {
    /// Wave name (e.g., "ψ_ssmi")
    pub name: String,
    /// Signal amplitude (0.0-1.0)
    pub amplitude: f64,
    /// Signal phase (-π to π)
    pub phase: f64,
    /// Signal confidence (0.0-1.0)
    pub confidence: f64,
    /// Whether this is a synthetic wave
    pub synthetic: bool,
}

impl HeuristicWave {
    /// Create a new HeuristicWave
    pub fn new(name: &str, amplitude: f64, phase: f64, confidence: f64) -> Self {
        Self {
            name: name.to_string(),
            amplitude: amplitude.clamp(0.0, 1.0),
            phase,
            confidence: confidence.clamp(0.0, 1.0),
            synthetic: false,
        }
    }

    /// Create a synthetic wave
    pub fn new_synthetic(name: &str, amplitude: f64, phase: f64, confidence: f64) -> Self {
        Self {
            name: name.to_string(),
            amplitude: amplitude.clamp(0.0, 1.0),
            phase,
            confidence: confidence.clamp(0.0, 1.0),
            synthetic: true,
        }
    }
}

impl Default for HeuristicWave {
    fn default() -> Self {
        Self {
            name: "ψ_default".to_string(),
            amplitude: 0.5,
            phase: 0.0,
            confidence: 0.5,
            synthetic: false,
        }
    }
}

/// Wave contribution to score (deprecated)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveContribution {
    /// Wave name
    pub name: String,
    /// Contribution to final score
    pub contribution: f64,
    /// Raw amplitude
    pub amplitude: f64,
    /// Phase value
    pub phase: f64,
}

/// Data source for QASS (deprecated)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataSource {
    /// Live data
    Live,
    /// Cached data
    Cached,
    /// Synthetic/fallback data
    Synthetic,
}

impl Default for DataSource {
    fn default() -> Self {
        DataSource::Live
    }
}

/// QASS Result - Deprecated, kept for backward compatibility
///
/// **DEPRECATED**: Use SurvivorScoreResult instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QASSResult {
    /// Score (0.0-1.0) - Now returns neutral 0.5
    pub score: f64,
    /// Score as 0-100 integer - Now returns 50
    pub score_100: u8,
    /// Confidence (0.0-1.0) - Now returns 0.0
    pub confidence: f64,
    /// Whether result is valid - Now returns false
    pub is_valid: bool,
    /// Dominant wave names
    pub dominant_waves: Vec<String>,
    /// Wave breakdown
    pub wave_breakdown: Vec<WaveContribution>,
    /// Data source
    pub data_source: DataSource,
    /// Analysis time in nanoseconds
    pub analysis_time_ns: u64,
}

impl Default for QASSResult {
    fn default() -> Self {
        Self {
            score: 0.5,
            score_100: 50,
            confidence: 0.0,
            is_valid: false,
            dominant_waves: vec![],
            wave_breakdown: vec![],
            data_source: DataSource::Synthetic,
            analysis_time_ns: 0,
        }
    }
}

/// Quantum Amplitude Scorer - Deprecated stub
///
/// **DEPRECATED**: This scorer is a no-op stub. Use SurvivorScoreCalculator instead.
#[derive(Debug, Clone, Default)]
pub struct QuantumAmplitudeScorer;

impl QuantumAmplitudeScorer {
    /// Create new scorer (no-op)
    pub fn new() -> Self {
        Self
    }

    /// Score waves - Always returns neutral result
    ///
    /// **DEPRECATED**: Use SurvivorScoreCalculator.calculate() instead.
    pub fn score(&self, _waves: &[HeuristicWave]) -> QASSResult {
        QASSResult::default()
    }

    /// Superposition score - Always returns neutral 0.5
    pub fn superposition_score(&self, _waves: &[HeuristicWave]) -> f64 {
        0.5
    }
}

/// Generic scorer with const N - Deprecated stub
#[derive(Debug, Clone, Default)]
pub struct QuantumAmplitudeScorerN<const N: usize>;

impl<const N: usize> QuantumAmplitudeScorerN<N> {
    /// Create new generic scorer (no-op)
    pub fn new() -> Self {
        Self
    }

    /// Score waves - Always returns neutral result
    pub fn score(&self, _waves: &[HeuristicWave]) -> QASSResult {
        QASSResult::default()
    }
}
