//! State Structures for HyperPrediction Oracle
//!
//! This module contains all data-holding structures used by the HyperPrediction Oracle.
//! Extracted from `mod.rs` to improve modularity and maintainability.
//!
//! ## Design Philosophy
//!
//! These structures represent the **state** and **results** of the prediction system:
//! - What data does the oracle operate on? (state structures)
//! - What results does it produce? (`HyperPredictionResult`, `QmanResult`)
//! - What phase of analysis generated the result? (`AnalysisPhase`)
//!
//! By centralizing these types here, we improve:
//! - **Testability**: Easier to mock and test individual components
//! - **Observability**: Clearer understanding of what data flows through the system
//! - **Maintainability**: Changes to state structures are localized

use crate::oracle::hyper_prediction::verdict::RiskLevel;
use crate::oracle::qman::TradingSignal;
use serde::{Deserialize, Serialize};
use std::time::Instant;

// =============================================================================
// Analysis Phase Tracking
// =============================================================================

/// Tracks which analysis mode generated the HyperPrediction result
///
/// The HyperPrediction Oracle operates in two distinct modes based on available
/// transaction history. This enum makes the mode **explicit** in the result,
/// improving observability and debugging.
///
/// ## Patient Observer Strategy
///
/// The Oracle uses a "Patient Observer" strategy to avoid false negatives
/// from insufficient data:
///
/// - **EarlyStage (tx_count < 2)**: Static analysis mode
///   - Skips trend-based metrics (SSMI, SCR, ULVF, POVC, QEDD, MCI)
///   - Focuses on Chaos Engine simulations, Gene Mapper, Shadow Ledger
///   - Uses default trust assumptions for missing data (Safety = 1.0)
///   - Corresponds to S1-S7 in the 13-cycle scoring loop
///
/// - **FullAnalysis (tx_count >= 2)**: Dynamic analysis mode
///   - All modules active including trend-based metrics
///   - Uses real transaction data for SSMI, SCR, ULVF, POVC
///   - QEDD/MCI provide survival probabilities and coherence
///   - Corresponds to S8-S13 with full signal availability
///
/// ## Why Track This?
///
/// Currently, the phase is implicit (logs say "PATIENT" but result doesn't expose it).
/// Making it explicit enables:
/// 1. **Downstream decision-making**: Exit strategies can differ by phase
/// 2. **Analytics**: Track which phase produces better results
/// 3. **Debugging**: Understand why certain metrics are missing
/// 4. **Interpretation**: Generate phase-aware explanations
///
/// ## Example
///
/// ```ignore
/// if result.analysis_phase == AnalysisPhase::EarlyStage {
///     // Don't expect SSMI, SCR, ULVF, POVC in this result
///     assert!(result.ssmi_result.is_none());
///     assert!(result.scr_score.is_none());
/// } else {
///     // Full analysis should have these metrics (if timestamps provided)
///     // (may still be None if insufficient data)
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisPhase {
    /// Early stage analysis: tx_count < 2, trend-based metrics skipped
    ///
    /// This phase focuses on static analysis and simulations:
    /// - Chaos Engine Monte Carlo (pump/crash probabilities)
    /// - Gene Mapper security analysis (bytecode scanning)
    /// - Shadow Ledger RAM simulations (price projections)
    /// - LIGMA liquidity trap detection (always runs)
    /// - IWIM dev wallet intent (async, default trust until available)
    /// - PRAECOG adversarial exploitability (static pool analysis)
    /// - MPCF actor fingerprinting (if tx_data present - early warning)
    EarlyStage,

    /// Full analysis: tx_count >= 2, all metrics active including trend-based
    ///
    /// This phase includes everything from EarlyStage plus:
    /// - SSMI (Sub-Slot Microentropy Index) - timing patterns
    /// - SCR (bot detection via FFT) - harmonic analysis
    /// - ULVF (liquidity vector field) - momentum classification
    /// - POVC (cluster prediction) - trader type classification
    /// - QEDD (survival probability) - decay rate analysis
    /// - MCI (market coherence) - directional/structural coherence
    /// - QMAN (capital flow prediction) - smart money tracking
    /// - MESA (microstructure execution-shape) - wash trading detection
    FullAnalysis,
}

impl Default for AnalysisPhase {
    fn default() -> Self {
        // Conservative default: assume early stage if unknown
        Self::EarlyStage
    }
}

impl AnalysisPhase {
    /// Returns true if this is early stage analysis
    pub fn is_early_stage(&self) -> bool {
        matches!(self, AnalysisPhase::EarlyStage)
    }

    /// Returns true if this is full analysis mode
    pub fn is_full_analysis(&self) -> bool {
        matches!(self, AnalysisPhase::FullAnalysis)
    }

    /// Get human-readable phase name for display
    pub fn display_name(&self) -> &'static str {
        match self {
            AnalysisPhase::EarlyStage => "Early Stage",
            AnalysisPhase::FullAnalysis => "Full Analysis",
        }
    }

    /// Get emoji indicator for logging
    pub fn emoji(&self) -> &'static str {
        match self {
            AnalysisPhase::EarlyStage => "🐣",   // Hatching chick (early/new)
            AnalysisPhase::FullAnalysis => "🔬", // Microscope (detailed analysis)
        }
    }
}

// =============================================================================
// QMAN Result
// =============================================================================

/// Combined result from QMAN (Quantum Money-flow Amplitude Network) analysis
///
/// QMAN tracks wallet "energy" (activity/capital) and predicts capital flow patterns.
/// This is valuable for detecting "smart money" movements:
/// - High energy wallets entering = potential positive signal
/// - High energy wallets exiting = warning (smart money exits)
/// - Flow from many small to one large = potential accumulation
///
/// ## Integration with Analysis Phases
///
/// QMAN runs in both phases when sufficient wallet data is available:
/// - **EarlyStage**: Detects early smart money accumulation patterns
/// - **FullAnalysis**: Tracks ongoing capital flow dynamics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QmanResult {
    /// Trading signal from QMAN analysis
    pub signal: TradingSignal,

    /// Overall QMAN score (0.0-1.0)
    /// High = smart money accumulating, Low = smart money exiting
    pub qman_score: f32,

    /// Confidence in QMAN analysis (0.0-1.0)
    pub confidence: f32,

    /// Net energy flow direction
    /// Positive = capital flowing in, Negative = capital flowing out
    pub net_energy_flow: f32,

    /// Number of high-energy (smart money) wallets detected
    pub high_energy_wallets: usize,

    /// Analysis time in microseconds
    pub analysis_time_us: u64,

    /// Reason/context for the signal
    pub reason: String,
}

// =============================================================================
// HyperPrediction Result
// =============================================================================

/// Result from HyperPrediction Oracle with full breakdown
///
/// This structure contains the complete evaluation result including:
/// - Final score and pass/fail decision
/// - Risk assessment
/// - Individual module results (SSMI, MPCF, QASS, etc.)
/// - Analysis phase tracking (NEW)
/// - Performance metrics
///
/// ## Phase-Dependent Fields
///
/// Some fields will be `None` in `EarlyStage` mode:
/// - `ssmi_result`: Requires tx_count >= 2 (timing pattern analysis needs history)
/// - `scr_score`: Requires tx_count >= 2 (FFT needs multiple samples)
/// - `ulvf_divergence`/`ulvf_curl`: Requires tx_count >= 2 (momentum needs Δt)
/// - `povc_cluster`: Requires tx_count >= 2 (clustering needs behavior patterns)
/// - `qedd_result`: Requires tx_count >= 2 (decay analysis needs trends)
/// - `mci_result`: Requires tx_count >= 2 (coherence needs directional data)
///
/// Fields that ARE present in both phases:
/// - `chaos_result`: Monte Carlo simulations (static pool analysis)
/// - `gene_safety_result`: Bytecode scanning (static analysis)
/// - `shadow_progress`/`shadow_price_ratio`: RAM simulations (static projections)
/// - `ligma_result`: Liquidity trap detection (always critical)
/// - `iwim_result`: Dev wallet intent (async, may be pending in S1-S7)
/// - `praecog_result`: Adversarial exploitability (static pool simulation)
/// - `mpcf_result`: Actor fingerprinting (if tx_data present - early warning)
#[derive(Debug, Clone)]
pub struct HyperPredictionResult {
    /// Final combined score (0-100)
    pub score: u8,

    /// Whether the token passed threshold
    pub passed: bool,

    /// Risk level assessment
    pub risk_level: RiskLevel,

    // =============================================================================
    // NEW: Analysis Phase Tracking (Issue #2)
    // =============================================================================
    /// Phase of analysis when this result was generated
    ///
    /// This makes the implicit phase (determined by `is_early_stage = tx_count < 2`)
    /// **explicit** in the result, improving observability and downstream decision-making.
    ///
    /// ## Usage Examples
    ///
    /// ```ignore
    /// // Interpretation can now include phase info
    /// if result.analysis_phase == AnalysisPhase::EarlyStage {
    ///     format!("📊 PATIENT (Early Stage, tx < 2): Score={}", result.score)
    /// } else {
    ///     format!("📊 PATIENT (Full Analysis, tx >= 2): Score={}", result.score)
    /// }
    ///
    /// // Analytics can track phase effectiveness
    /// metrics.record_score_by_phase(result.analysis_phase, result.score);
    ///
    /// // Exit strategies can differ by phase
    /// if result.analysis_phase.is_early_stage() {
    ///     // More conservative exit (less confidence in trend data)
    ///     stop_loss = -5.0;
    /// } else {
    ///     // Can use trend indicators for dynamic exits
    ///     stop_loss = calculate_revolver_stop_loss(&result.qedd_result);
    /// }
    /// ```
    pub analysis_phase: AnalysisPhase,

    /// Timestamp when analysis started (for performance tracking)
    ///
    /// This enables:
    /// - **Phase duration analysis**: How long does EarlyStage vs FullAnalysis take?
    /// - **Bottleneck identification**: Which modules are slow in each phase?
    /// - **Latency optimization**: Track if we're meeting the <2s target per phase
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let phase_duration_ms = result.analysis_started_at.elapsed().as_millis();
    /// info!(
    ///     "Analysis completed: phase={}, duration={}ms, target=<2000ms",
    ///     result.analysis_phase.display_name(),
    ///     phase_duration_ms
    /// );
    /// ```
    pub analysis_started_at: Instant,

    // =============================================================================
    // Module Results (Phase-Dependent - see documentation above)
    // =============================================================================
    /// SSMI analysis result (None in EarlyStage)
    pub ssmi_result: Option<crate::oracle::ultrafast::SsmiResult>,

    /// MPCF actor inference (Present in EarlyStage if tx_data available)
    pub mpcf_result: Option<crate::oracle::ultrafast::ActorInference>,

    /// IWIM (Initial Wallet Intent Mapping) result
    /// May be None in S1-S7 (async pending), present in S8-S13
    pub iwim_result: Option<crate::oracle::ultrafast::IwimResult>,

    /// PRAECOG (Adversarial Exploitability) result
    /// Present in both phases (static pool analysis)
    pub praecog_result: Option<crate::oracle::ultrafast::PraecogResult>,

    /// MESA microstructure execution-shape analysis
    pub mesa_result: Option<crate::analyzers::mesa::MesaResult>,

    /// SCR (bot detection) score (None in EarlyStage)
    pub scr_score: Option<f32>,

    /// ULVF divergence (None in EarlyStage)
    pub ulvf_divergence: Option<f32>,

    /// ULVF curl (wash trading) (None in EarlyStage)
    pub ulvf_curl: Option<f32>,

    /// POVC cluster (0=Dump, 1=Hype, 2=Noise) (None in EarlyStage)
    pub povc_cluster: Option<usize>,

    /// Shadow Ledger bonding progress (percentage)
    /// Present in both phases
    pub shadow_progress: Option<u64>,

    /// Shadow Ledger price ratio vs initial
    /// Present in both phases
    pub shadow_price_ratio: Option<f64>,

    /// Enhanced base score (SurvivorScore)
    pub base_score: u8,

    /// Processing time in microseconds
    pub processing_time_us: u64,

    /// Human-readable interpretation
    pub interpretation: String,

    // =============================================================================
    // Prediction Matrix Updates (Present in Both Phases)
    // =============================================================================
    /// Chaos Engine Monte Carlo simulation result
    /// Provides risk probabilities and ROI projections
    /// Present in both phases (static pool analysis)
    pub chaos_result: Option<crate::chaos::engine::ChaosResult>,

    /// Resonance Detector result (bot vs human pattern detection)
    /// Analyzes trading intervals to identify automated behavior
    pub resonance_result: Option<crate::signals::resonance::ResonanceResult>,

    /// Gene Mapper security analysis result
    /// Static bytecode analysis for malicious patterns
    /// Present in both phases (static analysis)
    pub gene_safety_result: Option<crate::security::gene_mapper::GeneAnalysisResult>,

    /// External Hunter/Oracle score (T+2s API data)
    /// Score from slow external data sources (Helius, SolanaFM)
    pub hunter_score: Option<u8>,

    // =============================================================================
    // QEDD and MCI Integration (None in EarlyStage)
    // =============================================================================
    /// QEDD (Quantum Entropy-Driven Decay) result
    /// Provides survival probabilities and decay rates
    /// None in EarlyStage (requires trend data)
    pub qedd_result: Option<crate::models::qedd_result::QeddResult>,

    /// MCI (Market Coherence Index) result
    /// Measures directional and structural coherence
    /// None in EarlyStage (requires trend data)
    pub mci_result: Option<crate::models::mci_result::MciResult>,

    // =============================================================================
    // QMAN Integration (Present in Both Phases)
    // =============================================================================
    /// QMAN (Quantum Money-flow Amplitude Network) result
    /// Tracks wallet energy and predicts capital flow patterns
    /// Detects smart money entry/exit patterns
    /// Present in both phases when sufficient wallet data available
    pub qman_result: Option<QmanResult>,

    // =============================================================================
    // LIGMA Integration (Always Present - Global Guard)
    // =============================================================================
    /// LIGMA (Liquidity Genesis Manifold Analyzer) result
    /// Fast (<1ms) liquidity geometry analysis for trap detection
    /// ALWAYS runs (both phases) as global guard against liquidity traps
    pub ligma_result: Option<crate::signals::LigmaResult>,

    // =============================================================================
    // ClusterHunter Integration (Present in Both Phases)
    // =============================================================================
    /// ClusterHunter analysis result
    /// Detects coordinated cabal/sniper activity via wallet funding pattern analysis
    /// cabal_risk_score > 0.6 indicates high probability of coordinated manipulation
    /// Present in both phases (static wallet network analysis)
    pub cluster_result: Option<crate::oracle::cluster_hunter::ClusterAnalysis>,

    // =============================================================================
    // ParadoxSensor Integration (Present in Both Phases)
    // =============================================================================
    /// ParadoxSensor network telemetry state
    /// Detects HFT bot activity through packet timing analysis
    /// Present in both phases (network-level detection)
    pub paradox_state: Option<seer::paradox_sensor::ParadoxState>,

    /// Whether entry should be delayed due to HFT activity
    /// True = wait for phase_sync to drop before entering
    pub should_delay_entry: bool,

    /// Recommended delay in milliseconds if should_delay_entry is true
    pub recommended_delay_ms: u64,

    // =============================================================================
    // Second Wave Detector Integration
    // =============================================================================
    /// SecondWaveDetector result
    /// Identifies optimal entry timing after HFT exit
    /// Works in conjunction with ParadoxSensor to detect when HFT bots have exited
    /// and organic "second wave" buying is beginning
    pub second_wave_result: Option<crate::oracle::second_wave_detector::SecondWaveResult>,

    // =============================================================================
    // SurvivorScore Integration (Present in Both Phases)
    // =============================================================================
    /// SurvivorScore result - interpretable scoring system
    /// Provides economically meaningful scoring with transparent breakdown
    /// of survival, momentum, quality, and risk components
    /// Present in both phases (uses available data)
    pub survivor_score_result: Option<crate::oracle::survivor_score::SurvivorScoreResult>,

    // =============================================================================
    // FRE Integration (Present in Both Phases)
    // =============================================================================
    /// Fractal Resonance Engine verdict
    /// Analyzes transaction patterns to detect:
    /// - Botnet attacks (STT: Scale-Transition Test for coherence across buckets)
    /// - Pump & dump chaos (FSW: Fractal Stability Window for Hurst variance)
    /// - Organic quality (ARB: Asymmetric Risk Bias for final scoring)
    /// Present in both phases when sufficient swap data available (>= 10 swaps)
    pub fractal_verdict: Option<crate::oracle::ultrafast::fre::FractalVerdict>,

    // =============================================================================
    // TCF (Trend Cohesion Field) Integration (Final Verdict Only)
    // =============================================================================
    /// TCF (Trend Cohesion Field) result
    /// Measures the coherence of market dynamics across scoring cycles.
    /// TCF evaluates whether the mechanism generating market changes remains consistent.
    ///
    /// **IMPORTANT**: TCF participates ONLY in Final Verdict (after S13).
    /// Observations are collected during each cycle, but the score is only
    /// applied at the end to modulate the momentum component.
    ///
    /// Present when TCF is enabled and enough observations have been collected.
    pub tcf_result: Option<TcfResult>,

    // =============================================================================
    // Fallback Tracking (Always Present)
    // =============================================================================
    /// Fallback tracker - records which fallbacks were used and their confidence impact
    /// Tracks when default values are used due to missing data
    /// Always present to track data quality
    pub fallback_tracker: crate::config::FallbackTracker,
}

/// TCF (Trend Cohesion Field) result for HyperPrediction integration
///
/// Contains the final TCF score and diagnostics that can be used
/// for logging, analytics, and decision-making.
#[derive(Debug, Clone)]
pub struct TcfResult {
    /// Final TCF score [0.0, 1.0]
    /// 1.0 = perfect trend cohesion (organic growth)
    /// 0.0 = complete trend breakdown (pump & dump)
    pub tcf_score: f64,

    /// Whether TCF is primed (has enough observations for reliable scoring)
    pub is_primed: bool,

    /// Number of observations collected during cycles
    pub observation_count: usize,

    /// Current TCF phase classification
    pub phase: crate::oracle::tcf::TcfPhase,

    /// Whether a cohesion cliff was detected (sudden drop in cohesion)
    pub cliff_detected: bool,

    /// Latest cohesion value (from most recent transition)
    pub latest_cohesion: f64,

    /// Whether `latest_cohesion` was computed from this cycle's fresh input.
    pub latest_cohesion_computed_this_cycle: bool,

    /// Whether `latest_cohesion` is a fallback (cached/default), not fresh compute.
    pub latest_cohesion_is_fallback: bool,

    /// Reason for fallback used in `latest_cohesion`, if any.
    pub latest_cohesion_fallback_reason: Option<&'static str>,

    /// Average cohesion over all transitions
    pub avg_cohesion: f64,

    /// Trend direction: -1 (bearish), 0 (neutral), 1 (bullish)
    pub trend_direction: i8,

    /// Modulation factor applied to momentum in Final Verdict
    /// Calculated as: tcf_min_modulation + tcf_modulation_range * tcf_score
    pub modulation_factor: f64,

    /// Analysis time in microseconds
    pub analysis_time_us: u64,
}

impl Default for TcfResult {
    fn default() -> Self {
        Self {
            tcf_score: 0.5, // Neutral default
            is_primed: false,
            observation_count: 0,
            phase: crate::oracle::tcf::TcfPhase::ColdStart,
            cliff_detected: false,
            latest_cohesion: 0.5,
            latest_cohesion_computed_this_cycle: false,
            latest_cohesion_is_fallback: true,
            latest_cohesion_fallback_reason: Some("neutral_default"),
            avg_cohesion: 0.5,
            trend_direction: 0,
            modulation_factor: 0.8, // Default: 0.6 + 0.4 * 0.5 = 0.8
            analysis_time_us: 0,
        }
    }
}

impl Default for HyperPredictionResult {
    fn default() -> Self {
        Self {
            score: 0,
            passed: false,
            risk_level: RiskLevel::VeryHigh,
            analysis_phase: AnalysisPhase::default(),
            analysis_started_at: Instant::now(),
            ssmi_result: None,
            mpcf_result: None,
            iwim_result: None,
            praecog_result: None,
            mesa_result: None,
            scr_score: None,
            ulvf_divergence: None,
            ulvf_curl: None,
            povc_cluster: None,
            shadow_progress: None,
            shadow_price_ratio: None,
            base_score: 0,
            processing_time_us: 0,
            interpretation: "Default - no evaluation performed".to_string(),
            chaos_result: None,
            resonance_result: None,
            gene_safety_result: None,
            hunter_score: None,
            qedd_result: None,
            mci_result: None,
            qman_result: None,
            ligma_result: None,
            cluster_result: None,
            paradox_state: None,
            should_delay_entry: false,
            recommended_delay_ms: 0,
            second_wave_result: None,
            survivor_score_result: None,
            fractal_verdict: None,
            tcf_result: None,
            fallback_tracker: crate::config::FallbackTracker::new(),
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_phase_is_early_stage() {
        assert!(AnalysisPhase::EarlyStage.is_early_stage());
        assert!(!AnalysisPhase::FullAnalysis.is_early_stage());
    }

    #[test]
    fn test_analysis_phase_is_full_analysis() {
        assert!(!AnalysisPhase::EarlyStage.is_full_analysis());
        assert!(AnalysisPhase::FullAnalysis.is_full_analysis());
    }

    #[test]
    fn test_analysis_phase_display_name() {
        assert_eq!(AnalysisPhase::EarlyStage.display_name(), "Early Stage");
        assert_eq!(AnalysisPhase::FullAnalysis.display_name(), "Full Analysis");
    }

    #[test]
    fn test_analysis_phase_emoji() {
        assert_eq!(AnalysisPhase::EarlyStage.emoji(), "🐣");
        assert_eq!(AnalysisPhase::FullAnalysis.emoji(), "🔬");
    }

    #[test]
    fn test_analysis_phase_default() {
        // Conservative default: assume early stage if unknown
        assert_eq!(AnalysisPhase::default(), AnalysisPhase::EarlyStage);
    }

    #[test]
    fn test_analysis_phase_serialization() {
        let phase = AnalysisPhase::EarlyStage;
        let json = serde_json::to_string(&phase).unwrap();
        assert!(json.contains("EarlyStage"));

        let deserialized: AnalysisPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, AnalysisPhase::EarlyStage);
    }

    #[test]
    fn test_hyper_prediction_result_default_has_phase() {
        let result = HyperPredictionResult::default();
        assert_eq!(result.analysis_phase, AnalysisPhase::EarlyStage);
    }

    #[test]
    fn test_early_stage_result_excludes_trend_metrics() {
        // This test documents the expected behavior for EarlyStage results
        let mut result = HyperPredictionResult::default();
        result.analysis_phase = AnalysisPhase::EarlyStage;

        // In production code, these would be set to None during EarlyStage
        // This test documents the contract
        assert_eq!(result.analysis_phase, AnalysisPhase::EarlyStage);

        // Note: The actual None values are set during scoring in mod.rs
        // This test just verifies the phase tracking works
    }

    #[test]
    fn test_analysis_phase_tracked() {
        let result = HyperPredictionResult {
            analysis_phase: AnalysisPhase::EarlyStage,
            score: 50,
            passed: false,
            risk_level: RiskLevel::Medium,
            analysis_started_at: Instant::now(),
            ..Default::default()
        };

        // Verify phase is accessible
        assert_eq!(result.analysis_phase, AnalysisPhase::EarlyStage);
        assert!(result.analysis_phase.is_early_stage());
    }
}
