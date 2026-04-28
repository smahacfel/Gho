//! WHF Part 3: Signal Detector & Launcher API
//!
//! This module interprets Harmonic Field Analysis (WHF Part 2) results and Flowfield
//! (WHF Part 1) data into actionable trading signals for the launcher/bot system.
//!
//! ## Signal Classification
//!
//! Based on the combination of three harmonic indicators:
//!
//! 1. **Curl** (rotation/vorticity) - Wash trading detection
//! 2. **Divergence** (flow concentration) - Accumulation/distribution
//! 3. **Resonance** (periodic patterns) - Bot activity detection
//!
//! ## Signal Types
//!
//! - **ORGANIC_EXPANSION**: Natural growth, low bot activity, positive divergence
//! - **BOT_MANIPULATION**: High resonance with abnormal curl patterns
//! - **WASH_TRADING**: High curl with low net flow
//! - **TREND_DECAY**: Negative divergence with declining energy
//! - **HOLD**: No clear pattern or mixed signals
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │         WHF Part 3: Signal Detector (Launcher API)      │
//! │                                                         │
//! │  Input: HarmonicFieldAnalysis (Part 2)                 │
//! │         FlowVector (Part 1)                             │
//! │                                                         │
//! │  ┌──────────────┐   ┌──────────────┐   ┌─────────────┐│
//! │  │ Pattern      │   │ Confidence   │   │ Trigger     ││
//! │  │ Classifier   │   │ Scoring      │   │ Level Calc  ││
//! │  └──────────────┘   └──────────────┘   └─────────────┘│
//! │                                                         │
//! │  Output: WhfSignal                                      │
//! │          { signal_type, confidence, trigger_level }     │
//! └─────────────────────────────────────────────────────────┘
//! ```

use super::field_analysis::HarmonicFieldAnalysis;
use super::flowfield::FlowVector;
use serde::{Deserialize, Serialize};

/// Trading signal type based on harmonic field analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhfSignalType {
    /// Organic market expansion with natural growth
    /// - Low bot activity (resonance < 0.3)
    /// - Positive divergence (> 0.2)
    /// - Low to moderate curl (< 0.4)
    #[serde(rename = "ORGANIC_EXPANSION")]
    OrganicExpansion,

    /// Bot manipulation detected
    /// - High resonance (> 0.7) indicating coordinated trading
    /// - May have elevated curl
    #[serde(rename = "BOT_MANIPULATION")]
    BotManipulation,

    /// Wash trading pattern detected
    /// - High curl (> 0.6) indicating circular flows
    /// - Near-zero net flow despite high volume
    #[serde(rename = "WASH_TRADING")]
    WashTrading,

    /// Market trend decay - distribution phase
    /// - Negative divergence (< -0.2)
    /// - Declining energy flow
    #[serde(rename = "TREND_DECAY")]
    TrendDecay,

    /// No clear signal - hold position
    /// - Mixed or weak indicators
    #[serde(rename = "HOLD")]
    Hold,
}

/// Complete signal with confidence and trigger levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhfSignal {
    /// Classified signal type
    pub signal_type: WhfSignalType,

    /// Confidence score (0.0 - 1.0)
    /// Higher values indicate stronger signal clarity
    pub confidence: f32,

    /// Trigger level for automated trading
    /// Range: 0.0 (no action) to 1.0 (maximum urgency)
    ///
    /// Interpretation:
    /// - < 0.3: Monitor only
    /// - 0.3 - 0.6: Prepare for action
    /// - 0.6 - 0.8: Execute with standard position sizing
    /// - > 0.8: Execute with increased position sizing
    pub trigger_level: f32,

    /// Timestamp of signal generation (milliseconds since epoch)
    pub timestamp_ms: u64,

    /// Underlying harmonic analysis data
    pub harmonic_analysis: HarmonicFieldAnalysis,

    /// Flow metrics
    pub flow_metrics: FlowMetrics,

    /// Additional context for the signal
    pub reason: String,
}

/// Flow metrics extracted from FlowVector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowMetrics {
    /// Total buy volume
    pub buy_volume: f32,

    /// Total sell volume
    pub sell_volume: f32,

    /// Net flow (buy - sell)
    pub net_flow: f32,

    /// Number of unique wallets
    pub wallet_count: usize,

    /// Volume volatility (absolute deviation from mean)
    pub volume_ratio: f32,
}

impl FlowMetrics {
    /// Create flow metrics from FlowVector
    pub fn from_flow_vector(flow: &FlowVector) -> Self {
        let total_volume = flow.buy + flow.sell;
        let volume_ratio = if total_volume > 0.0 {
            flow.buy / total_volume
        } else {
            0.5
        };

        Self {
            buy_volume: flow.buy,
            sell_volume: flow.sell,
            net_flow: flow.net,
            wallet_count: flow.wallets,
            volume_ratio,
        }
    }
}

/// Configuration for signal detection thresholds
#[derive(Debug, Clone)]
pub struct WhfSignalConfig {
    /// Curl threshold for wash trading detection
    pub wash_trading_curl_threshold: f32,

    /// Resonance threshold for bot detection
    pub bot_resonance_threshold: f32,

    /// Divergence threshold for accumulation detection
    pub accumulation_divergence_threshold: f32,

    /// Divergence threshold for distribution detection
    pub distribution_divergence_threshold: f32,

    /// Curl threshold for organic trading
    pub organic_curl_threshold: f32,

    /// Resonance threshold for organic trading
    pub organic_resonance_threshold: f32,

    /// Minimum net flow for significant signal (absolute value)
    pub min_significant_net_flow: f32,

    /// Minimum wallet count for signal validity
    pub min_wallet_count: usize,
}

impl Default for WhfSignalConfig {
    fn default() -> Self {
        Self {
            wash_trading_curl_threshold: 0.6,
            bot_resonance_threshold: 0.7,
            accumulation_divergence_threshold: 0.2,
            distribution_divergence_threshold: -0.2,
            organic_curl_threshold: 0.4,
            organic_resonance_threshold: 0.3,
            min_significant_net_flow: 1.0,
            min_wallet_count: 3,
        }
    }
}

/// WHF Signal Detector - Converts harmonic analysis into trading signals
pub struct WhfSignalDetector {
    config: WhfSignalConfig,
}

impl WhfSignalDetector {
    /// Create a new signal detector with default configuration
    pub fn new() -> Self {
        Self {
            config: WhfSignalConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: WhfSignalConfig) -> Self {
        Self { config }
    }

    /// Detect trading signal from harmonic analysis and flow data
    ///
    /// # Arguments
    ///
    /// * `analysis` - Harmonic field analysis from WHF Part 2
    /// * `flow` - Flow vector from WHF Part 1
    ///
    /// # Returns
    ///
    /// Complete signal with type, confidence, and trigger level
    pub fn detect_signal(&self, analysis: &HarmonicFieldAnalysis, flow: &FlowVector) -> WhfSignal {
        let flow_metrics = FlowMetrics::from_flow_vector(flow);

        // Check minimum requirements
        if flow.wallets < self.config.min_wallet_count {
            return self.create_hold_signal(
                analysis,
                &flow_metrics,
                "Insufficient wallet activity",
            );
        }

        // Classify signal based on harmonic indicators
        let (signal_type, reason) = self.classify_pattern(analysis, &flow_metrics);

        // Calculate confidence based on indicator strength
        let confidence = self.calculate_confidence(analysis, &flow_metrics, &signal_type);

        // Calculate trigger level based on signal urgency
        let trigger_level =
            self.calculate_trigger_level(&signal_type, confidence, analysis, &flow_metrics);

        WhfSignal {
            signal_type,
            confidence,
            trigger_level,
            timestamp_ms: analysis.timestamp_ms,
            harmonic_analysis: analysis.clone(),
            flow_metrics,
            reason,
        }
    }

    /// Classify the market pattern based on harmonic indicators
    fn classify_pattern(
        &self,
        analysis: &HarmonicFieldAnalysis,
        flow: &FlowMetrics,
    ) -> (WhfSignalType, String) {
        // Priority 1: Wash Trading Detection (highest priority for risk)
        if self.is_wash_trading(analysis, flow) {
            return (
                WhfSignalType::WashTrading,
                format!(
                    "High curl ({:.2}) with near-zero net flow ({:.2})",
                    analysis.curl, flow.net_flow
                ),
            );
        }

        // Priority 2: Bot Manipulation Detection
        if self.is_bot_manipulation(analysis) {
            return (
                WhfSignalType::BotManipulation,
                format!(
                    "High resonance ({:.2}) indicating coordinated trading",
                    analysis.resonance_score
                ),
            );
        }

        // Priority 3: Trend Decay (distribution phase)
        if self.is_trend_decay(analysis, flow) {
            return (
                WhfSignalType::TrendDecay,
                format!(
                    "Negative divergence ({:.2}) with declining energy",
                    analysis.divergence
                ),
            );
        }

        // Priority 4: Organic Expansion (positive signal)
        if self.is_organic_expansion(analysis, flow) {
            return (
                WhfSignalType::OrganicExpansion,
                format!(
                    "Natural growth: divergence={:.2}, curl={:.2}, resonance={:.2}",
                    analysis.divergence, analysis.curl, analysis.resonance_score
                ),
            );
        }

        // Default: Hold
        (
            WhfSignalType::Hold,
            "Mixed or weak signals - no clear pattern".to_string(),
        )
    }

    /// Detect wash trading pattern
    fn is_wash_trading(&self, analysis: &HarmonicFieldAnalysis, flow: &FlowMetrics) -> bool {
        // High curl indicates circular trading
        let high_curl = analysis.curl >= self.config.wash_trading_curl_threshold;

        // Near-zero net flow despite high volume
        let total_volume = flow.buy_volume + flow.sell_volume;
        let low_net_flow = total_volume > 0.0 && flow.net_flow.abs() / total_volume < 0.1;

        high_curl && low_net_flow
    }

    /// Detect bot manipulation
    fn is_bot_manipulation(&self, analysis: &HarmonicFieldAnalysis) -> bool {
        // High resonance indicates periodic/coordinated activity
        analysis.resonance_score >= self.config.bot_resonance_threshold
    }

    /// Detect trend decay (distribution phase)
    fn is_trend_decay(&self, analysis: &HarmonicFieldAnalysis, flow: &FlowMetrics) -> bool {
        // Negative divergence indicates distribution
        let negative_divergence =
            analysis.divergence <= self.config.distribution_divergence_threshold;

        // Significant selling pressure
        let selling_pressure = flow.net_flow < -self.config.min_significant_net_flow;

        negative_divergence && selling_pressure
    }

    /// Detect organic expansion
    fn is_organic_expansion(&self, analysis: &HarmonicFieldAnalysis, flow: &FlowMetrics) -> bool {
        // Positive divergence indicates accumulation
        let positive_divergence =
            analysis.divergence >= self.config.accumulation_divergence_threshold;

        // Low curl indicates non-circular trading
        let low_curl = analysis.curl < self.config.organic_curl_threshold;

        // Low resonance indicates human-like trading
        let low_resonance = analysis.resonance_score < self.config.organic_resonance_threshold;

        // Significant buying pressure
        let buying_pressure = flow.net_flow > self.config.min_significant_net_flow;

        positive_divergence && low_curl && low_resonance && buying_pressure
    }

    /// Calculate confidence score for the signal
    fn calculate_confidence(
        &self,
        analysis: &HarmonicFieldAnalysis,
        flow: &FlowMetrics,
        signal_type: &WhfSignalType,
    ) -> f32 {
        match signal_type {
            WhfSignalType::WashTrading => {
                // Confidence based on curl strength
                let curl_strength = (analysis.curl - self.config.wash_trading_curl_threshold)
                    / (1.0 - self.config.wash_trading_curl_threshold);
                curl_strength.max(0.0).min(1.0)
            }
            WhfSignalType::BotManipulation => {
                // Confidence based on resonance strength
                let resonance_strength = (analysis.resonance_score
                    - self.config.bot_resonance_threshold)
                    / (1.0 - self.config.bot_resonance_threshold);
                resonance_strength.max(0.0).min(1.0)
            }
            WhfSignalType::TrendDecay => {
                // Confidence based on divergence magnitude and net flow
                let div_strength =
                    (self.config.distribution_divergence_threshold - analysis.divergence).abs()
                        / self.config.distribution_divergence_threshold.abs();
                let flow_strength =
                    flow.net_flow.abs() / (flow.buy_volume + flow.sell_volume + 0.01);
                ((div_strength + flow_strength) / 2.0).max(0.0).min(1.0)
            }
            WhfSignalType::OrganicExpansion => {
                // Multi-factor confidence calculation
                let div_strength = (analysis.divergence
                    - self.config.accumulation_divergence_threshold)
                    / (1.0 - self.config.accumulation_divergence_threshold);
                let curl_inverse = 1.0 - (analysis.curl / self.config.organic_curl_threshold);
                let resonance_inverse =
                    1.0 - (analysis.resonance_score / self.config.organic_resonance_threshold);

                // Average of positive indicators
                ((div_strength + curl_inverse + resonance_inverse) / 3.0)
                    .max(0.0)
                    .min(1.0)
            }
            WhfSignalType::Hold => 0.0,
        }
    }

    /// Calculate trigger level for automated trading
    fn calculate_trigger_level(
        &self,
        signal_type: &WhfSignalType,
        confidence: f32,
        analysis: &HarmonicFieldAnalysis,
        flow: &FlowMetrics,
    ) -> f32 {
        match signal_type {
            WhfSignalType::OrganicExpansion => {
                // Trigger level based on confidence and accumulation strength
                let base_level = confidence * 0.7;
                let divergence_boost =
                    (analysis.divergence - self.config.accumulation_divergence_threshold).max(0.0)
                        * 0.3;
                (base_level + divergence_boost).min(1.0)
            }
            WhfSignalType::TrendDecay => {
                // Negative trigger (exit signal) - high urgency for risk management
                let base_level = confidence * 0.8;
                let flow_urgency =
                    (flow.net_flow.abs() / (flow.buy_volume + flow.sell_volume + 0.01)).min(1.0)
                        * 0.2;
                (base_level + flow_urgency).min(1.0)
            }
            WhfSignalType::WashTrading => {
                // No trigger for wash trading - risk avoidance signal
                0.0
            }
            WhfSignalType::BotManipulation => {
                // Low trigger - caution signal
                confidence * 0.3
            }
            WhfSignalType::Hold => 0.0,
        }
    }

    /// Create a hold signal
    fn create_hold_signal(
        &self,
        analysis: &HarmonicFieldAnalysis,
        flow_metrics: &FlowMetrics,
        reason: &str,
    ) -> WhfSignal {
        WhfSignal {
            signal_type: WhfSignalType::Hold,
            confidence: 0.0,
            trigger_level: 0.0,
            timestamp_ms: analysis.timestamp_ms,
            harmonic_analysis: analysis.clone(),
            flow_metrics: flow_metrics.clone(),
            reason: reason.to_string(),
        }
    }
}

impl Default for WhfSignalDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_flow(buy: f32, sell: f32, wallets: usize) -> FlowVector {
        FlowVector::from_components(buy, sell, wallets)
    }

    fn create_test_analysis(curl: f32, divergence: f32, resonance: f32) -> HarmonicFieldAnalysis {
        HarmonicFieldAnalysis {
            curl,
            divergence,
            resonance_score: resonance,
            timestamp_ms: 1000000,
        }
    }

    #[test]
    fn test_organic_expansion_detection() {
        let detector = WhfSignalDetector::new();

        // Low curl, low resonance, positive divergence, positive net flow
        let analysis = create_test_analysis(0.2, 0.5, 0.2);
        let flow = create_test_flow(100.0, 50.0, 10);

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::OrganicExpansion);
        assert!(signal.confidence > 0.0);
        assert!(signal.trigger_level > 0.3);
    }

    #[test]
    fn test_wash_trading_detection() {
        let detector = WhfSignalDetector::new();

        // High curl, near-zero net flow
        let analysis = create_test_analysis(0.7, 0.0, 0.5);
        let flow = create_test_flow(100.0, 99.0, 5);

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::WashTrading);
        assert!(signal.confidence > 0.0);
        assert_eq!(signal.trigger_level, 0.0); // No trigger for wash trading
    }

    #[test]
    fn test_bot_manipulation_detection() {
        let detector = WhfSignalDetector::new();

        // High resonance
        let analysis = create_test_analysis(0.4, 0.1, 0.8);
        let flow = create_test_flow(50.0, 40.0, 8);

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::BotManipulation);
        assert!(signal.confidence > 0.0);
    }

    #[test]
    fn test_trend_decay_detection() {
        let detector = WhfSignalDetector::new();

        // Negative divergence, selling pressure
        let analysis = create_test_analysis(0.3, -0.4, 0.4);
        let flow = create_test_flow(30.0, 100.0, 12);

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::TrendDecay);
        assert!(signal.confidence > 0.0);
        assert!(signal.trigger_level > 0.5); // High urgency for exit
    }

    #[test]
    fn test_hold_signal_insufficient_wallets() {
        let detector = WhfSignalDetector::new();

        let analysis = create_test_analysis(0.2, 0.3, 0.2);
        let flow = create_test_flow(50.0, 30.0, 2); // Only 2 wallets

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::Hold);
        assert_eq!(signal.confidence, 0.0);
    }

    #[test]
    fn test_hold_signal_mixed_indicators() {
        let detector = WhfSignalDetector::new();

        // Mixed indicators - moderate everything
        let analysis = create_test_analysis(0.5, 0.1, 0.5);
        let flow = create_test_flow(50.0, 50.0, 10);

        let signal = detector.detect_signal(&analysis, &flow);

        assert_eq!(signal.signal_type, WhfSignalType::Hold);
    }

    #[test]
    fn test_confidence_scaling() {
        let detector = WhfSignalDetector::new();

        // Strong organic signal
        let analysis1 = create_test_analysis(0.1, 0.8, 0.1);
        let flow1 = create_test_flow(100.0, 20.0, 15);
        let signal1 = detector.detect_signal(&analysis1, &flow1);

        // Weak organic signal
        let analysis2 = create_test_analysis(0.3, 0.3, 0.25);
        let flow2 = create_test_flow(60.0, 50.0, 5);
        let signal2 = detector.detect_signal(&analysis2, &flow2);

        // Strong signal should have higher confidence
        if signal1.signal_type == WhfSignalType::OrganicExpansion
            && signal2.signal_type == WhfSignalType::OrganicExpansion
        {
            assert!(signal1.confidence > signal2.confidence);
        }
    }

    #[test]
    fn test_trigger_level_scaling() {
        let detector = WhfSignalDetector::new();

        // Strong accumulation
        let analysis = create_test_analysis(0.1, 0.7, 0.1);
        let flow = create_test_flow(150.0, 30.0, 20);

        let signal = detector.detect_signal(&analysis, &flow);

        if signal.signal_type == WhfSignalType::OrganicExpansion {
            assert!(
                signal.trigger_level > 0.6,
                "Strong signal should have high trigger level"
            );
        }
    }

    #[test]
    fn test_flow_metrics_calculation() {
        let flow = create_test_flow(80.0, 20.0, 5);
        let metrics = FlowMetrics::from_flow_vector(&flow);

        assert_eq!(metrics.buy_volume, 80.0);
        assert_eq!(metrics.sell_volume, 20.0);
        assert_eq!(metrics.net_flow, 60.0);
        assert_eq!(metrics.wallet_count, 5);
        assert!((metrics.volume_ratio - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_custom_config() {
        let config = WhfSignalConfig {
            wash_trading_curl_threshold: 0.5,
            bot_resonance_threshold: 0.6,
            accumulation_divergence_threshold: 0.3,
            distribution_divergence_threshold: -0.3,
            organic_curl_threshold: 0.3,
            organic_resonance_threshold: 0.25,
            min_significant_net_flow: 2.0,
            min_wallet_count: 5,
        };

        let detector = WhfSignalDetector::with_config(config);

        // Test with custom thresholds
        let analysis = create_test_analysis(0.55, 0.1, 0.5);
        let flow = create_test_flow(100.0, 98.0, 8);

        let signal = detector.detect_signal(&analysis, &flow);

        // Should detect wash trading with lower threshold
        assert_eq!(signal.signal_type, WhfSignalType::WashTrading);
    }
}
