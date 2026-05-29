//! Ghost Brain Unified Configuration
//!
//! This module provides a comprehensive configuration structure for all Ghost Brain
//! components, allowing fine-tuning of the entire system from a single source.
//!
//! ## Components Covered
//!
//! - **MPCF** (Micro-Payload Cognitive Fingerprint): Actor-behavioral byte fingerprinting
//! - **SSMI** (Sub-Slot Microentropy Index): Transaction timing jitter analysis
//! - **IWIM** (Initial Wallet Intent Mapping): Dev-wallet behavioral analysis
//! - **QASS** (Quantum Amplitude Superposition Scoring): Signal aggregation engine
//! - **SOBP** (Slot-Over-Slot Buying Pressure): Buying pressure analysis
//! - **QOFSV** (Quantum Order-Flow State Vector): Quantum state vector mapping
//! - **FRB** (Frequency Resonance Bands): Frequency-based signal analysis
//! - **Resonance**: Bot detection via coefficient of variation
//! - **MCI** (Market Coherence Index): Directional and structural coherence
//! - **QEDD** (Quantum Entropy-Driven Decay): Survival probability analysis
//! - **Confidence**: Confidence scoring for Oracle decisions
//!
//! ## Usage
//!
//! ```rust,ignore
//! use ghost_brain::config::GhostBrainConfig;
//!
//! // Load default configuration
//! let config = GhostBrainConfig::default();
//!
//! // Load from JSON file
//! let config = GhostBrainConfig::from_json_file("config.json")?;
//!
//! // Load from TOML file
//! let config = GhostBrainConfig::from_toml_file("config.toml")?;
//!
//! // Customize specific components
//! let mut config = GhostBrainConfig::default();
//! config.mpcf.bot_entropy_threshold = 4.0;
//! config.sobp.hyper_pump_threshold = 3.5;
//! ```

use ghost_core::shadow_ledger::ShadowLedgerStaleFallback;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::warn;

use crate::config::gatekeeper_v25_config::{
    AdaptiveProsperityConfig, DynamicObservationWindowConfig, GatekeeperV25RolloutConfig,
    PumpAndDumpDetectorConfig, TrajectoryAwareScoringConfig,
};
use crate::config::gatekeeper_v3_config::GatekeeperV3Config;
use crate::config::mci_config::MciConfig;
use crate::config::qedd_config::QeddConfig;
use crate::guardian::post_buy::PostBuyGuardianConfig;
use crate::oracle::hyper_prediction::HyperPredictionConfig;

/// Unified configuration for all Ghost Brain components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostBrainConfig {
    /// Configuration version for API compatibility
    pub version: u8,

    /// Engine configuration (cycle timing, etc.)
    #[serde(default)]
    pub engine: EngineConfig,

    /// Gatekeeper configuration (pre-flight filter) [LEGACY - deprecated]
    #[serde(default)]
    pub gatekeeper: GatekeeperConfig,

    /// Gatekeeper V2 configuration (6-phase analytical filter)
    /// If present in TOML, overrides hardcoded defaults.
    #[serde(default)]
    pub gatekeeper_v2: Option<GatekeeperV2Config>,

    /// Gatekeeper V3 calibrated shadow sidecar configuration.
    #[serde(default)]
    pub gatekeeper_v3: GatekeeperV3Config,

    /// FSC v2 capture/evidence configuration.
    ///
    /// This is intentionally separate from legacy Gatekeeper V2 FSC thresholds
    /// so the new evidence payload cannot silently change active policy meaning.
    #[serde(default)]
    pub fsc_v2: FscV2Config,

    /// MPCF (Micro-Payload Cognitive Fingerprint) configuration
    pub mpcf: MpcfConfig,

    /// SSMI (Sub-Slot Microentropy Index) configuration
    pub ssmi: SsmiConfig,

    /// IWIM (Initial Wallet Intent Mapping) configuration
    pub iwim: IwimConfig,

    /// QASS (Quantum Amplitude Superposition Scoring) configuration
    pub qass: QassConfig,

    /// SOBP (Slot-Over-Slot Buying Pressure) configuration
    pub sobp: SobpConfig,

    /// QOFSV (Quantum Order-Flow State Vector) configuration
    pub qofsv: QofsvConfig,

    /// FRB (Frequency Resonance Bands) configuration
    pub frb: FrbConfig,

    /// Resonance detection configuration
    pub resonance: ResonanceConfig,

    /// MCI (Market Coherence Index) configuration
    #[serde(default)]
    pub mci: MciConfig,

    /// QEDD (Quantum Entropy-Driven Decay) configuration
    #[serde(default)]
    pub qedd: QeddConfig,

    /// Confidence Model configuration
    #[serde(default)]
    pub confidence: ConfidenceConfig,

    /// Normalization configuration (Operation Scale Master)
    #[serde(default)]
    pub normalization: NormalizationConfig,

    /// LIGMA (Liquidity Genesis Manifold Analyzer) configuration
    #[serde(default)]
    pub ligma: LigmaConfig,

    /// FRE (Fractal Resonance Engine) configuration
    #[serde(default)]
    pub fre: FreConfig,

    /// TCF (Trend Cohesion Field) configuration
    #[serde(default)]
    pub tcf: TcfConfig,

    /// BVA (Behavioral Vacuum Analysis) configuration
    #[serde(default)]
    pub bva: BvaConfig,

    /// PANIC (Heuristic congestion detector) configuration
    #[serde(default)]
    pub panic: PanicConfig,

    /// TCR-Φ (Temporal Causality Resonance) configuration
    #[serde(default)]
    pub tcr_phi: TcrPhiConfig,

    /// Behavioral scoring configuration (ECTO/BVA/PANIC/TCR/CIR)
    #[serde(default)]
    pub behavioral_scoring: BehavioralScoringConfig,

    /// HyperPrediction Oracle configuration
    #[serde(default)]
    pub hyper_prediction: Option<HyperPredictionConfig>,

    /// Scoring Weights configuration for penalties and boosts
    #[serde(default)]
    pub scoring: Option<ScoringWeightsConfig>,

    /// Survivor Score configuration (Section 9)
    /// Controls the weights and component configurations for SurvivorScore calculation
    #[serde(default)]
    pub survivor_score: Option<SurvivorScoreComponentConfig>,

    /// Cycle Weights configuration (Section 10)
    /// Controls the exponential weights for scoring cycles S1-S12
    #[serde(default)]
    pub cycle_weights: Option<CycleWeightsConfig>,

    /// Gunshot Thresholds configuration (Section 10)
    /// Controls the immediate buy trigger thresholds for each scoring cycle
    #[serde(default)]
    pub gunshot_thresholds: Option<GunshotThresholdsConfig>,

    /// Paradox Sensor configuration (HFT/paradox gate)
    #[serde(default)]
    pub paradox: Option<ParadoxConfig>,

    /// PostBuy Guardian configuration (real-time position monitoring)
    #[serde(default)]
    pub post_buy_guardian: PostBuyGuardianConfig,

    /// IWIM Veto Gate configuration (post-Gatekeeper dev history check)
    #[serde(default)]
    pub iwim_veto_gate: IwimVetoGateConfig,
}

// ═══════════════════════════════════════════════════════════════════════════════
// IWIM Veto Gate Configuration
// ═══════════════════════════════════════════════════════════════════════════════

/// Feed mode for IWIM dev history fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IwimFeedMode {
    /// PumpPortal WebSocket mode — fetches 30 TX from dev history.
    Pp,
    /// Yellowstone gRPC mode — fetches 150 TX from dev history.
    Grpc,
}

impl Default for IwimFeedMode {
    fn default() -> Self {
        Self::Pp
    }
}

impl std::fmt::Display for IwimFeedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pp => write!(f, "PP"),
            Self::Grpc => write!(f, "GRPC"),
        }
    }
}

/// IWIM Veto Gate — post-Gatekeeper dev history verification.
///
/// Acts as an independent "last veto" after the 10s Gatekeeper window.
/// Only runs for candidates that passed Gatekeeper with BUY verdict.
/// Uses dev wallet transaction history to detect serial ruggers, sybil clusters,
/// and other patterns invisible within the 10s observation window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IwimVetoGateConfig {
    /// Master switch: enable/disable IWIM veto gate.
    /// Default: false
    #[serde(default)]
    pub enabled: bool,

    /// Feed mode: "pp" (PumpPortal, N=30) or "grpc" (Yellowstone, N=150).
    /// Default: pp
    #[serde(default)]
    pub mode: IwimFeedMode,

    /// Total time budget for IWIM check (ms). Includes primary + fallback attempts.
    /// Default: 500
    #[serde(default = "default_iwim_veto_max_wait_ms")]
    pub max_wait_ms: u64,

    /// Primary RPC URL for dev history fetch.
    /// If empty, uses the global RPC_URL env var.
    #[serde(default)]
    pub primary_rpc_url: String,

    /// Fallback RPC URL for retry on primary failure.
    /// If empty, no fallback is attempted.
    #[serde(default)]
    pub fallback_rpc_url: String,

    // ── Confidence / quality gates ──────────────────────────────────────────
    /// Minimum IWIM confidence to honour a VETO.
    /// Below this, IWIM result is downgraded to UNKNOWN (no block).
    /// Default: 0.60
    #[serde(default = "default_iwim_veto_min_confidence")]
    pub min_confidence: f32,

    /// Minimum analyzed TX count for PP mode to be iwim_quality=HIGH.
    /// Default: 10
    #[serde(default = "default_iwim_veto_min_tx_pp")]
    pub min_tx_pp: usize,

    /// Minimum analyzed TX count for GRPC mode to be iwim_quality=HIGH.
    /// Default: 20
    #[serde(default = "default_iwim_veto_min_tx_grpc")]
    pub min_tx_grpc: usize,

    // ── Veto thresholds ────────────────────────────────────────────────────
    /// Rug threat score threshold: VETO if rug_threat_score >= this.
    /// Default: 0.70 (lowered for initial testing, production ~0.80)
    #[serde(default = "default_iwim_veto_rug_threshold")]
    pub rug_threat_threshold: f32,

    /// Sybil score threshold: VETO if sybil_score >= this.
    /// Default: 0.70
    #[serde(default = "default_iwim_veto_sybil_threshold")]
    pub sybil_threshold: f32,

    /// Organic score floor: VETO if organic_score <= this.
    /// Default: 0.15 (extremely low organic = almost certainly bot/scam)
    #[serde(default = "default_iwim_veto_organic_floor")]
    pub organic_floor: f32,

    // ── Gatekeeper strength classification ─────────────────────────────────
    /// Soft-point margin for STRONG classification.
    /// STRONG if soft_points <= (effective_max_soft_points - this).
    /// Default: 3
    #[serde(default = "default_iwim_veto_strong_margin")]
    pub strong_margin: u8,

    /// Maximum manipulation flags for STRONG classification.
    /// If any manipulation soft flag is raised → BORDERLINE.
    /// Default: 0
    #[serde(default)]
    pub strong_max_manipulation_flags: u8,
}

impl Default for IwimVetoGateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: IwimFeedMode::Pp,
            max_wait_ms: 500,
            primary_rpc_url: String::new(),
            fallback_rpc_url: String::new(),
            min_confidence: 0.60,
            min_tx_pp: 10,
            min_tx_grpc: 20,
            rug_threat_threshold: 0.70,
            sybil_threshold: 0.70,
            organic_floor: 0.15,
            strong_margin: 3,
            strong_max_manipulation_flags: 0,
        }
    }
}

fn default_iwim_veto_max_wait_ms() -> u64 {
    500
}
fn default_iwim_veto_min_confidence() -> f32 {
    0.60
}
fn default_iwim_veto_min_tx_pp() -> usize {
    10
}
fn default_iwim_veto_min_tx_grpc() -> usize {
    20
}
fn default_iwim_veto_rug_threshold() -> f32 {
    0.70
}
fn default_iwim_veto_sybil_threshold() -> f32 {
    0.70
}
fn default_iwim_veto_organic_floor() -> f32 {
    0.15
}
fn default_iwim_veto_strong_margin() -> u8 {
    3
}
fn default_iwim_veto_strong_margin_gk() -> u8 {
    3
}

fn default_curve_wait_ms() -> u64 {
    800
}

fn default_curve_require_for_buy() -> bool {
    true
}

fn default_enable_alpha_gate() -> bool {
    false
}

fn default_min_momentum() -> f64 {
    0.55
}

fn default_min_demand() -> f64 {
    0.55
}

fn default_min_alpha_joint() -> f64 {
    0.35
}

fn default_min_alpha_sample() -> usize {
    15
}

/// Paradox Sensor configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParadoxConfig {
    /// Enable Paradox Sensor processing
    #[serde(default = "default_paradox_enabled")]
    pub enabled: bool,

    /// Analysis window size in milliseconds (mirror-time window)
    #[serde(default = "default_paradox_window_ms")]
    pub window_size_ms: u64,

    /// Background analysis interval in milliseconds
    #[serde(default = "default_paradox_analysis_interval_ms")]
    pub analysis_interval_ms: u64,

    /// Tension threshold for anomaly detection (0.0 - 100.0)
    #[serde(default = "default_paradox_anomaly_tension_threshold")]
    pub anomaly_tension_threshold: f64,

    /// Maximum samples retained for analysis
    #[serde(default = "default_paradox_max_samples")]
    pub max_samples: usize,

    /// Minimum samples required before performing analysis
    #[serde(default = "default_paradox_min_samples")]
    pub min_samples_for_analysis: usize,
}

const fn default_paradox_enabled() -> bool {
    true
}
const fn default_paradox_window_ms() -> u64 {
    500
}
const fn default_paradox_analysis_interval_ms() -> u64 {
    50
}
const fn default_paradox_anomaly_tension_threshold() -> f64 {
    80.0
}
const fn default_paradox_max_samples() -> usize {
    2000
}
const fn default_paradox_min_samples() -> usize {
    10
}

/// Engine configuration controlling scoring cycle timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Duration of a single scoring cycle in milliseconds (default: 800ms)
    #[serde(default = "default_cycle_duration_ms")]
    pub cycle_duration_ms: u64,
}

pub const fn default_cycle_duration_ms() -> u64 {
    800
}

// BVA defaults
const fn default_bva_primary_window_secs() -> u64 {
    3
}
const fn default_bva_slot_duration_ms() -> u64 {
    700
}
const fn default_bva_early_reaction_slots() -> u64 {
    2
}
const fn default_bva_weight_tds() -> f64 {
    0.25
}
const fn default_bva_weight_dc() -> f64 {
    0.20
}
const fn default_bva_weight_se() -> f64 {
    0.15
}
const fn default_bva_weight_cer() -> f64 {
    0.20
}
const fn default_bva_weight_erp() -> f64 {
    0.20
}
const fn default_bva_sobp_dc_blend() -> f64 {
    0.25
}
const fn default_bva_scr_prior_weight() -> f64 {
    0.35
}
const fn default_bva_cir_min_weight() -> f64 {
    0.15
}
const fn default_bva_confidence_tx_divisor() -> u64 {
    5
}
const fn default_bva_confidence_signer_divisor() -> u64 {
    3
}
const fn default_bva_confidence_time_divisor_ms() -> u64 {
    2000
}
const fn default_bva_sobp_fallback_confidence_penalty() -> f64 {
    0.15
}
const fn default_bva_congestion_confidence_penalty() -> f64 {
    0.10
}
const fn default_bva_classification_confidence_floor() -> f64 {
    0.30
}
const fn default_bva_classification_organic_tds_min() -> f64 {
    0.55
}
const fn default_bva_classification_organic_se_min() -> f64 {
    0.55
}
const fn default_bva_classification_organic_dc_min() -> f64 {
    0.35
}
const fn default_bva_classification_steered_dc_min() -> f64 {
    0.70
}
const fn default_bva_classification_steered_erp_min() -> f64 {
    0.60
}

// TCR-Φ defaults
const fn default_tcr_phi_window_slots() -> u64 {
    6
}
const fn default_tcr_phi_tempo_window() -> usize {
    12
}
const fn default_tcr_phi_min_samples() -> usize {
    2
}
const fn default_tcr_phi_confidence_samples() -> usize {
    6
}
const fn default_tcr_phi_default_slot_ms() -> f64 {
    400.0
}
const fn default_tcr_phi_min_confidence_emit() -> f64 {
    0.25
}
const fn default_tcr_phi_synergy_timing_threshold() -> f64 {
    0.75
}
const fn default_tcr_phi_synergy_bias_threshold() -> f64 {
    0.8
}
const fn default_tcr_phi_synergy_boost() -> f64 {
    1.25
}
const fn default_tcr_phi_max_impacts() -> usize {
    256
}
const fn default_tcr_phi_ecto_enabled() -> bool {
    true
}
const fn default_tcr_phi_ecto_k_phi() -> f64 {
    0.35
}
const fn default_tcr_phi_ecto_early_event_weight() -> f64 {
    1.2
}
const fn default_tcr_phi_ecto_early_window_slots() -> u64 {
    2
}

// Behavioral Scoring defaults
const fn default_behavioral_enabled() -> bool {
    true
}
const fn default_behavioral_w_ecto() -> f32 {
    0.30
}
const fn default_behavioral_w_bva() -> f32 {
    0.25
}
const fn default_behavioral_w_panic() -> f32 {
    0.20
}
const fn default_behavioral_w_tcr() -> f32 {
    0.15
}
const fn default_behavioral_w_cir() -> f32 {
    0.10
}
const fn default_behavioral_early_stage_seconds() -> u64 {
    15
}
const fn default_behavioral_min_floor() -> f32 {
    0.30
}
const fn default_behavioral_use_additive_mode() -> bool {
    true
}
const fn default_behavioral_max_adjustment_points() -> f32 {
    15.0
}
const fn default_behavioral_neutral_point() -> f32 {
    0.5
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cycle_duration_ms: default_cycle_duration_ms(),
        }
    }
}

/// MPCF (Micro-Payload Cognitive Fingerprint) Configuration
///
/// Controls actor-behavioral byte fingerprinting parameters for ultra-fast
/// classification of transaction sources (Bot vs Human vs Sybil).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpcfConfig {
    /// Entropy threshold for bot detection (low entropy = regular patterns)
    /// Default: 3.5
    /// Range: [0.0, 10.0]
    pub bot_entropy_threshold: f32,

    /// Entropy threshold for human detection (high entropy = chaotic)
    /// Default: 5.5
    /// Range: [0.0, 10.0]
    pub human_entropy_threshold: f32,

    /// Instruction spacing variance threshold for bot detection
    /// Default: 0.15
    /// Range: [0.0, 1.0]
    pub bot_iss_variance_threshold: f32,

    /// Instruction spacing variance threshold for human detection
    /// Default: 0.35
    /// Range: [0.0, 1.0]
    pub human_iss_variance_threshold: f32,

    /// Entropy threshold for sybil network detection
    /// Default: 3.5
    /// Range: [0.0, 10.0]
    pub sybil_entropy_threshold: f32,

    /// Minimum payload size in bytes for reliable analysis
    /// Default: 32
    pub min_payload_size: usize,

    /// Maximum payload size in bytes to analyze (DoS protection)
    /// Default: 4096
    pub max_payload_size: usize,

    /// Confidence for unknown/ambiguous classification
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    pub unknown_confidence: f32,

    /// Base confidence when payload is too small
    /// Default: 0.4
    /// Range: [0.0, 1.0]
    pub low_confidence_small_payload: f32,

    /// Threshold for "highly organic" classification (transaction count basis)
    /// Default: 0.7 (70% human)
    /// Range: [0.0, 1.0]
    #[serde(default = "default_mpcf_high_organic_threshold")]
    pub high_organic_threshold: f32,

    /// Threshold for "bot dominated" classification (transaction count basis)
    /// Default: 0.5 (50% bots)
    /// Range: [0.0, 1.0]
    #[serde(default = "default_mpcf_bot_dominated_threshold")]
    pub bot_dominated_threshold: f32,

    /// Base MPCF score for highly organic tokens
    /// Default: 1.5
    #[serde(default = "default_mpcf_high_organic_base")]
    pub high_organic_base: f32,

    /// Maximum MPCF boost for organic activity
    /// Default: 2.5
    #[serde(default = "default_mpcf_max_organic_boost")]
    pub max_organic_boost: f32,

    /// Minimum MPCF penalty for bot activity
    /// Default: 0.2
    #[serde(default = "default_mpcf_min_bot_penalty")]
    pub min_bot_penalty: f32,
}

const fn default_mpcf_high_organic_threshold() -> f32 {
    0.7
}

const fn default_mpcf_bot_dominated_threshold() -> f32 {
    0.5
}

const fn default_mpcf_high_organic_base() -> f32 {
    1.5
}

const fn default_mpcf_max_organic_boost() -> f32 {
    2.5
}

const fn default_mpcf_min_bot_penalty() -> f32 {
    0.2
}

/// SSMI (Sub-Slot Microentropy Index) Configuration
///
/// Controls sub-slot timing jitter analysis for classification of transaction
/// sources based on timing patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsmiConfig {
    /// SCR threshold for bot detection (probability above indicates bot)
    /// Default: 0.7
    /// Range: [0.0, 1.0]
    pub bot_scr_threshold: f32,

    /// AR correlation threshold for bot detection (high = predictable)
    /// Default: 0.8
    /// Range: [0.0, 1.0]
    pub bot_ar_threshold: f32,

    /// Entropy threshold for bot detection (low entropy = regular patterns)
    /// Default: 1.5
    /// Range: [0.0, 10.0]
    pub bot_entropy_threshold: f32,

    /// Entropy threshold for human detection (high entropy = chaotic)
    /// Default: 3.0
    /// Range: [0.0, 10.0]
    pub human_entropy_threshold: f32,

    /// AR correlation threshold for human detection (low = unpredictable)
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    pub human_ar_threshold: f32,

    /// SCR threshold for human detection (low SCR = organic)
    /// Default: 0.4
    /// Range: [0.0, 1.0]
    pub human_scr_threshold: f32,

    /// Minimum transaction count for viral launch detection
    /// Default: 6
    pub viral_min_tx_count: usize,

    /// Minimum entropy for viral launch detection
    /// Default: 2.5
    /// Range: [0.0, 10.0]
    pub viral_entropy_min: f32,

    /// Maximum entropy for viral launch detection
    /// Default: 4.0
    /// Range: [0.0, 10.0]
    pub viral_entropy_max: f32,

    /// SCR threshold for viral launch detection
    /// Default: 0.5
    /// Range: [0.0, 1.0]
    pub viral_scr_threshold: f32,

    /// Weight for entropy component in combined score
    /// Default: 0.35
    /// Range: [0.0, 1.0]
    pub score_weight_entropy: f32,

    /// Weight for SCR component in combined score
    /// Default: 0.40
    /// Range: [0.0, 1.0]
    pub score_weight_scr: f32,

    /// Weight for AR correlation component in combined score
    /// Default: 0.25
    /// Range: [0.0, 1.0]
    pub score_weight_ar: f32,

    /// Bonus applied to viral launch source type
    /// Default: 0.15
    /// Range: [0.0, 1.0]
    pub viral_score_bonus: f32,

    /// Bonus applied to human source type
    /// Default: 0.05
    /// Range: [0.0, 1.0]
    pub human_score_bonus: f32,

    /// Penalty applied to bot source type
    /// Default: 0.20
    /// Range: [0.0, 1.0]
    pub bot_score_penalty: f32,

    /// Number of histogram bins for entropy calculation
    /// Default: 64
    pub histogram_bins: usize,

    /// Maximum jitter in milliseconds for histogram normalization
    /// Default: 2000
    pub max_jitter_ms: u64,
}

/// DEPRECATED: Replaced by GatekeeperV2Config in ghost-launcher.
/// Retained for backward compatibility with existing config files.
#[deprecated(
    since = "3.0.0",
    note = "Use GatekeeperV2Config from ghost-launcher instead."
)]
/// Gatekeeper pre-flight configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatekeeperConfig {
    /// Minimum total transactions required to pass the gatekeeper
    pub min_tx_count: usize,
    /// Minimum unique signers required to avoid sybil/wash trading
    pub min_unique_signers: usize,
    /// Maximum wait time before making a pass/reject decision (milliseconds)
    pub max_wait_time_ms: u64,
    /// Early pass threshold for extremely active pools
    pub early_pass_tx_count: usize,
    /// Minimum volume (SOL) required to pass the gatekeeper
    #[serde(default)]
    pub min_volume_sol: f64,
    /// Minimum transaction count required to enable Scoring Engine (The "Second Gatekeeper")
    /// Defaults to min_tx_count if not set
    #[serde(default)]
    pub min_tx_count_for_scoring: Option<usize>,

    /// Maximum number of scoring cycles (default: 12)
    #[serde(default)]
    pub max_cycles: Option<u64>,

    /// Maximum observation cycles for history (default: 12)
    #[serde(default)]
    pub max_observation_cycles: Option<u64>,
}

/// Gatekeeper operating mode.
///
/// - `Standard`: Original reactive mode — Phase 1 triggers evaluation, re-eval on each
///   `re_eval_tx_interval` TX, early Buy/Reject possible before deadline.
/// - `Long`: Full-window accumulation mode — waits the entire `max_wait_time_ms`,
///   collects ALL transactions, then performs a single final evaluation at deadline.
///   No early decisions (Buy/Reject/hard-reject) before the timer expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatekeeperMode {
    Standard,
    Long,
}

impl Default for GatekeeperMode {
    fn default() -> Self {
        GatekeeperMode::Standard
    }
}

impl std::fmt::Display for GatekeeperMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatekeeperMode::Standard => write!(f, "standard"),
            GatekeeperMode::Long => write!(f, "long"),
        }
    }
}

/// FSC v2 capture/evidence configuration.
///
/// PR-FSC1 only introduces an inert config surface. Active decision use remains
/// rejected by validation until a later ADR and implementation phase explicitly
/// promote FSC v2 beyond capture/evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FscV2Config {
    /// Collect FSC v2 source evidence.
    pub capture_enabled: bool,
    /// Emit FSC v2 evidence into logs/datasets.
    pub feature_emit_enabled: bool,
    /// Reserved for a future policy ADR. Must remain false in PR-FSC1.
    pub decision_enabled: bool,
    /// Reserved for a future policy ADR. Must remain false in PR-FSC1.
    pub hard_reject_enabled: bool,

    /// Provider label for evidence provenance.
    pub provider: String,
    /// Emit decision-time snapshots when capture is enabled.
    pub snapshot_decision_time_enabled: bool,
    /// Emit eventual/postfill snapshots when capture is enabled.
    pub snapshot_eventual_enabled: bool,

    /// Funding lookback window for FSC v2 attribution.
    pub lookback_window_s: u64,
    /// Warmup window required before clean capture status.
    pub warmup_window_s: u64,

    /// Minimum native-SOL transfer amount retained in the rolling index.
    pub min_abs_store_lamports: u64,
    /// Minimum absolute native-SOL transfer amount used for attribution.
    pub min_abs_attribution_lamports: u64,
    /// Minimum transfer-to-buy ratio for attribution.
    pub min_rel_to_buy: f64,
    /// Dominant source confidence required for non-neutral known attribution.
    pub min_attribution_confidence: f64,

    /// Minimum total buyers before FSC v2 can be clean.
    pub min_total_buyers: u8,
    /// Minimum known non-neutral buyers before HHI is defined.
    pub min_known_non_neutral_buyers: u8,
    /// Minimum known coverage before clean status.
    pub min_known_coverage: f64,
    /// Minimum non-neutral known coverage before clean scoring status.
    pub min_non_neutral_known_coverage: f64,

    /// Same-slot cross-signature ordering policy.
    pub same_slot_cross_signature_policy: String,
    /// Include WSOL transfers in primary FSC v2. Must remain false for V1.
    pub include_wsol: bool,
    /// Include SPL transfers in primary FSC v2. Must remain false for V1.
    pub include_spl: bool,

    /// Optional neutral funder set file.
    pub neutral_funder_set_path: Option<String>,
    /// Optional neutral funder set version.
    pub neutral_funder_set_version: Option<String>,
}

impl Default for FscV2Config {
    fn default() -> Self {
        Self {
            capture_enabled: false,
            feature_emit_enabled: false,
            decision_enabled: false,
            hard_reject_enabled: false,
            provider: "nln_program_streams".to_string(),
            snapshot_decision_time_enabled: true,
            snapshot_eventual_enabled: true,
            lookback_window_s: 300,
            warmup_window_s: 300,
            min_abs_store_lamports: 1_000_000,
            min_abs_attribution_lamports: 10_000_000,
            min_rel_to_buy: 0.20,
            min_attribution_confidence: 0.60,
            min_total_buyers: 2,
            min_known_non_neutral_buyers: 2,
            min_known_coverage: 0.50,
            min_non_neutral_known_coverage: 0.30,
            same_slot_cross_signature_policy: "require_tx_index".to_string(),
            include_wsol: false,
            include_spl: false,
            neutral_funder_set_path: Some("configs/fsc/neutral_funders_v1.toml".to_string()),
            neutral_funder_set_version: Some("neutral_funders_v1".to_string()),
        }
    }
}

impl FscV2Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.decision_enabled {
            anyhow::bail!(
                "fsc_v2.decision_enabled is reserved for a future FSC policy ADR and must remain false"
            );
        }
        if self.hard_reject_enabled {
            anyhow::bail!(
                "fsc_v2.hard_reject_enabled is reserved for a future FSC policy ADR and must remain false"
            );
        }
        if self.provider.trim().is_empty() {
            anyhow::bail!("fsc_v2.provider must not be empty");
        }
        if self.lookback_window_s == 0 {
            anyhow::bail!("fsc_v2.lookback_window_s must be positive");
        }
        if self.warmup_window_s == 0 {
            anyhow::bail!("fsc_v2.warmup_window_s must be positive");
        }
        if self.min_abs_store_lamports == 0 {
            anyhow::bail!("fsc_v2.min_abs_store_lamports must be positive");
        }
        if self.min_abs_attribution_lamports == 0 {
            anyhow::bail!("fsc_v2.min_abs_attribution_lamports must be positive");
        }
        if self.min_abs_store_lamports > self.min_abs_attribution_lamports {
            anyhow::bail!("fsc_v2.min_abs_store_lamports must be <= min_abs_attribution_lamports");
        }
        validate_unit_interval("fsc_v2.min_rel_to_buy", self.min_rel_to_buy)?;
        validate_unit_interval(
            "fsc_v2.min_attribution_confidence",
            self.min_attribution_confidence,
        )?;
        validate_unit_interval("fsc_v2.min_known_coverage", self.min_known_coverage)?;
        validate_unit_interval(
            "fsc_v2.min_non_neutral_known_coverage",
            self.min_non_neutral_known_coverage,
        )?;
        if self.min_total_buyers == 0 {
            anyhow::bail!("fsc_v2.min_total_buyers must be positive");
        }
        if self.min_known_non_neutral_buyers == 0 {
            anyhow::bail!("fsc_v2.min_known_non_neutral_buyers must be positive");
        }
        if self.same_slot_cross_signature_policy != "require_tx_index" {
            anyhow::bail!(
                "fsc_v2.same_slot_cross_signature_policy must be require_tx_index in PR-FSC1"
            );
        }
        if self.include_wsol || self.include_spl {
            anyhow::bail!("fsc_v2 primary capture must keep include_wsol/include_spl false");
        }
        Ok(())
    }
}

fn validate_unit_interval(name: &str, value: f64) -> anyhow::Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        anyhow::bail!("{name} must be in range [0.0, 1.0]");
    }
    Ok(())
}

/// Complete configuration for Gatekeeper v2.
///
/// All thresholds are tuneable per-deployment. Default values are calibrated
/// for PumpPortal WebSocket stream with ~2-10 TX/s per active pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatekeeperV2Config {
    // ═══════════════════════════════════════════
    // Mode
    // ═══════════════════════════════════════════
    /// Operating mode: "standard" (reactive, early decisions) or "long" (full-window accumulation).
    /// Default: Standard
    pub mode: GatekeeperMode,
    // ═══════════════════════════════════════════
    // Pre-filter
    // ═══════════════════════════════════════════
    /// Minimum SOL volume per transaction to be considered.
    /// Transactions below this are silently dropped (no metrics, no dedup).
    /// Default: 0.005 SOL
    pub min_sol_threshold: f64,

    // ═══════════════════════════════════════════
    // Phase 1: Quantity Gate
    // ═══════════════════════════════════════════
    /// Minimum unique (non-duplicate, non-dust) transactions to trigger evaluation.
    /// Default: 8
    pub min_tx_count: usize,

    /// Minimum unique signer wallets.
    /// Default: 5
    pub min_unique_signers: usize,

    /// Minimum buy transactions.
    /// Default: 5
    pub min_buy_count: usize,

    /// Hard deadline: maximum time (ms) from first TX to final decision.
    /// Default: 12_000
    pub max_wait_time_ms: u64,

    // ═══════════════════════════════════════════
    // Phase 2: Velocity Profile
    // ═══════════════════════════════════════════
    /// Minimum coefficient of variation for TX intervals.
    /// Default: 0.3
    pub min_interval_cv: f64,

    /// Maximum coefficient of variation for TX intervals.
    /// Deserialization also accepts the legacy/typo alias `mix_interval_cv`.
    /// Default: 9999.0 (neutral / no upper bound)
    #[serde(alias = "mix_interval_cv")]
    pub max_interval_cv: f64,

    /// Maximum fraction of TX arriving in the first 20% of the observation window.
    /// Default: 0.70
    pub max_burst_ratio: f64,

    /// Minimum average interval between TX (ms).
    /// Default: 60.0
    pub min_avg_interval_ms: f64,

    /// Maximum average interval between TX (ms).
    /// Default: 6000.0
    pub max_avg_interval_ms: f64,

    /// Minimum Shannon entropy of the timing distribution.
    /// Default: 1.2
    pub min_timing_entropy: f64,

    /// Maximum Shannon entropy of the timing distribution.
    /// Default: 9999.0 (neutral / no upper bound)
    pub max_timing_entropy: f64,

    /// Minimum number of dust-filtered transactions (below min_sol_threshold).
    /// Higher values correlate with organic pools that attract many small buyers.
    /// Default: 0
    pub min_dust_filtered_count: u64,

    // ═══════════════════════════════════════════
    // Phase 3: Signer Diversity
    // ═══════════════════════════════════════════
    /// Minimum ratio: unique_signers / total_tx.
    /// Default: 0.4
    pub min_unique_ratio: f64,

    /// Maximum ratio: unique_signers / total_tx (anti-sybil: all-fresh wallets = bot army).
    /// Default: 1.0
    pub max_unique_ratio: f64,

    /// Maximum Herfindahl-Hirschman Index (signer TX concentration).
    /// Default: 0.25
    pub max_hhi: f64,

    /// Maximum TX from any single signer.
    /// Default: 4
    pub max_tx_per_signer: usize,

    /// Maximum Gini coefficient for per-signer volume distribution.
    /// Default: 0.70
    pub max_volume_gini: f64,

    /// Minimum Gini coefficient for per-signer volume distribution.
    /// Default: 0.0 (no lower bound)
    pub min_volume_gini: f64,

    /// Maximum combined volume share of the top 3 signers.
    /// Default: 0.75
    pub max_top3_volume_pct: f64,

    /// Maximum ratio of TX arriving within 50ms of each other (Jito bundle detection).
    /// If more than this fraction of TX are near-simultaneous, it's likely a bundled MEV attack.
    /// Default: 0.30
    pub max_same_ms_tx_ratio: f64,

    // ═══════════════════════════════════════════
    // Phase 4: Volume Sanity
    // ═══════════════════════════════════════════
    /// Minimum buy ratio (buy_count / total_count).
    /// Default: 0.50
    pub min_buy_ratio: f64,

    /// Maximum buy ratio (buy_count / total_count). 100% buys = green wall = death signal.
    /// Default: 1.0
    pub max_buy_ratio: f64,

    /// Minimum average TX size in SOL.
    /// Default: 0.02
    pub min_avg_tx_sol: f64,

    /// Maximum average TX size in SOL.
    /// Default: 25.0
    pub max_avg_tx_sol: f64,

    /// Minimum coefficient of variation for TX volumes.
    /// Default: 0.15
    pub min_volume_cv: f64,

    /// Maximum coefficient of variation for TX volumes.
    /// Default: 9999.0 (neutral / no upper bound)
    pub max_volume_cv: f64,

    /// Minimum total SOL volume across all buffered TX.
    /// Default: 0.5
    pub min_total_volume_sol: f64,

    /// Maximum total SOL volume across all buffered TX.
    /// Default: 9999.0 (neutral / no upper bound)
    pub max_total_volume_sol: f64,

    /// Minimum ratio of buy volume (SOL) to total volume (SOL).
    /// Prevents manipulation where many small buys mask a large sell.
    /// sol_buy_ratio = sum(buy_volume) / sum(total_volume)
    /// Default: 0.50
    pub min_sol_buy_ratio: f64,

    /// Maximum ratio of buy volume (SOL) to total volume (SOL).
    /// Caps buy dominance to prevent pure green-wall pools where
    /// 100 % of the volume is buys (often wash-trading or self-fill).
    /// sol_buy_ratio = sum(buy_volume) / sum(total_volume)
    /// Default: 1.0 (neutral / no upper bound)
    pub max_sol_buy_ratio: f64,

    /// Minimum longest consecutive buy streak (FOMO indicator).
    /// A high streak means sustained buying pressure without any sells.
    /// Default: 0
    pub min_consecutive_buys: usize,

    // ═══════════════════════════════════════════
    // Phase 5: Dev Behavior
    // ═══════════════════════════════════════════
    /// Maximum SOL bought by developer wallet.
    /// Default: 8.0
    pub max_dev_buy_sol: f64,

    /// Minimum SOL bought by developer wallet.
    /// Ensures the dev has skin in the game. Only checked if dev wallet is known.
    /// Default: 0.0
    pub min_dev_buy_sol: f64,

    /// Maximum ratio of dev TX to total TX.
    /// Default: 0.20
    pub max_dev_tx_ratio: f64,

    /// Minimum ratio of dev TX to total TX.
    /// Default: 0.0 (no lower bound)
    pub min_dev_tx_ratio: f64,

    /// Maximum ratio of dev volume to total volume.
    /// Default: 0.40
    pub max_dev_volume_ratio: f64,

    /// Minimum ratio of dev volume to total volume.
    /// Filters pools where dev is suspiciously absent from trading activity.
    /// Default: 0.0
    pub min_dev_volume_ratio: f64,

    /// Instant HARD REJECT if dev sells during observation window.
    /// Default: true
    pub reject_on_dev_sell: bool,

    // ═══════════════════════════════════════════
    // Phase 6: Bonding Curve Dynamics
    // ═══════════════════════════════════════════
    /// Minimum price change ratio in window.
    /// Filters pools with zero or negative price movement (dead momentum).
    /// Default: 0.0 (neutral / no lower bound)
    pub min_price_change_ratio: f64,

    /// Maximum price change ratio in window.
    /// Default: 4.0
    pub max_price_change_ratio: f64,

    /// Maximum single-TX price impact (%).
    /// Default: 25.0
    pub max_single_tx_price_impact_pct: f64,

    /// Minimum single SELL TX price impact (%).
    /// Filters pools where no individual sell has meaningful price impact
    /// (thin liquidity / no organic sell pressure — often ghost-town pools).
    /// Default: 0.0 (neutral / no lower bound)
    pub min_single_sell_impact_pct: f64,

    /// Maximum single SELL TX price impact (%).
    /// Unlike max_single_tx_price_impact_pct, this only considers sell transactions.
    /// A large sell impact signals whale/dev evacuation.
    /// Default: 30.0
    pub max_single_sell_impact_pct: f64,

    /// Minimum bonding curve progress (%) at decision time.
    /// Rejects tokens that haven't accumulated enough curve momentum.
    /// Default: 0.0 (no lower bound)
    pub min_bonding_progress_pct: f64,

    /// Maximum bonding curve progress (%) at decision time.
    /// Default: 15.0
    pub max_bonding_progress_pct: f64,

    /// Minimum market cap (SOL) at decision time.
    /// Default: 20.0
    pub min_market_cap_sol: f64,

    // ═══════════════════════════════════════════
    // Decision
    // ═══════════════════════════════════════════
    /// Minimum phases (out of 6) that must pass for BUY recommendation.
    /// NOTE: In the three-layer decision system this is kept for logging/telemetry
    /// but the actual BUY/REJECT decision uses hard_fails → core_pass → soft_score.
    /// Default: 5
    pub min_phases_to_pass: u8,

    /// Re-evaluate phases 2-6 every N new TX after Phase 1 first passes.
    /// Only used in Standard mode. Ignored in Long mode.
    /// Default: 3
    pub re_eval_tx_interval: usize,

    // ═══════════════════════════════════════════
    // Three-Layer Decision System
    // ═══════════════════════════════════════════
    /// Enable three-layer decision system (hard_fails → core_pass → soft_signals).
    /// When true, replaces the simple `phases_passed >= min_phases_to_pass` check.
    /// When false, uses legacy phases_passed logic.
    /// Default: true
    pub use_three_layer_decision: bool,

    // ── Hard Fail extreme thresholds (kill-switches at obvious extremes) ────
    // These are HIGHER than the phase-level soft thresholds.
    // They catch only blatant manipulation, independent of phase pass/fail.
    /// HHI hard-fail threshold (extreme cabal). Phase-level is max_hhi.
    /// Default: 0.10
    pub hard_fail_hhi: f64,

    /// same_ms_tx_ratio hard-fail threshold (extreme bundling). Phase-level is max_same_ms_tx_ratio.
    /// Default: 0.60
    pub hard_fail_same_ms_tx_ratio: f64,

    /// top3_volume_pct hard-fail threshold (extreme whale dominance). Phase-level is max_top3_volume_pct.
    /// Default: 0.70
    pub hard_fail_top3_volume_pct: f64,

    // ── Soft signal scoring (weighted) ────────────────────────────────────
    // Soft signals are grouped by category. Each group has a weight.
    // soft_points = sum(weight_i * flag_i). BUY only if soft_points <= max_soft_points.
    /// Maximum soft points (weighted sum of raised flags) to allow BUY.
    /// If soft_points > max_soft_points → REJECT (SOFT_EXCESS).
    /// Default: 8
    pub max_soft_points: u8,

    /// Weight for Timing group: low_interval_cv, low_timing_entropy,
    /// avg_interval_out_of_range, high_burst_ratio.
    /// Default: 1
    pub soft_weight_timing: u8,

    /// Weight for Manipulation group: bundle_suspicion, cabal_suspicion, top3_dominance.
    /// Default: 3
    pub soft_weight_manipulation: u8,

    /// Weight for Diversity group: high_volume_gini, unique_ratio_out_of_range, high_tx_per_signer.
    /// Default: 2
    pub soft_weight_diversity: u8,

    /// Weight for Ecosystem group: low_dust_count.
    /// Default: 1
    pub soft_weight_ecosystem: u8,

    /// DEPRECATED — kept for deserialization compat. Use max_soft_points instead.
    #[serde(default)]
    pub max_soft_score: u8,

    // ── Alpha gate ────────────────────────────────────────────────────────────
    /// Enable deterministic alpha gate after hard/core/legacy-soft/sybil and before provisional BUY.
    #[serde(default = "default_enable_alpha_gate")]
    pub enable_alpha_gate: bool,

    /// Minimum momentum scalar required by the alpha gate.
    #[serde(default = "default_min_momentum")]
    pub min_momentum: f64,

    /// Minimum demand scalar required by the alpha gate.
    #[serde(default = "default_min_demand")]
    pub min_demand: f64,

    /// Minimum joint alpha score (`momentum * demand`) required by the alpha gate.
    #[serde(default = "default_min_alpha_joint")]
    pub min_alpha_joint: f64,

    /// Minimum buy-count sample before alpha gating becomes actionable.
    #[serde(default = "default_min_alpha_sample")]
    pub min_alpha_sample: usize,

    // ── Prosperity filter ───────────────────────────────────────────────────
    /// Enable the final prosperity selector after hard/core/soft/sybil/alpha.
    ///
    /// When enabled, the Balanced v1 policy fails closed on missing market-cap
    /// or CPV truth and then requires at least one positive prosperity branch.
    pub enable_prosperity_filter: bool,

    /// Light-veto market-cap floor for the prosperity selector.
    pub prosperity_min_market_cap_sol: f64,

    /// Light-veto max signer cross-pool velocity for the prosperity selector.
    pub prosperity_max_signer_cross_pool_velocity: f64,

    /// Branch B1: minimum block-0 sniped supply fraction.
    pub prosperity_branch1_min_block0_sniped_supply_pct: f64,

    /// Branch B1: maximum sell/buy ratio allowed alongside block-0 conviction.
    pub prosperity_branch1_max_sell_buy_ratio: f64,

    /// Branch B2: elevated market-cap floor for large-cap dominance flows.
    pub prosperity_branch2_min_market_cap_sol: f64,

    /// Branch B2: minimum early-slot buy-volume dominance.
    pub prosperity_branch2_min_early_slot_volume_dominance_buy: f64,

    /// Branch B3: maximum HHI for the organic-structure branch.
    pub prosperity_branch3_max_hhi: f64,

    /// Branch B3: minimum FTDI for the organic-structure branch.
    pub prosperity_branch3_min_fee_topology_diversity_index: f64,

    /// Enable the strict overlay on top of matched prosperity branches.
    ///
    /// When enabled, already-matched Balanced branches must also satisfy an
    /// overextension/quality veto derived from the current strict shadow regime.
    pub enable_prosperity_overlay: bool,

    /// Global overlay maximum price-change ratio allowed after a branch matches.
    pub prosperity_overlay_max_price_change_ratio: f64,

    /// Global overlay maximum bonding-progress percentage allowed after a branch matches.
    pub prosperity_overlay_max_bonding_progress_pct: f64,

    /// Global overlay minimum FTDI required after a branch matches.
    pub prosperity_overlay_min_fee_topology_diversity_index: f64,

    /// Overlay maximum sell/buy ratio for B2/B3 qualified branches.
    pub prosperity_overlay_branch23_max_sell_buy_ratio: f64,

    /// Stricter overlay maximum price-change ratio for B2 large-cap dominance.
    pub prosperity_overlay_branch2_max_price_change_ratio: f64,

    // ── Dev Unknown stricter requirements ───────────────────────────────────
    /// Elevated min_market_cap_sol when dev wallet is unknown.
    /// Closes the "no dev info → free pass" vector.
    /// Default: 65.0
    pub dev_unknown_min_market_cap_sol: f64,

    /// Elevated min_sol_buy_ratio when dev wallet is unknown.
    /// Default: 0.65
    pub dev_unknown_min_sol_buy_ratio: f64,

    /// Stricter max_soft_points when dev wallet is unknown.
    /// Adversarial bots can hide dev identity; compensate by lowering soft tolerance.
    /// Default: 5
    pub dev_unknown_max_soft_points: u8,

    /// Stricter max_single_tx_price_impact_pct when dev wallet is unknown.
    /// Lowers the allowable single-TX price impact to close the "clean spoof" vector:
    /// an adversary who keeps soft flags clean shouldn't get a free pass just because
    /// dev identity is hidden.
    /// Default: 28.0 (vs normal 33.0)
    pub dev_unknown_max_single_tx_price_impact_pct: f64,

    // ── Hybrid Fingerprint thresholds ────────────────────────────────────────
    /// Maximum acceptable SELL/BUY count ratio.
    /// Default: 9999.0 (neutral / telemetry-only)
    pub max_sell_buy_ratio: f64,

    /// Minimum acceptable SELL/BUY count ratio.
    /// Default: 0.0 (neutral / no lower bound)
    pub min_sell_buy_ratio: f64,

    /// Maximum acceptable clustered CU-dominance ratio.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_compute_unit_cluster_dominance: f64,

    /// Minimum acceptable clustered CU-dominance ratio.
    /// Default: 0.0 (neutral / no lower bound)
    pub min_compute_unit_cluster_dominance: f64,

    /// Maximum acceptable repeated exact `(cu_limit, cu_price)` buy-profile ratio.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_static_fee_profile_ratio: f64,

    /// Minimum acceptable repeated exact `(cu_limit, cu_price)` buy-profile ratio.
    /// Default: 0.0 (neutral / no lower bound)
    pub min_static_fee_profile_ratio: f64,

    /// Maximum acceptable dominant 0.001-SOL buy bucket ratio.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_fixed_size_buy_ratio: f64,

    /// Minimum acceptable dominant 0.001-SOL buy bucket ratio.
    /// Default: 0.0 (no lower bound)
    pub min_fixed_size_buy_ratio: f64,

    /// Maximum acceptable dominant 0.0001-SOL buy bucket ratio.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_fixed_size_buy_ratio_1e4: f64,

    /// Maximum acceptable fraction of wallets that both bought and sold early.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_flipper_presence_ratio: f64,

    /// Maximum acceptable fraction of tx with deterministic Jito tips.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_jito_tip_intensity: f64,

    /// Minimum acceptable fraction of tx with deterministic Jito tips.
    /// Default: 0.0 (neutral / no lower bound)
    pub min_jito_tip_intensity: f64,

    /// Maximum acceptable buy-volume concentration in the first N slots.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_early_slot_volume_dominance_buy: f64,

    /// Maximum acceptable share of early buy volume captured by the top-3 buyers
    /// within the first 3 seconds from pool birth.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_early_top3_buy_volume_pct_3s: f64,

    /// Minimum acceptable average inner-instruction count over the first 50 tx.
    /// Default: 0.0 (no lower bound)
    pub min_avg_inner_ix_count_50tx: f64,

    /// Maximum acceptable average inner-instruction count over the first 50 tx.
    /// Default: 9999.0 (neutral / no upper bound)
    pub max_avg_inner_ix_count_50tx: f64,

    /// Maximum acceptable sell/buy ratio for the top-3 early whales.
    /// Default: 9999.0 (neutral / telemetry-only)
    pub max_whale_reversal_ratio_top3: f64,

    /// Maximum acceptable sell/buy ratio for the top-1 early whale.
    /// Default: 9999.0 (neutral / telemetry-only)
    pub max_whale_reversal_ratio_top1: f64,

    /// Minimum acceptable latency before the first meaningful dev sell.
    /// Default: 0 ms (neutral / telemetry-only)
    pub min_dev_paperhand_latency_ms: u64,

    // ── Sybil resistance thresholds ────────────────────────────────────────
    /// Minimum acceptable fee topology diversity index.
    /// Default: 0.0 (neutral / telemetry-only)
    pub min_fee_topology_diversity_index: f64,

    /// Maximum acceptable dev-buyer infrastructure affinity.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_dev_buyer_infrastructure_affinity: f64,

    /// Minimum acceptable spend fraction divergence.
    /// Default: 0.0 (neutral / telemetry-only)
    pub min_spend_fraction_divergence: f64,

    /// Minimum acceptable demand elasticity score.
    /// Default: -1.0 (neutral / telemetry-only)
    pub min_demand_elasticity_score: f64,

    /// Maximum acceptable signer cross-pool velocity.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_signer_cross_pool_velocity: f64,

    /// Maximum acceptable funding source concentration.
    /// Default: 1.0 (neutral / telemetry-only)
    pub max_funding_source_concentration: f64,

    // ── Sybil resistance soft penalties ────────────────────────────────────
    /// Soft penalty for low FTDI.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_low_ftdi: u8,

    /// Soft penalty for high DBIA.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_dbia: u8,

    /// Soft penalty for low SFD.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_low_sfd: u8,

    /// Soft penalty for inelastic demand.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_inelastic_demand: u8,

    /// Soft penalty for high CPV.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_cpv: u8,

    /// Soft penalty for high FSC.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_fsc: u8,

    /// Soft penalty for the high-DBIA + low-FTDI combo.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_dbia_low_ftdi_combo: u8,

    /// Soft penalty for the low-DES + low-SFD combo.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_low_des_low_sfd_combo: u8,

    /// Soft penalty for the high-CPV + low-DES combo.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_cpv_low_des_combo: u8,

    /// Soft penalty for the high-FSC + high-CPV combo.
    /// Default: 0 (neutral / inactive)
    pub soft_penalty_high_fsc_high_cpv_combo: u8,

    // ── Sybil Interference policy layer ─────────────────────────────────────
    /// Master switch for verdict-level sybil bucket enforcement.
    /// Telemetry may still be computed when disabled.
    /// Default: false
    pub enable_sybil_interference_layer: bool,

    /// Dedicated sybil threshold independent from legacy max_soft_points.
    /// Default: 255 (neutral / inactive)
    pub max_sybil_soft_points: u8,

    /// Dedicated stricter sybil threshold when dev wallet is unknown.
    /// Default: 255 (neutral / inactive)
    pub dev_unknown_max_sybil_soft_points: u8,

    /// Master switch for high-confidence sybil combo veto.
    /// Default: false
    pub enable_sybil_combo_veto: bool,

    /// Emit aggregated sybil meta-score to decision logs.
    /// Default: false
    pub emit_sybil_meta_score: bool,

    /// Require FSC to be production-ready before it can participate in combo veto.
    /// Default: true
    pub require_ready_fsc_for_combo_veto: bool,

    // ── Sybil resistance rolling-state params ──────────────────────────────
    /// TTL / lookback for CPV index in seconds.
    /// Default: 300
    pub cpv_lookback_window_s: u64,

    /// TTL / lookback for funding-source index in seconds.
    /// Default: 300
    pub funding_lookback_window_s: u64,

    /// Minimum funding transfer size tracked by FSC.
    /// Default: 10_000_000 lamports (0.01 SOL)
    pub funding_dust_threshold_lamports: u64,

    /// Per-signer bounded history cap for CPV.
    /// Default: 16
    pub cpv_per_signer_cap: usize,

    /// Global signer cap for CPV rolling state.
    /// Default: 50_000
    pub cpv_global_signer_cap: usize,

    /// Per-recipient bounded history cap for FSC.
    /// Default: 4
    pub fsc_per_recipient_cap: usize,

    /// Global recipient cap for FSC rolling state.
    /// Default: 75_000
    pub fsc_global_recipient_cap: usize,

    // ── Sybil resistance neutral funding sources ───────────────────────────
    /// Funding sources explicitly treated as neutral (e.g. CEX hot wallets).
    #[serde(default)]
    pub neutral_funding_sources: Vec<String>,

    // ── Hard Fail Bot Detection guards ─────────────────────────────────────
    /// Minimum TX count required to trigger HF-9 (extreme bot timing).
    /// Prevents false positives from small sample sizes.
    /// Default: 20
    pub hard_fail_bot_min_tx: usize,

    /// Minimum observation window (ms) required to trigger HF-9.
    /// Default: 1500
    pub hard_fail_bot_min_observation_ms: u64,

    // ── IWIM Veto Strength Classification (embedded for GatekeeperBuffer access) ──
    /// Soft-point margin for STRONG classification in IWIM policy matrix.
    /// STRONG if soft_points <= (effective_max - this). Mirrored from IwimVetoGateConfig.
    /// Default: 3
    #[serde(default = "default_iwim_veto_strong_margin_gk")]
    pub iwim_veto_strong_margin: u8,

    /// Any manipulation flag count above this → BORDERLINE for IWIM policy.
    /// Default: 0
    #[serde(default)]
    pub iwim_veto_strong_max_manip_flags: u8,

    // ═══════════════════════════════════════════
    // Yellowstone-only (optional)
    // ═══════════════════════════════════════════
    /// Flag pools where failed_tx ratio > threshold. Yellowstone-only.
    /// Default: None
    pub min_failed_tx_ratio_for_bot_flag: Option<f64>,

    /// Use slot-level ordering for timing analysis.
    /// Default: false
    pub use_slot_ordering: bool,

    // ═══════════════════════════════════════════
    // Curve Readiness Latch
    // ═══════════════════════════════════════════
    /// Maximum time (ms) to wait for bonding curve data before rejecting.
    /// After this timeout, if curve_data_known is still false, the pool is
    /// hard-rejected with CURVE_NOT_READY_TIMEOUT.
    /// Default: 800
    #[serde(default = "default_curve_wait_ms")]
    pub curve_wait_ms: u64,

    /// Require curve data for BUY verdicts.  When true, BUY is impossible
    /// while curve_data_known == false (fail-closed).
    /// Default: true
    #[serde(default = "default_curve_require_for_buy")]
    pub curve_require_for_buy: bool,

    /// Policy for stale curve quality in Phase-5 Gatekeeper handling.
    /// Default: pending_curve
    #[serde(default)]
    pub stale_fallback: ShadowLedgerStaleFallback,

    // ═══════════════════════════════════════════
    // Gatekeeper V2.5 — PRECISION STRIKE modules
    // ═══════════════════════════════════════════
    /// Rollout guardrails for V2.5 modules. Controls shadow/live execution gating.
    #[serde(default)]
    pub v25: GatekeeperV25RolloutConfig,

    /// Dynamic Observation Window (dow) — multi-deadline decision points.
    #[serde(default)]
    pub dow: DynamicObservationWindowConfig,

    /// Trajectory Aware Scoring (tas) — momentum-trajectory confidence modulator.
    #[serde(default)]
    pub tas: TrajectoryAwareScoringConfig,

    /// Pump & Dump Detector (pdd) — synthetic pump pattern detection.
    #[serde(default)]
    pub pdd: PumpAndDumpDetectorConfig,

    /// Adaptive Prosperity (aps) — regime-aware prosperity branch adaptation.
    #[serde(default)]
    pub aps: AdaptiveProsperityConfig,
}

impl Default for GatekeeperV2Config {
    fn default() -> Self {
        Self {
            // Mode
            mode: GatekeeperMode::Standard,

            // Pre-filter
            min_sol_threshold: 0.1,

            // Phase 1
            min_tx_count: 30,
            min_unique_signers: 15,
            min_buy_count: 15,
            max_wait_time_ms: 2_222,

            // Phase 2
            min_interval_cv: 0.3,
            max_interval_cv: 9999.0,
            max_burst_ratio: 0.70,
            min_avg_interval_ms: 60.0,
            max_avg_interval_ms: 600.0,
            min_timing_entropy: 1.2,
            max_timing_entropy: 9999.0,
            min_dust_filtered_count: 0,

            // Phase 3
            min_unique_ratio: 0.4,
            max_unique_ratio: 1.0,
            max_hhi: 0.25,
            max_tx_per_signer: 4,
            max_volume_gini: 0.70,
            min_volume_gini: 0.0,
            max_top3_volume_pct: 0.75,
            max_same_ms_tx_ratio: 0.30,

            // Phase 4
            min_buy_ratio: 0.50,
            max_buy_ratio: 1.0,
            min_avg_tx_sol: 0.02,
            max_avg_tx_sol: 25.0,
            min_volume_cv: 0.15,
            max_volume_cv: 9999.0,
            min_total_volume_sol: 0.5,
            max_total_volume_sol: 9999.0,
            min_sol_buy_ratio: 0.50,
            max_sol_buy_ratio: 1.0,
            min_consecutive_buys: 0,

            // Phase 5
            max_dev_buy_sol: 8.0,
            min_dev_buy_sol: 0.0,
            max_dev_tx_ratio: 0.20,
            min_dev_tx_ratio: 0.0,
            max_dev_volume_ratio: 0.40,
            min_dev_volume_ratio: 0.0,
            reject_on_dev_sell: true,

            // Phase 6
            min_price_change_ratio: 0.0,
            max_price_change_ratio: 4.0,
            max_single_tx_price_impact_pct: 25.0,
            min_single_sell_impact_pct: 0.0,
            max_single_sell_impact_pct: 30.0,
            min_bonding_progress_pct: 0.0,
            max_bonding_progress_pct: 15.0,
            min_market_cap_sol: 20.0,

            // Decision
            min_phases_to_pass: 3,
            re_eval_tx_interval: 3,

            // Three-Layer Decision System
            use_three_layer_decision: true,
            hard_fail_hhi: 0.10,
            hard_fail_same_ms_tx_ratio: 0.60,
            hard_fail_top3_volume_pct: 0.70,

            // Weighted soft scoring
            max_soft_points: 8,
            soft_weight_timing: 1,
            soft_weight_manipulation: 3,
            soft_weight_diversity: 2,
            soft_weight_ecosystem: 1,
            max_soft_score: 6, // deprecated compat
            enable_alpha_gate: false,
            min_momentum: 0.55,
            min_demand: 0.55,
            min_alpha_joint: 0.35,
            min_alpha_sample: 15,
            enable_prosperity_filter: false,
            prosperity_min_market_cap_sol: 35.0,
            prosperity_max_signer_cross_pool_velocity: 0.50,
            prosperity_branch1_min_block0_sniped_supply_pct: 0.28,
            prosperity_branch1_max_sell_buy_ratio: 0.16,
            prosperity_branch2_min_market_cap_sol: 50.0,
            prosperity_branch2_min_early_slot_volume_dominance_buy: 0.90,
            prosperity_branch3_max_hhi: 0.0416,
            prosperity_branch3_min_fee_topology_diversity_index: 0.0909,
            enable_prosperity_overlay: false,
            prosperity_overlay_max_price_change_ratio: 2.2,
            prosperity_overlay_max_bonding_progress_pct: 85.0,
            prosperity_overlay_min_fee_topology_diversity_index: 0.10,
            prosperity_overlay_branch23_max_sell_buy_ratio: 0.18,
            prosperity_overlay_branch2_max_price_change_ratio: 2.0,

            // Dev unknown
            dev_unknown_min_market_cap_sol: 65.0,
            dev_unknown_min_sol_buy_ratio: 0.65,
            dev_unknown_max_soft_points: 5,
            dev_unknown_max_single_tx_price_impact_pct: 28.0,

            // Hybrid Fingerprint telemetry thresholds
            max_sell_buy_ratio: 9999.0,
            min_sell_buy_ratio: 0.0,
            max_compute_unit_cluster_dominance: 1.0,
            min_compute_unit_cluster_dominance: 0.0,
            max_static_fee_profile_ratio: 1.0,
            min_static_fee_profile_ratio: 0.0,
            max_fixed_size_buy_ratio: 1.0,
            min_fixed_size_buy_ratio: 0.0,
            max_fixed_size_buy_ratio_1e4: 1.0,
            max_flipper_presence_ratio: 1.0,
            max_jito_tip_intensity: 1.0,
            min_jito_tip_intensity: 0.0,
            max_early_slot_volume_dominance_buy: 1.0,
            max_early_top3_buy_volume_pct_3s: 1.0,
            min_avg_inner_ix_count_50tx: 0.0,
            max_avg_inner_ix_count_50tx: 9999.0,
            max_whale_reversal_ratio_top3: 9999.0,
            max_whale_reversal_ratio_top1: 9999.0,
            min_dev_paperhand_latency_ms: 0,

            // Sybil resistance thresholds
            min_fee_topology_diversity_index: 0.0,
            max_dev_buyer_infrastructure_affinity: 1.0,
            min_spend_fraction_divergence: 0.0,
            min_demand_elasticity_score: -1.0,
            max_signer_cross_pool_velocity: 1.0,
            max_funding_source_concentration: 1.0,

            // Sybil resistance soft penalties
            soft_penalty_low_ftdi: 0,
            soft_penalty_high_dbia: 0,
            soft_penalty_low_sfd: 0,
            soft_penalty_inelastic_demand: 0,
            soft_penalty_high_cpv: 0,
            soft_penalty_high_fsc: 0,
            soft_penalty_high_dbia_low_ftdi_combo: 0,
            soft_penalty_low_des_low_sfd_combo: 0,
            soft_penalty_high_cpv_low_des_combo: 0,
            soft_penalty_high_fsc_high_cpv_combo: 0,
            enable_sybil_interference_layer: false,
            max_sybil_soft_points: 255,
            dev_unknown_max_sybil_soft_points: 255,
            enable_sybil_combo_veto: false,
            emit_sybil_meta_score: false,
            require_ready_fsc_for_combo_veto: true,

            // Sybil resistance rolling-state params
            cpv_lookback_window_s: 300,
            funding_lookback_window_s: 300,
            funding_dust_threshold_lamports: 10_000_000,
            cpv_per_signer_cap: 16,
            cpv_global_signer_cap: 50_000,
            fsc_per_recipient_cap: 4,
            fsc_global_recipient_cap: 75_000,
            neutral_funding_sources: Vec::new(),

            // HF-9 bot detection guards
            hard_fail_bot_min_tx: 20,
            hard_fail_bot_min_observation_ms: 1500,

            // IWIM Veto Strength Classification
            iwim_veto_strong_margin: 3,
            iwim_veto_strong_max_manip_flags: 0,

            // Yellowstone
            min_failed_tx_ratio_for_bot_flag: None,
            use_slot_ordering: false,

            // Curve Readiness Latch
            curve_wait_ms: 800,
            curve_require_for_buy: true,
            stale_fallback: ShadowLedgerStaleFallback::PendingCurve,

            // Gatekeeper V2.5 modules
            v25: GatekeeperV25RolloutConfig::default(),
            dow: DynamicObservationWindowConfig::default(),
            tas: TrajectoryAwareScoringConfig::default(),
            pdd: PumpAndDumpDetectorConfig::default(),
            aps: AdaptiveProsperityConfig::default(),
        }
    }
}

impl GatekeeperV2Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.dow.enabled && self.dow.extended_window_ms > self.max_wait_time_ms {
            anyhow::bail!(
                "P0 invariant violated: [gatekeeper_v2.dow].extended_window_ms ({}) > [gatekeeper_v2].max_wait_time_ms ({})",
                self.dow.extended_window_ms,
                self.max_wait_time_ms,
            );
        }
        if self.dow.enabled && self.dow.tick_interval_ms == 0 {
            anyhow::bail!(
                "P0 invariant violated: [gatekeeper_v2.dow].tick_interval_ms must be > 0 when DOW is enabled"
            );
        }
        if self.pdd.entry_drift_elapsed_scaling_enabled {
            for (name, value) in [
                (
                    "entry_drift_elapsed_base_pct",
                    self.pdd.entry_drift_elapsed_base_pct,
                ),
                (
                    "entry_drift_elapsed_slope_pct_per_second",
                    self.pdd.entry_drift_elapsed_slope_pct_per_second,
                ),
                (
                    "entry_drift_elapsed_cap_pct",
                    self.pdd.entry_drift_elapsed_cap_pct,
                ),
            ] {
                if !value.is_finite() || value < 0.0 {
                    anyhow::bail!("gatekeeper_v2.pdd.{name} must be finite and non-negative");
                }
            }
            if self.pdd.entry_drift_elapsed_cap_pct < self.pdd.entry_drift_elapsed_base_pct {
                anyhow::bail!(
                    "gatekeeper_v2.pdd.entry_drift_elapsed_cap_pct must be >= entry_drift_elapsed_base_pct"
                );
            }
        }
        Ok(())
    }
}

/// IWIM (Initial Wallet Intent Mapping) Configuration
///
/// Controls dev-wallet behavioral analysis parameters for detecting creator
/// intentions (SCAMMER vs BUILDER vs SYBIL-BOT).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IwimConfig {
    /// IAPP threshold: number of token accounts created within 1s for rug flag
    /// ≥2 token accounts → 97% rug probability
    /// Default: 2
    pub iapp_rug_threshold: usize,

    /// Minimum rug threat score when IAPP threshold is met
    /// Default: 0.95 (97% rug probability per spec)
    /// Range: [0.0, 1.0]
    pub min_iapp_rug_score: f32,

    /// Authority change window in milliseconds
    /// Changes within this window trigger AT (Authority Transfer) flag
    /// Default: 1500
    pub at_window_ms: u64,

    /// Pre-mint quietness window in milliseconds
    /// 0 transactions for this period = organic
    /// Default: 5000
    pub quiet_window_ms: u64,

    /// Maximum transactions to analyze (DoS protection)
    /// Default: 50
    pub max_tx_analyze: usize,

    /// Confidence threshold for reliable classification
    /// Default: 0.6
    /// Range: [0.0, 1.0]
    pub confidence_threshold: f32,

    /// Performance target in microseconds
    /// Default: 120
    pub target_analysis_time_us: u64,
}

/// QASS (Quantum Amplitude Superposition Scoring) Configuration
///
/// Controls quantum-inspired signal aggregation parameters for deriving
/// unified scores from multiple heuristics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QassConfig {
    /// Default collapse threshold for wave function collapse
    /// Default: 0.5
    /// Range: [0.0, 1.0]
    pub collapse_threshold: f64,

    /// Score threshold for viral launch classification
    /// Default: 0.85
    /// Range: [0.0, 1.0]
    pub score_threshold_viral: f64,

    /// Score threshold for moderate bullish classification
    /// Default: 0.70
    /// Range: [0.0, 1.0]
    pub score_threshold_moderate: f64,

    /// Score threshold for neutral classification
    /// Default: 0.50
    /// Range: [0.0, 1.0]
    pub score_threshold_neutral: f64,

    /// Score threshold for suspicious classification
    /// Default: 0.30
    /// Range: [0.0, 1.0]
    pub score_threshold_suspicious: f64,

    /// Default weight for signals (used when not specified)
    /// Default: 1.0
    /// Range: [0.0, infinity)
    pub default_signal_weight: f64,
}

/// SOBP (Slot-Over-Slot Buying Pressure) Configuration
///
/// Controls buying pressure analysis parameters for detecting pump onset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SobpConfig {
    /// Weight multiplier for human actors (HumanMobile, HumanDesktop)
    /// Default: 2.0
    /// Range: [0.0, 10.0]
    pub human_weight_multiplier: f32,

    /// Weight multiplier for sniper bot actors
    /// Default: 0.5
    /// Range: [0.0, 10.0]
    pub sniper_weight_multiplier: f32,

    /// Default weight multiplier for other actors
    /// Default: 1.0
    /// Range: [0.0, 10.0]
    pub default_weight_multiplier: f32,

    /// Base transaction weight
    /// Default: 1.0
    pub base_transaction_weight: f32,

    /// SOBP threshold for hyper-aggressive buy influx (early pump)
    /// Default: 3.0
    /// Range: [0.0, 10.0]
    pub hyper_pump_threshold: f32,

    /// SOBP threshold for stable organic growth (bullish)
    /// Default: 1.5
    /// Range: [0.0, 10.0]
    pub growth_threshold: f32,

    /// SOBP threshold for stagnation/no clear direction
    /// Default: 0.8
    /// Range: [0.0, 2.0]
    pub stagnation_threshold: f32,

    /// SOBP threshold for demand implosion (panic/dump)
    /// Default: 0.4
    /// Range: [0.0, 1.0]
    pub implosion_threshold: f32,

    /// Weight for history component in confidence calculation
    /// Default: 0.4
    /// Range: [0.0, 1.0]
    pub confidence_weight_history: f32,

    /// Weight for transaction count component in confidence calculation
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    pub confidence_weight_tx_count: f32,

    /// Weight for intensity component in confidence calculation
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    pub confidence_weight_intensity: f32,

    /// Default slot capacity for history buffer
    /// Default: 64
    pub slot_capacity: usize,

    /// Minimum slot history required for reliable SOBP calculation
    /// Default: 2
    pub min_slot_history: usize,
}

/// QOFSV (Quantum Order-Flow State Vector) Configuration
///
/// Controls quantum state vector mapping parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QofsvConfig {
    /// Number of features in the state vector
    /// Currently: SOBP Pressure, IWIM Threat, MPCF Entropy, + 3 reserved
    /// Default: 6
    pub state_vector_dim: usize,

    /// Small epsilon for numerical stability
    /// Default: 1e-6
    pub epsilon: f32,

    /// Performance target for state construction in microseconds
    /// Default: 200
    pub target_construction_time_us: u64,

    /// Performance target for normalization in microseconds
    /// Default: 50
    pub target_normalization_time_us: u64,
}

/// FRB (Frequency Resonance Bands) Configuration
///
/// Controls frequency-based signal analysis parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrbConfig {
    /// Minimum amplitude threshold for signal detection
    /// Default: 0.001
    /// Range: [0.0, 1.0]
    pub min_amplitude_threshold: f32,

    /// Enable advanced frequency filtering
    /// Default: true
    pub enable_filtering: bool,
}

/// Resonance Detection Configuration
///
/// Controls bot detection via coefficient of variation analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceConfig {
    /// Coefficient of variation threshold for bot detection
    /// Low CV = regular bot timing patterns
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    pub bot_threshold_cv: f64,

    /// Coefficient of variation threshold for human detection
    /// High CV = irregular human timing patterns
    /// Default: 0.8
    /// Range: [0.0, 2.0]
    pub human_threshold_cv: f64,
}

/// LIGMA (Liquidity Genesis Manifold Analyzer) Configuration
///
/// Controls liquidity trap detection, tradability analysis, and integration
/// into scoring systems. LIGMA operates continuously throughout the entire
/// scoring cycle to detect retail-friendliness, sniper convexity, and liquidity
/// traps, providing real-time protection against sudden liquidity changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LigmaConfig {
    /// Enable LIGMA analysis
    /// Default: true
    pub enabled: bool,

    /// Retail impact limit in basis points (BPS)
    /// Trades with impact below this are considered retail-friendly
    /// Default: 700.0 (7%)
    /// Range: [0.0, 10000.0]
    pub retail_impact_limit_bps: f64,

    /// Soft impact threshold in basis points
    /// Used for normalization in tradability scoring
    /// Default: 2000.0 (20%)
    /// Range: [0.0, 10000.0]
    pub soft_impact_bps: f64,

    /// Hard impact threshold in basis points
    /// Used for liquidity trap risk assessment
    /// Default: 6000.0 (60%)
    /// Range: [0.0, 10000.0]
    pub hard_impact_bps: f64,

    /// Micro jump threshold in basis points
    /// Used for sniper attractiveness scoring
    /// Default: 2500.0 (25%)
    /// Range: [0.0, 10000.0]
    pub micro_jump_bps: f64,

    /// Weight of LIGMA in SurvivorScore calculation
    /// Default: 0.15 (15% contribution)
    /// Range: [0.0, 1.0]
    pub weight_in_survivor_score: f32,

    /// VETO threshold for liquidity trap risk
    /// If liquidity_trap_risk > this, reject immediately
    /// Default: 0.80 (80%)
    /// Range: [0.0, 1.0]
    pub veto_trap_threshold: f64,

    /// VETO threshold for psi_ligma
    /// If psi_ligma < this, reject immediately
    /// Default: -0.5
    /// Range: [-1.0, 1.0]
    pub veto_psi_ligma_threshold: f64,
}

impl Default for LigmaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retail_impact_limit_bps: 700.0,
            soft_impact_bps: 2000.0,
            hard_impact_bps: 6000.0,
            micro_jump_bps: 2500.0,
            weight_in_survivor_score: 0.15,
            veto_trap_threshold: 0.80,
            veto_psi_ligma_threshold: -0.5,
        }
    }
}

/// FRE (Fractal Resonance Engine) Configuration
///
/// Controls the Fractal Resonance Engine decision parameters that analyze
/// market fractality, scale coherence, and stability to detect organic pumps
/// versus bot-driven scams.
///
/// ## Components
///
/// - **STT (Scale-Transition Test)**: Analyzes consistency across transaction size buckets
/// - **FSW (Fractal Stability Window)**: Monitors variance in Hurst exponent over time
/// - **ARB (Asymmetric Risk Bias)**: Nonlinear risk evaluation combining all metrics
///
/// ## Key Concepts
///
/// - **Hurst Exponent**: Measures trend persistence (0.5 = random walk, >0.5 = trending)
/// - **Coherence**: Measures consistency of Hurst across small/mid/large transaction buckets
/// - **Stability**: Variance in Hurst over time (low = stable, high = chaotic)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct FreConfig {
    /// Minimum transaction count for STT analysis
    /// Below this threshold, STT returns neutral coherence (0.5)
    /// Default: 12
    /// Range: [5, 100]
    pub stt_min_tx_count: usize,

    /// FSW sigma threshold for WATCH state
    /// If stability_sigma >= this but < fsw_sigma_skip, signal is WATCH (jittery)
    /// Default: 0.12
    /// Range: [0.05, 0.30]
    pub fsw_sigma_watch: f64,

    /// FSW sigma threshold for SKIP state
    /// If stability_sigma > this, signal is SKIP (chaotic/pump & dump)
    /// Default: 0.18
    /// Range: [0.10, 0.40]
    pub fsw_sigma_skip: f64,

    /// Minimum organic score to consider BUY
    /// Organic score is 0-100 calculated from ARB logic
    /// Default: 75
    /// Range: [50, 95]
    pub min_organic_score: u8,

    /// Penalty factor for scale incoherence in ARB logic
    /// Higher values = stronger penalty for bot-like patterns
    /// Default: 1.5
    /// Range: [1.0, 3.0]
    pub arb_penalty_factor: f64,
}

impl Default for FreConfig {
    fn default() -> Self {
        Self {
            stt_min_tx_count: 12,
            fsw_sigma_watch: 0.12,
            fsw_sigma_skip: 0.18,
            min_organic_score: 75,
            arb_penalty_factor: 1.5,
        }
    }
}

/// TCF (Trend Cohesion Field) Configuration
///
/// Controls the Trend Cohesion Field module that measures the coherence of market
/// dynamics across scoring cycles. TCF evaluates whether the mechanism generating
/// market changes remains consistent, rather than just looking at price direction.
///
/// ## Key Concepts
///
/// - **Cohesion**: Measures how well observed transitions match expected patterns
/// - **Transition**: State change between consecutive observations
/// - **Field**: Accumulator tracking cohesion over time
///
/// ## Integration with Final Verdict
///
/// TCF participates ONLY in Final Verdict (after all 12 cycles complete).
/// It modulates the momentum component of the SurvivorScore.
///
/// Formula: `effective_momentum = base_momentum * (tcf_min_modulation + tcf_modulation_range * tcf_score)`
///
/// Example with defaults: `base=50, tcf=0.8 → effective = 50 * (0.6 + 0.4*0.8) = 46`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcfConfig {
    /// Enable TCF analysis
    ///
    /// **Default: true**
    ///
    /// When false, TCF is completely bypassed and tcf_score defaults to neutral (0.5).
    pub enabled: bool,

    /// Weight of TCF in Final Verdict calculation
    ///
    /// **Default: 0.15** (15% contribution to final momentum modulation)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// Higher values give TCF more influence on the final score.
    /// Recommended range: 0.10-0.25 based on backtesting.
    pub weight_in_final_verdict: f32,

    /// Minimum modulation factor (floor when tcf_score = 0)
    ///
    /// **Default: 0.6**
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// This is the floor multiplier applied to momentum when TCF detects
    /// complete trend breakdown. Value of 0.6 means momentum can be reduced
    /// to 60% at worst, preventing TCF from completely killing good tokens.
    pub tcf_min_modulation: f64,

    /// Modulation range (added to min when tcf_score = 1)
    ///
    /// **Default: 0.4**
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// When tcf_score = 1.0, modulation = min + range = 1.0 (full momentum preserved).
    /// When tcf_score = 0.5, modulation = 0.6 + 0.2 = 0.8 (80% momentum).
    pub tcf_modulation_range: f64,

    /// Decay factor for cohesion history integral
    ///
    /// **Default: 0.85**
    ///
    /// **Range: [0.5, 0.99]**
    ///
    /// Controls exponential forgetting of old cohesion values.
    /// Higher = faster forgetting, more responsive to recent changes.
    /// Lower = longer memory, more stable but slower to detect changes.
    pub decay_factor: f64,

    /// Minimum updates before TCF is considered "primed"
    ///
    /// **Default: 3**
    ///
    /// **Range: [2, 10]**
    ///
    /// TCF needs at least this many cycles to establish baseline expectations.
    /// Before primed, TCF returns neutral score (0.5).
    pub min_updates_for_primed: usize,

    /// Direction alignment weight in cohesion calculation
    ///
    /// **Default: 0.40** (40%)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// How much weight is given to direction consistency.
    /// Sum of direction_weight + rhythm_weight + stability_weight should equal 1.0.
    pub cohesion_direction_weight: f64,

    /// Rhythm (magnitude) weight in cohesion calculation
    ///
    /// **Default: 0.30** (30%)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// How much weight is given to volatility similarity.
    pub cohesion_rhythm_weight: f64,

    /// Stability weight in cohesion calculation
    ///
    /// **Default: 0.30** (30%)
    ///
    /// **Range: [0.0, 1.0]**
    ///
    /// How much weight is given to internal consistency preservation.
    pub cohesion_stability_weight: f64,

    /// Volatility sensitivity
    ///
    /// **Default: 2.0**
    ///
    /// **Range: [1.0, 5.0]**
    ///
    /// Higher values = more penalty for volatility spikes.
    /// Increase for aggressive pump detection.
    pub volatility_sensitivity: f64,

    /// Direction sensitivity
    ///
    /// **Default: 1.5**
    ///
    /// **Range: [1.0, 3.0]**
    ///
    /// Higher values = more penalty for direction contradictions.
    pub direction_sensitivity: f64,

    /// Penalty for price-volume divergence
    ///
    /// **Default: 0.15**
    ///
    /// **Range: [0.0, 0.5]**
    ///
    /// Applied when price and volume move in conflicting ways.
    pub price_volume_divergence_penalty: f64,

    /// Maximum bonus for perfect alignment
    ///
    /// **Default: 0.10**
    ///
    /// **Range: [0.0, 0.3]**
    ///
    /// Maximum cohesion bonus when transitions align perfectly.
    pub alignment_bonus_max: f64,

    /// Cliff detection threshold
    ///
    /// **Default: 0.25**
    ///
    /// **Range: [0.1, 0.5]**
    ///
    /// Drop in cohesion that triggers cliff warning.
    /// Higher = less sensitive to sudden changes.
    pub cliff_threshold: f64,

    /// High cohesion threshold
    ///
    /// **Default: 0.7**
    ///
    /// **Range: [0.5, 0.9]**
    ///
    /// Above this is considered "good" cohesion.
    pub high_cohesion_threshold: f64,

    /// Low cohesion threshold
    ///
    /// **Default: 0.4**
    ///
    /// **Range: [0.1, 0.5]**
    ///
    /// Below this triggers consecutive low counter.
    pub low_cohesion_threshold: f64,
}

impl Default for TcfConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            weight_in_final_verdict: 0.15,
            tcf_min_modulation: 0.6,
            tcf_modulation_range: 0.4,
            decay_factor: 0.85,
            min_updates_for_primed: 3,
            cohesion_direction_weight: 0.40,
            cohesion_rhythm_weight: 0.30,
            cohesion_stability_weight: 0.30,
            volatility_sensitivity: 2.0,
            direction_sensitivity: 1.5,
            price_volume_divergence_penalty: 0.15,
            alignment_bonus_max: 0.10,
            cliff_threshold: 0.25,
            high_cohesion_threshold: 0.7,
            low_cohesion_threshold: 0.4,
        }
    }
}

impl TcfConfig {
    /// Validate TCF configuration parameters
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.weight_in_final_verdict < 0.0 || self.weight_in_final_verdict > 1.0 {
            anyhow::bail!("TCF weight_in_final_verdict must be in [0.0, 1.0]");
        }
        if self.tcf_min_modulation < 0.0 || self.tcf_min_modulation > 1.0 {
            anyhow::bail!("TCF tcf_min_modulation must be in [0.0, 1.0]");
        }
        if self.tcf_modulation_range < 0.0 || self.tcf_modulation_range > 1.0 {
            anyhow::bail!("TCF tcf_modulation_range must be in [0.0, 1.0]");
        }
        if self.decay_factor < 0.5 || self.decay_factor > 0.99 {
            anyhow::bail!("TCF decay_factor must be in [0.5, 0.99]");
        }
        if self.min_updates_for_primed < 2 || self.min_updates_for_primed > 10 {
            anyhow::bail!("TCF min_updates_for_primed must be in [2, 10]");
        }

        // Validate cohesion weights sum to approximately 1.0
        let weight_sum = self.cohesion_direction_weight
            + self.cohesion_rhythm_weight
            + self.cohesion_stability_weight;
        if (weight_sum - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "TCF cohesion weights must sum to approximately 1.0, got {}",
                weight_sum
            );
        }

        // Validate sensitivities
        if self.volatility_sensitivity < 1.0 || self.volatility_sensitivity > 5.0 {
            anyhow::bail!("TCF volatility_sensitivity must be in [1.0, 5.0]");
        }
        if self.direction_sensitivity < 1.0 || self.direction_sensitivity > 3.0 {
            anyhow::bail!("TCF direction_sensitivity must be in [1.0, 3.0]");
        }

        // Validate thresholds
        if self.cliff_threshold < 0.1 || self.cliff_threshold > 0.5 {
            anyhow::bail!("TCF cliff_threshold must be in [0.1, 0.5]");
        }
        if self.high_cohesion_threshold < 0.5 || self.high_cohesion_threshold > 0.9 {
            anyhow::bail!("TCF high_cohesion_threshold must be in [0.5, 0.9]");
        }
        if self.low_cohesion_threshold < 0.1 || self.low_cohesion_threshold > 0.5 {
            anyhow::bail!("TCF low_cohesion_threshold must be in [0.1, 0.5]");
        }

        Ok(())
    }

    /// Build CohesionConfig from TcfConfig
    pub fn to_cohesion_config(&self) -> crate::oracle::tcf::CohesionConfig {
        crate::oracle::tcf::CohesionConfig {
            direction_weight: self.cohesion_direction_weight,
            rhythm_weight: self.cohesion_rhythm_weight,
            stability_weight: self.cohesion_stability_weight,
            volatility_sensitivity: self.volatility_sensitivity,
            direction_sensitivity: self.direction_sensitivity,
            price_volume_divergence_penalty: self.price_volume_divergence_penalty,
            alignment_bonus_max: self.alignment_bonus_max,
        }
    }
}

/// BVA (Behavioral Vacuum Analysis) Configuration
///
/// Controls early-stage scoring based on transaction metadata in the first
/// seconds after pool detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BvaConfig {
    /// Primary window in seconds (BVA is dominant)
    #[serde(default = "default_bva_primary_window_secs")]
    pub primary_window_secs: u64,

    /// Slot duration estimate for normalization (ms)
    #[serde(default = "default_bva_slot_duration_ms")]
    pub slot_duration_ms: u64,

    /// Number of slots counted as early reactions
    #[serde(default = "default_bva_early_reaction_slots")]
    pub early_reaction_slots: u64,

    /// Metric weights
    #[serde(default = "default_bva_weight_tds")]
    pub weight_tds: f64,
    #[serde(default = "default_bva_weight_dc")]
    pub weight_dc: f64,
    #[serde(default = "default_bva_weight_se")]
    pub weight_se: f64,
    #[serde(default = "default_bva_weight_cer")]
    pub weight_cer: f64,
    #[serde(default = "default_bva_weight_erp")]
    pub weight_erp: f64,

    /// SOBP directional blend weight (soft input)
    #[serde(default = "default_bva_sobp_dc_blend")]
    pub sobp_dc_blend: f64,

    /// SCR prior weight from BVA classification
    #[serde(default = "default_bva_scr_prior_weight")]
    pub scr_prior_weight: f64,

    /// Minimum weight for CIR echo timing decay
    #[serde(default = "default_bva_cir_min_weight")]
    pub cir_min_weight: f64,

    /// Confidence divisors
    #[serde(default = "default_bva_confidence_tx_divisor")]
    pub confidence_tx_divisor: u64,
    #[serde(default = "default_bva_confidence_signer_divisor")]
    pub confidence_signer_divisor: u64,
    #[serde(default = "default_bva_confidence_time_divisor_ms")]
    pub confidence_time_divisor_ms: u64,

    /// Confidence penalties
    #[serde(default = "default_bva_sobp_fallback_confidence_penalty")]
    pub sobp_fallback_confidence_penalty: f64,
    #[serde(default = "default_bva_congestion_confidence_penalty")]
    pub congestion_confidence_penalty: f64,

    /// Classification thresholds
    #[serde(default = "default_bva_classification_confidence_floor")]
    pub classification_confidence_floor: f64,
    #[serde(default = "default_bva_classification_organic_tds_min")]
    pub classification_organic_tds_min: f64,
    #[serde(default = "default_bva_classification_organic_se_min")]
    pub classification_organic_se_min: f64,
    #[serde(default = "default_bva_classification_organic_dc_min")]
    pub classification_organic_dc_min: f64,
    #[serde(default = "default_bva_classification_steered_dc_min")]
    pub classification_steered_dc_min: f64,
    #[serde(default = "default_bva_classification_steered_erp_min")]
    pub classification_steered_erp_min: f64,
}

/// PANIC (CIR-weighted congestion detector) Configuration
///
/// Controls the 0-7s PANIC pressure/friction heuristic using WS arrival timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicConfig {
    /// Minimum density required to reach full confidence.
    pub min_pressure_for_confidence: f64,
    /// Friction threshold for high-pressure detection.
    pub high_friction_threshold: f64,
    /// Demand spring threshold (pressure ratio) for high-pressure detection.
    pub demand_spring_threshold: f64,
    /// Baseline impulse threshold in tx/s.
    pub impulse_threshold_txps: usize,
    /// Entropy threshold for high-pressure vs bot-spam classification.
    pub entropy_threshold: f64,
    /// Minimum unique signers required to enable entropy.
    pub min_unique_signers: usize,
    /// Confidence cap when entropy is zero.
    pub zero_entropy_confidence_cap: f64,
    /// Confidence cap when unique signers below minimum.
    pub low_signer_confidence_cap: f64,
    /// Max confidence when signer entropy contradicts PANIC entropy.
    pub entropy_inconsistency_confidence_cap: f64,
    /// PANIC confidence threshold for CIR boost eligibility.
    pub cir_confidence_threshold: f64,
    /// Score threshold to mute SOBP.
    pub mute_sobp_score_threshold: f64,
    /// Confidence threshold to mute SOBP.
    pub mute_sobp_confidence_threshold: f64,
    /// Score threshold to mute SCR.
    pub mute_scr_score_threshold: f64,
    /// Friction threshold to mute SCR.
    pub mute_scr_friction_threshold: f64,
    /// SCR fee-spike hint weight.
    pub scr_fee_spike_weight: f64,
    /// SCR failed-ratio hint weight.
    pub scr_failed_ratio_weight: f64,
    /// SCR penalty when signer-entropy inconsistency detected.
    pub scr_inconsistency_penalty: f64,
}

impl Default for PanicConfig {
    fn default() -> Self {
        Self {
            min_pressure_for_confidence: 0.8,
            high_friction_threshold: 0.6,
            demand_spring_threshold: 2.0,
            impulse_threshold_txps: 15,
            entropy_threshold: 0.8,
            min_unique_signers: 3,
            zero_entropy_confidence_cap: 0.3,
            low_signer_confidence_cap: 0.3,
            entropy_inconsistency_confidence_cap: 0.4,
            cir_confidence_threshold: 0.5,
            mute_sobp_score_threshold: 0.7,
            mute_sobp_confidence_threshold: 0.6,
            mute_scr_score_threshold: 0.8,
            mute_scr_friction_threshold: 0.6,
            scr_fee_spike_weight: 0.15,
            scr_failed_ratio_weight: 0.2,
            scr_inconsistency_penalty: 0.15,
        }
    }
}

/// TCR-Φ (Temporal Causality Resonance – Phi Edition) Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcrPhiConfig {
    /// Reaction window in slots for each impact.
    #[serde(default = "default_tcr_phi_window_slots")]
    pub window_size_slots: u64,
    /// Rolling window size for SlotTempo median (samples).
    #[serde(default = "default_tcr_phi_tempo_window")]
    pub tempo_window: usize,
    /// Minimum samples required for timing stability.
    #[serde(default = "default_tcr_phi_min_samples")]
    pub min_samples: usize,
    /// Samples required to reach full confidence.
    #[serde(default = "default_tcr_phi_confidence_samples")]
    pub confidence_samples: usize,
    /// Default slot duration in ms when tempo is unknown.
    #[serde(default = "default_tcr_phi_default_slot_ms")]
    pub default_slot_ms: f64,
    /// Minimum confidence required to emit a score.
    #[serde(default = "default_tcr_phi_min_confidence_emit")]
    pub min_confidence_emit: f64,
    /// Timing threshold for synergy boost.
    #[serde(default = "default_tcr_phi_synergy_timing_threshold")]
    pub synergy_timing_threshold: f64,
    /// Directional bias threshold for synergy boost.
    #[serde(default = "default_tcr_phi_synergy_bias_threshold")]
    pub synergy_bias_threshold: f64,
    /// Synergy boost multiplier.
    #[serde(default = "default_tcr_phi_synergy_boost")]
    pub synergy_boost: f64,
    /// Maximum pending impacts kept in memory.
    #[serde(default = "default_tcr_phi_max_impacts")]
    pub max_impacts: usize,
    /// Enable ECTO integration.
    #[serde(default = "default_tcr_phi_ecto_enabled")]
    pub ecto_enabled: bool,
    /// ECTO influence on Φ curvature (K_PHI).
    #[serde(default = "default_tcr_phi_ecto_k_phi")]
    pub ecto_k_phi: f64,
    /// Multiplier for early event weighting when SNIPER_WALL is present.
    #[serde(default = "default_tcr_phi_ecto_early_event_weight")]
    pub ecto_early_event_weight: f64,
    /// Window in slots considered "early" for event weighting.
    #[serde(default = "default_tcr_phi_ecto_early_window_slots")]
    pub ecto_early_window_slots: u64,
}

impl Default for TcrPhiConfig {
    fn default() -> Self {
        Self {
            window_size_slots: default_tcr_phi_window_slots(),
            tempo_window: default_tcr_phi_tempo_window(),
            min_samples: default_tcr_phi_min_samples(),
            confidence_samples: default_tcr_phi_confidence_samples(),
            default_slot_ms: default_tcr_phi_default_slot_ms(),
            min_confidence_emit: default_tcr_phi_min_confidence_emit(),
            synergy_timing_threshold: default_tcr_phi_synergy_timing_threshold(),
            synergy_bias_threshold: default_tcr_phi_synergy_bias_threshold(),
            synergy_boost: default_tcr_phi_synergy_boost(),
            max_impacts: default_tcr_phi_max_impacts(),
            ecto_enabled: default_tcr_phi_ecto_enabled(),
            ecto_k_phi: default_tcr_phi_ecto_k_phi(),
            ecto_early_event_weight: default_tcr_phi_ecto_early_event_weight(),
            ecto_early_window_slots: default_tcr_phi_ecto_early_window_slots(),
        }
    }
}

impl TcrPhiConfig {
    /// Validate TCR-Φ config contract (warning-level violations).
    pub fn validate(&self) -> anyhow::Result<()> {
        let max_tempo = (self.window_size_slots.saturating_mul(5)).max(1) as usize;

        if self.min_samples < 2 {
            warn!("TCR-Φ config: min_samples < 2 (contract violation)");
        }
        if self.tempo_window == 0 {
            warn!("TCR-Φ config: tempo_window == 0 (contract violation)");
        }
        if self.tempo_window > max_tempo {
            warn!(
                "TCR-Φ config: tempo_window {} > 5x window_size ({})",
                self.tempo_window, max_tempo
            );
        }
        if self.confidence_samples < self.min_samples {
            warn!(
                "TCR-Φ config: confidence_samples < min_samples ({} < {})",
                self.confidence_samples, self.min_samples
            );
        }
        if !self.default_slot_ms.is_finite() || self.default_slot_ms <= 0.0 {
            warn!("TCR-Φ config: default_slot_ms invalid");
        }
        if !(0.0..=1.0).contains(&self.min_confidence_emit) {
            warn!("TCR-Φ config: min_confidence_emit out of [0,1]");
        }
        if !(0.2..=0.5).contains(&self.ecto_k_phi) {
            warn!("TCR-Φ config: ecto_k_phi out of [0.2,0.5]");
        }
        if !self.ecto_early_event_weight.is_finite() || self.ecto_early_event_weight < 1.0 {
            warn!("TCR-Φ config: ecto_early_event_weight < 1.0");
        }
        if self.ecto_early_window_slots == 0 {
            warn!("TCR-Φ config: ecto_early_window_slots == 0");
        }

        Ok(())
    }
}

/// Behavioral scoring configuration (ECTO/BVA/PANIC/TCR/CIR)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralScoringConfig {
    /// Enable behavioral scoring multiplier
    #[serde(default = "default_behavioral_enabled")]
    pub enabled: bool,
    /// Weight for ECTO score
    #[serde(default = "default_behavioral_w_ecto")]
    pub w_ecto: f32,
    /// Weight for BVA score
    #[serde(default = "default_behavioral_w_bva")]
    pub w_bva: f32,
    /// Weight for PANIC pressure
    #[serde(default = "default_behavioral_w_panic")]
    pub w_panic: f32,
    /// Weight for TCR causality
    #[serde(default = "default_behavioral_w_tcr")]
    pub w_tcr: f32,
    /// Weight for CIR strength
    #[serde(default = "default_behavioral_w_cir")]
    pub w_cir: f32,
    /// Early-stage window in seconds
    #[serde(default = "default_behavioral_early_stage_seconds")]
    pub early_stage_seconds: u64,
    /// Minimum behavioral floor to avoid zeroing
    #[serde(default = "default_behavioral_min_floor")]
    pub min_behavioral_floor: f32,
    /// Use additive behavioral modulation (true) or legacy multiplicative mode (false).
    #[serde(default = "default_behavioral_use_additive_mode")]
    pub use_additive_mode: bool,
    /// Maximum score adjustment in points for additive behavioral modulation.
    #[serde(default = "default_behavioral_max_adjustment_points")]
    pub max_adjustment_points: f32,
    /// Neutral behavioral point where additive modulation contributes zero offset.
    #[serde(default = "default_behavioral_neutral_point")]
    pub neutral_point: f32,
}

impl Default for BehavioralScoringConfig {
    fn default() -> Self {
        Self {
            enabled: default_behavioral_enabled(),
            w_ecto: default_behavioral_w_ecto(),
            w_bva: default_behavioral_w_bva(),
            w_panic: default_behavioral_w_panic(),
            w_tcr: default_behavioral_w_tcr(),
            w_cir: default_behavioral_w_cir(),
            early_stage_seconds: default_behavioral_early_stage_seconds(),
            min_behavioral_floor: default_behavioral_min_floor(),
            use_additive_mode: default_behavioral_use_additive_mode(),
            max_adjustment_points: default_behavioral_max_adjustment_points(),
            neutral_point: default_behavioral_neutral_point(),
        }
    }
}

impl Default for BvaConfig {
    fn default() -> Self {
        Self {
            primary_window_secs: default_bva_primary_window_secs(),
            slot_duration_ms: default_bva_slot_duration_ms(),
            early_reaction_slots: default_bva_early_reaction_slots(),
            weight_tds: default_bva_weight_tds(),
            weight_dc: default_bva_weight_dc(),
            weight_se: default_bva_weight_se(),
            weight_cer: default_bva_weight_cer(),
            weight_erp: default_bva_weight_erp(),
            sobp_dc_blend: default_bva_sobp_dc_blend(),
            scr_prior_weight: default_bva_scr_prior_weight(),
            cir_min_weight: default_bva_cir_min_weight(),
            confidence_tx_divisor: default_bva_confidence_tx_divisor(),
            confidence_signer_divisor: default_bva_confidence_signer_divisor(),
            confidence_time_divisor_ms: default_bva_confidence_time_divisor_ms(),
            sobp_fallback_confidence_penalty: default_bva_sobp_fallback_confidence_penalty(),
            congestion_confidence_penalty: default_bva_congestion_confidence_penalty(),
            classification_confidence_floor: default_bva_classification_confidence_floor(),
            classification_organic_tds_min: default_bva_classification_organic_tds_min(),
            classification_organic_se_min: default_bva_classification_organic_se_min(),
            classification_organic_dc_min: default_bva_classification_organic_dc_min(),
            classification_steered_dc_min: default_bva_classification_steered_dc_min(),
            classification_steered_erp_min: default_bva_classification_steered_erp_min(),
        }
    }
}

impl BvaConfig {
    /// Validate BVA configuration parameters
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.primary_window_secs == 0 {
            anyhow::bail!("BVA primary_window_secs must be > 0");
        }
        if self.slot_duration_ms == 0 {
            anyhow::bail!("BVA slot_duration_ms must be > 0");
        }
        let metric_sum =
            self.weight_tds + self.weight_dc + self.weight_se + self.weight_cer + self.weight_erp;
        if (metric_sum - 1.0).abs() > 0.01 {
            anyhow::bail!("BVA metric weights must sum to ~1.0, got {}", metric_sum);
        }
        if self.confidence_tx_divisor == 0
            || self.confidence_signer_divisor == 0
            || self.confidence_time_divisor_ms == 0
        {
            anyhow::bail!("BVA confidence divisors must be > 0");
        }
        if !(0.0..=1.0).contains(&self.sobp_fallback_confidence_penalty) {
            anyhow::bail!("BVA sobp_fallback_confidence_penalty must be in [0.0, 1.0]");
        }
        if !(0.0..=1.0).contains(&self.sobp_dc_blend) {
            anyhow::bail!("BVA sobp_dc_blend must be in [0.0, 1.0]");
        }
        if !(0.0..=1.0).contains(&self.scr_prior_weight) {
            anyhow::bail!("BVA scr_prior_weight must be in [0.0, 1.0]");
        }
        if !(0.0..=1.0).contains(&self.cir_min_weight) {
            anyhow::bail!("BVA cir_min_weight must be in [0.0, 1.0]");
        }
        if !(0.0..=1.0).contains(&self.congestion_confidence_penalty) {
            anyhow::bail!("BVA congestion_confidence_penalty must be in [0.0, 1.0]");
        }
        if !(0.0..=1.0).contains(&self.classification_confidence_floor) {
            anyhow::bail!("BVA classification_confidence_floor must be in [0.0, 1.0]");
        }
        for (name, value) in [
            (
                "classification_organic_tds_min",
                self.classification_organic_tds_min,
            ),
            (
                "classification_organic_se_min",
                self.classification_organic_se_min,
            ),
            (
                "classification_organic_dc_min",
                self.classification_organic_dc_min,
            ),
            (
                "classification_steered_dc_min",
                self.classification_steered_dc_min,
            ),
            (
                "classification_steered_erp_min",
                self.classification_steered_erp_min,
            ),
        ] {
            if !(0.0..=1.0).contains(&value) {
                anyhow::bail!("BVA {} must be in [0.0, 1.0]", name);
            }
        }
        Ok(())
    }
}

/// Normalization Configuration (Operation Scale Master)
///
/// Controls dynamic scaling of market data for activation functions (Sigmoid/Tanh).
/// Prevents "flattened graphs" where functions return values close to 0 or 1
/// due to improper input data scaling.
///
/// ## Purpose
/// When testing on small pools (e.g., liquidity $500), without proper scaling,
/// normalized output might be ~0.01, causing sigmoid/tanh to flatten.
/// These parameters allow adjusting the "midpoint" of normalization to match
/// the actual scale of your target market.
///
/// ## Example
/// For small pools ($500-$5000 liquidity):
/// - `liquidity_scale = 2500.0` (midpoint at $2.5k)
/// - `volume_scale = 1000.0` (midpoint at $1k)
///
/// For large pools ($50k-$500k liquidity):
/// - `liquidity_scale = 250000.0` (midpoint at $250k)
/// - `volume_scale = 100000.0` (midpoint at $100k)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizationConfig {
    /// Liquidity normalization scale (SOL)
    ///
    /// This is the "midpoint" value for sigmoid normalization of liquidity.
    /// At this value, sigmoid(x/scale) ≈ 0.73 (inflection point).
    ///
    /// Lower values = higher sensitivity for small pools
    /// Higher values = appropriate for large pools
    ///
    /// Default: 25000.0 (suitable for mid-size pools)
    /// Range: [100.0, 1000000.0]
    pub liquidity_scale: f64,

    /// Volume normalization scale (SOL)
    ///
    /// Controls the midpoint for volume sigmoid/tanh functions.
    /// Similar to liquidity_scale but for trading volume.
    ///
    /// Default: 10000.0
    /// Range: [100.0, 1000000.0]
    pub volume_scale: f64,

    /// Volatility factor for tanh normalization
    ///
    /// Higher values = more aggressive volatility response
    /// Lower values = smoother, less sensitive response
    ///
    /// Default: 5.0
    /// Range: [0.1, 100.0]
    pub volatility_factor: f64,
}

/// Weight profile selector for different market conditions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum WeightProfile {
    /// Standard profile for young pools (< 2 min): High QASS/Virality
    Standard,
    /// Reversal profile for mature pools (> 10 min): High QEDD/Stability + SOBP
    Reversal,
}

/// Weight profile configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightProfiles {
    /// Standard profile weights (for young pools)
    pub standard: ProfileWeights,
    /// Reversal profile weights (for mature pools)
    pub reversal: ProfileWeights,
}

/// Individual profile weights for confidence calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileWeights {
    pub weight_qass: f32,
    pub weight_sobp: f32,
    pub weight_mpcf: f32,
    pub weight_iwim: f32,
    pub weight_ssmi: f32,
    pub weight_qofsv: f32,
    pub weight_scr: f32,
    pub weight_frb: f32,
    pub weight_qman: f32,
    pub weight_gene_mapper: f32,
    #[serde(alias = "weight_chaos")]
    pub weight_chaos_engine: f32,
}

/// Confidence Model Configuration
///
/// Controls confidence calculation weights and thresholds for Oracle Brain
/// decision-making. These parameters determine how much each module contributes
/// to the overall confidence score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceConfig {
    // Module weights
    /// SOBP (Slot-Over-Slot Buying Pressure) weight
    /// Default: 12.0
    /// Range: [0.0, infinity)
    pub weight_sobp: f32,

    /// MPCF (Micro-Payload Cognitive Fingerprint) weight
    /// Default: 10.0
    /// Range: [0.0, infinity)
    pub weight_mpcf: f32,

    /// IWIM (Inter-Wallet Interaction Matrix) weight
    /// Default: 8.0
    /// Range: [0.0, infinity)
    pub weight_iwim: f32,

    /// SSMI (Sub-Slot Microentropy Index) weight
    /// Default: 9.0
    /// Range: [0.0, infinity)
    pub weight_ssmi: f32,

    /// QASS (Quantum Amplitude Scoring System) weight
    /// Default: 15.0
    /// Range: [0.0, infinity)
    pub weight_qass: f32,

    /// QOFSV (Quantum Orderflow Shadow Vector) weight
    /// Default: 11.0
    /// Range: [0.0, infinity)
    pub weight_qofsv: f32,

    /// SCR (Slot-Coherence Resonance) weight
    /// Default: 13.0
    /// Range: [0.0, infinity)
    pub weight_scr: f32,

    /// FRB (Flow Resonance Broker) weight
    /// Default: 7.0
    /// Range: [0.0, infinity)
    pub weight_frb: f32,

    /// QMAN (Quantum Market Anomaly Navigator) weight
    /// Default: 14.0
    /// Range: [0.0, infinity)
    pub weight_qman: f32,

    /// GeneMapper (Pattern Recognition) weight
    /// Default: 10.0
    /// Range: [0.0, infinity)
    pub weight_gene_mapper: f32,

    /// ChaosEngine (Monte Carlo Simulation) weight
    /// Default: 11.0
    /// Range: [0.0, infinity)
    #[serde(alias = "weight_chaos")]
    pub weight_chaos_engine: f32,

    /// Weight profile configuration
    #[serde(default)]
    pub profiles: WeightProfiles,

    // Decision thresholds
    /// High confidence threshold for full conviction decisions
    /// Default: 0.8
    /// Range: [0.0, 1.0]
    #[serde(alias = "threshold_high_confidence")]
    pub threshold_high: f32,

    /// Medium confidence threshold for moderate conviction decisions
    /// Default: 0.5
    /// Range: [0.0, 1.0]
    #[serde(alias = "threshold_medium_confidence")]
    pub threshold_medium: f32,

    /// Low confidence threshold for minimal/skip decisions
    /// Below this value indicates very low confidence
    /// Default: 0.3
    /// Range: [0.0, 1.0]
    #[serde(alias = "threshold_low_confidence")]
    pub threshold_low: f32,
}

/// Scoring Weights Configuration
///
/// Controls all penalty and boost multipliers in the HyperPrediction scoring system.
/// These weights allow fine-tuning of the scoring behavior without code changes.
///
/// ## Design Principles
///
/// - **Baseline = 1.0**: All multipliers default to 1.0 (neutral)
/// - **Increase > 1.0**: Makes penalties/boosts stronger
/// - **Decrease < 1.0**: Makes penalties/boosts weaker
/// - **Set to 0.0**: Disables the penalty/boost entirely
///
/// ## Example Usage
///
/// ```toml
/// [scoring]
/// # Make wash trading penalties 50% stronger
/// wash_penalty_mult = 1.5
///
/// # Make organic activity boosts 20% weaker
/// organic_boost_mult = 0.8
///
/// # Disable SSMI bot penalties entirely
/// ssmi_bot_penalty_mult = 0.0
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeightsConfig {
    // ============= Signal Weights =============
    /// Weight for LIGMA signal in scoring (default: 1.0)
    #[serde(default = "default_one")]
    pub ligma: f32,

    /// Weight for QEDD signal in scoring (default: 1.0)
    #[serde(default = "default_one")]
    pub qedd: f32,

    /// Weight for SurvivorScore in final calculation (default: 1.0)
    #[serde(default = "default_one")]
    pub survivor: f32,

    /// Weight for QASS as secondary modifier (default: 1.0)
    #[serde(default = "default_one")]
    pub qass_secondary: f32,

    /// Weight for MCI signal in scoring (default: 1.0)
    #[serde(default = "default_one")]
    pub mci: f32,

    /// Weight for Cluster analysis in scoring (default: 1.0)
    #[serde(default = "default_one")]
    pub cluster: f32,

    /// Weight for Chaos Engine results (default: 1.0)
    #[serde(default = "default_one")]
    pub chaos: f32,

    // ============= Penalty Multipliers =============
    /// Multiplier for wash trading penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub wash_penalty_mult: f32,

    /// Multiplier for bot pattern penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub bot_penalty_mult: f32,

    /// Multiplier for rug threat penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub rug_penalty_mult: f32,

    /// Multiplier for cluster cabal penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub cluster_penalty_mult: f32,

    /// Multiplier for SSMI bot detection penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub ssmi_bot_penalty_mult: f32,

    /// Multiplier for SCR bot detection penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub scr_penalty_mult: f32,

    /// Multiplier for ULVF divergence penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub ulvf_div_penalty_mult: f32,

    /// Multiplier for ULVF curl penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub ulvf_curl_penalty_mult: f32,

    /// Multiplier for POVC cluster penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub povc_penalty_mult: f32,

    /// Multiplier for POVC organic cluster boost (default: 1.0)
    #[serde(default = "default_one")]
    pub povc_organic_boost_mult: f32,

    /// Multiplier for MPCF sniper/MEV penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub mpcf_sniper_penalty_mult: f32,

    /// Multiplier for MPCF sybil penalty (default: 1.0)
    #[serde(default = "default_one")]
    pub mpcf_sybil_penalty_mult: f32,

    // ============= Boost Multipliers =============
    /// Multiplier for organic activity boost (default: 1.0)
    #[serde(default = "default_one")]
    pub organic_boost_mult: f32,

    /// Multiplier for smart money boost (default: 1.0)
    #[serde(default = "default_one")]
    pub smart_money_boost_mult: f32,

    /// Multiplier for SSMI viral launch boost (default: 1.0)
    #[serde(default = "default_one")]
    pub ssmi_viral_boost_mult: f32,

    /// Multiplier for SSMI human boost (default: 1.0)
    #[serde(default = "default_one")]
    pub ssmi_human_boost_mult: f32,

    /// Multiplier for MESA organic bonus (default: 1.0)
    #[serde(default = "default_one")]
    pub mesa_organic_boost_mult: f32,

    /// Multiplier for MESA entropy bonus (default: 1.0)
    #[serde(default = "default_one")]
    pub mesa_entropy_boost_mult: f32,

    /// Multiplier for Chaos pump probability boost (default: 1.0)
    #[serde(default = "default_one")]
    pub chaos_pump_boost_mult: f32,

    /// Multiplier for Resonance human detection boost (default: 1.0)
    #[serde(default = "default_one")]
    pub resonance_human_boost_mult: f32,

    /// Multiplier for clean cluster bonus (default: 1.0)
    #[serde(default = "default_one")]
    pub cluster_clean_boost_mult: f32,
}

/// Helper function for serde default value
const fn default_one() -> f32 {
    1.0
}

// ==============================================================================
// 🎯 SECTION 9: SURVIVOR SCORE CONFIGURATION
// ==============================================================================

/// Survivor Score Component Configuration
///
/// Controls all weights and sub-component configurations for the SurvivorScore
/// calculation system. This replaces hardcoded values in survivor_score.rs.
///
/// ## Formula
///
/// ```text
/// SurvivorScore = (survival)^Ws × (momentum)^Wm × (quality)^Wq × (1 - risk_discount)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorScoreComponentConfig {
    /// Exponent weight for survival component (NOT linear weight - this is a power)
    /// Default: 0.35
    /// Range: [0.0, 1.0]
    #[serde(default = "default_weight_survival")]
    pub weight_survival: f32,

    /// Exponent weight for momentum component (NOT linear weight - this is a power)
    /// Default: 0.30
    /// Range: [0.0, 1.0]
    #[serde(default = "default_weight_momentum")]
    pub weight_momentum: f32,

    /// Exponent weight for quality component (NOT linear weight - this is a power)
    /// Default: 0.20
    /// Range: [0.0, 1.0]
    #[serde(default = "default_weight_quality")]
    pub weight_quality: f32,

    /// Survival component weights for scoring cycles (no IWIM)
    #[serde(default)]
    pub survival_cycle: SurvivalCycleWeights,

    /// Survival component weights for Final Verdict (with IWIM)
    #[serde(default)]
    pub survival_final_verdict: SurvivalFinalVerdictWeights,

    /// Quality component weights for Early Stage (S1-S6, no SCR)
    #[serde(default)]
    pub quality_early_stage: QualityEarlyStageWeights,

    /// Quality component weights for Full Analysis (S7-S12, with SCR)
    #[serde(default)]
    pub quality_full_analysis: QualityFullAnalysisWeights,

    /// Momentum component configuration
    #[serde(default)]
    pub momentum: MomentumConfig,
}

/// Survival component weights for scoring cycles (no IWIM)
///
/// During cycles S1-S13, IWIM is NOT included in survival calculation.
/// Weights are redistributed: QEDD (62.5%) + Cluster (37.5%)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivalCycleWeights {
    /// QEDD weight in survival calculation during cycles
    /// Default: 0.625
    #[serde(default = "default_survival_cycle_qedd")]
    pub qedd: f32,

    /// Cluster weight in survival calculation during cycles
    /// Default: 0.375
    #[serde(default = "default_survival_cycle_cluster")]
    pub cluster: f32,
}

/// Survival component weights for Final Verdict (with IWIM)
///
/// At Final Verdict, IWIM is included: QEDD (50%) + IWIM (30%) + Cluster (20%)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivalFinalVerdictWeights {
    /// QEDD weight in survival calculation at Final Verdict
    /// Default: 0.5
    #[serde(default = "default_survival_final_qedd")]
    pub qedd: f32,

    /// IWIM weight in survival calculation at Final Verdict
    /// Default: 0.3
    #[serde(default = "default_survival_final_iwim")]
    pub iwim: f32,

    /// Cluster weight in survival calculation at Final Verdict
    /// Default: 0.2
    #[serde(default = "default_survival_final_cluster")]
    pub cluster: f32,
}

/// Quality component weights for Early Stage (S1-S6)
///
/// SCR is excluded because FFT requires ~20+ samples for statistical significance.
/// Formula: quality = mpcf * w_mpcf + mesa * w_mesa + wallet * w_wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityEarlyStageWeights {
    /// MPCF weight in Early Stage quality calculation
    /// Default: 0.44
    #[serde(default = "default_quality_early_mpcf")]
    pub mpcf: f32,

    /// MESA weight in Early Stage quality calculation
    /// Default: 0.31
    #[serde(default = "default_quality_early_mesa")]
    pub mesa: f32,

    /// Wallet ratio weight in Early Stage quality calculation
    /// Default: 0.25
    #[serde(default = "default_quality_early_wallet")]
    pub wallet: f32,
}

/// Quality component weights for Full Analysis (S7-S12)
///
/// All metrics active including SCR for bot detection.
/// Formula: quality = mpcf * w_mpcf + mesa * w_mesa + scr * w_scr + wallet * w_wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityFullAnalysisWeights {
    /// MPCF weight in Full Analysis quality calculation
    /// Default: 0.35
    #[serde(default = "default_quality_full_mpcf")]
    pub mpcf: f32,

    /// MESA weight in Full Analysis quality calculation
    /// Default: 0.25
    #[serde(default = "default_quality_full_mesa")]
    pub mesa: f32,

    /// SCR (inverted) weight in Full Analysis quality calculation
    /// Default: 0.20
    #[serde(default = "default_quality_full_scr")]
    pub scr: f32,

    /// Wallet ratio weight in Full Analysis quality calculation
    /// Default: 0.20
    #[serde(default = "default_quality_full_wallet")]
    pub wallet: f32,
}

/// Momentum component configuration
///
/// Controls how SOBP, QMAN, and Chaos signals contribute to momentum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentumConfig {
    /// Base value for SOBP momentum calculation
    /// Default: 1.0
    #[serde(default = "default_momentum_sobp_base")]
    pub sobp_base: f32,

    /// Multiplier for SOBP momentum contribution
    /// Default: 0.5
    #[serde(default = "default_momentum_sobp_mult")]
    pub sobp_multiplier: f32,

    /// Base value for Chaos Engine contribution
    /// Default: 0.7
    #[serde(default = "default_momentum_chaos_base")]
    pub chaos_base: f32,

    /// Multiplier for Chaos Engine pump probability
    /// Default: 0.6
    #[serde(default = "default_momentum_chaos_mult")]
    pub chaos_multiplier: f32,
}

// Default value functions for SurvivorScoreComponentConfig
const fn default_weight_survival() -> f32 {
    0.35
}
const fn default_weight_momentum() -> f32 {
    0.30
}
const fn default_weight_quality() -> f32 {
    0.20
}

// Default value functions for SurvivalCycleWeights
const fn default_survival_cycle_qedd() -> f32 {
    0.625
}
const fn default_survival_cycle_cluster() -> f32 {
    0.375
}

// Default value functions for SurvivalFinalVerdictWeights
const fn default_survival_final_qedd() -> f32 {
    0.5
}
const fn default_survival_final_iwim() -> f32 {
    0.3
}
const fn default_survival_final_cluster() -> f32 {
    0.2
}

// Default value functions for QualityEarlyStageWeights
const fn default_quality_early_mpcf() -> f32 {
    0.44
}
const fn default_quality_early_mesa() -> f32 {
    0.31
}
const fn default_quality_early_wallet() -> f32 {
    0.25
}

// Default value functions for QualityFullAnalysisWeights
const fn default_quality_full_mpcf() -> f32 {
    0.35
}
const fn default_quality_full_mesa() -> f32 {
    0.25
}
const fn default_quality_full_scr() -> f32 {
    0.20
}
const fn default_quality_full_wallet() -> f32 {
    0.20
}

// Default value functions for MomentumConfig
const fn default_momentum_sobp_base() -> f32 {
    1.0
}
const fn default_momentum_sobp_mult() -> f32 {
    0.5
}
const fn default_momentum_chaos_base() -> f32 {
    0.7
}
const fn default_momentum_chaos_mult() -> f32 {
    0.6
}

impl Default for SurvivorScoreComponentConfig {
    fn default() -> Self {
        Self {
            weight_survival: default_weight_survival(),
            weight_momentum: default_weight_momentum(),
            weight_quality: default_weight_quality(),
            survival_cycle: SurvivalCycleWeights::default(),
            survival_final_verdict: SurvivalFinalVerdictWeights::default(),
            quality_early_stage: QualityEarlyStageWeights::default(),
            quality_full_analysis: QualityFullAnalysisWeights::default(),
            momentum: MomentumConfig::default(),
        }
    }
}

impl Default for SurvivalCycleWeights {
    fn default() -> Self {
        Self {
            qedd: default_survival_cycle_qedd(),
            cluster: default_survival_cycle_cluster(),
        }
    }
}

impl Default for SurvivalFinalVerdictWeights {
    fn default() -> Self {
        Self {
            qedd: default_survival_final_qedd(),
            iwim: default_survival_final_iwim(),
            cluster: default_survival_final_cluster(),
        }
    }
}

impl Default for QualityEarlyStageWeights {
    fn default() -> Self {
        Self {
            mpcf: default_quality_early_mpcf(),
            mesa: default_quality_early_mesa(),
            wallet: default_quality_early_wallet(),
        }
    }
}

impl Default for QualityFullAnalysisWeights {
    fn default() -> Self {
        Self {
            mpcf: default_quality_full_mpcf(),
            mesa: default_quality_full_mesa(),
            scr: default_quality_full_scr(),
            wallet: default_quality_full_wallet(),
        }
    }
}

impl Default for MomentumConfig {
    fn default() -> Self {
        Self {
            sobp_base: default_momentum_sobp_base(),
            sobp_multiplier: default_momentum_sobp_mult(),
            chaos_base: default_momentum_chaos_base(),
            chaos_multiplier: default_momentum_chaos_mult(),
        }
    }
}

impl SurvivorScoreComponentConfig {
    /// Validate that all weights are within acceptable ranges
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate exponent weights are in [0.0, 1.0]
        let exponents = [
            ("weight_survival", self.weight_survival),
            ("weight_momentum", self.weight_momentum),
            ("weight_quality", self.weight_quality),
        ];
        for (name, value) in &exponents {
            if !(0.0..=1.0).contains(value) {
                anyhow::bail!(
                    "survivor_score.{} must be in [0.0, 1.0], got {}",
                    name,
                    value
                );
            }
        }

        // Validate survival_cycle weights sum to approximately 1.0
        let survival_cycle_sum = self.survival_cycle.qedd + self.survival_cycle.cluster;
        if (survival_cycle_sum - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "survivor_score.survival_cycle weights must sum to 1.0, got {}",
                survival_cycle_sum
            );
        }

        // Validate survival_final_verdict weights sum to approximately 1.0
        let survival_final_sum = self.survival_final_verdict.qedd
            + self.survival_final_verdict.iwim
            + self.survival_final_verdict.cluster;
        if (survival_final_sum - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "survivor_score.survival_final_verdict weights must sum to 1.0, got {}",
                survival_final_sum
            );
        }

        // Validate quality_early_stage weights sum to approximately 1.0
        let quality_early_sum = self.quality_early_stage.mpcf
            + self.quality_early_stage.mesa
            + self.quality_early_stage.wallet;
        if (quality_early_sum - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "survivor_score.quality_early_stage weights must sum to 1.0, got {}",
                quality_early_sum
            );
        }

        // Validate quality_full_analysis weights sum to approximately 1.0
        let quality_full_sum = self.quality_full_analysis.mpcf
            + self.quality_full_analysis.mesa
            + self.quality_full_analysis.scr
            + self.quality_full_analysis.wallet;
        if (quality_full_sum - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "survivor_score.quality_full_analysis weights must sum to 1.0, got {}",
                quality_full_sum
            );
        }

        Ok(())
    }
}

// ==============================================================================
// 🎯 SECTION 10: CYCLE WEIGHTS & GUNSHOT THRESHOLDS
// ==============================================================================

/// Cycle Weights Configuration
///
/// Controls the exponential weights for scoring cycles S1-S12.
/// Later cycles have exponentially higher influence on the final decision.
///
/// Weight distribution follows geometric progression with multiplier ~1.3:
/// - S1-S6 (Early Stage): Lower weights (1.3 to 4.6)
/// - S7-S12 (Full Analysis): Higher weights (6.0 to 22.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleWeightsConfig {
    /// Weight for cycle S1 (default: 1.3)
    #[serde(default = "default_cw_s1")]
    pub s1: f32,

    /// Weight for cycle S2 (default: 1.7)
    #[serde(default = "default_cw_s2")]
    pub s2: f32,

    /// Weight for cycle S3 (default: 2.2)
    #[serde(default = "default_cw_s3")]
    pub s3: f32,

    /// Weight for cycle S4 (default: 2.8)
    #[serde(default = "default_cw_s4")]
    pub s4: f32,

    /// Weight for cycle S5 (default: 3.6)
    #[serde(default = "default_cw_s5")]
    pub s5: f32,

    /// Weight for cycle S6 (end of Early Stage, default: 4.6)
    #[serde(default = "default_cw_s6")]
    pub s6: f32,

    /// Weight for cycle S7 (start of Full Analysis, default: 6.0)
    #[serde(default = "default_cw_s7")]
    pub s7: f32,

    /// Weight for cycle S8 (default: 7.8)
    #[serde(default = "default_cw_s8")]
    pub s8: f32,

    /// Weight for cycle S9 (default: 10.0)
    #[serde(default = "default_cw_s9")]
    pub s9: f32,

    /// Weight for cycle S10 (default: 13.0)
    #[serde(default = "default_cw_s10")]
    pub s10: f32,

    /// Weight for cycle S11 (default: 17.0)
    #[serde(default = "default_cw_s11")]
    pub s11: f32,

    /// Weight for cycle S12 (Dominant weight, default: 22.0)
    #[serde(default = "default_cw_s12")]
    pub s12: f32,
}

// Default value functions for CycleWeightsConfig
const fn default_cw_s1() -> f32 {
    1.3
}
const fn default_cw_s2() -> f32 {
    1.7
}
const fn default_cw_s3() -> f32 {
    2.2
}
const fn default_cw_s4() -> f32 {
    2.8
}
const fn default_cw_s5() -> f32 {
    3.6
}
const fn default_cw_s6() -> f32 {
    4.6
}
const fn default_cw_s7() -> f32 {
    6.0
}
const fn default_cw_s8() -> f32 {
    7.8
}
const fn default_cw_s9() -> f32 {
    10.0
}
const fn default_cw_s10() -> f32 {
    13.0
}
const fn default_cw_s11() -> f32 {
    17.0
}
const fn default_cw_s12() -> f32 {
    22.0
}

impl Default for CycleWeightsConfig {
    fn default() -> Self {
        Self {
            s1: default_cw_s1(),
            s2: default_cw_s2(),
            s3: default_cw_s3(),
            s4: default_cw_s4(),
            s5: default_cw_s5(),
            s6: default_cw_s6(),
            s7: default_cw_s7(),
            s8: default_cw_s8(),
            s9: default_cw_s9(),
            s10: default_cw_s10(),
            s11: default_cw_s11(),
            s12: default_cw_s12(),
        }
    }
}

impl CycleWeightsConfig {
    /// Get weight for a specific cycle (0-indexed 0..11)
    /// Returns 1.0 if out of bounds (fallback)
    pub fn get_weight(&self, cycle_idx: usize) -> f32 {
        match cycle_idx {
            0 => self.s1,
            1 => self.s2,
            2 => self.s3,
            3 => self.s4,
            4 => self.s5,
            5 => self.s6,
            6 => self.s7,
            7 => self.s8,
            8 => self.s9,
            9 => self.s10,
            10 => self.s11,
            11 => self.s12,
            _ => 1.0,
        }
    }

    /// Get all weights as an array for iteration
    pub fn as_array(&self) -> [f32; 12] {
        [
            self.s1, self.s2, self.s3, self.s4, self.s5, self.s6, self.s7, self.s8, self.s9,
            self.s10, self.s11, self.s12,
        ]
    }

    /// Calculate the sum of all weights (for normalization)
    pub fn sum(&self) -> f32 {
        self.s1
            + self.s2
            + self.s3
            + self.s4
            + self.s5
            + self.s6
            + self.s7
            + self.s8
            + self.s9
            + self.s10
            + self.s11
            + self.s12
    }

    /// Validate that all weights are positive
    pub fn validate(&self) -> anyhow::Result<()> {
        let weights = [
            ("s1", self.s1),
            ("s2", self.s2),
            ("s3", self.s3),
            ("s4", self.s4),
            ("s5", self.s5),
            ("s6", self.s6),
            ("s7", self.s7),
            ("s8", self.s8),
            ("s9", self.s9),
            ("s10", self.s10),
            ("s11", self.s11),
            ("s12", self.s12),
        ];
        for (name, value) in &weights {
            if *value <= 0.0 {
                anyhow::bail!("cycle_weights.{} must be positive, got {}", name, value);
            }
        }
        Ok(())
    }
}

/// Gunshot Thresholds Configuration
///
/// Controls the immediate buy trigger thresholds for each scoring cycle.
/// If the raw score in a cycle meets or exceeds this threshold, buy immediately.
///
/// Early Stage (S1-S6): Higher thresholds due to limited data confidence
/// Full Analysis (S7-S12): Lower thresholds as confidence increases
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GunshotThresholdsConfig {
    /// Threshold for cycle S1 (God Candle - requires perfection, default: 100.0)
    #[serde(default = "default_gs_s1")]
    pub s1: f32,

    /// Threshold for cycle S2 (default: 99.0)
    #[serde(default = "default_gs_s2")]
    pub s2: f32,

    /// Threshold for cycle S3 (default: 98.0)
    #[serde(default = "default_gs_s3")]
    pub s3: f32,

    /// Threshold for cycle S4 (default: 97.0)
    #[serde(default = "default_gs_s4")]
    pub s4: f32,

    /// Threshold for cycle S5 (default: 96.0)
    #[serde(default = "default_gs_s5")]
    pub s5: f32,

    /// Threshold for cycle S6 (end of Early Stage, default: 95.0)
    #[serde(default = "default_gs_s6")]
    pub s6: f32,

    /// Threshold for cycle S7 (Transition to Full Analysis, default: 88.0)
    #[serde(default = "default_gs_s7")]
    pub s7: f32,

    /// Threshold for cycle S8 (default: 87.0)
    #[serde(default = "default_gs_s8")]
    pub s8: f32,

    /// Threshold for cycle S9 (default: 86.0)
    #[serde(default = "default_gs_s9")]
    pub s9: f32,

    /// Threshold for cycle S10 (Strong trend confirmation, default: 85.0)
    #[serde(default = "default_gs_s10")]
    pub s10: f32,

    /// Threshold for cycle S11 (default: 83.5)
    #[serde(default = "default_gs_s11")]
    pub s11: f32,

    /// Threshold for cycle S12 (Final Stand, default: 82.0)
    #[serde(default = "default_gs_s12")]
    pub s12: f32,
}

// Default value functions for GunshotThresholdsConfig
const fn default_gs_s1() -> f32 {
    100.0
}
const fn default_gs_s2() -> f32 {
    99.0
}
const fn default_gs_s3() -> f32 {
    98.0
}
const fn default_gs_s4() -> f32 {
    97.0
}
const fn default_gs_s5() -> f32 {
    96.0
}
const fn default_gs_s6() -> f32 {
    95.0
}
const fn default_gs_s7() -> f32 {
    88.0
}
const fn default_gs_s8() -> f32 {
    87.0
}
const fn default_gs_s9() -> f32 {
    86.0
}
const fn default_gs_s10() -> f32 {
    85.0
}
const fn default_gs_s11() -> f32 {
    83.5
}
const fn default_gs_s12() -> f32 {
    82.0
}

impl Default for GunshotThresholdsConfig {
    fn default() -> Self {
        Self {
            s1: default_gs_s1(),
            s2: default_gs_s2(),
            s3: default_gs_s3(),
            s4: default_gs_s4(),
            s5: default_gs_s5(),
            s6: default_gs_s6(),
            s7: default_gs_s7(),
            s8: default_gs_s8(),
            s9: default_gs_s9(),
            s10: default_gs_s10(),
            s11: default_gs_s11(),
            s12: default_gs_s12(),
        }
    }
}

impl GunshotThresholdsConfig {
    /// Get threshold for a specific cycle (0-indexed 0..11)
    /// Returns 100.0 (max difficulty) if out of bounds (safety)
    pub fn get_threshold(&self, cycle_idx: usize) -> f32 {
        match cycle_idx {
            0 => self.s1,
            1 => self.s2,
            2 => self.s3,
            3 => self.s4,
            4 => self.s5,
            5 => self.s6,
            6 => self.s7,
            7 => self.s8,
            8 => self.s9,
            9 => self.s10,
            10 => self.s11,
            11 => self.s12,
            _ => 100.0,
        }
    }

    /// Get all thresholds as an array for iteration
    pub fn as_array(&self) -> [f32; 12] {
        [
            self.s1, self.s2, self.s3, self.s4, self.s5, self.s6, self.s7, self.s8, self.s9,
            self.s10, self.s11, self.s12,
        ]
    }

    /// Validate that all thresholds are within valid range [0.0, 100.0]
    pub fn validate(&self) -> anyhow::Result<()> {
        let thresholds = [
            ("s1", self.s1),
            ("s2", self.s2),
            ("s3", self.s3),
            ("s4", self.s4),
            ("s5", self.s5),
            ("s6", self.s6),
            ("s7", self.s7),
            ("s8", self.s8),
            ("s9", self.s9),
            ("s10", self.s10),
            ("s11", self.s11),
            ("s12", self.s12),
        ];
        for (name, value) in &thresholds {
            if !(0.0..=100.0).contains(value) {
                anyhow::bail!(
                    "gunshot_thresholds.{} must be in [0.0, 100.0], got {}",
                    name,
                    value
                );
            }
        }
        Ok(())
    }
}

impl Default for ScoringWeightsConfig {
    fn default() -> Self {
        Self {
            // Signal weights (neutral baseline)
            ligma: 1.0,
            qedd: 1.0,
            survivor: 1.0,
            qass_secondary: 1.0,
            mci: 1.0,
            cluster: 1.0,
            chaos: 1.0,

            // Penalty multipliers (baseline = 1.0)
            wash_penalty_mult: 1.0,
            bot_penalty_mult: 1.0,
            rug_penalty_mult: 1.0,
            cluster_penalty_mult: 1.0,
            ssmi_bot_penalty_mult: 1.0,
            scr_penalty_mult: 1.0,
            ulvf_div_penalty_mult: 1.0,
            ulvf_curl_penalty_mult: 1.0,
            povc_penalty_mult: 1.0,
            povc_organic_boost_mult: 1.0,
            mpcf_sniper_penalty_mult: 1.0,
            mpcf_sybil_penalty_mult: 1.0,

            // Boost multipliers (baseline = 1.0)
            organic_boost_mult: 1.0,
            smart_money_boost_mult: 1.0,
            ssmi_viral_boost_mult: 1.0,
            ssmi_human_boost_mult: 1.0,
            mesa_organic_boost_mult: 1.0,
            mesa_entropy_boost_mult: 1.0,
            chaos_pump_boost_mult: 1.0,
            resonance_human_boost_mult: 1.0,
            cluster_clean_boost_mult: 1.0,
        }
    }
}

impl Default for GhostBrainConfig {
    fn default() -> Self {
        Self {
            version: 1,
            engine: EngineConfig::default(),
            gatekeeper: GatekeeperConfig::default(),
            gatekeeper_v2: None,
            gatekeeper_v3: GatekeeperV3Config::default(),
            fsc_v2: FscV2Config::default(),
            mpcf: MpcfConfig::default(),
            ssmi: SsmiConfig::default(),
            iwim: IwimConfig::default(),
            qass: QassConfig::default(),
            sobp: SobpConfig::default(),
            qofsv: QofsvConfig::default(),
            frb: FrbConfig::default(),
            resonance: ResonanceConfig::default(),
            mci: MciConfig::default(),
            qedd: QeddConfig::default(),
            confidence: ConfidenceConfig::default(),
            normalization: NormalizationConfig::default(),
            ligma: LigmaConfig::default(),
            fre: FreConfig::default(),
            tcf: TcfConfig::default(),
            bva: BvaConfig::default(),
            panic: PanicConfig::default(),
            tcr_phi: TcrPhiConfig::default(),
            behavioral_scoring: BehavioralScoringConfig::default(),
            hyper_prediction: None,
            scoring: None,
            survivor_score: None,
            cycle_weights: None,
            gunshot_thresholds: None,
            paradox: None,
            post_buy_guardian: PostBuyGuardianConfig::default(),
            iwim_veto_gate: IwimVetoGateConfig::default(),
        }
    }
}

impl Default for MpcfConfig {
    fn default() -> Self {
        Self {
            bot_entropy_threshold: 3.5,
            human_entropy_threshold: 5.5,
            bot_iss_variance_threshold: 0.15,
            human_iss_variance_threshold: 0.35,
            sybil_entropy_threshold: 3.5,
            min_payload_size: 32,
            max_payload_size: 4096,
            unknown_confidence: 0.3,
            low_confidence_small_payload: 0.4,
            high_organic_threshold: default_mpcf_high_organic_threshold(),
            bot_dominated_threshold: default_mpcf_bot_dominated_threshold(),
            high_organic_base: default_mpcf_high_organic_base(),
            max_organic_boost: default_mpcf_max_organic_boost(),
            min_bot_penalty: default_mpcf_min_bot_penalty(),
        }
    }
}

impl Default for SsmiConfig {
    fn default() -> Self {
        Self {
            bot_scr_threshold: 0.7,
            bot_ar_threshold: 0.8,
            bot_entropy_threshold: 1.5,
            human_entropy_threshold: 3.0,
            human_ar_threshold: 0.3,
            human_scr_threshold: 0.4,
            viral_min_tx_count: 6,
            viral_entropy_min: 2.5,
            viral_entropy_max: 4.0,
            viral_scr_threshold: 0.5,
            score_weight_entropy: 0.35,
            score_weight_scr: 0.40,
            score_weight_ar: 0.25,
            viral_score_bonus: 0.15,
            human_score_bonus: 0.05,
            bot_score_penalty: 0.20,
            histogram_bins: 64,
            max_jitter_ms: 2000,
        }
    }
}

#[allow(deprecated)]
impl Default for GatekeeperConfig {
    fn default() -> Self {
        Self {
            min_tx_count: 15,
            min_unique_signers: 10,
            max_wait_time_ms: 2000,
            early_pass_tx_count: 50,
            min_volume_sol: 0.0,
            min_tx_count_for_scoring: None,
            max_cycles: None,
            max_observation_cycles: None,
        }
    }
}

impl Default for IwimConfig {
    fn default() -> Self {
        Self {
            iapp_rug_threshold: 2,
            min_iapp_rug_score: 0.95,
            at_window_ms: 1500,
            quiet_window_ms: 5000,
            max_tx_analyze: 50,
            confidence_threshold: 0.6,
            target_analysis_time_us: 120,
        }
    }
}

impl Default for QassConfig {
    fn default() -> Self {
        Self {
            collapse_threshold: 0.5,
            score_threshold_viral: 0.85,
            score_threshold_moderate: 0.70,
            score_threshold_neutral: 0.50,
            score_threshold_suspicious: 0.30,
            default_signal_weight: 1.0,
        }
    }
}

impl Default for SobpConfig {
    fn default() -> Self {
        Self {
            human_weight_multiplier: 2.0,
            sniper_weight_multiplier: 0.5,
            default_weight_multiplier: 1.0,
            base_transaction_weight: 1.0,
            hyper_pump_threshold: 3.0,
            growth_threshold: 1.5,
            stagnation_threshold: 0.8,
            implosion_threshold: 0.4,
            confidence_weight_history: 0.4,
            confidence_weight_tx_count: 0.3,
            confidence_weight_intensity: 0.3,
            slot_capacity: 64,
            min_slot_history: 2,
        }
    }
}

impl Default for QofsvConfig {
    fn default() -> Self {
        Self {
            state_vector_dim: 6,
            epsilon: 1e-6,
            target_construction_time_us: 200,
            target_normalization_time_us: 50,
        }
    }
}

impl Default for FrbConfig {
    fn default() -> Self {
        Self {
            min_amplitude_threshold: 0.001,
            enable_filtering: true,
        }
    }
}

impl Default for ResonanceConfig {
    fn default() -> Self {
        Self {
            bot_threshold_cv: 0.3,
            human_threshold_cv: 0.8,
        }
    }
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            liquidity_scale: 25000.0,
            volume_scale: 10000.0,
            volatility_factor: 5.0,
        }
    }
}

impl Default for ProfileWeights {
    fn default() -> Self {
        // Default to standard profile weights
        Self {
            weight_qass: 20.0,
            weight_sobp: 12.0,
            weight_mpcf: 10.0,
            weight_iwim: 8.0,
            weight_ssmi: 9.0,
            weight_qofsv: 11.0,
            weight_scr: 13.0,
            weight_frb: 7.0,
            weight_qman: 8.0,
            weight_gene_mapper: 10.0,
            weight_chaos_engine: 11.0,
        }
    }
}

impl Default for WeightProfiles {
    fn default() -> Self {
        Self {
            // Standard profile: High QASS (virality), moderate SOBP, low QEDD
            standard: ProfileWeights {
                weight_qass: 20.0,
                weight_sobp: 12.0,
                weight_mpcf: 10.0,
                weight_iwim: 8.0,
                weight_ssmi: 9.0,
                weight_qofsv: 11.0,
                weight_scr: 13.0,
                weight_frb: 7.0,
                weight_qman: 8.0, // QMAN acts as QEDD proxy
                weight_gene_mapper: 10.0,
                weight_chaos_engine: 11.0,
            },
            // Reversal profile: Low QASS, high SOBP (stability), high QEDD
            reversal: ProfileWeights {
                weight_qass: 10.0,
                weight_sobp: 18.0,
                weight_mpcf: 10.0,
                weight_iwim: 8.0,
                weight_ssmi: 9.0,
                weight_qofsv: 11.0,
                weight_scr: 13.0,
                weight_frb: 7.0,
                weight_qman: 16.0, // QMAN acts as QEDD proxy - higher for stability
                weight_gene_mapper: 10.0,
                weight_chaos_engine: 11.0,
            },
        }
    }
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            // Module weights (from confidence_model.rs ConfidenceWeights::default())
            weight_sobp: 12.0,
            weight_mpcf: 10.0,
            weight_iwim: 8.0,
            weight_ssmi: 9.0,
            weight_qass: 15.0,
            weight_qofsv: 11.0,
            weight_scr: 13.0,
            weight_frb: 7.0,
            weight_qman: 14.0,
            weight_gene_mapper: 10.0,
            weight_chaos_engine: 11.0,
            // Weight profiles
            profiles: WeightProfiles::default(),
            // Decision thresholds
            threshold_high: 0.8,
            threshold_medium: 0.5,
            threshold_low: 0.3,
        }
    }
}

impl ConfidenceConfig {
    /// Convert the high confidence threshold (0.0-1.0) into a 0-100 score
    pub fn high_threshold_points(&self) -> u8 {
        const SCALE: f32 = 100.0;
        const MIN_POINTS: f32 = 0.0;
        const MAX_POINTS: f32 = 100.0;

        (self.threshold_high * SCALE)
            .round()
            .clamp(MIN_POINTS, MAX_POINTS) as u8
    }

    /// Select weight profile based on pool age
    ///
    /// # Arguments
    /// * `time_since_creation_seconds` - Time since pool creation in seconds
    ///
    /// # Returns
    /// Reference to the appropriate ProfileWeights
    ///
    /// # Profile Selection Logic
    /// - < 2 minutes (120s): Standard profile (high QASS/virality focus)
    /// - 2-10 minutes: Standard profile (transition zone, use standard by default)
    /// - > 10 minutes (600s): Reversal profile (high SOBP/stability + QEDD)
    pub fn select_profile(&self, time_since_creation_seconds: u64) -> &ProfileWeights {
        if time_since_creation_seconds < 120 {
            &self.profiles.standard // < 2 minutes
        } else if time_since_creation_seconds > 600 {
            &self.profiles.reversal // > 10 minutes
        } else {
            // Transition zone: use standard by default
            &self.profiles.standard
        }
    }

    /// Validate configuration parameters
    ///
    /// Ensures all weights are non-negative and thresholds are in proper order.
    ///
    /// # Returns
    /// * `Result<()>` - Success if valid, error otherwise
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate all weights are non-negative
        let weights = [
            ("weight_sobp", self.weight_sobp),
            ("weight_mpcf", self.weight_mpcf),
            ("weight_iwim", self.weight_iwim),
            ("weight_ssmi", self.weight_ssmi),
            ("weight_qass", self.weight_qass),
            ("weight_qofsv", self.weight_qofsv),
            ("weight_scr", self.weight_scr),
            ("weight_frb", self.weight_frb),
            ("weight_qman", self.weight_qman),
            ("weight_gene_mapper", self.weight_gene_mapper),
            ("weight_chaos_engine", self.weight_chaos_engine),
        ];

        for (name, weight) in &weights {
            if *weight < 0.0 {
                anyhow::bail!("Confidence {} must be non-negative", name);
            }
        }

        for (name, value) in [
            ("threshold_high", self.threshold_high),
            ("threshold_medium", self.threshold_medium),
            ("threshold_low", self.threshold_low),
        ] {
            if !value.is_finite() {
                anyhow::bail!("Confidence {} must be finite", name);
            }
            if value < 0.0 || value > 1.0 {
                anyhow::bail!("Confidence {} must be in range [0.0, 1.0]", name);
            }
        }

        // Validate thresholds are in proper order
        if self.threshold_high <= self.threshold_medium {
            anyhow::bail!("Confidence threshold_high must be > threshold_medium");
        }
        if self.threshold_medium <= self.threshold_low {
            anyhow::bail!("Confidence threshold_medium must be > threshold_low");
        }

        Ok(())
    }
}

impl GhostBrainConfig {
    /// Load configuration from JSON file
    ///
    /// # Arguments
    /// * `path` - Path to the JSON configuration file
    ///
    /// # Returns
    /// * `Result<Self>` - Loaded configuration or error
    pub fn from_json_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from TOML file
    ///
    /// # Arguments
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Returns
    /// * `Result<Self>` - Loaded configuration or error
    pub fn from_toml_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Load ONLY `[gatekeeper_v2]` from TOML without validating the full GhostBrain config.
    ///
    /// This is used by launcher runtime to ensure Gatekeeper V2 thresholds remain
    /// manually tuneable even when unrelated config sections are temporarily invalid.
    pub fn gatekeeper_v2_from_toml_file<P: AsRef<Path>>(
        path: P,
    ) -> anyhow::Result<Option<GatekeeperV2Config>> {
        let contents = fs::read_to_string(path)?;
        let doc: toml::Value = toml::from_str(&contents)?;

        let Some(gatekeeper_v2) = doc.get("gatekeeper_v2") else {
            return Ok(None);
        };

        let config: GatekeeperV2Config = gatekeeper_v2.clone().try_into()?;
        config.validate()?;
        Ok(Some(config))
    }

    /// Save configuration to JSON file
    ///
    /// # Arguments
    /// * `path` - Path where to save the JSON configuration file
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    pub fn to_json_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Save configuration to TOML file
    ///
    /// # Arguments
    /// * `path` - Path where to save the TOML configuration file
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    pub fn to_toml_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let toml = toml::to_string_pretty(self)?;
        fs::write(path, toml)?;
        Ok(())
    }

    /// Validate configuration parameters
    ///
    /// Ensures all thresholds, weights, and ranges are within valid bounds.
    ///
    /// # Returns
    /// * `Result<()>` - Success if valid, error otherwise
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate MPCF
        if self.mpcf.bot_entropy_threshold < 0.0 || self.mpcf.bot_entropy_threshold > 10.0 {
            anyhow::bail!("MPCF bot_entropy_threshold must be in range [0.0, 10.0]");
        }
        if self.mpcf.human_entropy_threshold < 0.0 || self.mpcf.human_entropy_threshold > 10.0 {
            anyhow::bail!("MPCF human_entropy_threshold must be in range [0.0, 10.0]");
        }
        if self.mpcf.bot_iss_variance_threshold < 0.0 || self.mpcf.bot_iss_variance_threshold > 1.0
        {
            anyhow::bail!("MPCF bot_iss_variance_threshold must be in range [0.0, 1.0]");
        }
        if self.mpcf.unknown_confidence < 0.0 || self.mpcf.unknown_confidence > 1.0 {
            anyhow::bail!("MPCF unknown_confidence must be in range [0.0, 1.0]");
        }
        if self.mpcf.high_organic_threshold < 0.0 || self.mpcf.high_organic_threshold > 1.0 {
            anyhow::bail!("MPCF high_organic_threshold must be in range [0.0, 1.0]");
        }
        if self.mpcf.bot_dominated_threshold < 0.0 || self.mpcf.bot_dominated_threshold > 1.0 {
            anyhow::bail!("MPCF bot_dominated_threshold must be in range [0.0, 1.0]");
        }
        if self.mpcf.min_bot_penalty < 0.0
            || self.mpcf.min_bot_penalty > self.mpcf.high_organic_base
        {
            anyhow::bail!("MPCF min_bot_penalty must be non-negative and <= high_organic_base");
        }
        if self.mpcf.max_organic_boost < self.mpcf.high_organic_base {
            anyhow::bail!("MPCF max_organic_boost must be >= high_organic_base");
        }

        // Validate SSMI
        if self.ssmi.bot_scr_threshold < 0.0 || self.ssmi.bot_scr_threshold > 1.0 {
            anyhow::bail!("SSMI bot_scr_threshold must be in range [0.0, 1.0]");
        }
        let weight_sum =
            self.ssmi.score_weight_entropy + self.ssmi.score_weight_scr + self.ssmi.score_weight_ar;
        if (weight_sum - 1.0).abs() > 0.001 {
            anyhow::bail!(
                "SSMI score weights must sum to approximately 1.0, got {}",
                weight_sum
            );
        }

        // Validate IWIM
        if self.iwim.min_iapp_rug_score < 0.0 || self.iwim.min_iapp_rug_score > 1.0 {
            anyhow::bail!("IWIM min_iapp_rug_score must be in range [0.0, 1.0]");
        }
        if self.iwim.confidence_threshold < 0.0 || self.iwim.confidence_threshold > 1.0 {
            anyhow::bail!("IWIM confidence_threshold must be in range [0.0, 1.0]");
        }

        if let Some(ref gatekeeper_v2) = self.gatekeeper_v2 {
            gatekeeper_v2.validate()?;
        }
        self.gatekeeper_v3.validate()?;
        self.fsc_v2.validate()?;

        // Validate QASS
        if self.qass.collapse_threshold < 0.0 || self.qass.collapse_threshold > 1.0 {
            anyhow::bail!("QASS collapse_threshold must be in range [0.0, 1.0]");
        }
        if self.qass.score_threshold_viral < 0.0 || self.qass.score_threshold_viral > 1.0 {
            anyhow::bail!("QASS score_threshold_viral must be in range [0.0, 1.0]");
        }

        // Validate SOBP
        if self.sobp.hyper_pump_threshold < 0.0 || self.sobp.hyper_pump_threshold > 10.0 {
            anyhow::bail!("SOBP hyper_pump_threshold must be in range [0.0, 10.0]");
        }
        let sobp_weight_sum = self.sobp.confidence_weight_history
            + self.sobp.confidence_weight_tx_count
            + self.sobp.confidence_weight_intensity;
        if (sobp_weight_sum - 1.0).abs() > 0.001 {
            anyhow::bail!(
                "SOBP confidence weights must sum to approximately 1.0, got {}",
                sobp_weight_sum
            );
        }
        if self.sobp.slot_capacity < self.sobp.min_slot_history {
            anyhow::bail!("SOBP slot_capacity must be >= min_slot_history");
        }

        // Validate MCI initial state
        if let Some(initial_state) = &self.mci.initial_state {
            if initial_state.base_sentiment < 0.0 || initial_state.base_sentiment > 1.0 {
                anyhow::bail!("MCI initial_state.base_sentiment must be in range [0.0, 1.0]");
            }
            if initial_state.volatility_index < 0.0 || initial_state.volatility_index > 1.0 {
                anyhow::bail!("MCI initial_state.volatility_index must be in range [0.0, 1.0]");
            }
        }

        // Validate QOFSV
        if self.qofsv.state_vector_dim == 0 {
            anyhow::bail!("QOFSV state_vector_dim must be > 0");
        }

        // Validate FRB
        if self.frb.min_amplitude_threshold < 0.0 || self.frb.min_amplitude_threshold > 1.0 {
            anyhow::bail!("FRB min_amplitude_threshold must be in range [0.0, 1.0]");
        }

        // Validate Resonance
        if self.resonance.bot_threshold_cv < 0.0 || self.resonance.bot_threshold_cv > 1.0 {
            anyhow::bail!("Resonance bot_threshold_cv must be in range [0.0, 1.0]");
        }
        if self.resonance.human_threshold_cv < 0.0 || self.resonance.human_threshold_cv > 2.0 {
            anyhow::bail!("Resonance human_threshold_cv must be in range [0.0, 2.0]");
        }

        // Validate Confidence
        self.confidence.validate()?;

        // Validate Normalization
        if self.normalization.liquidity_scale < 0.001
            || self.normalization.liquidity_scale > 1_000_000.0
        {
            anyhow::bail!("Normalization liquidity_scale must be in range [0.001, 1000000.0]");
        }
        if self.normalization.volume_scale < 0.001 || self.normalization.volume_scale > 1_000_000.0
        {
            anyhow::bail!("Normalization volume_scale must be in range [0.001, 1000000.0]");
        }
        if self.normalization.volatility_factor < 0.1
            || self.normalization.volatility_factor > 100.0
        {
            anyhow::bail!("Normalization volatility_factor must be in range [0.1, 100.0]");
        }

        // Validate LIGMA
        if self.ligma.retail_impact_limit_bps < 0.0 || self.ligma.retail_impact_limit_bps > 10_000.0
        {
            anyhow::bail!("LIGMA retail_impact_limit_bps must be in range [0.0, 10000.0]");
        }
        if self.ligma.soft_impact_bps < 0.0 || self.ligma.soft_impact_bps > 10_000.0 {
            anyhow::bail!("LIGMA soft_impact_bps must be in range [0.0, 10000.0]");
        }
        if self.ligma.hard_impact_bps < 0.0 || self.ligma.hard_impact_bps > 10_000.0 {
            anyhow::bail!("LIGMA hard_impact_bps must be in range [0.0, 10000.0]");
        }
        if self.ligma.micro_jump_bps < 0.0 || self.ligma.micro_jump_bps > 10_000.0 {
            anyhow::bail!("LIGMA micro_jump_bps must be in range [0.0, 10000.0]");
        }
        if self.ligma.weight_in_survivor_score < 0.0 || self.ligma.weight_in_survivor_score > 1.0 {
            anyhow::bail!("LIGMA weight_in_survivor_score must be in range [0.0, 1.0]");
        }
        if self.ligma.veto_trap_threshold < 0.0 || self.ligma.veto_trap_threshold > 1.0 {
            anyhow::bail!("LIGMA veto_trap_threshold must be in range [0.0, 1.0]");
        }
        if self.ligma.veto_psi_ligma_threshold < -1.0 || self.ligma.veto_psi_ligma_threshold > 1.0 {
            anyhow::bail!("LIGMA veto_psi_ligma_threshold must be in range [-1.0, 1.0]");
        }

        // Validate BVA
        self.bva.validate()?;

        // Validate TCF
        self.tcf.validate()?;

        // Validate TCR-Φ (warnings only, runtime sanitizes)
        self.tcr_phi.validate()?;

        // Validate Scoring Weights (if present)
        if let Some(ref scoring) = self.scoring {
            scoring.validate()?;
        }

        // Validate Survivor Score Config (if present)
        if let Some(ref survivor_score) = self.survivor_score {
            survivor_score.validate()?;
        }

        // Validate Cycle Weights (if present)
        if let Some(ref cycle_weights) = self.cycle_weights {
            cycle_weights.validate()?;
        }

        // Validate Gunshot Thresholds (if present)
        if let Some(ref gunshot_thresholds) = self.gunshot_thresholds {
            gunshot_thresholds.validate()?;
        }

        Ok(())
    }

    /// Get cycle weight for a specific cycle index (0-indexed 0..11)
    /// Uses config if present, falls back to hardcoded defaults
    pub fn get_cycle_weight(&self, cycle_idx: usize) -> f32 {
        if let Some(ref config) = self.cycle_weights {
            config.get_weight(cycle_idx)
        } else {
            // Fallback to hardcoded defaults
            match cycle_idx {
                0 => 1.3,
                1 => 1.7,
                2 => 2.2,
                3 => 2.8,
                4 => 3.6,
                5 => 4.6,
                6 => 6.0,
                7 => 7.8,
                8 => 10.0,
                9 => 13.0,
                10 => 17.0,
                11 => 22.0,
                _ => 1.0,
            }
        }
    }

    /// Get gunshot threshold for a specific cycle index (0-indexed 0..11)
    /// Uses config if present, falls back to hardcoded defaults
    pub fn get_gunshot_threshold(&self, cycle_idx: usize) -> f32 {
        if let Some(ref config) = self.gunshot_thresholds {
            config.get_threshold(cycle_idx)
        } else {
            // Fallback to hardcoded defaults
            match cycle_idx {
                0 => 100.0,
                1 => 99.0,
                2 => 98.0,
                3 => 97.0,
                4 => 96.0,
                5 => 95.0,
                6 => 88.0,
                7 => 87.0,
                8 => 86.0,
                9 => 85.0,
                10 => 83.5,
                11 => 82.0,
                _ => 100.0,
            }
        }
    }

    /// Get cycle weights sum for normalization
    /// Uses config if present, falls back to hardcoded sum (92.0)
    pub fn get_cycle_weights_sum(&self) -> f32 {
        if let Some(ref config) = self.cycle_weights {
            config.sum()
        } else {
            // Fallback to hardcoded default
            92.0
        }
    }

    /// Get survivor score config, returning default if not configured
    pub fn get_survivor_score_config(&self) -> SurvivorScoreComponentConfig {
        self.survivor_score
            .clone()
            .unwrap_or_else(SurvivorScoreComponentConfig::default)
    }
}

impl ScoringWeightsConfig {
    /// Validate scoring weight ranges
    ///
    /// Ensures all weights are non-negative and within reasonable bounds.
    pub fn validate(&self) -> anyhow::Result<()> {
        // Check all penalty multipliers are non-negative
        let penalty_mults = [
            ("wash_penalty_mult", self.wash_penalty_mult),
            ("bot_penalty_mult", self.bot_penalty_mult),
            ("rug_penalty_mult", self.rug_penalty_mult),
            ("cluster_penalty_mult", self.cluster_penalty_mult),
            ("ssmi_bot_penalty_mult", self.ssmi_bot_penalty_mult),
            ("scr_penalty_mult", self.scr_penalty_mult),
            ("ulvf_div_penalty_mult", self.ulvf_div_penalty_mult),
            ("ulvf_curl_penalty_mult", self.ulvf_curl_penalty_mult),
            ("povc_penalty_mult", self.povc_penalty_mult),
            ("mpcf_sniper_penalty_mult", self.mpcf_sniper_penalty_mult),
            ("mpcf_sybil_penalty_mult", self.mpcf_sybil_penalty_mult),
        ];

        for (name, value) in &penalty_mults {
            if *value < 0.0 {
                anyhow::bail!("Scoring {} must be non-negative, got {}", name, value);
            }
            if !value.is_finite() {
                anyhow::bail!("Scoring {} must be finite, got {}", name, value);
            }
        }

        // Check all boost multipliers are non-negative
        let boost_mults = [
            ("organic_boost_mult", self.organic_boost_mult),
            ("smart_money_boost_mult", self.smart_money_boost_mult),
            ("ssmi_viral_boost_mult", self.ssmi_viral_boost_mult),
            ("ssmi_human_boost_mult", self.ssmi_human_boost_mult),
            ("mesa_organic_boost_mult", self.mesa_organic_boost_mult),
            ("mesa_entropy_boost_mult", self.mesa_entropy_boost_mult),
            ("chaos_pump_boost_mult", self.chaos_pump_boost_mult),
            (
                "resonance_human_boost_mult",
                self.resonance_human_boost_mult,
            ),
            ("cluster_clean_boost_mult", self.cluster_clean_boost_mult),
            ("povc_organic_boost_mult", self.povc_organic_boost_mult),
        ];

        for (name, value) in &boost_mults {
            if *value < 0.0 {
                anyhow::bail!("Scoring {} must be non-negative, got {}", name, value);
            }
            if !value.is_finite() {
                anyhow::bail!("Scoring {} must be finite, got {}", name, value);
            }
        }

        // Check signal weights are non-negative
        let signal_weights = [
            ("ligma", self.ligma),
            ("qedd", self.qedd),
            ("survivor", self.survivor),
            ("qass_secondary", self.qass_secondary),
            ("mci", self.mci),
            ("cluster", self.cluster),
            ("chaos", self.chaos),
        ];

        for (name, value) in &signal_weights {
            if *value < 0.0 {
                anyhow::bail!(
                    "Scoring signal weight {} must be non-negative, got {}",
                    name,
                    value
                );
            }
            if !value.is_finite() {
                anyhow::bail!(
                    "Scoring signal weight {} must be finite, got {}",
                    name,
                    value
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_default_config() {
        let config = GhostBrainConfig::default();
        assert_eq!(config.version, 1);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_gatekeeper_v2_default_enables_three_layer_decision() {
        assert!(
            GatekeeperV2Config::default().use_three_layer_decision,
            "Phase 2 requires the feature-driven three-layer Gatekeeper path to be the default contract"
        );
    }

    #[test]
    fn test_mpcf_defaults() {
        let config = MpcfConfig::default();
        assert_eq!(config.bot_entropy_threshold, 3.5);
        assert_eq!(config.human_entropy_threshold, 5.5);
        assert_eq!(config.min_payload_size, 32);
        assert_eq!(config.max_payload_size, 4096);
        assert_eq!(config.high_organic_threshold, 0.7);
        assert_eq!(config.bot_dominated_threshold, 0.5);
        assert_eq!(config.high_organic_base, 1.5);
        assert_eq!(config.max_organic_boost, 2.5);
        assert_eq!(config.min_bot_penalty, 0.2);
    }

    #[test]
    fn test_ssmi_defaults() {
        let config = SsmiConfig::default();
        assert_eq!(config.bot_scr_threshold, 0.7);
        assert_eq!(config.histogram_bins, 64);
        // Check that weights sum to 1.0
        let weight_sum =
            config.score_weight_entropy + config.score_weight_scr + config.score_weight_ar;
        assert!((weight_sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_sobp_defaults() {
        let config = SobpConfig::default();
        assert_eq!(config.hyper_pump_threshold, 3.0);
        assert_eq!(config.growth_threshold, 1.5);
        assert_eq!(config.slot_capacity, 64);
        // Check that confidence weights sum to 1.0
        let weight_sum = config.confidence_weight_history
            + config.confidence_weight_tx_count
            + config.confidence_weight_intensity;
        assert!((weight_sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_validation_mpcf_invalid_entropy() {
        let mut config = GhostBrainConfig::default();
        config.mpcf.bot_entropy_threshold = 15.0; // Invalid: > 10.0
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_ssmi_invalid_weights() {
        let mut config = GhostBrainConfig::default();
        config.ssmi.score_weight_entropy = 0.5;
        config.ssmi.score_weight_scr = 0.5;
        config.ssmi.score_weight_ar = 0.5; // Sum = 1.5, should be 1.0
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_sobp_slot_capacity() {
        let mut config = GhostBrainConfig::default();
        config.sobp.slot_capacity = 1;
        config.sobp.min_slot_history = 2; // slot_capacity < min_slot_history
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_serialization_json() {
        let config = GhostBrainConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GhostBrainConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.version, deserialized.version);
        assert_eq!(
            config.mpcf.bot_entropy_threshold,
            deserialized.mpcf.bot_entropy_threshold
        );
    }

    #[test]
    fn test_serialization_toml() {
        let config = GhostBrainConfig::default();
        let toml = toml::to_string(&config).unwrap();
        let deserialized: GhostBrainConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config.version, deserialized.version);
        assert_eq!(
            config.sobp.hyper_pump_threshold,
            deserialized.sobp.hyper_pump_threshold
        );
    }

    #[test]
    fn test_weight_profile_defaults() {
        let config = ConfidenceConfig::default();

        // Standard profile should favor QASS (virality)
        assert_eq!(
            config.profiles.standard.weight_qass, 20.0,
            "Standard QASS should be high"
        );
        assert_eq!(
            config.profiles.standard.weight_sobp, 12.0,
            "Standard SOBP should be moderate"
        );
        assert_eq!(
            config.profiles.standard.weight_qman, 8.0,
            "Standard QMAN (QEDD proxy) should be low"
        );

        // Reversal profile should favor SOBP and QEDD
        assert_eq!(
            config.profiles.reversal.weight_qass, 10.0,
            "Reversal QASS should be low"
        );
        assert_eq!(
            config.profiles.reversal.weight_sobp, 18.0,
            "Reversal SOBP should be high"
        );
        assert_eq!(
            config.profiles.reversal.weight_qman, 16.0,
            "Reversal QMAN (QEDD proxy) should be high"
        );
    }

    #[test]
    fn test_select_profile_young_pool() {
        let config = ConfidenceConfig::default();

        // Young pool (< 2 minutes)
        let profile = config.select_profile(60); // 1 minute
        assert_eq!(
            profile.weight_qass, 20.0,
            "Young pool should use standard profile with high QASS"
        );

        let profile = config.select_profile(119); // 1:59
        assert_eq!(
            profile.weight_qass, 20.0,
            "Pool just under 2 minutes should use standard profile"
        );
    }

    #[test]
    fn test_select_profile_transition_zone() {
        let config = ConfidenceConfig::default();

        // Transition zone (2-10 minutes) - should use standard by default
        let profile = config.select_profile(120); // 2 minutes
        assert_eq!(
            profile.weight_qass, 20.0,
            "Transition zone should use standard profile"
        );

        let profile = config.select_profile(300); // 5 minutes
        assert_eq!(
            profile.weight_qass, 20.0,
            "Transition zone should use standard profile"
        );

        let profile = config.select_profile(599); // 9:59
        assert_eq!(
            profile.weight_qass, 20.0,
            "Pool just under 10 minutes should use standard profile"
        );
    }

    #[test]
    fn test_select_profile_mature_pool() {
        let config = ConfidenceConfig::default();

        // Mature pool (> 10 minutes)
        let profile = config.select_profile(601); // 10:01
        assert_eq!(
            profile.weight_qass, 10.0,
            "Mature pool should use reversal profile with low QASS"
        );
        assert_eq!(
            profile.weight_sobp, 18.0,
            "Mature pool should use reversal profile with high SOBP"
        );
        assert_eq!(
            profile.weight_qman, 16.0,
            "Mature pool should use reversal profile with high QMAN"
        );

        let profile = config.select_profile(3600); // 1 hour
        assert_eq!(
            profile.weight_qass, 10.0,
            "Old pool should use reversal profile"
        );
    }

    #[test]
    fn test_weight_profile_enum() {
        assert_eq!(WeightProfile::Standard, WeightProfile::Standard);
        assert_ne!(WeightProfile::Standard, WeightProfile::Reversal);
    }

    #[test]
    fn test_normalization_defaults() {
        let config = NormalizationConfig::default();
        assert_eq!(config.liquidity_scale, 25000.0);
        assert_eq!(config.volume_scale, 10000.0);
        assert_eq!(config.volatility_factor, 5.0);
    }

    #[test]
    fn test_normalization_validation() {
        let mut config = GhostBrainConfig::default();

        // Test valid normalization config
        assert!(config.validate().is_ok());

        // Test valid small SOL-based liquidity_scale (pump.fun micro-caps)
        config.normalization.liquidity_scale = 3.0;
        assert!(config.validate().is_ok());
        config.normalization.liquidity_scale = 0.001;
        assert!(config.validate().is_ok());

        // Test invalid liquidity_scale (too low - below 0.001)
        config.normalization.liquidity_scale = 0.0;
        assert!(config.validate().is_err());
        config.normalization.liquidity_scale = -1.0;
        assert!(config.validate().is_err());

        // Reset and test invalid liquidity_scale (too high)
        config.normalization.liquidity_scale = 2_000_000.0;
        assert!(config.validate().is_err());

        // Reset and test valid small volume_scale (SOL-based)
        config.normalization.liquidity_scale = 25000.0;
        config.normalization.volume_scale = 3.0;
        assert!(config.validate().is_ok());
        config.normalization.volume_scale = 0.001;
        assert!(config.validate().is_ok());

        // Test invalid volume_scale (zero or negative)
        config.normalization.volume_scale = 0.0;
        assert!(config.validate().is_err());
        config.normalization.volume_scale = -5.0;
        assert!(config.validate().is_err());

        // Reset and test invalid volatility_factor
        config.normalization.volume_scale = 10000.0;
        config.normalization.volatility_factor = 0.05;
        assert!(config.validate().is_err());

        // Reset to valid values
        config.normalization.volatility_factor = 5.0;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ligma_config_defaults() {
        let config = LigmaConfig::default();
        assert_eq!(config.enabled, true);
        assert_eq!(config.retail_impact_limit_bps, 700.0);
        assert_eq!(config.soft_impact_bps, 2000.0);
        assert_eq!(config.hard_impact_bps, 6000.0);
        assert_eq!(config.micro_jump_bps, 2500.0);
        assert_eq!(config.weight_in_survivor_score, 0.15);
        assert_eq!(config.veto_trap_threshold, 0.80);
        assert_eq!(config.veto_psi_ligma_threshold, -0.5);
    }

    #[test]
    fn test_ligma_config_validation() {
        let mut config = GhostBrainConfig::default();

        // Test valid LIGMA config
        assert!(config.validate().is_ok());

        // Test invalid retail_impact_limit_bps (too high)
        config.ligma.retail_impact_limit_bps = 15_000.0;
        assert!(config.validate().is_err());

        // Reset and test invalid soft_impact_bps (negative)
        config.ligma.retail_impact_limit_bps = 700.0;
        config.ligma.soft_impact_bps = -100.0;
        assert!(config.validate().is_err());

        // Reset and test invalid weight_in_survivor_score (too high)
        config.ligma.soft_impact_bps = 2000.0;
        config.ligma.weight_in_survivor_score = 1.5;
        assert!(config.validate().is_err());

        // Reset and test invalid veto_trap_threshold (negative)
        config.ligma.weight_in_survivor_score = 0.15;
        config.ligma.veto_trap_threshold = -0.1;
        assert!(config.validate().is_err());

        // Reset and test invalid veto_psi_ligma_threshold (too low)
        config.ligma.veto_trap_threshold = 0.80;
        config.ligma.veto_psi_ligma_threshold = -1.5;
        assert!(config.validate().is_err());

        // Reset to valid values
        config.ligma.veto_psi_ligma_threshold = -0.5;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ligma_serialization_toml() {
        let config = GhostBrainConfig::default();
        let toml = toml::to_string(&config).unwrap();

        // Check that LIGMA section is present in TOML
        assert!(
            toml.contains("[ligma]"),
            "TOML should contain [ligma] section"
        );
        assert!(
            toml.contains("retail_impact_limit_bps"),
            "TOML should contain retail_impact_limit_bps"
        );
        assert!(
            toml.contains("weight_in_survivor_score"),
            "TOML should contain weight_in_survivor_score"
        );

        // Deserialize and verify
        let deserialized: GhostBrainConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config.ligma.enabled, deserialized.ligma.enabled);
        assert_eq!(
            config.ligma.retail_impact_limit_bps,
            deserialized.ligma.retail_impact_limit_bps
        );
    }

    #[test]
    fn test_gatekeeper_v2_from_toml_file_partial_override() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_gk2_partial_{ts}.toml"));

        let toml = r#"
version = 10

[gatekeeper_v2]
min_tx_count = 77
max_wait_time_ms = 3333
max_static_fee_profile_ratio = 0.82
mix_interval_cv = 0.91
min_dev_paperhand_latency_ms = 2500
"#;

        std::fs::write(&path, toml).unwrap();
        let parsed = GhostBrainConfig::gatekeeper_v2_from_toml_file(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(parsed.is_some());
        let cfg = parsed.unwrap();
        assert_eq!(cfg.min_tx_count, 77);
        assert_eq!(cfg.max_wait_time_ms, 3333);
        assert_eq!(cfg.max_static_fee_profile_ratio, 0.82);
        assert_eq!(cfg.max_interval_cv, 0.91);
        assert_eq!(cfg.min_dev_paperhand_latency_ms, 2500);
        // Missing fields should come from GatekeeperV2Config::default()
        assert_eq!(
            cfg.use_three_layer_decision,
            GatekeeperV2Config::default().use_three_layer_decision
        );
        assert_eq!(
            cfg.min_unique_signers,
            GatekeeperV2Config::default().min_unique_signers
        );
        assert_eq!(
            cfg.max_whale_reversal_ratio_top3,
            GatekeeperV2Config::default().max_whale_reversal_ratio_top3
        );
    }

    #[test]
    fn test_fsc_v2_defaults_are_capture_inert() {
        let config = GhostBrainConfig::default();
        assert!(!config.fsc_v2.capture_enabled);
        assert!(!config.fsc_v2.feature_emit_enabled);
        assert!(!config.fsc_v2.decision_enabled);
        assert!(!config.fsc_v2.hard_reject_enabled);
        assert_eq!(config.fsc_v2.provider, "nln_program_streams");
        assert_eq!(config.fsc_v2.lookback_window_s, 300);
        assert_eq!(config.fsc_v2.min_abs_store_lamports, 1_000_000);
        assert_eq!(config.fsc_v2.min_abs_attribution_lamports, 10_000_000);
        assert_eq!(
            config.fsc_v2.same_slot_cross_signature_policy,
            "require_tx_index"
        );
        assert!(!config.fsc_v2.include_wsol);
        assert!(!config.fsc_v2.include_spl);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_fsc_v2_capture_config_deserializes_without_decision_enablement() {
        let toml = r#"
capture_enabled = true
feature_emit_enabled = true
decision_enabled = false
hard_reject_enabled = false
provider = "nln_program_streams"
lookback_window_s = 300
warmup_window_s = 300
min_abs_store_lamports = 1000000
min_abs_attribution_lamports = 10000000
min_rel_to_buy = 0.20
min_attribution_confidence = 0.60
min_total_buyers = 2
min_known_non_neutral_buyers = 2
min_known_coverage = 0.50
min_non_neutral_known_coverage = 0.30
same_slot_cross_signature_policy = "require_tx_index"
include_wsol = false
include_spl = false
"#;

        let config: FscV2Config = toml::from_str(toml).unwrap();
        assert!(config.capture_enabled);
        assert!(config.feature_emit_enabled);
        assert!(!config.decision_enabled);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_fsc_v2_decision_enablement_is_rejected_in_pr_fsc1() {
        let mut config = GhostBrainConfig::default();
        config.fsc_v2.decision_enabled = true;
        let err = config
            .validate()
            .expect_err("PR-FSC1 must reject active FSC v2 decision enablement");
        assert!(err.to_string().contains("fsc_v2.decision_enabled"));
    }

    #[test]
    fn test_fsc_v2_capture_rollout_profile_loads_decision_off() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .join("../configs/rollout")
            .join("ghost_brain_v3_p36_primary_only_fsc_capture.toml");
        let config = GhostBrainConfig::from_toml_file(&path)
            .unwrap_or_else(|err| panic!("failed to load {}: {err}", path.display()));

        assert!(config.fsc_v2.capture_enabled);
        assert!(config.fsc_v2.feature_emit_enabled);
        assert!(!config.fsc_v2.decision_enabled);
        assert!(!config.fsc_v2.hard_reject_enabled);
        assert_eq!(config.fsc_v2.provider, "nln_program_streams");
        assert_eq!(
            config.fsc_v2.same_slot_cross_signature_policy,
            "require_tx_index"
        );
    }

    #[test]
    fn parse_gatekeeper_v25_config() {
        let contents = include_str!("../../ghost_brain_config.toml");
        let doc: toml::Value =
            toml::from_str(contents).expect("ghost_brain_config.toml should parse as valid TOML");
        let gatekeeper_v2 = doc
            .get("gatekeeper_v2")
            .expect("TOML should have [gatekeeper_v2] section");
        let config: GatekeeperV2Config = gatekeeper_v2
            .clone()
            .try_into()
            .expect("Should deserialize gatekeeper_v2 with v25 sub-sections");
        // V2.5 rollout
        assert!(config.v25.shadow_enabled);
        assert!(!config.v25.live_execution_enabled);
        assert!(config.v25.require_promotion_adr);
        assert!(config.v25.emit_shadow_decisions);
        // DOW
        assert!(config.dow.enabled);
        assert_eq!(config.dow.early_entry_min_ms, 2000);
        assert_eq!(config.dow.normal_window_ms, 7000);
        assert_eq!(config.dow.extended_window_ms, 10000);
        // P0 invariant
        assert!(config.dow.extended_window_ms <= config.max_wait_time_ms);
        assert!((config.dow.early_entry_min_confidence - 0.50).abs() < f64::EPSILON);
        assert!(!config.dow.extended_require_pdd_clean);
        // TAS
        assert!(config.tas.enabled);
        assert!((config.tas.tas_hard_reject_threshold - 0.10).abs() < f64::EPSILON);
        assert!((config.tas.momentum_trajectory_weight - 0.25).abs() < f64::EPSILON);
        // PDD
        assert!(config.pdd.enabled);
        assert!(!config.pdd.spike_promoted_to_live);
        assert!(!config.pdd.ramping_promoted_to_live);
        assert!((config.pdd.entry_drift_max_pct - 5.0).abs() < f64::EPSILON);
        assert!(config.pdd.spike_detection_enabled);
        assert!(config.pdd.ramping_detection_enabled);
        // APS
        assert!(config.aps.enabled);
        assert!(!config.aps.adaptive_enabled);
        assert_eq!(config.aps.adaptation_interval_buys, 50);
        // Legacy fields must still deserialize
        assert!(config.use_three_layer_decision);
        assert_eq!(config.mode, GatekeeperMode::Long);
        // Collector profile keeps the legacy cap permissive for wide evidence gathering.
        assert!(
            (config.max_price_change_ratio - 9.50).abs() < f64::EPSILON,
            "max_price_change_ratio must be 9.50 in the collector profile, got {}",
            config.max_price_change_ratio,
        );
    }

    #[test]
    fn gatekeeper_v25_dow_rejects_extended_window_beyond_deadline() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_gk2_bad_dow_{ts}.toml"));

        let toml = r#"
version = 10

[gatekeeper_v2]
max_wait_time_ms = 5000

[gatekeeper_v2.dow]
enabled = true
extended_window_ms = 10000
"#;

        std::fs::write(&path, toml).unwrap();
        let err = GhostBrainConfig::gatekeeper_v2_from_toml_file(&path)
            .expect_err("extended window beyond hard deadline must fail fast");
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("P0 invariant violated"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn gatekeeper_v25_dow_rejects_zero_tick_interval() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_gk2_zero_tick_{ts}.toml"));

        let toml = r#"
version = 10

[gatekeeper_v2]
max_wait_time_ms = 12000

[gatekeeper_v2.dow]
enabled = true
extended_window_ms = 10000
tick_interval_ms = 0
"#;

        std::fs::write(&path, toml).unwrap();
        let err = GhostBrainConfig::gatekeeper_v2_from_toml_file(&path)
            .expect_err("zero DOW tick interval must fail fast");
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("tick_interval_ms must be > 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn gatekeeper_v25_default_backward_compat() {
        // When TOML has NO v25 sub-sections, everything defaults to disabled
        let toml_without_v25 = r#"
mode = "standard"
min_tx_count = 30
"#;
        let config: GatekeeperV2Config =
            toml::from_str(toml_without_v25).expect("Should deserialize without v25 sections");
        assert_eq!(config.min_tx_count, 30);
        // V2.5 fields default to disabled
        assert!(!config.v25.shadow_enabled);
        assert!(!config.dow.enabled);
        assert!(!config.tas.enabled);
        assert!(!config.pdd.enabled);
        assert!(!config.aps.enabled);
    }

    #[test]
    fn gatekeeper_v2_from_toml_file_picks_up_v25() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_gk2_v25_{ts}.toml"));
        let toml = r#"
[gatekeeper_v2]
min_tx_count = 10
[gatekeeper_v2.pdd]
enabled = true
entry_drift_max_pct = 7.5
"#;
        std::fs::write(&path, toml).unwrap();
        let parsed = GhostBrainConfig::gatekeeper_v2_from_toml_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(parsed.is_some());
        let cfg = parsed.unwrap();
        assert_eq!(cfg.min_tx_count, 10);
        assert!(cfg.pdd.enabled);
        assert!((cfg.pdd.entry_drift_max_pct - 7.5).abs() < f64::EPSILON);
        // Other V2.5 modules not in TOML -> defaults (disabled)
        assert!(!cfg.dow.enabled);
        assert!(!cfg.tas.enabled);
        assert!(!cfg.aps.enabled);
        // v25 rollout default
        assert!(!cfg.v25.shadow_enabled);
    }
}
