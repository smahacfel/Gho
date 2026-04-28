//! Stub module for deprecated wave builder functions
//!
//! **DEPRECATED**: These functions are stubs for backward compatibility.
//! Wave building for QASS has been replaced by direct SurvivorScore integration.
//!
//! These functions return neutral/default waves that have minimal impact on scoring.
//! New code should use SurvivorScore components directly.

use super::iwim::IwimResult;
use super::praecog::PraecogResult;
use super::qass_stub::HeuristicWave;
use super::ssmi::SsmiResult;
use crate::oracle::cluster_hunter::ClusterAnalysis;
use crate::oracle::profiler::DevProfile;
use crate::oracle::vision_critic::VisionCriticResult;
use crate::signals::ligma::LigmaResult;

// =============================================================================
// Constants for Alert Thresholds (deprecated)
// =============================================================================

/// Viral launch detection threshold
pub const VIRAL_LAUNCH_THRESHOLD: f64 = 0.85;
/// Strong buy signal threshold
pub const STRONG_BUY_THRESHOLD: f64 = 0.75;
/// Moderate opportunity threshold
pub const MODERATE_OPPORTUNITY_THRESHOLD: f64 = 0.65;
/// Alert confidence threshold
pub const ALERT_CONFIDENCE_THRESHOLD: f64 = 0.7;

// =============================================================================
// Alert Types (deprecated)
// =============================================================================

/// Alert level for viral launch detection (deprecated)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    /// Viral launch detected
    Viral,
    /// Strong buying signal
    Strong,
    /// Moderate opportunity
    Moderate,
    /// No alert
    None,
}

impl Default for AlertLevel {
    fn default() -> Self {
        AlertLevel::None
    }
}

/// Viral launch alert (deprecated)
#[derive(Debug, Clone, Default)]
pub struct ViralLaunchAlert {
    /// Alert level
    pub level: AlertLevel,
    /// Score (0.0-1.0)
    pub score: f64,
    /// Confidence
    pub confidence: f64,
}

// =============================================================================
// Oracle Signals Container (deprecated)
// =============================================================================

/// Container for oracle signals (deprecated)
#[derive(Debug, Clone, Default)]
pub struct OracleSignals {
    /// SSMI result
    pub ssmi: Option<SsmiResult>,
    /// SCR score
    pub scr_score: Option<f64>,
    /// Transaction count for SCR
    pub tx_count: Option<usize>,
    /// ULVF divergence
    pub ulvf_divergence: Option<f64>,
    /// ULVF curl
    pub ulvf_curl: Option<f64>,
    /// POVC cluster
    pub povc_cluster: Option<usize>,
    /// Cluster analysis
    pub cluster: Option<ClusterAnalysis>,
    /// Dev profile
    pub profiler: Option<DevProfile>,
    /// Vision critic result
    pub vision: Option<VisionCriticResult>,
    /// Shadow bonding progress
    pub shadow_progress: Option<u64>,
    /// Shadow price ratio
    pub shadow_price_ratio: Option<f64>,
    /// IWIM result
    pub iwim: Option<IwimResult>,
    /// PRAECOG result
    pub praecog: Option<PraecogResult>,
    /// LIGMA result
    pub ligma: Option<LigmaResult>,
}

// =============================================================================
// Wave Builder Functions (all return neutral/default waves)
// =============================================================================

/// Build SSMI wave (deprecated - returns neutral wave)
pub fn build_ssmi_wave(_result: &SsmiResult) -> HeuristicWave {
    HeuristicWave::new("ψ_ssmi", 0.5, 0.0, 0.0)
}

/// Build SCR wave (deprecated - returns neutral wave)
pub fn build_scr_wave(_scr_score: f64, _tx_count: usize) -> HeuristicWave {
    HeuristicWave::new("ψ_scr", 0.5, 0.0, 0.0)
}

/// Build ULVF wave (deprecated - returns neutral wave)
pub fn build_ulvf_wave(_divergence: f64, _curl: f64) -> HeuristicWave {
    HeuristicWave::new("ψ_ulvf", 0.5, 0.0, 0.0)
}

/// Build POVC wave (deprecated - returns neutral wave)
pub fn build_povc_wave(_cluster_idx: usize) -> HeuristicWave {
    HeuristicWave::new("ψ_povc", 0.5, 0.0, 0.0)
}

/// Build cluster wave (deprecated - returns neutral wave)
pub fn build_cluster_wave(_analysis: &ClusterAnalysis) -> HeuristicWave {
    HeuristicWave::new("ψ_cluster", 0.5, 0.0, 0.0)
}

/// Build profiler wave (deprecated - returns neutral wave)
pub fn build_profiler_wave(_profile: &DevProfile) -> HeuristicWave {
    HeuristicWave::new("ψ_profiler", 0.5, 0.0, 0.0)
}

/// Build vision wave (deprecated - returns None)
pub fn build_vision_wave(_result: &VisionCriticResult) -> Option<HeuristicWave> {
    None
}

/// Build shadow wave (deprecated - returns neutral wave)
pub fn build_shadow_wave(
    _bonding_progress: Option<u64>,
    _price_ratio: Option<f64>,
) -> HeuristicWave {
    HeuristicWave::new("ψ_shadow", 0.5, 0.0, 0.0)
}

/// Build IWIM wave (deprecated - returns neutral wave)
pub fn build_iwim_wave(_result: &IwimResult) -> HeuristicWave {
    HeuristicWave::new("ψ_iwim", 0.5, 0.0, 0.0)
}

/// Build PRAECOG wave (deprecated - returns neutral wave)
pub fn build_praecog_wave(_result: &PraecogResult) -> HeuristicWave {
    HeuristicWave::new("ψ_praecog", 0.5, 0.0, 0.0)
}

/// Build LIGMA wave (deprecated - returns neutral wave)
pub fn build_ligma_wave(_result: &LigmaResult) -> HeuristicWave {
    HeuristicWave::new("ψ_ligma", 0.5, 0.0, 0.0)
}

/// Build all waves from signals (deprecated - returns empty vector)
///
/// **DEPRECATED**: Use SurvivorScore components directly.
pub fn build_waves_from_signals(_signals: &OracleSignals) -> Vec<HeuristicWave> {
    vec![]
}
