//! MAR (Mechanism-Aware Rug Guard) public API.
//!
//! ## PRE/POST usage contract
//!
//! - **PRE-BUY (Gatekeeper)**: consult [`MarketExploitabilityState`] before allowing entry.
//!   `ExecutionReady` or `InvalidCoverage` must deny entry.
//! - **POST-BUY (Guardian)**: monitor transitions to `ExecutionReady` for open positions.
//!   `InvalidCoverage` handling must follow [`MarConfig::fail_closed_on_invalid_coverage`].

pub mod engine;
pub mod execution_cost;
pub mod impact;
pub mod perturbation;
pub mod supply_snapshot;
pub mod types;

pub use types::{MarConfig, MarMetricsSnapshot, MarPoolReserves, MarketExploitabilityState};
