//! Signal Processing and Detection Module
//!
//! This module provides real-time signal processing capabilities for detecting
//! trading patterns, bot activity, and market anomalies during the "2-Second Void"
//! between fast module completion and external API responses.

pub mod frb;
pub mod frb_integrator;
pub mod ligma;
pub mod market_signals;
pub mod resonance;

// Re-export main types
pub use resonance::{
    ActivityClassification, CircularBuffer, ResonanceConfig, ResonanceDetector, ResonanceResult,
};

// Re-export FRB types
pub use frb::{
    analyze_resonance, extract_bands, BandConfig, BandExtractor, BandProfile, BandRange,
    BandTransaction, FrbResult, FrbSignal, ResonanceAnalyzer,
    ResonanceConfig as FrbResonanceConfig, DEFAULT_LONG_BAND_MAX, DEFAULT_LONG_BAND_MIN,
    DEFAULT_MEDIUM_BAND_MAX, DEFAULT_MEDIUM_BAND_MIN, DEFAULT_SHORT_BAND_MAX,
    DEFAULT_SHORT_BAND_MIN, WINDOW_15S, WINDOW_1S, WINDOW_5S, WINDOW_60S,
};

// Re-export FRB Integrator types
pub use frb_integrator::{
    FrbIntegrationResult, FrbIntegrator, FrbResonanceConfig as FrbIntegratorConfig,
    QmanEnhancement, QofsvEnhancement, WhfValidation,
};

// Re-export MarketSignals
pub use market_signals::{
    MarketSignals, OrderbookSignals, PriceSignals, TimeSignals, VolumeSignals,
};

// Re-export LIGMA types
pub use ligma::{compute_ligma, LigmaDiagnostics, LigmaResult};
