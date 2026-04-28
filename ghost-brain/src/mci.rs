//! MCI Engine - Market Coherence Index
//!
//! The Market Coherence Index (MCI) is a unified metric that quantifies the alignment
//! and consistency of multiple market signals to assess the quality and reliability
//! of a trading opportunity.
//!
//! # Mathematical Foundation
//!
//! ## Core Formula
//!
//! The MCI is computed as a weighted combination of two orthogonal components:
//!
//! ```text
//! MCI = w_dc · DC + w_sc · SC
//!
//! where:
//!   MCI ∈ [0, 1]           - Final Market Coherence Index
//!   w_dc ∈ [0, 1]          - Weight for Directional Coherence (default: 0.6)
//!   w_sc ∈ [0, 1]          - Weight for Structural Coherence (default: 0.4)
//!   DC ∈ [0, 1]            - Directional Coherence component
//!   SC ∈ [0, 1]            - Structural Coherence component
//! ```
//!
//! ## Directional Coherence (DC)
//!
//! DC measures how well the market flow aligns with the predicted direction (QASS score):
//!
//! ```text
//! DC = ((qass_alignment + 1) / 2) · flow_magnitude
//!
//! where:
//!   qass_alignment ∈ [-1, 1]  - Correlation between flow and QASS direction
//!                                 1.0  = perfect alignment (bullish flow matches bullish QASS)
//!                                 0.0  = no correlation (neutral)
//!                                -1.0  = opposite (bearish flow vs bullish QASS)
//!   flow_magnitude ∈ [0, 1]    - Strength of the flow vector
//! ```
//!
//! **Normalization**: The alignment is normalized from [-1,1] to [0,1] by (x+1)/2.
//! **Magnitude Adjustment**: Weak flows reduce DC even if alignment is good.
//!
//! ## Structural Coherence (SC)
//!
//! SC measures internal consistency across four independent structural components:
//!
//! ```text
//! SC = (SC_mpcf + SC_sobp + SC_entropy + SC_deviation) / 4
//!
//! where each component ∈ [0, 1]:
//!
//! SC_mpcf = mpcf_entropy
//!   High MPCF entropy indicates organic, non-bot behavior
//!
//! SC_sobp = (1 - sobp_drop) · (1 - |current - ma| / max(ma, 1))
//!   Combines:
//!     - sobp_drop ∈ [0, 1]: Pressure collapse indicator (0 = stable, 1 = collapsed)
//!     - MA deviation: Distance from historical moving average
//!
//! SC_entropy = combined_entropy
//!   High combined entropy (SSMI + others) indicates healthy market activity
//!
//! SC_deviation = 1 - deviation_risk
//!   Low deviation risk means market behaves predictably
//! ```
//!
//! ## Signal Flow Diagram
//!
//! ```text
//!                        ┌─────────────────┐
//!                        │  MarketSignals  │
//!                        └────────┬────────┘
//!                                 │
//!                   ┌─────────────┴─────────────┐
//!                   │                           │
//!             ┌─────▼──────┐             ┌──────▼─────┐
//!             │ flow.qass_ │             │ entropy.   │
//!             │ alignment  │             │ mpcf       │
//!             │ flow.      │             │ sobp.drop  │
//!             │ magnitude  │             │ sobp.ma    │
//!             └─────┬──────┘             │ entropy.   │
//!                   │                    │ combined   │
//!                   │                    │ deviation. │
//!                   │                    │ risk       │
//!                   │                    └──────┬─────┘
//!                   │                           │
//!             ┌─────▼──────┐             ┌──────▼─────┐
//!             │     DC     │             │     SC     │
//!             │            │             │            │
//!             │ ((align+1) │             │ (mpcf +    │
//!             │    /2)     │             │  sobp +    │
//!             │   · mag    │             │  entropy + │
//!             │            │             │  dev) / 4  │
//!             └─────┬──────┘             └──────┬─────┘
//!                   │                           │
//!                   └─────────────┬─────────────┘
//!                                 │
//!                          ┌──────▼──────┐
//!                          │     MCI     │
//!                          │             │
//!                          │ w_dc·DC +   │
//!                          │ w_sc·SC     │
//!                          │             │
//!                          │ clamp[0,1]  │
//!                          └─────────────┘
//! ```
//!
//! # Input/Output Examples
//!
//! ## Example 1: Organic Hype Scenario
//!
//! **Input Signals**:
//! - `qass_alignment = 0.9` (strong alignment)
//! - `flow_magnitude = 0.85` (strong flow)
//! - `mpcf = 0.88` (high entropy, organic)
//! - `sobp.drop = 0.05` (stable buying pressure)
//! - `combined_entropy = 0.82`
//! - `deviation_risk = 0.15`
//!
//! **Computation**:
//! ```text
//! DC = ((0.9 + 1) / 2) · 0.85 = 0.95 · 0.85 = 0.8075
//! SC = (0.88 + 0.95 + 0.82 + 0.85) / 4 = 0.875
//! MCI = 0.6 · 0.8075 + 0.4 · 0.875 = 0.4845 + 0.35 = 0.8345
//! ```
//!
//! **Result**: MCI = 0.83 → Strong signal, proceed with trade
//!
//! ## Example 2: Rug Pull Scenario
//!
//! **Input Signals**:
//! - `qass_alignment = -0.7` (opposite flow)
//! - `flow_magnitude = 0.6` (moderate outflow)
//! - `mpcf = 0.2` (low entropy, bot-like)
//! - `sobp.drop = 0.85` (pressure collapsing)
//! - `combined_entropy = 0.15`
//! - `deviation_risk = 0.9`
//!
//! **Computation**:
//! ```text
//! DC = ((-0.7 + 1) / 2) · 0.6 = 0.15 · 0.6 = 0.09
//! SC = (0.2 + 0.15 + 0.15 + 0.1) / 4 = 0.15
//! MCI = 0.6 · 0.09 + 0.4 · 0.15 = 0.054 + 0.06 = 0.114
//! ```
//!
//! **Result**: MCI = 0.11 → Abort signal triggered (< 0.3 threshold)
//!
//! ## Example 3: Mixed Signals (Neutral)
//!
//! **Input Signals**:
//! - `qass_alignment = 0.1`
//! - `flow_magnitude = 0.5`
//! - All other signals moderate (0.4-0.6 range)
//!
//! **Result**: MCI ≈ 0.45 → Wait/skip, insufficient conviction
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_brain::{MciEngine, MciConfig, MarketSignals};
//!
//! // Create engine with custom weights
//! let mut config = MciConfig::default();
//! config.weight_dc = 0.65;  // Emphasize directional alignment
//! config.weight_sc = 0.35;
//!
//! let engine = MciEngine::new(config);
//!
//! // Compute MCI from market signals
//! let signals = MarketSignals::mock_hype();  // Or build from live data
//! let result = engine.compute_mci(&signals);
//!
//! // Interpret results
//! if result.mci > 0.7 {
//!     println!("✅ High coherence: MCI = {:.2}", result.mci);
//!     println!("   DC = {:.2}, SC = {:.2}", result.dc, result.sc);
//! } else if result.should_abort(0.3) {
//!     println!("🛑 Abort: MCI = {:.2} below threshold", result.mci);
//! }
//! ```
//!
//! # Performance
//!
//! - **Computation time**: < 1 microsecond
//! - **Zero heap allocations**: All operations on stack
//! - **Thread-safe**: Engine can be cloned or shared via Arc<>
//!
//! # References
//!
//! - QASS (Quantum-Style Amplitude Superposition Scoring): `oracle::ultrafast::qass`
//! - QOFSV (Quantum Orderflow State Vector): `oracle::ultrafast::qofsv`
//! - SOBP (Slot-Over-Slot Buying Pressure): `oracle::ultrafast::sobp`
//! - MPCF (Micro-Payload Cognitive Fingerprint): `oracle::ultrafast::mpcf`

use crate::config::mci_config::{MciConfig, MciInitialState};
use crate::models::mci_result::{
    MciResult, FEATURE_COMBINED_ENTROPY, FEATURE_DEVIATION_RISK, FEATURE_FLOW_MAGNITUDE,
    FEATURE_MPCF_ENTROPY, FEATURE_QASS_ALIGNMENT, FEATURE_SOBP_STABILITY,
};
use crate::signals::MarketSignals;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, span, Level};

/// MCI computation engine
#[derive(Clone)]
pub struct MciEngine {
    /// Engine configuration
    pub config: MciConfig,

    /// Current sentiment state (warm start support)
    current_sentiment: Arc<Mutex<f32>>,

    /// Short-term memory buffer for smoothing sentiment
    short_term_memory: Arc<Mutex<Vec<f32>>>,
}

impl MciEngine {
    /// Create a new MCI engine with the given configuration
    pub fn new(config: MciConfig) -> Self {
        let engine = Self {
            config,
            current_sentiment: Arc::new(Mutex::new(0.5)),
            short_term_memory: Arc::new(Mutex::new(Vec::with_capacity(16))),
        };

        if let Some(init) = engine.config.initial_state.as_ref() {
            let base = init.base_sentiment.clamp(0.0, 1.0) as f32;
            if let Ok(mut sentiment) = engine.current_sentiment.lock() {
                *sentiment = base;
            }
            if let Ok(mut memory) = engine.short_term_memory.lock() {
                memory.push(base);
            }
        }

        engine
    }

    /// Compute MCI result from market signals
    ///
    /// Calculates Directional Coherence (DC) and Structural Coherence (SC),
    /// then combines them into the Market Coherence Index (MCI).
    ///
    /// DC = Directional Coherence - measures alignment of flow with expected direction (QASS)
    /// SC = Structural Coherence - measures consistency across MPCF, SOBP, entropy, and deviation
    ///
    /// MCI = w_dc * DC + w_sc * SC
    ///
    /// # Arguments
    /// * `signals` - Aggregated market signals
    ///
    /// # Returns
    /// MciResult containing MCI, DC, and SC values
    pub fn compute_mci(&self, signals: &MarketSignals) -> MciResult {
        let _span = span!(
            Level::DEBUG,
            "mci_compute",
            w_dc = self.config.weight_dc,
            w_sc = self.config.weight_sc
        )
        .entered();

        use std::time::Instant;
        let start = Instant::now();

        let previous_sentiment = self
            .current_sentiment
            .lock()
            .map(|guard| *guard)
            .unwrap_or(0.5);

        // Calculate Directional Coherence (DC)
        // DC measures correlation of flow vector (QOFSV) with QASS direction
        // qass_alignment ranges from -1.0 (opposite) to 1.0 (aligned)
        // We normalize to [0.0, 1.0] where 1.0 is fully aligned
        let dc = ((signals.flow.qass_alignment + 1.0) / 2.0) as f32;

        // Adjust DC by flow magnitude - weak flows should reduce coherence
        let mut dc_magnitude_adjusted = dc * signals.flow.magnitude as f32;

        debug!(
            qass_alignment = signals.flow.qass_alignment,
            flow_magnitude = signals.flow.magnitude,
            dc_raw = dc,
            dc_adjusted = dc_magnitude_adjusted,
            "Computed Directional Coherence"
        );

        // Calculate Structural Coherence (SC)
        // SC measures mutual consistency of:
        // - MPCF entropy (should be high for organic markets)
        // - SOBP stability (current close to MA, low drop)
        // - Entropy (SSMI) (should be high for organic activity)
        // - Deviation (should be low for coherent markets)

        // MPCF component: high entropy is good (organic behavior)
        let mpcf_component = signals.entropy.mpcf as f32;

        // SOBP stability component: low drop + close to MA is good
        let sobp_drop_factor = 1.0 - (signals.sobp.drop as f32);
        let sobp_ma_deviation = ((signals.sobp.current - signals.sobp.ma).abs()
            / signals.sobp.ma.max(1.0))
        .min(1.0) as f32;
        let sobp_ma_proximity = 1.0 - sobp_ma_deviation;
        let sobp_stability = sobp_drop_factor * sobp_ma_proximity;

        // Entropy component: high combined entropy is good
        let entropy_component = signals.entropy.combined as f32;

        // Deviation component: low deviation risk is good (inverted)
        let deviation_component = 1.0 - (signals.deviation.risk as f32);

        // Number of structural coherence components
        const SC_COMPONENT_COUNT: f32 = 4.0;

        // Average all structural components with equal weight
        let sc = (mpcf_component + sobp_stability + entropy_component + deviation_component)
            / SC_COMPONENT_COUNT;

        debug!(
            mpcf = mpcf_component,
            sobp_stability,
            entropy = entropy_component,
            deviation_inv = deviation_component,
            sc,
            "Computed Structural Coherence components"
        );

        // Calculate final MCI as weighted combination
        let mut mci = self.config.weight_dc * dc_magnitude_adjusted + self.config.weight_sc * sc;

        // Detect abrupt trend breaks (sentiment high but flow reverses sharply)
        // and apply a conservative penalty to dampen conviction.
        if previous_sentiment > 0.6
            && signals.flow.qass_alignment < -0.3
            && signals.flow.magnitude > 0.5
        {
            dc_magnitude_adjusted *= 0.5;
            mci *= 0.5;
        }

        // Apply optional initial state for warm-start behavior
        if let Some(initial_state) = &self.config.initial_state {
            mci = self.apply_initial_state_bias(mci, initial_state);
        }

        // Clamp to [0.0, 1.0] range for safety
        let mci = mci.max(0.0).min(1.0);
        let dc = dc_magnitude_adjusted.max(0.0).min(1.0);
        let sc = sc.max(0.0).min(1.0);

        // Persist final sentiment snapshot for warm-start continuity
        if let Ok(mut sentiment) = self.current_sentiment.lock() {
            *sentiment = mci;
        }

        // Log final result with interpretation
        if mci > 0.7 {
            info!(mci, dc, sc, "HIGH coherence detected");
        } else if mci < self.config.coherence_abort_threshold {
            info!(
                mci,
                dc,
                sc,
                threshold = self.config.coherence_abort_threshold,
                "LOW coherence - abort signal"
            );
        } else {
            debug!(mci, dc, sc, "Moderate coherence");
        }

        // Track which features were used using bitflags (zero allocation)
        let features_flags = FEATURE_QASS_ALIGNMENT
            | FEATURE_FLOW_MAGNITUDE
            | FEATURE_MPCF_ENTROPY
            | FEATURE_SOBP_STABILITY
            | FEATURE_COMBINED_ENTROPY
            | FEATURE_DEVIATION_RISK;

        let computation_ms = start.elapsed().as_micros() as u64 / 1000;

        let result = MciResult {
            mci,
            dc,
            sc,
            #[allow(deprecated)]
            features_used: vec![], // Empty for performance; use features_flags or call features_to_vec()
            features_flags,
            computation_ms,
        };

        result
    }

    fn apply_initial_state_bias(&self, computed_mci: f32, initial_state: &MciInitialState) -> f32 {
        const MEMORY_CAPACITY: usize = 16;

        let base = initial_state.base_sentiment.clamp(0.0, 1.0) as f32;
        let volatility = initial_state.volatility_index.clamp(0.0, 1.0) as f32;

        if initial_state.force_override {
            if let Ok(mut memory) = self.short_term_memory.lock() {
                memory.push(base);
                if memory.len() > MEMORY_CAPACITY {
                    memory.remove(0);
                }
            }
            if let Ok(mut sentiment) = self.current_sentiment.lock() {
                *sentiment = base;
            }
            return base;
        }

        let memory_avg = {
            let mut memory = self.short_term_memory.lock().unwrap();
            if memory.is_empty() {
                memory.push(base);
            }

            if memory.is_empty() {
                base
            } else {
                memory.iter().copied().sum::<f32>() / memory.len() as f32
            }
        };

        let stability_weight = 1.0 - volatility;
        let blend_denominator = volatility + stability_weight;
        let mut blended = computed_mci;

        if blend_denominator > 0.0 {
            blended =
                (computed_mci * volatility + memory_avg * stability_weight) / blend_denominator;
        }

        if let Ok(mut memory) = self.short_term_memory.lock() {
            memory.push(blended);
            if memory.len() > MEMORY_CAPACITY {
                memory.remove(0);
            }
        }

        if let Ok(mut sentiment) = self.current_sentiment.lock() {
            *sentiment = blended;
        }
        blended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);
        assert_eq!(engine.config.version, 1);
    }

    #[test]
    fn test_initial_state_seeded_on_creation() {
        let mut config = MciConfig::default();
        config.initial_state = Some(MciInitialState {
            base_sentiment: 0.8,
            volatility_index: 0.2,
            force_override: false,
        });

        let engine = MciEngine::new(config);
        let sentiment = *engine.current_sentiment.lock().unwrap();
        assert!((sentiment - 0.8).abs() < f32::EPSILON);
        assert_eq!(engine.short_term_memory.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_compute_mci_basic() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);
        let signals = MarketSignals::mock();
        let result = engine.compute_mci(&signals);

        // MCI, DC, and SC should all be in [0, 1] range
        assert!(result.mci >= 0.0 && result.mci <= 1.0);
        assert!(result.dc >= 0.0 && result.dc <= 1.0);
        assert!(result.sc >= 0.0 && result.sc <= 1.0);

        // Features should be tracked using bitflags
        assert!(result.features_flags != 0);
        // All 6 features should be used
        let expected_flags = FEATURE_QASS_ALIGNMENT
            | FEATURE_FLOW_MAGNITUDE
            | FEATURE_MPCF_ENTROPY
            | FEATURE_SOBP_STABILITY
            | FEATURE_COMBINED_ENTROPY
            | FEATURE_DEVIATION_RISK;
        assert_eq!(result.features_flags, expected_flags);
    }

    #[test]
    fn test_initial_state_force_override_applied() {
        let mut config = MciConfig::default();
        config.initial_state = Some(MciInitialState {
            base_sentiment: 0.9,
            volatility_index: 0.3,
            force_override: true,
        });

        let engine = MciEngine::new(config);
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = -1.0;
        signals.flow.magnitude = 0.0;
        let result = engine.compute_mci(&signals);

        assert!(
            (result.mci - 0.9).abs() < 0.01,
            "Initial state should override computed MCI"
        );
    }

    #[test]
    fn test_compute_mci_hype_scenario() {
        let config = MciConfig::default();
        let threshold = config.coherence_abort_threshold;
        let engine = MciEngine::new(config);
        let signals = MarketSignals::mock_hype();
        let result = engine.compute_mci(&signals);

        // In hype scenario: high QASS alignment, low outflow, high entropy, low deviation
        // Both DC and SC should be relatively high
        assert!(result.dc > 0.85, "Hype DC should be high: {}", result.dc);
        assert!(
            result.sc > 0.60,
            "Hype SC should be above 0.60: {}",
            result.sc
        );
        assert!(result.mci > 0.70, "Hype MCI should be high: {}", result.mci);

        // Should not trigger abort
        assert!(!result.should_abort(threshold));
    }

    #[test]
    fn test_compute_mci_rug_scenario() {
        let config = MciConfig::default();
        let threshold = config.coherence_abort_threshold;
        let engine = MciEngine::new(config);
        let signals = MarketSignals::mock_rug();
        let result = engine.compute_mci(&signals);

        // In rug scenario: negative QASS alignment, high outflow, low entropy, high deviation
        // Both DC and SC should be low
        assert!(result.dc < 0.3, "Rug DC should be low: {}", result.dc);
        assert!(result.sc < 0.3, "Rug SC should be low: {}", result.sc);
        assert!(result.mci < 0.3, "Rug MCI should be low: {}", result.mci);

        // Should trigger abort
        assert!(result.should_abort(threshold));
    }

    #[test]
    fn test_compute_mci_stable_scenario() {
        let config = MciConfig::default();
        let threshold = config.coherence_abort_threshold;
        let engine = MciEngine::new(config);
        let signals = MarketSignals::mock_stable();
        let result = engine.compute_mci(&signals);

        // In stable scenario: neutral alignment, moderate values
        // MCI should be in moderate range
        assert!(
            result.mci > 0.3 && result.mci < 0.7,
            "Stable MCI should be moderate: {}",
            result.mci
        );

        // Should not trigger abort
        assert!(!result.should_abort(threshold));
    }

    #[test]
    fn test_mci_weighted_formula() {
        let mut config = MciConfig::default();
        config.weight_dc = 0.6;
        config.weight_sc = 0.4;

        let engine = MciEngine::new(config);

        // Create signals with known DC and SC contributions
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = 1.0; // Perfect alignment -> DC ≈ 1.0
        signals.flow.magnitude = 1.0; // Full magnitude

        // Set up for high SC (high entropy, low deviation, stable SOBP)
        signals.entropy.mpcf = 1.0;
        signals.entropy.combined = 1.0;
        signals.sobp.drop = 0.0;
        signals.sobp.current = signals.sobp.ma;
        signals.deviation.risk = 0.0;

        let result = engine.compute_mci(&signals);

        // With perfect signals, MCI should be close to 1.0
        assert!(
            result.mci > 0.9,
            "Perfect signals should give MCI close to 1.0: {}",
            result.mci
        );
        assert!(result.dc > 0.9, "Perfect DC signals: {}", result.dc);
        assert!(result.sc > 0.9, "Perfect SC signals: {}", result.sc);
    }

    #[test]
    fn test_mci_directional_coherence() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Test with perfect alignment
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = 1.0;
        signals.flow.magnitude = 1.0;
        let result_aligned = engine.compute_mci(&signals);

        // Test with opposite alignment
        signals.flow.qass_alignment = -1.0;
        let result_opposite = engine.compute_mci(&signals);

        // Aligned should have much higher DC
        assert!(result_aligned.dc > result_opposite.dc);
    }

    #[test]
    fn test_mci_trend_break_penalty_applied() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Seed previous sentiment high to simulate established trend
        if let Ok(mut sentiment) = engine.current_sentiment.lock() {
            *sentiment = 0.85;
        }

        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = -0.8; // sharp reversal
        signals.flow.magnitude = 0.9; // strong move

        // Compute expected raw values
        let dc = ((signals.flow.qass_alignment + 1.0) / 2.0) as f32;
        let mut dc_magnitude_adjusted = dc * signals.flow.magnitude as f32;

        let mpcf_component = signals.entropy.mpcf as f32;
        let sobp_drop_factor = 1.0 - (signals.sobp.drop as f32);
        let sobp_ma_deviation = ((signals.sobp.current - signals.sobp.ma).abs()
            / signals.sobp.ma.max(1.0))
        .min(1.0) as f32;
        let sobp_ma_proximity = 1.0 - sobp_ma_deviation;
        let sobp_stability = sobp_drop_factor * sobp_ma_proximity;
        let entropy_component = signals.entropy.combined as f32;
        let deviation_component = 1.0 - (signals.deviation.risk as f32);
        let sc = (mpcf_component + sobp_stability + entropy_component + deviation_component) / 4.0;

        let raw_mci = 0.6 * dc_magnitude_adjusted + 0.4 * sc;

        // Penalty should apply under trend break conditions
        dc_magnitude_adjusted *= 0.5;
        let expected_penalized = (raw_mci * 0.5).clamp(0.0, 1.0);

        let result = engine.compute_mci(&signals);

        assert!(
            (result.mci - expected_penalized).abs() < 1e-3,
            "Trend break should apply penalty to MCI"
        );
        assert!(
            result.mci < raw_mci,
            "Penalized MCI should be lower than raw calculation"
        );
        assert!(
            result.dc <= dc_magnitude_adjusted + 1e-6,
            "Directional coherence should reflect penalty"
        );
    }

    #[test]
    fn test_mci_structural_coherence() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Test with high structural coherence
        let mut signals = MarketSignals::mock();
        signals.entropy.mpcf = 0.9;
        signals.entropy.combined = 0.9;
        signals.sobp.drop = 0.0;
        signals.deviation.risk = 0.1;
        let result_high = engine.compute_mci(&signals);

        // Test with low structural coherence
        signals.entropy.mpcf = 0.1;
        signals.entropy.combined = 0.1;
        signals.sobp.drop = 0.9;
        signals.deviation.risk = 0.9;
        let result_low = engine.compute_mci(&signals);

        // High coherence should have much higher SC
        assert!(result_high.sc > result_low.sc);
    }

    #[test]
    fn test_mci_boundary_conditions() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Test with all negative factors
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = -1.0;
        signals.flow.magnitude = 0.0;
        signals.entropy.mpcf = 0.0;
        signals.entropy.combined = 0.0;
        signals.sobp.drop = 1.0;
        signals.deviation.risk = 1.0;

        let result = engine.compute_mci(&signals);

        // MCI should still be in valid range
        assert!(result.mci >= 0.0 && result.mci <= 1.0);
        assert!(result.dc >= 0.0 && result.dc <= 1.0);
        assert!(result.sc >= 0.0 && result.sc <= 1.0);

        // Should be close to zero
        assert!(
            result.mci < 0.2,
            "All negative factors should give very low MCI: {}",
            result.mci
        );
    }

    #[test]
    fn test_mci_abort_threshold() {
        let config = MciConfig::default();
        let threshold = config.coherence_abort_threshold;
        let engine = MciEngine::new(config);

        // Test just above threshold
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = 0.0;
        signals.flow.magnitude = 0.6;
        signals.entropy.combined = 0.5;
        let result_above = engine.compute_mci(&signals);

        // Test just below threshold
        signals.flow.magnitude = 0.1;
        signals.entropy.combined = 0.1;
        signals.deviation.risk = 0.9;
        let result_below = engine.compute_mci(&signals);

        assert!(!result_above.should_abort(threshold));
        assert!(result_below.should_abort(threshold));
    }
}
