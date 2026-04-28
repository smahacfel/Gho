//! Signal Collection Module for HyperPrediction Oracle
//!
//! This module provides a unified interface for collecting and tracking signals
//! from various analysis modules (LIGMA, QEDD, Cluster, MCI, Paradox).
//!
//! ## Key Features
//!
//! - **Explicit Fallback Tracking**: Every signal has a `SignalSource` indicating
//!   whether it came from real data (Explicit), a default value (Fallback), or is
//!   completely unavailable (Unavailable).
//!
//! - **Centralized Phase Logic**: Phase-dependent signal collection is handled by
//!   the `SignalCollector`, eliminating duplicated phase checks throughout the codebase.
//!
//! - **Structured Logging**: All signal status is logged via the collector using
//!   structured logging patterns.
//!
//! ## Architecture
//!
//! ```text
//! HyperPrediction Oracle
//!         ↓
//! SignalCollector::collect()
//!         ↓
//! ┌───────────────────────────────────┐
//! │  check_ligma()                   │
//! │  check_qedd()                    │
//! │  check_cluster()                 │
//! │  check_mci()                     │
//! │  check_paradox()                 │
//! └───────────────────────────────────┘
//!         ↓
//! SignalBundle (with explicit source tracking)
//! ```

pub mod builders;
pub mod cluster;
pub mod ligma;
pub mod mci;
pub mod paradox;
pub mod qedd;

// Re-export builder functions for convenience
pub use builders::{build_dev_buy_wave_scaled, build_liquidity_wave_scaled};

use crate::chaos::amm_math::AmmPool;
use crate::fast_pipeline::EnhancedCandidate;
use crate::models::mci_result::MciResult;
use crate::models::qedd_result::QeddResult;
use crate::oracle::cluster_hunter::ClusterAnalysis;
use crate::oracle::hyper_prediction::state::AnalysisPhase;
use crate::signals::LigmaResult;
use seer::paradox_sensor::ParadoxState;
use tracing::{debug, warn};

// =============================================================================
// Core Signal Abstractions
// =============================================================================

/// Result from a signal check with explicit fallback tracking
///
/// This structure makes signal availability and reliability transparent:
/// - `value`: The actual signal data (if available)
/// - `source`: How this signal was obtained (explicit, fallback, or unavailable)
/// - `confidence`: Quality metric for the signal (0.0 = no confidence, 1.0 = full confidence)
///
/// ## Examples
///
/// ```ignore
/// // Signal computed from real data
/// SignalResult {
///     value: Some(ligma_data),
///     source: SignalSource::Explicit,
///     confidence: 0.95,
/// }
///
/// // Signal unavailable due to early stage
/// SignalResult {
///     value: None,
///     source: SignalSource::Unavailable,
///     confidence: 0.0,
/// }
///
/// // Signal using fallback/default value
/// SignalResult {
///     value: Some(default_qedd),
///     source: SignalSource::Fallback,
///     confidence: 0.3,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SignalResult<T> {
    pub value: Option<T>,
    pub source: SignalSource,
    pub confidence: f32,
}

impl<T> SignalResult<T> {
    /// Create a signal result from explicit computation
    pub fn explicit(value: T, confidence: f32) -> Self {
        Self {
            value: Some(value),
            source: SignalSource::Explicit,
            confidence,
        }
    }

    /// Create a signal result using fallback/default value
    pub fn fallback(value: T, confidence: f32) -> Self {
        Self {
            value: Some(value),
            source: SignalSource::Fallback,
            confidence,
        }
    }

    /// Create an unavailable signal result
    pub fn unavailable() -> Self {
        Self {
            value: None,
            source: SignalSource::Unavailable,
            confidence: 0.0,
        }
    }
}

/// Indicates how a signal value was obtained
///
/// This enum makes the provenance of signal data explicit, enabling:
/// - Better debugging (know when fallbacks are used)
/// - Confidence adjustment (penalize fallback sources)
/// - Analytics (track fallback rates across signals)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalSource {
    /// Signal computed from real data with full analysis
    Explicit,

    /// Signal unavailable, default/fallback value used
    Fallback,

    /// Signal completely unavailable (None)
    Unavailable,
}

/// Bundle of all collected signals for a candidate
///
/// This structure aggregates all signal results with their source tracking.
/// Each field is a `SignalResult<T>`, making it clear whether the signal
/// came from real data, fallback, or is unavailable.
#[derive(Debug, Clone)]
pub struct SignalBundle {
    pub ligma: SignalResult<LigmaResult>,
    pub qedd: SignalResult<QeddResult>,
    pub cluster: SignalResult<ClusterAnalysis>,
    pub mci: SignalResult<MciResult>,
    pub paradox: SignalResult<ParadoxState>,
}

// =============================================================================
// Signal Collector
// =============================================================================

/// Orchestrates signal collection from all analysis modules
///
/// The `SignalCollector` is responsible for:
/// - Invoking signal-specific collection functions
/// - Applying phase-based logic (early stage vs. full analysis)
/// - Logging signal status in a structured format
/// - Aggregating results into a `SignalBundle`
///
/// ## Usage
///
/// ```ignore
/// let collector = SignalCollector::new(ligma_config, qedd_engine, ...);
/// let bundle = collector.collect(candidate, state, pool);
///
/// // Check signal sources
/// if bundle.ligma.source == SignalSource::Explicit {
///     // Use LIGMA data with high confidence
/// }
/// ```
pub struct SignalCollector {
    // Signal-specific configurations/engines will be added as needed
    // For now, we keep the collector lightweight and pass dependencies
    // directly to collection functions
}

impl SignalCollector {
    /// Create a new signal collector
    pub fn new() -> Self {
        Self {}
    }

    /// Collect all available signals for the current candidate and analysis phase
    ///
    /// This is the main entry point for signal collection. It:
    /// 1. Determines the analysis phase (early stage vs. full analysis)
    /// 2. Invokes signal-specific collection functions
    /// 3. Logs signal status
    /// 4. Returns a `SignalBundle` with explicit source tracking
    ///
    /// # Arguments
    ///
    /// * `candidate` - The enhanced candidate being analyzed
    /// * `phase` - Current analysis phase (early stage or full analysis)
    /// * `pool` - Optional pool state for signal computation
    ///
    /// # Returns
    ///
    /// A `SignalBundle` containing all collected signals with source tracking
    pub fn collect(
        &self,
        candidate: &EnhancedCandidate,
        phase: AnalysisPhase,
        pool: Option<&AmmPool>,
    ) -> SignalBundle {
        debug!(
            "SignalCollector: Starting signal collection for phase={:?}, pool_available={}",
            phase,
            pool.is_some()
        );

        // For now, return unavailable signals
        // Individual signal collection will be implemented in their respective modules
        let ligma = SignalResult::unavailable();
        let qedd = SignalResult::unavailable();
        let cluster = SignalResult::unavailable();
        let mci = SignalResult::unavailable();
        let paradox = SignalResult::unavailable();

        let bundle = SignalBundle {
            ligma,
            qedd,
            cluster,
            mci,
            paradox,
        };

        self.log_signal_status(&bundle);

        bundle
    }

    /// Centralized helper for phase-conditional signal collection
    ///
    /// This method eliminates duplicated phase checks by providing a single
    /// point where phase logic is applied.
    ///
    /// # Arguments
    ///
    /// * `phase` - Current analysis phase
    /// * `f` - Function to call if phase allows (returns signal data)
    ///
    /// # Returns
    ///
    /// - `SignalResult::Explicit` if phase is FullAnalysis and function succeeds
    /// - `SignalResult::Unavailable` if phase is EarlyStage
    ///
    /// # Example
    ///
    /// ```ignore
    /// self.run_if_mature(phase, || compute_scr(timestamps))
    /// ```
    pub fn run_if_mature<T, F>(&self, phase: AnalysisPhase, f: F) -> SignalResult<T>
    where
        F: FnOnce() -> Option<T>,
    {
        match phase {
            AnalysisPhase::EarlyStage => SignalResult::unavailable(),
            AnalysisPhase::FullAnalysis => {
                if let Some(value) = f() {
                    SignalResult::explicit(value, 1.0)
                } else {
                    SignalResult::unavailable()
                }
            }
        }
    }

    /// Log signal collection status using structured logging
    ///
    /// This method provides a centralized logging point for all signal status,
    /// reducing the 47+ debug log occurrences scattered throughout the codebase.
    fn log_signal_status(&self, bundle: &SignalBundle) {
        debug!(
            signal = "LIGMA",
            source = ?bundle.ligma.source,
            confidence = bundle.ligma.confidence,
            available = bundle.ligma.value.is_some(),
            "Signal status"
        );

        debug!(
            signal = "QEDD",
            source = ?bundle.qedd.source,
            confidence = bundle.qedd.confidence,
            available = bundle.qedd.value.is_some(),
            "Signal status"
        );

        debug!(
            signal = "CLUSTER",
            source = ?bundle.cluster.source,
            confidence = bundle.cluster.confidence,
            available = bundle.cluster.value.is_some(),
            "Signal status"
        );

        debug!(
            signal = "MCI",
            source = ?bundle.mci.source,
            confidence = bundle.mci.confidence,
            available = bundle.mci.value.is_some(),
            "Signal status"
        );

        debug!(
            signal = "PARADOX",
            source = ?bundle.paradox.source,
            confidence = bundle.paradox.confidence,
            available = bundle.paradox.value.is_some(),
            "Signal status"
        );
    }
}

impl Default for SignalCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_result_explicit() {
        let result = SignalResult::explicit(42, 0.95);
        assert_eq!(result.value, Some(42));
        assert_eq!(result.source, SignalSource::Explicit);
        assert_eq!(result.confidence, 0.95);
    }

    #[test]
    fn test_signal_result_fallback() {
        let result = SignalResult::fallback(100, 0.3);
        assert_eq!(result.value, Some(100));
        assert_eq!(result.source, SignalSource::Fallback);
        assert_eq!(result.confidence, 0.3);
    }

    #[test]
    fn test_signal_result_unavailable() {
        let result: SignalResult<i32> = SignalResult::unavailable();
        assert_eq!(result.value, None);
        assert_eq!(result.source, SignalSource::Unavailable);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_run_if_mature_early_stage() {
        let collector = SignalCollector::new();
        let result = collector.run_if_mature(AnalysisPhase::EarlyStage, || Some(42));

        assert_eq!(result.source, SignalSource::Unavailable);
        assert_eq!(result.value, None);
    }

    #[test]
    fn test_run_if_mature_full_analysis() {
        let collector = SignalCollector::new();
        let result = collector.run_if_mature(AnalysisPhase::FullAnalysis, || Some(42));

        assert_eq!(result.source, SignalSource::Explicit);
        assert_eq!(result.value, Some(42));
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn test_run_if_mature_full_analysis_none() {
        let collector = SignalCollector::new();
        let result: SignalResult<i32> =
            collector.run_if_mature(AnalysisPhase::FullAnalysis, || None);

        assert_eq!(result.source, SignalSource::Unavailable);
        assert_eq!(result.value, None);
    }
}
