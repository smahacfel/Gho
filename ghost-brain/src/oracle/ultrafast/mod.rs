//! Ultrafast T+2s Oracle Components
//!
//! This module contains advanced signal processing components designed
//! to analyze token launches within the critical 0-2 second window.
//!
//! ## Components
//!
//! - **SSMI (Sub-Slot Microentropy Index)**: Analyzes transaction timing patterns
//!   to classify sources (Bot/Human/ViralLaunch) using Shannon entropy and AR-1 correlation.
//!   Supports configurable histogram bins via `SubSlotMicroentropyConfigurable<BINS>`.
//!   Now includes adaptive thresholds for dynamic Solana environments.
//!
//! - **MPCF (Micro-Payload Cognitive Fingerprint)**: Actor-behavioral byte fingerprinting
//!   for ultra-fast classification (30-70μs) of transaction sources. Analyzes raw transaction
//!   bytes to detect bots vs humans vs sybil networks before any on-chain patterns emerge.
//!   Zero-heap, stack-only design using std/core/alloc.
//!
//! - **IWIM (Initial Wallet Intent Mapping)**: Ultra-fast dev-wallet behavioral analysis
//!   within the critical 0-2s window after token launch. Detects creator intentions
//!   (SCAMMER vs BUILDER vs SYBIL-BOT) using Lightning CTP, CMM, and CDIS analysis.
//!   Performance target: <120μs. Zero-history, RPC-free design.
//!
//! - **SOBP (Slot-Over-Slot Buying Pressure)**: Ultra-fast buying pressure analysis using
//!   slot-level transaction bucketing. Detects pump onset within 0-2s by measuring rate of
//!   change in weighted buy intensity across consecutive slots. Zero-heap circular buffer
//!   with O(1) access. Integrates with MPCF for intelligent actor weighting (humans 2.0x,
//!   bots 0.5x). Performance target: <10μs per transaction.
//!
//! - **CIR (Causal Impact Ratio)**: Slot-based causal impact scoring for environments
//!   without raw bytes. Incrementally evaluates whether a tx triggered reactions from
//!   independent actors and emits only causally impactful txs for SOBP.
//!
//! - **PRAECOG (Predictive Rapid Adversarial Evaluation & Counterfactual Oracle Guard)**:
//!   Ultrafast adversarial simulation for evaluating pool exploitability within 0-2s.
//!   Answers: "How quickly could an attacker destroy this pool?" by simulating attack paths
//!   (buy→sell, sandwich, crash attempts) and measuring minimum capital to crash, sandwich
//!   feasibility, and overall adversarial vulnerability. Performance target: <250μs.
//!
//! - **FRE (Fractal Resonance Engine)**: Fractal-based analysis for market microstructure.

pub mod cir;
pub mod ecto;
pub mod fre;
pub mod iwim;
pub mod market_anomaly;
pub mod mpcf;
pub mod panic;
pub mod praecog;
pub mod qass_stub;
pub mod signer_entropy;
pub mod sobp;
pub mod ssmi;
pub mod tcr_phi;
pub mod wave_builder_stub;

// Re-export IWIM types
pub use iwim::{iwim_analyze, CdisSignal, CmmSignal, CtpSignal, IwimInput, IwimResult, TxType};

// Re-export MPCF types
pub use mpcf::{mpcf_infer, ActorInference, ActorType};

// Re-export CIR types
pub use cir::{
    cir_tx_key, BuySell, CirConfig, CirContext, CirCore, CirEmittedTx, CirEvent, CirTelemetry,
};

// Re-export TCR-Φ types
pub use tcr_phi::{CausalBreak, TcrImpact, TcrPhiConfig, TcrPhiCore, TcrReaction, TcrScore};

// Re-export ECTO types
pub use ecto::{
    EctoConfig, EctoFlags, EctoSignal, EctoState, EctoVerdict, DEFAULT_ECTO_BUFFER_CAPACITY,
    ECTO_MAX_WINDOW_MS,
};

// Re-export PANIC types
pub use crate::config::PanicConfig;
pub use market_anomaly::{MarketAnomalyOutput, MarketAnomalyState, MarketAnomalyTx};
pub use panic::{PanicOutput, PanicState, PanicTx};
pub use signer_entropy::SignerEntropyState;

// Re-export SOBP types
pub use sobp::{
    CircularBuffer, PressureState, SlotMetrics, SobpCore, SobpResult, TransactionRecord,
    DEFAULT_SLOT_CAPACITY,
};

// Re-export FRE types
pub use crate::config::ghost_brain_config::FreConfig;
pub use fre::{FractalAction, FractalEngine, FractalMath, FractalVerdict, WelfordVariance};

// Re-export SSMI types
pub use ssmi::{
    AdaptiveThresholds, SourceType, SsmiResult, SubSlotMicroentropy,
    SubSlotMicroentropyConfigurable,
};

// Re-export PRAECOG types
pub use praecog::{praecog_analyze, PraecogInput, PraecogParams, PraecogResult, SwapInfo};

// Re-export QASS stub types (deprecated - for backward compatibility)
pub use qass_stub::{
    DataSource, HeuristicWave, QASSResult, QuantumAmplitudeScorer, QuantumAmplitudeScorerN,
    WaveContribution, MAX_WAVES, NUM_DOMINANT_WAVES,
};

// Re-export wave builder stub functions (deprecated - for backward compatibility)
pub use wave_builder_stub::{
    build_cluster_wave, build_iwim_wave, build_ligma_wave, build_povc_wave, build_praecog_wave,
    build_profiler_wave, build_scr_wave, build_shadow_wave, build_ssmi_wave, build_ulvf_wave,
    build_vision_wave, build_waves_from_signals, AlertLevel, OracleSignals, ViralLaunchAlert,
    ALERT_CONFIDENCE_THRESHOLD, MODERATE_OPPORTUNITY_THRESHOLD, STRONG_BUY_THRESHOLD,
    VIRAL_LAUNCH_THRESHOLD,
};
