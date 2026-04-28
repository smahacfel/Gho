//! Event Pipeline — structured event logging for execution analysis.
//!
//! Provides:
//! - `ExecutionEvent` envelope + `EventKind` payloads (see `schema.rs`)
//! - `EventWriter` for JSONL file output with rotation (see `writer.rs`)
//! - `EventEmitter` convenience wrapper for typed event emission (see `emitter.rs`)
//! - `ComparisonReport` generator for paper-vs-live analysis (see `comparison.rs`)
//!
//! Every stage of the pipeline emits its events through this module,
//! enabling offline analysis, join validation, and paper-vs-live comparison.

pub mod comparison;
pub mod emitter;
pub mod schema;
pub mod validator;
pub mod writer;

// Re-exports
pub use comparison::{ComparisonReport, LaneReport, LatencyStats, SlippageStats};
pub use emitter::EventEmitter;
pub use schema::{
    AemTickPayload,
    // Payloads
    CandidatePayload,
    CloseReason,
    ControlCommandAppliedPayload,
    ControlCommandIssuedPayload,
    EntryFilledPayload,
    EntrySubmittedPayload,
    EventEnvelope,
    EventKind,
    ExecutionEvent,
    ExecutionStressChangedPayload,
    ExitFilledPayload,
    ExitSubmittedPayload,
    LedgerDegradedPayload,
    ManagementDecisionPayload,
    ManagementOutcomePayload,
    OracleStalePayload,
    PositionClosedPayload,
    PositionOpenedPayload,
};
pub use validator::{EventValidator, InvariantViolation, ValidatorMetrics};
pub use writer::{EventWriter, EventWriterConfig};
