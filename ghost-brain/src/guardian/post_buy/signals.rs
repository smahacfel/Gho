//! PostBuy Guardian signal types and position health assessment.
//!
//! Defines the communication protocol between the MonitoringEngine
//! and the Signal Router / Revolver integration.

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::execution::backend::Lane;

// ═══════════════════════════════════════════════════════════════════════
// Signal severity & source
// ═══════════════════════════════════════════════════════════════════════

/// Severity level of a guardian signal.
///
/// Ordered from lowest to highest — supports `PartialOrd` comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SignalSeverity {
    /// Informational — log only, no action taken.
    Info,
    /// Warning — may tighten stop-loss via Revolver.
    Warning,
    /// Critical — may trigger panic sell via PanicExecutor.
    Critical,
}

impl SignalSeverity {
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Info => "ℹ️",
            Self::Warning => "⚠️",
            Self::Critical => "🚨",
        }
    }
}

/// Source module that generated the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalSource {
    /// LIGMA — Liquidity Impact Guard & Market Assessment
    Ligma,
    /// WHF — Wash-trading & Harmonic Field detection
    Whf,
    /// TCF — Trend Cohesion Field
    Tcf,
    /// PANIC — Congestion & Impulse Analysis
    Panic,
}

impl SignalSource {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ligma => "LIGMA",
            Self::Whf => "WHF",
            Self::Tcf => "TCF",
            Self::Panic => "PANIC",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Ligma => "💧",
            Self::Whf => "🌀",
            Self::Tcf => "📈",
            Self::Panic => "⚡",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Guardian Signal
// ═══════════════════════════════════════════════════════════════════════

/// A guardian signal emitted by the monitoring engine.
///
/// Signals flow: `MonitoringEngine` → `mpsc::channel` → `SignalRouter` → lane-aware position sink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSignal {
    /// Execution lane that owns the position.
    pub lane: Lane,

    /// Stable position identifier when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,

    /// Base mint of the monitored token.
    pub base_mint: Pubkey,

    /// Pool AMM ID (bonding curve address for pump.fun).
    pub pool_amm_id: Pubkey,

    /// Source module that generated this signal.
    pub source: SignalSource,

    /// Severity determines the action taken by SignalRouter.
    pub severity: SignalSeverity,

    /// Human-readable reason for this signal.
    pub reason: String,

    /// Confidence of the signal (0.0–1.0).
    pub confidence: f32,

    /// Unix timestamp (ms) when signal was generated.
    pub timestamp_ms: u64,

    /// Optional numeric score from the source module.
    pub raw_score: Option<f64>,
}

impl std::fmt::Display for GuardianSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "🛡️{}{} [{}:{}] mint={} | {}",
            self.severity.emoji(),
            self.source.emoji(),
            self.lane,
            self.source.name(),
            &self.base_mint.to_string()[..8],
            self.reason,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Position Health
// ═══════════════════════════════════════════════════════════════════════

/// Aggregated health status for a monitored position.
///
/// Computed on-demand from recent signal history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionHealth {
    /// Overall health score (0.0 = critical, 1.0 = healthy).
    pub health_score: f32,

    /// Current LIGMA tradability (0.0 = untradeable, 1.0 = deep liquidity).
    pub liquidity_tradability: f32,

    /// Current TCF cohesion (0.0 = chaos, 1.0 = strong trend).
    pub trend_cohesion: f32,

    /// Active manipulation detected by WHF?
    pub manipulation_detected: bool,

    /// Panic impulse detected by PANIC module?
    pub panic_impulse_active: bool,

    /// Number of Warning signals in the current aggregation window.
    pub warning_count: u32,

    /// Number of Critical signals in the current aggregation window.
    pub critical_count: u32,

    /// Recommended strategy adjustment for Revolver.
    pub recommended_action: RecommendedAction,
}

impl PositionHealth {
    /// Returns `true` if any module flagged a critical condition.
    pub fn is_critical(&self) -> bool {
        self.critical_count > 0 || self.recommended_action == RecommendedAction::PanicSell
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Recommended Action
// ═══════════════════════════════════════════════════════════════════════

/// Strategy recommendation from Guardian to Revolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendedAction {
    /// Everything looks good — keep current strategy.
    Hold,
    /// Minor concerns — tighten trailing stop.
    TightenStop,
    /// Serious concerns — switch to tight stop loss + prepare exit.
    DefensiveMode,
    /// Critical — immediate exit (panic sell).
    PanicSell,
}

impl RecommendedAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hold => "HOLD",
            Self::TightenStop => "TIGHTEN_STOP",
            Self::DefensiveMode => "DEFENSIVE",
            Self::PanicSell => "PANIC_SELL",
        }
    }
}

impl std::fmt::Display for RecommendedAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(SignalSeverity::Info < SignalSeverity::Warning);
        assert!(SignalSeverity::Warning < SignalSeverity::Critical);
    }

    #[test]
    fn position_health_critical_detection() {
        let health = PositionHealth {
            health_score: 0.3,
            liquidity_tradability: 0.5,
            trend_cohesion: 0.4,
            manipulation_detected: false,
            panic_impulse_active: false,
            warning_count: 2,
            critical_count: 1,
            recommended_action: RecommendedAction::DefensiveMode,
        };
        assert!(health.is_critical());
    }

    #[test]
    fn position_health_not_critical_when_healthy() {
        let health = PositionHealth {
            health_score: 0.9,
            liquidity_tradability: 0.8,
            trend_cohesion: 0.7,
            manipulation_detected: false,
            panic_impulse_active: false,
            warning_count: 0,
            critical_count: 0,
            recommended_action: RecommendedAction::Hold,
        };
        assert!(!health.is_critical());
    }

    #[test]
    fn signal_display_format() {
        let sig = GuardianSignal {
            lane: Lane::Shadow,
            position_id: Some("shadow:1".to_string()),
            base_mint: Pubkey::default(),
            pool_amm_id: Pubkey::default(),
            source: SignalSource::Ligma,
            severity: SignalSeverity::Warning,
            reason: "test reason".to_string(),
            confidence: 0.8,
            timestamp_ms: 1000,
            raw_score: Some(42.0),
        };
        let display = format!("{}", sig);
        assert!(display.contains("LIGMA"));
        assert!(display.contains("shadow"));
        assert!(display.contains("test reason"));
    }
}
