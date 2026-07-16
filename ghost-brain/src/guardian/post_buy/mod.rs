//! PostBuy Guardian — Real-time position monitoring layer.
//!
//! Monitors purchased positions using 4 lightweight analytical modules:
//! - **LIGMA** — Liquidity depth & tradability assessment
//! - **WHF** — Wash trading & bot manipulation detection
//! - **TCF** — Trend cohesion & regime change detection
//! - **PANIC** — Congestion impulse & coordinated sell-off detection
//!
//! ## Data Flow
//!
//! ```text
//! Pipeline (BUY success)
//!   → MonitoringEngine::register_position()
//!
//! AccountStateCore (canonical snapshots)
//!   → MonitoringEngine::tick() every N ms
//!     → immutable PostBuyDecisionSnapshot
//!       → pure ExitPolicyV1
//!         → lazy full-position executable quote
//!           → guarded shadow outcome apply
//!             → typed terminal notification
//!
//! LIGMA, WHF, TCF and PANIC continue to emit evidence. In the active launcher
//! their SignalRouter is observation-only and cannot mutate the position or
//! virtual magazine.
//! ```

pub mod config;
pub mod engine;
mod exit_policy_v1;
mod exit_policy_v2;
pub mod exit_replay;
pub mod integration;
pub mod shadow_v2;
pub mod shadow_v2_execution;
pub mod signals;
mod trajectory_v1;

pub use crate::events::ShadowUnresolvedReason;
pub use config::{
    CrashGuardMode, ExitPolicyV1Config, HetPmV2Config, HetPmV2Mode, PostBuyGuardianConfig,
    ShadowExitReplayConfig, TimeStopV2Config,
};
pub use engine::{
    MonitoringEngine, MonitoringEngineConfigError, RegisteredShadowPosition,
    ShadowTerminalDisposition,
};
pub use exit_policy_v1::{
    validate_exit_policy_v1_config, ExitPolicyConfigError, ExitPolicyV1Status,
};
pub use exit_policy_v2::{validate_het_pm_v2_config, HetPmV2ConfigError, HetPmV2Status};
pub use integration::{PositionRuntimeRouter, ShadowPositionBook, SignalRouter};
pub use signals::{
    GuardianSignal, PositionHealth, RecommendedAction, SignalSeverity, SignalSource,
};
