//! Ghost Launcher Library
//!
//! This library provides the unified event bus and shared components
//! for the Ghost trading system.
//!
//! ## Event Bus
//!
//! The `events` module provides a unified memory bus using `tokio::sync::broadcast`
//! for inter-component communication.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use ghost_launcher::events::{create_event_bus, GhostEvent, DetectedPool};
//!
//! // Create event bus
//! let (tx, rx) = create_event_bus();
//!
//! // Seer emits events
//! let pool = DetectedPool {
//!     semantic: ghost_core::EventSemanticEnvelope::default(),
//!     pool_amm_id: "pool".to_string(),
//!     base_mint: "mint".to_string(),
//!     quote_mint: "sol".to_string(),
//!     amm_program: "pumpfun".to_string(),
//!     bonding_curve: "curve".to_string(),
//!     creator: "creator".to_string(),
//!     slot: Some(12345),
//!     timestamp_ms: 1700000000000,
//!     event_time: ghost_core::EventTimeMetadata::default(),
//!     detected_wall_ts_ms: Some(1700000000123),
//!     initial_liquidity_sol: Some(10.0),
//!     signature: "sig".to_string(),
//! };
//! tx.send(GhostEvent::new_pool_detected(pool)).unwrap();
//! ```

pub mod ace_core_one_day_probe;
pub mod ace_ev_v2_probe;
pub mod candidate_integrity;
pub mod capture_resilience;
pub mod components;
pub mod config;
pub mod events;
pub mod logging;
pub mod metric_contracts;
pub mod oracle_metrics;
pub mod oracle_runtime;
pub mod rug_reality_capture;
pub mod rug_scalp_v2;
pub mod rug_scalp_validation_tape;
pub mod session;
pub mod tx_intelligence;
pub mod wal_recovery;

#[cfg(test)]
mod pr1e_qualification;

// Re-export commonly used types
pub use events::{
    create_event_bus, create_event_bus_with_capacity, DetectedPool, EventBusReceiver,
    EventBusSender, GhostEvent, PoolScoredEvent, PoolTransaction, TradeResult,
    EVENT_BUS_BUFFER_SIZE,
};

// Re-export oracle runtime types
pub use oracle_runtime::OracleRuntime;
