//! Confidence Model
//!
//! This module implements a formal Confidence Model that quantifies the reliability
//! and trustworthiness of Oracle Brain scoring decisions. The confidence score
//! represents how certain the system is about its predictions based on the quality
//! and coherence of signals from all analytical modules.
//!
//! # Mathematical Foundation
//!
//! ## Confidence Score Formula
//!
//! The overall confidence C ∈ [0, 1] is computed as a weighted sum of module contributions:
//!
//! ```text
//! C = Σ(w_i · c_i) / Σ(w_i)
//!
//! where:
//!   C ∈ [0, 1]           - Overall confidence score
//!   w_i ∈ ℝ⁺            - Weight for module i
//!   c_i ∈ [0, 1]        - Normalized contribution from module i
//! ```
//!
//! ## Module Contributions
//!
//! Each module contributes to the overall confidence based on:
//! 1. **Signal Quality**: How clean and reliable the input data is
//! 2. **Signal Strength**: How strong the detected patterns are
//! 3. **Noise Level**: Inversely proportional to noise in the signal
//! 4. **Data Completeness**: Whether all required data is available
//!
//! ### SOBP (Slot-Over-Slot Buying Pressure)
//! - **Weight**: 12.0
//! - **Contribution**: Based on pressure stability and lack of sudden collapse
//! - **Formula**: `c_sobp = (1.0 - sobp_drop) · min(1.0, sobp_current / sobp_ma)`
//! - **Noise Sources**: Slot boundary artifacts, network latency variations
//!
//! ### MPCF (Micro-Payload Cognitive Fingerprint)
//! - **Weight**: 10.0
//! - **Contribution**: Based on entropy (high entropy = organic behavior)
//! - **Formula**: `c_mpcf = mpcf_entropy`
//! - **Noise Sources**: Transaction batching, MEV bundles
//!
//! ### IWIM (Inter-Wallet Interaction Matrix)
//! - **Weight**: 8.0
//! - **Contribution**: Based on network coherence and non-bot patterns
//! - **Formula**: `c_iwim = network_coherence · (1.0 - bot_score)`
//! - **Noise Sources**: Sybil wallets, wash trading
//!
//! ### SSMI (Sub-Slot Microentropy Index)
//! - **Weight**: 9.0
//! - **Contribution**: Based on microstructure entropy
//! - **Formula**: `c_ssmi = ssmi_entropy`
//! - **Noise Sources**: Slot timing jitter, concurrent transactions
//!
//! ### QASS (Quantum Amplitude Scoring System)
//! - **Weight**: 15.0
//! - **Contribution**: Based on score magnitude and stability
//! - **Formula**: `c_qass = min(1.0, qass_score / 100.0) · (1.0 - qass_volatility)`
//! - **Noise Sources**: Oracle MEV, price manipulation
//!
//! ### QOFSV (Quantum Orderflow Shadow Vector)
//! - **Weight**: 11.0
//! - **Contribution**: Based on flow magnitude and alignment coherence
//! - **Formula**: `c_qofsv = flow_magnitude · (1.0 - alignment_noise)`
//! - **Noise Sources**: Order splitting, hidden liquidity
//!
//! ### SCR (Slot-Coherence Resonance)
//! - **Weight**: 13.0
//! - **Contribution**: Inverse of bot activity detection
//! - **Formula**: `c_scr = 1.0 - scr_score` (lower SCR = less bots = higher confidence)
//! - **Noise Sources**: Network congestion, validator scheduling
//!
//! ### FRB (Flow Resonance Broker)
//! - **Weight**: 7.0
//! - **Contribution**: Based on flow pattern coherence
//! - **Formula**: `c_frb = flow_coherence · (1.0 - resonance_noise)`
//! - **Noise Sources**: Multi-pool arbitrage, cross-DEX activity
//!
//! ### QMAN (Quantum Market Anomaly Navigator)
//! - **Weight**: 14.0
//! - **Contribution**: Inverse of deviation risk
//! - **Formula**: `c_qman = 1.0 - deviation_risk`
//! - **Noise Sources**: Regime transitions, black swan events
//!
//! ### GeneMapper (Pattern Recognition)
//! - **Weight**: 10.0
//! - **Contribution**: Confidence in pattern identification
//! - **Formula**: `c_gene = 1.0 - match_score` (low match = no known scam = high confidence)
//! - **Noise Sources**: False positives, pattern drift
//!
//! ### ChaosEngine (Monte Carlo Simulation)
//! - **Weight**: 11.0
//! - **Contribution**: Inverse of simulated loss probability
//! - **Formula**: `c_chaos = 1.0 - loss_probability`
//! - **Noise Sources**: Model assumptions, scenario coverage
//!
//! ## Confidence Degradation Factors
//!
//! Confidence degrades when:
//! - **Data Quality Issues**: Missing data, stale data, corrupted signals
//! - **High Noise Levels**: Excessive bot activity, wash trading, manipulation
//! - **Signal Conflicts**: Contradictory signals from different modules
//! - **Uncertainty Conditions**: Low liquidity, thin order books, volatile markets
//!
//! ## Integration with Oracle Decision
//!
//! The confidence score modulates the final Oracle decision:
//! - **High Confidence (C > 0.8)**: Full conviction, normal position sizing
//! - **Medium Confidence (0.5 < C ≤ 0.8)**: Moderate conviction, reduced position
//! - **Low Confidence (C ≤ 0.5)**: Low conviction, skip or minimal position
//!
//! ## Calibration and Validation
//!
//! The confidence model should be:
//! 1. **Calibrated**: P(success | C=x) ≈ x for all x ∈ [0, 1]
//! 2. **Sharp**: Maximize separation between successful and failed trades
//! 3. **Stable**: Confidence should not fluctuate wildly on small input changes
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use ghost_brain::oracle::confidence_model::{ConfidenceModel, ConfidenceInputs};
//!
//! let model = ConfidenceModel::default();
//! let inputs = ConfidenceInputs {
//!     sobp_drop: 0.1,
//!     sobp_current: 1.5,
//!     sobp_ma: 1.4,
//!     mpcf_entropy: 0.7,
//!     // ... other fields
//! };
//!
//! let confidence_score = model.calculate_confidence(&inputs);
//! println!("Overall Confidence: {:.2}", confidence_score.overall);
//! ```

use serde::{Deserialize, Serialize};

/// Contribution weights for each module
///
/// These weights represent the relative importance of each module's
/// confidence contribution to the overall confidence score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceWeights {
    /// SOBP (Slot-Over-Slot Buying Pressure) weight
    pub sobp: f32,
    /// MPCF (Micro-Payload Cognitive Fingerprint) weight
    pub mpcf: f32,
    /// IWIM (Inter-Wallet Interaction Matrix) weight
    pub iwim: f32,
    /// SSMI (Sub-Slot Microentropy Index) weight
    pub ssmi: f32,
    /// QASS (Quantum Amplitude Scoring System) weight
    pub qass: f32,
    /// QOFSV (Quantum Orderflow Shadow Vector) weight
    pub qofsv: f32,
    /// SCR (Slot-Coherence Resonance) weight
    pub scr: f32,
    /// FRB (Flow Resonance Broker) weight
    pub frb: f32,
    /// QMAN (Quantum Market Anomaly Navigator) weight
    pub qman: f32,
    /// GeneMapper (Pattern Recognition) weight
    pub gene_mapper: f32,
    /// ChaosEngine (Monte Carlo Simulation) weight
    pub chaos_engine: f32,
}

impl Default for ConfidenceWeights {
    fn default() -> Self {
        Self {
            sobp: 12.0,
            mpcf: 10.0,
            iwim: 8.0,
            ssmi: 9.0,
            qass: 15.0,
            qofsv: 11.0,
            scr: 13.0,
            frb: 7.0,
            qman: 14.0,
            gene_mapper: 10.0,
            chaos_engine: 11.0,
        }
    }
}

impl ConfidenceWeights {
    /// Get the total sum of all weights
    pub fn total_weight(&self) -> f32 {
        self.sobp
            + self.mpcf
            + self.iwim
            + self.ssmi
            + self.qass
            + self.qofsv
            + self.scr
            + self.frb
            + self.qman
            + self.gene_mapper
            + self.chaos_engine
    }

    /// Create ConfidenceWeights from GhostBrainConfig's ConfidenceConfig
    ///
    /// This allows the confidence model to use weights defined in the central configuration.
    pub fn from_config(config: &crate::config::ConfidenceConfig) -> Self {
        Self {
            sobp: config.weight_sobp,
            mpcf: config.weight_mpcf,
            iwim: config.weight_iwim,
            ssmi: config.weight_ssmi,
            qass: config.weight_qass,
            qofsv: config.weight_qofsv,
            scr: config.weight_scr,
            frb: config.weight_frb,
            qman: config.weight_qman,
            gene_mapper: config.weight_gene_mapper,
            chaos_engine: config.weight_chaos_engine,
        }
    }

    /// Create ConfidenceWeights from a specific profile in ConfidenceConfig
    ///
    /// This allows the confidence model to use profile-specific weights based on
    /// pool age or market conditions.
    ///
    /// # Arguments
    /// * `config` - Reference to ConfidenceConfig from GhostBrainConfig
    /// * `time_since_creation_seconds` - Time since pool creation in seconds
    ///
    /// # Example
    /// ```rust,ignore
    /// use ghost_brain::config::GhostBrainConfig;
    /// use ghost_brain::oracle::confidence_model::ConfidenceWeights;
    ///
    /// let config = GhostBrainConfig::default();
    /// let weights = ConfidenceWeights::from_config_with_profile(&config.confidence, 60); // Young pool
    /// ```
    pub fn from_config_with_profile(
        config: &crate::config::ConfidenceConfig,
        time_since_creation_seconds: u64,
    ) -> Self {
        let profile = config.select_profile(time_since_creation_seconds);
        Self {
            sobp: profile.weight_sobp,
            mpcf: profile.weight_mpcf,
            iwim: profile.weight_iwim,
            ssmi: profile.weight_ssmi,
            qass: profile.weight_qass,
            qofsv: profile.weight_qofsv,
            scr: profile.weight_scr,
            frb: profile.weight_frb,
            qman: profile.weight_qman,
            gene_mapper: profile.weight_gene_mapper,
            chaos_engine: profile.weight_chaos_engine,
        }
    }

    /// Create ConfidenceWeights from a specific ProfileWeights
    ///
    /// # Arguments
    /// * `profile` - Reference to ProfileWeights
    pub fn from_profile(profile: &crate::config::ProfileWeights) -> Self {
        Self {
            sobp: profile.weight_sobp,
            mpcf: profile.weight_mpcf,
            iwim: profile.weight_iwim,
            ssmi: profile.weight_ssmi,
            qass: profile.weight_qass,
            qofsv: profile.weight_qofsv,
            scr: profile.weight_scr,
            frb: profile.weight_frb,
            qman: profile.weight_qman,
            gene_mapper: profile.weight_gene_mapper,
            chaos_engine: profile.weight_chaos_engine,
        }
    }
}

/// Input signals required for confidence calculation
///
/// All values should be normalized to reasonable ranges as documented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceInputs {
    // SOBP signals
    /// SOBP drop indicator [0.0, 1.0]
    pub sobp_drop: f32,
    /// Current SOBP value
    pub sobp_current: f32,
    /// SOBP moving average
    pub sobp_ma: f32,

    // MPCF signals
    /// MPCF entropy [0.0, 1.0]
    pub mpcf_entropy: f32,

    // IWIM signals
    /// Network coherence [0.0, 1.0]
    pub iwim_network_coherence: f32,
    /// Bot score [0.0, 1.0]
    pub iwim_bot_score: f32,

    // SSMI signals
    /// SSMI entropy [0.0, 1.0]
    pub ssmi_entropy: f32,

    // QASS signals
    /// QASS score [0.0, 100.0]
    pub qass_score: f32,
    /// QASS volatility/instability [0.0, 1.0]
    pub qass_volatility: f32,

    // QOFSV signals
    /// Flow magnitude [0.0, 1.0]
    pub qofsv_flow_magnitude: f32,
    /// Alignment noise [0.0, 1.0]
    pub qofsv_alignment_noise: f32,

    // SCR signals
    /// SCR score (bot activity) [0.0, 1.0]
    pub scr_score: f32,

    // FRB signals
    /// Flow coherence [0.0, 1.0]
    pub frb_flow_coherence: f32,
    /// Resonance noise [0.0, 1.0]
    pub frb_resonance_noise: f32,

    // QMAN signals
    /// Deviation risk [0.0, 1.0]
    pub qman_deviation_risk: f32,

    // GeneMapper signals
    /// Pattern match score [0.0, 1.0]
    pub gene_mapper_match_score: f32,

    // ChaosEngine signals
    /// Simulated loss probability [0.0, 1.0]
    pub chaos_loss_probability: f32,
}

impl Default for ConfidenceInputs {
    fn default() -> Self {
        Self {
            sobp_drop: 0.0,
            sobp_current: 1.0,
            sobp_ma: 1.0,
            mpcf_entropy: 0.5,
            iwim_network_coherence: 0.5,
            iwim_bot_score: 0.0,
            ssmi_entropy: 0.5,
            qass_score: 50.0,
            qass_volatility: 0.0,
            qofsv_flow_magnitude: 0.5,
            qofsv_alignment_noise: 0.0,
            scr_score: 0.0,
            frb_flow_coherence: 0.5,
            frb_resonance_noise: 0.0,
            qman_deviation_risk: 0.0,
            gene_mapper_match_score: 0.0,
            chaos_loss_probability: 0.0,
        }
    }
}

/// Per-module confidence contributions
///
/// Each field represents the confidence contribution [0.0, 1.0] from a specific module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleContributions {
    pub sobp: f32,
    pub mpcf: f32,
    pub iwim: f32,
    pub ssmi: f32,
    pub qass: f32,
    pub qofsv: f32,
    pub scr: f32,
    pub frb: f32,
    pub qman: f32,
    pub gene_mapper: f32,
    pub chaos_engine: f32,
}

/// Complete confidence score with overall score and module contributions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceScore {
    /// Overall confidence score [0.0, 1.0]
    pub overall: f32,
    /// Individual module contributions
    pub contributions: ModuleContributions,
    /// Metadata about the calculation
    pub metadata: ConfidenceMetadata,
}

/// Metadata about confidence calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceMetadata {
    /// Total weight sum used in calculation
    pub total_weight: f32,
    /// Number of modules with valid data
    pub valid_modules: u8,
    /// Overall data quality [0.0, 1.0]
    pub data_quality: f32,
    /// Overall noise level [0.0, 1.0]
    pub noise_level: f32,
    /// Veto information - indicates if any safety module vetoed the signal
    pub veto_info: VetoInfo,
}

/// Veto information for safety modules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VetoInfo {
    /// Whether GeneMapper vetoed (scam detection)
    pub gene_mapper_vetoed: bool,
    /// Whether SCR vetoed (bot activity too high)
    pub scr_vetoed: bool,
    /// Whether ChaosEngine vetoed (loss probability too high)
    pub chaos_engine_vetoed: bool,
    /// Signal score before veto multipliers were applied
    pub signal_score_before_veto: f32,
}

impl Default for VetoInfo {
    fn default() -> Self {
        Self {
            gene_mapper_vetoed: false,
            scr_vetoed: false,
            chaos_engine_vetoed: false,
            signal_score_before_veto: 0.0,
        }
    }
}

/// Confidence Model implementation
#[derive(Debug, Clone)]
pub struct ConfidenceModel {
    /// Module contribution weights
    pub weights: ConfidenceWeights,
}

impl Default for ConfidenceModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfidenceModel {
    /// Create a new ConfidenceModel with default weights
    pub fn new() -> Self {
        Self {
            weights: ConfidenceWeights::default(),
        }
    }

    /// Create a ConfidenceModel with custom weights
    pub fn with_weights(weights: ConfidenceWeights) -> Self {
        Self { weights }
    }

    /// Create a ConfidenceModel from GhostBrainConfig's ConfidenceConfig
    ///
    /// This is the recommended way to create a ConfidenceModel when using
    /// the centralized configuration system.
    ///
    /// # Arguments
    /// * `config` - Reference to ConfidenceConfig from GhostBrainConfig
    ///
    /// # Example
    /// ```rust,ignore
    /// use ghost_brain::config::GhostBrainConfig;
    /// use ghost_brain::oracle::confidence_model::ConfidenceModel;
    ///
    /// let config = GhostBrainConfig::from_toml_file("config.toml")?;
    /// let model = ConfidenceModel::from_config(&config.confidence);
    /// ```
    pub fn from_config(config: &crate::config::ConfidenceConfig) -> Self {
        Self {
            weights: ConfidenceWeights::from_config(config),
        }
    }

    /// Calculate confidence score from input signals
    pub fn calculate_confidence(&self, inputs: &ConfidenceInputs) -> ConfidenceScore {
        // Calculate individual module contributions
        let contributions = self.calculate_module_contributions(inputs);

        // Split modules into Signal (additive) and Veto (multipliers)
        // Signal Modules: QASS, SOBP, MPCF, IWIM, SSMI, QMAN, QOFSV, FRB
        // Veto Modules: GeneMapper, SCR, ChaosEngine

        // 1. Calculate signal_score from Signal Modules (0.0 - 1.0)
        let signal_weights = self.weights.qass
            + self.weights.sobp
            + self.weights.mpcf
            + self.weights.iwim
            + self.weights.ssmi
            + self.weights.qman
            + self.weights.qofsv
            + self.weights.frb;

        let signal_weighted_sum = self.weights.qass * contributions.qass
            + self.weights.sobp * contributions.sobp
            + self.weights.mpcf * contributions.mpcf
            + self.weights.iwim * contributions.iwim
            + self.weights.ssmi * contributions.ssmi
            + self.weights.qman * contributions.qman
            + self.weights.qofsv * contributions.qofsv
            + self.weights.frb * contributions.frb;

        let signal_score = if signal_weights > 0.0 {
            (signal_weighted_sum / signal_weights).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 2. Calculate veto multipliers (0.0 or 1.0) - HARD VETO below thresholds
        // Note: We use RAW scores from inputs, not the inverted contributions
        // GeneMapper: Raw score is match_score, high = scam detected
        let gene_raw = inputs.gene_mapper_match_score;
        let gene_mult = if gene_raw >= 0.5 { 0.0 } else { 1.0 };
        let gene_vetoed = gene_mult == 0.0;

        // SCR: Raw score is bot activity, high = lots of bots
        let scr_raw = inputs.scr_score;
        let scr_mult = if scr_raw >= 0.7 { 0.0 } else { 1.0 };
        let scr_vetoed = scr_mult == 0.0;

        // ChaosEngine: Raw score is loss_probability, high = risky
        let chaos_raw = inputs.chaos_loss_probability;
        let chaos_mult = if chaos_raw >= 0.6 { 0.0 } else { 1.0 };
        let chaos_vetoed = chaos_mult == 0.0;

        // 3. Calculate final confidence
        let final_confidence = signal_score * gene_mult * scr_mult * chaos_mult;

        // 4. Log veto reason if signal was high but got vetoed
        if final_confidence == 0.0 && signal_score > 0.7 {
            tracing::warn!(
                target: "confidence_veto",
                "High signal ({:.3}) VETOED by Safety! GeneMapper: {} (raw: {:.3}), SCR: {} (raw: {:.3}), Chaos: {} (raw: {:.3})",
                signal_score,
                if gene_vetoed { "VETO" } else { "pass" }, gene_raw,
                if scr_vetoed { "VETO" } else { "pass" }, scr_raw,
                if chaos_vetoed { "VETO" } else { "pass" }, chaos_raw
            );
        }

        // Calculate metadata with veto information
        let metadata = self.calculate_metadata_with_veto(
            inputs,
            &contributions,
            signal_score,
            gene_vetoed,
            scr_vetoed,
            chaos_vetoed,
        );

        ConfidenceScore {
            overall: final_confidence,
            contributions,
            metadata,
        }
    }

    /// Build ConfidenceInputs from MarketSignals and other Oracle modules
    ///
    /// This helper method constructs ConfidenceInputs from the various
    /// Oracle components to simplify integration.
    ///
    /// # Approximations
    ///
    /// Some modules do not yet have dedicated output structures, so this function
    /// uses reasonable approximations from available signals:
    ///
    /// - **IWIM**: Approximated from `deviation.coherence_loss` (inverted) and `resonance.risk`
    ///   - TODO: Replace with actual IWIM module when available
    /// - **QOFSV alignment_noise**: Approximated from inverse of `qass_alignment` absolute value
    ///   - This assumes high alignment = low noise, which is semantically reasonable
    ///   - TODO: Replace with actual QOFSV noise metric when available
    /// - **FRB flow_coherence**: Approximated from inverse of `outflow`
    ///   - Assumes low outflow = high coherence, reasonable for basic cases
    ///   - TODO: Replace with actual FRB coherence metric when available
    ///
    /// These approximations allow the confidence model to be used immediately
    /// while the full module implementations are being completed.
    pub fn build_inputs_from_signals(
        signals: &crate::signals::MarketSignals,
        qass_score: f32,
        qass_volatility: f32,
        scr_score: f32,
        gene_mapper_score: f32,
        chaos_loss_prob: f32,
    ) -> ConfidenceInputs {
        ConfidenceInputs {
            // SOBP signals
            sobp_drop: signals.sobp.drop as f32,
            sobp_current: signals.sobp.current as f32,
            sobp_ma: signals.sobp.ma as f32,

            // MPCF entropy (from entropy signals)
            mpcf_entropy: signals.entropy.mpcf as f32,

            // IWIM - we approximate using available signals
            // In a full implementation, these would come from a dedicated IWIM module
            iwim_network_coherence: (1.0 - signals.deviation.coherence_loss as f32).clamp(0.0, 1.0),
            iwim_bot_score: signals.resonance.risk as f32,

            // SSMI entropy
            ssmi_entropy: signals.entropy.ssmi as f32,

            // QASS signals (passed as parameters)
            qass_score,
            qass_volatility,

            // QOFSV signals
            qofsv_flow_magnitude: signals.flow.magnitude as f32,
            qofsv_alignment_noise: (1.0 - signals.flow.qass_alignment.abs() as f32).clamp(0.0, 1.0),

            // SCR signals (passed as parameter)
            scr_score,

            // FRB signals - approximated from flow and resonance
            frb_flow_coherence: (1.0 - signals.flow.outflow as f32).clamp(0.0, 1.0),
            frb_resonance_noise: signals.resonance.risk as f32,

            // QMAN signals
            qman_deviation_risk: signals.deviation.risk as f32,

            // GeneMapper (passed as parameter)
            gene_mapper_match_score: gene_mapper_score,

            // ChaosEngine (passed as parameter)
            chaos_loss_probability: chaos_loss_prob,
        }
    }

    /// Calculate individual module contributions
    fn calculate_module_contributions(&self, inputs: &ConfidenceInputs) -> ModuleContributions {
        ModuleContributions {
            sobp: self.calculate_sobp_contribution(inputs),
            mpcf: self.calculate_mpcf_contribution(inputs),
            iwim: self.calculate_iwim_contribution(inputs),
            ssmi: self.calculate_ssmi_contribution(inputs),
            qass: self.calculate_qass_contribution(inputs),
            qofsv: self.calculate_qofsv_contribution(inputs),
            scr: self.calculate_scr_contribution(inputs),
            frb: self.calculate_frb_contribution(inputs),
            qman: self.calculate_qman_contribution(inputs),
            gene_mapper: self.calculate_gene_mapper_contribution(inputs),
            chaos_engine: self.calculate_chaos_contribution(inputs),
        }
    }

    /// SOBP contribution: (1.0 - sobp_drop) · min(1.0, sobp_current / sobp_ma)
    fn calculate_sobp_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        let stability = 1.0 - inputs.sobp_drop.clamp(0.0, 1.0);
        let ratio = if inputs.sobp_ma > 0.0 {
            (inputs.sobp_current / inputs.sobp_ma).min(1.0)
        } else {
            0.5 // Neutral if no MA available
        };
        (stability * ratio).clamp(0.0, 1.0)
    }

    /// MPCF contribution: mpcf_entropy (already normalized)
    fn calculate_mpcf_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        inputs.mpcf_entropy.clamp(0.0, 1.0)
    }

    /// IWIM contribution: network_coherence · (1.0 - bot_score)
    fn calculate_iwim_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        let coherence = inputs.iwim_network_coherence.clamp(0.0, 1.0);
        let organic = 1.0 - inputs.iwim_bot_score.clamp(0.0, 1.0);
        (coherence * organic).clamp(0.0, 1.0)
    }

    /// SSMI contribution: ssmi_entropy (already normalized)
    fn calculate_ssmi_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        inputs.ssmi_entropy.clamp(0.0, 1.0)
    }

    /// QASS contribution: min(1.0, qass_score / 100.0) · (1.0 - qass_volatility)
    fn calculate_qass_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        let normalized_score = (inputs.qass_score / 100.0).min(1.0).max(0.0);
        let stability = 1.0 - inputs.qass_volatility.clamp(0.0, 1.0);
        (normalized_score * stability).clamp(0.0, 1.0)
    }

    /// QOFSV contribution: flow_magnitude · (1.0 - alignment_noise)
    fn calculate_qofsv_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        let magnitude = inputs.qofsv_flow_magnitude.clamp(0.0, 1.0);
        let clarity = 1.0 - inputs.qofsv_alignment_noise.clamp(0.0, 1.0);
        (magnitude * clarity).clamp(0.0, 1.0)
    }

    /// SCR contribution: 1.0 - scr_score (inverse, lower bot activity = higher confidence)
    fn calculate_scr_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        (1.0 - inputs.scr_score.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// FRB contribution: flow_coherence · (1.0 - resonance_noise)
    fn calculate_frb_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        let coherence = inputs.frb_flow_coherence.clamp(0.0, 1.0);
        let clarity = 1.0 - inputs.frb_resonance_noise.clamp(0.0, 1.0);
        (coherence * clarity).clamp(0.0, 1.0)
    }

    /// QMAN contribution: 1.0 - deviation_risk (inverse)
    fn calculate_qman_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        (1.0 - inputs.qman_deviation_risk.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// GeneMapper contribution: 1.0 - match_score (inverse, no scam match = high confidence)
    fn calculate_gene_mapper_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        (1.0 - inputs.gene_mapper_match_score.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// ChaosEngine contribution: 1.0 - loss_probability (inverse)
    fn calculate_chaos_contribution(&self, inputs: &ConfidenceInputs) -> f32 {
        (1.0 - inputs.chaos_loss_probability.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Calculate metadata about the confidence calculation with veto information
    fn calculate_metadata_with_veto(
        &self,
        inputs: &ConfidenceInputs,
        contributions: &ModuleContributions,
        signal_score_before_veto: f32,
        gene_mapper_vetoed: bool,
        scr_vetoed: bool,
        chaos_engine_vetoed: bool,
    ) -> ConfidenceMetadata {
        let base_metadata = self.calculate_metadata_base(inputs, contributions);

        ConfidenceMetadata {
            total_weight: base_metadata.total_weight,
            valid_modules: base_metadata.valid_modules,
            data_quality: base_metadata.data_quality,
            noise_level: base_metadata.noise_level,
            veto_info: VetoInfo {
                gene_mapper_vetoed,
                scr_vetoed,
                chaos_engine_vetoed,
                signal_score_before_veto,
            },
        }
    }

    /// Calculate base metadata about the confidence calculation
    fn calculate_metadata_base(
        &self,
        inputs: &ConfidenceInputs,
        contributions: &ModuleContributions,
    ) -> ConfidenceMetadata {
        // Count valid modules (contributions > 0.01)
        let valid_modules = [
            contributions.sobp,
            contributions.mpcf,
            contributions.iwim,
            contributions.ssmi,
            contributions.qass,
            contributions.qofsv,
            contributions.scr,
            contributions.frb,
            contributions.qman,
            contributions.gene_mapper,
            contributions.chaos_engine,
        ]
        .iter()
        .filter(|&&c| c > 0.01)
        .count() as u8;

        // Estimate overall data quality (inverse of variance in contributions)
        let avg = (contributions.sobp
            + contributions.mpcf
            + contributions.iwim
            + contributions.ssmi
            + contributions.qass
            + contributions.qofsv
            + contributions.scr
            + contributions.frb
            + contributions.qman
            + contributions.gene_mapper
            + contributions.chaos_engine)
            / 11.0;

        let variance = [
            contributions.sobp,
            contributions.mpcf,
            contributions.iwim,
            contributions.ssmi,
            contributions.qass,
            contributions.qofsv,
            contributions.scr,
            contributions.frb,
            contributions.qman,
            contributions.gene_mapper,
            contributions.chaos_engine,
        ]
        .iter()
        .map(|&c| (c - avg).powi(2))
        .sum::<f32>()
            / 11.0;

        let data_quality = (1.0 - variance.sqrt()).clamp(0.0, 1.0);

        // Estimate overall noise level (average of noise-related inputs)
        // NOTE: This is a simplified aggregate. In production, different types
        // of noise (volatility, bot activity, risk) may need different weightings.
        // Current implementation treats all noise sources equally.
        let noise_level = (inputs.sobp_drop
            + inputs.iwim_bot_score
            + inputs.qass_volatility
            + inputs.qofsv_alignment_noise
            + inputs.scr_score
            + inputs.frb_resonance_noise
            + inputs.qman_deviation_risk
            + inputs.gene_mapper_match_score
            + inputs.chaos_loss_probability)
            / 9.0;

        ConfidenceMetadata {
            total_weight: self.weights.total_weight(),
            valid_modules,
            data_quality,
            noise_level: noise_level.clamp(0.0, 1.0),
            veto_info: VetoInfo::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_model_default() {
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs::default();
        let score = model.calculate_confidence(&inputs);

        assert!(score.overall >= 0.0 && score.overall <= 1.0);
        assert!(score.metadata.valid_modules > 0);
    }

    #[test]
    fn test_confidence_perfect_signals() {
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            sobp_drop: 0.0,
            sobp_current: 1.5,
            sobp_ma: 1.0,
            mpcf_entropy: 1.0,
            iwim_network_coherence: 1.0,
            iwim_bot_score: 0.0,
            ssmi_entropy: 1.0,
            qass_score: 100.0,
            qass_volatility: 0.0,
            qofsv_flow_magnitude: 1.0,
            qofsv_alignment_noise: 0.0,
            scr_score: 0.0,
            frb_flow_coherence: 1.0,
            frb_resonance_noise: 0.0,
            qman_deviation_risk: 0.0,
            gene_mapper_match_score: 0.0,
            chaos_loss_probability: 0.0,
        };

        let score = model.calculate_confidence(&inputs);

        // Perfect signals should yield very high confidence
        assert!(score.overall > 0.9, "Overall confidence: {}", score.overall);
        assert_eq!(score.metadata.valid_modules, 11);
    }

    #[test]
    fn test_confidence_poor_signals() {
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            sobp_drop: 1.0,
            sobp_current: 0.5,
            sobp_ma: 2.0,
            mpcf_entropy: 0.1,
            iwim_network_coherence: 0.2,
            iwim_bot_score: 0.9,
            ssmi_entropy: 0.1,
            qass_score: 10.0,
            qass_volatility: 0.9,
            qofsv_flow_magnitude: 0.1,
            qofsv_alignment_noise: 0.9,
            scr_score: 0.9,
            frb_flow_coherence: 0.1,
            frb_resonance_noise: 0.9,
            qman_deviation_risk: 0.9,
            gene_mapper_match_score: 0.9,
            chaos_loss_probability: 0.9,
        };

        let score = model.calculate_confidence(&inputs);

        // Poor signals should yield low confidence
        assert!(score.overall < 0.3, "Overall confidence: {}", score.overall);
        assert!(score.metadata.noise_level > 0.7);
    }

    #[test]
    fn test_confidence_bounds() {
        let model = ConfidenceModel::default();

        // Test with extreme values to ensure clamping works
        let inputs = ConfidenceInputs {
            sobp_drop: 10.0,    // Out of bounds
            sobp_current: -5.0, // Out of bounds
            sobp_ma: 1.0,
            mpcf_entropy: 2.0,            // Out of bounds
            iwim_network_coherence: -1.0, // Out of bounds
            iwim_bot_score: 5.0,          // Out of bounds
            ssmi_entropy: 0.5,
            qass_score: 150.0,     // Out of bounds
            qass_volatility: -0.5, // Out of bounds
            qofsv_flow_magnitude: 0.5,
            qofsv_alignment_noise: 0.5,
            scr_score: 0.5,
            frb_flow_coherence: 0.5,
            frb_resonance_noise: 0.5,
            qman_deviation_risk: 0.5,
            gene_mapper_match_score: 0.5,
            chaos_loss_probability: 0.5,
        };

        let score = model.calculate_confidence(&inputs);

        // Despite extreme inputs, output should be bounded
        assert!(score.overall >= 0.0 && score.overall <= 1.0);

        // All contributions should be bounded
        assert!(score.contributions.sobp >= 0.0 && score.contributions.sobp <= 1.0);
        assert!(score.contributions.mpcf >= 0.0 && score.contributions.mpcf <= 1.0);
        assert!(score.contributions.iwim >= 0.0 && score.contributions.iwim <= 1.0);
        assert!(score.contributions.qass >= 0.0 && score.contributions.qass <= 1.0);
    }

    #[test]
    fn test_sobp_contribution() {
        let model = ConfidenceModel::default();

        let inputs_stable = ConfidenceInputs {
            sobp_drop: 0.0,
            sobp_current: 1.5,
            sobp_ma: 1.0,
            ..Default::default()
        };

        let inputs_unstable = ConfidenceInputs {
            sobp_drop: 0.9,
            sobp_current: 0.5,
            sobp_ma: 2.0,
            ..Default::default()
        };

        let stable_contrib = model.calculate_sobp_contribution(&inputs_stable);
        let unstable_contrib = model.calculate_sobp_contribution(&inputs_unstable);

        assert!(stable_contrib > unstable_contrib);
    }

    #[test]
    fn test_custom_weights() {
        let custom_weights = ConfidenceWeights {
            sobp: 20.0,
            qass: 25.0,
            ..Default::default()
        };

        let model = ConfidenceModel::with_weights(custom_weights);
        let inputs = ConfidenceInputs {
            sobp_drop: 0.0,
            sobp_current: 2.0,
            sobp_ma: 1.0,
            qass_score: 90.0,
            qass_volatility: 0.1,
            ..Default::default()
        };

        let score = model.calculate_confidence(&inputs);

        // With increased weights for SOBP and QASS, and good values for both,
        // confidence should be higher
        assert!(score.overall >= 0.0 && score.overall <= 1.0);
    }

    #[test]
    fn test_veto_high_signal_scam_detection() {
        // Test veto scenario: QASS = 1.0 (super viral), GeneMapper = 0.0 (scam) → Confidence = 0.0
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            // Super high signal - perfect QASS
            qass_score: 100.0,
            qass_volatility: 0.0,
            sobp_drop: 0.0,
            sobp_current: 2.0,
            sobp_ma: 1.0,
            mpcf_entropy: 1.0,
            iwim_network_coherence: 1.0,
            iwim_bot_score: 0.0,
            ssmi_entropy: 1.0,
            qofsv_flow_magnitude: 1.0,
            qofsv_alignment_noise: 0.0,
            frb_flow_coherence: 1.0,
            frb_resonance_noise: 0.0,
            qman_deviation_risk: 0.0,
            // But GeneMapper detects a scam pattern (high match score)
            gene_mapper_match_score: 0.9,
            // Other veto modules are fine
            scr_score: 0.2,
            chaos_loss_probability: 0.1,
        };

        let score = model.calculate_confidence(&inputs);

        // Confidence should be ZERO due to GeneMapper veto
        assert_eq!(
            score.overall, 0.0,
            "Confidence should be 0.0 due to GeneMapper veto"
        );

        // Signal score before veto should be high
        assert!(
            score.metadata.veto_info.signal_score_before_veto > 0.7,
            "Signal score before veto should be high: {}",
            score.metadata.veto_info.signal_score_before_veto
        );

        // GeneMapper should have vetoed
        assert!(
            score.metadata.veto_info.gene_mapper_vetoed,
            "GeneMapper should have vetoed"
        );
        assert!(
            !score.metadata.veto_info.scr_vetoed,
            "SCR should not have vetoed"
        );
        assert!(
            !score.metadata.veto_info.chaos_engine_vetoed,
            "ChaosEngine should not have vetoed"
        );
    }

    #[test]
    fn test_veto_passing_all_thresholds() {
        // Test passing scenario: All modules above thresholds → High confidence
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            // High signal
            qass_score: 90.0,
            qass_volatility: 0.1,
            sobp_drop: 0.0,
            sobp_current: 2.0,
            sobp_ma: 1.0,
            mpcf_entropy: 0.9,
            iwim_network_coherence: 0.9,
            iwim_bot_score: 0.1,
            ssmi_entropy: 0.9,
            qofsv_flow_magnitude: 0.9,
            qofsv_alignment_noise: 0.1,
            frb_flow_coherence: 0.9,
            frb_resonance_noise: 0.1,
            qman_deviation_risk: 0.1,
            // All veto modules pass
            gene_mapper_match_score: 0.2, // < 0.5 threshold
            scr_score: 0.3,               // < 0.7 threshold
            chaos_loss_probability: 0.2,  // < 0.6 threshold
        };

        let score = model.calculate_confidence(&inputs);

        // Confidence should be high
        assert!(
            score.overall > 0.7,
            "Confidence should be high: {}",
            score.overall
        );

        // No vetos should have occurred
        assert!(
            !score.metadata.veto_info.gene_mapper_vetoed,
            "GeneMapper should not veto"
        );
        assert!(!score.metadata.veto_info.scr_vetoed, "SCR should not veto");
        assert!(
            !score.metadata.veto_info.chaos_engine_vetoed,
            "ChaosEngine should not veto"
        );
    }

    #[test]
    fn test_veto_scr_bot_activity() {
        // Test partial veto: High signal but SCR fails (too many bots)
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            // High signal
            qass_score: 95.0,
            qass_volatility: 0.05,
            sobp_drop: 0.0,
            sobp_current: 2.5,
            sobp_ma: 1.5,
            mpcf_entropy: 0.9,
            iwim_network_coherence: 0.9,
            iwim_bot_score: 0.05,
            ssmi_entropy: 0.9,
            qofsv_flow_magnitude: 0.9,
            qofsv_alignment_noise: 0.05,
            frb_flow_coherence: 0.9,
            frb_resonance_noise: 0.05,
            qman_deviation_risk: 0.05,
            gene_mapper_match_score: 0.1,
            // SCR detects high bot activity
            scr_score: 0.85, // >= 0.7 threshold - VETO
            chaos_loss_probability: 0.2,
        };

        let score = model.calculate_confidence(&inputs);

        // Confidence should be ZERO due to SCR veto
        assert_eq!(
            score.overall, 0.0,
            "Confidence should be 0.0 due to SCR veto"
        );

        // Signal score before veto should be high
        assert!(
            score.metadata.veto_info.signal_score_before_veto > 0.7,
            "Signal score before veto should be high"
        );

        // SCR should have vetoed
        assert!(
            !score.metadata.veto_info.gene_mapper_vetoed,
            "GeneMapper should not veto"
        );
        assert!(
            score.metadata.veto_info.scr_vetoed,
            "SCR should have vetoed"
        );
        assert!(
            !score.metadata.veto_info.chaos_engine_vetoed,
            "ChaosEngine should not veto"
        );
    }

    #[test]
    fn test_veto_chaos_engine_loss_probability() {
        // Test partial veto: High signal but ChaosEngine predicts high loss
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            // High signal
            qass_score: 88.0,
            qass_volatility: 0.1,
            sobp_drop: 0.0,
            sobp_current: 2.2,
            sobp_ma: 1.5,
            mpcf_entropy: 0.85,
            iwim_network_coherence: 0.85,
            iwim_bot_score: 0.1,
            ssmi_entropy: 0.85,
            qofsv_flow_magnitude: 0.85,
            qofsv_alignment_noise: 0.1,
            frb_flow_coherence: 0.85,
            frb_resonance_noise: 0.1,
            qman_deviation_risk: 0.1,
            gene_mapper_match_score: 0.2,
            scr_score: 0.4,
            // ChaosEngine predicts high loss probability
            chaos_loss_probability: 0.75, // >= 0.6 threshold - VETO
        };

        let score = model.calculate_confidence(&inputs);

        // Confidence should be ZERO due to ChaosEngine veto
        assert_eq!(
            score.overall, 0.0,
            "Confidence should be 0.0 due to ChaosEngine veto"
        );

        // Signal score before veto should be high
        assert!(
            score.metadata.veto_info.signal_score_before_veto > 0.7,
            "Signal score before veto should be high"
        );

        // ChaosEngine should have vetoed
        assert!(
            !score.metadata.veto_info.gene_mapper_vetoed,
            "GeneMapper should not veto"
        );
        assert!(!score.metadata.veto_info.scr_vetoed, "SCR should not veto");
        assert!(
            score.metadata.veto_info.chaos_engine_vetoed,
            "ChaosEngine should have vetoed"
        );
    }

    #[test]
    fn test_veto_multiple_failures() {
        // Test multiple veto modules failing
        let model = ConfidenceModel::default();
        let inputs = ConfidenceInputs {
            qass_score: 95.0,
            qass_volatility: 0.0,
            sobp_drop: 0.0,
            sobp_current: 3.0,
            sobp_ma: 1.0,
            mpcf_entropy: 1.0,
            iwim_network_coherence: 1.0,
            iwim_bot_score: 0.0,
            ssmi_entropy: 1.0,
            qofsv_flow_magnitude: 1.0,
            qofsv_alignment_noise: 0.0,
            frb_flow_coherence: 1.0,
            frb_resonance_noise: 0.0,
            qman_deviation_risk: 0.0,
            // All veto modules fail
            gene_mapper_match_score: 0.8, // VETO
            scr_score: 0.9,               // VETO
            chaos_loss_probability: 0.9,  // VETO
        };

        let score = model.calculate_confidence(&inputs);

        // Confidence should be ZERO
        assert_eq!(
            score.overall, 0.0,
            "Confidence should be 0.0 due to multiple vetos"
        );

        // All veto modules should have vetoed
        assert!(
            score.metadata.veto_info.gene_mapper_vetoed,
            "GeneMapper should veto"
        );
        assert!(score.metadata.veto_info.scr_vetoed, "SCR should veto");
        assert!(
            score.metadata.veto_info.chaos_engine_vetoed,
            "ChaosEngine should veto"
        );
    }

    #[test]
    fn test_profile_weights_young_pool() {
        use crate::config::GhostBrainConfig;

        let config = GhostBrainConfig::default();

        // Young pool (1 minute old) should use standard profile with high QASS weight
        let weights = ConfidenceWeights::from_config_with_profile(&config.confidence, 60);

        assert_eq!(
            weights.qass, 20.0,
            "Young pool should have high QASS weight"
        );
        assert_eq!(
            weights.sobp, 12.0,
            "Young pool should have moderate SOBP weight"
        );
        assert_eq!(weights.qman, 8.0, "Young pool should have low QMAN weight");
    }

    #[test]
    fn test_profile_weights_mature_pool() {
        use crate::config::GhostBrainConfig;

        let config = GhostBrainConfig::default();

        // Mature pool (15 minutes old) should use reversal profile with low QASS, high SOBP/QMAN
        let weights = ConfidenceWeights::from_config_with_profile(&config.confidence, 900);

        assert_eq!(
            weights.qass, 10.0,
            "Mature pool should have low QASS weight"
        );
        assert_eq!(
            weights.sobp, 18.0,
            "Mature pool should have high SOBP weight"
        );
        assert_eq!(
            weights.qman, 16.0,
            "Mature pool should have high QMAN weight"
        );
    }

    #[test]
    fn test_profile_weights_impact_on_confidence() {
        use crate::config::GhostBrainConfig;

        let config = GhostBrainConfig::default();

        // Create inputs with high QASS but moderate SOBP
        let inputs = ConfidenceInputs {
            qass_score: 95.0,
            qass_volatility: 0.0,
            sobp_drop: 0.0,
            sobp_current: 1.5,
            sobp_ma: 1.3,
            mpcf_entropy: 0.7,
            iwim_network_coherence: 0.7,
            iwim_bot_score: 0.2,
            ssmi_entropy: 0.7,
            qofsv_flow_magnitude: 0.7,
            qofsv_alignment_noise: 0.2,
            frb_flow_coherence: 0.7,
            frb_resonance_noise: 0.2,
            qman_deviation_risk: 0.3,
            gene_mapper_match_score: 0.1,
            scr_score: 0.3,
            chaos_loss_probability: 0.2,
        };

        // Young pool model (high QASS weight)
        let young_weights = ConfidenceWeights::from_config_with_profile(&config.confidence, 60);
        let young_model = ConfidenceModel::with_weights(young_weights);
        let young_score = young_model.calculate_confidence(&inputs);

        // Mature pool model (low QASS weight, high SOBP/QMAN weight)
        let mature_weights = ConfidenceWeights::from_config_with_profile(&config.confidence, 900);
        let mature_model = ConfidenceModel::with_weights(mature_weights);
        let mature_score = mature_model.calculate_confidence(&inputs);

        // Young pool should have higher confidence due to high QASS
        assert!(young_score.overall > mature_score.overall,
                "Young pool (high QASS weight) should have higher confidence with high QASS score: young={}, mature={}",
                young_score.overall, mature_score.overall);
    }

    #[test]
    fn test_from_profile_direct() {
        use crate::config::ProfileWeights;

        let profile = ProfileWeights {
            weight_qass: 25.0,
            weight_sobp: 15.0,
            weight_mpcf: 12.0,
            weight_iwim: 10.0,
            weight_ssmi: 11.0,
            weight_qofsv: 13.0,
            weight_scr: 14.0,
            weight_frb: 8.0,
            weight_qman: 9.0,
            weight_gene_mapper: 11.0,
            weight_chaos_engine: 12.0,
        };

        let weights = ConfidenceWeights::from_profile(&profile);

        assert_eq!(weights.qass, 25.0);
        assert_eq!(weights.sobp, 15.0);
        assert_eq!(weights.qman, 9.0);
    }
}
