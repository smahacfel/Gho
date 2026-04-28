//! Execution Backend Abstraction Layer
//!
//! This module provides the `ExecutionMode` SSOT switch and `ExecutionBackend` trait
//! that isolates the pipeline from knowing whether execution is live, paper, or dual.
//!
//! # Architecture
//!
//! ```text
//! Pipeline (Gatekeeper → AEM → Revolver)
//!     ↓ (talks only to ExecutionBackend trait)
//! ┌───────────────────────────────────┐
//! │ match execution_mode {            │  ← ONE branch at startup
//! │   Live  → LiveBackend             │
//! │   Paper → PaperBackend            │
//! │   Dual  → DualBackend             │
//! │ }                                 │
//! └───────────────────────────────────┘
//! ```

pub mod backend;
pub mod dual;
pub mod live;
pub mod paper;
pub mod paper_lifecycle;
pub mod shadow;

// Re-exports
pub use backend::*;
pub use dual::{DualBackend, DualBackendConfig};
pub use live::LiveBackend;
pub use paper::{PaperBackend, PaperBroker, PaperBrokerConfig, StressTransition};
pub use paper_lifecycle::{PaperLifecycleConfig, PaperPositionLifecycle};
pub use shadow::{ShadowBackend, ShadowPositionState};
