//! Pool State SSOT (Single Source of Truth)
//!
//! This module provides a unified, production-grade pool state layer that
//! tracks pool state in real time for BOTH phases:
//! - **Bonding Curve** (pump.fun) — virtual reserves pricing
//! - **AMM** (post-migration) — real reserves constant-product pricing
//!
//! ## Architecture
//!
//! - [`PoolPhase`] — bonding vs AMM phase enum with one-way switch
//! - [`PoolSnapshot`] — per-pool SSOT object with atomic updates
//! - [`SnapshotStore`] — concurrent store keyed by pool/mint
//! - [`QuoteEngine`] — executability quotes (expected_out, effective_price, min_out)
//! - [`SsotConfig`] — configuration knobs (staleness, slippage, fees)
//! - Staleness guard — ORACLE_STALE emission when stale
//! - Logging proof — structured logging on every snapshot update

pub mod config;
pub mod phase;
pub mod quote_engine;
pub mod snapshot;
pub mod store;
pub mod yellowstone;

pub use config::SsotConfig;
pub use phase::PoolPhase;
pub use quote_engine::{Quote, QuoteEngine, QuoteSide};
pub use snapshot::{PoolSnapshot, SnapshotSource};
pub use store::{SnapshotStore, SsotMetrics};
pub use yellowstone::{
    parse_bonding_curve_raw, parse_token_account_amount, AccountRole, SubscriberDiagnostics,
    TokenParseError, YellowstoneSubscriber,
};
