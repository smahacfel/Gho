//! FRB Integrator - Integration Layer for FRB with QOFSV/QMAN/WHF
//!
//! This module provides the integration layer that composes FRB (Fractal Resonance Bands)
//! signals with the quantum pipeline (QOFSV), quantum capital flow (QMAN), and harmonic
//! field analysis (WHF) for enhanced signal detection and false-positive reduction.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              FRB Integration Pipeline                    │
//! │                                                          │
//! │  Transaction Stream                                      │
//! │         ↓                                                │
//! │   BandExtractor → Band Profiles [short, medium, long]   │
//! │         ↓                                                │
//! │   ResonanceAnalyzer → Resonance Score + Coherence Map   │
//! │         ↓                                                │
//! │   FrbIntegrator:                                         │
//! │     ├── QOFSV Enhancement (coherence boost)             │
//! │     ├── WHF Cross-Validation (wash/bot detection)       │
//! │     └── QMAN Signal Enrichment (flow confidence)        │
//! │         ↓                                                │
//! │   Enhanced Signal Output                                 │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Features
//!
//! 1. **QOFSV Coherence Boost**: Map FRB resonance → amplitude boost for QOFSV StateVector
//! 2. **WHF Validation**: Cross-check WHF wash/bot signals against FRB band patterns
//! 3. **QMAN Enhancement**: Use FRB multi-scale analysis to improve capital flow confidence
//! 4. **False-Positive Reduction**: Multi-signal consensus reduces spurious signals
//!
//! ## Usage
//!
//! ```rust,ignore
//! use ghost_brain::signals::{BandExtractor, ResonanceAnalyzer, FrbIntegrator};
//!
//! let mut extractor = BandExtractor::new();
//! let analyzer = ResonanceAnalyzer::new();
//! let mut integrator = FrbIntegrator::new();
//!
//! // Process transaction stream
//! for tx in transaction_stream {
//!     extractor.add_transaction(tx);
//! }
//!
//! // Extract bands and analyze resonance
//! let bands = extractor.extract_bands();
//! let frb_result = analyzer.analyze(bands);
//!
//! // Integrate with other signals
//! let qofsv_boost = integrator.calculate_qofsv_boost(&frb_result);
//! let whf_validation = integrator.validate_whf_signal(&frb_result, &whf_signal);
//! ```

use crate::chaos::whf_signals::{WhfSignal, WhfSignalType};
use crate::oracle::qman::signal_detector::{SignalResult as QmanSignal, TradingSignal};
use crate::signals::frb::{BandProfile, FrbResult, FrbSignal, ResonanceConfig};
use serde::{Deserialize, Serialize};

/// Configuration for FRB integration thresholds
#[derive(Debug, Clone)]
pub struct FrbResonanceConfig {
    /// Minimum resonance score for QOFSV coherence boost
    pub min_resonance_for_boost: f32,

    /// Maximum coherence boost factor for QOFSV (amplitude multiplier)
    pub max_coherence_boost: f32,

    /// Threshold for detecting bot manipulation via FRB
    pub bot_manipulation_threshold: f32,

    /// Threshold for detecting wash trading via FRB
    pub wash_trading_threshold: f32,

    /// Minimum buyer count for organic signal validation
    pub min_organic_buyers: u32,

    /// False positive rate tolerance (0.0-1.0)
    /// Higher = more permissive, lower = stricter filtering
    pub false_positive_tolerance: f32,
}

impl Default for FrbResonanceConfig {
    fn default() -> Self {
        Self {
            min_resonance_for_boost: 0.5,
            max_coherence_boost: 1.5,
            bot_manipulation_threshold: 0.7,
            wash_trading_threshold: 0.6,
            min_organic_buyers: 5,
            false_positive_tolerance: 0.2,
        }
    }
}

impl FrbResonanceConfig {
    /// Create config with custom thresholds for production tuning
    pub fn with_thresholds(
        min_resonance: f32,
        max_boost: f32,
        bot_threshold: f32,
        wash_threshold: f32,
    ) -> Self {
        Self {
            min_resonance_for_boost: min_resonance,
            max_coherence_boost: max_boost,
            bot_manipulation_threshold: bot_threshold,
            wash_trading_threshold: wash_threshold,
            ..Default::default()
        }
    }
}

/// QOFSV enhancement result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QofsvEnhancement {
    /// Coherence boost factor (1.0 = no boost, >1.0 = boost)
    pub coherence_boost: f32,

    /// Amplitude enhancement for state vector
    pub amplitude_multiplier: f32,

    /// Confidence in the boost (0.0-1.0)
    pub confidence: f32,

    /// Reason for enhancement
    pub reason: String,
}

/// WHF validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhfValidation {
    /// Whether FRB validates or contradicts the WHF signal
    pub is_valid: bool,

    /// Confidence in validation (0.0-1.0)
    pub confidence: f32,

    /// Supporting evidence from FRB analysis
    pub evidence: Vec<String>,

    /// Risk flags detected
    pub risk_flags: Vec<String>,
}

/// QMAN enhancement result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QmanEnhancement {
    /// Confidence boost for capital flow prediction (additive %)
    pub confidence_boost: f32,

    /// Multi-scale validation score (0.0-1.0)
    pub multiscale_score: f32,

    /// Reason for enhancement
    pub reason: String,
}

/// Complete FRB integration result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrbIntegrationResult {
    /// Original FRB analysis
    pub frb_result: FrbResult,

    /// QOFSV enhancements
    pub qofsv_enhancement: QofsvEnhancement,

    /// WHF validation (if applicable)
    pub whf_validation: Option<WhfValidation>,

    /// QMAN enhancement (if applicable)
    pub qman_enhancement: Option<QmanEnhancement>,

    /// Overall integration confidence
    pub integration_confidence: f32,

    /// Detected anomalies/warnings
    pub warnings: Vec<String>,

    /// Timestamp
    pub timestamp_ms: u64,
}

/// FRB Integrator - Main integration engine
pub struct FrbIntegrator {
    config: FrbResonanceConfig,
}

impl FrbIntegrator {
    /// Create new integrator with default configuration
    pub fn new() -> Self {
        Self {
            config: FrbResonanceConfig::default(),
        }
    }

    /// Create with custom configuration for tuning
    pub fn with_config(config: FrbResonanceConfig) -> Self {
        Self { config }
    }

    /// Calculate QOFSV coherence boost from FRB resonance
    ///
    /// Maps FRB resonance score to amplitude boost for QOFSV StateVector.
    /// Higher resonance = stronger multi-scale confirmation = higher boost.
    ///
    /// # Algorithm
    ///
    /// ```text
    /// if resonance >= min_threshold:
    ///     boost = 1.0 + (resonance - threshold) * scale_factor
    ///     boost = clamp(boost, 1.0, max_boost)
    /// else:
    ///     boost = 1.0 (no boost)
    /// ```
    pub fn calculate_qofsv_boost(&self, frb_result: &FrbResult) -> QofsvEnhancement {
        let resonance = frb_result.resonance_score;

        // Check if resonance is strong enough for boost
        if resonance < self.config.min_resonance_for_boost {
            return QofsvEnhancement {
                coherence_boost: 1.0,
                amplitude_multiplier: 1.0,
                confidence: 0.0,
                reason: format!(
                    "Resonance {:.3} below threshold {:.3}",
                    resonance, self.config.min_resonance_for_boost
                ),
            };
        }

        // Calculate boost factor (linear scaling with resonance)
        let boost_range = self.config.max_coherence_boost - 1.0;
        let resonance_range = 1.0 - self.config.min_resonance_for_boost;
        let normalized_resonance =
            (resonance - self.config.min_resonance_for_boost) / resonance_range;

        let coherence_boost = 1.0 + (normalized_resonance * boost_range);
        let clamped_boost = coherence_boost.clamp(1.0, self.config.max_coherence_boost);

        // Amplitude multiplier based on trend likelihood
        let amplitude_multiplier = 1.0 + (frb_result.trend_likelihood * 0.3);

        // Confidence based on coherence map strength
        let avg_coherence = frb_result.coherence_map.iter().sum::<f32>() / 3.0;
        let confidence = (avg_coherence * 0.6 + frb_result.trend_likelihood * 0.4).clamp(0.0, 1.0);

        QofsvEnhancement {
            coherence_boost: clamped_boost,
            amplitude_multiplier,
            confidence,
            reason: format!(
                "Resonance {:.3} → boost {:.2}x (trend likelihood: {:.3})",
                resonance, clamped_boost, frb_result.trend_likelihood
            ),
        }
    }

    /// Validate WHF signal against FRB analysis
    ///
    /// Cross-checks WHF signals (wash trading, bot manipulation) with FRB band patterns
    /// to reduce false positives and confirm signal validity.
    pub fn validate_whf_signal(
        &self,
        frb_result: &FrbResult,
        whf_signal: &WhfSignal,
    ) -> WhfValidation {
        let mut evidence = Vec::new();
        let mut risk_flags = Vec::new();
        let mut is_valid = true;

        match whf_signal.signal_type {
            WhfSignalType::WashTrading => {
                // FRB should show high short-band activity with low buyer count
                let short_band = &frb_result.band_profiles[0];

                // Check for circular trading pattern (low buyers, high volume)
                if short_band.buyers < self.config.min_organic_buyers {
                    evidence.push(format!(
                        "Low buyer count ({}) confirms wash trading",
                        short_band.buyers
                    ));
                } else {
                    risk_flags.push(format!(
                        "High buyer count ({}) contradicts wash trading signal",
                        short_band.buyers
                    ));
                    is_valid = false;
                }

                // Check for fake pump pattern
                if frb_result.signal == FrbSignal::ResFake {
                    evidence.push("FRB classifies as fake pump".to_string());
                } else if frb_result.signal == FrbSignal::ResContinue {
                    risk_flags.push(
                        "FRB shows organic continuation - potential false positive".to_string(),
                    );
                    is_valid = false;
                }
            }

            WhfSignalType::BotManipulation => {
                // FRB should show high resonance (periodic patterns)
                if frb_result.resonance_score >= self.config.bot_manipulation_threshold {
                    evidence.push(format!(
                        "High resonance {:.3} confirms bot activity",
                        frb_result.resonance_score
                    ));
                } else {
                    risk_flags.push(format!(
                        "Low resonance {:.3} contradicts bot signal",
                        frb_result.resonance_score
                    ));
                    is_valid = false;
                }

                // Check buyer count
                let short_band = &frb_result.band_profiles[0];
                if short_band.buyers < self.config.min_organic_buyers {
                    evidence.push(format!(
                        "Low buyer diversity ({}) supports bot hypothesis",
                        short_band.buyers
                    ));
                }
            }

            WhfSignalType::OrganicExpansion => {
                // FRB should show multi-scale resonance with good buyer diversity
                if frb_result.signal == FrbSignal::ResContinue {
                    evidence.push("FRB confirms organic continuation".to_string());
                } else if frb_result.signal == FrbSignal::ResFake {
                    risk_flags
                        .push("FRB signals fake pump - contradicts organic expansion".to_string());
                    is_valid = false;
                }

                // Check all bands are active (multi-scale)
                let all_bands_active = frb_result.band_profiles.iter().all(|b| b.is_significant());

                if all_bands_active {
                    evidence.push("All FRB bands active - confirms multi-scale growth".to_string());
                } else {
                    risk_flags.push("Not all bands active - may be premature signal".to_string());
                }
            }

            WhfSignalType::TrendDecay => {
                // Check for declining band amplitudes
                let short_amp = frb_result.band_profiles[0].amplitude;
                let medium_amp = frb_result.band_profiles[1].amplitude;

                if medium_amp > short_amp * 1.2 {
                    evidence.push("Short band weaker than medium - confirms decay".to_string());
                } else {
                    risk_flags.push(
                        "Short band still strong - decay signal may be premature".to_string(),
                    );
                }
            }

            WhfSignalType::Hold => {
                // Nothing to validate for hold signal
                evidence.push("No action signal - validation not applicable".to_string());
            }
        }

        // Calculate confidence based on evidence vs risk flags
        let total_signals = evidence.len() + risk_flags.len();
        let confidence = if total_signals > 0 {
            evidence.len() as f32 / total_signals as f32
        } else {
            0.5
        };

        WhfValidation {
            is_valid,
            confidence,
            evidence,
            risk_flags,
        }
    }

    /// Enhance QMAN signal with FRB multi-scale analysis
    ///
    /// Uses FRB band profiles to increase confidence in QMAN capital flow predictions
    /// when multi-scale patterns align with predicted flows.
    pub fn enhance_qman_signal(
        &self,
        frb_result: &FrbResult,
        qman_signal: &QmanSignal,
    ) -> QmanEnhancement {
        let mut confidence_boost = 0.0;
        let mut reason_parts = Vec::new();

        match qman_signal.signal {
            TradingSignal::PrepareSecondWave => {
                // Check if FRB shows re-accumulation (increasing bands)
                if frb_result.signal == FrbSignal::ResTransition
                    || frb_result.signal == FrbSignal::ResContinue
                {
                    confidence_boost += 0.15;
                    reason_parts.push("FRB supports re-accumulation");
                }

                // Check if buyers are increasing
                if frb_result.band_profiles[0].buyers >= self.config.min_organic_buyers {
                    confidence_boost += 0.10;
                    reason_parts.push("Good buyer diversity");
                }
            }

            TradingSignal::AllInMainTrend => {
                // Check for strong resonance (coordinated flow)
                if frb_result.resonance_score > 0.6 {
                    confidence_boost += 0.20;
                    reason_parts.push("Strong multi-scale resonance");
                }

                // All bands should be active
                let all_active = frb_result.band_profiles.iter().all(|b| b.is_significant());
                if all_active {
                    confidence_boost += 0.15;
                    reason_parts.push("All bands active");
                }
            }

            TradingSignal::ExitNow => {
                // Check for declining bands or fake signals
                if frb_result.signal == FrbSignal::ResFake {
                    confidence_boost += 0.20;
                    reason_parts.push("FRB detects fake pump");
                }

                // Check for low buyer count (distribution to few wallets)
                if frb_result.band_profiles[0].buyers < self.config.min_organic_buyers {
                    confidence_boost += 0.10;
                    reason_parts.push("Low buyer count supports exit");
                }
            }

            TradingSignal::Hold => {
                // No enhancement for hold
                reason_parts.push("Hold signal - no enhancement");
            }
        }

        // Calculate multi-scale score based on band coherence
        let multiscale_score = frb_result.coherence_map.iter().sum::<f32>() / 3.0;

        let reason = if reason_parts.is_empty() {
            "No FRB enhancement applicable".to_string()
        } else {
            reason_parts.join(", ")
        };

        QmanEnhancement {
            confidence_boost,
            multiscale_score,
            reason,
        }
    }

    /// Perform full integration of FRB with QOFSV, WHF, and QMAN
    ///
    /// This is the main API for complete signal composition.
    pub fn integrate(
        &self,
        frb_result: FrbResult,
        whf_signal: Option<&WhfSignal>,
        qman_signal: Option<&QmanSignal>,
    ) -> FrbIntegrationResult {
        let mut warnings = Vec::new();

        // Always calculate QOFSV enhancement
        let qofsv_enhancement = self.calculate_qofsv_boost(&frb_result);

        // Optionally validate WHF signal
        let whf_validation = whf_signal.map(|signal| {
            let validation = self.validate_whf_signal(&frb_result, signal);

            // Add warnings for invalid signals
            if !validation.is_valid {
                warnings.push(format!(
                    "WHF signal {:?} validation failed: {}",
                    signal.signal_type,
                    validation.risk_flags.join("; ")
                ));
            }

            validation
        });

        // Optionally enhance QMAN signal
        let qman_enhancement =
            qman_signal.map(|signal| self.enhance_qman_signal(&frb_result, signal));

        // Calculate overall integration confidence
        let integration_confidence = self.calculate_integration_confidence(
            &frb_result,
            &qofsv_enhancement,
            whf_validation.as_ref(),
            qman_enhancement.as_ref(),
        );

        // Add warnings for weak signals
        if integration_confidence < 0.4 {
            warnings.push("Low integration confidence - signals may be unreliable".to_string());
        }

        let timestamp_ms = frb_result.timestamp;

        FrbIntegrationResult {
            frb_result,
            qofsv_enhancement,
            whf_validation,
            qman_enhancement,
            integration_confidence,
            warnings,
            timestamp_ms,
        }
    }

    /// Calculate overall integration confidence
    fn calculate_integration_confidence(
        &self,
        frb_result: &FrbResult,
        qofsv: &QofsvEnhancement,
        whf: Option<&WhfValidation>,
        qman: Option<&QmanEnhancement>,
    ) -> f32 {
        let mut confidence = frb_result.resonance_score * 0.3;

        // Add QOFSV contribution
        confidence += qofsv.confidence * 0.3;

        // Add WHF contribution if available
        if let Some(whf_val) = whf {
            confidence += whf_val.confidence * 0.2;
        } else {
            confidence += 0.1; // Neutral contribution if not available
        }

        // Add QMAN contribution if available
        if let Some(qman_enh) = qman {
            confidence += qman_enh.multiscale_score * 0.2;
        } else {
            confidence += 0.1; // Neutral contribution if not available
        }

        confidence.clamp(0.0, 1.0)
    }

    /// Update configuration for runtime tuning
    pub fn update_config(&mut self, config: FrbResonanceConfig) {
        self.config = config;
    }

    /// Get current configuration
    pub fn config(&self) -> &FrbResonanceConfig {
        &self.config
    }
}

impl Default for FrbIntegrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chaos::field_analysis::HarmonicFieldAnalysis;
    use crate::chaos::whf_signals::{FlowMetrics, WhfSignal, WhfSignalType};
    use crate::signals::frb::{BandProfile, FrbResult, FrbSignal};

    fn create_test_frb_result(
        resonance: f32,
        trend_likelihood: f32,
        signal: FrbSignal,
        short_buyers: u32,
    ) -> FrbResult {
        let bands = [
            BandProfile {
                amplitude: 100.0,
                buyers: short_buyers,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 300.0,
                buyers: short_buyers * 2,
                volatility: 7.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.8),
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 800.0,
                buyers: short_buyers * 3,
                volatility: 10.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.5),
                transaction_count: 200,
            },
        ];

        FrbResult {
            band_profiles: bands,
            resonance_score: resonance,
            coherence_map: [0.8, 0.7, 0.6],
            trend_likelihood,
            signal,
            timestamp: 1000,
        }
    }

    fn create_test_whf_signal(signal_type: WhfSignalType) -> WhfSignal {
        let analysis = HarmonicFieldAnalysis {
            curl: 0.5,
            divergence: 0.3,
            resonance_score: 0.6,
            timestamp_ms: 1000,
        };

        let flow_metrics = FlowMetrics {
            buy_volume: 100.0,
            sell_volume: 50.0,
            net_flow: 50.0,
            wallet_count: 10,
            volume_ratio: 0.67,
        };

        WhfSignal {
            signal_type,
            confidence: 0.8,
            trigger_level: 0.7,
            timestamp_ms: 1000,
            harmonic_analysis: analysis,
            flow_metrics,
            reason: "Test signal".to_string(),
        }
    }

    #[test]
    fn test_qofsv_boost_calculation() {
        let integrator = FrbIntegrator::new();

        // High resonance should provide boost
        let frb_high = create_test_frb_result(0.8, 0.7, FrbSignal::ResContinue, 10);
        let boost_high = integrator.calculate_qofsv_boost(&frb_high);

        assert!(boost_high.coherence_boost > 1.0);
        assert!(boost_high.amplitude_multiplier > 1.0);
        assert!(boost_high.confidence > 0.5);

        // Low resonance should not boost
        let frb_low = create_test_frb_result(0.3, 0.2, FrbSignal::ResHold, 5);
        let boost_low = integrator.calculate_qofsv_boost(&frb_low);

        assert_eq!(boost_low.coherence_boost, 1.0);
        assert_eq!(boost_low.confidence, 0.0);
    }

    #[test]
    fn test_whf_wash_trading_validation() {
        let integrator = FrbIntegrator::new();

        // FRB with low buyers + fake signal should validate wash trading
        let frb = create_test_frb_result(0.6, 0.3, FrbSignal::ResFake, 2);
        let whf = create_test_whf_signal(WhfSignalType::WashTrading);

        let validation = integrator.validate_whf_signal(&frb, &whf);

        assert!(validation.is_valid);
        assert!(!validation.evidence.is_empty());
        assert!(validation.confidence > 0.5);
    }

    #[test]
    fn test_whf_bot_manipulation_validation() {
        let integrator = FrbIntegrator::new();

        // High resonance should validate bot manipulation
        let frb = create_test_frb_result(0.8, 0.5, FrbSignal::ResTransition, 3);
        let whf = create_test_whf_signal(WhfSignalType::BotManipulation);

        let validation = integrator.validate_whf_signal(&frb, &whf);

        assert!(validation.is_valid);
        assert!(!validation.evidence.is_empty());
    }

    #[test]
    fn test_whf_organic_expansion_validation() {
        let integrator = FrbIntegrator::new();

        // Multi-scale with good buyers should validate organic
        let frb = create_test_frb_result(0.7, 0.8, FrbSignal::ResContinue, 10);
        let whf = create_test_whf_signal(WhfSignalType::OrganicExpansion);

        let validation = integrator.validate_whf_signal(&frb, &whf);

        assert!(validation.is_valid);
        assert!(!validation.evidence.is_empty());
    }

    #[test]
    fn test_false_positive_detection() {
        let integrator = FrbIntegrator::new();

        // Contradictory signals: WHF says organic but FRB shows fake
        let frb = create_test_frb_result(0.3, 0.2, FrbSignal::ResFake, 2);
        let whf = create_test_whf_signal(WhfSignalType::OrganicExpansion);

        let validation = integrator.validate_whf_signal(&frb, &whf);

        assert!(!validation.is_valid);
        assert!(!validation.risk_flags.is_empty());
    }

    #[test]
    fn test_full_integration() {
        let integrator = FrbIntegrator::new();

        let frb = create_test_frb_result(0.75, 0.7, FrbSignal::ResContinue, 8);
        let whf = create_test_whf_signal(WhfSignalType::OrganicExpansion);

        let result = integrator.integrate(frb, Some(&whf), None);

        assert!(result.qofsv_enhancement.coherence_boost > 1.0);
        assert!(result.whf_validation.is_some());
        assert!(result.integration_confidence > 0.5);

        if let Some(whf_val) = result.whf_validation {
            assert!(whf_val.is_valid);
        }
    }

    #[test]
    fn test_config_tuning() {
        let custom_config = FrbResonanceConfig::with_thresholds(
            0.6, // min_resonance
            2.0, // max_boost
            0.8, // bot_threshold
            0.7, // wash_threshold
        );

        let integrator = FrbIntegrator::with_config(custom_config);

        assert_eq!(integrator.config().min_resonance_for_boost, 0.6);
        assert_eq!(integrator.config().max_coherence_boost, 2.0);
    }

    #[test]
    fn test_integration_confidence_scaling() {
        let integrator = FrbIntegrator::new();

        // Strong signal
        let frb_strong = create_test_frb_result(0.85, 0.9, FrbSignal::ResContinue, 12);
        let result_strong = integrator.integrate(frb_strong, None, None);

        // Weak signal
        let frb_weak = create_test_frb_result(0.3, 0.2, FrbSignal::ResHold, 3);
        let result_weak = integrator.integrate(frb_weak, None, None);

        assert!(result_strong.integration_confidence > result_weak.integration_confidence);
    }
}
