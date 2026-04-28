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
//! ShadowLedger (continuous snapshots)
//!   → MonitoringEngine::tick() every N ms
//!     → [LIGMA, WHF, TCF, PANIC] checks
//!       → GuardianSignal (via mpsc channel)
//!         → SignalRouter
//!           → lane-aware position sink (live Revolver / shadow virtual magazine)
//!
//! Pipeline (SELL / position closed)
//!   → MonitoringEngine::unregister_position()
//! ```

pub mod config;
pub mod engine;
pub mod integration;
pub mod signals;

pub use config::PostBuyGuardianConfig;
pub use engine::MonitoringEngine;
pub use integration::{PositionRuntimeRouter, ShadowPositionBook, SignalRouter};
pub use signals::{
    GuardianSignal, PositionHealth, RecommendedAction, SignalSeverity, SignalSource,
};
