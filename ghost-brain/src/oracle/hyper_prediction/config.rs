//! HyperPrediction Configuration Module
//!
//! This module centralizes all configuration parameters for the HyperPrediction Oracle,
//! replacing hardcoded constants with configurable values from `ghost_brain_config.toml`.
//!
//! ## Design Rationale
//!
//! The HyperPrediction Oracle uses numerous thresholds and constants that affect
//! scoring behavior. By centralizing these in a config file, we enable:
//!
//! - **Runtime tuning** without recompilation
//! - **A/B testing** of different parameter sets
//! - **Environment-specific** configurations (dev, staging, prod)
//! - **Documentation** of parameter meaning and expected ranges
//!
//! ## Configuration Categories
//!
//! 1. **SurvivorScore Thresholds**: Early exit and modifier constraints
//! 2. **Cold Start Parameters**: Adjustment factors for early-stage analysis
//! 3. **MESA Microstructure**: Wash trading, bot detection, organic activity thresholds
//! 4. **Scoring Normalization**: Volume scales, factor caps, burst handling
//! 5. **Risk Assessment**: Configurable risk level boundaries
//!
//! ## Usage Example
//!
//! ```ignore
//! use ghost_brain::config::GhostBrainConfig;
//! use ghost_brain::oracle::hyper_prediction::HyperPredictionConfig;
//!
//! // Load from config file
//! let brain_config = GhostBrainConfig::from_toml_file("config.toml")?;
//! let hp_config = HyperPredictionConfig::from_config(&brain_config)?;
//!
//! // Validate parameters
//! hp_config.validate()?;
//!
//! // Use in oracle
//! let oracle = HyperPredictionOracle::new_with_config(70, &brain_config);
//! ```

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::GhostBrainConfig;
use crate::oracle::hyper_prediction::RiskThresholds;

/// Central configuration for HyperPrediction Oracle
///
/// Contains all tunable parameters that affect scoring, thresholds, and risk assessment.
/// All values have documented expected ranges and default values based on empirical testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperPredictionConfig {
    /// Gatekeeper minimum transaction count (hard gate for pre-flight)
    ///
    /// Used to align orchestrator early-stage detection with Gatekeeper config.
    ///
    /// **Default: 15** (from GatekeeperConfig defaults)
    ///
    /// **Range: [1, 500]**
    #[serde(default = "default_gatekeeper_min_tx_count")]
    pub gatekeeper_min_tx_count: usize,

    /// Early-stage multiplier relative to Gatekeeper minimum
    ///
    /// Early stage threshold = gatekeeper_min_tx_count × early_stage_multiplier
    ///
    /// **Default: 1.5**
    ///
    /// **Range: [1.0, 3.0]**
    #[serde(default = "default_early_stage_multiplier")]
    pub early_stage_multiplier: f32,

    /// SurvivorScore threshold for early exit (0-100)
    ///
    /// Tokens with SurvivorScore below this value are rejected immediately without
    /// applying additional modifiers. This provides a fail-fast gate for clearly
    /// unsuitable tokens.
    ///
    /// **Default: 35**
    ///
    /// **Range: [0, 100]**
    ///
    /// **Rationale**: Empirical testing shows that tokens scoring below 35 have
    /// extremely low survival rates (< 5%) even when other signals are favorable.
    /// This threshold prevents wasting compute on hopeless candidates.
    pub survivor_critical_threshold: u8,

    /// QASS maximum adjustment as secondary modifier (±pts)
    ///
    /// In Phase 4.5, QASS was demoted from primary scorer to secondary modifier.
    /// This limits how much QASS can adjust the SurvivorScore-based result.
    ///
    /// **Default: 10**
    ///
    /// **Range: [0, 30]**
    ///
    /// **Rationale**: QASS provides valuable wave interference analysis but should
    /// not dominate the more interpretable SurvivorScore. ±10 points allows meaningful
    /// adjustment without overwhelming the primary signal.
    pub qass_secondary_max_adjustment: i8,

    /// Minimum QASS confidence for modifier to apply (0.0-1.0)
    ///
    /// Low-confidence QASS results (below this threshold) are ignored to prevent
    /// noise from affecting the final score.
    ///
    /// **Default: 0.6**
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// **Rationale**: QASS confidence below 60% typically indicates insufficient
    /// wave data or conflicting signals. At this confidence level, the adjustment
    /// is more likely to add noise than value.
    pub qass_min_confidence_for_modifier: f32,

    /// Cold start maximum adjustment factor (0.0-1.0)
    ///
    /// Maximum percentage by which the Cold Start Multiplier can adjust the score
    /// based on Chaos Engine pump probability. Value of 0.3 means ±30% adjustment.
    ///
    /// **Default: 0.3**
    ///
    /// **Range: [0.0, 0.5]**
    ///
    /// **Rationale**: In cold start (tx_count < 10), Chaos Engine Monte Carlo
    /// simulations provide valuable pump/crash probability signals. 30% adjustment
    /// balances early detection benefits against false positive risk.
    ///
    /// **Example**: base_score=50, pump_prob=80% → adjustment=+9% → final=54.5
    pub cold_start_max_adjustment: f32,

    /// Cold start QEDD/MCI extrapolation weight (0.0-20.0)
    ///
    /// Weight multiplier for first-candle volume when extrapolating QEDD/MCI
    /// signals in cold start mode (tx_count < 10).
    ///
    /// **Default: 10.0**
    ///
    /// **Range: [1.0, 20.0]**
    ///
    /// **Rationale**: First transaction volume carries strong signal in pump.fun
    /// launches. Weight of 10x makes 5 SOL burst in S1 equivalent to 50 SOL
    /// accumulated volume for QEDD/MCI computation, allowing early detection of
    /// strong launches without waiting for history accumulation.
    pub cold_start_qedd_mci_weight: f32,

    /// MESA wash trading severe threshold (0.0-1.0)
    ///
    /// Wash likeness above this triggers -25 pts penalty and VeryHigh risk escalation.
    ///
    /// **Default: 0.85**
    ///
    /// **Range: [0.7, 1.0]**
    ///
    /// **Rationale**: 85% wash likeness indicates coordinated buy-sell cycling
    /// (e.g., alternating 1 SOL buys/sells from 2-3 wallets). This is a strong
    /// indicator of artificial volume inflation.
    pub mesa_wash_severe_threshold: f32,

    /// MESA wash trading elevated threshold (0.0-1.0)
    ///
    /// Wash likeness above this triggers -12 pts penalty.
    ///
    /// **Default: 0.70**
    ///
    /// **Range: [0.5, 0.85]**
    ///
    /// **Rationale**: 70% wash likeness indicates suspicious patterns (e.g., high
    /// buy-sell symmetry, low unique wallets) but not necessarily coordinated attack.
    /// Moderate penalty allows recovery with strong organic signals.
    pub mesa_wash_elevated_threshold: f32,

    /// MESA bot pattern high threshold (0.0-1.0)
    ///
    /// Bot likeness above this triggers -15 pts penalty.
    ///
    /// **Default: 0.90**
    ///
    /// **Range: [0.7, 1.0]**
    ///
    /// **Rationale**: 90% bot likeness indicates automated snipers (identical tx
    /// sizes, microsecond timing, low entropy). While not necessarily malicious,
    /// bot-dominated launches often lack organic follow-through.
    pub mesa_bot_high_threshold: f32,

    /// MESA bot pattern moderate threshold (0.0-1.0)
    ///
    /// Bot likeness above this triggers -8 pts penalty.
    ///
    /// **Default: 0.75**
    ///
    /// **Range: [0.5, 0.90]**
    ///
    /// **Rationale**: 75% bot likeness indicates significant sniper presence but
    /// with some organic mixing. Moderate penalty reflects reduced but not eliminated
    /// organic potential.
    pub mesa_bot_moderate_threshold: f32,

    /// MESA organic activity threshold for bonus (0.0-1.0)
    ///
    /// Organic likeness above this (with low wash) triggers +8 pts bonus.
    ///
    /// **Default: 0.75**
    ///
    /// **Range: [0.6, 0.9]**
    ///
    /// **Rationale**: 75% organic likeness (varied volumes, high unique wallets,
    /// high entropy) with wash < 40% indicates genuine community interest. This
    /// is the sweet spot for pump.fun launches with retail momentum.
    pub mesa_organic_bonus_threshold: f32,

    /// MESA max wash likeness for organic bonus (0.0-1.0)
    ///
    /// Maximum wash likeness allowed to receive organic bonus.
    ///
    /// **Default: 0.40**
    ///
    /// **Range: [0.3, 0.6]**
    ///
    /// **Rationale**: Even organic launches may show 30-40% wash patterns due to
    /// normal trading behavior (profit-taking, rebalancing). Above 40% suggests
    /// manipulation rather than organic volatility.
    pub mesa_organic_max_wash: f32,

    /// MESA high entropy threshold for bonus (0.0-1.0)
    ///
    /// Entropy score above this (with low wash) triggers +5 pts bonus.
    ///
    /// **Default: 0.80**
    ///
    /// **Range: [0.7, 0.95]**
    ///
    /// **Rationale**: 80% entropy indicates high unpredictability in transaction
    /// timing, sizes, and wallet distribution. This signals genuine decentralized
    /// activity rather than coordinated patterns.
    pub mesa_entropy_bonus_threshold: f32,

    /// MESA max wash likeness for entropy bonus (0.0-1.0)
    ///
    /// Maximum wash likeness allowed to receive entropy bonus.
    ///
    /// **Default: 0.50**
    ///
    /// **Range: [0.4, 0.7]**
    ///
    /// **Rationale**: Entropy bonus requires lower wash threshold (50% vs 40%) than
    /// organic bonus because high entropy can coexist with some wash trading if the
    /// wash patterns themselves are unpredictable.
    pub mesa_entropy_max_wash: f32,

    /// Minimum volume scale for normalization (scientific notation: 1e-4 = 0.0001)
    ///
    /// Floor value for volume normalization to prevent division by zero and
    /// stabilize scores when volume is near zero.
    ///
    /// **Default: 1e-4 (0.0001)**
    ///
    /// **Range: [1e-5, 1e-3]**
    ///
    /// **Rationale**: Pump.fun launches with < 0.0001 SOL volume are effectively
    /// dead on arrival. This floor prevents numerical instability without affecting
    /// meaningful signals.
    pub min_volume_scale: f64,

    /// Relative factor cap for burst normalization
    ///
    /// Maximum multiplier for volume burst normalization. Caps extreme spikes to
    /// prevent single-transaction manipulation of scores.
    ///
    /// **Default: 2.0**
    ///
    /// **Range: [1.5, 5.0]**
    ///
    /// **Rationale**: Factor of 2.0 allows a 2x burst to have full impact while
    /// capping 10x+ bursts (which are likely manipulative or anomalous).
    pub relative_factor_cap: f64,

    /// Burst normalization divisor
    ///
    /// Divides relative burst factor before applying to score. Controls sensitivity
    /// to volume spikes.
    ///
    /// **Default: 2.0**
    ///
    /// **Range: [1.5, 3.0]**
    ///
    /// **Rationale**: Dividing by 2.0 halves the burst impact. A 2x burst becomes
    /// 1.0x multiplier (neutral), 4x burst becomes 2x multiplier, etc. This
    /// dampens burst volatility while preserving signal.
    pub burst_normalization: f64,

    /// Risk assessment thresholds
    ///
    /// Configurable boundaries for mapping SurvivorScore confidence and final score
    /// to risk levels (Low, Medium, High, VeryHigh).
    ///
    /// **Defaults**:
    /// - `very_high_confidence`: 0.5 (confidence < 50% → VeryHigh risk)
    /// - `high_confidence`: 0.7 (confidence < 70% → High risk)
    /// - `medium_score`: 60 (score < 60 → Medium risk if confidence OK)
    ///
    /// See [`RiskThresholds`] documentation for detailed rationale.
    pub risk_thresholds: RiskThresholds,

    /// Thresholds for trend detection and followup scoring
    ///
    /// These parameters control when follow-up scoring loop triggers penalties
    /// during observation cycles (S1-S13). They detect MCI degradation and
    /// QEDD survival drops that indicate token quality deterioration.
    ///
    /// **Defaults**:
    /// - `mci_drop_threshold`: 0.35 (was hardcoded 0.50)
    /// - `qedd_survival_drop_pct`: 0.50 (was hardcoded 0.30)
    /// - `enable_followup_penalties`: true
    pub followup_scoring: FollowupScoringConfig,

    /// Thresholds for survivor score calculation
    ///
    /// These parameters control when SurvivorScore applies instant penalties
    /// for low survival, quality, or liquidity. They also set MESA wash trading
    /// detection thresholds and wallet quality scoring parameters.
    ///
    /// **Defaults**:
    /// - `min_survival_threshold`: 0.35 (was hardcoded 0.4)
    /// - `min_quality_threshold`: 0.35 (was hardcoded 0.4)
    /// - `min_ligma_threshold`: 0.35 (was hardcoded 0.4)
    /// - `wallet_quality_threshold`: 0.6 (was hardcoded)
    pub survivor_thresholds: SurvivorScoreThresholds,

    /// Risk multipliers and penalty factors
    ///
    /// These parameters control the strength of various risk penalties and
    /// multipliers applied during scoring calculations.
    ///
    /// **Defaults**:
    /// - `exit_signal_weight`: 0.5 (was hardcoded)
    /// - `crash_risk_factor`: 0.5 (was hardcoded)
    /// - `anomaly_penalty_factor`: 0.5 (was hardcoded)
    pub risk_multipliers: RiskMultipliers,

    /// Orchestrator-specific thresholds
    ///
    /// These parameters control veto/interpretation thresholds in the orchestrator.
    ///
    /// **Defaults**:
    /// - `cabal_risk_threshold`: 0.65 (was CABAL_RISK_THRESHOLD constant)
    /// - `mesa_interpretation_bot_threshold`: 0.75 (was MESA_INTERPRETATION_BOT_THRESHOLD)
    /// - `mesa_interpretation_organic_threshold`: 0.70 (was MESA_INTERPRETATION_ORGANIC_THRESHOLD)
    pub orchestrator_thresholds: OrchestratorThresholds,
}

/// Orchestrator-specific thresholds
///
/// Controls veto triggers and interpretation thresholds in the orchestrator module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorThresholds {
    /// Cabal risk threshold for cluster hunter veto
    ///
    /// **Default: 0.65** (was CABAL_RISK_THRESHOLD constant)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If ClusterHunter detects cabal risk above this, token is vetoed immediately.
    pub cabal_risk_threshold: f32,

    /// MESA bot pattern threshold for interpretation
    ///
    /// **Default: 0.75** (was MESA_INTERPRETATION_BOT_THRESHOLD)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Used for display/logging of bot-dominated launches.
    pub mesa_interpretation_bot_threshold: f32,

    /// MESA organic activity threshold for interpretation
    ///
    /// **Default: 0.70** (was MESA_INTERPRETATION_ORGANIC_THRESHOLD)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Used for display/logging of organic launches.
    pub mesa_interpretation_organic_threshold: f32,
}

impl Default for OrchestratorThresholds {
    fn default() -> Self {
        Self {
            cabal_risk_threshold: 0.65,
            mesa_interpretation_bot_threshold: 0.75,
            mesa_interpretation_organic_threshold: 0.70,
        }
    }
}

impl OrchestratorThresholds {
    /// Validate configuration values are within acceptable ranges
    pub fn validate(&self) -> Result<()> {
        let thresholds = [
            ("cabal_risk_threshold", self.cabal_risk_threshold),
            (
                "mesa_interpretation_bot_threshold",
                self.mesa_interpretation_bot_threshold,
            ),
            (
                "mesa_interpretation_organic_threshold",
                self.mesa_interpretation_organic_threshold,
            ),
        ];

        for (name, value) in &thresholds {
            if !(0.0..=1.0).contains(value) {
                bail!("{} must be in [0.0, 1.0], got {}", name, value);
            }
        }

        Ok(())
    }
}

/// Thresholds for trend detection and followup scoring
///
/// Controls when follow-up scoring triggers corrections based on
/// MCI drops and QEDD survival degradation during observation cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupScoringConfig {
    /// MCI drop threshold - if MCI falls below this during cycles, apply penalty
    ///
    /// **Default: 0.35** (was hardcoded 0.50)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// **Lower values (0.2-0.3)**: More forgiving, allows market coherence to drop
    ///
    /// **Higher values (0.5-0.6)**: Stricter, kills tokens faster when coherence degrades
    ///
    /// **Rationale**: MCI=0.50 was too strict, causing S9-S12 score crashes from 66→28pts.
    /// Lowering to 0.35 allows natural volatility while still catching true coherence loss.
    pub mci_drop_threshold: f32,

    /// QEDD survival drop percentage - if survival drops by this % between cycles, apply penalty
    ///
    /// **Default: 0.50** (50%, was hardcoded 0.30)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// **Lower values (0.2-0.3)**: Less tolerant of survival degradation
    ///
    /// **Higher values (0.5-0.7)**: More tolerant, allows natural volatility
    ///
    /// **Example**: Cycle S8 survival=0.80 → S9 survival=0.50 → drop=(0.80-0.50)/0.80=37.5%
    /// With threshold 0.50 (50%): PASS. With threshold 0.30 (30%): FAIL (false positive).
    ///
    /// **Rationale**: 0.30 was causing false positives on natural volatility. 0.50 allows
    /// reasonable drops while catching true death spirals.
    pub qedd_survival_drop_pct: f32,

    /// Enable followup penalties (can disable for testing)
    ///
    /// **Default: true**
    ///
    /// When false, follow-up scoring runs but doesn't apply penalties.
    /// Useful for A/B testing and debugging scoring behavior.
    pub enable_followup_penalties: bool,
}

impl Default for FollowupScoringConfig {
    fn default() -> Self {
        Self {
            mci_drop_threshold: 0.35,
            qedd_survival_drop_pct: 0.50,
            enable_followup_penalties: true,
        }
    }
}

impl FollowupScoringConfig {
    /// Validate configuration values are within acceptable ranges
    pub fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.mci_drop_threshold) {
            bail!(
                "mci_drop_threshold must be in [0.0, 1.0], got {}",
                self.mci_drop_threshold
            );
        }
        if !(0.0..=1.0).contains(&self.qedd_survival_drop_pct) {
            bail!(
                "qedd_survival_drop_pct must be in [0.0, 1.0], got {}",
                self.qedd_survival_drop_pct
            );
        }
        Ok(())
    }
}

/// Thresholds for survivor score calculation
///
/// Controls when SurvivorScore applies instant penalties for low survival,
/// quality, or liquidity. Also configures MESA wash trading detection and
/// wallet quality scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorScoreThresholds {
    /// Minimum survival score to avoid penalty
    ///
    /// **Default: 0.35** (was hardcoded 0.4)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If survival component < this threshold, token gets "⚠️ Rug risk" flag.
    pub min_survival_threshold: f32,

    /// Minimum quality score to avoid penalty
    ///
    /// **Default: 0.35** (was hardcoded 0.4)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If quality component < this threshold, token gets "🤖 Bot dominated" flag.
    pub min_quality_threshold: f32,

    /// Minimum LIGMA tradability to avoid penalty
    ///
    /// **Default: 0.35** (was hardcoded 0.4)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If LIGMA tradability < this threshold, token gets "⚠️ Low tradability" flag.
    pub min_ligma_threshold: f32,

    /// Wallet quality threshold (for quality_from_wallets bonus calculation)
    ///
    /// **Default: 0.6** (was hardcoded in survivor_score.rs line 448)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If unique_wallet_ratio > this threshold, apply wallet quality multiplier.
    /// Below this, no wallet quality bonus is applied.
    pub wallet_quality_threshold: f32,

    /// Wash trading threshold for risk penalty
    ///
    /// **Default: 0.6** (was hardcoded in survivor_score.rs line 448)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// If MESA wash_likeness > this threshold, apply wash trading penalty.
    pub wash_trading_threshold: f32,

    /// MESA wash trading severe threshold
    ///
    /// **Default: 0.85** (was hardcoded in orchestrator.rs as part of MESA interpretation)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Wash likeness above this triggers severe penalties and VeryHigh risk escalation.
    pub mesa_wash_severe: f32,

    /// MESA wash trading elevated threshold
    ///
    /// **Default: 0.70** (was hardcoded in orchestrator.rs as MESA_INTERPRETATION_WASH_THRESHOLD)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Wash likeness above this triggers moderate penalties.
    pub mesa_wash_elevated: f32,
}

impl Default for SurvivorScoreThresholds {
    fn default() -> Self {
        Self {
            min_survival_threshold: 0.35,
            min_quality_threshold: 0.35,
            min_ligma_threshold: 0.35,
            wallet_quality_threshold: 0.6,
            wash_trading_threshold: 0.6,
            mesa_wash_severe: 0.85,
            mesa_wash_elevated: 0.70,
        }
    }
}

impl SurvivorScoreThresholds {
    /// Validate configuration values are within acceptable ranges
    pub fn validate(&self) -> Result<()> {
        let thresholds = [
            ("min_survival_threshold", self.min_survival_threshold),
            ("min_quality_threshold", self.min_quality_threshold),
            ("min_ligma_threshold", self.min_ligma_threshold),
            ("wallet_quality_threshold", self.wallet_quality_threshold),
            ("wash_trading_threshold", self.wash_trading_threshold),
            ("mesa_wash_severe", self.mesa_wash_severe),
            ("mesa_wash_elevated", self.mesa_wash_elevated),
        ];

        for (name, value) in &thresholds {
            if !(0.0..=1.0).contains(value) {
                bail!("{} must be in [0.0, 1.0], got {}", name, value);
            }
        }

        Ok(())
    }
}

/// Risk multipliers and penalty factors
///
/// Controls the strength of various risk penalties applied during scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMultipliers {
    /// Exit signal penalty weight
    ///
    /// **Default: 0.5** (was hardcoded in survivor_score.rs line 467)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Weight applied to exit signal risk in risk discount calculation.
    /// Higher = stronger penalty for smart money exits.
    pub exit_signal_weight: f32,

    /// Crash risk discount factor
    ///
    /// **Default: 0.5** (was hardcoded in various calculations)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Factor applied to crash risk penalties.
    pub crash_risk_factor: f32,

    /// Anomaly penalty factor
    ///
    /// **Default: 0.5** (was hardcoded in various calculations)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Factor applied to volume/pattern anomaly penalties.
    pub anomaly_penalty_factor: f32,

    /// Wallet quality multiplier
    ///
    /// **Default: 0.5** (was hardcoded in survivor_score.rs line 448)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Multiplier applied to wallet ratio when calculating quality bonus.
    /// If wallet_ratio > wallet_quality_threshold, quality = wallet_ratio * this multiplier.
    pub wallet_quality_multiplier: f32,

    /// Wash trading penalty multiplier
    ///
    /// **Default: 0.5** (was hardcoded in survivor_score.rs line 475)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Multiplier applied to wash likeness when calculating wash penalty.
    /// If wash_likeness > wash_threshold, penalty = wash_likeness * this multiplier.
    pub wash_penalty_multiplier: f32,
}

impl Default for RiskMultipliers {
    fn default() -> Self {
        Self {
            exit_signal_weight: 0.5,
            crash_risk_factor: 0.5,
            anomaly_penalty_factor: 0.5,
            wallet_quality_multiplier: 0.5,
            wash_penalty_multiplier: 0.5,
        }
    }
}

impl RiskMultipliers {
    /// Validate configuration values are within acceptable ranges
    pub fn validate(&self) -> Result<()> {
        let multipliers = [
            ("exit_signal_weight", self.exit_signal_weight),
            ("crash_risk_factor", self.crash_risk_factor),
            ("anomaly_penalty_factor", self.anomaly_penalty_factor),
            ("wallet_quality_multiplier", self.wallet_quality_multiplier),
            ("wash_penalty_multiplier", self.wash_penalty_multiplier),
        ];

        for (name, value) in &multipliers {
            if !(0.0..=1.0).contains(value) {
                bail!("{} must be in [0.0, 1.0], got {}", name, value);
            }
        }

        Ok(())
    }
}

impl HyperPredictionConfig {
    /// Load HyperPredictionConfig from GhostBrainConfig
    ///
    /// Extracts the `[hyper_prediction]` section from the unified config.
    /// Falls back to defaults if section is missing (backward compatibility).
    ///
    /// ## Errors
    ///
    /// Returns error if config values are present but invalid (e.g., out of range).
    /// Missing config is NOT an error - defaults are used instead.
    pub fn from_config(config: &GhostBrainConfig) -> Result<Self> {
        // Try to extract hyper_prediction config, use defaults if missing
        let mut hp_config = config
            .hyper_prediction
            .as_ref()
            .cloned()
            .unwrap_or_else(Self::default);

        // Align orchestrator thresholds with Gatekeeper config
        hp_config.gatekeeper_min_tx_count = config.gatekeeper.min_tx_count.max(1);

        // Validate extracted/default config
        hp_config
            .validate()
            .context("Invalid HyperPrediction configuration")?;

        Ok(hp_config)
    }

    /// Validate all configuration values are within expected ranges
    ///
    /// This provides fail-fast feedback if config values are misconfigured.
    /// Validation catches:
    /// - Out-of-range values (e.g., threshold > 1.0)
    /// - Illogical relationships (e.g., moderate_threshold > severe_threshold)
    /// - Dangerous extremes (e.g., critical_threshold = 100)
    ///
    /// ## Errors
    ///
    /// Returns detailed error message indicating which parameter is invalid and why.
    pub fn validate(&self) -> Result<()> {
        if self.gatekeeper_min_tx_count == 0 || self.gatekeeper_min_tx_count > 500 {
            bail!(
                "gatekeeper_min_tx_count must be in [1, 500], got {}",
                self.gatekeeper_min_tx_count
            );
        }

        if !(1.0..=3.0).contains(&self.early_stage_multiplier) {
            bail!(
                "early_stage_multiplier must be in [1.0, 3.0], got {}",
                self.early_stage_multiplier
            );
        }

        // Validate SurvivorScore thresholds
        if self.survivor_critical_threshold > 100 {
            bail!(
                "survivor_critical_threshold must be <= 100, got {}",
                self.survivor_critical_threshold
            );
        }

        if !(0..=30).contains(&self.qass_secondary_max_adjustment) {
            bail!(
                "qass_secondary_max_adjustment must be in [0, 30], got {}",
                self.qass_secondary_max_adjustment
            );
        }

        if !(0.0..=1.0).contains(&self.qass_min_confidence_for_modifier) {
            bail!(
                "qass_min_confidence_for_modifier must be in [0.0, 1.0], got {}",
                self.qass_min_confidence_for_modifier
            );
        }

        // Validate Cold Start parameters
        if !(0.0..=0.5).contains(&self.cold_start_max_adjustment) {
            bail!(
                "cold_start_max_adjustment must be in [0.0, 0.5], got {}",
                self.cold_start_max_adjustment
            );
        }

        if !(1.0..=20.0).contains(&self.cold_start_qedd_mci_weight) {
            bail!(
                "cold_start_qedd_mci_weight must be in [1.0, 20.0], got {}",
                self.cold_start_qedd_mci_weight
            );
        }

        // Validate MESA thresholds (all must be in [0.0, 1.0])
        let mesa_thresholds = [
            (
                "mesa_wash_severe_threshold",
                self.mesa_wash_severe_threshold,
            ),
            (
                "mesa_wash_elevated_threshold",
                self.mesa_wash_elevated_threshold,
            ),
            ("mesa_bot_high_threshold", self.mesa_bot_high_threshold),
            (
                "mesa_bot_moderate_threshold",
                self.mesa_bot_moderate_threshold,
            ),
            (
                "mesa_organic_bonus_threshold",
                self.mesa_organic_bonus_threshold,
            ),
            ("mesa_organic_max_wash", self.mesa_organic_max_wash),
            (
                "mesa_entropy_bonus_threshold",
                self.mesa_entropy_bonus_threshold,
            ),
            ("mesa_entropy_max_wash", self.mesa_entropy_max_wash),
        ];

        for (name, value) in &mesa_thresholds {
            if !(0.0..=1.0).contains(value) {
                bail!("{} must be in [0.0, 1.0], got {}", name, value);
            }
        }

        // Validate MESA threshold relationships
        if self.mesa_wash_elevated_threshold > self.mesa_wash_severe_threshold {
            bail!(
                "mesa_wash_elevated_threshold ({}) must be <= mesa_wash_severe_threshold ({})",
                self.mesa_wash_elevated_threshold,
                self.mesa_wash_severe_threshold
            );
        }

        if self.mesa_bot_moderate_threshold > self.mesa_bot_high_threshold {
            bail!(
                "mesa_bot_moderate_threshold ({}) must be <= mesa_bot_high_threshold ({})",
                self.mesa_bot_moderate_threshold,
                self.mesa_bot_high_threshold
            );
        }

        // Validate scoring normalization parameters
        if !(1e-5..=1e-3).contains(&self.min_volume_scale) {
            bail!(
                "min_volume_scale must be in [1e-5, 1e-3], got {}",
                self.min_volume_scale
            );
        }

        if !(1.5..=5.0).contains(&self.relative_factor_cap) {
            bail!(
                "relative_factor_cap must be in [1.5, 5.0], got {}",
                self.relative_factor_cap
            );
        }

        if !(1.5..=3.0).contains(&self.burst_normalization) {
            bail!(
                "burst_normalization must be in [1.5, 3.0], got {}",
                self.burst_normalization
            );
        }

        // Validate risk thresholds (delegated to RiskThresholds struct)
        // Note: RiskThresholds doesn't have a validate() method yet, but we check basic sanity
        if !(0.0..=1.0).contains(&self.risk_thresholds.very_high_confidence) {
            bail!(
                "risk_thresholds.very_high_confidence must be in [0.0, 1.0], got {}",
                self.risk_thresholds.very_high_confidence
            );
        }

        if !(0.0..=1.0).contains(&self.risk_thresholds.high_confidence) {
            bail!(
                "risk_thresholds.high_confidence must be in [0.0, 1.0], got {}",
                self.risk_thresholds.high_confidence
            );
        }

        if self.risk_thresholds.very_high_confidence > self.risk_thresholds.high_confidence {
            bail!(
                "risk_thresholds.very_high_confidence ({}) must be <= high_confidence ({})",
                self.risk_thresholds.very_high_confidence,
                self.risk_thresholds.high_confidence
            );
        }

        if self.risk_thresholds.medium_score > 100 {
            bail!(
                "risk_thresholds.medium_score must be <= 100, got {}",
                self.risk_thresholds.medium_score
            );
        }

        // Validate new config sections
        self.followup_scoring
            .validate()
            .context("Invalid followup_scoring configuration")?;

        self.survivor_thresholds
            .validate()
            .context("Invalid survivor_thresholds configuration")?;

        self.risk_multipliers
            .validate()
            .context("Invalid risk_multipliers configuration")?;

        self.orchestrator_thresholds
            .validate()
            .context("Invalid orchestrator_thresholds configuration")?;

        Ok(())
    }
}

impl Default for HyperPredictionConfig {
    /// Default configuration based on empirical testing during Phase 4.5
    ///
    /// These values represent the production configuration as of the SurvivorScore
    /// integration. They are intentionally conservative (high bar for acceptance)
    /// to minimize false positives in the aggressive Ghost trading strategy.
    fn default() -> Self {
        Self {
            gatekeeper_min_tx_count: default_gatekeeper_min_tx_count(),
            early_stage_multiplier: default_early_stage_multiplier(),
            // SurvivorScore Configuration
            survivor_critical_threshold: 35,
            qass_secondary_max_adjustment: 10,
            qass_min_confidence_for_modifier: 0.6,

            // Cold Start Configuration
            cold_start_max_adjustment: 0.3,
            cold_start_qedd_mci_weight: 10.0,

            // MESA Microstructure Thresholds
            mesa_wash_severe_threshold: 0.85,
            mesa_wash_elevated_threshold: 0.70,
            mesa_bot_high_threshold: 0.90,
            mesa_bot_moderate_threshold: 0.75,
            mesa_organic_bonus_threshold: 0.75,
            mesa_organic_max_wash: 0.40,
            mesa_entropy_bonus_threshold: 0.80,
            mesa_entropy_max_wash: 0.50,

            // Scoring Normalization
            min_volume_scale: 1e-4,
            relative_factor_cap: 2.0,
            burst_normalization: 2.0,

            // Risk Thresholds
            risk_thresholds: RiskThresholds::default(),

            // Followup Scoring Configuration
            followup_scoring: FollowupScoringConfig::default(),

            // Survivor Score Thresholds
            survivor_thresholds: SurvivorScoreThresholds::default(),

            // Risk Multipliers
            risk_multipliers: RiskMultipliers::default(),

            // Orchestrator Thresholds
            orchestrator_thresholds: OrchestratorThresholds::default(),
        }
    }
}

fn default_gatekeeper_min_tx_count() -> usize {
    15
}

fn default_early_stage_multiplier() -> f32 {
    1.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = HyperPredictionConfig::default();
        assert!(
            config.validate().is_ok(),
            "Default config should pass validation"
        );
    }

    #[test]
    fn test_survivor_threshold_validation() {
        let mut config = HyperPredictionConfig::default();
        config.survivor_critical_threshold = 101;
        assert!(
            config.validate().is_err(),
            "survivor_critical_threshold > 100 should fail"
        );
    }

    #[test]
    fn test_qass_adjustment_validation() {
        let mut config = HyperPredictionConfig::default();
        config.qass_secondary_max_adjustment = 50;
        assert!(
            config.validate().is_err(),
            "qass_secondary_max_adjustment > 30 should fail"
        );
    }

    #[test]
    fn test_mesa_threshold_ordering() {
        let mut config = HyperPredictionConfig::default();
        config.mesa_wash_elevated_threshold = 0.90;
        config.mesa_wash_severe_threshold = 0.80;
        assert!(
            config.validate().is_err(),
            "elevated > severe threshold should fail"
        );
    }

    #[test]
    fn test_cold_start_weight_bounds() {
        let mut config = HyperPredictionConfig::default();
        config.cold_start_qedd_mci_weight = 25.0;
        assert!(
            config.validate().is_err(),
            "cold_start_qedd_mci_weight > 20.0 should fail"
        );
    }

    #[test]
    fn test_risk_threshold_ordering() {
        let mut config = HyperPredictionConfig::default();
        config.risk_thresholds.very_high_confidence = 0.8;
        config.risk_thresholds.high_confidence = 0.6;
        assert!(
            config.validate().is_err(),
            "very_high_confidence > high_confidence should fail"
        );
    }
}
