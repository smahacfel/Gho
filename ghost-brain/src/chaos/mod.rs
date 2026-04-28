//! Chaos Engine Module
//!
//! This module provides Monte Carlo simulation capabilities for AMM price discovery
//! and risk assessment during the "2-Second Void" period.
//!
//! ## Components
//!
//! - **amm_math**: Analytic formulas for Constant Product AMM calculation
//! - **engine**: Monte Carlo simulation engine for parallel risk assessment
//! - **distributions**: Buyer profile probability distributions for market scenarios
//! - **flowfield**: Flowfield construction & extraction (WHF Part 1) from transaction streams
//! - **field_analysis**: Harmonic & field analysis (WHF Part 2) for wallet energy fields
//! - **whf_signals**: Signal detector & launcher API (WHF Part 3) for trading automation

pub mod amm_math;
pub mod distributions;
pub mod engine;
pub mod field_analysis;
pub mod flowfield;
pub mod whf_signals;

// Re-export key types from amm_math
pub use amm_math::{
    build_pumpfun_amm_pool, AmmMathError, AmmPool, BatchSwapInput, BatchSwapOutput,
    CompactSwapResult, SwapResult,
};

// Re-export key types from distributions
pub use distributions::{action_to_amount_multiplier, is_buy, is_sell, BuyerProfile, MarketAction};

// Re-export key types from engine
pub use engine::{ChaosEngine, ChaosResult, MarketScenario, SimulationConfig, SimulationRun};

// Re-export key types from flowfield
pub use flowfield::{
    flow_transaction_from_pool_event, FlowDirection, FlowTransaction, FlowVector, FlowfieldConfig,
    FlowfieldExtractor, DEFAULT_WINDOW_MS, MAX_WINDOW_MS, MIN_WINDOW_MS,
};

// Re-export key types from field_analysis
pub use field_analysis::{HarmonicFieldAnalysis, HarmonicFieldAnalyzer, DEFAULT_BUFFER_SIZE};

// Re-export key types from whf_signals
pub use whf_signals::{FlowMetrics, WhfSignal, WhfSignalConfig, WhfSignalDetector, WhfSignalType};
