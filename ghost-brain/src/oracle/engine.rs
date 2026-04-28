//! Cyclic Prediction Engine - S1-S12 Heartbeat Loop
//!
//! This module implements the continuous re-evaluation process that replaces
//! the single-shot pipeline.evaluate() with a 420ms heartbeat cycle system.
//!
//! ## Architecture
//!
//! The engine progresses through 12 cycles (S1-S12), evolving from:
//! - **Sniping Mode** (S1-S2): Early aggressive detection
//! - **Stabilization** (S3-S7): Building confidence
//! - **Final Verdict** (S8-S12): Weighted decision making
//!
//! ## Timing
//!
//! - Cycle Duration: configurable via `GHOST_ENGINE_CYCLE_MS` (default: 420ms, Solana slot duration)
//! - Total Duration: ~5.04s (12 cycles)
//! - Early Exit: "Gunshot" mechanism for high-confidence opportunities
//!
//! ## Integration
//!
//! The engine is spawned by OracleRuntime after Gatekeeper approval.
//! It runs independently, accessing ShadowLedger for real-time market state.
//!
//! Time-axis contract (SnapshotEngine -> Engine -> SOBP/CIR): slot is metadata-only, carried as
//! `Option<u64>`. Missing slot stays `None`; slot `0` is treated as contract violation.

use parking_lot::Mutex;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashSet;
use std::str::FromStr;
use std::{env, sync::Arc, time::SystemTime};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, info, warn};

use crate::analyzers::mesa::MesaAnalyzer;
use crate::chaos::amm_math::AmmPool;
use crate::chaos::engine::{ChaosEngine, MarketScenario, SimulationConfig};
use crate::config::ghost_brain_config::default_cycle_duration_ms;
use crate::config::ghost_brain_config::MpcfConfig;
use crate::config::qedd_config::QeddConfig;
use crate::config::PanicConfig;
use crate::config::{BvaConfig, GhostBrainConfig, TcrPhiConfig};
use crate::oracle::bva::{BvaAnalyzer, BvaOutput, BvaState};
use crate::oracle::hyper_oracle::HyperOracle;
use crate::oracle::scoring_phase::ScoringPhase;
use crate::oracle::snapshot_engine::{EventTsSource, SnapshotEngine, TransactionRecord};
use crate::oracle::survivor_score::{
    DeferReason, SurvivorScoreCalculator, SurvivorScoreInput, SurvivorSessionStage, VetoReason,
};
use crate::oracle::tcf::{MarketObservation, TrendCohesionField};
use crate::oracle::ultrafast::cir::{cir_tx_key, BuySell, CirConfig, CirCore, CirEvent};
use crate::oracle::ultrafast::mpcf::{self, ActorType};
use crate::oracle::ultrafast::sobp::{SobpCore, TransactionRecord as SobpTransactionRecord};
use crate::oracle::ultrafast::{
    EctoConfig, EctoState, IwimResult, MarketAnomalyOutput, MarketAnomalyState, MarketAnomalyTx,
    PanicOutput, PanicState, PanicTx, SignerEntropyState, TcrImpact, TcrPhiCore, TcrReaction,
    TcrScore,
};
use crate::oracle::{tx_metrics::IntervalSource, TransactionMetrics};
use crate::qedd::QeddEngine;
use ghost_core::market_state::BondingCurve;
use ghost_core::shadow_ledger::types::{PriceReason, PriceState};
use ghost_core::shadow_ledger::{
    BvaClassification as LedgerBvaClassification, MarketSnapshot, ShadowLedger,
};
use metrics::increment_counter;
use seer::types::RawBytesMissingReason;

// =============================================================================
// Module Configuration Constants
// =============================================================================

/// Default fee in basis points for Pump.fun AMM (0.1% = 10 bps)
const PUMP_FUN_FEE_BPS: u16 = 10;

/// Default LIGMA tradability score when LIGMA module unavailable
const DEFAULT_LIGMA_TRADABILITY: f32 = 0.8;

/// Default LIGMA psi value when LIGMA module unavailable
const DEFAULT_LIGMA_PSI: f32 = 0.0;

/// Default LIGMA liquidity trap risk when LIGMA module unavailable
const DEFAULT_LIGMA_TRAP_RISK: f32 = 0.1;

/// Conservative default for interval standard deviation when variance is unknown.
const DEFAULT_INTERVAL_STD_DEV: f64 = 50.0;
const MIN_RESERVE_THRESHOLD: f64 = 1e-6;
/// Epsilon used to compare cumulative volumes and avoid false regressions due to floating point jitter.
const VOLUME_COMPARISON_EPSILON: f64 = 1e-6;
/// Minimum consecutive live snapshots required before scoring starts.
const WARMUP_LIVE_MIN: u8 = 2;
/// Feature flag controlling IWIM integration in scoring cycles.
const IWIM_ENV_FLAG: &str = "GHOST_IWIM_ENABLED";
/// Minimum CIR global score to run chaos simulation.
const CIR_CHAOS_THRESHOLD: f64 = 0.8;
const CHAOS_SCENARIO_PRICE_UP: f64 = 0.02;
const CHAOS_SCENARIO_PRICE_DOWN: f64 = 0.02;
const CHAOS_SCENARIO_VOL_UP: f64 = 0.05;
const CHAOS_SCENARIO_VOL_DOWN: f64 = -0.05;
const CHAOS_SCENARIO_TX_ACTIVE: f64 = 0.6;
const CHAOS_SCENARIO_VOLATILE: f64 = 0.35;
const CHAOS_SCENARIO_RUG_PRICE_DROP: f64 = -0.08;
const CHAOS_SCENARIO_RUG_TX_MOMENTUM: f64 = 0.2;
const EARLY_BVA_CONFIDENCE_FLOOR: f64 = 0.5;
const EARLY_NO_BVA_SCORE_CAP: f64 = 30.0;
const DEFAULT_SOBP_MIN_EMITTED_TX_EARLY: usize = 3;
const ENV_SOBP_MIN_EMITTED_TX_EARLY: &str = "GHOST_SOBP_MIN_EMITTED_TX_EARLY";
const CYCLE_SNAPSHOT_LOOKBACK: usize = 128;
const DEFAULT_ANALYSIS_WINDOW_MS_CONFIG: u64 = 8_000;
const ENV_ANALYSIS_WINDOW_MS: &str = "GHOST_ANALYSIS_WINDOW_MS";
const LOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoEmitReasonCode {
    OutOfOrderInput,
    WindowSpanInsufficient,
    ResponderGatesNotMet,
    ThetaNotReached,
    SkippedDueToWindowContract,
    SobpDataUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyFalseReason {
    BaselineMissing,
    Discontinuity,
    InsufficientSpan,
    SnapshotUnavailable,
}

impl ReadyFalseReason {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            Self::BaselineMissing => "baseline_missing",
            Self::Discontinuity => "discontinuity",
            Self::InsufficientSpan => "insufficient_span",
            Self::SnapshotUnavailable => "snapshot_unavailable",
        }
    }
}

impl NoEmitReasonCode {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            Self::OutOfOrderInput => "OUT_OF_ORDER_INPUT",
            Self::WindowSpanInsufficient => "WINDOW_SPAN_INSUFFICIENT",
            Self::ResponderGatesNotMet => "RESPONDER_GATES_NOT_MET",
            Self::ThetaNotReached => "THETA_NOT_REACHED",
            Self::SkippedDueToWindowContract => "SKIPPED_DUE_TO_WINDOW_CONTRACT",
            Self::SobpDataUnavailable => "SOBP_DATA_UNAVAILABLE",
        }
    }
}

fn bva_scr_prior(classification: crate::oracle::bva::BvaClassification) -> f32 {
    match classification {
        crate::oracle::bva::BvaClassification::Organic => 0.85,
        crate::oracle::bva::BvaClassification::Chaotic => 0.55,
        crate::oracle::bva::BvaClassification::Dormant => 0.4,
        crate::oracle::bva::BvaClassification::Steered => 0.15,
    }
}

/// Configuration for PredictionSession runtime behavior.
#[derive(Debug, Clone)]
pub struct PredictionSessionConfig {
    pub iwim_enabled: bool,
    pub cycle_duration: Duration,
    pub bva_config: BvaConfig,
    pub panic_config: PanicConfig,
    pub tcr_phi_config: TcrPhiConfig,
    pub sobp_min_emitted_tx_early: usize,
    pub analysis_window_ms_config: u64,
    pub config_path: Option<String>,
    pub ghost_brain_config: Option<GhostBrainConfig>,
}

impl PredictionSessionConfig {
    pub fn from_env() -> Self {
        let iwim_enabled = env::var(IWIM_ENV_FLAG)
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "True" | "yes" | "YES"))
            .unwrap_or(false);
        let cycle_ms = env::var("GHOST_ENGINE_CYCLE_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or_else(default_cycle_duration_ms);
        let sobp_min_emitted_tx_early = env::var(ENV_SOBP_MIN_EMITTED_TX_EARLY)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_SOBP_MIN_EMITTED_TX_EARLY);
        let analysis_window_ms_config = env::var(ENV_ANALYSIS_WINDOW_MS)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_ANALYSIS_WINDOW_MS_CONFIG);
        let config_path = env::var("GHOST_BRAIN_CONFIG_PATH")
            .ok()
            .or_else(|| Some("ghost-brain/ghost_brain_config.toml".to_string()));
        let ghost_brain_config = config_path
            .as_deref()
            .and_then(|path| GhostBrainConfig::from_toml_file(path).ok());
        let (panic_config, tcr_phi_config) = ghost_brain_config
            .as_ref()
            .map(|config| (config.panic.clone(), config.tcr_phi.clone()))
            .unwrap_or_else(|| (PanicConfig::default(), TcrPhiConfig::default()));
        Self {
            iwim_enabled,
            cycle_duration: Duration::from_millis(cycle_ms),
            bva_config: BvaConfig::default(),
            panic_config,
            tcr_phi_config,
            sobp_min_emitted_tx_early,
            analysis_window_ms_config,
            config_path,
            ghost_brain_config,
        }
    }
}

#[inline]
fn chaos_inputs_valid(snapshot: &MarketSnapshot) -> bool {
    snapshot.price_state == PriceState::Valid
        && snapshot.reserve_base > MIN_RESERVE_THRESHOLD
        && snapshot.reserve_quote > MIN_RESERVE_THRESHOLD
}

/// Provider for cached IWIM results supplied by runtime.
pub trait IwimProvider: Send + Sync {
    fn fetch_cached_iwim(&self, pool_amm_id: Pubkey) -> Option<IwimResult>;
}

/// Provider for PANIC transaction metadata supplied by runtime.
pub trait PanicProvider: Send + Sync {
    fn fetch_panic_transactions(&self, pool_amm_id: Pubkey, since_ts_ms: u64) -> Vec<PanicTx>;
}

/// Centralized gate decision used to control scoring and execution flow.
#[derive(Debug, PartialEq, Eq)]
enum GateAction {
    Allowed,
    Deferred(DeferReason),
    Blocked(VetoReason),
}

// =============================================================================
// Organic Weighting Constants (SOBP & MPCF Actor-Based Scoring)
// =============================================================================

/// High organic ratio threshold (unique addresses / transactions)
/// Ratios >= 0.8 indicate Human-like activity (each tx from different wallet)
const ORGANIC_RATIO_HIGH_THRESHOLD: f64 = 0.8;

/// Medium organic ratio threshold
/// Ratios >= 0.3 indicate Mixed human/bot activity
const ORGANIC_RATIO_MEDIUM_THRESHOLD: f64 = 0.3;

/// Weight multiplier for high organic activity (Human actors)
/// Applied to transactions with organic ratio >= 0.8
const WEIGHT_MULTIPLIER_HUMAN_MIN: f64 = 1.5;
const WEIGHT_MULTIPLIER_HUMAN_MAX: f64 = 2.0;

/// Weight multiplier for medium organic activity (Mixed actors)
/// Applied to transactions with organic ratio 0.3-0.8
const WEIGHT_MULTIPLIER_MIXED_MIN: f64 = 1.0;
const WEIGHT_MULTIPLIER_MIXED_MAX: f64 = 1.5;

/// Weight multiplier for low organic activity (Sniper bots)
/// Applied to transactions with organic ratio < 0.3
const WEIGHT_MULTIPLIER_BOT_MIN: f64 = 0.5;
const WEIGHT_MULTIPLIER_BOT_MAX: f64 = 1.0;

/// Scaling factor for bot range (0.3 * 1.67 ≈ 0.5)
const BOT_RATIO_SCALE: f64 = 1.67;

/// Scaling factor for human range ((1.0 - 0.8) * 2.5 = 0.5)
const HUMAN_RATIO_SCALE: f64 = 2.5;

// =============================================================================
// QEDD Consolidation Detection Constants
// =============================================================================

/// Price stability threshold for consolidation detection
/// Price change < 5% is considered "stable" during volume drops
const CONSOLIDATION_PRICE_STABILITY_THRESHOLD: f64 = 0.95;

/// Transaction momentum threshold for consolidation detection
/// TX rate >= 0.5 tx/sec indicates continued interest
const CONSOLIDATION_TX_MOMENTUM_THRESHOLD: f64 = 0.5;

/// SOBP Normalization Formula (Fixed 2026-01-04)
///
/// Input: sobp_ratio ∈ [0.0, 1.0]
///   - 0.0 = 100% sells
///   - 0.5 = neutral (50/50)
///   - 1.0 = 100% buys
///
/// Output: sobp_normalized ∈ [-2.0, 3.0]
///
/// Buying pressure (ratio > 0.5):
///   - Uses exponential amplification:  (excess)^1.2 * 8.0
///   - 0.7 (70% buys) → ~1.16
///   - 0.9 (90% buys) → ~2.66
///   - 1.0 (100% buys) → 3.0 (capped)
///
/// Selling pressure (ratio < 0.5):
///   - Uses linear penalty: (ratio - 0.5) * 4.0
///   - 0.3 (70% sells) → -0.8
///   - 0.0 (100% sells) → -2.0 (capped)
fn normalize_sobp(sobp_ratio: f64) -> f32 {
    if sobp_ratio > 0.5 {
        let excess = sobp_ratio - 0.5;
        (excess.powf(1.2) * 8.0).min(3.0) as f32
    } else if sobp_ratio < 0.5 {
        ((sobp_ratio - 0.5) * 4.0).max(-2.0) as f32
    } else {
        0.0
    }
}

fn classify_actor_heuristic(tx: &TransactionRecord) -> ActorType {
    if tx.sol_amount >= 1.0 && tx.sol_amount <= 5.0 {
        ActorType::HumanDesktop
    } else if tx.sol_amount < 0.01 {
        ActorType::SniperScript
    } else {
        ActorType::Unknown
    }
}

fn mpcf_result_from_counts(
    human_count: usize,
    sniper_count: usize,
    mev_count: usize,
    unknown_count: usize,
    config: &MpcfConfig,
) -> MpcfResult {
    let total = human_count + sniper_count + mev_count + unknown_count;
    if total == 0 {
        return MpcfResult {
            score: 1.0,
            human_count,
            sniper_count,
            mev_count,
            unknown_count,
            human_ratio: 0.0,
            bot_ratio: 0.0,
            classification: MpcfClassification::Mixed,
        };
    }

    let total_f = total as f64;
    let human_ratio = human_count as f64 / total_f;
    let bot_ratio = (sniper_count + mev_count) as f64 / total_f;
    let unknown_ratio = unknown_count as f64 / total_f;

    let mut score = if human_ratio > config.high_organic_threshold as f64 {
        let excess = human_ratio - config.high_organic_threshold as f64;
        let organic_cap = (config.max_organic_boost - config.high_organic_base).max(0.0) as f64;
        config.high_organic_base as f64 + (excess * MPCF_ORGANIC_EXP_MULT).min(organic_cap)
    } else if bot_ratio > config.bot_dominated_threshold as f64 {
        let bot_severity =
            (bot_ratio - config.bot_dominated_threshold as f64) * MPCF_BOT_SEVERITY_MULT;
        (0.5 * (1.0 - bot_severity)).max(config.min_bot_penalty as f64)
    } else if human_ratio > MPCF_HUMAN_LINEAR_THRESHOLD {
        1.0 + (human_ratio - MPCF_HUMAN_LINEAR_THRESHOLD) * MPCF_HUMAN_LINEAR_MULT
    } else if unknown_ratio > MPCF_UNKNOWN_THRESHOLD {
        MPCF_UNKNOWN_BASE + (1.0 - unknown_ratio) * MPCF_UNKNOWN_ADJ
    } else {
        1.0
    };

    score = score.clamp(
        config.min_bot_penalty as f64,
        config.max_organic_boost as f64,
    );

    let classification = if human_ratio > config.high_organic_threshold as f64 {
        MpcfClassification::HighlyOrganic
    } else if bot_ratio > config.bot_dominated_threshold as f64 {
        MpcfClassification::BotDominated
    } else if human_ratio > MPCF_HUMAN_LINEAR_THRESHOLD {
        MpcfClassification::ModerateOrganic
    } else if unknown_ratio > MPCF_UNKNOWN_THRESHOLD {
        MpcfClassification::UnknownDominated
    } else {
        MpcfClassification::Mixed
    };

    MpcfResult {
        score,
        human_count,
        sniper_count,
        mev_count,
        unknown_count,
        human_ratio,
        bot_ratio,
        classification,
    }
}

/// Scale factor to map SOBP momentum [-2.0, 3.0] into the price_delta proxy range [-1.0, 1.2]
const SOBP_PRICE_DELTA_SCALE: f64 = 2.5;
const SOBP_PRICE_DELTA_MIN: f64 = -1.0;
const SOBP_PRICE_DELTA_MAX: f64 = 1.2;

// =============================================================================
// Engine Action - Decision Output
// =============================================================================

/// The result of a prediction cycle or session
#[derive(Debug, Clone, PartialEq)]
pub enum EngineAction {
    /// Continue to next cycle - no decision yet
    Continue,

    /// Execute buy with specified SOL amount
    /// Triggered by Gunshot (early) or Final Verdict (S12)
    Buy(f64),

    /// Kill session with reason
    /// Triggered by VETO conditions (LIGMA trap, rug detection, etc.)
    Kill(String),
}

// =============================================================================
// Cycle Result - Single Heartbeat Output
// =============================================================================

/// Result of a single heartbeat cycle
#[derive(Debug, Clone)]
pub struct CycleResult {
    /// Cycle number (1-12)
    pub cycle_id: u8,

    /// Score for this cycle (0.0-100.0)
    pub score: f64,

    /// Raw score before TCF adjustments (if available)
    pub raw_score: Option<f64>,

    /// True if IWIM was applied in this cycle's scoring
    pub iwim_applied: bool,

    /// Diagnostic source for IWIM usage
    pub iwim_source: IwimSource,

    /// Diagnostic threat score used (if any)
    pub iwim_threat_score: Option<f32>,

    /// Veto reason (if cycle killed due to critical guardrails)
    pub veto_reason: Option<VetoReason>,

    /// Defer reason (if cycle evaluation deferred/postponed)
    pub defer_reason: Option<DeferReason>,

    /// Whether this cycle triggered a Gunshot (immediate buy)
    pub is_gunshot: bool,

    /// Action to take after this cycle
    pub action: EngineAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IwimSource {
    Disabled,
    NotFinalPhase,
    NoProvider,
    ProviderMiss,
    ProviderHit,
}

/// MPCF classification buckets for telemetry and decision logging
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MpcfClassification {
    HighlyOrganic,
    ModerateOrganic,
    Mixed,
    BotDominated,
    UnknownDominated,
}

/// Detailed MPCF scoring result based on transaction counts
#[derive(Debug, Clone)]
pub struct MpcfResult {
    pub score: f64,
    pub human_count: usize,
    pub sniper_count: usize,
    pub mev_count: usize,
    pub unknown_count: usize,
    pub human_ratio: f64,
    pub bot_ratio: f64,
    pub classification: MpcfClassification,
}

// =============================================================================
// MPCF Scoring Constants
// =============================================================================

const MPCF_ORGANIC_EXP_MULT: f64 = 8.0;
const MPCF_BOT_SEVERITY_MULT: f64 = 2.0;
const MPCF_HUMAN_LINEAR_THRESHOLD: f64 = 0.4;
const MPCF_HUMAN_LINEAR_MULT: f64 = 1.67;
const MPCF_UNKNOWN_THRESHOLD: f64 = 0.6;
const MPCF_UNKNOWN_BASE: f64 = 0.8;
const MPCF_UNKNOWN_ADJ: f64 = 0.5;

// =============================================================================
// Prediction Session - State Container
// =============================================================================

/// State container for a single pool's prediction lifecycle
///
/// Manages the 12-cycle evaluation process with access to:
/// - Shadow Ledger is diagnostic-only (not a scoring input)
/// - Cycle history for derivative calculations
/// - Configuration for timing and thresholds
/// - Module instances for scoring calculations
pub struct PredictionSession {
    /// Base mint (token address) - PRIMARY KEY for snapshot access
    base_mint: Pubkey,

    /// Pool AMM ID (for logging/debugging only)
    pool_amm_id: Pubkey,

    /// Bonding curve address - CANONICAL KEY for curve state lookups in ShadowLedger
    /// This is the ONLY correct key for accessing curves in shadow_ledger.get()
    bonding_curve: Pubkey,

    /// Session start time (for elapsed calculations)
    start_time: Instant,

    /// Shared reference to Shadow Ledger for diagnostics only (never scoring input)
    shadow_ledger: Arc<ShadowLedger>,

    /// Shared reference to Snapshot Engine for transaction buffer access
    snapshot_engine: Option<Arc<SnapshotEngine>>,

    /// Optional provider for IWIM cache (runtime-owned)
    iwim_provider: Option<Arc<dyn IwimProvider>>,

    /// Optional provider for PANIC raw transaction metadata
    panic_provider: Option<Arc<dyn PanicProvider>>,

    /// Cached feature flag value for IWIM integration
    iwim_enabled: bool,

    /// Current cycle number (0-12, where 0 = pre-start)
    current_cycle: u8,

    /// Maximum number of cycles to run
    max_cycles: u8,

    /// Duration of each cycle
    cycle_duration: Duration,

    /// Full history of cycle results (not just scores)
    history: Vec<CycleResult>,

    /// Trend Cohesion Field for verifying trend authenticity (thread-safe for async spawning)
    tcf: Mutex<TrendCohesionField>,

    /// SurvivorScore calculator for cycle scoring
    survivor_calculator: SurvivorScoreCalculator,

    /// MESA analyzer for microstructure analysis
    mesa_analyzer: MesaAnalyzer,

    /// BVA analyzer for early-stage behavioral scoring
    bva_analyzer: BvaAnalyzer,

    /// BVA state for this token (0-7s window)
    bva_state: Option<BvaState>,

    /// Whether BVA was archived to Shadow Ledger
    bva_archived: bool,

    /// QEDD engine for survival probability calculation
    qedd_engine: QeddEngine,

    /// MPCF tuning parameters (count-based weighting)
    mpcf_config: MpcfConfig,

    /// Cached MPCF method (count/volume/ab)
    mpcf_method: String,

    /// Whether to log both MPCF methods for A/B telemetry
    mpcf_log_both: bool,

    /// Monte Carlo engine for chaos simulations
    chaos_engine: ChaosEngine,

    /// SOBP Core for slot-over-slot buying pressure analysis (thread-safe for async)
    sobp_core: Mutex<SobpCore<64>>,
    /// Deduplication for SOBP inputs across cycles.
    sobp_seen_tx_keys: HashSet<u64>,

    /// Minimum CIR-emitted transactions required for SOBP during early (0-7s) window
    sobp_min_emitted_tx_early: usize,

    /// CIR Core for causal impact filtering (thread-safe for async)
    cir_core: Mutex<CirCore>,

    /// Cached CIR global score from latest processing
    last_cir_global: Option<f64>,

    /// Whether any transactions have been observed for CIR processing
    cir_events_seen: bool,

    /// Count of consecutive snapshots observed for warmup gating (event-time based).
    live_snapshots_seen: u8,

    /// PANIC state for latent demand detection
    panic_state: PanicState,

    /// PANIC configuration
    panic_config: PanicConfig,

    /// ECTO state for early genesis window analysis
    ecto_state: EctoState,

    /// TCR-Φ core for temporal causality resonance
    tcr_core: Mutex<TcrPhiCore>,

    /// Last emitted TCR-Φ score
    last_tcr_score: Option<TcrScore>,

    /// Last processed transaction timestamp for PANIC
    panic_last_processed_ts_ms: u64,

    /// Offline anomaly state (slot/fee/frantic)
    market_anomaly_state: MarketAnomalyState,

    /// Signer entropy tracker for PANIC/SCR sanity checks
    signer_entropy_state: SignerEntropyState,

    /// Config path for behavioral scoring reloads
    behavioral_config_path: Option<String>,

    /// Last modified timestamp for behavioral config reloads
    behavioral_config_mtime: Option<std::time::SystemTime>,

    /// First event timestamp observed (for session-level early window detection)
    first_event_ts_ms: Option<u64>,

    /// Latch: Once true, the session is permanently in "Stable Window"
    reached_stable_state: bool,

    /// Last evaluated "current" timestamp (live counters)
    last_eval_current_ts_ms: u64,
    /// Last evaluated "current" tx count (live counters)
    last_eval_current_tx_count: u64,
    /// Last evaluated "current" cumulative volume (live counters)
    last_eval_current_volume_sol: f64,
    /// Last evaluated snapshot ring size (live counters)
    last_eval_snapshot_count: usize,
    /// Last evaluated tx buffer length (live counters)
    last_eval_tx_buffer_len: usize,
    /// Last evaluated unique tx count in window (post-dedup)
    last_eval_unique_tx_in_window: usize,
    /// Last stage reported to SurvivorScore for transition diagnostics.
    last_survivor_stage: Option<SurvivorSessionStage>,
    /// Canonical current event-time for the active cycle.
    cycle_now_event_ts_ms: u64,
    /// Event-time window start for the active cycle.
    start_event_ts_ms: u64,
    /// Event-time window end for the active cycle.
    end_event_ts_ms: u64,
    /// Single readiness gate for cycle-time consumers (SOBP/CIR).
    cycle_window_ready: bool,
    /// Transactions in cycle event-time window.
    txs_in_cycle_window_count: usize,
    /// Event-time range fed into SOBP.
    sobp_input_event_ts_min_ms: Option<u64>,
    /// Event-time range fed into SOBP.
    sobp_input_event_ts_max_ms: Option<u64>,
    /// Event-time range fed into CIR.
    cir_input_event_ts_min_ms: Option<u64>,
    /// Event-time range fed into CIR.
    cir_input_event_ts_max_ms: Option<u64>,
    /// Configured analysis window (target scoring window).
    analysis_window_ms_config: u64,
    /// Effective retention span in tx buffer for current cycle.
    retention_span_ms_effective: u64,
    /// Effective cycle span: min(analysis_window, retention_span).
    cycle_window_span_ms: u64,
    /// Whether tx buffer reached fixed capacity.
    tx_buffer_at_capacity: bool,
    /// Event-time source of canonical cycle timestamp.
    event_ts_source: EventTsSource,
    /// Sort telemetry for CIR/SOBP cycle input.
    input_sorted: bool,
    tx_out_of_order_count_before: usize,
    tx_out_of_order_count_after: usize,
    /// CIR/SOBP telemetry for reasoned no-emit paths.
    cir_input_count: usize,
    cir_emitted_count: usize,
    cir_ic_only_count: usize,
    sobp_input_count: usize,
    sobp_event_count_internal: usize,
    sobp_bucket_fill_ratio: f64,
    last_no_emit_reason_code: Option<NoEmitReasonCode>,
    reason_code_version: u32,
    ready_false_reason: Option<ReadyFalseReason>,
}

impl PredictionSession {
    #[inline]
    fn is_out_of_order_pair(a: &TransactionRecord, b: &TransactionRecord) -> bool {
        a.timestamp_ms > b.timestamp_ms || (a.timestamp_ms == b.timestamp_ms && a.seq_no > b.seq_no)
    }

    #[inline]
    fn tcr_timing_ts_ms(tx: &TransactionRecord) -> Option<u64> {
        tx.decision_event_ts_ms()
    }

    #[inline]
    fn has_decision_event_clock(source: EventTsSource, timestamp_ms: u64) -> bool {
        source.decision_event_ts_ms(timestamp_ms).is_some()
    }

    #[inline]
    fn normalize_decision_tx(mut tx: TransactionRecord) -> Option<TransactionRecord> {
        let decision_ts_ms = tx.decision_event_ts_ms()?;
        tx.timestamp_ms = decision_ts_ms;
        Some(tx)
    }

    fn decision_axis_transactions<I>(transactions: I) -> Vec<TransactionRecord>
    where
        I: IntoIterator<Item = TransactionRecord>,
    {
        transactions
            .into_iter()
            .filter_map(Self::normalize_decision_tx)
            .collect()
    }

    fn decision_window_transactions<I>(
        transactions: I,
        start_event_ts_ms: u64,
        end_event_ts_ms: u64,
    ) -> Vec<TransactionRecord>
    where
        I: IntoIterator<Item = TransactionRecord>,
    {
        Self::decision_axis_transactions(transactions)
            .into_iter()
            .filter(|tx| tx.timestamp_ms >= start_event_ts_ms && tx.timestamp_ms <= end_event_ts_ms)
            .collect()
    }

    #[inline]
    fn current_event_axis_ts_ms(&self, current: &MarketSnapshot) -> u64 {
        self.event_ts_source
            .decision_event_ts_ms(self.cycle_now_event_ts_ms)
            .or_else(|| {
                self.event_ts_source
                    .decision_event_ts_ms(current.timestamp_ms)
            })
            .unwrap_or(0)
    }

    #[inline]
    fn count_out_of_order(transactions: &[TransactionRecord]) -> usize {
        transactions
            .windows(2)
            .filter(|w| Self::is_out_of_order_pair(&w[0], &w[1]))
            .count()
    }

    #[inline]
    fn classify_no_emit_reason(
        tx_out_of_order_count_before: usize,
        cir_span_ms: u64,
        cir_ic_only_count: usize,
    ) -> NoEmitReasonCode {
        if tx_out_of_order_count_before > 0 {
            NoEmitReasonCode::OutOfOrderInput
        } else if cir_span_ms < CirConfig::default().tau2_ms {
            NoEmitReasonCode::WindowSpanInsufficient
        } else if cir_ic_only_count > 0 {
            NoEmitReasonCode::ResponderGatesNotMet
        } else {
            NoEmitReasonCode::ThetaNotReached
        }
    }

    #[inline]
    fn normalize_slot_metadata(
        slot: Option<u64>,
        origin: &str,
        base_mint: Pubkey,
        pool_amm_id: Pubkey,
    ) -> Option<u64> {
        match slot {
            Some(0) => {
                increment_counter!("slot_contract_violation_total");
                warn!(
                    base_mint = %base_mint,
                    pool = %pool_amm_id,
                    origin = origin,
                    "SLOT_CONTRACT_VIOLATION: received slot=0, normalizing to None"
                );
                None
            }
            other => other,
        }
    }

    /// Create a new prediction session
    ///
    /// # Arguments
    ///
    /// * `base_mint` - Token address (PRIMARY KEY for snapshot access)
    /// * `pool_amm_id` - Pool AMM ID (for logging/debugging)
    /// * `bonding_curve` - Bonding curve address (CANONICAL KEY for curve lookups)
    /// * `shadow_ledger` - Shared reference to Shadow Ledger
    /// * `snapshot_engine` - Optional reference to Snapshot Engine for transaction access
    ///
    /// # Returns
    ///
    /// A new `PredictionSession` ready to run
    pub fn new(
        base_mint: Pubkey,
        pool_amm_id: Pubkey,
        bonding_curve: Pubkey,
        shadow_ledger: Arc<ShadowLedger>,
        snapshot_engine: Option<Arc<SnapshotEngine>>,
    ) -> Self {
        let config = PredictionSessionConfig::from_env();
        Self::new_with_config(
            base_mint,
            pool_amm_id,
            bonding_curve,
            shadow_ledger,
            snapshot_engine,
            config,
        )
    }

    pub fn new_with_config(
        base_mint: Pubkey,
        pool_amm_id: Pubkey,
        bonding_curve: Pubkey,
        shadow_ledger: Arc<ShadowLedger>,
        snapshot_engine: Option<Arc<SnapshotEngine>>,
        config: PredictionSessionConfig,
    ) -> Self {
        // Initialize QEDD engine with default config
        let qedd_config = QeddConfig::default();
        let qedd_engine = QeddEngine::new(qedd_config);

        // Initialize Chaos engine with default config
        let sim_config = SimulationConfig::default();
        let chaos_engine = ChaosEngine::new(sim_config);

        // MPCF configuration (count-weighted scoring defaults)
        let mpcf_config = MpcfConfig::default();
        let mpcf_method = env::var("GHOST_MPCF_METHOD").unwrap_or_else(|_| "count".to_string());
        let mpcf_log_both = mpcf_method.eq_ignore_ascii_case("ab")
            || env::var("GHOST_MPCF_LOG_BOTH").is_ok()
            || mpcf_method.eq_ignore_ascii_case("volume");

        // Initialize SOBP core with event-time thresholds aligned to session config.
        let mut sobp = SobpCore::<64>::new();
        sobp.min_events = config.sobp_min_emitted_tx_early.max(1);
        let sobp_core = Mutex::new(sobp);

        // Initialize CIR core with defaults
        let cir_core = Mutex::new(CirCore::new(CirConfig::default()));

        // Initialize TCR-Φ core using config
        let tcr_core = Mutex::new(TcrPhiCore::new(config.tcr_phi_config));

        Self {
            base_mint,
            pool_amm_id,
            bonding_curve,
            start_time: Instant::now(),
            shadow_ledger,
            snapshot_engine,
            iwim_provider: None,
            panic_provider: None,
            iwim_enabled: config.iwim_enabled,
            current_cycle: 0,
            max_cycles: 12,
            cycle_duration: config.cycle_duration,

            // Initialize newly added fields
            history: Vec::with_capacity(12),
            // Używamy konfiguracji "pump_sensitive" - czułej na nagłe skoki
            tcf: Mutex::new(TrendCohesionField::pump_detector()),

            // Module instances
            survivor_calculator: config
                .ghost_brain_config
                .as_ref()
                .map(SurvivorScoreCalculator::from_ghost_brain_config)
                .unwrap_or_else(SurvivorScoreCalculator::new),
            mesa_analyzer: MesaAnalyzer::new(),
            bva_analyzer: BvaAnalyzer::new(config.bva_config),
            bva_state: None,
            bva_archived: false,
            qedd_engine,
            mpcf_config,
            mpcf_method,
            mpcf_log_both,
            chaos_engine,
            sobp_core,
            sobp_seen_tx_keys: HashSet::with_capacity(256),
            cir_core,
            last_cir_global: None,
            sobp_min_emitted_tx_early: config.sobp_min_emitted_tx_early,
            cir_events_seen: false,
            live_snapshots_seen: 0,
            panic_state: PanicState::new(),
            panic_config: config.panic_config.clone(),
            ecto_state: EctoState::new(EctoConfig::default(), None),
            tcr_core,
            last_tcr_score: None,
            panic_last_processed_ts_ms: 0,
            market_anomaly_state: MarketAnomalyState::new(),
            signer_entropy_state: SignerEntropyState::new(),
            behavioral_config_path: config.config_path.clone(),
            behavioral_config_mtime: None,
            first_event_ts_ms: None,
            reached_stable_state: false,
            last_eval_current_ts_ms: 0,
            last_eval_current_tx_count: 0,
            last_eval_current_volume_sol: 0.0,
            last_eval_snapshot_count: 0,
            last_eval_tx_buffer_len: 0,
            last_eval_unique_tx_in_window: 0,
            last_survivor_stage: None,
            cycle_now_event_ts_ms: 0,
            start_event_ts_ms: 0,
            end_event_ts_ms: 0,
            cycle_window_ready: false,
            txs_in_cycle_window_count: 0,
            sobp_input_event_ts_min_ms: None,
            sobp_input_event_ts_max_ms: None,
            cir_input_event_ts_min_ms: None,
            cir_input_event_ts_max_ms: None,
            analysis_window_ms_config: config.analysis_window_ms_config,
            retention_span_ms_effective: 0,
            cycle_window_span_ms: 0,
            tx_buffer_at_capacity: false,
            event_ts_source: EventTsSource::LegacyCompat,
            input_sorted: false,
            tx_out_of_order_count_before: 0,
            tx_out_of_order_count_after: 0,
            cir_input_count: 0,
            cir_emitted_count: 0,
            cir_ic_only_count: 0,
            sobp_input_count: 0,
            sobp_event_count_internal: 0,
            sobp_bucket_fill_ratio: 0.0,
            last_no_emit_reason_code: None,
            reason_code_version: 1,
            ready_false_reason: None,
        }
    }

    pub fn set_iwim_provider(&mut self, provider: Option<Arc<dyn IwimProvider>>) {
        self.iwim_provider = provider;
    }

    pub fn set_panic_provider(&mut self, provider: Option<Arc<dyn PanicProvider>>) {
        self.panic_provider = provider;
    }

    pub fn set_panic_config(&mut self, config: PanicConfig) {
        self.panic_config = config;
    }

    /// Strategia Wag Wykładniczych (S1=1.0 ... S12=22.0)
    fn get_cycle_weight(&self, cycle: u8) -> f64 {
        match cycle {
            1..=4 => 1.0 + (cycle as f64 - 1.0) * 0.45, // Faza Rozruchu (1.0 - 2.35)
            5..=9 => 2.8 + (cycle as f64 - 5.0) * 1.4,  // Faza Walki (2.8 - 8.4)
            10..=12 => 10.0 + (cycle as f64 - 10.0) * 6.0, // Faza Prawdy (10.0 - 22.0)
            _ => 1.0,
        }
    }

    #[inline]
    fn stage_from_cycle(cycle: u8) -> SurvivorSessionStage {
        match ScoringPhase::from_cycle(cycle.max(1)) {
            ScoringPhase::EarlyStage => SurvivorSessionStage::Early,
            ScoringPhase::FullAnalysis => SurvivorSessionStage::Full,
        }
    }

    fn session_stage_for_final_verdict(&self) -> SurvivorSessionStage {
        // Prefer the last scored cycle (non-deferred) to reflect effective scoring progress.
        let last_scored_cycle = self
            .history
            .iter()
            .rev()
            .find(|res| res.defer_reason.is_none())
            .map(|res| res.cycle_id);

        Self::stage_from_cycle(last_scored_cycle.unwrap_or(self.current_cycle))
    }

    /// Run the prediction session through all cycles
    ///
    /// This is the main heartbeat loop that executes 12 cycles at configurable intervals (default 420ms).
    /// It can exit early via:
    /// - Gunshot mechanism (high score in early cycles)
    /// - VETO conditions (liquidity traps, rug detection)
    /// - Data flow issues (no updates from Shadow Ledger)
    ///
    /// # Returns
    ///
    /// Final `EngineAction` (Buy, Kill, or Continue)
    pub async fn run(&mut self) -> EngineAction {
        let mut ticker = interval(self.cycle_duration);
        // Skip the first immediate tick - we want to start timing from now
        ticker.tick().await;

        let cycle_ms = self.cycle_duration.as_millis();
        info!(
            "🔄 [ENGINE] Session started for base_mint={} pool={} (12 cycles × {}ms)",
            self.base_mint, self.pool_amm_id, cycle_ms
        );

        loop {
            // Wait for next heartbeat
            ticker.tick().await;
            self.current_cycle += 1;

            debug!(
                "⚡ [ENGINE|heartbeat] Cycle S{}/12 for base_mint={} pool={} (elapsed={}ms)",
                self.current_cycle,
                self.base_mint,
                self.pool_amm_id,
                self.start_time.elapsed().as_millis()
            );

            // Execute single cycle evaluation
            let cycle_result = self.evaluate_cycle().await;

            // Store full cycle result for weighted averaging (not just score)
            self.history.push(cycle_result.clone());

            // Log cycle result
            info!(
                "⚡ CYCLE S{}: base_mint={} pool={} raw_score={:.2} final_score={:.2} gunshot={} action={:?} veto_reason={}",
                cycle_result.cycle_id,
                self.base_mint,
                self.pool_amm_id,
                cycle_result.raw_score.unwrap_or(cycle_result.score),
                cycle_result.score,
                cycle_result.is_gunshot,
                cycle_result.action,
                cycle_result
                    .veto_reason
                    .as_ref()
                    .map(|r| r.as_str())
                    .unwrap_or("none"),
            );

            // Check for early exit conditions
            match cycle_result.action {
                EngineAction::Buy(amount) => {
                    info!(
                        "🚀 [ENGINE] GUNSHOT TRIGGERED! base_mint={} pool={} cycle=S{} amount={:.2} SOL",
                        self.base_mint, self.pool_amm_id, self.current_cycle, amount
                    );
                    return EngineAction::Buy(amount);
                }
                EngineAction::Kill(reason) => {
                    warn!(
                        "💀 [ENGINE] Session killed: base_mint={} pool={} cycle=S{} reason={}",
                        self.base_mint, self.pool_amm_id, self.current_cycle, reason
                    );
                    return EngineAction::Kill(reason);
                }
                EngineAction::Continue => {
                    // Check if we've reached the end
                    if self.current_cycle >= self.max_cycles {
                        return self.final_verdict().await;
                    }
                    // Otherwise, continue to next cycle
                }
            }
        }
    }

    /// Converts organic ratio to weight multiplier for actor-based scoring
    ///
    /// This shared helper implements the actor weighting logic used by both
    /// SOBP and MPCF calculations:
    /// - High organic ratio (>= 0.8): Human actors → 1.5-2.0x multiplier
    /// - Medium organic ratio (0.3-0.8): Mixed → 1.0-1.5x multiplier
    /// - Low organic ratio (< 0.3): Sniper bots → 0.5-1.0x multiplier
    ///
    /// # Arguments
    /// * `organic_ratio` - Ratio of unique addresses to transactions (0.0-1.0)
    ///
    /// # Returns
    /// Weight multiplier in range [0.5, 2.0]
    fn organic_ratio_to_weight(organic_ratio: f64) -> f64 {
        if organic_ratio >= ORGANIC_RATIO_HIGH_THRESHOLD {
            // Human-like: High confidence
            WEIGHT_MULTIPLIER_HUMAN_MIN
                + (organic_ratio - ORGANIC_RATIO_HIGH_THRESHOLD) * HUMAN_RATIO_SCALE
        } else if organic_ratio >= ORGANIC_RATIO_MEDIUM_THRESHOLD {
            // Mixed: Moderate confidence
            WEIGHT_MULTIPLIER_MIXED_MIN + (organic_ratio - ORGANIC_RATIO_MEDIUM_THRESHOLD)
        } else {
            // Bot-like: Low confidence (but never zero!)
            WEIGHT_MULTIPLIER_BOT_MIN + organic_ratio * BOT_RATIO_SCALE
        }
    }

    fn update_panic_state(&mut self) -> PanicOutput {
        if let Some(provider) = &self.panic_provider {
            let transactions = provider
                .fetch_panic_transactions(self.pool_amm_id, self.panic_last_processed_ts_ms);

            for tx in transactions {
                if tx.arrival_ts_ms > self.panic_last_processed_ts_ms {
                    self.panic_last_processed_ts_ms = tx.arrival_ts_ms;
                }
                let accepted = self.panic_state.update(tx);
                if accepted {
                    self.signer_entropy_state.record_signer(tx.signer);
                    self.market_anomaly_state.update(MarketAnomalyTx {
                        slot: tx.slot,
                        event_ts_ms: tx.observation_ts_ms().unwrap_or(tx.arrival_ts_ms),
                        signer: tx.signer,
                        success: tx.success,
                        priority_fee_micro_lamports: tx.priority_fee_micro_lamports,
                        is_jito_bundle: false,
                    });
                }
            }
        }

        let mut output = self.panic_state.calculate_score(&self.panic_config);
        if let Some(score) = self.last_tcr_score {
            output.tcr_value = Some(score.tcr_value);
            output.tcr_confidence = Some(score.confidence);
            output.tcr_directional_bias = Some(score.directional_bias);
            output.tcr_variance = Some(score.variance_phi);
        }
        output
    }

    fn refresh_behavioral_config(&mut self) {
        let Some(path) = self.behavioral_config_path.as_deref() else {
            return;
        };

        let metadata = std::fs::metadata(path).ok();
        let modified = metadata.as_ref().and_then(|m| m.modified().ok());

        if modified.is_some() && modified == self.behavioral_config_mtime {
            return;
        }

        if let Ok(config) = GhostBrainConfig::from_toml_file(path) {
            self.survivor_calculator
                .update_behavioral_config(config.behavioral_scoring);
            self.behavioral_config_mtime = modified.or(Some(SystemTime::now()));
        }
    }

    fn cir_scale_from_panic(
        min_weight: f64,
        confidence_threshold: f64,
        panic_output: &PanicOutput,
    ) -> f64 {
        let min_weight = min_weight.clamp(0.0, 1.0);
        if panic_output.is_bot_spam
            || !panic_output.is_high_pressure
            || panic_output.confidence < confidence_threshold
        {
            min_weight
        } else {
            1.0
        }
    }

    fn tcr_impact_id(event_ts_ms: u64, signer: &Pubkey, amount_sol: f64, is_buy: bool) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        event_ts_ms.hash(&mut hasher);
        signer.hash(&mut hasher);
        amount_sol.to_bits().hash(&mut hasher);
        is_buy.hash(&mut hasher);
        hasher.finish()
    }

    fn adjust_scr_bot_score(
        scr_bot_score: f32,
        anomaly: MarketAnomalyOutput,
        panic_config: &PanicConfig,
        entropy_inconsistency: bool,
    ) -> f32 {
        let fee_spike_hint = anomaly.fee_spike.clamp(0.0, 1.0);
        let failed_ratio_hint = anomaly.failed_ratio.clamp(0.0, 1.0);
        let mut adjusted = scr_bot_score
            + (fee_spike_hint * panic_config.scr_fee_spike_weight
                + failed_ratio_hint * panic_config.scr_failed_ratio_weight) as f32;
        if entropy_inconsistency {
            adjusted += panic_config.scr_inconsistency_penalty as f32;
        }
        adjusted.clamp(0.0, 1.0)
    }

    /// Calculates SOBP (Slot-Over-Slot Buying Pressure) using CIR-filtered transactions.
    ///
    /// This method uses CIR (Causal Impact Ratio) to filter transactions that caused
    /// independent market reactions, and feeds ONLY those into SOBP.
    ///
    /// Returns SOBP ratio representing buying pressure:
    /// - Values > 0.5 indicate buying pressure (more buys than sells)
    /// - Values > 0.7 indicate strong buying (pump signal)
    /// - Values < 0.3 indicate selling pressure
    ///
    /// If snapshot_engine is not available, falls back to snapshot-level approximation.
    /// Calculates SOBP based on accumulated transactions using event-time.
    ///
    /// NOTE: Per FAZA 7, cycles are execution timers NOT market time.
    /// This method no longer takes current_slot as a parameter - it uses
    /// current_sobp() which operates on the current event-time state.
    fn calculate_sobp(
        &mut self,
        current_data: &MarketSnapshot,
        congestion_flag: bool,
        bva_active: bool,
        panic_output: &PanicOutput,
    ) -> Option<f64> {
        let current_ts_ms = self.current_event_axis_ts_ms(current_data);
        self.input_sorted = false;
        self.tx_out_of_order_count_before = 0;
        self.tx_out_of_order_count_after = 0;
        self.cir_input_count = 0;
        self.cir_emitted_count = 0;
        self.cir_ic_only_count = 0;
        self.sobp_input_count = 0;
        self.sobp_event_count_internal = 0;
        self.sobp_bucket_fill_ratio = 0.0;
        self.last_no_emit_reason_code = None;
        self.ready_false_reason = None;
        self.sobp_input_event_ts_min_ms = None;
        self.sobp_input_event_ts_max_ms = None;
        // Try to get transactions from snapshot engine
        if let Some(ref engine) = self.snapshot_engine {
            let mut transactions = Self::decision_window_transactions(
                engine.get_transactions(&self.pool_amm_id),
                self.start_event_ts_ms,
                self.end_event_ts_ms,
            );

            if transactions.is_empty() {
                debug!("No transactions available for SOBP calculation, returning None");
                return None;
            }

            if !transactions.is_empty() {
                self.tx_out_of_order_count_before = Self::count_out_of_order(&transactions);
                transactions.sort_by(|a, b| {
                    a.timestamp_ms
                        .cmp(&b.timestamp_ms)
                        .then_with(|| a.seq_no.cmp(&b.seq_no))
                });
                self.tx_out_of_order_count_after = Self::count_out_of_order(&transactions);
                self.input_sorted = true;
                self.cir_events_seen = true;
                self.txs_in_cycle_window_count = transactions.len();
                self.cir_input_count = transactions.len();
                self.sobp_input_count = transactions.len();
                self.cir_input_event_ts_min_ms =
                    transactions.iter().map(|tx| tx.timestamp_ms).min();
                self.cir_input_event_ts_max_ms =
                    transactions.iter().map(|tx| tx.timestamp_ms).max();
            } else {
                self.txs_in_cycle_window_count = 0;
                self.cir_input_event_ts_min_ms = None;
                self.cir_input_event_ts_max_ms = None;
            }

            // Ensure BVA state is initialized
            self.ensure_bva_state(current_data.slot, current_ts_ms, &transactions);
            if let Some(ref mut bva_state) = self.bva_state {
                bva_state.update_congestion_flag(congestion_flag);
            }

            let bva_window_ms = self
                .bva_analyzer
                .config()
                .primary_window_secs
                .saturating_mul(1000);
            let bva_window_closed = self
                .bva_state
                .as_ref()
                .map(|state| current_ts_ms.saturating_sub(state.birth_ts_ms) > bva_window_ms)
                .unwrap_or(false);

            // Unified window contract: when cycle window is not ready, both CIR and SOBP
            // are skipped with a single deterministic reason code.
            if !self.cycle_window_ready {
                if !bva_window_closed {
                    if let Some(ref mut bva_state) = self.bva_state {
                        for tx in &transactions {
                            bva_state.process_tx(tx, self.bva_analyzer.config());
                        }
                    }
                }
                debug!(
                    "[CIR] skipped_due_to_window=true pool={} cycle=S{} tx_count={} reason_code={} reason_code_version={} log_schema_version={}",
                    self.pool_amm_id,
                    self.current_cycle,
                    transactions.len(),
                    NoEmitReasonCode::SkippedDueToWindowContract.as_str(),
                    1,
                    LOG_SCHEMA_VERSION
                );
                debug!(
                    "SOBP skipped_due_to_window=true pool={} cycle=S{} elapsed_ms={} unique_tx_in_window={} slot={:?} reason_code={} reason_code_version={} log_schema_version={}",
                    self.pool_amm_id,
                    self.current_cycle,
                    current_ts_ms.saturating_sub(self.start_event_ts_ms),
                    transactions.len(),
                    current_data.slot,
                    NoEmitReasonCode::SkippedDueToWindowContract.as_str(),
                    1,
                    LOG_SCHEMA_VERSION
                );
                self.last_no_emit_reason_code = Some(NoEmitReasonCode::SkippedDueToWindowContract);
                if let Some(ref mut bva_state) = self.bva_state {
                    bva_state.update_sobp(None, true);
                }
                return None;
            }

            // Process transactions through CIR core and feed emitted txs into SOBP
            let mut sobp = self.sobp_core.lock();
            let mut cir = self.cir_core.lock();
            let mut tcr = self.tcr_core.lock();
            let mut tcr_best: Option<TcrScore> = None;
            let mut tcr_timestamp_lookup: std::collections::HashMap<u64, u64> =
                std::collections::HashMap::with_capacity(transactions.len().min(256));
            for tx in &transactions {
                if let Some(tcr_ts_ms) = Self::tcr_timing_ts_ms(tx) {
                    let key = Self::tcr_impact_id(tcr_ts_ms, &tx.signer, tx.sol_amount, tx.is_buy);
                    tcr_timestamp_lookup.insert(key, tcr_ts_ms);
                }
            }
            let mut emitted_count: usize = 0;
            let mut emitted_out_of_window_count: usize = 0;
            let mut emitted_buy_count: usize = 0;
            let mut emitted_amount_sum: f64 = 0.0;
            let mut emitted_cir_sum: f64 = 0.0;
            let mut emitted_min_ts_ms: Option<u64> = None;
            let mut emitted_max_ts_ms: Option<u64> = None;
            let cir_scale = Self::cir_scale_from_panic(
                self.bva_analyzer.config().cir_min_weight,
                self.panic_config.cir_confidence_threshold,
                panic_output,
            );

            for tx in &transactions {
                let normalized_slot = Self::normalize_slot_metadata(
                    tx.slot,
                    "engine.calculate_sobp",
                    self.base_mint,
                    self.pool_amm_id,
                );
                if tx.is_dev_buy {
                    let _ = self.ecto_state.set_dev_pubkey_once(tx.signer);
                }

                let _ = self.ecto_state.update_with_signer(
                    tx.signer,
                    tx.is_buy,
                    tx.sol_amount,
                    tx.timestamp_ms,
                );

                if let Some(signal) = self.ecto_state.analyze() {
                    if signal.confidence >= 0.3 && signal.window_ms >= 1_500 {
                        tcr.apply_ecto_signal(&signal);
                    }
                }

                let event = CirEvent {
                    slot: normalized_slot,
                    timestamp_ms: tx.timestamp_ms,
                    signer: tx.signer,
                    side: if tx.is_buy {
                        BuySell::Buy
                    } else {
                        BuySell::Sell
                    },
                    amount_sol: tx.sol_amount,
                };

                let key = cir_tx_key(
                    &tx.signature,
                    normalized_slot,
                    tx.timestamp_ms,
                    &tx.signer,
                    tx.sol_amount,
                );

                if !bva_window_closed {
                    if let Some(ref mut bva_state) = self.bva_state {
                        bva_state.process_tx(tx, self.bva_analyzer.config());
                    }
                }

                let emitted = cir.process_event(event, key);
                for emitted_tx in emitted {
                    let tcr_key = Self::tcr_impact_id(
                        emitted_tx.timestamp_ms,
                        &emitted_tx.signer,
                        emitted_tx.amount_sol,
                        emitted_tx.is_buy,
                    );
                    let impact_ts = tcr_timestamp_lookup.get(&tcr_key).copied();
                    tcr.register_impact(TcrImpact {
                        id: tcr_key,
                        slot: emitted_tx.slot,
                        timestamp_ms: impact_ts,
                        signer: emitted_tx.signer,
                        side: if emitted_tx.is_buy {
                            BuySell::Buy
                        } else {
                            BuySell::Sell
                        },
                    });

                    emitted_count += 1;
                    if emitted_tx.is_buy {
                        emitted_buy_count += 1;
                    }
                    emitted_amount_sum += emitted_tx.amount_sol;
                    let adjusted_cir = (emitted_tx.cir_effective * cir_scale).min(1.0);
                    emitted_cir_sum += adjusted_cir;
                    emitted_min_ts_ms = Some(
                        emitted_min_ts_ms
                            .map_or(emitted_tx.timestamp_ms, |s| s.min(emitted_tx.timestamp_ms)),
                    );
                    emitted_max_ts_ms = Some(
                        emitted_max_ts_ms
                            .map_or(emitted_tx.timestamp_ms, |s| s.max(emitted_tx.timestamp_ms)),
                    );
                    if !bva_window_closed {
                        if let Some(ref mut bva_state) = self.bva_state {
                            let config = self.bva_analyzer.config();
                            bva_state.register_cir_emitted(&emitted_tx, config);
                        }
                    }
                    if self.sobp_seen_tx_keys.insert(emitted_tx.tx_key) {
                        let sobp_ts_ms = impact_ts.unwrap_or(emitted_tx.timestamp_ms);
                        if sobp_ts_ms < self.start_event_ts_ms || sobp_ts_ms > self.end_event_ts_ms
                        {
                            emitted_out_of_window_count =
                                emitted_out_of_window_count.saturating_add(1);
                            continue;
                        }
                        let sobp_tx = SobpTransactionRecord {
                            slot: emitted_tx.slot,
                            actor_type: ActorType::Unknown,
                            amount_sol: emitted_tx.amount_sol as f32,
                            cir_effective: Some(adjusted_cir as f32),
                            tx_size_bytes: 0,
                            is_buy: emitted_tx.is_buy,
                            timestamp_ms: sobp_ts_ms,
                            price: None, // Price not available from CIR emission
                        };
                        sobp.record_transaction(&sobp_tx);
                        self.sobp_input_event_ts_min_ms = Some(
                            self.sobp_input_event_ts_min_ms
                                .map_or(sobp_ts_ms, |min_ts| min_ts.min(sobp_ts_ms)),
                        );
                        self.sobp_input_event_ts_max_ms = Some(
                            self.sobp_input_event_ts_max_ms
                                .map_or(sobp_ts_ms, |max_ts| max_ts.max(sobp_ts_ms)),
                        );
                    }
                }

                let reaction_ts = Self::tcr_timing_ts_ms(tx);
                let reactions = tcr.process_reaction(TcrReaction {
                    slot: normalized_slot,
                    timestamp_ms: reaction_ts,
                    signer: tx.signer,
                    side: if tx.is_buy {
                        BuySell::Buy
                    } else {
                        BuySell::Sell
                    },
                });
                if let Some(best) = reactions.into_iter().max_by(|a, b| {
                    a.tcr_value
                        .partial_cmp(&b.tcr_value)
                        .unwrap_or(std::cmp::Ordering::Less)
                }) {
                    if tcr_best
                        .as_ref()
                        .map(|current| best.tcr_value > current.tcr_value)
                        .unwrap_or(true)
                    {
                        tcr_best = Some(best);
                    }
                }
            }

            if emitted_count == 0 {
                let fallback_total = transactions.len();
                let mut fallback_inserted = 0usize;
                // Fallback must be idempotent per window; rebuild SOBP from current window.
                let mut rebuilt = SobpCore::<64>::new();
                rebuilt.min_events = self.sobp_min_emitted_tx_early.max(1);
                *sobp = rebuilt;
                let mut fallback_seen: std::collections::HashSet<u64> =
                    std::collections::HashSet::with_capacity(transactions.len().min(256));
                for tx in transactions.iter() {
                    let normalized_slot = Self::normalize_slot_metadata(
                        tx.slot,
                        "engine.calculate_sobp.fallback",
                        self.base_mint,
                        self.pool_amm_id,
                    );
                    let sobp_key = cir_tx_key(
                        tx.signature.as_str(),
                        normalized_slot,
                        tx.timestamp_ms,
                        &tx.signer,
                        tx.sol_amount,
                    );
                    if !fallback_seen.insert(sobp_key) {
                        continue;
                    }
                    if !bva_window_closed {
                        if let Some(ref mut bva_state) = self.bva_state {
                            bva_state.process_tx(tx, self.bva_analyzer.config());
                        }
                    }
                    let sobp_tx = SobpTransactionRecord {
                        slot: normalized_slot,
                        actor_type: ActorType::Unknown,
                        amount_sol: tx.sol_amount as f32,
                        cir_effective: None,
                        tx_size_bytes: 0,
                        is_buy: tx.is_buy,
                        timestamp_ms: tx.timestamp_ms,
                        price: None, // Price not available from fallback
                    };
                    sobp.record_transaction(&sobp_tx);
                    self.sobp_input_event_ts_min_ms = Some(
                        self.sobp_input_event_ts_min_ms
                            .map_or(tx.timestamp_ms, |min_ts| min_ts.min(tx.timestamp_ms)),
                    );
                    self.sobp_input_event_ts_max_ms = Some(
                        self.sobp_input_event_ts_max_ms
                            .map_or(tx.timestamp_ms, |max_ts| max_ts.max(tx.timestamp_ms)),
                    );
                    fallback_inserted += 1;
                }
                debug!(
                    "SOBP/CIR fallback to raw txs: pool={} cycle=S{} ts_ms={} total={} inserted={} cir_emitted_out_of_window={} reason_code={} reason_code_version={} log_schema_version={}",
                    self.pool_amm_id,
                    self.current_cycle,
                    current_ts_ms,
                    fallback_total,
                    fallback_inserted,
                    emitted_out_of_window_count,
                    NoEmitReasonCode::ThetaNotReached.as_str(),
                    1,
                    LOG_SCHEMA_VERSION
                );
                self.last_no_emit_reason_code = Some(NoEmitReasonCode::ThetaNotReached);
                if let Some(ref mut bva_state) = self.bva_state {
                    bva_state.update_sobp(None, true);
                }
            }

            self.last_cir_global = cir.global_score();
            if let Some(best) = tcr_best {
                self.last_tcr_score = Some(best);
                debug!(
                    "[TCR-Φ] pool={} impact={} value={:.3} conf={:.3} bias={:.3} var={:.4} n={}",
                    self.pool_amm_id,
                    best.impact_id,
                    best.tcr_value,
                    best.confidence,
                    best.directional_bias,
                    best.variance_phi,
                    best.sample_count
                );
            }
            cir.record_metrics();
            if let Some(snapshot) = cir.telemetry_snapshot() {
                self.cir_ic_only_count = snapshot.ic_only_count;
                debug!(
                    "[CIR] pool={} avg={:.3} IC={:.2} SC={:.2} AD={:.2} count={} ic_only_count={} log_schema_version={}",
                    self.pool_amm_id,
                    snapshot.avg_cir,
                    snapshot.avg_ic,
                    snapshot.avg_sc,
                    snapshot.avg_ad,
                    snapshot.count,
                    snapshot.ic_only_count,
                    LOG_SCHEMA_VERSION
                );
            }
            self.cir_emitted_count = emitted_count;

            if emitted_count == 0 {
                let cir_span_ms = self
                    .cir_input_event_ts_max_ms
                    .zip(self.cir_input_event_ts_min_ms)
                    .map(|(max_ts, min_ts)| max_ts.saturating_sub(min_ts))
                    .unwrap_or(0);
                let no_emit_reason = Self::classify_no_emit_reason(
                    self.tx_out_of_order_count_before,
                    cir_span_ms,
                    self.cir_ic_only_count,
                );
                debug!(
                    "SOBP/CIR emitted=0 pool={} cycle=S{} slot={:?} tx_count={} reason_code={} reason_code_version={} log_schema_version={}",
                    self.pool_amm_id,
                    self.current_cycle,
                    current_data.slot,
                    transactions.len(),
                    no_emit_reason.as_str(),
                    1,
                    LOG_SCHEMA_VERSION
                );
                self.last_no_emit_reason_code = Some(no_emit_reason);
            }

            let unique_tx_in_window = transactions.len();
            let elapsed_ms = if sobp.first_event_ts_ms == 0 {
                0
            } else {
                current_ts_ms.saturating_sub(sobp.first_event_ts_ms)
            };

            // Calculate SOBP ratio using current state (event-time based)
            // NOTE: We use current_sobp() instead of calculate_sobp(slot) per FAZA 7
            // Cycles are execution timers, NOT market time indicators
            if let Some(ratio) = sobp.current_sobp() {
                self.sobp_event_count_internal = sobp.event_count;
                self.sobp_bucket_fill_ratio = (sobp.slot_count() as f64 / 64.0).clamp(0.0, 1.0);
                if let Some(ref mut bva_state) = self.bva_state {
                    bva_state.update_sobp(Some(ratio as f64), emitted_count == 0);
                }
                let avg_cir_effective = if emitted_count > 0 {
                    emitted_cir_sum / emitted_count as f64
                } else {
                    0.0
                };
                debug!(
                    "SOBP (CIR-filtered): slot={:?} ratio={:.3} tx_count={} emitted={} emitted_out_of_window={} buys={} amount_sum={:.6} avg_cir={:.3} emitted_event_ts_range_ms={:?}-{:?} cir_global={:.3}",
                    current_data.slot,  // slot only for logging
                    ratio,
                    transactions.len(),
                    emitted_count,
                    emitted_out_of_window_count,
                    emitted_buy_count,
                    emitted_amount_sum,
                    avg_cir_effective,
                    emitted_min_ts_ms,
                    emitted_max_ts_ms,
                    self.last_cir_global.unwrap_or(0.0)
                );
                Some(ratio as f64)
            } else {
                self.sobp_event_count_internal = sobp.event_count;
                self.sobp_bucket_fill_ratio = (sobp.slot_count() as f64 / 64.0).clamp(0.0, 1.0);
                // No SOBP calculated yet - use fallback based on event-time window
                if bva_active {
                    debug!(
                        "SOBP none merytoryczne: pool={} cycle=S{} elapsed_ms={} event_count_internal={} unique_tx_in_window={} slot={:?} reason_code={} reason_code_version={} log_schema_version={}",
                        self.pool_amm_id,
                        self.current_cycle,
                        elapsed_ms,
                        sobp.event_count,
                        unique_tx_in_window,
                        current_data.slot,
                        NoEmitReasonCode::SobpDataUnavailable.as_str(),
                        1,
                        LOG_SCHEMA_VERSION
                    );
                    self.last_no_emit_reason_code = Some(NoEmitReasonCode::SobpDataUnavailable);
                    if let Some(ref mut bva_state) = self.bva_state {
                        bva_state.update_sobp(None, true);
                    }
                    None
                } else {
                    // Fallback to neutral when no SOBP data available
                    // NOTE: No slot comparisons - we don't decide based on slots per FAZA 7
                    debug!(
                        "SOBP insufficient data, returning neutral (slot={:?})",
                        current_data.slot
                    );
                    Some(0.5)
                }
            }
        } else {
            // Fallback: snapshot-level approximation (for backward compatibility)
            warn!("SnapshotEngine not available, using snapshot-level SOBP approximation");
            if bva_active {
                None
            } else {
                Some(0.5) // Neutral when no data available
            }
        }
    }

    /// Calculates MPCF (Multi-Party Confidence Factor) using transaction **count**
    /// weighting instead of SOL volume weighting. Returns a detailed breakdown
    /// for telemetry and downstream scoring.
    fn calculate_mpcf(&self) -> MpcfResult {
        if let Some(ref engine) = self.snapshot_engine {
            let transactions = engine.get_transactions(&self.pool_amm_id);

            if transactions.is_empty() {
                return mpcf_result_from_counts(0, 0, 0, 0, &self.mpcf_config);
            }

            // MPCF disabled when no raw bytes are present (PumpPortal WS limitation)
            if transactions.iter().all(|tx| tx.raw_bytes.is_none()) {
                debug!(
                    "MPCF disabled (no raw bytes): pool={} tx_count={}",
                    self.pool_amm_id,
                    transactions.len()
                );
                return mpcf_result_from_counts(0, 0, 0, 0, &self.mpcf_config);
            }

            let mut human_count = 0usize;
            let mut sniper_count = 0usize;
            let mut mev_count = 0usize;
            let mut unknown_count = 0usize;
            let mut weighted_volume = 0.0;
            let mut total_volume = 0.0;

            for tx in &transactions {
                let actor_type = self.infer_actor_type(tx);

                match actor_type {
                    ActorType::HumanMobile | ActorType::HumanDesktop => human_count += 1,
                    ActorType::SniperScript => sniper_count += 1,
                    ActorType::MEVArb | ActorType::SybilBot => mev_count += 1,
                    _ => unknown_count += 1,
                }

                let weight = match actor_type {
                    ActorType::HumanMobile | ActorType::HumanDesktop => 2.0,
                    ActorType::SniperScript | ActorType::MEVArb | ActorType::SybilBot => 0.5,
                    ActorType::LiquidityBot | ActorType::RPCFiller | ActorType::Unknown => 1.0,
                };

                weighted_volume += tx.sol_amount * weight;
                total_volume += tx.sol_amount;
            }

            let count_result = mpcf_result_from_counts(
                human_count,
                sniper_count,
                mev_count,
                unknown_count,
                &self.mpcf_config,
            );

            let volume_score = if total_volume > 0.0 {
                (weighted_volume / total_volume).clamp(
                    self.mpcf_config.min_bot_penalty as f64,
                    self.mpcf_config.max_organic_boost as f64,
                )
            } else {
                1.0
            };

            let mut result = count_result.clone();
            if self.mpcf_method.eq_ignore_ascii_case("volume") {
                result.score = volume_score;
            }

            if self.mpcf_log_both {
                debug!(
                    "MPCF A/B: tx_count={} count_score={:.3} volume_score={:.3} delta={:.3} human={:.1}% bot={:.1}%",
                    transactions.len(),
                    count_result.score,
                    volume_score,
                    count_result.score - volume_score,
                    count_result.human_ratio * 100.0,
                    count_result.bot_ratio * 100.0,
                );
            }

            debug!(
                "MPCF (count): tx_count={} score={:.3} human={:.1}% bot={:.1}% classification={:?}",
                transactions.len(),
                result.score,
                result.human_ratio * 100.0,
                result.bot_ratio * 100.0,
                result.classification
            );

            result
        } else {
            warn!("SnapshotEngine not available for MPCF, returning neutral");
            mpcf_result_from_counts(0, 0, 0, 0, &self.mpcf_config)
        }
    }

    fn infer_actor_type(&self, tx: &TransactionRecord) -> ActorType {
        let _ = tx;
        ActorType::Unknown
    }

    /// Logika Fazy (Safety Check)
    fn verify_safety(&self, cycle: u8) -> f64 {
        // Faza 1 (S1-S7): Domniemanie niewinności
        if cycle <= 7 {
            return 1.0;
        }

        // Faza 2 (S8-S12): Tu w przyszłości wepniemy wynik IWIM
        0.9
    }

    /// Convert BondingCurve to AmmPool for module calculations
    fn bonding_curve_to_amm_pool(curve: &BondingCurve) -> Result<AmmPool, String> {
        AmmPool::new(
            curve.virtual_sol_reserves as u128,
            curve.virtual_token_reserves as u128,
            PUMP_FUN_FEE_BPS,
        )
        .map_err(|e| format!("Failed to create AmmPool: {}", e))
    }

    /// Convert live snapshot reserves to AmmPool for Chaos simulation
    fn snapshot_to_amm_pool(snapshot: &MarketSnapshot) -> Option<AmmPool> {
        if !snapshot.reserve_base.is_finite() || !snapshot.reserve_quote.is_finite() {
            return None;
        }

        if snapshot.reserve_base <= MIN_RESERVE_THRESHOLD
            || snapshot.reserve_quote <= MIN_RESERVE_THRESHOLD
        {
            return None;
        }

        // CHAOS uses AmmPool where reserve_a is SOL and reserve_b is the token.
        // SnapshotEngine provides reserve_quote as SOL and reserve_base as token.
        let reserve_a = snapshot.reserve_quote.max(0.0).round() as u128;
        let reserve_b = snapshot.reserve_base.max(0.0).round() as u128;

        AmmPool::new(reserve_a, reserve_b, PUMP_FUN_FEE_BPS).ok()
    }

    /// Build TransactionMetrics from snapshot deltas for MESA analysis
    fn build_transaction_metrics(
        current: &MarketSnapshot,
        prev: Option<&MarketSnapshot>,
        cycle_duration_ms: f64,
    ) -> TransactionMetrics {
        let (delta_tx, delta_volume, delta_unique) = match prev {
            Some(p) => (
                current.tx_count.saturating_sub(p.tx_count) as usize,
                current.cum_volume_sol - p.cum_volume_sol,
                current.unique_addrs.saturating_sub(p.unique_addrs) as usize,
            ),
            None => (
                current.tx_count as usize,
                current.cum_volume_sol,
                current.unique_addrs as usize,
            ),
        };

        let (mut interval_ms, interval_source): (f64, IntervalSource) = match prev {
            Some(p) => {
                let ts_delta = current.timestamp_ms.saturating_sub(p.timestamp_ms);
                if ts_delta > 0 {
                    (ts_delta as f64, IntervalSource::TimestampDelta)
                } else {
                    (cycle_duration_ms, IntervalSource::Unknown)
                }
            }
            None => (cycle_duration_ms, IntervalSource::Unknown),
        };

        // Guard against zero/degenerate intervals
        interval_ms = interval_ms.max(1.0_f64);

        // Build metrics from snapshot data
        TransactionMetrics {
            tx_count: delta_tx,
            unique_addrs: delta_unique,
            total_volume_sol: delta_volume,
            buy_count: delta_tx / 2, // Estimate 50/50 split
            sell_count: delta_tx / 2,
            buy_volume_sol: delta_volume * 0.6, // Estimate buy-heavy (60/40)
            sell_volume_sol: delta_volume * 0.4,
            max_tx_sol: delta_volume / delta_tx.max(1) as f64, // Average as proxy
            volumes_sol: vec![],          // Empty for now, can be enhanced later
            is_buys: vec![],              // Empty for now, can be enhanced later
            avg_interval_ms: interval_ms, // Derived from real delta time/slot when available
            interval_std_dev: DEFAULT_INTERVAL_STD_DEV,
            interval_source,
            has_dev_activity: false,
            dev_volume_sol: 0.0,
        }
    }

    /// Build SurvivorScoreInput from current market state
    ///
    /// This collects all module outputs and packages them for SurvivorScore calculation.
    /// Now calls REAL modules (MESA, Chaos) instead of simplified estimations.
    fn build_survivor_input(
        &mut self,
        current: &MarketSnapshot,
        prev: Option<&MarketSnapshot>,
        bva_active: bool,
        bva_output: Option<&BvaOutput>,
        panic_output: &PanicOutput,
        entropy_inconsistency: bool,
    ) -> SurvivorScoreInput {
        let mut input = SurvivorScoreInput::default();
        input.session_stage = Some(Self::stage_from_cycle(self.current_cycle));

        // Initialize BVA state on first snapshot (birth)
        let current_event_ts_ms = self.current_event_axis_ts_ms(current);
        self.ensure_bva_state(current.slot, current_event_ts_ms, &[]);

        // === SOBP Momentum ===
        // Uses event-time based SOBP calculation (FAZA 7: cycles are execution timers, NOT market time)
        let congestion_flag = self.estimate_congestion(current, prev);
        let sobp_ratio = self.calculate_sobp(current, congestion_flag, bva_active, panic_output);
        let delta_tx = prev
            .map(|p| current.tx_count.saturating_sub(p.tx_count))
            .unwrap_or(current.tx_count);
        let delta_volume = prev
            .map(|p| (current.cum_volume_sol - p.cum_volume_sol).max(0.0))
            .unwrap_or(current.cum_volume_sol.max(0.0));
        // Zero activity guard: if no newly emitted market activity, keep SOBP neutral.
        // This prevents mapping ratio=1.0 into an artificial positive momentum.
        if delta_tx == 0 && delta_volume <= VOLUME_COMPARISON_EPSILON {
            input.sobp_momentum = None;
        } else {
            // SOBP returns ratio 0.0-1.0, normalize to [-2.0, 3.0] momentum where 0.0 = neutral
            // Ratios >0.7 amplify buying pressure; ratios <0.3 apply linear selling penalty
            input.sobp_momentum = sobp_ratio.map(normalize_sobp);
        }

        // === CIR Global Score (MPCF disabled) ===
        let cir_scale = Self::cir_scale_from_panic(
            self.bva_analyzer.config().cir_min_weight,
            self.panic_config.cir_confidence_threshold,
            panic_output,
        );
        let cir_global = (self.last_cir_global.unwrap_or(0.0) * cir_scale).min(1.0);
        if !bva_active {
            input.mpcf_organic_ratio = Some(cir_global as f32);
        }
        input.cir_strength = Some(cir_global as f32);

        // === MESA Analysis (REAL MODULE) ===
        // SnapshotEngine is the sole market-state source for scoring.
        if !bva_active {
            if let Some(amm_pool) = Self::snapshot_to_amm_pool(current) {
                let metrics = Self::build_transaction_metrics(
                    current,
                    prev,
                    self.cycle_duration.as_millis() as f64,
                );
                let mesa_result = self
                    .mesa_analyzer
                    .analyze_microstructure(&amm_pool, &[metrics]);

                input.mesa_organic_likeness = Some(mesa_result.organic_likeness);
                input.mesa_wash_likeness = Some(mesa_result.wash_likeness);

                debug!(
                    "🔬 [MESA] base_mint={} pool={} organic={:.3} wash={:.3} bot={:.3}",
                    self.base_mint,
                    self.pool_amm_id,
                    mesa_result.organic_likeness,
                    mesa_result.wash_likeness,
                    mesa_result.bot_likeness
                );
            } else {
                debug!(
                    "🔬 [MESA] Skipping microstructure: snapshot lacks stable reserves (base_mint={} pool={})",
                    self.base_mint,
                    self.pool_amm_id
                );
            }
        } else {
            debug!(
                "🔬 [MESA] Skipping microstructure: BVA primary window (base_mint={} pool={})",
                self.base_mint, self.pool_amm_id
            );
        }

        // === QEDD Survival Probability ===
        // QEDD requires full MarketSignals which needs extensive data we don't have
        // For now, use an enhanced heuristic based on volume momentum, price stability, and transaction activity
        let (volume_momentum, price_stability, tx_momentum, price_delta) = if let Some(p) = prev {
            let delta_vol = current.cum_volume_sol - p.cum_volume_sol;
            let delta_time =
                (current.timestamp_ms.saturating_sub(p.timestamp_ms)).max(1) as f64 / 1000.0;
            let vol_mom = delta_vol / delta_time.max(0.1);

            let price_data_valid = current.price_state == PriceState::Valid
                && p.price_state == PriceState::Valid
                && p.price_sol_per_token.abs() > f64::EPSILON;
            let price_change = if price_data_valid {
                ((current.price_sol_per_token - p.price_sol_per_token) / p.price_sol_per_token)
                    .abs()
            } else {
                0.0
            };
            let price_stab = if price_data_valid {
                1.0 - price_change.min(1.0)
            } else {
                1.0
            };

            // Calculate transaction momentum (are transactions still coming in?)
            let delta_tx = current.tx_count.saturating_sub(p.tx_count) as f64;
            let tx_mom = delta_tx / delta_time.max(0.1); // Transactions per second

            let price_delta = if price_data_valid {
                (current.price_sol_per_token - p.price_sol_per_token) / p.price_sol_per_token
            } else {
                0.0
            };

            (vol_mom, price_stab, tx_mom, price_delta)
        } else {
            let vol_mom = current.cum_volume_sol / self.start_time.elapsed().as_secs_f64().max(0.1);
            let tx_mom = current.tx_count as f64 / self.start_time.elapsed().as_secs_f64().max(0.1);
            (vol_mom, 1.0, tx_mom, 0.0)
        };

        // Enhanced QEDD Heuristic with Consolidation Detection
        //
        // Key insight: Distinguish healthy consolidation from crash:
        // - Consolidation: Volume drops BUT price stable AND transactions continue
        // - Crash: Volume drops AND (price crashes OR transactions stop)

        let base_survival = 0.5;

        // Volume factor (0.0 to 0.3)
        // Negative volume momentum might indicate:
        // 1. Crash (if accompanied by price drop or tx stop)
        // 2. Consolidation (if price stable and txs continue)
        let volume_factor = if volume_momentum < 0.0 {
            // Negative volume momentum - check for consolidation
            let is_consolidation = price_stability > CONSOLIDATION_PRICE_STABILITY_THRESHOLD
                && tx_momentum >= CONSOLIDATION_TX_MOMENTUM_THRESHOLD;

            if is_consolidation {
                // Healthy consolidation: Reduce penalty
                // Give partial credit (0.1-0.15 instead of 0.0)
                0.15
            } else {
                // Real crash: Full penalty
                0.0
            }
        } else {
            // Positive volume momentum - normal calculation
            (volume_momentum / 20.0).min(0.3)
        };

        // Price stability factor (0.0 to 0.2)
        let stability_factor = price_stability * 0.2;

        // Transaction activity factor (0.0 to 0.15)
        // Bonus if transactions are still coming in (shows continued interest)
        let tx_activity_factor = if tx_momentum >= 1.0 {
            0.15 // Strong transaction activity
        } else if tx_momentum >= 0.5 {
            0.10 // Moderate transaction activity
        } else if tx_momentum >= 0.2 {
            0.05 // Light transaction activity
        } else {
            0.0 // No/minimal transaction activity (warning sign)
        };

        let survival_est =
            (base_survival + volume_factor + stability_factor + tx_activity_factor) as f32;

        // Ensure survival never goes below 0.2 (always some survival chance)
        // and cap at 0.95 (never 100% certain)
        input.qedd_survival_60s = Some(survival_est.clamp(0.2, 0.95));

        debug!(
            "⚡ [QEDD ENHANCED] base_mint={} pool={} vol_momentum={:.2} price_stab={:.2} tx_momentum={:.2} survival={:.3}",
            self.base_mint,
            self.pool_amm_id,
            volume_momentum,
            price_stability,
            tx_momentum,
            survival_est
        );

        // === CHAOS Engine Pump Probability (DIAGNOSTIC ONLY) ===
        // Shadow Ledger cannot influence scoring. Run CHAOS for observability only.
        let cir_gate = (self.last_cir_global.unwrap_or(0.0) * cir_scale) >= CIR_CHAOS_THRESHOLD;
        if !bva_active {
            if let Some(amm_pool) = Self::snapshot_to_amm_pool(current) {
                if chaos_inputs_valid(&current) && cir_gate {
                    let scenario = if price_delta <= CHAOS_SCENARIO_RUG_PRICE_DROP
                        && tx_momentum < CHAOS_SCENARIO_RUG_TX_MOMENTUM
                        && volume_momentum < 0.0
                    {
                        MarketScenario::RugPull
                    } else if price_delta >= CHAOS_SCENARIO_PRICE_UP
                        && volume_momentum >= CHAOS_SCENARIO_VOL_UP
                    {
                        MarketScenario::Bullish
                    } else if price_delta <= -CHAOS_SCENARIO_PRICE_DOWN
                        && volume_momentum <= CHAOS_SCENARIO_VOL_DOWN
                    {
                        MarketScenario::Bearish
                    } else if price_stability < CHAOS_SCENARIO_VOLATILE
                        && tx_momentum >= CHAOS_SCENARIO_TX_ACTIVE
                    {
                        MarketScenario::Chaotic
                    } else {
                        MarketScenario::Mixed
                    };

                    debug!(
                    "🎲 [CHAOS|diag] base_mint={} pool={} scenario={:?} reserve_sol={} reserve_token={} fee_bps={} price_state={:?} price_delta={:.4} vol_momentum={:.4} tx_momentum={:.4} price_stability={:.3} cir_global={:.3}",
                    self.base_mint,
                    self.pool_amm_id,
                    scenario,
                    amm_pool.reserve_a,
                    amm_pool.reserve_b,
                    amm_pool.fee_bps,
                    current.price_state,
                    price_delta,
                    volume_momentum,
                    tx_momentum,
                    price_stability,
                    self.last_cir_global.unwrap_or(0.0)
                );

                    if let Ok(chaos_result) = self.chaos_engine.run_simulation(&amm_pool, scenario)
                    {
                        debug!(
                        "🎲 [CHAOS|diag] base_mint={} pool={} scenario={:?} pump_prob={:.1}% crash_prob={:.1}%",
                        self.base_mint,
                        self.pool_amm_id,
                        scenario,
                        chaos_result.pump_probability,
                        chaos_result.crash_probability
                    );
                    } else {
                        warn!(
                            "⚠️ [CHAOS|diag] Simulation failed for base_mint={} pool={}",
                            self.base_mint, self.pool_amm_id
                        );
                    }
                } else if !cir_gate {
                    debug!(
                        "⏭️ [CHAOS|diag] Skipping simulation due to CIR gate (cir_global={:.3}) base_mint={} pool={}",
                        self.last_cir_global.unwrap_or(0.0),
                        self.base_mint,
                        self.pool_amm_id
                    );
                } else {
                    debug!(
                        "⏭️ [CHAOS|diag] Skipping simulation due to invalid price/reserves for base_mint={} pool={} price_state={:?} reserve_base={} reserve_quote={}",
                        self.base_mint,
                        self.pool_amm_id,
                        current.price_state,
                        current.reserve_base,
                        current.reserve_quote
                    );
                }
            } else {
                debug!(
                    "🎲 [CHAOS|diag] Skipping simulation: snapshot lacks stable reserves (base_mint={} pool={})",
                    self.base_mint,
                    self.pool_amm_id
                );
            }
        } else {
            debug!(
                "🎲 [CHAOS|diag] Skipping simulation: BVA primary window (base_mint={} pool={})",
                self.base_mint, self.pool_amm_id
            );
        }

        // === SCR Bot Score (HyperOracle) ===
        if let Some(ref engine) = self.snapshot_engine {
            let txs = Self::decision_axis_transactions(engine.get_transactions(&self.pool_amm_id));
            if txs.len() >= 4 {
                let mut timestamps: Vec<u64> = txs.iter().map(|tx| tx.timestamp_ms).collect();
                timestamps.sort_unstable();
                let hyper = HyperOracle::new();
                let scr_bot_score = hyper.calculate_scr(&timestamps);
                let anomaly = self.market_anomaly_state.snapshot();
                let scr_bot_score = Self::adjust_scr_bot_score(
                    scr_bot_score,
                    anomaly,
                    &self.panic_config,
                    entropy_inconsistency,
                );
                input.scr_bot_score = Some(scr_bot_score);
                debug!(
                    "🧲 [SCR] base_mint={} pool={} tx_count={} scr_bot_score={:.3} fee_spike_hint={:.3} failed_ratio_hint={:.3} entropy_inconsistency={}",
                    self.base_mint,
                    self.pool_amm_id,
                    txs.len(),
                    scr_bot_score,
                    anomaly.fee_spike.clamp(0.0, 1.0),
                    anomaly.failed_ratio.clamp(0.0, 1.0),
                    entropy_inconsistency
                );
            }
        }

        // === QMAN Momentum Proxy (buy/sell flow ratio) ===
        if let Some(ref engine) = self.snapshot_engine {
            let txs = engine.get_transactions(&self.pool_amm_id);
            if !txs.is_empty() {
                let mut buy_volume = 0.0f64;
                let mut sell_volume = 0.0f64;
                for tx in &txs {
                    if tx.is_buy {
                        buy_volume += tx.sol_amount;
                    } else {
                        sell_volume += tx.sol_amount;
                    }
                }
                let total = buy_volume + sell_volume;
                if total > 0.0 {
                    let flow = (buy_volume - sell_volume) / total;
                    input.qman_score = Some(((flow + 1.0) * 0.5).clamp(0.0, 1.0) as f32);
                    debug!(
                        "🧭 [QMAN|proxy] base_mint={} pool={} tx_count={} buy_vol={:.4} sell_vol={:.4} flow={:.3} qman_score={:.3}",
                        self.base_mint,
                        self.pool_amm_id,
                        txs.len(),
                        buy_volume,
                        sell_volume,
                        flow,
                        input.qman_score.unwrap_or(0.0)
                    );
                }
            }
        }

        // === Quality Metrics ===
        // Unique wallet ratio from snapshot data
        let wallet_ratio = if current.tx_count > 0 {
            (current.unique_addrs as f64 / current.tx_count as f64).min(1.0)
        } else {
            0.5
        };
        input.unique_wallet_ratio = Some(wallet_ratio as f32);

        // === Risk Signals ===
        // Price crash detection: check if price dropped >30% from previous
        input.price_crash_detected = if let Some(p) = prev {
            let price_change =
                (current.price_sol_per_token - p.price_sol_per_token) / p.price_sol_per_token;
            price_change < -0.3
        } else {
            false
        };

        // QMAN exit signal: detect if large holders are selling (volume spike with price drop)
        input.qman_exit_signal = if let Some(p) = prev {
            let volume_spike =
                (current.cum_volume_sol - p.cum_volume_sol) > (p.cum_volume_sol * 0.5);
            let price_drop = current.price_sol_per_token < p.price_sol_per_token * 0.95;
            volume_spike && price_drop
        } else {
            false
        };

        input.paradox_anomaly = false; // ParadoxSensor integration pending

        // === IWIM and ClusterHunter ===
        // These are async and not available during cycles - will be None
        // SurvivorScore will use default trust (1.0) during cycles
        input.iwim_threat_score = None;
        input.cluster_risk_score = None;

        // === LIGMA ===
        // Default values until full LIGMA integration
        input.ligma_tradability_score = Some(DEFAULT_LIGMA_TRADABILITY);
        input.ligma_psi = Some(DEFAULT_LIGMA_PSI);
        input.ligma_liquidity_trap_risk = Some(DEFAULT_LIGMA_TRAP_RISK);

        if let Some(bva) = bva_output {
            let chaos = input.chaos_pump_prob.unwrap_or(0.0);
            let denom = (1.0 - chaos).max(0.05);
            let bva_score = (bva.score as f32 / denom).clamp(0.0, 1.0);
            input.bva_score = Some(bva_score);
        }

        // === Behavioral Signals ===
        if let Some(signal) = self.ecto_state.last_signal() {
            input.ecto_score = Some(signal.score as f32);
            input.ecto_verdict = Some(signal.verdict);
        }
        input.panic_pressure = Some((panic_output.pressure.min(3.0) / 3.0).clamp(0.0, 1.0) as f32);
        input.tcr_causality = panic_output.tcr_value.map(|v| v.clamp(0.0, 1.0) as f32);

        // === Context Metadata ===
        // Pass transaction count for dynamic threshold selection
        input.tx_count = Some(current.tx_count);
        input.age_secs = Some(self.start_time.elapsed().as_secs_f32());

        input
    }

    fn ensure_bva_state(
        &mut self,
        current_slot: Option<u64>,
        current_ts_ms: u64,
        transactions: &[TransactionRecord],
    ) {
        if self.bva_state.is_some() {
            return;
        }

        let birth_slot = transactions
            .first()
            .and_then(|t| {
                Self::normalize_slot_metadata(
                    t.slot,
                    "engine.ensure_bva_state",
                    self.base_mint,
                    self.pool_amm_id,
                )
            })
            .or_else(|| {
                Self::normalize_slot_metadata(
                    current_slot,
                    "engine.ensure_bva_state",
                    self.base_mint,
                    self.pool_amm_id,
                )
            });
        let birth_ts = transactions
            .first()
            .map(|t| t.timestamp_ms)
            .unwrap_or_else(|| {
                if current_ts_ms > 0 {
                    current_ts_ms
                } else {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    now.as_millis() as u64
                }
            });

        self.bva_state = Some(BvaState::new(birth_slot, birth_ts));
    }

    fn compute_bva_output(&mut self, current: &MarketSnapshot) -> Option<BvaOutput> {
        let now_ts = self.current_event_axis_ts_ms(current);

        let state = self.bva_state.as_ref()?;
        let output = self.bva_analyzer.analyze(state, now_ts);

        // Archive after primary window (<=7s) and close state
        if !self.bva_archived {
            let age_ms = now_ts.saturating_sub(state.birth_ts_ms);
            if age_ms
                >= self
                    .bva_analyzer
                    .config()
                    .primary_window_secs
                    .saturating_mul(1000)
            {
                let classification = match output.classification {
                    crate::oracle::bva::BvaClassification::Organic => {
                        LedgerBvaClassification::Organic
                    }
                    crate::oracle::bva::BvaClassification::Steered => {
                        LedgerBvaClassification::Steered
                    }
                    crate::oracle::bva::BvaClassification::Chaotic => {
                        LedgerBvaClassification::Chaotic
                    }
                    crate::oracle::bva::BvaClassification::Dormant => {
                        LedgerBvaClassification::Dormant
                    }
                };
                let archive = ghost_core::shadow_ledger::BvaArchive {
                    birth_slot: state.birth_slot,
                    birth_ts_ms: state.birth_ts_ms,
                    last_update_slot: state.last_update_slot,
                    last_update_ts_ms: state.last_update_ts_ms,
                    tx_count_total: state.tx_count_total as u64,
                    unique_signers: state.unique_signers.len() as u64,
                    score: output.score,
                    confidence: output.confidence,
                    classification,
                    metrics: ghost_core::shadow_ledger::BvaMetrics {
                        tds: output.metrics.tds,
                        dc: output.metrics.dc,
                        se: output.metrics.se,
                        cer: output.metrics.cer,
                        erp: output.metrics.erp,
                    },
                };
                self.shadow_ledger.set_bva_archive(self.base_mint, archive);
                self.bva_archived = true;
            }
        }

        Some(output)
    }

    /// Estimate congestion using localized slot vs timestamp delta
    fn estimate_congestion(&self, current: &MarketSnapshot, prev: Option<&MarketSnapshot>) -> bool {
        if let Some(p) = prev {
            let time_delta_ms = current.timestamp_ms.saturating_sub(p.timestamp_ms);
            // If time delta > 2x expected cycle duration (approx 400ms * 1.5 = 600ms), flag congestion
            return time_delta_ms > 600;
        }
        false
    }

    /// Checks if the session is in the "Early Window" phase.
    ///
    /// Defined by Event Time and Transaction Count:
    /// - Elapsed time < 10s (MIN_WINDOW_MS)
    /// - Total tx count < min_events
    ///
    /// Returns FALSE if we are STABLE.
    /// Latch: Once we leave Early Window, we never return.
    fn is_early_window(&mut self, now_ts_ms: u64, current_tx_count: u64) -> bool {
        if self.reached_stable_state {
            return false;
        }

        // Constants defined here for clarity, could be moved to config.
        // The hard floor is adapted to the session budget below.
        const MIN_WINDOW_MS: u64 = 1_500;

        let start_ts = match self.first_event_ts_ms {
            Some(ts) => ts,
            None => return true, // No events recorded yet
        };

        let elapsed = now_ts_ms.saturating_sub(start_ts);
        let cycle_ms = self.cycle_duration.as_millis() as u64;
        let session_budget_ms = cycle_ms.saturating_mul(self.max_cycles as u64);
        let dynamic_min_window_ms =
            MIN_WINDOW_MS.min(session_budget_ms.saturating_sub(cycle_ms).max(cycle_ms));

        const ENGINE_MIN_TX_COUNT: u64 = 5;
        let min_events = (self.sobp_min_emitted_tx_early as u64).max(ENGINE_MIN_TX_COUNT);

        let time_condition_met = elapsed >= dynamic_min_window_ms;
        let tx_condition_met = current_tx_count >= min_events;

        if time_condition_met && tx_condition_met {
            // We have reached stability. Latch it.
            self.reached_stable_state = true;
            return false;
        }

        true
    }

    /// Detect snapshot discontinuities (slot/tx/volume regression).
    fn detect_snapshot_discontinuity(
        &mut self,
        current: &MarketSnapshot,
        prev: Option<&MarketSnapshot>,
        live_now_ts_ms: u64,
        live_tx_count: u64,
    ) -> Option<VetoReason> {
        let prev = prev?;

        // EVENT-TIME FIX: Check Early Window first
        // We pass current.tx_count as the metric for event sufficiency
        let effective_now_ts_ms = if live_now_ts_ms > 0 {
            live_now_ts_ms
        } else {
            current.timestamp_ms
        };
        let effective_tx_count = if live_tx_count > 0 {
            live_tx_count
        } else {
            current.tx_count
        };
        if self.is_early_window(effective_now_ts_ms, effective_tx_count) {
            // If we are in Early Window, we check for discontinuity but DO NOT VETO.
            // We return None, but we should log if it's a regression for visibility.
            if let (Some(curr_slot), Some(prev_slot)) = (current.slot, prev.slot) {
                if curr_slot < prev_slot {
                    warn!("⚠️ [ENGINE] Early Window Discontinuity ignored (slot regression): cur={} prev={}", curr_slot, prev_slot);
                }
            }
            if current.tx_count < prev.tx_count {
                warn!("⚠️ [ENGINE] Early Window Discontinuity ignored (tx regression): cur={} prev={}", current.tx_count, prev.tx_count);
            }
            return None;
        }

        if let (Some(curr), Some(prev_slot)) = (current.slot, prev.slot) {
            if curr < prev_slot {
                warn!(
                    "⚠️ [ENGINE] Slot regression detected (metadata only): cur={} prev={}",
                    curr, prev_slot
                );
            }
        }

        if current.tx_count < prev.tx_count {
            return Some(VetoReason::SnapshotDiscontinuity);
        }

        if current.cum_volume_sol + VOLUME_COMPARISON_EPSILON < prev.cum_volume_sol {
            return Some(VetoReason::SnapshotDiscontinuity);
        }

        None
    }

    /// Centralized chaos gate that decides whether scoring can proceed,
    /// should be deferred, or must be blocked with a veto.
    fn chaos_gate(
        &mut self,
        current: &MarketSnapshot,
        prev: Option<&MarketSnapshot>,
        live_now_ts_ms: u64,
        live_tx_count: u64,
        discontinuity_current: Option<&MarketSnapshot>,
    ) -> GateAction {
        // Deprecated strict slot check. Use metadata if present, but relying on timestamp primarily.
        // We allow slot to be None (synthetic).
        // if current.slot.unwrap_or(0) == 0 { ... } -> Removed strict check

        if prev.is_none() {
            self.live_snapshots_seen = self.live_snapshots_seen.saturating_add(1);
        }

        let monotonic_ok = prev
            .map(|p| {
                current.tx_count >= p.tx_count
                    && current.cum_volume_sol + VOLUME_COMPARISON_EPSILON >= p.cum_volume_sol
            })
            .unwrap_or(false);

        if monotonic_ok {
            self.live_snapshots_seen = self.live_snapshots_seen.saturating_add(1);
        }
        if prev.is_some()
            && current.tx_count > 0
            && current.cum_volume_sol > 0.0
            && current.price_state == PriceState::Valid
        {
            self.live_snapshots_seen = self.live_snapshots_seen.max(WARMUP_LIVE_MIN);
        }

        if current.price_state == PriceState::Unknown {
            let reason = DeferReason::PriceUnknown;
            self.log_defer_with_metrics(&reason, current);
            return GateAction::Deferred(reason);
        }
        if self.live_snapshots_seen < WARMUP_LIVE_MIN {
            let reason = DeferReason::WarmupNotReady;
            self.log_defer_with_metrics(&reason, current);
            return GateAction::Deferred(reason);
        }

        if let Some(reason) = self.preflight_veto(current) {
            self.log_veto_with_metrics(&reason, current);
            return GateAction::Blocked(reason);
        }

        // Removed slot-based adaptive limit check.
        // TODO: Implement time-based CIR emission rate limiting if needed.

        let discontinuity_current = discontinuity_current.unwrap_or(current);
        if let Some(reason) = self.detect_snapshot_discontinuity(
            discontinuity_current,
            prev,
            live_now_ts_ms,
            live_tx_count,
        ) {
            self.log_veto_with_metrics(&reason, current);
            return GateAction::Blocked(reason);
        }

        GateAction::Allowed
    }

    fn preflight_veto(&self, snapshot: &MarketSnapshot) -> Option<VetoReason> {
        if snapshot.price_state == PriceState::Unknown {
            return None;
        }

        if !snapshot.price_sol_per_token.is_finite() {
            return Some(VetoReason::PriceInvalid);
        }

        if snapshot.price_state == PriceState::Invalid {
            return Some(VetoReason::PriceInvalid);
        }

        if snapshot.price_state == PriceState::Valid {
            debug_assert!(
                snapshot.price_sol_per_token > 0.0,
                "Valid price_state must have positive price"
            );
            if snapshot.price_sol_per_token <= 0.0 {
                return Some(VetoReason::PriceInvalid);
            }
        }
        if snapshot.reserve_base <= MIN_RESERVE_THRESHOLD
            || snapshot.reserve_quote <= MIN_RESERVE_THRESHOLD
        {
            if snapshot.price_state != PriceState::Valid
                || snapshot.tx_count == 0
                || snapshot.cum_volume_sol <= 0.0
            {
                return Some(VetoReason::ReservesTooLow);
            }
        }
        if snapshot.tx_count == 0 {
            return Some(VetoReason::InsufficientTx);
        }
        None
    }

    fn log_veto_context(&self, reason: &VetoReason, snapshot: &MarketSnapshot) {
        warn!(
            "💀 [ENGINE] VETO: reason={} base_mint={} pool={} cycle=S{} slot={:?} tx_count={} cum_volume={:.4} price_state={:?} reserve_base={:.6} reserve_quote={:.6}",
            reason.as_str(),
            self.base_mint,
            self.pool_amm_id,
            self.current_cycle,
            snapshot.slot,
            snapshot.tx_count,
            snapshot.cum_volume_sol,
            snapshot.price_state,
            snapshot.reserve_base,
            snapshot.reserve_quote
        );
    }

    fn log_veto_with_metrics(&self, reason: &VetoReason, snapshot: &MarketSnapshot) {
        self.log_veto_context(reason, snapshot);
        increment_counter!("survivor_veto_total", "reason" => reason.as_str());
    }

    fn log_defer_context(&self, reason: &DeferReason, snapshot: &MarketSnapshot) {
        debug!(
            "⏸️ [ENGINE] DEFER: reason={} base_mint={} pool={} cycle=S{} slot={:?} tx_count={} cum_volume={:.4} price_state={:?} reserve_base={:.6} reserve_quote={:.6}",
            reason.as_str(),
            self.base_mint,
            self.pool_amm_id,
            self.current_cycle,
            snapshot.slot,
            snapshot.tx_count,
            snapshot.cum_volume_sol,
            snapshot.price_state,
            snapshot.reserve_base,
            snapshot.reserve_quote
        );
    }

    fn log_defer_with_metrics(&self, reason: &DeferReason, snapshot: &MarketSnapshot) {
        self.log_defer_context(reason, snapshot);
        increment_counter!("cycle_deferred_total", "reason" => reason.as_str());
    }

    /// Evaluate a single cycle
    ///
    /// This is where the core scoring logic is integrated.
    /// It fetches snapshots from SnapshotEngine, calculates SOBP, MPCF, and safety,
    /// integrates TCF for trend verification, then applies the gunshot mechanism for early exit.
    ///
    /// # Returns
    ///
    /// `CycleResult` with score and action
    async fn evaluate_cycle(&mut self) -> CycleResult {
        // 1. Fetch snapshots from SnapshotEngine (sole source of truth for scoring)
        let default_iwim_source = if !self.iwim_enabled {
            IwimSource::Disabled
        } else if self.current_cycle < 8 {
            IwimSource::NotFinalPhase
        } else {
            IwimSource::NoProvider
        };
        let mut used_snapshot_engine = false;
        let mut snapshot_list: Vec<MarketSnapshot> = Vec::new();

        if let Some(ref engine) = self.snapshot_engine {
            let mut engine_snaps = engine.last_n(&self.pool_amm_id, CYCLE_SNAPSHOT_LOOKBACK);
            if !engine_snaps.is_empty() {
                engine_snaps.reverse();
                snapshot_list = engine_snaps
                    .into_iter()
                    .map(|snap| snap.to_ghost_core_snapshot())
                    .collect();
                used_snapshot_engine = true;
            }
        }

        if snapshot_list.is_empty() {
            if let Some(ledger_snaps) = self.shadow_ledger.get_snapshots(&self.base_mint) {
                if !ledger_snaps.is_empty() {
                    snapshot_list = ledger_snaps;
                    snapshot_list.sort_by_key(|snap| snap.timestamp_ms);
                    used_snapshot_engine = false;
                }
            }
        }

        if snapshot_list.is_empty() {
            debug!(
                "⏸️ [ENGINE] ODPULAM: reason=snapshot_unavailable base_mint={} pool={} cycle=S{}",
                self.base_mint, self.pool_amm_id, self.current_cycle
            );
            return CycleResult {
                cycle_id: self.current_cycle,
                score: 0.0,
                raw_score: None,
                iwim_applied: false,
                iwim_source: default_iwim_source,
                iwim_threat_score: None,
                veto_reason: None,
                defer_reason: Some(DeferReason::SnapshotUnavailable),
                is_gunshot: false,
                action: EngineAction::Continue,
            };
        }

        let snapshot_count_raw = snapshot_list.len();
        let current_snapshot_raw = snapshot_list.last().unwrap().clone();

        let mut current_snapshot = current_snapshot_raw.clone();
        let mut cycle_now_event_ts_ms = self
            .event_ts_source
            .decision_event_ts_ms(current_snapshot.timestamp_ms)
            .unwrap_or(0);
        let mut current_tx = current_snapshot.tx_count;
        let mut current_vol = current_snapshot.cum_volume_sol;
        let mut tx_buffer_len = 0usize;
        let mut unique_tx_in_window = 0usize;
        let mut snapshot_count = snapshot_count_raw;
        let mut event_time_source = "snapshot_fallback";
        let mut cycle_event_ts_source = self.event_ts_source;

        if let Some(ref engine) = self.snapshot_engine {
            if let Some(live) = engine.get_live_counters(self.pool_amm_id) {
                cycle_now_event_ts_ms = live
                    .event_ts_source
                    .decision_event_ts_ms(live.now_ts_ms)
                    .unwrap_or(0);
                current_tx = live.cum_tx_count;
                current_vol = live.cum_volume_sol;
                current_snapshot.timestamp_ms = cycle_now_event_ts_ms;
                current_snapshot.tx_count = current_tx;
                current_snapshot.cum_volume_sol = current_vol;
                tx_buffer_len = live.tx_buffer_len;
                unique_tx_in_window = engine.get_transactions(&self.pool_amm_id).len();
                snapshot_count = live.snapshot_count;
                event_time_source = "live";
                cycle_event_ts_source = live.event_ts_source;
            } else {
                tx_buffer_len = engine.get_transactions(&self.pool_amm_id).len();
                unique_tx_in_window = tx_buffer_len;
            }
        }

        let prev_eval_ts_ms = self.last_eval_current_ts_ms;
        let prev_eval_tx_count = self.last_eval_current_tx_count;
        let data_moved =
            (current_tx > prev_eval_tx_count) || (cycle_now_event_ts_ms != prev_eval_ts_ms);

        let mut start_event_ts_ms = cycle_now_event_ts_ms;
        let end_event_ts_ms = cycle_now_event_ts_ms;
        if start_event_ts_ms > end_event_ts_ms {
            start_event_ts_ms = end_event_ts_ms;
        }

        self.last_eval_current_ts_ms = cycle_now_event_ts_ms;
        self.last_eval_current_tx_count = current_tx;
        self.last_eval_current_volume_sol = current_vol;
        self.last_eval_snapshot_count = snapshot_count;
        self.last_eval_tx_buffer_len = tx_buffer_len;
        self.last_eval_unique_tx_in_window = unique_tx_in_window;
        self.cycle_now_event_ts_ms = cycle_now_event_ts_ms;
        self.start_event_ts_ms = start_event_ts_ms;
        self.end_event_ts_ms = end_event_ts_ms;
        self.event_ts_source = cycle_event_ts_source;
        self.txs_in_cycle_window_count = 0;
        self.cir_input_event_ts_min_ms = None;
        self.cir_input_event_ts_max_ms = None;
        self.sobp_input_event_ts_min_ms = None;
        self.sobp_input_event_ts_max_ms = None;
        self.tx_buffer_at_capacity = tx_buffer_len >= 128;
        self.retention_span_ms_effective = 0;
        self.cycle_window_span_ms = 0;

        if let Some(ref engine) = self.snapshot_engine {
            let all_txs =
                Self::decision_axis_transactions(engine.get_transactions(&self.pool_amm_id));
            let retention_span_ms_effective = if all_txs.is_empty() {
                0
            } else {
                let min_ts = all_txs.iter().map(|tx| tx.timestamp_ms).min().unwrap_or(0);
                let max_ts = all_txs.iter().map(|tx| tx.timestamp_ms).max().unwrap_or(0);
                max_ts.saturating_sub(min_ts)
            };
            self.retention_span_ms_effective = retention_span_ms_effective;
            self.cycle_window_span_ms = self
                .analysis_window_ms_config
                .min(retention_span_ms_effective);
            start_event_ts_ms = end_event_ts_ms.saturating_sub(self.cycle_window_span_ms);
            self.start_event_ts_ms = start_event_ts_ms;
            self.end_event_ts_ms = end_event_ts_ms;

            let txs_in_cycle_window =
                Self::decision_window_transactions(all_txs, start_event_ts_ms, end_event_ts_ms);
            self.txs_in_cycle_window_count = txs_in_cycle_window.len();
            unique_tx_in_window = self.txs_in_cycle_window_count;
            self.last_eval_unique_tx_in_window = self.txs_in_cycle_window_count;
        }

        let has_decision_event_clock =
            Self::has_decision_event_clock(cycle_event_ts_source, cycle_now_event_ts_ms);
        self.cycle_window_ready =
            has_decision_event_clock && !self.is_early_window(cycle_now_event_ts_ms, current_tx);
        let cycle_ms = self.cycle_duration.as_millis() as u64;
        let session_budget_ms = cycle_ms.saturating_mul(self.max_cycles as u64);
        let cycle_ready_min_window_ms =
            1_500u64.min(session_budget_ms.saturating_sub(cycle_ms).max(cycle_ms));
        let cycle_ready_elapsed_ms = self
            .first_event_ts_ms
            .map(|start_ts| cycle_now_event_ts_ms.saturating_sub(start_ts))
            .unwrap_or(0);
        let min_events = (self.sobp_min_emitted_tx_early as u64).max(5);

        // Event-time aligned snapshots for cycle delta:
        // end = latest snapshot at or before cycle end; baseline = latest snapshot before cycle start.
        let snapshot_end = snapshot_list
            .iter()
            .rev()
            .find(|snap| snap.timestamp_ms <= end_event_ts_ms)
            .cloned()
            .unwrap_or_else(|| current_snapshot_raw.clone());
        let snapshot_baseline = snapshot_list
            .iter()
            .rev()
            .find(|snap| snap.timestamp_ms < start_event_ts_ms)
            .cloned()
            .unwrap_or_else(|| snapshot_end.clone());

        let current_slot = current_snapshot.slot;
        if current_slot.is_none() {
            let snapshot_slot_zero = current_snapshot_raw.slot == Some(0);
            let tx_slot_zero = self
                .snapshot_engine
                .as_ref()
                .map(|engine| {
                    engine
                        .get_transactions(&self.pool_amm_id)
                        .iter()
                        .any(|tx| tx.slot == Some(0))
                })
                .unwrap_or(false);
            if snapshot_slot_zero || tx_slot_zero {
                increment_counter!("slot_contract_violation_total", "origin" => "engine.evaluate_cycle", "type" => "none_vs_zero_axis");
                warn!(
                    base_mint = %self.base_mint,
                    pool = %self.pool_amm_id,
                    snapshot_slot = ?current_snapshot_raw.slot,
                    tx_slot_zero = tx_slot_zero,
                    "SLOT_CONTRACT_VIOLATION: current_slot=None with zero slot observed in inputs"
                );
            }
        }

        // EVENT-TIME FIX: Initialize session start time on first valid event
        if self.first_event_ts_ms.is_none() && cycle_now_event_ts_ms > 0 {
            self.first_event_ts_ms = Some(cycle_now_event_ts_ms);
        }

        // Fetch previous snapshot (if available) for delta calculations
        let prev_snapshot = Some(snapshot_baseline.clone());
        let current_data = &snapshot_end;
        let prev_data = prev_snapshot.as_ref();

        match self.chaos_gate(
            current_data,
            prev_data,
            cycle_now_event_ts_ms,
            current_tx,
            Some(&current_snapshot_raw),
        ) {
            GateAction::Allowed => {}
            GateAction::Deferred(reason) => {
                return CycleResult {
                    cycle_id: self.current_cycle,
                    score: 0.0,
                    raw_score: None,
                    iwim_applied: false,
                    iwim_source: default_iwim_source,
                    iwim_threat_score: None,
                    veto_reason: None,
                    defer_reason: Some(reason),
                    is_gunshot: false,
                    action: EngineAction::Continue,
                };
            }
            GateAction::Blocked(reason) => {
                let action_reason = reason.to_string();
                return CycleResult {
                    cycle_id: self.current_cycle,
                    score: 0.0,
                    raw_score: None,
                    iwim_applied: false,
                    iwim_source: default_iwim_source,
                    iwim_threat_score: None,
                    veto_reason: Some(reason),
                    defer_reason: None,
                    is_gunshot: false,
                    action: EngineAction::Kill(action_reason),
                };
            }
        }

        // Enhanced diagnostic logging to verify snapshot flow
        debug!(
            "📊 [ENGINE|snapshot] Cycle S{} base_mint={} pool={} snapshot_count={} current_slot={:?} cycle_now_event_ts_ms={} start_event_ts_ms={} end_event_ts_ms={} cycle_window_ready={} cycle_ready_elapsed_ms={} cycle_ready_min_window_ms={} current_tx={} current_vol={:.4} tx_buffer_len={} txs_in_cycle_window_count={} source={} event_ts_source={} analysis_window_ms_config={} retention_span_ms_effective={} cycle_window_span_ms={} tx_buffer_at_capacity={} log_schema_version={}",
            self.current_cycle,
            self.base_mint,
            self.pool_amm_id,
            snapshot_count,
            current_slot,
            cycle_now_event_ts_ms,
            start_event_ts_ms,
            end_event_ts_ms,
            self.cycle_window_ready,
            cycle_ready_elapsed_ms,
            cycle_ready_min_window_ms,
            current_data.tx_count,
            current_data.cum_volume_sol,
            tx_buffer_len,
            self.txs_in_cycle_window_count,
            if used_snapshot_engine { event_time_source } else { "snapshot_fallback" },
            self.event_ts_source.as_str(),
            self.analysis_window_ms_config,
            self.retention_span_ms_effective,
            self.cycle_window_span_ms,
            self.tx_buffer_at_capacity,
            LOG_SCHEMA_VERSION
        );

        if !self.cycle_window_ready {
            self.cir_input_count = 0;
            self.cir_emitted_count = 0;
            self.cir_ic_only_count = 0;
            self.sobp_input_count = 0;
            self.sobp_event_count_internal = 0;
            self.sobp_bucket_fill_ratio = 0.0;
            self.last_no_emit_reason_code = Some(NoEmitReasonCode::SkippedDueToWindowContract);
            self.ready_false_reason = Some(
                if self.first_event_ts_ms.is_none() || self.txs_in_cycle_window_count == 0 {
                    ReadyFalseReason::BaselineMissing
                } else if current_tx < prev_eval_tx_count {
                    ReadyFalseReason::Discontinuity
                } else if cycle_ready_elapsed_ms < cycle_ready_min_window_ms
                    || current_tx < min_events
                {
                    ReadyFalseReason::InsufficientSpan
                } else {
                    ReadyFalseReason::SnapshotUnavailable
                },
            );

            debug!(
                "[CIR] skipped_due_to_window=true pool={} cycle=S{} tx_count={} reason_code={} ready_false_reason={} reason_code_version={} log_schema_version={}",
                self.pool_amm_id,
                self.current_cycle,
                self.txs_in_cycle_window_count,
                NoEmitReasonCode::SkippedDueToWindowContract.as_str(),
                self.ready_false_reason.map(|r| r.as_str()).unwrap_or("none"),
                self.reason_code_version,
                LOG_SCHEMA_VERSION
            );
            debug!(
                "SOBP skipped_due_to_window=true pool={} cycle=S{} elapsed_ms={} unique_tx_in_window={} slot={:?} reason_code={} ready_false_reason={} reason_code_version={} log_schema_version={}",
                self.pool_amm_id,
                self.current_cycle,
                cycle_ready_elapsed_ms,
                self.txs_in_cycle_window_count,
                current_slot,
                NoEmitReasonCode::SkippedDueToWindowContract.as_str(),
                self.ready_false_reason.map(|r| r.as_str()).unwrap_or("none"),
                self.reason_code_version,
                LOG_SCHEMA_VERSION
            );
            debug!(
                "📐 [ENGINE|event_window] cycle=S{} base_mint={} pool={} cycle_now_event_ts_ms={} start_event_ts_ms={} end_event_ts_ms={} cycle_window_ready={} txs_in_cycle_window_count={} sobp_input_event_ts_range={:?}-{:?} cir_input_event_ts_range={:?}-{:?} input_sorted={} tx_out_of_order_count_before={} tx_out_of_order_count_after={} cir_input_count={} cir_emitted_count={} cir_ic_only_count={} sobp_input_count={} sobp_event_count_internal={} sobp_bucket_fill_ratio={:.4} analysis_window_ms_config={} retention_span_ms_effective={} cycle_window_span_ms={} event_ts_source={} no_emit_reason_code={} ready_false_reason={} reason_code_version={} log_schema_version={}",
                self.current_cycle,
                self.base_mint,
                self.pool_amm_id,
                self.cycle_now_event_ts_ms,
                self.start_event_ts_ms,
                self.end_event_ts_ms,
                self.cycle_window_ready,
                self.txs_in_cycle_window_count,
                self.sobp_input_event_ts_min_ms,
                self.sobp_input_event_ts_max_ms,
                self.cir_input_event_ts_min_ms,
                self.cir_input_event_ts_max_ms,
                self.input_sorted,
                self.tx_out_of_order_count_before,
                self.tx_out_of_order_count_after,
                self.cir_input_count,
                self.cir_emitted_count,
                self.cir_ic_only_count,
                self.sobp_input_count,
                self.sobp_event_count_internal,
                self.sobp_bucket_fill_ratio,
                self.analysis_window_ms_config,
                self.retention_span_ms_effective,
                self.cycle_window_span_ms,
                self.event_ts_source.as_str(),
                self.last_no_emit_reason_code
                    .map(|reason| reason.as_str())
                    .unwrap_or("NONE"),
                self.ready_false_reason.map(|r| r.as_str()).unwrap_or("none"),
                self.reason_code_version,
                LOG_SCHEMA_VERSION
            );
            return CycleResult {
                cycle_id: self.current_cycle,
                score: 0.0,
                raw_score: None,
                iwim_applied: false,
                iwim_source: default_iwim_source,
                iwim_threat_score: None,
                veto_reason: None,
                defer_reason: Some(DeferReason::WarmupNotReady),
                is_gunshot: false,
                action: EngineAction::Continue,
            };
        }

        let age_secs = self.start_time.elapsed().as_secs_f64();
        let bva_active = age_secs <= self.bva_analyzer.config().primary_window_secs as f64;

        self.refresh_behavioral_config();

        let mut panic_output = self.update_panic_state();
        let signer_entropy_ratio = self.signer_entropy_state.entropy_ratio();
        let entropy_inconsistency = panic_output.entropy_score
            >= self.panic_config.entropy_threshold
            && signer_entropy_ratio < self.panic_config.entropy_threshold;
        if entropy_inconsistency {
            warn!(
                "⚠️ [PANIC] entropy inconsistency: base_mint={} pool={} panic_entropy={:.3} signer_entropy={:.3} confidence={:.3}",
                self.base_mint,
                self.pool_amm_id,
                panic_output.entropy_score,
                signer_entropy_ratio,
                panic_output.confidence
            );
            panic_output.confidence = panic_output
                .confidence
                .min(self.panic_config.entropy_inconsistency_confidence_cap);
        }

        // 2. Build SurvivorScoreInput from market data
        let current_event_ts_ms = self.current_event_axis_ts_ms(current_data);
        self.ensure_bva_state(current_data.slot, current_event_ts_ms, &[]);
        let raw_bva_output = self.compute_bva_output(current_data);
        if let Some(bva) = raw_bva_output {
            if bva.confidence == 0.0 {
                increment_counter!("bva_conf0_ignored_total");
                warn!(
                    "BVA_IGNORED_CONF0: base_mint={} pool={} cycle=S{} score={:.3} confidence={:.3} class={:?}",
                    self.base_mint,
                    self.pool_amm_id,
                    self.current_cycle,
                    bva.score,
                    bva.confidence,
                    bva.classification
                );
            }
        }
        let bva_output = raw_bva_output.filter(|bva| bva.confidence > 0.0);

        let mut survivor_input = self.build_survivor_input(
            current_data,
            prev_data,
            bva_active,
            bva_output.as_ref(),
            &panic_output,
            entropy_inconsistency,
        );
        debug!(
            "📐 [ENGINE|event_window] cycle=S{} base_mint={} pool={} cycle_now_event_ts_ms={} start_event_ts_ms={} end_event_ts_ms={} cycle_window_ready={} txs_in_cycle_window_count={} sobp_input_event_ts_range={:?}-{:?} cir_input_event_ts_range={:?}-{:?} input_sorted={} tx_out_of_order_count_before={} tx_out_of_order_count_after={} cir_input_count={} cir_emitted_count={} cir_ic_only_count={} sobp_input_count={} sobp_event_count_internal={} sobp_bucket_fill_ratio={:.4} analysis_window_ms_config={} retention_span_ms_effective={} cycle_window_span_ms={} event_ts_source={} no_emit_reason_code={} ready_false_reason={} reason_code_version={} log_schema_version={}",
            self.current_cycle,
            self.base_mint,
            self.pool_amm_id,
            self.cycle_now_event_ts_ms,
            self.start_event_ts_ms,
            self.end_event_ts_ms,
            self.cycle_window_ready,
            self.txs_in_cycle_window_count,
            self.sobp_input_event_ts_min_ms,
            self.sobp_input_event_ts_max_ms,
            self.cir_input_event_ts_min_ms,
            self.cir_input_event_ts_max_ms,
            self.input_sorted,
            self.tx_out_of_order_count_before,
            self.tx_out_of_order_count_after,
            self.cir_input_count,
            self.cir_emitted_count,
            self.cir_ic_only_count,
            self.sobp_input_count,
            self.sobp_event_count_internal,
            self.sobp_bucket_fill_ratio,
            self.analysis_window_ms_config,
            self.retention_span_ms_effective,
            self.cycle_window_span_ms,
            self.event_ts_source.as_str(),
            self.last_no_emit_reason_code
                .map(|reason| reason.as_str())
                .unwrap_or("NONE"),
            self.ready_false_reason.map(|r| r.as_str()).unwrap_or("none"),
            self.reason_code_version,
            LOG_SCHEMA_VERSION
        );
        let (session_stage, effective_threshold, stage_source, tx_stage) = self
            .survivor_calculator
            .resolve_stage_and_threshold(&survivor_input);
        if self.last_survivor_stage != Some(session_stage) {
            info!(
                "🎚️ [SURVIVOR_STAGE] base_mint={} pool={} cycle=S{} stage_change {} -> {} (source={} tx_stage={})",
                self.base_mint,
                self.pool_amm_id,
                self.current_cycle,
                self.last_survivor_stage
                    .map(|stage| stage.as_str())
                    .unwrap_or("N/A"),
                session_stage.as_str(),
                stage_source,
                tx_stage.map(|stage| stage.as_str()).unwrap_or("N/A")
            );
            self.last_survivor_stage = Some(session_stage);
        }

        // 2.2 Use BVA as early prior for SCR and confidence weight for QMAN
        if let Some(ref bva) = bva_output {
            if bva_active && bva.confidence < EARLY_BVA_CONFIDENCE_FLOOR {
                if !panic_output.is_bot_spam {
                    survivor_input.scr_bot_score = None;
                }
                survivor_input.qman_score = None;
            } else if bva_active && bva.confidence >= EARLY_BVA_CONFIDENCE_FLOOR {
                if let Some(scr) = survivor_input.scr_bot_score {
                    let prior = bva_scr_prior(bva.classification);
                    let weight = self.bva_analyzer.config().scr_prior_weight.clamp(0.0, 1.0) as f32;
                    survivor_input.scr_bot_score = Some(scr * (1.0 - weight) + prior * weight);
                } else {
                    survivor_input.scr_bot_score = Some(bva_scr_prior(bva.classification));
                }

                if let Some(qman) = survivor_input.qman_score {
                    survivor_input.qman_score =
                        Some((qman * bva.confidence as f32).clamp(0.0, 1.0));
                }
            }
        } else if bva_active {
            survivor_input.scr_bot_score = None;
            survivor_input.qman_score = None;
        }

        // IWIM integration state
        let mut iwim_applied = false;
        let mut iwim_source = if !self.iwim_enabled {
            IwimSource::Disabled
        } else if self.current_cycle < 2 {
            IwimSource::NotFinalPhase
        } else {
            IwimSource::NoProvider
        };
        let mut iwim_threat_score: Option<f32> = None;

        if self.iwim_enabled && self.current_cycle >= 2 {
            if let Some(provider) = &self.iwim_provider {
                if let Some(iwim) = provider.fetch_cached_iwim(self.pool_amm_id) {
                    iwim_threat_score = Some(iwim.rug_threat_score);
                    survivor_input.iwim_threat_score = iwim_threat_score;
                    iwim_applied = true;
                    iwim_source = IwimSource::ProviderHit;
                } else {
                    iwim_source = IwimSource::ProviderMiss;
                }
            }
        }

        let phase_label = if self.current_cycle >= 2 {
            "final"
        } else {
            "cycle"
        };
        match iwim_source {
            IwimSource::ProviderHit => {
                increment_counter!("iwim_lookup_total", "caller" => "engine", "phase" => phase_label, "result" => "hit", "reason" => "hit");
            }
            IwimSource::ProviderMiss => {
                increment_counter!("iwim_lookup_total", "caller" => "engine", "phase" => phase_label, "result" => "miss", "reason" => "cache_miss");
            }
            IwimSource::NoProvider => {
                increment_counter!("iwim_lookup_total", "caller" => "engine", "phase" => phase_label, "result" => "miss", "reason" => "no_provider");
            }
            IwimSource::NotFinalPhase => {
                increment_counter!("iwim_lookup_total", "caller" => "engine", "phase" => phase_label, "result" => "miss", "reason" => "not_final_phase");
            }
            IwimSource::Disabled => {
                increment_counter!("iwim_lookup_total", "caller" => "engine", "phase" => phase_label, "result" => "miss", "reason" => "disabled");
            }
        }

        // 3. Calculate SurvivorScore using the full module integration
        // IWIM model only when applied; otherwise use cycle version
        let survivor_result = if iwim_applied {
            self.survivor_calculator
                .calculate_with_iwim(&survivor_input)
        } else {
            self.survivor_calculator.calculate(&survivor_input)
        };
        debug!(
            "🎯 [ENGINE|survivor] base_mint={} pool={} cycle=S{} stage={} threshold={} source={} tx_count={:?} tx_stage={} score={} passed={}",
            self.base_mint,
            self.pool_amm_id,
            self.current_cycle,
            session_stage.as_str(),
            effective_threshold,
            stage_source,
            survivor_input.tx_count,
            tx_stage.map(|stage| stage.as_str()).unwrap_or("N/A"),
            survivor_result.score,
            survivor_result.passed
        );

        // Extract score (0-100 range)
        let mut base_score = survivor_result.score as f64;
        let bva_confident = bva_active
            && bva_output
                .as_ref()
                .map(|bva| bva.confidence >= EARLY_BVA_CONFIDENCE_FLOOR)
                .unwrap_or(false);
        let bva_ceiling = bva_output
            .as_ref()
            .map(|bva| (bva.score as f64 * 100.0).clamp(0.0, 100.0));

        if panic_output.is_bot_spam {
            base_score = base_score.min(0.3);
        }
        if panic_output.is_high_pressure {
            base_score = (base_score * 1.25).min(100.0);
        }

        debug!(
            "🧨 [PANIC] base_mint={} pool={} score={:.3} confidence={:.3} pressure={:.3} friction={:.3} fee_spike={:.3} impulse={:.3} entropy={:.3} high_pressure={} bot_spam={}",
            self.base_mint,
            self.pool_amm_id,
            panic_output.score,
            panic_output.confidence,
            panic_output.pressure,
            panic_output.friction,
            panic_output.fee_spike,
            panic_output.impulse_score,
            panic_output.entropy_score,
            panic_output.is_high_pressure,
            panic_output.is_bot_spam
        );

        if bva_active {
            if bva_confident {
                if let Some(ceiling) = bva_ceiling {
                    base_score = base_score.min(ceiling);
                }
            } else {
                base_score = base_score.min(EARLY_NO_BVA_SCORE_CAP);
            }
        }

        if let Some(bva) = raw_bva_output {
            debug!(
                "🧪 [BVA] base_mint={} pool={} active={} age_secs={:.2} score={:.3} confidence={:.3} class={:?} tds={:.3} dc={:.3} se={:.3} cer={:.3} erp={:.3}",
                self.base_mint,
                self.pool_amm_id,
                bva_active,
                age_secs,
                bva.score,
                bva.confidence,
                bva.classification,
                bva.metrics.tds,
                bva.metrics.dc,
                bva.metrics.se,
                bva.metrics.cer,
                bva.metrics.erp
            );
        }

        if let Some(reason) = &survivor_result.veto_reason {
            self.log_veto_context(reason, current_data);
            let action_reason = reason.to_string();
            return CycleResult {
                cycle_id: self.current_cycle,
                score: survivor_result.score as f64,
                raw_score: None,
                iwim_applied,
                iwim_source,
                iwim_threat_score,
                veto_reason: Some(reason.clone()),
                defer_reason: None,
                is_gunshot: false,
                action: EngineAction::Kill(action_reason),
            };
        }

        let sobp_log = survivor_input
            .sobp_momentum
            .map(|v| format!("{:.3}", v))
            .unwrap_or_else(|| "none".to_string());
        let cir_log = survivor_input
            .mpcf_organic_ratio
            .map(|v| format!("{:.3}", v))
            .unwrap_or_else(|| "none".to_string());
        debug!(
            "📊 [ENGINE] Cycle S{} scores: SOBP={} CIR={} Survival={:.3} Momentum={:.3} Quality={:.3} → Score={:.2}",
            self.current_cycle,
            sobp_log,
            cir_log,
            survivor_result.breakdown.survival,
            survivor_result.breakdown.momentum,
            survivor_result.breakdown.quality,
            base_score
        );

        // === 4. INTEGRACJA TCF (TU JEST KLUCZ!) ===
        let sobp_opt = survivor_input.sobp_momentum;
        let mpcf_opt = survivor_input.mpcf_organic_ratio;

        let (tcf_status_opt, mut final_score) = if bva_active && sobp_opt.is_none() {
            (None, base_score)
        } else {
            // Tworzymy obserwację dla TCF
            let sobp = sobp_opt.unwrap_or(0.0) as f64;
            let mpcf = mpcf_opt.unwrap_or(0.5) as f64;
            let volume_delta = (panic_output.pressure * 2.0 - 1.0).clamp(-1.0, 1.0);
            let liquidity_entropy = panic_output.entropy_score.clamp(0.0, 1.0);
            let jitter = panic_output.friction.clamp(0.0, 1.0);
            let phase_sync = panic_output.impulse_score.clamp(0.0, 1.0);

            let observation = MarketObservation::new(
                // Map SOBP [-2.0, 3.0] → price_delta ~[-0.8, 1.2]; sells stay within TCF bounds while buys keep headroom
                (sobp / SOBP_PRICE_DELTA_SCALE).clamp(SOBP_PRICE_DELTA_MIN, SOBP_PRICE_DELTA_MAX),
                volume_delta,
                liquidity_entropy,
                (mpcf * 2.0 - 1.0), // order_flow
                mpcf,
                jitter,
                phase_sync,
            );

            // KARMIMY TCF
            let tcf_status = self
                .tcf
                .lock()
                .update_with_progress(&observation, data_moved);

            // APLIKUJEMY KARY ZA DIVERGENCE
            let score = if let Some(cohesion) = tcf_status.cohesion {
                if cohesion < 0.6 {
                    // TCF MÓWI: TO JEST FEJK! Obniżamy score o 30%
                    base_score * 0.7
                } else {
                    // TCF MÓWI: LEGIT! Podbijamy o 5%
                    base_score * 1.05
                }
            } else {
                base_score // Brak danych TCF
            }
            .min(100.0);

            (Some(tcf_status), score)
        };

        if bva_active {
            if bva_confident {
                if let Some(ceiling) = bva_ceiling {
                    final_score = final_score.min(ceiling);
                }
            } else {
                final_score = final_score.min(EARLY_NO_BVA_SCORE_CAP);
            }
        }

        debug!(
            "🧭 [ENGINE|tcf] base_score={:.2} cohesion={} phase={:?} final_score={:.2} data_moved={} stale_input={} pump_candidate_without_gate={} pump_suppressed_reason={}",
            base_score,
            tcf_status_opt
                .as_ref()
                .and_then(|s| s.cohesion)
                .map(|c| format!("{:.3}", c))
                .unwrap_or_else(|| "none".into()),
            tcf_status_opt.as_ref().map(|s| s.phase),
            final_score,
            tcf_status_opt.as_ref().map(|s| s.data_moved).unwrap_or(data_moved),
            tcf_status_opt.as_ref().map(|s| s.stale_input).unwrap_or(!data_moved),
            tcf_status_opt
                .as_ref()
                .map(|s| s.pump_candidate_without_gate)
                .unwrap_or(false),
            tcf_status_opt
                .as_ref()
                .and_then(|s| s.pump_suppressed_reason)
                .unwrap_or("none"),
        );

        // Per-cycle TCF component diagnostics
        if let Some(ref tcf_status) = tcf_status_opt {
            if tcf_status.cohesion_computed_this_cycle {
                if let Some(ref cr) = tcf_status.last_cohesion_result {
                    debug!(
                        "[TCF|diag] pool={} cycle=S{} cohesion_computed_this_cycle=true \
                         price_volume_divergent={} direction_contradiction={} \
                         direction_score={:.4} rhythm_score={:.4} stability_score={:.4} \
                         total_penalty={:.4} total_bonus={:.4} cohesion_final={:.4}",
                        self.bonding_curve,
                        self.current_cycle,
                        cr.breakdown.price_volume_divergent,
                        cr.breakdown.direction_contradiction,
                        cr.direction_score,
                        cr.rhythm_score,
                        cr.stability_score,
                        cr.total_penalty,
                        cr.total_bonus,
                        cr.cohesion,
                    );
                }
            } else {
                debug!(
                    "[TCF|diag] pool={} cycle=S{} cohesion_computed_this_cycle=false \
                     price_volume_divergent=n/a direction_contradiction=n/a \
                     direction_score=n/a rhythm_score=n/a stability_score=n/a \
                     total_penalty=n/a total_bonus=n/a cohesion_final=n/a",
                    self.bonding_curve, self.current_cycle,
                );
            }
        }

        // === KONIEC INTEGRACJI TCF ===

        // 5. Gunshot Logic (na finalnym wyniku)
        let gunshot_threshold = match self.current_cycle {
            1 => 100.0,
            2 => 99.0,
            3 => 98.0,
            4 => 97.0,
            5 => 96.0,
            6 => 95.0,
            7 => 88.0,
            8 => 87.0,
            9 => 86.0,
            10 => 85.0,
            11 => 83.5,
            12 => 82.0,
            _ => 82.0, // Fallback for any cycle beyond 12
        };

        let is_gunshot = if bva_active {
            bva_confident && final_score >= gunshot_threshold
        } else {
            final_score >= gunshot_threshold
        };
        let action = if is_gunshot {
            EngineAction::Buy(final_score) // amount = score placeholder
        } else {
            EngineAction::Continue
        };

        CycleResult {
            cycle_id: self.current_cycle,
            score: final_score,
            raw_score: Some(base_score),
            iwim_applied,
            iwim_source,
            iwim_threat_score,
            veto_reason: None,
            defer_reason: None,
            is_gunshot,
            action,
        }
    }

    /// Calculate final verdict after S12
    ///
    /// This uses weighted geometric mean with exponential weighting
    /// to give more weight to later cycles (especially S10-S12).
    ///
    /// # Returns
    ///
    /// Final `EngineAction` (Buy or Kill)
    async fn final_verdict(&self) -> EngineAction {
        info!(
            "⚖️ [ENGINE] Calculating Final SurvivorScore for base_mint={} pool={}",
            self.base_mint, self.pool_amm_id
        );

        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;

        // Iterujemy po historii, aplikując wagi czasowe
        for res in &self.history {
            let w = self.get_cycle_weight(res.cycle_id);
            let cycle_score = res.raw_score.unwrap_or(res.score);
            weighted_sum += cycle_score * w;
            weight_total += w;
        }

        let survivor_score = if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            0.0
        };

        let tx_count = self.last_eval_current_tx_count;
        let session_stage = self.session_stage_for_final_verdict();
        let threshold_input = SurvivorScoreInput {
            session_stage: Some(session_stage),
            tx_count: Some(tx_count),
            ..Default::default()
        };
        let (threshold_stage, threshold, threshold_source, tx_stage) = self
            .survivor_calculator
            .resolve_stage_and_threshold(&threshold_input);

        info!(
            "⚖️ RESULT: SurvivorScore={:.2} (Threshold: {} | stage={} source={} | tx_count={} tx_stage={}) | Cycles={}",
            survivor_score,
            threshold,
            threshold_stage.as_str(),
            threshold_source,
            tx_count,
            tx_stage.map(|stage| stage.as_str()).unwrap_or("N/A"),
            self.history.len()
        );

        if survivor_score >= threshold as f64 {
            EngineAction::Buy(survivor_score)
        } else {
            EngineAction::Kill(format!("SurvivorScore too low: {:.2}", survivor_score))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::snapshot_engine::{
        DataSource, InitPoolEvent, MarketSnapshot as EngineSnapshot, PoolLifecycle, PoolMetrics,
        SnapshotEngine, TxEvent,
    };
    use seer::types::RawBytesMissingReason;
    use tokio::time::Instant as TokioInstant;
    // Timestamp used to verify slot-based indexing over timestamp-based indexing.
    const DIVERGENT_TIMESTAMP_FOR_SLOT_TEST: u64 = 99_999;

    fn build_snapshot(slot: Option<u64>, tx_count: u64, cum_volume: f64) -> MarketSnapshot {
        MarketSnapshot {
            slot,
            tx_key: None,
            timestamp_ms: slot.unwrap_or(0),
            cum_volume_sol: cum_volume,
            tx_count,
            unique_addrs: tx_count.max(1),
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            price_reason: None,
            market_cap_sol: 0.0,
            reserve_base: 1.0,
            reserve_quote: 1.0,
            bonding_progress_pct: 0.0,
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        }
    }

    fn build_session() -> PredictionSession {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        session.reached_stable_state = true;
        session
    }

    fn build_tx_record(timestamp_ms: u64, event_ts_source: EventTsSource) -> TransactionRecord {
        TransactionRecord {
            slot: Some(1),
            signature: format!("tx-{timestamp_ms}-{}", event_ts_source.as_str()),
            signer: Pubkey::new_unique(),
            sol_amount: 1.0,
            is_buy: true,
            is_dev_buy: false,
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            event_ts_source,
            seq_no: timestamp_ms,
            raw_bytes: None,
            raw_bytes_missing_reason: RawBytesMissingReason::Unknown,
            price_quote: None,
        }
    }

    fn build_active_snapshot_engine(
        base_mint: Pubkey,
        pool_id: Pubkey,
        start_ts: u64,
    ) -> Arc<SnapshotEngine> {
        let snapshot_engine = Arc::new(SnapshotEngine::new(256, 1000));
        snapshot_engine.mark_pool_active(pool_id);
        let init_event = InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(1),
            timestamp_ms: start_ts,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        };
        snapshot_engine.handle_initialize_pool_event(&init_event);
        snapshot_engine
    }

    #[test]
    fn test_sobp_normalization_boundaries() {
        assert_eq!(normalize_sobp(0.5), 0.0);
        assert_eq!(normalize_sobp(0.0), -2.0);
        assert_eq!(normalize_sobp(1.0), 3.0);
    }

    #[test]
    fn test_sobp_signal_amplification() {
        let result = normalize_sobp(0.7);
        assert!(
            result > 1.0 && result < 1.5,
            "Expected strong buying signal, got {}",
            result
        );

        let result = normalize_sobp(0.9);
        assert!(
            result > 2.0 && result < 3.0,
            "Expected extreme buying signal, got {}",
            result
        );
    }

    #[test]
    fn test_snapshot_discontinuity_monotonic_ok() {
        let mut session = build_session();
        let prev = build_snapshot(Some(10), 5, 2.0);
        let current = build_snapshot(Some(11), 6, 3.0);

        assert_eq!(
            session.detect_snapshot_discontinuity(
                &current,
                Some(&prev),
                current.timestamp_ms,
                current.tx_count
            ),
            None
        );
    }

    #[test]
    fn test_snapshot_discontinuity_triggers_on_decrease() {
        let mut session = build_session();
        let prev = build_snapshot(Some(10), 5, 2.0);
        let current_tx_drop = build_snapshot(Some(11), 4, 3.0);
        let current_vol_drop = build_snapshot(Some(11), 6, 1.0);

        assert_eq!(
            session.detect_snapshot_discontinuity(
                &current_tx_drop,
                Some(&prev),
                current_tx_drop.timestamp_ms,
                current_tx_drop.tx_count
            ),
            Some(VetoReason::SnapshotDiscontinuity)
        );
        assert_eq!(
            session.detect_snapshot_discontinuity(
                &current_vol_drop,
                Some(&prev),
                current_vol_drop.timestamp_ms,
                current_vol_drop.tx_count
            ),
            Some(VetoReason::SnapshotDiscontinuity)
        );
    }

    #[test]
    fn test_window_metrics_do_not_affect_continuity() {
        let mut session = build_session();
        let prev_engine = EngineSnapshot {
            timestamp_ms: 1_000,
            slot: Some(10),
            cum_volume_sol: 5.0,
            tx_count: 5,
            cum_buy_volume_sol: 3.0,
            cum_sell_volume_sol: 2.0,
            window_tx_count: 10,
            window_volume_sol: 9.0,
            window_buy_volume_sol: 6.0,
            window_sell_volume_sol: 3.0,
            ..Default::default()
        };
        let current_engine = EngineSnapshot {
            timestamp_ms: 1_500,
            slot: Some(11),
            cum_volume_sol: 6.0,
            tx_count: 6,
            cum_buy_volume_sol: 3.5,
            cum_sell_volume_sol: 2.5,
            window_tx_count: 1,
            window_volume_sol: 0.5,
            window_buy_volume_sol: 0.4,
            window_sell_volume_sol: 0.1,
            ..Default::default()
        };

        let prev = prev_engine.to_ghost_core_snapshot();
        let current = current_engine.to_ghost_core_snapshot();

        assert_eq!(
            session.detect_snapshot_discontinuity(
                &current,
                Some(&prev),
                current.timestamp_ms,
                current.tx_count
            ),
            None
        );
    }

    #[test]
    fn test_continuity_check_not_triggered_by_snapshot_lag() {
        let mut session = build_session();
        session.reached_stable_state = false;
        session.first_event_ts_ms = Some(0);

        let prev = build_snapshot(Some(10), 10, 5.0);
        let current = build_snapshot(Some(10), 10, 5.0); // no new snapshot emitted yet

        let live_now_ts_ms = 20_000;
        let live_tx_count = 24;

        assert_eq!(
            session.detect_snapshot_discontinuity(
                &current,
                Some(&prev),
                live_now_ts_ms,
                live_tx_count
            ),
            None
        );
    }

    #[tokio::test]
    async fn test_engine_uses_live_counters_not_snapshot() {
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 1000));
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let quote_mint = Pubkey::new_unique();
        snapshot_engine.mark_pool_active(pool_id);

        let init_ts = 1_700_000_000_000u64;
        let init_event = InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            slot: Some(1),
            timestamp_ms: init_ts,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        };
        snapshot_engine.handle_initialize_pool_event(&init_event);

        for i in 0..24u64 {
            let tx = TxEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool_id,
                base_mint,
                pool_state: PoolLifecycle::Active,
                metrics: PoolMetrics::default(),
                slot: Some(1),
                timestamp_ms: init_ts + (i * 5),
                event_time: ghost_core::EventTimeMetadata::default(),
                signer: Pubkey::new_unique(),
                is_buy: true,
                volume_sol: 1.0,
                reserve_base: Some(1_000.0),
                reserve_quote: Some(100.0),
                price_quote: Some(0.1),
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: None,
                event_ordinal: None,
                block_time: None,
                arrival_time_ms: None,
                data_source: DataSource::SoftTruth,
                intra_slot_offset_ms: None,
                raw_data: None,
                raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            };
            snapshot_engine.handle_tx_event(&tx);
        }

        let live = snapshot_engine
            .get_live_counters(pool_id)
            .expect("live counters should exist");
        assert_eq!(live.cum_tx_count, 24);

        let latest_snapshot = snapshot_engine
            .last_n(&pool_id, 1)
            .pop()
            .expect("snapshot should exist");
        assert!(
            latest_snapshot.tx_count < 24,
            "latest snapshot should lag behind live counters"
        );

        let ledger = Arc::new(ShadowLedger::new());
        let bonding_curve = Pubkey::new_unique();
        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.current_cycle = 1;
        let _ = session.evaluate_cycle().await;

        assert_eq!(session.last_eval_current_tx_count, 24);
        assert_eq!(session.last_eval_current_volume_sol, 24.0);
    }

    #[tokio::test]
    async fn test_p0_cycle_input_sorting_contract() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let start_ts = 1_700_000_000_000u64;
        let snapshot_engine = build_active_snapshot_engine(base_mint, pool_id, start_ts);

        let mut tx = |ts: u64, sig: &str| TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(1),
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(1_000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(sig.to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };

        // Intentionally out-of-order by event-time.
        snapshot_engine.handle_tx_event(&tx(start_ts + 300, "sort-1"));
        snapshot_engine.handle_tx_event(&tx(start_ts + 100, "sort-2"));
        snapshot_engine.handle_tx_event(&tx(start_ts + 200, "sort-3"));

        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.start_event_ts_ms = start_ts;
        session.end_event_ts_ms = start_ts + 1_000;
        session.cycle_window_ready = true;

        let current_data = build_snapshot(Some(2), 3, 3.0);
        let _ = session.calculate_sobp(&current_data, false, false, &PanicOutput::default());
        assert!(session.input_sorted, "input sorting flag must be set");
        assert!(
            session.tx_out_of_order_count_before > 0,
            "test setup should produce out-of-order input"
        );
        assert_eq!(
            session.tx_out_of_order_count_after, 0,
            "sorted input to CIR/SOBP must be order-clean"
        );
    }

    #[tokio::test]
    async fn test_p1_cycle_window_contract_min_analysis_retention() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let start_ts = 1_700_100_000_000u64;
        let snapshot_engine = build_active_snapshot_engine(base_mint, pool_id, start_ts);

        for i in 0..140u64 {
            let tx = TxEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool_id,
                base_mint,
                pool_state: PoolLifecycle::Active,
                metrics: PoolMetrics::default(),
                slot: Some(1),
                timestamp_ms: start_ts + i,
                event_time: ghost_core::EventTimeMetadata::default(),
                signer: Pubkey::new_unique(),
                is_buy: true,
                volume_sol: 0.1,
                reserve_base: Some(1_000.0),
                reserve_quote: Some(100.0),
                price_quote: Some(0.1),
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: Some(format!("span-{i}")),
                event_ordinal: None,
                block_time: None,
                arrival_time_ms: None,
                data_source: DataSource::SoftTruth,
                intra_slot_offset_ms: None,
                raw_data: None,
                raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            };
            snapshot_engine.handle_tx_event(&tx);
        }

        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.current_cycle = 1;
        let _ = session.evaluate_cycle().await;

        assert!(
            session.tx_buffer_at_capacity,
            "buffer capacity flag must be true"
        );
        assert_eq!(
            session.cycle_window_span_ms,
            session
                .analysis_window_ms_config
                .min(session.retention_span_ms_effective)
        );
    }

    #[tokio::test]
    async fn test_p2_window_contract_drives_no_emit_reason() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let start_ts = 1_700_200_000_000u64;
        let snapshot_engine = build_active_snapshot_engine(base_mint, pool_id, start_ts);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(1),
            timestamp_ms: start_ts + 10,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 0.2,
            reserve_base: Some(1_000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("window-contract".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };
        snapshot_engine.handle_tx_event(&tx);

        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.start_event_ts_ms = start_ts;
        session.end_event_ts_ms = start_ts + 1_000;
        session.cycle_window_ready = false;

        let current_data = build_snapshot(Some(2), 1, 0.2);
        let sobp = session.calculate_sobp(&current_data, false, true, &PanicOutput::default());
        assert!(sobp.is_none(), "window contract should skip module output");
        assert_eq!(
            session.last_no_emit_reason_code.map(|r| r.as_str()),
            Some(NoEmitReasonCode::SkippedDueToWindowContract.as_str())
        );
    }

    #[tokio::test]
    async fn test_cycle_not_ready_skips_all_window_modules() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let start_ts = 1_700_250_000_000u64;
        let snapshot_engine = build_active_snapshot_engine(base_mint, pool_id, start_ts);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(1),
            timestamp_ms: start_ts + 10,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 0.2,
            reserve_base: Some(1_000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("not-ready-cycle".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };
        snapshot_engine.handle_tx_event(&tx);

        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.current_cycle = 1;

        let result = session.evaluate_cycle().await;
        assert!(!session.cycle_window_ready);
        assert_eq!(result.defer_reason, Some(DeferReason::WarmupNotReady));
        assert_eq!(session.cir_input_count, 0);
        assert_eq!(session.cir_emitted_count, 0);
        assert_eq!(session.sobp_input_count, 0);
        assert_eq!(
            session.last_no_emit_reason_code.map(|r| r.as_str()),
            Some(NoEmitReasonCode::SkippedDueToWindowContract.as_str())
        );
        assert_eq!(
            session.ready_false_reason.map(|r| r.as_str()),
            Some("insufficient_span")
        );
    }

    #[tokio::test]
    async fn test_p1_no_double_gating_when_cycle_window_ready_true() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let start_ts = 1_700_300_000_000u64;
        let snapshot_engine = build_active_snapshot_engine(base_mint, pool_id, start_ts);

        for i in 0..5u64 {
            let tx = TxEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool_id,
                base_mint,
                pool_state: PoolLifecycle::Active,
                metrics: PoolMetrics::default(),
                slot: Some(1),
                timestamp_ms: start_ts + (i * 100),
                event_time: ghost_core::EventTimeMetadata::default(),
                signer: Pubkey::new_unique(),
                is_buy: i % 2 == 0,
                volume_sol: 0.2,
                reserve_base: Some(1_000.0),
                reserve_quote: Some(100.0),
                price_quote: Some(0.1),
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: Some(format!("ready-{i}")),
                event_ordinal: None,
                block_time: None,
                arrival_time_ms: None,
                data_source: DataSource::SoftTruth,
                intra_slot_offset_ms: None,
                raw_data: None,
                raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            };
            snapshot_engine.handle_tx_event(&tx);
        }

        let mut session = PredictionSession::new(
            base_mint,
            pool_id,
            bonding_curve,
            ledger,
            Some(Arc::clone(&snapshot_engine)),
        );
        session.start_event_ts_ms = start_ts;
        session.end_event_ts_ms = start_ts + 1_000;
        session.cycle_window_ready = true;

        let current_data = build_snapshot(Some(2), 5, 1.0);
        let _ = session.calculate_sobp(&current_data, false, false, &PanicOutput::default());

        assert_ne!(
            session.last_no_emit_reason_code.map(|r| r.as_str()),
            Some(NoEmitReasonCode::SkippedDueToWindowContract.as_str()),
            "ready=true must not be classified as window-contract skip"
        );
    }

    #[test]
    fn test_reason_code_enum_contract_is_finite_and_versioned() {
        let allowed = [
            NoEmitReasonCode::OutOfOrderInput.as_str(),
            NoEmitReasonCode::WindowSpanInsufficient.as_str(),
            NoEmitReasonCode::ResponderGatesNotMet.as_str(),
            NoEmitReasonCode::ThetaNotReached.as_str(),
            NoEmitReasonCode::SkippedDueToWindowContract.as_str(),
            NoEmitReasonCode::SobpDataUnavailable.as_str(),
        ];
        assert_eq!(allowed.len(), 6);
        assert!(allowed
            .iter()
            .all(|v| v.chars().all(|c| c.is_ascii_uppercase() || c == '_')));
    }

    #[test]
    fn test_scoring_guards_unchanged() {
        // Merge-gate guard: no threshold/stage drift in this package.
        assert_eq!(EARLY_BVA_CONFIDENCE_FLOOR, 0.5);
        assert_eq!(EARLY_NO_BVA_SCORE_CAP, 30.0);
        assert_eq!(CIR_CHAOS_THRESHOLD, 0.8);
    }

    #[tokio::test]
    async fn test_session_initialization() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);

        assert_eq!(session.current_cycle, 0);
        assert_eq!(session.max_cycles, 12);
        assert_eq!(
            session.cycle_duration,
            Duration::from_millis(default_cycle_duration_ms())
        );
        assert_eq!(session.history.len(), 0);
    }

    #[tokio::test]
    async fn test_session_executes_all_cycles() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        let start = TokioInstant::now();

        let result = session.run().await;
        let elapsed = start.elapsed();

        // Should complete all 12 cycles
        assert_eq!(session.current_cycle, 12);
        assert_eq!(session.history.len(), 12);

        // Should take approximately 12 * default cycle duration (with broad CI tolerance)
        let expected_ms = (default_cycle_duration_ms() as u128) * 12;
        let elapsed_ms = elapsed.as_millis();
        assert!(elapsed_ms >= expected_ms.saturating_sub(expected_ms / 2));
        assert!(elapsed_ms <= expected_ms + expected_ms);

        // With new weighted scoring, should evaluate based on SurvivorScore threshold
        match result {
            EngineAction::Kill(reason) => {
                // Should contain "SurvivorScore" in the reason now
                assert!(reason.contains("SurvivorScore") || reason.contains("too low"));
            }
            EngineAction::Buy(_) => {
                // Or it might pass if scores are high enough
                // This is acceptable behavior
            }
            _ => panic!("Expected Kill or Buy action, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_snapshot_discontinuity_triggers_veto() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        // STABLE WINDOW SCENARIO:
        // Must have elapsed > 10s AND tx_count >= 5
        let start_ts = 1_000_000;
        let stable_ts = start_ts + 15_000; // +15s

        let mut s1 = build_snapshot(Some(10), 100, 50.0);
        s1.timestamp_ms = start_ts;

        // Regression: tx_count 100 -> 90.
        // 90 >= 5, so count condition for Early Window is FALSE.
        // 15s >= 10s, so time condition for Early Window is FALSE.
        // -> STABLE WINDOW.
        let mut s2 = build_snapshot(Some(11), 90, 60.0);
        s2.timestamp_ms = stable_ts;

        let snapshots = vec![s1, s2];
        ledger.commit_history(base_mint, snapshots, None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;
        session.current_cycle = 1;
        // Manually seed start time to simulate stable session
        session.first_event_ts_ms = Some(start_ts);

        let result = session.evaluate_cycle().await;

        assert!(
            matches!(result.action, EngineAction::Kill(_)),
            "Snapshot discontinuity should veto in Stable Window, got {:?}",
            result.action
        );
        assert_eq!(result.veto_reason, Some(VetoReason::SnapshotDiscontinuity));
    }

    #[tokio::test]
    async fn test_final_verdict_uses_engine_stage_not_tx_fallback() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);

        // Force final-session context with low absolute tx_count that would imply EARLY in fallback.
        session.current_cycle = 12;
        session.last_eval_current_tx_count = 10;
        session.history.push(CycleResult {
            cycle_id: 12,
            score: 60.0,
            raw_score: Some(60.0),
            iwim_applied: false,
            iwim_source: IwimSource::Disabled,
            iwim_threat_score: None,
            veto_reason: None,
            defer_reason: None,
            is_gunshot: false,
            action: EngineAction::Continue,
        });

        let action = session.final_verdict().await;
        assert!(
            matches!(action, EngineAction::Kill(_)),
            "Expected FULL-stage threshold to reject score=60 despite low tx_count fallback"
        );
    }

    #[tokio::test]
    async fn test_early_window_discontinuity_ignored() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        // EARLY WINDOW SCENARIO:
        // Recent start time (<10s elapsed)
        let start_ts = 1_000_000;
        let current_ts = start_ts + 5_000; // +5s (Early)

        let mut s1 = build_snapshot(Some(10), 100, 50.0);
        s1.timestamp_ms = start_ts;

        // Regression: tx_count 100 -> 90.
        // Even though 90 >= 5, the time < 10s makes it Early Window.
        let mut s2 = build_snapshot(Some(11), 90, 60.0);
        s2.timestamp_ms = current_ts;

        let snapshots = vec![s1, s2];
        ledger.commit_history(base_mint, snapshots, None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;
        session.current_cycle = 1;
        // Seed start time
        session.first_event_ts_ms = Some(start_ts);

        let result = session.evaluate_cycle().await;

        match result.action {
            EngineAction::Kill(_) => {
                panic!("Should NOT kill session in Early Window due to discontinuity!")
            }
            _ => {
                // Expected behavior (likely Continue or Buy depending on scores, but NOT Kill)
                assert!(
                    result.veto_reason.is_none(),
                    "Should not have veto reason, got {:?}",
                    result.veto_reason
                );
            }
        }
    }

    #[tokio::test]
    async fn test_continuity_ok_no_veto() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let snapshots = vec![
            build_snapshot(Some(10), 5, 1.0),
            build_snapshot(Some(11), 6, 1.5),
        ];
        ledger.commit_history(base_mint, snapshots, None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.current_cycle = 1;

        let result = session.evaluate_cycle().await;

        assert!(
            matches!(result.action, EngineAction::Continue | EngineAction::Buy(_)),
            "Continuity should not trigger veto, got {:?}",
            result.action
        );
    }

    #[tokio::test]
    async fn test_transfusion_to_live_regression_skips_scoring() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let snapshots = vec![
            build_snapshot(Some(390_999), 15, 6.5),
            build_snapshot(Some(391_000), 1, 0.1),
        ];
        ledger.commit_history(base_mint, snapshots, None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;
        session.current_cycle = 1;
        // Explicitly set Stable State to ensure regression is caught (ignoring low tx count check)
        session.reached_stable_state = true;

        let result = session.evaluate_cycle().await;
        assert!(
            matches!(result.action, EngineAction::Kill(_)),
            "Transfusion→live regression should veto, got {:?}",
            result.action
        );
        assert_eq!(result.veto_reason, Some(VetoReason::SnapshotDiscontinuity));
    }

    #[tokio::test]
    async fn test_unknown_price_defers_instead_of_kill() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut snap = build_snapshot(Some(10), 1, 1.0);
        snap.price_state = PriceState::Unknown;
        snap.price_sol_per_token = 0.0;
        snap.price_reason = Some(PriceReason::MissingPriceData);
        ledger.commit_history(base_mint, vec![snap], None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.current_cycle = 1;

        let result = session.evaluate_cycle().await;

        assert_eq!(result.action, EngineAction::Continue);
        assert_eq!(result.defer_reason, Some(DeferReason::PriceUnknown));
        assert!(result.veto_reason.is_none());
    }

    #[tokio::test]
    async fn test_unknown_price_with_zero_reserves_defers() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut snap = build_snapshot(Some(10), 1, 1.0);
        snap.price_state = PriceState::Unknown;
        snap.price_sol_per_token = f64::NAN;
        snap.reserve_base = 0.0;
        snap.reserve_quote = 0.0;
        ledger.commit_history(base_mint, vec![snap], None);

        let mut session =
            PredictionSession::new(base_mint, pool_id, bonding_curve, Arc::clone(&ledger), None);
        session.current_cycle = 1;

        let result = session.evaluate_cycle().await;

        assert_eq!(result.action, EngineAction::Continue);
        assert_eq!(result.defer_reason, Some(DeferReason::PriceUnknown));
        assert!(result.veto_reason.is_none());
    }

    #[test]
    fn test_chaos_gate_blocks_invalid_price() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut snap = build_snapshot(Some(1), 1, 1.0);
        snap.price_state = PriceState::Invalid;

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;

        let gate = session.chaos_gate(&snap, None, snap.timestamp_ms, snap.tx_count, None);
        assert_eq!(gate, GateAction::Blocked(VetoReason::PriceInvalid));
    }

    #[test]
    fn test_chaos_gate_defers_unknown_price_after_warmup() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut snap = build_snapshot(Some(5), 2, 1.0);
        snap.price_state = PriceState::Unknown;
        snap.price_sol_per_token = 0.0;

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;

        let gate = session.chaos_gate(&snap, None, snap.timestamp_ms, snap.tx_count, None);
        assert_eq!(gate, GateAction::Deferred(DeferReason::PriceUnknown));
    }

    #[test]
    fn test_chaos_gate_defers_slot_zero_without_warmup_increment() {
        // NOTE: Per FAZA 7, slot=0 or slot=None is NOT a rejection criterion.
        // Events without slot are first-class citizens.
        // The test now verifies warmup-based deferral (correct behavior).
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let snap = build_snapshot(Some(0), 0, 0.0);
        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);

        let gate = session.chaos_gate(&snap, None, snap.timestamp_ms, snap.tx_count, None);
        // WarmupNotReady is expected because we don't have enough live snapshots
        // SlotMissingOrZero is NO LONGER a valid deferral reason per FAZA 7
        assert_eq!(gate, GateAction::Deferred(DeferReason::WarmupNotReady));
        // live_snapshots_seen=1 because chaos_gate counts the snapshot before checking warmup
        // This is correct - warmup check uses the incremented counter
        assert_eq!(session.live_snapshots_seen, 1);
    }

    #[test]
    fn test_chaos_gate_warmup_progression_requires_two_live_snapshots() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);

        let snap1 = build_snapshot(Some(1), 1, 1.0);
        let gate1 = session.chaos_gate(&snap1, None, snap1.timestamp_ms, snap1.tx_count, None);
        assert_eq!(gate1, GateAction::Deferred(DeferReason::WarmupNotReady));
        assert_eq!(session.live_snapshots_seen, 1);

        let snap2 = build_snapshot(Some(2), 2, 2.0);
        let gate2 = session.chaos_gate(
            &snap2,
            Some(&snap1),
            snap2.timestamp_ms,
            snap2.tx_count,
            None,
        );
        assert_eq!(gate2, GateAction::Allowed);
        assert_eq!(session.live_snapshots_seen, 2);
    }

    #[test]
    fn test_chaos_gate_blocks_live_discontinuity_regression() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        session.live_snapshots_seen = WARMUP_LIVE_MIN;
        session.reached_stable_state = true;

        let prev = build_snapshot(Some(5), 5, 10.0);
        let regress = build_snapshot(Some(6), 4, 9.0);

        let gate = session.chaos_gate(
            &regress,
            Some(&prev),
            regress.timestamp_ms,
            regress.tx_count,
            None,
        );
        assert_eq!(gate, GateAction::Blocked(VetoReason::SnapshotDiscontinuity));
    }

    #[test]
    fn test_preflight_allows_unknown_price_without_veto() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut snap = build_snapshot(Some(10), 1, 1.0);
        snap.price_state = PriceState::Unknown;
        snap.price_sol_per_token = f64::NAN;
        snap.reserve_base = 1.0;
        snap.reserve_quote = 1.0;

        let session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);
        assert!(session.preflight_veto(&snap).is_none());
    }

    #[tokio::test]
    async fn test_cycle_timing() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let mut session = PredictionSession::new(base_mint, pool_id, bonding_curve, ledger, None);

        // Manually test a few cycles to verify timing
        let mut ticker = interval(session.cycle_duration);
        ticker.tick().await; // Skip first immediate tick

        let start = TokioInstant::now();

        // Wait for 3 cycles
        for _ in 0..3 {
            ticker.tick().await;
        }

        let elapsed = start.elapsed();

        // Should take approximately 3 * default cycle duration
        assert!(elapsed.as_millis() >= 1240);
        assert!(elapsed.as_millis() < 1400);
    }

    struct StaticIwimProvider(f32);

    impl IwimProvider for StaticIwimProvider {
        fn fetch_cached_iwim(&self, _pool_amm_id: Pubkey) -> Option<IwimResult> {
            Some(IwimResult {
                rug_threat_score: self.0,
                ..IwimResult::default()
            })
        }
    }

    #[tokio::test]
    async fn test_iwim_included_in_final_phase_when_enabled() {
        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let config = PredictionSessionConfig {
            iwim_enabled: true,
            cycle_duration: Duration::from_millis(default_cycle_duration_ms()),
            bva_config: BvaConfig::default(),
            panic_config: PanicConfig::default(),
            tcr_phi_config: TcrPhiConfig::default(),
            sobp_min_emitted_tx_early: DEFAULT_SOBP_MIN_EMITTED_TX_EARLY,
            analysis_window_ms_config: DEFAULT_ANALYSIS_WINDOW_MS_CONFIG,
            config_path: None,
            ghost_brain_config: None,
        };

        let snapshots = vec![
            build_snapshot(Some(10), 5, 1.0),
            build_snapshot(Some(11), 10, 2.0),
        ];
        ledger.commit_history(base_mint, snapshots, None);

        let mut session_risky = PredictionSession::new_with_config(
            base_mint,
            pool_id,
            bonding_curve,
            Arc::clone(&ledger),
            None,
            config.clone(),
        );
        session_risky.live_snapshots_seen = WARMUP_LIVE_MIN;
        session_risky.current_cycle = 3;
        session_risky.set_iwim_provider(Some(Arc::new(StaticIwimProvider(0.9))));

        let mut session_safe = PredictionSession::new_with_config(
            base_mint,
            pool_id,
            bonding_curve,
            Arc::clone(&ledger),
            None,
            config,
        );
        session_safe.live_snapshots_seen = WARMUP_LIVE_MIN;
        session_safe.current_cycle = 3;
        session_safe.set_iwim_provider(Some(Arc::new(StaticIwimProvider(0.1))));

        let risky = session_risky.evaluate_cycle().await;
        let safe = session_safe.evaluate_cycle().await;

        assert!(risky.iwim_applied && safe.iwim_applied);
        assert_eq!(risky.iwim_source, IwimSource::ProviderHit);
        assert_eq!(safe.iwim_source, IwimSource::ProviderHit);
        assert_eq!(risky.iwim_threat_score, Some(0.9));
        assert_eq!(safe.iwim_threat_score, Some(0.1));
    }

    #[test]
    fn test_mpcf_highly_organic() {
        let config = MpcfConfig::default();
        let result = mpcf_result_from_counts(9, 1, 0, 0, &config);

        assert_eq!(result.human_count, 9);
        assert_eq!(result.sniper_count, 1);
        assert_eq!(result.classification, MpcfClassification::HighlyOrganic);
        assert!(result.score >= 1.5 && result.score <= 2.5);
    }

    #[test]
    fn test_mpcf_bot_dominated() {
        let config = MpcfConfig::default();
        let result = mpcf_result_from_counts(3, 7, 0, 0, &config);

        assert_eq!(result.sniper_count, 7);
        assert_eq!(result.human_count, 3);
        assert!(result.bot_ratio > 0.5);
        assert_eq!(result.classification, MpcfClassification::BotDominated);
        assert!(result.score >= 0.2 && result.score <= 0.5);
    }

    #[test]
    fn test_prediction_session_defaults_event_ts_source_to_legacy_compat() {
        let session = PredictionSession::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Arc::new(ShadowLedger::new()),
            None,
        );

        assert_eq!(session.event_ts_source, EventTsSource::LegacyCompat);
    }

    #[test]
    fn test_tcr_timing_ts_ms_requires_explicit_epoch_like_source() {
        let base = TransactionRecord {
            slot: Some(1),
            signature: "timing".to_string(),
            signer: Pubkey::new_unique(),
            sol_amount: 1.0,
            is_buy: true,
            is_dev_buy: false,
            timestamp_ms: 1_234,
            event_time: ghost_core::EventTimeMetadata::default(),
            event_ts_source: EventTsSource::Event,
            seq_no: 1,
            raw_bytes: None,
            raw_bytes_missing_reason: RawBytesMissingReason::Unknown,
            price_quote: None,
        };

        assert_eq!(PredictionSession::tcr_timing_ts_ms(&base), Some(1_234));
        assert_eq!(
            PredictionSession::tcr_timing_ts_ms(&TransactionRecord {
                event_ts_source: EventTsSource::IngressWall,
                ..base.clone()
            }),
            Some(1_234)
        );
        assert_eq!(
            PredictionSession::tcr_timing_ts_ms(&TransactionRecord {
                event_ts_source: EventTsSource::LegacyCompat,
                ..base.clone()
            }),
            None
        );
        assert_eq!(
            PredictionSession::tcr_timing_ts_ms(&TransactionRecord {
                event_ts_source: EventTsSource::Arrival,
                ..base.clone()
            }),
            None
        );
        assert_eq!(
            PredictionSession::tcr_timing_ts_ms(&TransactionRecord {
                event_ts_source: EventTsSource::Wallclock,
                ..base
            }),
            None
        );
    }

    #[test]
    fn test_current_event_axis_ts_ms_prefers_cycle_now_over_snapshot_timestamp() {
        let mut session = PredictionSession::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Arc::new(ShadowLedger::new()),
            None,
        );
        session.cycle_now_event_ts_ms = 9_000;
        session.event_ts_source = EventTsSource::Event;

        let snapshot = build_snapshot(Some(7), 1, 1.0);
        assert_eq!(snapshot.timestamp_ms, 7);
        assert_eq!(session.current_event_axis_ts_ms(&snapshot), 9_000);
    }

    #[test]
    fn test_current_event_axis_ts_ms_rejects_legacy_compat_snapshot_clock() {
        let session = PredictionSession::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Arc::new(ShadowLedger::new()),
            None,
        );

        let snapshot = build_snapshot(Some(7), 1, 1.0);
        assert_eq!(snapshot.timestamp_ms, 7);
        assert_eq!(session.current_event_axis_ts_ms(&snapshot), 0);
    }

    #[test]
    fn test_has_decision_event_clock_requires_explicit_epoch_like_source() {
        assert!(PredictionSession::has_decision_event_clock(
            EventTsSource::Event,
            1_000
        ));
        assert!(PredictionSession::has_decision_event_clock(
            EventTsSource::IngressWall,
            1_000
        ));
        assert!(!PredictionSession::has_decision_event_clock(
            EventTsSource::LegacyCompat,
            1_000
        ));
        assert!(!PredictionSession::has_decision_event_clock(
            EventTsSource::Arrival,
            1_000
        ));
        assert!(!PredictionSession::has_decision_event_clock(
            EventTsSource::Wallclock,
            1_000
        ));
    }

    #[test]
    fn test_decision_axis_transactions_drop_storage_only_sources() {
        let transactions = vec![
            build_tx_record(10, EventTsSource::Event),
            build_tx_record(20, EventTsSource::IngressWall),
            build_tx_record(30, EventTsSource::LegacyCompat),
            build_tx_record(40, EventTsSource::Arrival),
            build_tx_record(50, EventTsSource::Wallclock),
        ];

        let normalized = PredictionSession::decision_axis_transactions(transactions);

        assert_eq!(normalized.len(), 2);
        assert_eq!(
            normalized
                .iter()
                .map(|tx| tx.timestamp_ms)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
        assert!(normalized
            .iter()
            .all(|tx| tx.event_ts_source.is_decision_event_source()));
    }

    #[test]
    fn test_decision_window_transactions_filter_by_decision_clock_only() {
        let transactions = vec![
            build_tx_record(10, EventTsSource::Event),
            build_tx_record(20, EventTsSource::IngressWall),
            build_tx_record(25, EventTsSource::LegacyCompat),
            build_tx_record(30, EventTsSource::Event),
        ];

        let windowed = PredictionSession::decision_window_transactions(transactions, 15, 30);

        assert_eq!(windowed.len(), 2);
        assert_eq!(
            windowed
                .iter()
                .map(|tx| tx.timestamp_ms)
                .collect::<Vec<_>>(),
            vec![20, 30]
        );
    }

    #[test]
    fn test_mpcf_rejects_wash_trading() {
        let config = MpcfConfig::default();
        let mut transactions = Vec::new();

        transactions.push(TransactionRecord {
            slot: None,
            signature: "whale".to_string(),
            signer: Pubkey::new_unique(),
            sol_amount: 50.0,
            is_buy: true,
            is_dev_buy: false,
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            event_ts_source: EventTsSource::Event,
            seq_no: 1,
            raw_bytes: None,
            raw_bytes_missing_reason: RawBytesMissingReason::Unknown,
            price_quote: None,
        });

        for i in 0..99 {
            transactions.push(TransactionRecord {
                slot: Some(i),
                signature: format!("sniper-{i}"),
                signer: Pubkey::new_unique(),
                sol_amount: 0.009,
                is_buy: true,
                is_dev_buy: false,
                timestamp_ms: i as u64,
                event_time: ghost_core::EventTimeMetadata::default(),
                event_ts_source: EventTsSource::Event,
                seq_no: (i + 2) as u64,
                raw_bytes: None,
                raw_bytes_missing_reason: RawBytesMissingReason::Unknown,
                price_quote: None,
            });
        }

        let mut human = 0usize;
        let mut sniper = 0usize;
        let mut mev = 0usize;
        let mut unknown = 0usize;
        let mut weighted_volume = 0.0;
        let mut total_volume = 0.0;

        for tx in &transactions {
            let actor = classify_actor_heuristic(tx);
            match actor {
                ActorType::HumanDesktop | ActorType::HumanMobile => human += 1,
                ActorType::SniperScript => sniper += 1,
                ActorType::MEVArb | ActorType::SybilBot => mev += 1,
                _ => unknown += 1,
            }

            let weight = match actor {
                ActorType::HumanMobile | ActorType::HumanDesktop => 2.0,
                ActorType::SniperScript | ActorType::MEVArb | ActorType::SybilBot => 0.5,
                _ => 1.0,
            };

            weighted_volume += tx.sol_amount * weight;
            total_volume += tx.sol_amount;
        }

        let count_based = mpcf_result_from_counts(human, sniper, mev, unknown, &config);
        let volume_based = (weighted_volume / total_volume).clamp(
            config.min_bot_penalty as f64,
            config.max_organic_boost as f64,
        );

        assert!(volume_based > 0.9 && volume_based < 1.1);
        assert!(count_based.score < 0.3);
        assert!(count_based.score < volume_based - 0.5);
    }

    // NOTE: test_sobp_uses_slot_not_timestamp was removed per FAZA 7 migration.
    // SOBP now uses event-time via current_sobp() - there is no slot-based calculate_sobp(slot) anymore.
    // Cycles are execution timers, NOT market time indicators.

    #[test]
    fn test_cir_scale_blocks_bot_spam() {
        let min_weight = 0.2;
        let panic_output = PanicOutput {
            is_bot_spam: true,
            confidence: 1.0,
            is_high_pressure: true,
            ..PanicOutput::default()
        };
        let scale = PredictionSession::cir_scale_from_panic(min_weight, 0.5, &panic_output);
        assert!((scale - min_weight).abs() < 1e-6);
    }

    #[test]
    fn test_cir_scale_blocks_low_confidence() {
        let min_weight = 0.2;
        let panic_output = PanicOutput {
            is_bot_spam: false,
            confidence: 0.4,
            is_high_pressure: true,
            ..PanicOutput::default()
        };
        let scale = PredictionSession::cir_scale_from_panic(min_weight, 0.5, &panic_output);
        assert!((scale - min_weight).abs() < 1e-6);
    }

    #[test]
    fn test_cir_scale_allows_crowd_pressure() {
        let min_weight = 0.2;
        let panic_output = PanicOutput {
            is_bot_spam: false,
            confidence: 0.8,
            is_high_pressure: true,
            ..PanicOutput::default()
        };
        let scale = PredictionSession::cir_scale_from_panic(min_weight, 0.5, &panic_output);
        assert!((scale - 1.0).abs() < 1e-6);
    }

    fn make_panic_tx(
        ts: u64,
        signer: Pubkey,
        success: bool,
        requested: f64,
        executed: f64,
    ) -> PanicTx {
        PanicTx {
            slot: Some(1),
            arrival_ts_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            impulse_weight: 1.0,
            requested_sol_amount: requested,
            executed_sol_amount: executed,
            priority_fee_micro_lamports: 1_000,
            success,
            signer,
        }
    }

    #[test]
    fn test_panic_system_bot_spam_injection() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();
        let signer = Pubkey::new_unique();

        for i in 0..config.impulse_threshold_txps {
            let _ = state.update(make_panic_tx(1_000 + i as u64, signer, false, 1.0, 0.0));
        }

        let output = state.calculate_score(&config);
        assert!(output.is_bot_spam);
        assert!(!output.is_high_pressure);
        let scale =
            PredictionSession::cir_scale_from_panic(0.2, config.cir_confidence_threshold, &output);
        assert!((scale - 0.2).abs() < 1e-6);
    }

    #[test]
    fn test_panic_system_crowd_burst() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();

        for i in 0..config.impulse_threshold_txps {
            let signer = Pubkey::new_unique();
            let _ = state.update(make_panic_tx(2_000 + i as u64, signer, false, 2.0, 0.0));
        }

        let output = state.calculate_score(&config);
        assert!(output.is_high_pressure);
        assert!(!output.is_bot_spam);
        let scale =
            PredictionSession::cir_scale_from_panic(0.2, config.cir_confidence_threshold, &output);
        assert!((scale - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_panic_system_mixed_case() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();
        let bot = Pubkey::new_unique();

        for i in 0..10u64 {
            let _ = state.update(make_panic_tx(3_000 + i, bot, false, 1.0, 0.0));
        }
        for i in 0..10u64 {
            let _ = state.update(make_panic_tx(
                4_000 + i,
                Pubkey::new_unique(),
                true,
                1.0,
                1.0,
            ));
        }

        let output = state.calculate_score(&config);
        assert!(!output.is_bot_spam);
        assert!(!output.is_high_pressure);
        let scale =
            PredictionSession::cir_scale_from_panic(0.2, config.cir_confidence_threshold, &output);
        assert!((scale - 0.2).abs() < 1e-6);
    }

    #[test]
    fn test_panic_system_entropy_inconsistency_caps_confidence() {
        let config = PanicConfig::default();
        let mut panic_state = PanicState::new();

        for i in 0..config.impulse_threshold_txps {
            let signer = Pubkey::new_unique();
            let _ = panic_state.update(make_panic_tx(5_000 + i as u64, signer, true, 1.0, 1.0));
        }

        let mut panic_output = panic_state.calculate_score(&config);
        let mut signer_entropy = SignerEntropyState::new();
        let single = Pubkey::new_unique();
        for _ in 0..20 {
            signer_entropy.record_signer(single);
        }

        let signer_entropy_ratio = signer_entropy.entropy_ratio();
        let entropy_inconsistency = panic_output.entropy_score >= config.entropy_threshold
            && signer_entropy_ratio < config.entropy_threshold;
        if entropy_inconsistency {
            panic_output.confidence = panic_output
                .confidence
                .min(config.entropy_inconsistency_confidence_cap);
        }

        assert!(entropy_inconsistency);
        assert!(panic_output.confidence <= config.entropy_inconsistency_confidence_cap);
    }

    #[test]
    fn test_scr_hint_adjustment_inconsistency_penalizes() {
        let config = PanicConfig::default();
        let anomaly = MarketAnomalyOutput {
            failed_ratio: 0.8,
            fee_spike: 0.9,
            avg_fee_prev_slot: 0.0,
            current_avg_fee: 0.0,
            frantic_signer_count: 0,
        };
        let base = 0.2_f32;
        let adjusted = PredictionSession::adjust_scr_bot_score(base, anomaly, &config, false);
        let adjusted_inconsistent =
            PredictionSession::adjust_scr_bot_score(base, anomaly, &config, true);
        assert!(adjusted > base);
        assert!(adjusted_inconsistent > adjusted);
    }

    #[test]
    fn chaos_inputs_reject_invalid_price() {
        let mut snap = MarketSnapshot::default();
        snap.price_state = PriceState::Invalid;
        snap.reserve_base = 10.0;
        snap.reserve_quote = 5.0;

        assert!(!chaos_inputs_valid(&snap));
    }

    #[test]
    fn chaos_inputs_accept_valid_reserves_and_price() {
        let mut snap = MarketSnapshot::default();
        snap.price_state = PriceState::Valid;
        snap.reserve_base = 10.0;
        snap.reserve_quote = 5.0;

        assert!(chaos_inputs_valid(&snap));
    }
}
