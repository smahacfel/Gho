//! Shadow Burnin Simulation V2 contract types.
//!
//! These types are intentionally inert. PR1 defines the schema and validation
//! vocabulary only; no runtime writer, lifecycle path, replay path, BUY/REJECT
//! policy, selector, TX/Jito path, shadow_close_only path, or active close path
//! consumes these records yet.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use ghost_core::account_state_core::types::CanonicalPoolState;
use serde::{Deserialize, Serialize};

pub const SHADOW_V2_SIMULATION_CONTRACT_VERSION: &str = "shadow_burnin_simulation_v2_20260629";
pub const EVENT_ORDER_UNKNOWN_INDEX: u32 = u32::MAX;
pub const EVENT_ORDER_UNKNOWN_SIGNATURE: &str = "UNKNOWN";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SimulationLevel {
    MarkOnly,
    FillModelStatic,
    FillModelCalibrated,
    LiveConfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MeasurementGrade {
    DiagnosticOnly,
    MarkPriceReplay,
    ResearchGradeCandidate,
    ShadowV2ResearchGrade,
    ShadowV2ResearchGradeOnly,
    ShadowV2LiveEquivalenceCandidate,
    ShadowV2LiveEquivalenceGrade,
    BlockedByData,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TemporalClass {
    PreDetection,
    PreDecision,
    AtDecision,
    PostEntry,
    PostExit,
    Outcome,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClockDomain {
    WallClockMs,
    MonotonicProcessMs,
    ChainSlot,
    BlockTime,
    StreamObservedMs,
    RpcObservedMs,
    DecisionTsMs,
    SubmitTsMs,
    LandingTsMs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FillStatus {
    Filled,
    NoFill,
    Failed,
    BlockedByData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PoolStateSource {
    YellowstoneEvent,
    AccountStateCore,
    RpcFallback,
    ShadowLedgerDiagnostic,
    ReconstructedFromTradeEvent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalReasonV2 {
    Target,
    Stop,
    Timeout,
    NoFill,
    Failed,
    BlockedByData,
    ManualDiagnosticClose,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventOrderUnknown {
    Unknown,
}

/// Typed chain-order component used by `EventOrderKey`.
///
/// Serialization intentionally preserves the schema contract shape: known
/// numeric/string components serialize as their raw value, and missing chain
/// ordering serializes as the literal `UNKNOWN`. A missing JSON field is a
/// schema error instead of an implicit unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventOrderComponent<T> {
    Unknown(EventOrderUnknown),
    Known(T),
}

impl<T> EventOrderComponent<T> {
    pub fn known(value: T) -> Self {
        Self::Known(value)
    }

    pub fn unknown() -> Self {
        Self::Unknown(EventOrderUnknown::Unknown)
    }

    pub fn as_known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventOrderKey {
    pub slot: EventOrderComponent<u64>,
    pub block_time: EventOrderComponent<i64>,
    pub signature: EventOrderComponent<String>,
    pub transaction_index_or_unknown: EventOrderComponent<u32>,
    pub instruction_index_or_unknown: EventOrderComponent<u32>,
    pub inner_instruction_index_or_unknown: EventOrderComponent<u32>,
    pub log_index_or_unknown: EventOrderComponent<u32>,
    pub event_seq_in_process: u64,
    pub observed_at_wall_ms: u64,
}

impl EventOrderKey {
    pub fn missing_chain_order_components(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if matches!(&self.signature, EventOrderComponent::Known(signature) if signature.trim().is_empty())
        {
            missing.push("signature");
        }
        missing
    }

    pub fn explicit_unknown_chain_order_components(&self) -> Vec<&'static str> {
        let mut unknown = Vec::new();
        if self.slot.is_unknown() {
            unknown.push("slot");
        }
        if self.block_time.is_unknown() {
            unknown.push("block_time");
        }
        if self.signature.is_unknown() {
            unknown.push("signature");
        }
        if self.transaction_index_or_unknown.is_unknown() {
            unknown.push("transaction_index_or_unknown");
        }
        if self.instruction_index_or_unknown.is_unknown() {
            unknown.push("instruction_index_or_unknown");
        }
        if self.inner_instruction_index_or_unknown.is_unknown() {
            unknown.push("inner_instruction_index_or_unknown");
        }
        if self.log_index_or_unknown.is_unknown() {
            unknown.push("log_index_or_unknown");
        }
        unknown
    }

    pub fn has_explicit_unknown_chain_order(&self) -> bool {
        !self.explicit_unknown_chain_order_components().is_empty()
    }

    pub fn ambiguity_labels(&self) -> Vec<String> {
        let unknown = self.explicit_unknown_chain_order_components();
        if unknown.is_empty() {
            return Vec::new();
        }
        vec![
            "EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT".to_string(),
            format!("EVENT_ORDER_UNKNOWN_COMPONENTS={}", unknown.join("|")),
            "EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string(),
        ]
    }

    pub fn has_complete_chain_order(&self) -> bool {
        self.slot.as_known().is_some()
            && matches!(&self.signature, EventOrderComponent::Known(signature) if !signature.trim().is_empty())
            && self.transaction_index_or_unknown.as_known().is_some()
            && self.instruction_index_or_unknown.as_known().is_some()
            && self.inner_instruction_index_or_unknown.as_known().is_some()
            && self.log_index_or_unknown.as_known().is_some()
    }

    pub fn same_slot_ambiguous_with(&self, other: &Self) -> bool {
        matches!(
            (self.slot.as_known(), other.slot.as_known()),
            (Some(lhs), Some(rhs)) if lhs == rhs
        ) && (!self.has_complete_chain_order() || !other.has_complete_chain_order())
    }

    pub fn is_after_process_seq(&self, previous_seq: u64) -> bool {
        self.event_seq_in_process > previous_seq
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockedTimestamp {
    pub field_name: String,
    pub value: Option<i64>,
    pub clock_domain: ClockDomain,
    pub clock_source: String,
    pub causal_boundary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2Envelope {
    pub schema: String,
    pub schema_version: u32,
    pub simulation_contract_version: String,
    pub simulation_level: SimulationLevel,
    pub measurement_grade: MeasurementGrade,
    pub run_id: String,
    pub session_id: Option<String>,
    pub candidate_id: Option<String>,
    pub position_id: String,
    pub event_id: String,
    pub parent_event_id: Option<String>,
    pub source_event_id: Option<String>,
    pub pool_id: String,
    pub base_mint: String,
    pub bonding_curve: Option<String>,
    pub produced_at_ms: u64,
    pub produced_at_slot: Option<u64>,
    pub temporal_class: TemporalClass,
    pub clock_domain: ClockDomain,
    pub source_refs: Vec<String>,
    pub quality: String,
    pub limitations: Vec<String>,
}

impl ShadowV2Envelope {
    pub fn contract_header(
        schema: impl Into<String>,
        run_id: impl Into<String>,
        position_id: impl Into<String>,
        event_id: impl Into<String>,
        pool_id: impl Into<String>,
        base_mint: impl Into<String>,
    ) -> Self {
        Self {
            schema: schema.into(),
            schema_version: 1,
            simulation_contract_version: SHADOW_V2_SIMULATION_CONTRACT_VERSION.to_string(),
            simulation_level: SimulationLevel::MarkOnly,
            measurement_grade: MeasurementGrade::DiagnosticOnly,
            run_id: run_id.into(),
            session_id: None,
            candidate_id: None,
            position_id: position_id.into(),
            event_id: event_id.into(),
            parent_event_id: None,
            source_event_id: None,
            pool_id: pool_id.into(),
            base_mint: base_mint.into(),
            bonding_curve: None,
            produced_at_ms: 0,
            produced_at_slot: None,
            temporal_class: TemporalClass::Unknown,
            clock_domain: ClockDomain::WallClockMs,
            source_refs: Vec::new(),
            quality: "DIAGNOSTIC_ONLY".to_string(),
            limitations: Vec::new(),
        }
    }

    pub fn exact_join_key(&self) -> Result<ShadowV2ExactJoinKey, ShadowV2Error> {
        let session_id =
            self.session_id
                .clone()
                .ok_or_else(|| ShadowV2Error::MissingExactJoinKey {
                    event_id: self.event_id.clone(),
                    missing_field: "session_id",
                })?;
        ShadowV2ExactJoinKey::new(
            self.run_id.clone(),
            session_id,
            self.position_id.clone(),
            self.pool_id.clone(),
            self.base_mint.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowPositionV2 {
    pub envelope: ShadowV2Envelope,
    pub created_at_wall_ms: ClockedTimestamp,
    pub created_at_slot: Option<u64>,
    pub decision_id: Option<String>,
    pub strategy_context: Option<String>,
    pub lane: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowPositionEventKindV2 {
    PositionCreated,
    PoolStateSample,
    EntryDecision,
    EntryAttempt,
    EntryFill,
    PathSample,
    ExitAttempt,
    ExitFill,
    TerminalTruth,
    ReplayDerived,
    LifecycleSubEvent,
}

impl ShadowPositionEventKindV2 {
    pub const fn is_canonical_terminal(self) -> bool {
        matches!(self, Self::TerminalTruth)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowPositionEventV2 {
    pub envelope: ShadowV2Envelope,
    pub event_kind: ShadowPositionEventKindV2,
    pub event_order_key: Option<EventOrderKey>,
    pub canonical_payload_schema: String,
    pub canonical_payload_event_id: String,
    pub canonical_terminal_event_id: Option<String>,
    pub payload: serde_json::Value,
}

impl ShadowPositionEventV2 {
    pub fn from_record(record: ShadowV2Record) -> Result<Self, ShadowV2Error> {
        let envelope = record.envelope().clone();
        let event_kind = record.event_kind();
        let canonical_payload_schema = envelope.schema.clone();
        let canonical_payload_event_id = envelope.event_id.clone();
        let event_order_key = record.event_order_key().cloned();
        let canonical_terminal_event_id = event_kind
            .is_canonical_terminal()
            .then(|| envelope.event_id.clone());
        let payload = serde_json::to_value(&record).map_err(ShadowV2Error::from)?;

        Ok(Self {
            envelope,
            event_kind,
            event_order_key,
            canonical_payload_schema,
            canonical_payload_event_id,
            canonical_terminal_event_id,
            payload,
        })
    }

    pub fn is_canonical_terminal(&self) -> bool {
        self.event_kind.is_canonical_terminal()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record_type", content = "record", rename_all = "snake_case")]
pub enum ShadowV2Record {
    ShadowPositionV2(ShadowPositionV2),
    PoolStateSampleV2(PoolStateSampleV2),
    ShadowEntryDecisionV2(ShadowEntryDecisionV2),
    ShadowEntryAttemptV2(ShadowEntryAttemptV2),
    ShadowEntryFillV2(ShadowEntryFillV2),
    ShadowPathSampleV2(ShadowPathSampleV2),
    ShadowExitAttemptV2(ShadowExitAttemptV2),
    ShadowExitFillV2(ShadowExitFillV2),
    ShadowTerminalTruthV2(ShadowTerminalTruthV2),
    ShadowReplayV2(ShadowReplayV2),
}

impl ShadowV2Record {
    pub fn envelope(&self) -> &ShadowV2Envelope {
        match self {
            Self::ShadowPositionV2(record) => &record.envelope,
            Self::PoolStateSampleV2(record) => &record.envelope,
            Self::ShadowEntryDecisionV2(record) => &record.envelope,
            Self::ShadowEntryAttemptV2(record) => &record.envelope,
            Self::ShadowEntryFillV2(record) => &record.envelope,
            Self::ShadowPathSampleV2(record) => &record.envelope,
            Self::ShadowExitAttemptV2(record) => &record.envelope,
            Self::ShadowExitFillV2(record) => &record.envelope,
            Self::ShadowTerminalTruthV2(record) => &record.envelope,
            Self::ShadowReplayV2(record) => &record.envelope,
        }
    }

    pub fn event_kind(&self) -> ShadowPositionEventKindV2 {
        match self {
            Self::ShadowPositionV2(_) => ShadowPositionEventKindV2::PositionCreated,
            Self::PoolStateSampleV2(_) => ShadowPositionEventKindV2::PoolStateSample,
            Self::ShadowEntryDecisionV2(_) => ShadowPositionEventKindV2::EntryDecision,
            Self::ShadowEntryAttemptV2(_) => ShadowPositionEventKindV2::EntryAttempt,
            Self::ShadowEntryFillV2(_) => ShadowPositionEventKindV2::EntryFill,
            Self::ShadowPathSampleV2(_) => ShadowPositionEventKindV2::PathSample,
            Self::ShadowExitAttemptV2(_) => ShadowPositionEventKindV2::ExitAttempt,
            Self::ShadowExitFillV2(_) => ShadowPositionEventKindV2::ExitFill,
            Self::ShadowTerminalTruthV2(_) => ShadowPositionEventKindV2::TerminalTruth,
            Self::ShadowReplayV2(_) => ShadowPositionEventKindV2::ReplayDerived,
        }
    }

    pub fn event_order_key(&self) -> Option<&EventOrderKey> {
        match self {
            Self::PoolStateSampleV2(record) => Some(&record.event_order_key),
            Self::ShadowEntryAttemptV2(record) => Some(&record.event_order_key),
            Self::ShadowEntryFillV2(record) => Some(&record.event_order_key),
            Self::ShadowPathSampleV2(record) => Some(&record.event_order_key),
            Self::ShadowExitAttemptV2(record) => Some(&record.event_order_key),
            Self::ShadowExitFillV2(record) => Some(&record.event_order_key),
            Self::ShadowPositionV2(_)
            | Self::ShadowEntryDecisionV2(_)
            | Self::ShadowTerminalTruthV2(_)
            | Self::ShadowReplayV2(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowV2Error {
    EmptyPositionId {
        event_id: String,
    },
    EmptyEventId {
        position_id: String,
    },
    DuplicateEventId {
        event_id: String,
    },
    DuplicateTerminalTruth {
        position_id: String,
        existing_terminal_event_id: String,
        attempted_terminal_event_id: String,
    },
    NonMonotonicEventSequence {
        run_id: String,
        previous_seq: u64,
        attempted_seq: u64,
    },
    MissingExactJoinKey {
        event_id: String,
        missing_field: &'static str,
    },
    AmbiguousExactJoinKey {
        key: ShadowV2ExactJoinKey,
        existing_event_id: String,
        attempted_event_id: String,
    },
    FallbackJoinDisallowed {
        reason: String,
    },
    PoolStateBlocked {
        event_id: String,
        blockers: Vec<String>,
    },
    JsonlIndex {
        path: String,
        line_number: usize,
        error: String,
    },
    Io(String),
    Serialization(String),
}

impl fmt::Display for ShadowV2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPositionId { event_id } => {
                write!(f, "shadow v2 event {event_id} has empty position_id")
            }
            Self::EmptyEventId { position_id } => {
                write!(f, "shadow v2 position {position_id} has empty event_id")
            }
            Self::DuplicateEventId { event_id } => {
                write!(f, "shadow v2 duplicate event_id {event_id}")
            }
            Self::DuplicateTerminalTruth {
                position_id,
                existing_terminal_event_id,
                attempted_terminal_event_id,
            } => write!(
                f,
                "shadow v2 position {position_id} already has terminal event {existing_terminal_event_id}; attempted {attempted_terminal_event_id}"
            ),
            Self::NonMonotonicEventSequence {
                run_id,
                previous_seq,
                attempted_seq,
            } => write!(
                f,
                "shadow v2 run {run_id} non-monotonic event_seq_in_process: previous={previous_seq}, attempted={attempted_seq}"
            ),
            Self::MissingExactJoinKey {
                event_id,
                missing_field,
            } => write!(
                f,
                "shadow v2 event {event_id} missing exact join key field {missing_field}"
            ),
            Self::AmbiguousExactJoinKey {
                key,
                existing_event_id,
                attempted_event_id,
            } => write!(
                f,
                "shadow v2 ambiguous exact join key {key:?}: existing={existing_event_id}, attempted={attempted_event_id}"
            ),
            Self::FallbackJoinDisallowed { reason } => {
                write!(f, "shadow v2 fallback join disallowed: {reason}")
            }
            Self::PoolStateBlocked { event_id, blockers } => {
                write!(f, "shadow v2 pool state sample {event_id} blocked: {blockers:?}")
            }
            Self::JsonlIndex {
                path,
                line_number,
                error,
            } => write!(
                f,
                "shadow v2 jsonl index error in {path} at line {line_number}: {error}"
            ),
            Self::Io(error) => write!(f, "shadow v2 io error: {error}"),
            Self::Serialization(error) => write!(f, "shadow v2 serialization error: {error}"),
        }
    }
}

impl std::error::Error for ShadowV2Error {}

impl From<io::Error> for ShadowV2Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for ShadowV2Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShadowV2ExactJoinKey {
    pub run_id: String,
    pub session_id: String,
    pub position_id: String,
    pub pool_id: String,
    pub base_mint: String,
}

impl ShadowV2ExactJoinKey {
    pub fn new(
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        position_id: impl Into<String>,
        pool_id: impl Into<String>,
        base_mint: impl Into<String>,
    ) -> Result<Self, ShadowV2Error> {
        let key = Self {
            run_id: run_id.into(),
            session_id: session_id.into(),
            position_id: position_id.into(),
            pool_id: pool_id.into(),
            base_mint: base_mint.into(),
        };
        if key.run_id.trim().is_empty() {
            return Err(ShadowV2Error::MissingExactJoinKey {
                event_id: "exact_join_key".to_string(),
                missing_field: "run_id",
            });
        }
        if key.session_id.trim().is_empty() {
            return Err(ShadowV2Error::MissingExactJoinKey {
                event_id: "exact_join_key".to_string(),
                missing_field: "session_id",
            });
        }
        if key.position_id.trim().is_empty() {
            return Err(ShadowV2Error::MissingExactJoinKey {
                event_id: "exact_join_key".to_string(),
                missing_field: "position_id",
            });
        }
        if key.pool_id.trim().is_empty() {
            return Err(ShadowV2Error::MissingExactJoinKey {
                event_id: "exact_join_key".to_string(),
                missing_field: "pool_id",
            });
        }
        if key.base_mint.trim().is_empty() {
            return Err(ShadowV2Error::MissingExactJoinKey {
                event_id: "exact_join_key".to_string(),
                missing_field: "base_mint",
            });
        }
        Ok(key)
    }
}

/// Position-level terminal-truth join guard. This is not an event-level index:
/// multiple non-terminal canonical events may share the same exact position key.
#[derive(Debug, Default, Clone)]
pub struct ShadowV2ExactJoinIndex {
    terminal_event_id_by_key: HashMap<ShadowV2ExactJoinKey, String>,
}

impl ShadowV2ExactJoinIndex {
    pub fn insert_terminal(&mut self, envelope: &ShadowV2Envelope) -> Result<(), ShadowV2Error> {
        let key = envelope.exact_join_key()?;
        if let Some(existing_event_id) = self.terminal_event_id_by_key.get(&key) {
            return Err(ShadowV2Error::AmbiguousExactJoinKey {
                key,
                existing_event_id: existing_event_id.clone(),
                attempted_event_id: envelope.event_id.clone(),
            });
        }
        self.terminal_event_id_by_key
            .insert(key, envelope.event_id.clone());
        Ok(())
    }

    pub fn fallback_join_disallowed(reason: impl Into<String>) -> ShadowV2Error {
        ShadowV2Error::FallbackJoinDisallowed {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ShadowV2CanonicalEventStream {
    events: Vec<ShadowPositionEventV2>,
    seen_event_ids: HashSet<String>,
    terminal_event_by_position: HashMap<String, String>,
    last_process_seq_by_run: HashMap<String, u64>,
}

impl ShadowV2CanonicalEventStream {
    pub fn append_record(
        &mut self,
        record: ShadowV2Record,
    ) -> Result<&ShadowPositionEventV2, ShadowV2Error> {
        let event = self.prepare_record(record)?;
        self.commit_prepared_event(event)
    }

    pub fn prepare_record(
        &self,
        record: ShadowV2Record,
    ) -> Result<ShadowPositionEventV2, ShadowV2Error> {
        let event = ShadowPositionEventV2::from_record(record)?;
        self.validate_event(&event)?;
        Ok(event)
    }

    pub fn append_indexed_event(
        &mut self,
        event: ShadowPositionEventV2,
    ) -> Result<&ShadowPositionEventV2, ShadowV2Error> {
        self.commit_prepared_event(event)
    }

    pub fn commit_prepared_event(
        &mut self,
        event: ShadowPositionEventV2,
    ) -> Result<&ShadowPositionEventV2, ShadowV2Error> {
        self.validate_event(&event)?;
        self.commit_event(event);
        self.events.last().ok_or_else(|| {
            ShadowV2Error::Io(
                "shadow v2 invariant violation: committed stream has no last event".to_string(),
            )
        })
    }

    pub fn events(&self) -> &[ShadowPositionEventV2] {
        &self.events
    }

    pub fn terminal_event_id(&self, position_id: &str) -> Option<&str> {
        self.terminal_event_by_position
            .get(position_id)
            .map(String::as_str)
    }

    fn validate_event(&self, event: &ShadowPositionEventV2) -> Result<(), ShadowV2Error> {
        if event.envelope.position_id.is_empty() {
            return Err(ShadowV2Error::EmptyPositionId {
                event_id: event.envelope.event_id.clone(),
            });
        }
        if event.envelope.event_id.is_empty() {
            return Err(ShadowV2Error::EmptyEventId {
                position_id: event.envelope.position_id.clone(),
            });
        }
        if self.seen_event_ids.contains(&event.envelope.event_id) {
            return Err(ShadowV2Error::DuplicateEventId {
                event_id: event.envelope.event_id.clone(),
            });
        }
        if event.is_canonical_terminal() {
            if let Some(existing_terminal_event_id) = self
                .terminal_event_by_position
                .get(&event.envelope.position_id)
            {
                return Err(ShadowV2Error::DuplicateTerminalTruth {
                    position_id: event.envelope.position_id.clone(),
                    existing_terminal_event_id: existing_terminal_event_id.clone(),
                    attempted_terminal_event_id: event.envelope.event_id.clone(),
                });
            }
        }
        if let Some(order_key) = event.event_order_key.as_ref() {
            if let Some(previous_seq) = self.last_process_seq_by_run.get(&event.envelope.run_id) {
                if !order_key.is_after_process_seq(*previous_seq) {
                    return Err(ShadowV2Error::NonMonotonicEventSequence {
                        run_id: event.envelope.run_id.clone(),
                        previous_seq: *previous_seq,
                        attempted_seq: order_key.event_seq_in_process,
                    });
                }
            }
        }
        Ok(())
    }

    fn commit_event(&mut self, event: ShadowPositionEventV2) {
        if let Some(order_key) = event.event_order_key.as_ref() {
            self.last_process_seq_by_run.insert(
                event.envelope.run_id.clone(),
                order_key.event_seq_in_process,
            );
        }
        if event.is_canonical_terminal() {
            self.terminal_event_by_position.insert(
                event.envelope.position_id.clone(),
                event.envelope.event_id.clone(),
            );
        }
        self.seen_event_ids.insert(event.envelope.event_id.clone());
        self.events.push(event);
    }
}

#[derive(Debug)]
pub struct JsonlShadowV2CanonicalWriter {
    path: PathBuf,
    stream: ShadowV2CanonicalEventStream,
}

impl JsonlShadowV2CanonicalWriter {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ShadowV2Error> {
        let path = path.into();
        let stream = index_existing_jsonl_stream(&path)?;
        Ok(Self { path, stream })
    }

    pub fn append_record(&mut self, record: ShadowV2Record) -> Result<(), ShadowV2Error> {
        let event = self.stream.prepare_record(record)?;
        append_jsonl_record(&self.path, &event)?;
        self.stream.commit_prepared_event(event)?;
        Ok(())
    }

    pub fn stream(&self) -> &ShadowV2CanonicalEventStream {
        &self.stream
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn index_existing_jsonl_stream(path: &Path) -> Result<ShadowV2CanonicalEventStream, ShadowV2Error> {
    let mut stream = ShadowV2CanonicalEventStream::default();
    if !path.exists() {
        return Ok(stream);
    }
    if path.is_dir() {
        return Err(ShadowV2Error::Io(format!(
            "shadow v2 canonical writer path is a directory: {}",
            path.display()
        )));
    }

    let file = File::open(path)?;
    for (line_index, line_result) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line_result.map_err(|error| ShadowV2Error::JsonlIndex {
            path: path.display().to_string(),
            line_number,
            error: error.to_string(),
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event: ShadowPositionEventV2 =
            serde_json::from_str(line.trim()).map_err(|error| ShadowV2Error::JsonlIndex {
                path: path.display().to_string(),
                line_number,
                error: error.to_string(),
            })?;
        stream
            .append_indexed_event(event)
            .map_err(|error| ShadowV2Error::JsonlIndex {
                path: path.display().to_string(),
                line_number,
                error: error.to_string(),
            })?;
    }
    Ok(stream)
}

fn append_jsonl_record(path: &Path, value: &impl Serialize) -> Result<(), ShadowV2Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_data()?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolStateSampleV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub observed_at_wall_ms: u64,
    pub observed_slot: Option<u64>,
    pub block_time: Option<i64>,
    pub source: PoolStateSource,
    pub commitment: Option<String>,
    pub event_signature: Option<String>,
    pub event_index: Option<u32>,
    pub account_data_hash: Option<String>,
    pub virtual_sol_reserves: Option<u64>,
    pub virtual_token_reserves: Option<u64>,
    pub real_sol_reserves: Option<u64>,
    pub real_token_reserves: Option<u64>,
    pub token_decimals: Option<u8>,
    pub sol_lamports: Option<u64>,
    pub price_sol_per_token: Option<f64>,
    pub market_cap_sol: Option<f64>,
    pub bonding_progress_pct: Option<f64>,
    pub source_quality: String,
    pub staleness_ms: Option<u64>,
    pub staleness_slots: Option<u64>,
}

impl PoolStateSampleV2 {
    pub fn from_account_state_core(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        state: &CanonicalPoolState,
        observed_at_wall_ms: u64,
        account_data_hash: Option<String>,
        temporal_class: TemporalClass,
        clock_domain: ClockDomain,
        token_decimals: u8,
    ) -> Self {
        envelope.schema = "pool_state_sample_v2".to_string();
        envelope.produced_at_ms = observed_at_wall_ms;
        envelope.produced_at_slot = Some(state.last_update_slot);
        envelope.temporal_class = temporal_class;
        envelope.clock_domain = clock_domain;
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;

        let staleness_ms = observed_at_wall_ms.checked_sub(state.last_update_ts_ms);
        let staleness_slots = event_order_key
            .slot
            .as_known()
            .and_then(|slot| slot.checked_sub(state.last_update_slot));
        for label in event_order_key.ambiguity_labels() {
            envelope.limitations.push(label);
        }
        match (staleness_ms, staleness_slots) {
            (Some(0), Some(0)) => {
                envelope
                    .limitations
                    .push("POOL_STATE_STALENESS=FRESH".to_string());
            }
            (Some(ms), Some(slots)) => {
                envelope
                    .limitations
                    .push(format!("POOL_STATE_STALENESS_MS={ms}"));
                envelope
                    .limitations
                    .push(format!("POOL_STATE_STALENESS_SLOTS={slots}"));
            }
            _ => {
                envelope
                    .limitations
                    .push("POOL_STATE_STALENESS_REVERSED_OR_UNKNOWN".to_string());
            }
        }
        let source_quality = match (staleness_ms, staleness_slots) {
            (Some(0), Some(0)) => "ACCOUNT_STATE_CORE_CANONICAL_FRESH",
            (Some(_), Some(_)) => "ACCOUNT_STATE_CORE_CANONICAL_STALENESS_MARKED",
            _ => "ACCOUNT_STATE_CORE_CANONICAL_STALENESS_BLOCKED",
        }
        .to_string();

        Self {
            envelope,
            observed_at_wall_ms,
            observed_slot: Some(state.last_update_slot),
            block_time: event_order_key.block_time.as_known().copied(),
            source: PoolStateSource::AccountStateCore,
            commitment: None,
            event_signature: match &event_order_key.signature {
                EventOrderComponent::Known(signature) => Some(signature.clone()),
                EventOrderComponent::Unknown(_) => Some(EVENT_ORDER_UNKNOWN_SIGNATURE.to_string()),
            },
            event_index: match &event_order_key.log_index_or_unknown {
                EventOrderComponent::Known(index) => Some(*index),
                EventOrderComponent::Unknown(_) => Some(EVENT_ORDER_UNKNOWN_INDEX),
            },
            account_data_hash,
            virtual_sol_reserves: Some(state.virtual_sol_reserves),
            virtual_token_reserves: Some(state.virtual_token_reserves),
            real_sol_reserves: Some(state.real_sol_reserves),
            real_token_reserves: Some(state.real_token_reserves),
            token_decimals: Some(token_decimals),
            sol_lamports: Some(1_000_000_000),
            price_sol_per_token: Some(state.price_sol),
            market_cap_sol: Some(state.market_cap_sol),
            bonding_progress_pct: Some(state.bonding_curve_progress),
            source_quality,
            staleness_ms,
            staleness_slots,
            event_order_key,
        }
    }

    pub fn ambiguity_labels(&self) -> Vec<String> {
        self.event_order_key.ambiguity_labels()
    }

    pub fn research_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.observed_at_wall_ms == 0 {
            blockers.push("POOL_STATE_OBSERVED_AT_WALL_MS_MISSING".to_string());
        }
        if self.observed_slot.is_none() {
            blockers.push("POOL_STATE_SLOT_MISSING".to_string());
        }
        if self.event_order_key.observed_at_wall_ms == 0 {
            blockers.push("EVENT_ORDER_OBSERVED_AT_WALL_MS_MISSING".to_string());
        }
        if self.event_order_key.slot.is_unknown() {
            blockers.push("EVENT_ORDER_SLOT_UNKNOWN".to_string());
        }
        for component in self.event_order_key.missing_chain_order_components() {
            match component {
                "slot" => {}
                "signature" => blockers.push("EVENT_ORDER_SIGNATURE_MISSING".to_string()),
                "transaction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_TRANSACTION_INDEX_MISSING".to_string())
                }
                "instruction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_INSTRUCTION_INDEX_MISSING".to_string())
                }
                "inner_instruction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_INNER_INSTRUCTION_INDEX_MISSING".to_string())
                }
                "log_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_LOG_INDEX_MISSING".to_string())
                }
                _ => blockers.push(format!("EVENT_ORDER_COMPONENT_MISSING:{component}")),
            }
        }
        if self
            .account_data_hash
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            blockers.push("POOL_STATE_ACCOUNT_DATA_HASH_MISSING".to_string());
        }
        if self.staleness_ms.is_none() {
            blockers.push("POOL_STATE_STALENESS_MS_MISSING_OR_REVERSED".to_string());
        }
        if self.staleness_slots.is_none() {
            blockers.push("POOL_STATE_STALENESS_SLOTS_MISSING_OR_REVERSED".to_string());
        }
        if matches!(self.envelope.temporal_class, TemporalClass::Unknown) {
            blockers.push("POOL_STATE_TEMPORAL_CLASS_UNKNOWN".to_string());
        }
        match self.source {
            PoolStateSource::Unknown => {
                blockers.push("POOL_STATE_SOURCE_UNKNOWN".to_string());
            }
            PoolStateSource::ShadowLedgerDiagnostic => {
                blockers.push("SHADOW_LEDGER_DIAGNOSTIC_NOT_LIVE_TRUTH".to_string());
            }
            PoolStateSource::RpcFallback if self.commitment.is_none() => {
                blockers.push("RPC_FALLBACK_COMMITMENT_MISSING".to_string());
            }
            _ => {}
        }
        if self.price_sol_per_token.is_some()
            && !has_reserve_pair(self.virtual_sol_reserves, self.virtual_token_reserves)
            && !has_reserve_pair(self.real_sol_reserves, self.real_token_reserves)
        {
            blockers.push("POOL_STATE_PRICE_WITHOUT_RESERVE_PROVENANCE".to_string());
        }
        if self.token_decimals.is_none() {
            blockers.push("TOKEN_DECIMALS_MISSING".to_string());
        }
        if self.sol_lamports.is_none() {
            blockers.push("SOL_LAMPORTS_NORMALIZATION_MISSING".to_string());
        }
        blockers
    }

    pub fn is_research_ready(&self) -> bool {
        self.research_blockers().is_empty()
    }
}

fn has_reserve_pair(sol_reserves: Option<u64>, token_reserves: Option<u64>) -> bool {
    matches!((sol_reserves, token_reserves), (Some(sol), Some(tokens)) if sol > 0 && tokens > 0)
}

pub fn account_data_hash_blake3(raw_account_data: &[u8]) -> String {
    blake3::hash(raw_account_data).to_hex().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolStateSampleValidation {
    pub event_id: String,
    pub research_ready: bool,
    pub blockers: Vec<String>,
    pub ambiguity_labels: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct PoolStateProvenanceRecorder {
    samples_by_event_id: HashMap<String, PoolStateSampleV2>,
    canonical_stream: ShadowV2CanonicalEventStream,
}

impl PoolStateProvenanceRecorder {
    pub fn record_sample(
        &mut self,
        sample: PoolStateSampleV2,
    ) -> Result<PoolStateSampleValidation, ShadowV2Error> {
        let event_id = sample.envelope.event_id.clone();
        if self.samples_by_event_id.contains_key(&event_id) {
            return Err(ShadowV2Error::DuplicateEventId { event_id });
        }
        let validation = PoolStateSampleValidation {
            event_id: event_id.clone(),
            research_ready: sample.is_research_ready(),
            blockers: sample.research_blockers(),
            ambiguity_labels: sample.ambiguity_labels(),
        };
        self.canonical_stream
            .append_record(ShadowV2Record::PoolStateSampleV2(sample.clone()))?;
        self.samples_by_event_id.insert(event_id, sample);
        Ok(validation)
    }

    pub fn record_research_sample(
        &mut self,
        sample: PoolStateSampleV2,
    ) -> Result<PoolStateSampleValidation, ShadowV2Error> {
        let blockers = sample.research_blockers();
        if !blockers.is_empty() {
            return Err(ShadowV2Error::PoolStateBlocked {
                event_id: sample.envelope.event_id.clone(),
                blockers,
            });
        }
        self.record_sample(sample)
    }

    pub fn sample(&self, event_id: &str) -> Option<&PoolStateSampleV2> {
        self.samples_by_event_id.get(event_id)
    }

    pub fn canonical_stream(&self) -> &ShadowV2CanonicalEventStream {
        &self.canonical_stream
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowEntryDecisionV2 {
    pub envelope: ShadowV2Envelope,
    pub decision_ts_ms: ClockedTimestamp,
    pub decision_slot: Option<u64>,
    pub feature_timestamps: Vec<ClockedTimestamp>,
    pub feature_temporal_classes: Vec<TemporalClass>,
    pub selected: bool,
    pub reason_or_veto: String,
    pub no_lookahead_proof: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowEntryAttemptV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub intended_entry_ts_ms: ClockedTimestamp,
    pub intended_entry_slot: Option<u64>,
    pub intended_price_source: String,
    pub intended_quote: Option<f64>,
    pub simulated_submit_ts_ms: Option<ClockedTimestamp>,
    pub simulated_landing_slot: Option<u64>,
    pub simulated_landing_delay_ms: Option<u64>,
    pub entry_failure_mode: Option<String>,
    pub executable_fill_model_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowEntryFillV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub fill_status: FillStatus,
    pub fill_price: Option<f64>,
    pub fill_price_source: Option<String>,
    pub fill_amount_sol: Option<f64>,
    pub fill_amount_tokens: Option<f64>,
    pub slippage_bps: Option<i32>,
    pub own_impact_bps: Option<i32>,
    pub fee_bps: Option<i32>,
    pub min_out: Option<u64>,
    pub pool_state_before: Option<String>,
    pub pool_state_after: Option<String>,
    pub reconstruction_status: String,
    pub quality: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowPathSampleV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub sample_ts_ms: ClockedTimestamp,
    pub sample_slot: Option<u64>,
    pub age_ms: u64,
    pub pool_state_ref: String,
    pub mark_price: Option<f64>,
    pub executable_exit_quote: Option<f64>,
    pub pnl_mark_bps: Option<i32>,
    pub pnl_executable_bps: Option<i32>,
    pub mfe_mark_bps: Option<i32>,
    pub mae_mark_bps: Option<i32>,
    pub source_quality: String,
    pub sampling_reason: String,
    pub exact_or_approx: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowExitAttemptV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub exit_trigger: String,
    pub trigger_ts_ms: ClockedTimestamp,
    pub trigger_slot: Option<u64>,
    pub trigger_source: String,
    pub target_bps: Option<i32>,
    pub stop_bps: Option<i32>,
    pub max_hold_ms: Option<u64>,
    pub tie_break_policy: Option<String>,
    pub same_slot_ambiguity: bool,
    pub executable_fill_model_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowExitFillV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub fill_status: FillStatus,
    pub fill_price: Option<f64>,
    pub fill_amount_sol: Option<f64>,
    pub fill_amount_tokens: Option<f64>,
    pub slippage_bps: Option<i32>,
    pub own_impact_bps: Option<i32>,
    pub fee_bps: Option<i32>,
    pub min_out: Option<u64>,
    pub pool_state_before: Option<String>,
    pub pool_state_after: Option<String>,
    pub reconstruction_status: String,
    pub quality: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowTerminalTruthV2 {
    pub envelope: ShadowV2Envelope,
    pub terminal_reason: TerminalReasonV2,
    pub terminal_ts_ms: ClockedTimestamp,
    pub terminal_slot: Option<u64>,
    pub terminal_source: String,
    pub final_pnl_mark_bps: Option<i32>,
    pub final_pnl_executable_bps: Option<i32>,
    pub close_age_ms: Option<u64>,
    pub linked_entry_fill: Option<String>,
    pub linked_exit_fill: Option<String>,
    pub reconciliation_status: String,
    pub duplicate_terminal_handling: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowReplayV2 {
    pub envelope: ShadowV2Envelope,
    pub canonical_event_stream_ref: String,
    pub mark_replay_ref: Option<String>,
    pub executable_replay_ref: Option<String>,
    pub coverage_metadata_ref: String,
    pub derived_from_canonical_stream: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::account_state_core::types::StatePhase;
    use ghost_core::CurveFinality;
    use serde_json::{json, Value};
    use solana_sdk::pubkey::Pubkey;

    fn event_order_key(slot: Option<u64>, tx_index: Option<u32>) -> EventOrderKey {
        EventOrderKey {
            slot: slot
                .map(EventOrderComponent::known)
                .unwrap_or_else(EventOrderComponent::unknown),
            block_time: EventOrderComponent::known(1_785_000_000),
            signature: EventOrderComponent::known("sig".to_string()),
            transaction_index_or_unknown: tx_index
                .map(EventOrderComponent::known)
                .unwrap_or_else(EventOrderComponent::unknown),
            instruction_index_or_unknown: EventOrderComponent::known(0),
            inner_instruction_index_or_unknown: EventOrderComponent::known(0),
            log_index_or_unknown: EventOrderComponent::known(0),
            event_seq_in_process: 7,
            observed_at_wall_ms: 1_785_000_000_123,
        }
    }

    fn explicit_unknown_event_order_key() -> EventOrderKey {
        EventOrderKey {
            slot: EventOrderComponent::known(42),
            block_time: EventOrderComponent::known(1_785_000_000),
            signature: EventOrderComponent::unknown(),
            transaction_index_or_unknown: EventOrderComponent::unknown(),
            instruction_index_or_unknown: EventOrderComponent::unknown(),
            inner_instruction_index_or_unknown: EventOrderComponent::unknown(),
            log_index_or_unknown: EventOrderComponent::unknown(),
            event_seq_in_process: 7,
            observed_at_wall_ms: 1_785_000_000_123,
        }
    }

    fn test_envelope(schema: &str, position_id: &str, event_id: &str) -> ShadowV2Envelope {
        let mut envelope = ShadowV2Envelope::contract_header(
            schema,
            "run-a",
            position_id,
            event_id,
            "pool-a",
            "mint-a",
        );
        envelope.session_id = Some("session-a".to_string());
        envelope.produced_at_ms = 1_785_000_000_123;
        envelope.temporal_class = TemporalClass::AtDecision;
        envelope
    }

    fn clocked(field_name: &str, value: i64, domain: ClockDomain) -> ClockedTimestamp {
        ClockedTimestamp {
            field_name: field_name.to_string(),
            value: Some(value),
            clock_domain: domain,
            clock_source: "shadow_v2_test".to_string(),
            causal_boundary: "AT_DECISION".to_string(),
        }
    }

    fn position_record(position_id: &str, event_id: &str) -> ShadowPositionV2 {
        ShadowPositionV2 {
            envelope: test_envelope("shadow_position_v2", position_id, event_id),
            created_at_wall_ms: clocked(
                "created_at_wall_ms",
                1_785_000_000_123,
                ClockDomain::WallClockMs,
            ),
            created_at_slot: Some(42),
            decision_id: Some("decision-a".to_string()),
            strategy_context: Some("shadow_v2_fixture".to_string()),
            lane: "shadow".to_string(),
        }
    }

    fn terminal_record(position_id: &str, event_id: &str) -> ShadowTerminalTruthV2 {
        let mut envelope = test_envelope("shadow_terminal_truth_v2", position_id, event_id);
        envelope.temporal_class = TemporalClass::PostExit;
        ShadowTerminalTruthV2 {
            envelope,
            terminal_reason: TerminalReasonV2::Timeout,
            terminal_ts_ms: clocked(
                "terminal_ts_ms",
                1_785_000_003_123,
                ClockDomain::StreamObservedMs,
            ),
            terminal_slot: Some(45),
            terminal_source: "canonical_event_stream".to_string(),
            final_pnl_mark_bps: Some(12),
            final_pnl_executable_bps: None,
            close_age_ms: Some(3_000),
            linked_entry_fill: None,
            linked_exit_fill: None,
            reconciliation_status: "CANONICAL_TERMINAL".to_string(),
            duplicate_terminal_handling: "REJECT_DUPLICATE_TERMINAL_TRUTH".to_string(),
        }
    }

    fn canonical_pool_state() -> CanonicalPoolState {
        CanonicalPoolState {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 7_000_000_000,
            real_token_reserves: 500_000_000_000,
            bonding_curve_progress: 42.5,
            price_sol: 0.00003,
            market_cap_sol: 30.0,
            token_total_supply: 1_000_000_000_000,
            is_complete: false,
            last_update_slot: 41,
            last_update_ts_ms: 1_785_000_000_000,
            curve_finality: CurveFinality::Provisional,
            state_phase: StatePhase::Canonical,
            update_count: 3,
            initial_price_sol: 0.00001,
            price_change_since_t0_pct: 200.0,
            reserve_velocity_sol_per_sec: 0.5,
        }
    }

    fn account_state_pool_sample(event_id: &str, seq: u64) -> PoolStateSampleV2 {
        let mut order_key = event_order_key(Some(42), Some(1));
        order_key.event_seq_in_process = seq;
        order_key.observed_at_wall_ms = 1_785_000_000_250;
        PoolStateSampleV2::from_account_state_core(
            test_envelope("pool_state_sample_v2", "pos-a", event_id),
            order_key,
            &canonical_pool_state(),
            1_785_000_000_250,
            Some(account_data_hash_blake3(b"account-data")),
            TemporalClass::PreDecision,
            ClockDomain::StreamObservedMs,
            6,
        )
    }

    #[test]
    fn envelope_serializes_simulation_level_and_measurement_grade() {
        let mut envelope = ShadowV2Envelope::contract_header(
            "shadow_position_v2",
            "run",
            "pos",
            "event",
            "pool",
            "mint",
        );
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::ResearchGradeCandidate;

        let serialized = serde_json::to_value(&envelope).unwrap();

        assert_eq!(serialized["simulation_level"], "FILL_MODEL_STATIC");
        assert_eq!(serialized["measurement_grade"], "RESEARCH_GRADE_CANDIDATE");
        assert_eq!(
            serialized["simulation_contract_version"],
            SHADOW_V2_SIMULATION_CONTRACT_VERSION
        );
    }

    #[test]
    fn event_order_key_requires_full_chain_indices_for_complete_order() {
        assert!(event_order_key(Some(42), Some(1)).has_complete_chain_order());
        assert!(!event_order_key(Some(42), None).has_complete_chain_order());
        assert!(!event_order_key(None, Some(1)).has_complete_chain_order());
        assert!(!explicit_unknown_event_order_key().has_complete_chain_order());
        assert!(explicit_unknown_event_order_key().has_explicit_unknown_chain_order());
        assert_eq!(
            event_order_key(Some(42), None).explicit_unknown_chain_order_components(),
            vec!["transaction_index_or_unknown"]
        );
    }

    #[test]
    fn event_order_key_serializes_typed_unknown_and_rejects_missing_schema_fields() {
        let unknown = explicit_unknown_event_order_key();
        let serialized = serde_json::to_value(&unknown).unwrap();

        assert_eq!(serialized["signature"], "UNKNOWN");
        assert_eq!(serialized["transaction_index_or_unknown"], "UNKNOWN");
        assert_eq!(serialized["instruction_index_or_unknown"], "UNKNOWN");
        assert_eq!(serialized["inner_instruction_index_or_unknown"], "UNKNOWN");
        assert_eq!(serialized["log_index_or_unknown"], "UNKNOWN");

        let parsed: EventOrderKey = serde_json::from_value(serialized).unwrap();
        assert!(parsed.has_explicit_unknown_chain_order());
        assert!(parsed
            .explicit_unknown_chain_order_components()
            .contains(&"signature"));

        let missing_required_component = json!({
            "slot": 42,
            "block_time": 1_785_000_000,
            "signature": "sig",
            "instruction_index_or_unknown": 0,
            "inner_instruction_index_or_unknown": 0,
            "log_index_or_unknown": 0,
            "event_seq_in_process": 7,
            "observed_at_wall_ms": 1_785_000_000_123_u64
        });
        assert!(serde_json::from_value::<EventOrderKey>(missing_required_component).is_err());
    }

    #[test]
    fn event_order_key_flags_same_slot_unknown_order_as_ambiguous() {
        let lhs = event_order_key(Some(42), None);
        let rhs = event_order_key(Some(42), Some(2));

        assert!(lhs.same_slot_ambiguous_with(&rhs));
        assert!(!event_order_key(Some(42), Some(1))
            .same_slot_ambiguous_with(&event_order_key(Some(43), None)));
    }

    #[test]
    fn clocked_timestamp_preserves_domain_and_boundary() {
        let ts = ClockedTimestamp {
            field_name: "decision_ts_ms".to_string(),
            value: Some(1_785_000_000_123),
            clock_domain: ClockDomain::DecisionTsMs,
            clock_source: "oracle_runtime".to_string(),
            causal_boundary: "AT_DECISION".to_string(),
        };

        let serialized = serde_json::to_value(&ts).unwrap();

        assert_eq!(serialized["field_name"], "decision_ts_ms");
        assert_eq!(serialized["clock_domain"], "DECISION_TS_MS");
        assert_eq!(serialized["causal_boundary"], "AT_DECISION");
    }

    #[test]
    fn replay_v2_is_marked_as_derived_from_canonical_stream() {
        let replay = ShadowReplayV2 {
            envelope: ShadowV2Envelope::contract_header(
                "shadow_replay_v2",
                "run",
                "pos",
                "event",
                "pool",
                "mint",
            ),
            canonical_event_stream_ref: "shadow_position_event_v2.jsonl#event".to_string(),
            mark_replay_ref: Some("shadow_replay_v2.jsonl#mark".to_string()),
            executable_replay_ref: None,
            coverage_metadata_ref: "coverage.json#pos".to_string(),
            derived_from_canonical_stream: true,
        };

        assert!(replay.derived_from_canonical_stream);
    }

    #[test]
    fn shadow_v2_terminal_stream_rejects_duplicate_terminal_truth() {
        let mut stream = ShadowV2CanonicalEventStream::default();
        stream
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-a",
                "event-position",
            )))
            .unwrap();
        stream
            .append_record(ShadowV2Record::ShadowTerminalTruthV2(terminal_record(
                "pos-a",
                "event-terminal-a",
            )))
            .unwrap();

        let error = stream
            .append_record(ShadowV2Record::ShadowTerminalTruthV2(terminal_record(
                "pos-a",
                "event-terminal-b",
            )))
            .unwrap_err();

        assert!(matches!(
            error,
            ShadowV2Error::DuplicateTerminalTruth {
                ref position_id,
                ref existing_terminal_event_id,
                ref attempted_terminal_event_id,
            } if position_id == "pos-a"
                && existing_terminal_event_id == "event-terminal-a"
                && attempted_terminal_event_id == "event-terminal-b"
        ));
        assert_eq!(stream.terminal_event_id("pos-a"), Some("event-terminal-a"));
    }

    #[test]
    fn shadow_v2_terminal_stream_rejects_duplicate_event_id() {
        let mut stream = ShadowV2CanonicalEventStream::default();
        stream
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-a",
                "event-position",
            )))
            .unwrap();

        let error = stream
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-b",
                "event-position",
            )))
            .unwrap_err();

        assert!(matches!(
            error,
            ShadowV2Error::DuplicateEventId { ref event_id } if event_id == "event-position"
        ));
    }

    #[test]
    fn shadow_v2_terminal_exact_join_index_rejects_ambiguous_key() {
        let mut index = ShadowV2ExactJoinIndex::default();
        let first = test_envelope("shadow_position_v2", "pos-a", "event-a");
        let mut second = first.clone();
        second.event_id = "event-b".to_string();

        index.insert_terminal(&first).unwrap();
        let error = index.insert_terminal(&second).unwrap_err();

        assert!(matches!(
            error,
            ShadowV2Error::AmbiguousExactJoinKey {
                ref existing_event_id,
                ref attempted_event_id,
                ..
            } if existing_event_id == "event-a" && attempted_event_id == "event-b"
        ));
        assert!(matches!(
            ShadowV2ExactJoinIndex::fallback_join_disallowed("pool_id/base_mint fallback"),
            ShadowV2Error::FallbackJoinDisallowed { .. }
        ));
    }

    #[test]
    fn shadow_v2_exact_join_key_rejects_empty_identity_fields() {
        for (field, key_result) in [
            (
                "run_id",
                ShadowV2ExactJoinKey::new("", "session-a", "pos-a", "pool-a", "mint-a"),
            ),
            (
                "session_id",
                ShadowV2ExactJoinKey::new("run-a", "", "pos-a", "pool-a", "mint-a"),
            ),
            (
                "position_id",
                ShadowV2ExactJoinKey::new("run-a", "session-a", "", "pool-a", "mint-a"),
            ),
            (
                "pool_id",
                ShadowV2ExactJoinKey::new("run-a", "session-a", "pos-a", "", "mint-a"),
            ),
            (
                "base_mint",
                ShadowV2ExactJoinKey::new("run-a", "session-a", "pos-a", "pool-a", ""),
            ),
        ] {
            let error = key_result.unwrap_err();
            assert!(matches!(
                error,
                ShadowV2Error::MissingExactJoinKey { missing_field, .. }
                    if missing_field == field
            ));
        }
    }

    #[test]
    fn shadow_v2_terminal_jsonl_writer_emits_canonical_event_stream() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("shadow_position_event_v2.jsonl");
        let mut writer = JsonlShadowV2CanonicalWriter::new(&path).unwrap();

        writer
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-a",
                "event-position",
            )))
            .unwrap();

        let line = std::fs::read_to_string(path).unwrap();
        let json: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(json["event_kind"], "POSITION_CREATED");
        assert_eq!(json["envelope"]["position_id"], "pos-a");
        assert_eq!(json["canonical_payload_schema"], "shadow_position_v2");
        assert_eq!(writer.stream().events().len(), 1);
    }

    #[test]
    fn shadow_v2_terminal_jsonl_writer_indexes_existing_file_on_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("shadow_position_event_v2.jsonl");
        {
            let mut writer = JsonlShadowV2CanonicalWriter::new(&path).unwrap();
            writer
                .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                    "pos-a",
                    "event-position",
                )))
                .unwrap();
            writer
                .append_record(ShadowV2Record::ShadowTerminalTruthV2(terminal_record(
                    "pos-a",
                    "event-terminal-a",
                )))
                .unwrap();
        }

        let mut restarted = JsonlShadowV2CanonicalWriter::new(&path).unwrap();
        assert_eq!(restarted.stream().events().len(), 2);
        assert_eq!(
            restarted.stream().terminal_event_id("pos-a"),
            Some("event-terminal-a")
        );

        let duplicate_terminal = restarted
            .append_record(ShadowV2Record::ShadowTerminalTruthV2(terminal_record(
                "pos-a",
                "event-terminal-b",
            )))
            .unwrap_err();
        assert!(matches!(
            duplicate_terminal,
            ShadowV2Error::DuplicateTerminalTruth {
                ref existing_terminal_event_id,
                ref attempted_terminal_event_id,
                ..
            } if existing_terminal_event_id == "event-terminal-a"
                && attempted_terminal_event_id == "event-terminal-b"
        ));

        let duplicate_event_id = restarted
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-b",
                "event-position",
            )))
            .unwrap_err();
        assert!(matches!(
            duplicate_event_id,
            ShadowV2Error::DuplicateEventId { ref event_id } if event_id == "event-position"
        ));
    }

    #[test]
    fn shadow_v2_terminal_jsonl_writer_keeps_memory_clean_after_io_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let directory_path = tmp.path().join("shadow_position_event_v2.jsonl");
        std::fs::create_dir_all(&directory_path).unwrap();
        let mut writer = JsonlShadowV2CanonicalWriter {
            path: directory_path,
            stream: ShadowV2CanonicalEventStream::default(),
        };

        let error = writer
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-a",
                "event-position",
            )))
            .unwrap_err();

        assert!(matches!(error, ShadowV2Error::Io(_)));
        assert!(writer.stream().events().is_empty());
        assert!(writer.stream().terminal_event_id("pos-a").is_none());
    }

    #[test]
    fn shadow_v2_pool_state_from_account_state_core_carries_provenance() {
        let sample = account_state_pool_sample("pool-event-a", 1);

        assert_eq!(sample.envelope.schema, "pool_state_sample_v2");
        assert_eq!(sample.source, PoolStateSource::AccountStateCore);
        assert_eq!(sample.observed_slot, Some(41));
        assert_eq!(sample.event_order_key.slot.as_known(), Some(&42));
        assert_eq!(sample.staleness_ms, Some(250));
        assert_eq!(sample.staleness_slots, Some(1));
        assert_eq!(sample.envelope.temporal_class, TemporalClass::PreDecision);
        assert_eq!(sample.envelope.clock_domain, ClockDomain::StreamObservedMs);
        assert_eq!(sample.virtual_sol_reserves, Some(30_000_000_000));
        assert_eq!(sample.token_decimals, Some(6));
        assert_eq!(sample.sol_lamports, Some(1_000_000_000));
        assert_eq!(
            sample.account_data_hash.as_deref(),
            Some(account_data_hash_blake3(b"account-data").as_str())
        );
        assert!(sample.is_research_ready());
    }

    #[test]
    fn shadow_v2_pool_state_constructor_requires_explicit_temporal_context() {
        let sample = PoolStateSampleV2::from_account_state_core(
            test_envelope("pool_state_sample_v2", "pos-a", "pool-event-post-entry"),
            event_order_key(Some(42), Some(1)),
            &canonical_pool_state(),
            1_785_000_000_250,
            Some(account_data_hash_blake3(b"account-data")),
            TemporalClass::PostEntry,
            ClockDomain::LandingTsMs,
            9,
        );

        assert_eq!(sample.envelope.temporal_class, TemporalClass::PostEntry);
        assert_eq!(sample.envelope.clock_domain, ClockDomain::LandingTsMs);
        assert_eq!(sample.token_decimals, Some(9));
    }

    #[test]
    fn shadow_v2_pool_state_research_sample_blocks_missing_hash_staleness_and_chain_order() {
        let mut sample = account_state_pool_sample("pool-event-a", 1);
        sample.account_data_hash = None;
        sample.staleness_ms = None;
        sample.staleness_slots = None;
        sample.event_order_key.signature = EventOrderComponent::known("".to_string());

        let blockers = sample.research_blockers();

        for expected in [
            "POOL_STATE_ACCOUNT_DATA_HASH_MISSING",
            "POOL_STATE_STALENESS_MS_MISSING_OR_REVERSED",
            "POOL_STATE_STALENESS_SLOTS_MISSING_OR_REVERSED",
            "EVENT_ORDER_SIGNATURE_MISSING",
        ] {
            assert!(
                blockers.contains(&expected.to_string()),
                "expected blocker {expected}, got {blockers:?}"
            );
        }
    }

    #[test]
    fn shadow_v2_pool_state_explicit_unknown_chain_order_is_labeled_not_silent() {
        let mut order_key = explicit_unknown_event_order_key();
        order_key.event_seq_in_process = 1;
        let sample = PoolStateSampleV2::from_account_state_core(
            test_envelope("pool_state_sample_v2", "pos-a", "pool-event-unknown-order"),
            order_key,
            &canonical_pool_state(),
            1_785_000_000_250,
            Some(account_data_hash_blake3(b"account-data")),
            TemporalClass::PreDecision,
            ClockDomain::StreamObservedMs,
            6,
        );

        assert!(sample.research_blockers().is_empty());
        assert!(sample
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT".to_string()));
        assert!(sample
            .envelope
            .limitations
            .contains(&"EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string()));

        let mut recorder = PoolStateProvenanceRecorder::default();
        let validation = recorder.record_research_sample(sample).unwrap();
        assert!(validation.research_ready);
        assert!(validation
            .ambiguity_labels
            .contains(&"EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT".to_string()));
    }

    #[test]
    fn shadow_v2_pool_state_research_sample_blocks_missing_slot_or_unknown_source() {
        let mut sample = account_state_pool_sample("pool-event-a", 1);
        sample.observed_slot = None;
        sample.event_order_key.slot = EventOrderComponent::unknown();
        sample.source = PoolStateSource::Unknown;

        let mut recorder = PoolStateProvenanceRecorder::default();
        let error = recorder.record_research_sample(sample).unwrap_err();

        assert!(matches!(
            error,
            ShadowV2Error::PoolStateBlocked { blockers, .. }
                if blockers.contains(&"POOL_STATE_SLOT_MISSING".to_string())
                    && blockers.contains(&"EVENT_ORDER_SLOT_UNKNOWN".to_string())
                    && blockers.contains(&"POOL_STATE_SOURCE_UNKNOWN".to_string())
        ));
    }

    #[test]
    fn shadow_v2_pool_state_shadowledger_source_is_diagnostic_only() {
        let mut sample = account_state_pool_sample("pool-event-a", 1);
        sample.source = PoolStateSource::ShadowLedgerDiagnostic;
        sample.source_quality = "SHADOW_LEDGER_DIAGNOSTIC_ONLY".to_string();

        let mut recorder = PoolStateProvenanceRecorder::default();
        let validation = recorder.record_sample(sample.clone()).unwrap();
        assert!(!validation.research_ready);
        assert!(validation
            .blockers
            .contains(&"SHADOW_LEDGER_DIAGNOSTIC_NOT_LIVE_TRUTH".to_string()));

        let mut research_recorder = PoolStateProvenanceRecorder::default();
        let error = research_recorder
            .record_research_sample(sample)
            .unwrap_err();
        assert!(matches!(
            error,
            ShadowV2Error::PoolStateBlocked { blockers, .. }
                if blockers.contains(&"SHADOW_LEDGER_DIAGNOSTIC_NOT_LIVE_TRUTH".to_string())
        ));
    }

    #[test]
    fn shadow_v2_pool_state_recorder_enforces_monotonic_event_sequence() {
        let mut recorder = PoolStateProvenanceRecorder::default();
        recorder
            .record_sample(account_state_pool_sample("pool-event-a", 2))
            .unwrap();

        let error = recorder
            .record_sample(account_state_pool_sample("pool-event-b", 1))
            .unwrap_err();

        assert!(matches!(
            error,
            ShadowV2Error::NonMonotonicEventSequence {
                previous_seq: 2,
                attempted_seq: 1,
                ..
            }
        ));
    }
}
