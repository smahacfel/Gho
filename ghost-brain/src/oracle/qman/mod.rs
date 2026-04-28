//! QMAN - Quantum Market Analysis Network
//!
//! A quantum-inspired framework for market analysis and capital flow prediction.
//!
//! ## Components
//!
//! - **Part 1 (WEST)**: Wallet Energy & State Tracker - tracks wallet states and energy distribution
//! - **Part 2 (Matrix Engine)**: Transition Matrix & Unitary Evolution - predicts capital flow
//! - **Part 3 (Signal Detector)**: Hyper-Bubble Detection & Oracle Output - generates trading signals

pub mod signal_detector;
pub mod transition_matrix;
pub mod unitary_evolution;

// Re-export main types
pub use signal_detector::{
    MigrationForecast, SignalDetector, SignalDetectorConfig, SignalResult, TradingSignal,
};
pub use transition_matrix::{
    SparseTransitionMatrix, Transition, TransitionMatrix, TransitionMatrixBuilder,
};
pub use unitary_evolution::{PredictionResult, UnitaryEvolution};
