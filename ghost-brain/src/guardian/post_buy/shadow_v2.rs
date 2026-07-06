//! Shadow Burnin Simulation V2 contract types.
//!
//! These types remain decision-inert. The Shadow V2 foundation defines schema,
//! validation vocabulary, canonical event guards, pool-state provenance,
//! deterministic price/fill formulas, exit fill simulation, path sampling, and
//! logging-only validation harness helpers. No BUY/REJECT policy, selector,
//! TX/Jito path, shadow_close_only path, or active close path consumes these
//! records.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::shadow_v2_execution::{
    ShadowV2BoundaryKind, ShadowV2ExecutionInput, ShadowV2ExecutionLabelGrade,
    ShadowV2ExecutionOutcome, ShadowV2ExecutionSide, ShadowV2FillEngine, ShadowV2NoFillReason,
};
use ghost_core::account_state_core::types::CanonicalPoolState;
use ghost_core::{ShadowV2PoolPhase, ShadowV2Quote, ShadowV2Reserves, SHADOW_V2_BPS_DENOMINATOR};
use serde::{Deserialize, Serialize};

pub const SHADOW_V2_SIMULATION_CONTRACT_VERSION: &str = "shadow_burnin_simulation_v2_20260629";
pub const SHADOW_V2_ENTRY_FILL_MODEL_VERSION: &str =
    "shadow_v2_entry_fill_static_constant_product_v1";
pub const SHADOW_V2_EXIT_FILL_MODEL_VERSION: &str =
    "shadow_v2_exit_fill_static_constant_product_v1";
pub const SHADOW_V2_REPLAY_DERIVATION_VERSION: &str =
    "shadow_v2_replay_derived_from_canonical_stream_v1";
pub const SHADOW_V2_LIFECYCLE_DERIVATION_VERSION: &str =
    "shadow_v2_lifecycle_derived_from_canonical_stream_v1";
pub const SHADOW_V2_VALIDATION_HARNESS_VERSION: &str =
    "shadow_v2_validation_harness_logging_only_v1";
pub const SHADOW_V2_L2_DECLARED_DENSITY_HORIZONS_MS: [u64; 5] =
    [2_000, 3_000, 10_000, 30_000, 120_000];
pub const SHADOW_V2_L2_UNDECLARED_LONG_HORIZONS_MS: [u64; 2] = [300_000, 500_000];
pub const SHADOW_V2_DENSITY_FULL_STREAM_ENV: &str = "SHADOW_V2_DENSITY_FULL_STREAM";
pub const SHADOW_V2_ARTIFACT_BUDGET_BLOCKER: &str = "BLOCKED_L2_ARTIFACT_BUDGET_EXCEEDED";
pub const SHADOW_V2_SOURCE_REF_MANIFEST_SCHEMA: &str = "shadow_source_ref_manifest_v2";
pub const SHADOW_V2_ARTIFACT_ROTATION_MANIFEST_SCHEMA: &str =
    "shadow_artifact_rotation_manifest_v2";
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
pub enum ShadowExitFillFailureModeV2 {
    NoFill,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowPathSamplingModeV2 {
    Dense3s,
    Standard120s,
    Long500s,
}

impl ShadowPathSamplingModeV2 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dense3s => "shadow_path_dense_3s",
            Self::Standard120s => "shadow_path_standard_120s",
            Self::Long500s => "shadow_path_long_500s",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowPathSamplingReasonV2 {
    EventSample,
    Heartbeat,
    LevelHit,
    LargePriceDelta,
    Terminal,
}

impl ShadowPathSamplingReasonV2 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EventSample => "EVENT_SAMPLE",
            Self::Heartbeat => "HEARTBEAT",
            Self::LevelHit => "LEVEL_HIT",
            Self::LargePriceDelta => "LARGE_PRICE_DELTA",
            Self::Terminal => "TERMINAL",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "EVENT_SAMPLE" => Some(Self::EventSample),
            "HEARTBEAT" => Some(Self::Heartbeat),
            "LEVEL_HIT" => Some(Self::LevelHit),
            "LARGE_PRICE_DELTA" => Some(Self::LargePriceDelta),
            "TERMINAL" => Some(Self::Terminal),
            _ => None,
        }
    }

    pub const fn is_must_keep(self) -> bool {
        matches!(self, Self::LevelHit | Self::Terminal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowPathHorizonVerdictV2 {
    EvaluableExact,
    EvaluableApprox,
    SparseApproxOnly,
    NotEvaluableNoCoverage,
    NotEvaluableHorizonExceedsReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowExitTieBreakPolicyV2 {
    BlockAmbiguous,
    TargetFirst,
    StopFirst,
    EarliestEventOrder,
}

impl ShadowExitTieBreakPolicyV2 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::BlockAmbiguous => "BLOCK_AMBIGUOUS",
            Self::TargetFirst => "TARGET_FIRST",
            Self::StopFirst => "STOP_FIRST",
            Self::EarliestEventOrder => "EARLIEST_EVENT_ORDER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowExitHitSourceV2 {
    ExactLevel,
    SampledPath,
    TimeoutPathPoint,
    BlockedByData,
}

impl ShadowExitHitSourceV2 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExactLevel => "EXACT_LEVEL",
            Self::SampledPath => "SAMPLED_PATH",
            Self::TimeoutPathPoint => "TIMEOUT_PATH_POINT",
            Self::BlockedByData => "BLOCKED_BY_DATA",
        }
    }
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
pub enum ShadowLifecycleEventTypeV2 {
    PositionOpen,
    PositionClosed,
    TerminalBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventOrderUnknown {
    Unknown,
    NotApplicable,
    Derived,
    RuntimeLocal,
}

/// Typed chain-order component used by `EventOrderKey`.
///
/// Serialization intentionally preserves the schema contract shape: known
/// numeric/string components serialize as their raw value. Missing chain
/// ordering serializes as the literal `UNKNOWN`. Non-chain-observed components
/// may be explicitly classified as `NOT_APPLICABLE`, `DERIVED`, or
/// `RUNTIME_LOCAL`; those values are never treated as known chain order. A
/// missing JSON field is a schema error instead of an implicit unknown.
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

    pub fn not_applicable() -> Self {
        Self::Unknown(EventOrderUnknown::NotApplicable)
    }

    pub fn derived() -> Self {
        Self::Unknown(EventOrderUnknown::Derived)
    }

    pub fn runtime_local() -> Self {
        Self::Unknown(EventOrderUnknown::RuntimeLocal)
    }

    pub fn as_known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(EventOrderUnknown::Unknown))
    }

    pub fn non_known_classification(&self) -> Option<&'static str> {
        match self {
            Self::Known(_) => None,
            Self::Unknown(EventOrderUnknown::Unknown) => Some("UNKNOWN"),
            Self::Unknown(EventOrderUnknown::NotApplicable) => Some("NOT_APPLICABLE"),
            Self::Unknown(EventOrderUnknown::Derived) => Some("DERIVED"),
            Self::Unknown(EventOrderUnknown::RuntimeLocal) => Some("RUNTIME_LOCAL"),
        }
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

    pub fn not_applicable_or_derived_chain_order_components(&self) -> Vec<String> {
        let mut classified = Vec::new();
        for (name, classification) in [
            ("slot", self.slot.non_known_classification()),
            ("block_time", self.block_time.non_known_classification()),
            ("signature", self.signature.non_known_classification()),
            (
                "transaction_index_or_unknown",
                self.transaction_index_or_unknown.non_known_classification(),
            ),
            (
                "instruction_index_or_unknown",
                self.instruction_index_or_unknown.non_known_classification(),
            ),
            (
                "inner_instruction_index_or_unknown",
                self.inner_instruction_index_or_unknown
                    .non_known_classification(),
            ),
            (
                "log_index_or_unknown",
                self.log_index_or_unknown.non_known_classification(),
            ),
        ] {
            if let Some(classification @ ("NOT_APPLICABLE" | "DERIVED" | "RUNTIME_LOCAL")) =
                classification
            {
                classified.push(format!("{name}:{classification}"));
            }
        }
        classified
    }

    pub fn has_explicit_unknown_chain_order(&self) -> bool {
        !self.explicit_unknown_chain_order_components().is_empty()
    }

    pub fn ambiguity_labels(&self) -> Vec<String> {
        let unknown = self.explicit_unknown_chain_order_components();
        let classified = self.not_applicable_or_derived_chain_order_components();
        let mut labels = Vec::new();
        if !unknown.is_empty() {
            labels.push("EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT".to_string());
            labels.push(format!(
                "EVENT_ORDER_UNKNOWN_COMPONENTS={}",
                unknown.join("|")
            ));
            labels.push("EVENT_ORDER_UNKNOWN_BUT_REQUIRED_FOR_RESEARCH".to_string());
            labels.push("EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string());
        }
        if !classified.is_empty() {
            labels.push("EVENT_ORDER_CHAIN_COMPONENT_CLASSIFIED_NOT_CHAIN_OBSERVED".to_string());
            labels.push(format!(
                "EVENT_ORDER_NON_CHAIN_COMPONENTS={}",
                classified.join("|")
            ));
        }
        labels
    }

    pub fn has_complete_chain_order(&self) -> bool {
        self.slot.as_known().is_some()
            && matches!(&self.signature, EventOrderComponent::Known(signature) if !signature.trim().is_empty())
            && self.transaction_index_or_unknown.as_known().is_some()
            && self.instruction_index_or_unknown.as_known().is_some()
            && self.inner_instruction_index_or_unknown.as_known().is_some()
            && self.log_index_or_unknown.as_known().is_some()
    }

    pub fn solana_transaction_source_proof_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        push_missing_source_component(&mut blockers, &self.slot, "TRANSACTION_SOURCE_SLOT");
        push_missing_source_component(
            &mut blockers,
            &self.block_time,
            "TRANSACTION_SOURCE_BLOCK_TIME",
        );
        match &self.signature {
            EventOrderComponent::Known(signature) if !signature.trim().is_empty() => {}
            EventOrderComponent::Known(_) => {
                blockers.push("TRANSACTION_SOURCE_SIGNATURE_EMPTY".to_string())
            }
            EventOrderComponent::Unknown(_) => push_missing_source_component(
                &mut blockers,
                &self.signature,
                "TRANSACTION_SOURCE_SIGNATURE",
            ),
        }
        push_missing_source_component(
            &mut blockers,
            &self.transaction_index_or_unknown,
            "TRANSACTION_SOURCE_TRANSACTION_INDEX",
        );
        push_missing_source_component(
            &mut blockers,
            &self.instruction_index_or_unknown,
            "TRANSACTION_SOURCE_INSTRUCTION_INDEX",
        );
        blockers
    }

    pub fn has_complete_solana_transaction_source_proof(&self) -> bool {
        self.solana_transaction_source_proof_blockers().is_empty()
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

fn push_missing_source_component<T>(
    blockers: &mut Vec<String>,
    component: &EventOrderComponent<T>,
    label: &str,
) {
    if component.as_known().is_some() {
        return;
    }
    let classification = component.non_known_classification().unwrap_or("UNKNOWN");
    blockers.push(format!("{label}_{classification}"));
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

    pub const fn requires_event_ordering(self) -> bool {
        matches!(
            self,
            Self::PoolStateSample
                | Self::EntryAttempt
                | Self::EntryFill
                | Self::PathSample
                | Self::ExitAttempt
                | Self::ExitFill
                | Self::TerminalTruth
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowPositionEventV2 {
    #[serde(default = "shadow_position_event_v2_schema")]
    pub schema: String,
    pub envelope: ShadowV2Envelope,
    pub event_kind: ShadowPositionEventKindV2,
    pub event_order_key: Option<EventOrderKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordering_exemption: Option<String>,
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
        let ordering_exemption = record.ordering_exemption();
        let canonical_terminal_event_id = event_kind
            .is_canonical_terminal()
            .then(|| envelope.event_id.clone());
        let payload = serde_json::to_value(&record).map_err(ShadowV2Error::from)?;

        Ok(Self {
            schema: shadow_position_event_v2_schema(),
            envelope,
            event_kind,
            event_order_key,
            ordering_exemption,
            canonical_payload_schema,
            canonical_payload_event_id,
            canonical_terminal_event_id,
            payload,
        })
    }

    pub fn is_canonical_terminal(&self) -> bool {
        self.event_kind.is_canonical_terminal()
    }

    pub fn has_explicit_ordering_exemption(&self) -> bool {
        matches!(
            (self.event_kind, self.ordering_exemption.as_deref()),
            (
                ShadowPositionEventKindV2::PositionCreated,
                Some(
                    "ORDERING_EXEMPT_POSITION_CREATED" | "ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER"
                )
            )
        )
    }
}

fn shadow_position_event_v2_schema() -> String {
    "shadow_position_event_v2".to_string()
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
    ShadowLifecycleV2(ShadowLifecycleV2),
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
            Self::ShadowLifecycleV2(record) => &record.envelope,
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
            Self::ShadowLifecycleV2(_) => ShadowPositionEventKindV2::LifecycleSubEvent,
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
            Self::ShadowTerminalTruthV2(record) => Some(&record.event_order_key),
            Self::ShadowPositionV2(_)
            | Self::ShadowEntryDecisionV2(_)
            | Self::ShadowReplayV2(_)
            | Self::ShadowLifecycleV2(_) => None,
        }
    }

    pub fn ordering_exemption(&self) -> Option<String> {
        match self {
            Self::ShadowPositionV2(record) => {
                let is_smoke_marker = matches!(
                    record.envelope.candidate_id.as_deref(),
                    Some("VALIDATION_SMOKE_MARKER")
                ) || record
                    .envelope
                    .limitations
                    .iter()
                    .any(|limitation| limitation == "VALIDATION_SMOKE_MARKER_V2");
                Some(
                    if is_smoke_marker {
                        "ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER"
                    } else {
                        "ORDERING_EXEMPT_POSITION_CREATED"
                    }
                    .to_string(),
                )
            }
            _ => None,
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
        position_id: String,
        previous_seq: u64,
        attempted_seq: u64,
    },
    MissingRequiredEventOrderKey {
        event_id: String,
        event_kind: ShadowPositionEventKindV2,
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
    MissingCanonicalPositionEvents {
        position_id: String,
    },
    HarnessConfig {
        reason: String,
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
                position_id,
                previous_seq,
                attempted_seq,
            } => write!(
                f,
                "shadow v2 run {run_id} position {position_id} non-monotonic event_seq_in_process: previous={previous_seq}, attempted={attempted_seq}"
            ),
            Self::MissingRequiredEventOrderKey {
                event_id,
                event_kind,
            } => write!(
                f,
                "shadow v2 event {event_id} kind {event_kind:?} missing required event_order_key"
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
            Self::MissingCanonicalPositionEvents { position_id } => write!(
                f,
                "shadow v2 canonical stream has no events for position {position_id}"
            ),
            Self::HarnessConfig { reason } => {
                write!(f, "shadow v2 validation harness config error: {reason}")
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
    last_process_seq_by_position: HashMap<(String, String), u64>,
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
        let mut event = ShadowPositionEventV2::from_record(record)?;
        self.canonicalize_event_order_for_position(&mut event);
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

    pub fn events_for_position(&self, position_id: &str) -> Vec<&ShadowPositionEventV2> {
        self.events
            .iter()
            .filter(|event| event.envelope.position_id == position_id)
            .collect()
    }

    pub fn terminal_event_id(&self, position_id: &str) -> Option<&str> {
        self.terminal_event_by_position
            .get(position_id)
            .map(String::as_str)
    }

    pub fn canonical_terminal_event(&self, position_id: &str) -> Option<&ShadowPositionEventV2> {
        let terminal_event_id = self.terminal_event_id(position_id)?;
        self.events
            .iter()
            .find(|event| event.envelope.event_id == terminal_event_id)
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
        if event.event_kind.requires_event_ordering() && event.event_order_key.is_none() {
            return Err(ShadowV2Error::MissingRequiredEventOrderKey {
                event_id: event.envelope.event_id.clone(),
                event_kind: event.event_kind,
            });
        }
        if let Some(order_key) = event.event_order_key.as_ref() {
            let sequence_key = shadow_v2_position_sequence_key(event);
            if let Some(previous_seq) = self.last_process_seq_by_position.get(&sequence_key) {
                if !order_key.is_after_process_seq(*previous_seq) {
                    return Err(ShadowV2Error::NonMonotonicEventSequence {
                        run_id: event.envelope.run_id.clone(),
                        position_id: event.envelope.position_id.clone(),
                        previous_seq: *previous_seq,
                        attempted_seq: order_key.event_seq_in_process,
                    });
                }
            }
        }
        Ok(())
    }

    fn canonicalize_event_order_for_position(&self, event: &mut ShadowPositionEventV2) {
        let sequence_key = shadow_v2_position_sequence_key(event);
        let Some(order_key) = event.event_order_key.as_mut() else {
            return;
        };
        let canonical_seq = shadow_v2_event_seq_for_position(
            self.last_process_seq_by_position
                .get(&sequence_key)
                .copied(),
            order_key.event_seq_in_process,
        );
        if canonical_seq != order_key.event_seq_in_process {
            order_key.event_seq_in_process = canonical_seq;
            set_payload_event_seq_in_process(&mut event.payload, canonical_seq);
        }
    }

    fn commit_event(&mut self, event: ShadowPositionEventV2) {
        if let Some(order_key) = event.event_order_key.as_ref() {
            self.last_process_seq_by_position.insert(
                shadow_v2_position_sequence_key(&event),
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

fn shadow_v2_position_sequence_key(event: &ShadowPositionEventV2) -> (String, String) {
    (
        event.envelope.run_id.clone(),
        event.envelope.position_id.clone(),
    )
}

pub fn shadow_v2_event_seq_for_position(previous_seq: Option<u64>, attempted_seq: u64) -> u64 {
    previous_seq.map_or(attempted_seq, |previous| {
        attempted_seq.max(previous.saturating_add(1))
    })
}

fn set_payload_event_seq_in_process(payload: &mut serde_json::Value, event_seq_in_process: u64) {
    let Some(record) = payload.get_mut("record") else {
        return;
    };
    let Some(event_order_key) = record.get_mut("event_order_key") else {
        return;
    };
    if let Some(object) = event_order_key.as_object_mut() {
        object.insert(
            "event_seq_in_process".to_string(),
            serde_json::Value::from(event_seq_in_process),
        );
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

fn blake3_file_hex(path: &Path) -> Result<String, ShadowV2Error> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn shadow_v2_rotated_jsonl_part_path(path: &Path, rotation_index: u64) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("shadow_v2_artifact.jsonl");
    let base = file_name.strip_suffix(".jsonl").unwrap_or(file_name);
    let rotated_name = format!("{base}.part-{rotation_index:06}.jsonl");
    path.with_file_name(rotated_name)
}

fn next_shadow_v2_rotated_jsonl_part_path(path: &Path) -> (PathBuf, u64) {
    for rotation_index in 1..=u64::MAX {
        let candidate = shadow_v2_rotated_jsonl_part_path(path, rotation_index);
        if !candidate.exists() {
            return (candidate, rotation_index);
        }
    }
    unreachable!("u64 rotation index space exhausted")
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_owner_or_program: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_write_version: Option<u64>,
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
        let account_data_hash = account_data_hash.or_else(|| state.account_data_hash.clone());

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
            account_data_len: state.account_data_len,
            source_account_pubkey: state.source_account_pubkey.map(|pubkey| pubkey.to_string()),
            source_account_owner_or_program: state
                .source_account_owner_or_program
                .map(|pubkey| pubkey.to_string()),
            source_account_slot: Some(state.last_update_slot),
            source_write_version: state.source_write_version,
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

    pub fn account_state_source_proof_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self
            .account_data_hash
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            blockers.push("ACCOUNT_STATE_SOURCE_HASH_MISSING".to_string());
        }
        if self
            .source_account_pubkey
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            blockers.push("ACCOUNT_STATE_SOURCE_PUBKEY_MISSING".to_string());
        }
        if self.source_account_slot.is_none() {
            blockers.push("ACCOUNT_STATE_SOURCE_SLOT_MISSING".to_string());
        }
        if self.source_write_version.is_none() {
            blockers.push("ACCOUNT_STATE_SOURCE_WRITE_VERSION_MISSING".to_string());
        }
        blockers
    }

    pub fn has_complete_account_state_source_proof(&self) -> bool {
        self.account_state_source_proof_blockers().is_empty()
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
        for component in self
            .event_order_key
            .explicit_unknown_chain_order_components()
        {
            match component {
                "slot" => {}
                "block_time" => blockers.push("EVENT_ORDER_BLOCK_TIME_UNKNOWN".to_string()),
                "signature" => blockers.push("EVENT_ORDER_SIGNATURE_UNKNOWN".to_string()),
                "transaction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_TRANSACTION_INDEX_UNKNOWN".to_string())
                }
                "instruction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_INSTRUCTION_INDEX_UNKNOWN".to_string())
                }
                "inner_instruction_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_INNER_INSTRUCTION_INDEX_UNKNOWN".to_string())
                }
                "log_index_or_unknown" => {
                    blockers.push("EVENT_ORDER_LOG_INDEX_UNKNOWN".to_string())
                }
                _ => blockers.push(format!("EVENT_ORDER_COMPONENT_UNKNOWN:{component}")),
            }
        }
        for component in self
            .event_order_key
            .not_applicable_or_derived_chain_order_components()
        {
            blockers.push(format!(
                "EVENT_ORDER_COMPONENT_NOT_CHAIN_OBSERVED:{component}"
            ));
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
        if self.account_data_len.is_none() {
            blockers.push("POOL_STATE_ACCOUNT_DATA_LEN_MISSING".to_string());
        }
        if self
            .source_account_pubkey
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            blockers.push("POOL_STATE_SOURCE_ACCOUNT_PUBKEY_MISSING".to_string());
        }
        if self
            .source_account_owner_or_program
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            blockers.push("POOL_STATE_SOURCE_ACCOUNT_OWNER_MISSING".to_string());
        }
        if self.source_account_slot.is_none() {
            blockers.push("POOL_STATE_SOURCE_ACCOUNT_SLOT_MISSING".to_string());
        }
        if self.source_write_version.is_none() {
            blockers.push("POOL_STATE_SOURCE_WRITE_VERSION_MISSING".to_string());
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
    pub decision_mark_price: Option<f64>,
    pub entry_quote_price: Option<f64>,
    pub entry_quote_tokens_out: Option<u64>,
    pub entry_quote_min_out: Option<u64>,
    pub simulated_submit_ts_ms: Option<ClockedTimestamp>,
    pub simulated_landing_slot: Option<u64>,
    pub simulated_landing_delay_ms: Option<u64>,
    pub entry_failure_mode: Option<String>,
    pub executable_fill_model_version: Option<String>,
}

impl ShadowEntryAttemptV2 {
    pub fn attach_static_entry_quote(
        &mut self,
        quote: &ShadowV2Quote,
        model_version: impl Into<String>,
    ) {
        self.intended_quote = Some(quote.fill_price_sol_per_token);
        self.decision_mark_price = Some(quote.mark_price_sol_per_token);
        self.entry_quote_price = Some(quote.fill_price_sol_per_token);
        self.entry_quote_tokens_out = Some(quote.expected_output_amount);
        self.entry_quote_min_out = Some(quote.min_output_amount);
        self.executable_fill_model_version = Some(model_version.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowEntryFillModelConfig {
    pub pool_phase: ShadowV2PoolPhase,
    pub input_sol_lamports: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_out_raw: Option<u64>,
    pub slippage_bps: u16,
    pub fee_bps: u16,
    pub executable_fill_model_version: String,
}

impl ShadowEntryFillModelConfig {
    pub fn bonding_curve(
        input_sol_lamports: u64,
        slippage_bps: u16,
        fee_bps: u16,
        executable_fill_model_version: impl Into<String>,
    ) -> Self {
        Self {
            pool_phase: ShadowV2PoolPhase::BondingCurve,
            input_sol_lamports,
            min_out_raw: None,
            slippage_bps,
            fee_bps,
            executable_fill_model_version: executable_fill_model_version.into(),
        }
    }

    pub fn with_min_out_raw(mut self, min_out_raw: Option<u64>) -> Self {
        self.min_out_raw = min_out_raw;
        self
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_simulation_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_provenance_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_label_grade: Option<ShadowV2ExecutionLabelGrade>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance_blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_fill_reason: Option<ShadowV2NoFillReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output_raw: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_amount_raw: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage_tolerance_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_price_impact_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_slippage_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_fill_divergence_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_state_after_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_model_version: Option<String>,
}

impl ShadowEntryFillV2 {
    pub fn from_static_buy_model(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        pool_state_before: &PoolStateSampleV2,
        config: &ShadowEntryFillModelConfig,
    ) -> Self {
        envelope.schema = "shadow_entry_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::ResearchGradeCandidate;
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state_before.envelope.event_id
        ));

        let outcome = ShadowV2FillEngine::simulate(ShadowV2ExecutionInput {
            side: ShadowV2ExecutionSide::Buy,
            pool_phase: config.pool_phase,
            pool_state_before: Some(pool_state_before),
            boundary_kind: ShadowV2BoundaryKind::EntryBefore,
            event_order_key: event_order_key.clone(),
            input_amount_raw: Some(config.input_sol_lamports),
            min_out_raw: config.min_out_raw,
            fee_bps: Some(config.fee_bps),
            slippage_tolerance_bps: Some(config.slippage_bps),
            model_version: config.executable_fill_model_version.clone(),
        });
        Self::from_execution_outcome(envelope, event_order_key, outcome)
    }

    pub fn from_execution_outcome(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        outcome: ShadowV2ExecutionOutcome,
    ) -> Self {
        envelope.schema = "shadow_entry_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = measurement_grade_for_execution_outcome(&outcome);
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = outcome.quality.clone();
        envelope.limitations.extend(outcome.limitations.clone());

        let pool_state_after = outcome
            .pool_state_after_derived
            .as_ref()
            .map(|derived| derived.ref_label());
        Self {
            envelope,
            event_order_key,
            fill_status: outcome.fill_status,
            fill_price: outcome.fill_price,
            fill_price_source: outcome.fill_price_source,
            fill_amount_sol: outcome.fill_amount_sol,
            fill_amount_tokens: outcome.fill_amount_tokens,
            slippage_bps: outcome.slippage_tolerance_bps,
            own_impact_bps: outcome.own_impact_bps,
            fee_bps: outcome.fee_bps,
            min_out: outcome.min_out_raw,
            pool_state_before: outcome.pool_state_before_ref,
            pool_state_after,
            reconstruction_status: outcome.reconstruction_status,
            quality: outcome.quality,
            limitations: outcome.limitations,
            execution_simulation_ready: Some(outcome.execution_simulation_ready),
            research_provenance_ready: Some(outcome.research_provenance_ready),
            execution_label_grade: Some(outcome.execution_label_grade),
            provenance_ready: Some(outcome.provenance_ready),
            provenance_blockers: outcome.provenance_blockers,
            blocked_reasons: outcome.blocked_reasons,
            no_fill_reason: outcome.no_fill_reason,
            fail_reason: outcome.fail_reason,
            expected_output_raw: outcome.expected_output_raw,
            output_amount_raw: outcome.output_amount_raw,
            slippage_tolerance_bps: outcome.slippage_tolerance_bps,
            deterministic_price_impact_bps: outcome.deterministic_price_impact_bps,
            realized_slippage_bps: outcome.realized_slippage_bps,
            quote_fill_divergence_bps: outcome.quote_fill_divergence_bps,
            pool_state_after_source: outcome.pool_state_after_source,
            execution_model_version: Some(outcome.model_version),
        }
    }

    pub fn blocked_without_pool_state(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        mut blockers: Vec<String>,
    ) -> Self {
        envelope.schema = "shadow_entry_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::BlockedByData;
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = "BLOCKED_BY_DATA".to_string();
        blockers.push("ENTRY_FILL_POOL_STATE_SAMPLE_MISSING".to_string());
        blockers.push("ENTRY_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE".to_string());
        blockers.push("ENTRY_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        blockers.sort();
        blockers.dedup();
        envelope.limitations.extend(blockers.clone());

        Self {
            envelope,
            event_order_key,
            fill_status: FillStatus::BlockedByData,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            slippage_bps: None,
            own_impact_bps: None,
            fee_bps: None,
            min_out: None,
            pool_state_before: None,
            pool_state_after: None,
            reconstruction_status: "ENTRY_FILL_BLOCKED_BY_MISSING_POOL_STATE".to_string(),
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations: blockers,
            execution_simulation_ready: Some(false),
            research_provenance_ready: Some(false),
            execution_label_grade: Some(ShadowV2ExecutionLabelGrade::DiagnosticSim),
            provenance_ready: Some(false),
            provenance_blockers: Vec::new(),
            blocked_reasons: vec!["BLOCKED_POOL_STATE_MISSING".to_string()],
            no_fill_reason: None,
            fail_reason: None,
            expected_output_raw: None,
            output_amount_raw: None,
            slippage_tolerance_bps: None,
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_after_source: None,
            execution_model_version: None,
        }
    }

    pub fn blocked_with_pool_state(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        pool_state_before: &PoolStateSampleV2,
        mut blockers: Vec<String>,
    ) -> Self {
        envelope.schema = "shadow_entry_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::BlockedByData;
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = "BLOCKED_BY_DATA".to_string();
        envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state_before.envelope.event_id
        ));
        blockers.extend(pool_state_before.research_blockers());
        blockers.push("ENTRY_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        blockers.sort();
        blockers.dedup();
        envelope.limitations.extend(blockers.clone());

        Self {
            envelope,
            event_order_key,
            fill_status: FillStatus::BlockedByData,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            slippage_bps: None,
            own_impact_bps: None,
            fee_bps: None,
            min_out: None,
            pool_state_before: Some(pool_state_before.envelope.event_id.clone()),
            pool_state_after: None,
            reconstruction_status: "ENTRY_FILL_BLOCKED_BY_DATA_WITH_POOL_STATE_REF".to_string(),
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations: blockers,
            execution_simulation_ready: Some(false),
            research_provenance_ready: Some(false),
            execution_label_grade: Some(ShadowV2ExecutionLabelGrade::DiagnosticSim),
            provenance_ready: Some(false),
            provenance_blockers: Vec::new(),
            blocked_reasons: vec!["BLOCKED_BY_DATA_WITH_POOL_STATE_REF".to_string()],
            no_fill_reason: None,
            fail_reason: None,
            expected_output_raw: None,
            output_amount_raw: None,
            slippage_tolerance_bps: None,
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_after_source: None,
            execution_model_version: None,
        }
    }
}

fn measurement_grade_for_execution_outcome(outcome: &ShadowV2ExecutionOutcome) -> MeasurementGrade {
    match outcome.fill_status {
        FillStatus::BlockedByData => MeasurementGrade::BlockedByData,
        FillStatus::Filled | FillStatus::NoFill => {
            if outcome.research_provenance_ready
                && outcome.execution_label_grade == ShadowV2ExecutionLabelGrade::ResearchCandidate
            {
                MeasurementGrade::ResearchGradeCandidate
            } else {
                MeasurementGrade::DiagnosticOnly
            }
        }
        FillStatus::Failed => MeasurementGrade::DiagnosticOnly,
    }
}

pub(crate) fn chain_order_tuple_for_execution(
    order: &EventOrderKey,
) -> Option<(u32, u32, u32, u32)> {
    Some((
        *order.transaction_index_or_unknown.as_known()?,
        *order.instruction_index_or_unknown.as_known()?,
        *order.inner_instruction_index_or_unknown.as_known()?,
        *order.log_index_or_unknown.as_known()?,
    ))
}

fn chain_order_tuple(order: &EventOrderKey) -> Option<(u32, u32, u32, u32)> {
    chain_order_tuple_for_execution(order)
}

fn reserves_from_pool_state(
    pool_state: &PoolStateSampleV2,
    pool_phase: ShadowV2PoolPhase,
) -> Option<ShadowV2Reserves> {
    let (sol_reserves, token_reserves) = match pool_phase {
        ShadowV2PoolPhase::BondingCurve => (
            pool_state.virtual_sol_reserves?,
            pool_state.virtual_token_reserves?,
        ),
        ShadowV2PoolPhase::Amm => (
            pool_state.real_sol_reserves?,
            pool_state.real_token_reserves?,
        ),
    };
    Some(ShadowV2Reserves::new(
        sol_reserves,
        token_reserves,
        pool_state.token_decimals?,
        pool_state.sol_lamports?,
    ))
}

fn pnl_bps_from_prices(
    entry_price_sol_per_token: f64,
    current_price_sol_per_token: f64,
) -> Option<i32> {
    if !entry_price_sol_per_token.is_finite()
        || !current_price_sol_per_token.is_finite()
        || entry_price_sol_per_token <= 0.0
    {
        return None;
    }
    Some(
        (((current_price_sol_per_token - entry_price_sol_per_token) / entry_price_sol_per_token)
            * SHADOW_V2_BPS_DENOMINATOR as f64)
            .round() as i32,
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowPathSampleV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
    pub sampling_mode: ShadowPathSamplingModeV2,
    pub path_horizon_ms: u64,
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
    pub truncated: bool,
}

impl ShadowPathSampleV2 {
    pub fn from_pool_state_mark(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        sample_ts_ms: ClockedTimestamp,
        age_ms: u64,
        pool_state: &PoolStateSampleV2,
        pool_phase: ShadowV2PoolPhase,
        entry_mark_price_sol_per_token: Option<f64>,
        sampling_mode: ShadowPathSamplingModeV2,
        sampling_reason: ShadowPathSamplingReasonV2,
    ) -> Self {
        envelope.schema = "shadow_path_sample_v2".to_string();
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = MeasurementGrade::MarkPriceReplay;
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::StreamObservedMs;
        envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state.envelope.event_id
        ));

        let mut limitations = vec![
            format!("PATH_SAMPLING_MODE={}", sampling_mode.label()),
            format!("PATH_SAMPLING_REASON={}", sampling_reason.label()),
            "MARK_PRICE_REPLAY_NOT_EXECUTABLE_FILL".to_string(),
        ];
        limitations.extend(pool_state.ambiguity_labels());

        let blockers = pool_state.research_blockers();
        let mark_price = reserves_from_pool_state(pool_state, pool_phase)
            .and_then(|reserves| reserves.mark_price_sol_per_token().ok())
            .or(pool_state.price_sol_per_token);
        if mark_price.is_none() {
            limitations.push("PATH_SAMPLE_MARK_PRICE_MISSING_OR_UNRECONSTRUCTABLE".to_string());
        }
        limitations.extend(blockers.clone());

        let pnl_mark_bps = mark_price
            .and_then(|price| pnl_bps_from_prices(entry_mark_price_sol_per_token?, price));
        let exact_or_approx = if mark_price.is_none() || !blockers.is_empty() {
            "BLOCKED_BY_DATA".to_string()
        } else if event_order_key.has_complete_chain_order() {
            "EXACT_EVENT_ORDER".to_string()
        } else {
            "APPROX_AMBIGUOUS_EVENT_ORDER".to_string()
        };
        let quality = if mark_price.is_some() && blockers.is_empty() {
            "MARK_PRICE_REPLAY_SAMPLE".to_string()
        } else {
            "BLOCKED_BY_DATA".to_string()
        };
        envelope.quality = quality.clone();
        envelope.limitations.extend(limitations.clone());

        Self {
            envelope,
            event_order_key,
            sampling_mode,
            path_horizon_ms: ShadowPathSamplerConfigV2::for_mode(sampling_mode).max_horizon_ms,
            sample_ts_ms,
            sample_slot: pool_state
                .event_order_key
                .slot
                .as_known()
                .copied()
                .or(pool_state.observed_slot),
            age_ms,
            pool_state_ref: pool_state.envelope.event_id.clone(),
            mark_price,
            executable_exit_quote: None,
            pnl_mark_bps,
            pnl_executable_bps: None,
            mfe_mark_bps: pnl_mark_bps,
            mae_mark_bps: pnl_mark_bps,
            source_quality: pool_state.source_quality.clone(),
            sampling_reason: sampling_reason.label().to_string(),
            exact_or_approx,
            truncated: false,
        }
    }

    pub fn from_legacy_lifecycle_mark(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        sample_ts_ms: ClockedTimestamp,
        sample_slot: Option<u64>,
        age_ms: u64,
        mark_price: Option<f64>,
        pnl_mark_bps: Option<i32>,
        sampling_mode: ShadowPathSamplingModeV2,
        sampling_reason: ShadowPathSamplingReasonV2,
        source_quality: impl Into<String>,
        mut limitations: Vec<String>,
    ) -> Self {
        envelope.schema = "shadow_path_sample_v2".to_string();
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = if mark_price.is_some() || pnl_mark_bps.is_some() {
            MeasurementGrade::MarkPriceReplay
        } else {
            MeasurementGrade::BlockedByData
        };
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::StreamObservedMs;
        envelope.quality = if mark_price.is_some() || pnl_mark_bps.is_some() {
            "LEGACY_LIFECYCLE_MARK_PATH_SAMPLE".to_string()
        } else {
            "BLOCKED_BY_DATA".to_string()
        };
        limitations.push("LEGACY_LIFECYCLE_PRICE_TRUTH_NOT_POOL_STATE_SAMPLE".to_string());
        limitations.push("PATH_SAMPLE_POOL_STATE_PROVENANCE_MISSING".to_string());
        limitations.push("MARK_PRICE_REPLAY_NOT_EXECUTABLE_FILL".to_string());
        limitations.extend(event_order_key.ambiguity_labels());
        limitations.sort();
        limitations.dedup();
        envelope.limitations.extend(limitations.clone());

        let exact_or_approx = if mark_price.is_none() && pnl_mark_bps.is_none() {
            "BLOCKED_BY_DATA".to_string()
        } else if event_order_key.has_complete_chain_order() {
            "EXACT_EVENT_ORDER".to_string()
        } else {
            "APPROX_AMBIGUOUS_EVENT_ORDER".to_string()
        };

        Self {
            envelope,
            event_order_key,
            sampling_mode,
            path_horizon_ms: ShadowPathSamplerConfigV2::for_mode(sampling_mode).max_horizon_ms,
            sample_ts_ms,
            sample_slot,
            age_ms,
            pool_state_ref: "MISSING_POOL_STATE_SAMPLE_LEGACY_LIFECYCLE_PRICE_TRUTH_ONLY"
                .to_string(),
            mark_price,
            executable_exit_quote: None,
            pnl_mark_bps,
            pnl_executable_bps: None,
            mfe_mark_bps: pnl_mark_bps,
            mae_mark_bps: pnl_mark_bps,
            source_quality: source_quality.into(),
            sampling_reason: sampling_reason.label().to_string(),
            exact_or_approx,
            truncated: false,
        }
    }

    pub fn attach_static_exit_quote(
        &mut self,
        quote: &ShadowV2Quote,
        entry_fill_price_sol_per_token: Option<f64>,
    ) {
        self.envelope.simulation_level = SimulationLevel::FillModelStatic;
        self.envelope.measurement_grade = MeasurementGrade::ResearchGradeCandidate;
        self.executable_exit_quote = Some(quote.fill_price_sol_per_token);
        self.pnl_executable_bps = entry_fill_price_sol_per_token
            .and_then(|entry| pnl_bps_from_prices(entry, quote.fill_price_sol_per_token));
        self.envelope.source_refs.push(format!(
            "shadow_v2_price_quote:{}:{}",
            quote.formula_version,
            quote.price_source_label()
        ));
        self.envelope
            .limitations
            .push("EXECUTABLE_EXIT_QUOTE_IS_STATIC_MODEL_NOT_LIVE_FILL".to_string());
    }
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

impl ShadowExitAttemptV2 {
    pub fn from_mark_path_trigger(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        exit_trigger: impl Into<String>,
        trigger_ts_ms: ClockedTimestamp,
        trigger_slot: Option<u64>,
        trigger_source: impl Into<String>,
        target_bps: Option<i32>,
        stop_bps: Option<i32>,
        max_hold_ms: Option<u64>,
        same_slot_ambiguity: bool,
        tie_break_policy: Option<String>,
    ) -> Self {
        envelope.schema = "shadow_exit_attempt_v2".to_string();
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = MeasurementGrade::MarkPriceReplay;
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::StreamObservedMs;
        if same_slot_ambiguity {
            envelope
                .limitations
                .push("EXIT_ATTEMPT_SAME_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string());
        }

        Self {
            envelope,
            event_order_key,
            exit_trigger: exit_trigger.into(),
            trigger_ts_ms,
            trigger_slot,
            trigger_source: trigger_source.into(),
            target_bps,
            stop_bps,
            max_hold_ms,
            tie_break_policy,
            same_slot_ambiguity,
            executable_fill_model_version: None,
        }
    }

    pub fn attach_static_exit_model(&mut self, model_version: impl Into<String>) {
        self.executable_fill_model_version = Some(model_version.into());
    }

    pub fn research_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.exit_trigger.trim().is_empty() {
            blockers.push("EXIT_ATTEMPT_TRIGGER_MISSING".to_string());
        }
        if self.trigger_ts_ms.value.is_none() {
            blockers.push("EXIT_ATTEMPT_TRIGGER_TS_MISSING".to_string());
        }
        if self.event_order_key.slot.is_unknown() {
            blockers.push("EXIT_ATTEMPT_EVENT_ORDER_SLOT_UNKNOWN".to_string());
        }
        if self.same_slot_ambiguity
            && self
                .tie_break_policy
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            blockers.push("EXIT_ATTEMPT_TIE_BREAK_POLICY_MISSING".to_string());
        }
        blockers
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowExitPathReplayConfigV2 {
    pub target_bps: Option<i32>,
    pub stop_bps: Option<i32>,
    pub max_hold_ms: u64,
    pub tie_break_policy: ShadowExitTieBreakPolicyV2,
}

impl ShadowExitPathReplayConfigV2 {
    pub const fn new(
        target_bps: Option<i32>,
        stop_bps: Option<i32>,
        max_hold_ms: u64,
        tie_break_policy: ShadowExitTieBreakPolicyV2,
    ) -> Self {
        Self {
            target_bps,
            stop_bps,
            max_hold_ms,
            tie_break_policy,
        }
    }

    pub fn research_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.max_hold_ms == 0 {
            blockers.push("EXIT_PATH_REPLAY_MAX_HOLD_MS_ZERO".to_string());
        }
        if self.target_bps.is_none() && self.stop_bps.is_none() && self.max_hold_ms == 0 {
            blockers.push("EXIT_PATH_REPLAY_NO_TARGET_STOP_OR_TIMEOUT_CONFIG".to_string());
        }
        if self.target_bps.is_some_and(|target| target <= 0) {
            blockers.push("EXIT_PATH_REPLAY_TARGET_BPS_NOT_POSITIVE".to_string());
        }
        if self.stop_bps.is_some_and(|stop| stop >= 0) {
            blockers.push("EXIT_PATH_REPLAY_STOP_BPS_NOT_NEGATIVE".to_string());
        }
        blockers
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowExitPathHitV2 {
    pub terminal_reason: TerminalReasonV2,
    pub hit_source: ShadowExitHitSourceV2,
    pub age_ms: Option<u64>,
    pub event_order_key: Option<EventOrderKey>,
    pub path_sample_ref: Option<String>,
    pub pnl_mark_bps: Option<i32>,
    pub same_slot_ambiguity: bool,
    pub limitations: Vec<String>,
}

impl ShadowExitPathHitV2 {
    fn from_sample(
        terminal_reason: TerminalReasonV2,
        hit_source: ShadowExitHitSourceV2,
        sample: &ShadowPathSampleV2,
        limitations: Vec<String>,
    ) -> Self {
        Self {
            terminal_reason,
            hit_source,
            age_ms: Some(sample.age_ms),
            event_order_key: Some(sample.event_order_key.clone()),
            path_sample_ref: Some(sample.envelope.event_id.clone()),
            pnl_mark_bps: sample.pnl_mark_bps,
            same_slot_ambiguity: false,
            limitations,
        }
    }

    fn blocked(limitations: Vec<String>) -> Self {
        Self {
            terminal_reason: TerminalReasonV2::BlockedByData,
            hit_source: ShadowExitHitSourceV2::BlockedByData,
            age_ms: None,
            event_order_key: None,
            path_sample_ref: None,
            pnl_mark_bps: None,
            same_slot_ambiguity: true,
            limitations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowExitPathReplayResultV2 {
    pub exact_level_hit: Option<ShadowExitPathHitV2>,
    pub sampled_path_hit: Option<ShadowExitPathHitV2>,
    pub timeout_path_point: Option<ShadowExitPathHitV2>,
    pub selected_exit: ShadowExitPathHitV2,
    pub mfe_mark_bps: Option<i32>,
    pub mae_mark_bps: Option<i32>,
    pub terminal_pnl_mark_bps: Option<i32>,
    pub quality: String,
    pub limitations: Vec<String>,
}

pub fn replay_exit_from_path_v2(
    samples: &[ShadowPathSampleV2],
    config: &ShadowExitPathReplayConfigV2,
) -> ShadowExitPathReplayResultV2 {
    let mut limitations = config.research_blockers();
    let has_config_blockers = !limitations.is_empty();
    let input_stats = path_density_input_stats(samples);
    if input_stats.non_monotonic_input {
        limitations.push("EXIT_PATH_REPLAY_INPUT_NON_MONOTONIC".to_string());
    }
    if input_stats.duplicate_age_count > 0 {
        limitations.push(format!(
            "EXIT_PATH_REPLAY_DUPLICATE_AGE_COUNT={}",
            input_stats.duplicate_age_count
        ));
    }
    if has_config_blockers {
        let selected_exit = ShadowExitPathHitV2::blocked(limitations.clone());
        return ShadowExitPathReplayResultV2 {
            exact_level_hit: None,
            sampled_path_hit: None,
            timeout_path_point: None,
            selected_exit,
            mfe_mark_bps: None,
            mae_mark_bps: None,
            terminal_pnl_mark_bps: None,
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations,
        };
    }

    let mut ordered = samples.iter().collect::<Vec<_>>();
    ordered.sort_by(|lhs, rhs| compare_samples_for_replay(lhs, rhs));

    if ordered.is_empty() {
        limitations.push("EXIT_PATH_REPLAY_NO_PATH_POINTS".to_string());
        let selected_exit =
            ShadowExitPathHitV2::blocked(vec!["EXIT_PATH_REPLAY_NO_PATH_POINTS".to_string()]);
        return ShadowExitPathReplayResultV2 {
            exact_level_hit: None,
            sampled_path_hit: None,
            timeout_path_point: None,
            selected_exit,
            mfe_mark_bps: None,
            mae_mark_bps: None,
            terminal_pnl_mark_bps: None,
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations,
        };
    }

    let replay_horizon_ms = ordered.last().map(|sample| sample.age_ms);
    let mut first_target: Option<ShadowExitPathHitV2> = None;
    let mut first_stop: Option<ShadowExitPathHitV2> = None;
    let mut exact_level_hit: Option<ShadowExitPathHitV2> = None;
    let mut sampled_path_hit: Option<ShadowExitPathHitV2> = None;

    for sample in ordered
        .iter()
        .copied()
        .filter(|sample| sample.age_ms <= config.max_hold_ms)
    {
        let Some(pnl_bps) = sample.pnl_mark_bps else {
            limitations.push(format!(
                "EXIT_PATH_SAMPLE_PNL_MARK_MISSING={}",
                sample.envelope.event_id
            ));
            continue;
        };
        if config
            .target_bps
            .is_some_and(|target_bps| pnl_bps >= target_bps)
        {
            let hit = path_hit_from_sample(TerminalReasonV2::Target, sample);
            record_hit_source(&hit, &mut exact_level_hit, &mut sampled_path_hit);
            if first_target.is_none() {
                first_target = Some(hit);
            }
        }
        if config.stop_bps.is_some_and(|stop_bps| pnl_bps <= stop_bps) {
            let hit = path_hit_from_sample(TerminalReasonV2::Stop, sample);
            record_hit_source(&hit, &mut exact_level_hit, &mut sampled_path_hit);
            if first_stop.is_none() {
                first_stop = Some(hit);
            }
        }
    }

    let selected_exit = choose_target_stop_hit(
        first_target.as_ref(),
        first_stop.as_ref(),
        config.tie_break_policy,
        &mut limitations,
    )
    .unwrap_or_else(|| {
        timeout_hit_from_path(
            &ordered,
            config.max_hold_ms,
            replay_horizon_ms,
            &mut limitations,
        )
    });

    let terminal_age_ms = selected_exit.age_ms.unwrap_or(config.max_hold_ms);
    let path_until_terminal = ordered
        .iter()
        .copied()
        .filter(|sample| sample.age_ms <= terminal_age_ms)
        .collect::<Vec<_>>();
    let mfe_mark_bps = path_until_terminal
        .iter()
        .filter_map(|sample| sample.pnl_mark_bps)
        .max();
    let mae_mark_bps = path_until_terminal
        .iter()
        .filter_map(|sample| sample.pnl_mark_bps)
        .min();
    let terminal_pnl_mark_bps = selected_exit.pnl_mark_bps;
    let timeout_path_point = if selected_exit.terminal_reason == TerminalReasonV2::Timeout {
        Some(selected_exit.clone())
    } else {
        timeout_candidate_from_path(&ordered, config.max_hold_ms, &mut Vec::new())
    };
    let quality = if selected_exit.terminal_reason == TerminalReasonV2::BlockedByData {
        "BLOCKED_BY_DATA"
    } else if selected_exit.hit_source == ShadowExitHitSourceV2::ExactLevel {
        "EXIT_PATH_REPLAY_EXACT_LEVEL"
    } else if selected_exit
        .limitations
        .iter()
        .any(|item| item.contains("USES_LAST_KNOWN_PATH_POINT"))
    {
        "EXIT_PATH_REPLAY_APPROX_TIMEOUT"
    } else {
        "EXIT_PATH_REPLAY_SAMPLED_PATH"
    }
    .to_string();

    ShadowExitPathReplayResultV2 {
        exact_level_hit,
        sampled_path_hit,
        timeout_path_point,
        selected_exit,
        mfe_mark_bps,
        mae_mark_bps,
        terminal_pnl_mark_bps,
        quality,
        limitations,
    }
}

fn path_hit_from_sample(
    terminal_reason: TerminalReasonV2,
    sample: &ShadowPathSampleV2,
) -> ShadowExitPathHitV2 {
    let hit_source = if sample.sampling_reason == ShadowPathSamplingReasonV2::LevelHit.label()
        && sample.exact_or_approx == "EXACT_EVENT_ORDER"
    {
        ShadowExitHitSourceV2::ExactLevel
    } else {
        ShadowExitHitSourceV2::SampledPath
    };
    let mut limitations = vec![format!("EXIT_HIT_SOURCE={}", hit_source.label())];
    if hit_source == ShadowExitHitSourceV2::SampledPath {
        limitations.push("TARGET_STOP_HIT_IS_SAMPLED_PATH_APPROXIMATION".to_string());
    }
    ShadowExitPathHitV2::from_sample(terminal_reason, hit_source, sample, limitations)
}

fn record_hit_source(
    hit: &ShadowExitPathHitV2,
    exact_level_hit: &mut Option<ShadowExitPathHitV2>,
    sampled_path_hit: &mut Option<ShadowExitPathHitV2>,
) {
    match hit.hit_source {
        ShadowExitHitSourceV2::ExactLevel if exact_level_hit.is_none() => {
            *exact_level_hit = Some(hit.clone());
        }
        ShadowExitHitSourceV2::SampledPath if sampled_path_hit.is_none() => {
            *sampled_path_hit = Some(hit.clone());
        }
        _ => {}
    }
}

fn choose_target_stop_hit(
    target_hit: Option<&ShadowExitPathHitV2>,
    stop_hit: Option<&ShadowExitPathHitV2>,
    tie_break_policy: ShadowExitTieBreakPolicyV2,
    limitations: &mut Vec<String>,
) -> Option<ShadowExitPathHitV2> {
    match (target_hit, stop_hit) {
        (Some(target), None) => Some(target.clone()),
        (None, Some(stop)) => Some(stop.clone()),
        (Some(target), Some(stop)) => match compare_exit_hits(target, stop) {
            Some(std::cmp::Ordering::Less) => Some(target.clone()),
            Some(std::cmp::Ordering::Greater) => Some(stop.clone()),
            Some(std::cmp::Ordering::Equal) | None => {
                limitations.push("EXIT_PATH_TARGET_STOP_ORDER_AMBIGUOUS".to_string());
                match tie_break_policy {
                    ShadowExitTieBreakPolicyV2::BlockAmbiguous
                    | ShadowExitTieBreakPolicyV2::EarliestEventOrder => {
                        let mut blocked = ShadowExitPathHitV2::blocked(vec![format!(
                            "EXIT_PATH_TARGET_STOP_AMBIGUOUS_TIE_BREAK={}",
                            tie_break_policy.label()
                        )]);
                        blocked.same_slot_ambiguity = true;
                        Some(blocked)
                    }
                    ShadowExitTieBreakPolicyV2::TargetFirst => {
                        let mut hit = target.clone();
                        hit.same_slot_ambiguity = true;
                        hit.limitations.push(
                            "EXIT_PATH_TARGET_STOP_AMBIGUITY_RESOLVED_TARGET_FIRST".to_string(),
                        );
                        Some(hit)
                    }
                    ShadowExitTieBreakPolicyV2::StopFirst => {
                        let mut hit = stop.clone();
                        hit.same_slot_ambiguity = true;
                        hit.limitations.push(
                            "EXIT_PATH_TARGET_STOP_AMBIGUITY_RESOLVED_STOP_FIRST".to_string(),
                        );
                        Some(hit)
                    }
                }
            }
        },
        (None, None) => None,
    }
}

fn compare_exit_hits(
    lhs: &ShadowExitPathHitV2,
    rhs: &ShadowExitPathHitV2,
) -> Option<std::cmp::Ordering> {
    match (lhs.age_ms, rhs.age_ms) {
        (Some(lhs_age), Some(rhs_age)) if lhs_age != rhs_age => Some(lhs_age.cmp(&rhs_age)),
        (Some(_), Some(_)) => {
            compare_event_order_keys(lhs.event_order_key.as_ref()?, rhs.event_order_key.as_ref()?)
        }
        _ => None,
    }
}

fn compare_event_order_keys(
    lhs: &EventOrderKey,
    rhs: &EventOrderKey,
) -> Option<std::cmp::Ordering> {
    match (lhs.slot.as_known(), rhs.slot.as_known()) {
        (Some(lhs_slot), Some(rhs_slot)) if lhs_slot != rhs_slot => Some(lhs_slot.cmp(rhs_slot)),
        (Some(_), Some(_)) => {
            if lhs.same_slot_ambiguous_with(rhs) {
                return None;
            }
            Some(chain_order_tuple(lhs)?.cmp(&chain_order_tuple(rhs)?))
        }
        _ => None,
    }
}

fn timeout_hit_from_path(
    ordered_samples: &[&ShadowPathSampleV2],
    max_hold_ms: u64,
    replay_horizon_ms: Option<u64>,
    limitations: &mut Vec<String>,
) -> ShadowExitPathHitV2 {
    if replay_horizon_ms.is_none_or(|horizon| horizon < max_hold_ms) {
        limitations.push("TIMEOUT_MAX_HOLD_EXCEEDS_REPLAY_HORIZON".to_string());
        return ShadowExitPathHitV2::blocked(vec![
            "TIMEOUT_PNL_BLOCKED_BY_INSUFFICIENT_REPLAY_HORIZON".to_string(),
        ]);
    }
    timeout_candidate_from_path(ordered_samples, max_hold_ms, limitations).unwrap_or_else(|| {
        ShadowExitPathHitV2::blocked(vec![
            "TIMEOUT_PNL_NO_PATH_POINT_AT_OR_BEFORE_MAX_HOLD".to_string()
        ])
    })
}

fn timeout_candidate_from_path(
    ordered_samples: &[&ShadowPathSampleV2],
    max_hold_ms: u64,
    limitations: &mut Vec<String>,
) -> Option<ShadowExitPathHitV2> {
    let sample = ordered_samples
        .iter()
        .copied()
        .filter(|sample| sample.age_ms <= max_hold_ms)
        .max_by(|lhs, rhs| compare_samples_for_replay(lhs, rhs))?;
    let mut hit_limitations = vec!["TIMEOUT_PNL_USES_REAL_PATH_POINT".to_string()];
    if sample.age_ms < max_hold_ms {
        hit_limitations.push("TIMEOUT_PNL_USES_LAST_KNOWN_PATH_POINT_BEFORE_MAX_HOLD".to_string());
        limitations.push("TIMEOUT_PNL_STALE_BEFORE_MAX_HOLD".to_string());
    }
    Some(ShadowExitPathHitV2::from_sample(
        TerminalReasonV2::Timeout,
        ShadowExitHitSourceV2::TimeoutPathPoint,
        sample,
        hit_limitations,
    ))
}

fn compare_samples_for_replay(
    lhs: &ShadowPathSampleV2,
    rhs: &ShadowPathSampleV2,
) -> std::cmp::Ordering {
    lhs.age_ms.cmp(&rhs.age_ms).then_with(|| {
        lhs.event_order_key
            .event_seq_in_process
            .cmp(&rhs.event_order_key.event_seq_in_process)
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowExitFillModelConfig {
    pub pool_phase: ShadowV2PoolPhase,
    pub input_token_raw: u64,
    pub slippage_bps: u16,
    pub fee_bps: u16,
    pub executable_fill_model_version: String,
    pub modeled_failure_mode: Option<ShadowExitFillFailureModeV2>,
}

impl ShadowExitFillModelConfig {
    pub fn bonding_curve(
        input_token_raw: u64,
        slippage_bps: u16,
        fee_bps: u16,
        executable_fill_model_version: impl Into<String>,
    ) -> Self {
        Self {
            pool_phase: ShadowV2PoolPhase::BondingCurve,
            input_token_raw,
            slippage_bps,
            fee_bps,
            executable_fill_model_version: executable_fill_model_version.into(),
            modeled_failure_mode: None,
        }
    }

    pub fn with_modeled_failure(mut self, failure_mode: ShadowExitFillFailureModeV2) -> Self {
        self.modeled_failure_mode = Some(failure_mode);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowExitFillV2 {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_simulation_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_provenance_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_label_grade: Option<ShadowV2ExecutionLabelGrade>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance_blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_fill_reason: Option<ShadowV2NoFillReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output_raw: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_amount_raw: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage_tolerance_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_price_impact_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_slippage_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_fill_divergence_bps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_state_after_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_model_version: Option<String>,
}

impl ShadowExitFillV2 {
    pub fn from_static_sell_model(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        pool_state_before: &PoolStateSampleV2,
        config: &ShadowExitFillModelConfig,
    ) -> Self {
        envelope.schema = "shadow_exit_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::ResearchGradeCandidate;
        envelope.temporal_class = TemporalClass::PostExit;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state_before.envelope.event_id
        ));

        if let Some(failure_mode) = config.modeled_failure_mode {
            return Self::modeled_failure(
                envelope,
                event_order_key,
                pool_state_before,
                config,
                failure_mode,
            );
        }

        let outcome = ShadowV2FillEngine::simulate(ShadowV2ExecutionInput {
            side: ShadowV2ExecutionSide::Sell,
            pool_phase: config.pool_phase,
            pool_state_before: Some(pool_state_before),
            boundary_kind: ShadowV2BoundaryKind::ExitBefore,
            event_order_key: event_order_key.clone(),
            input_amount_raw: Some(config.input_token_raw),
            min_out_raw: None,
            fee_bps: Some(config.fee_bps),
            slippage_tolerance_bps: Some(config.slippage_bps),
            model_version: config.executable_fill_model_version.clone(),
        });
        Self::from_execution_outcome(envelope, event_order_key, outcome)
    }

    pub fn from_execution_outcome(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        outcome: ShadowV2ExecutionOutcome,
    ) -> Self {
        envelope.schema = "shadow_exit_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = measurement_grade_for_execution_outcome(&outcome);
        envelope.temporal_class = TemporalClass::PostExit;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = outcome.quality.clone();
        envelope.limitations.extend(outcome.limitations.clone());

        let pool_state_after = outcome
            .pool_state_after_derived
            .as_ref()
            .map(|derived| derived.ref_label());
        Self {
            envelope,
            event_order_key,
            fill_status: outcome.fill_status,
            fill_price: outcome.fill_price,
            fill_price_source: outcome.fill_price_source,
            fill_amount_sol: outcome.fill_amount_sol,
            fill_amount_tokens: outcome.fill_amount_tokens,
            slippage_bps: outcome.slippage_tolerance_bps,
            own_impact_bps: outcome.own_impact_bps,
            fee_bps: outcome.fee_bps,
            min_out: outcome.min_out_raw,
            pool_state_before: outcome.pool_state_before_ref,
            pool_state_after,
            reconstruction_status: outcome.reconstruction_status,
            quality: outcome.quality,
            limitations: outcome.limitations,
            execution_simulation_ready: Some(outcome.execution_simulation_ready),
            research_provenance_ready: Some(outcome.research_provenance_ready),
            execution_label_grade: Some(outcome.execution_label_grade),
            provenance_ready: Some(outcome.provenance_ready),
            provenance_blockers: outcome.provenance_blockers,
            blocked_reasons: outcome.blocked_reasons,
            no_fill_reason: outcome.no_fill_reason,
            fail_reason: outcome.fail_reason,
            expected_output_raw: outcome.expected_output_raw,
            output_amount_raw: outcome.output_amount_raw,
            slippage_tolerance_bps: outcome.slippage_tolerance_bps,
            deterministic_price_impact_bps: outcome.deterministic_price_impact_bps,
            realized_slippage_bps: outcome.realized_slippage_bps,
            quote_fill_divergence_bps: outcome.quote_fill_divergence_bps,
            pool_state_after_source: outcome.pool_state_after_source,
            execution_model_version: Some(outcome.model_version),
        }
    }

    pub fn blocked_without_pool_state(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        mut blockers: Vec<String>,
    ) -> Self {
        envelope.schema = "shadow_exit_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::BlockedByData;
        envelope.temporal_class = TemporalClass::PostExit;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = "BLOCKED_BY_DATA".to_string();
        blockers.push("EXIT_FILL_POOL_STATE_SAMPLE_MISSING".to_string());
        blockers.push("EXIT_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE".to_string());
        blockers.push("EXIT_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        blockers.push("STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string());
        blockers.sort();
        blockers.dedup();
        envelope.limitations.extend(blockers.clone());

        Self {
            envelope,
            event_order_key,
            fill_status: FillStatus::BlockedByData,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            slippage_bps: None,
            own_impact_bps: None,
            fee_bps: None,
            min_out: None,
            pool_state_before: None,
            pool_state_after: None,
            reconstruction_status: "EXIT_FILL_BLOCKED_BY_MISSING_POOL_STATE".to_string(),
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations: blockers,
            execution_simulation_ready: Some(false),
            research_provenance_ready: Some(false),
            execution_label_grade: Some(ShadowV2ExecutionLabelGrade::DiagnosticSim),
            provenance_ready: Some(false),
            provenance_blockers: Vec::new(),
            blocked_reasons: vec!["BLOCKED_POOL_STATE_MISSING".to_string()],
            no_fill_reason: None,
            fail_reason: None,
            expected_output_raw: None,
            output_amount_raw: None,
            slippage_tolerance_bps: None,
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_after_source: None,
            execution_model_version: None,
        }
    }

    pub fn blocked_with_pool_state(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        pool_state_before: &PoolStateSampleV2,
        mut blockers: Vec<String>,
    ) -> Self {
        envelope.schema = "shadow_exit_fill_v2".to_string();
        envelope.simulation_level = SimulationLevel::FillModelStatic;
        envelope.measurement_grade = MeasurementGrade::BlockedByData;
        envelope.temporal_class = TemporalClass::PostExit;
        envelope.clock_domain = ClockDomain::LandingTsMs;
        envelope.quality = "BLOCKED_BY_DATA".to_string();
        envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state_before.envelope.event_id
        ));
        blockers.extend(pool_state_before.research_blockers());
        blockers.push("EXIT_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        blockers.push("STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string());
        blockers.sort();
        blockers.dedup();
        envelope.limitations.extend(blockers.clone());

        Self {
            envelope,
            event_order_key,
            fill_status: FillStatus::BlockedByData,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            slippage_bps: None,
            own_impact_bps: None,
            fee_bps: None,
            min_out: None,
            pool_state_before: Some(pool_state_before.envelope.event_id.clone()),
            pool_state_after: None,
            reconstruction_status: "EXIT_FILL_BLOCKED_BY_DATA_WITH_POOL_STATE_REF".to_string(),
            quality: "BLOCKED_BY_DATA".to_string(),
            limitations: blockers,
            execution_simulation_ready: Some(false),
            research_provenance_ready: Some(false),
            execution_label_grade: Some(ShadowV2ExecutionLabelGrade::DiagnosticSim),
            provenance_ready: Some(false),
            provenance_blockers: Vec::new(),
            blocked_reasons: vec!["BLOCKED_BY_DATA_WITH_POOL_STATE_REF".to_string()],
            no_fill_reason: None,
            fail_reason: None,
            expected_output_raw: None,
            output_amount_raw: None,
            slippage_tolerance_bps: None,
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_after_source: None,
            execution_model_version: None,
        }
    }

    fn modeled_failure(
        mut envelope: ShadowV2Envelope,
        event_order_key: EventOrderKey,
        pool_state_before: &PoolStateSampleV2,
        config: &ShadowExitFillModelConfig,
        failure_mode: ShadowExitFillFailureModeV2,
    ) -> Self {
        let (fill_status, reconstruction_status, quality, limitation) = match failure_mode {
            ShadowExitFillFailureModeV2::NoFill => (
                FillStatus::NoFill,
                "EXIT_FILL_MODELED_NO_FILL",
                "FILL_MODEL_STATIC_EXIT_NO_FILL",
                "EXIT_FILL_MODELED_NO_FILL_NOT_LIVE_CONFIRMED",
            ),
            ShadowExitFillFailureModeV2::Failed => (
                FillStatus::Failed,
                "EXIT_FILL_MODELED_FAILED",
                "FILL_MODEL_STATIC_EXIT_FAILED",
                "EXIT_FILL_MODELED_FAILURE_NOT_LIVE_CONFIRMED",
            ),
        };
        let limitations = vec![
            limitation.to_string(),
            "FILL_MODEL_STATIC_NOT_LIVE_CONFIRMED".to_string(),
            "STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string(),
            "MODELED_EXIT_FAILURE_NOT_L1_EXECUTION_SIM".to_string(),
            format!(
                "EXIT_FILL_MODEL_VERSION={}",
                config.executable_fill_model_version
            ),
        ];
        envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;
        envelope.quality = quality.to_string();
        envelope.limitations.extend(limitations.clone());

        Self {
            envelope,
            event_order_key,
            fill_status,
            fill_price: None,
            fill_price_source: None,
            fill_amount_sol: None,
            fill_amount_tokens: None,
            slippage_bps: Some(config.slippage_bps as i32),
            own_impact_bps: None,
            fee_bps: Some(config.fee_bps as i32),
            min_out: None,
            pool_state_before: Some(pool_state_before.envelope.event_id.clone()),
            pool_state_after: None,
            reconstruction_status: reconstruction_status.to_string(),
            quality: quality.to_string(),
            limitations,
            execution_simulation_ready: Some(false),
            research_provenance_ready: Some(false),
            execution_label_grade: Some(ShadowV2ExecutionLabelGrade::DiagnosticSim),
            provenance_ready: Some(false),
            provenance_blockers: Vec::new(),
            blocked_reasons: Vec::new(),
            no_fill_reason: None,
            fail_reason: match fill_status {
                FillStatus::NoFill => Some("MODELED_EXIT_NO_FILL_NOT_L1_EXECUTION_SIM".to_string()),
                FillStatus::Failed => Some("MODELED_EXIT_FAILURE".to_string()),
                _ => None,
            },
            expected_output_raw: None,
            output_amount_raw: None,
            slippage_tolerance_bps: Some(config.slippage_bps as i32),
            deterministic_price_impact_bps: None,
            realized_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_after_source: None,
            execution_model_version: Some(config.executable_fill_model_version.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowPathSamplerConfigV2 {
    pub mode: ShadowPathSamplingModeV2,
    pub max_horizon_ms: u64,
    pub heartbeat_ms: u64,
    pub exact_interval_ms: u64,
    pub approx_interval_ms: u64,
    pub large_price_delta_bps: i32,
    pub max_path_points: usize,
    pub keep_every_event_sample: bool,
    pub requires_storage_budget: bool,
}

impl ShadowPathSamplerConfigV2 {
    pub const fn for_mode(mode: ShadowPathSamplingModeV2) -> Self {
        match mode {
            ShadowPathSamplingModeV2::Dense3s => Self::dense_3s(),
            ShadowPathSamplingModeV2::Standard120s => Self::standard_120s(),
            ShadowPathSamplingModeV2::Long500s => Self::long_500s(),
        }
    }

    pub const fn dense_3s() -> Self {
        Self {
            mode: ShadowPathSamplingModeV2::Dense3s,
            max_horizon_ms: 3_000,
            heartbeat_ms: 250,
            exact_interval_ms: 1_000,
            approx_interval_ms: 1_500,
            large_price_delta_bps: 50,
            max_path_points: 512,
            keep_every_event_sample: true,
            requires_storage_budget: true,
        }
    }

    pub const fn standard_120s() -> Self {
        Self {
            mode: ShadowPathSamplingModeV2::Standard120s,
            max_horizon_ms: 121_000,
            heartbeat_ms: 1_000,
            exact_interval_ms: 1_000,
            approx_interval_ms: 5_000,
            large_price_delta_bps: 100,
            max_path_points: 4_096,
            keep_every_event_sample: false,
            requires_storage_budget: false,
        }
    }

    pub const fn long_500s() -> Self {
        Self {
            mode: ShadowPathSamplingModeV2::Long500s,
            max_horizon_ms: 500_000,
            heartbeat_ms: 5_000,
            exact_interval_ms: 5_000,
            approx_interval_ms: 30_000,
            large_price_delta_bps: 250,
            max_path_points: 8_192,
            keep_every_event_sample: false,
            requires_storage_budget: true,
        }
    }

    pub fn should_keep_sample(
        &self,
        age_ms: u64,
        pnl_bps: i32,
        reason: ShadowPathSamplingReasonV2,
        previous_kept_age_ms: Option<u64>,
        previous_kept_pnl_bps: Option<i32>,
    ) -> bool {
        if reason.is_must_keep() || previous_kept_age_ms.is_none() {
            return true;
        }
        if self.keep_every_event_sample && reason == ShadowPathSamplingReasonV2::EventSample {
            return true;
        }
        let age_delta = age_ms.saturating_sub(previous_kept_age_ms.unwrap_or_default());
        if age_delta >= self.heartbeat_ms {
            return true;
        }
        if let Some(previous_pnl) = previous_kept_pnl_bps {
            (pnl_bps - previous_pnl).abs() >= self.large_price_delta_bps
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowPathHorizonEvaluationV2 {
    pub horizon_ms: u64,
    pub verdict: ShadowPathHorizonVerdictV2,
    pub path_points: usize,
    pub coverage_points: usize,
    pub replay_horizon_ms: Option<u64>,
    pub first_path_point_age_ms: Option<u64>,
    pub median_interval_ms: Option<u64>,
    pub p90_interval_ms: Option<u64>,
    pub max_interval_ms: Option<u64>,
    pub duplicate_age_count: usize,
    pub non_monotonic_input: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowPathDensityV2 {
    pub schema: String,
    pub schema_version: u32,
    pub run_id: String,
    pub session_id: Option<String>,
    pub position_id: String,
    pub pool_id: String,
    pub base_mint: String,
    pub canonical_event_stream_ref: String,
    pub source_path_sample_event_ids: Vec<String>,
    pub source_canonical_high_watermark: String,
    pub horizon_ms: u64,
    pub verdict: ShadowPathHorizonVerdictV2,
    pub path_points: usize,
    pub coverage_points: usize,
    pub replay_horizon_ms: Option<u64>,
    pub first_path_point_age_ms: Option<u64>,
    pub median_interval_ms: Option<u64>,
    pub p90_interval_ms: Option<u64>,
    pub max_interval_ms: Option<u64>,
    pub duplicate_age_count: usize,
    pub non_monotonic_input: bool,
    pub truncated: bool,
    pub limitations: Vec<String>,
    pub created_at_wall_ms: u64,
}

impl ShadowPathDensityV2 {
    pub fn from_evaluation(
        high_watermark_event: &ShadowPositionEventV2,
        canonical_event_stream_ref: impl Into<String>,
        source_path_sample_event_ids: Vec<String>,
        truncated: bool,
        evaluation: ShadowPathHorizonEvaluationV2,
        created_at_wall_ms: u64,
    ) -> Self {
        let canonical_event_stream_ref = canonical_event_stream_ref.into();
        Self {
            schema: "shadow_path_density_v2".to_string(),
            schema_version: 1,
            run_id: high_watermark_event.envelope.run_id.clone(),
            session_id: high_watermark_event.envelope.session_id.clone(),
            position_id: high_watermark_event.envelope.position_id.clone(),
            pool_id: high_watermark_event.envelope.pool_id.clone(),
            base_mint: high_watermark_event.envelope.base_mint.clone(),
            canonical_event_stream_ref,
            source_path_sample_event_ids,
            source_canonical_high_watermark: high_watermark_event.envelope.event_id.clone(),
            horizon_ms: evaluation.horizon_ms,
            verdict: evaluation.verdict,
            path_points: evaluation.path_points,
            coverage_points: evaluation.coverage_points,
            replay_horizon_ms: evaluation.replay_horizon_ms,
            first_path_point_age_ms: evaluation.first_path_point_age_ms,
            median_interval_ms: evaluation.median_interval_ms,
            p90_interval_ms: evaluation.p90_interval_ms,
            max_interval_ms: evaluation.max_interval_ms,
            duplicate_age_count: evaluation.duplicate_age_count,
            non_monotonic_input: evaluation.non_monotonic_input,
            truncated,
            limitations: evaluation.limitations,
            created_at_wall_ms,
        }
    }
}

pub fn evaluate_path_density_v2(
    samples: &[ShadowPathSampleV2],
    config: &ShadowPathSamplerConfigV2,
    horizons_ms: &[u64],
) -> Vec<ShadowPathHorizonEvaluationV2> {
    let input_stats = path_density_input_stats(samples);
    let mut ages: Vec<u64> = samples.iter().map(|sample| sample.age_ms).collect();
    ages.sort_unstable();
    ages.dedup();
    let replay_horizon_ms = ages.last().copied();
    horizons_ms
        .iter()
        .map(|horizon_ms| {
            evaluate_single_horizon_density(
                &ages,
                replay_horizon_ms,
                config,
                *horizon_ms,
                input_stats,
            )
        })
        .collect()
}

pub fn select_path_samples_v2(
    samples: &[ShadowPathSampleV2],
    config: &ShadowPathSamplerConfigV2,
) -> Vec<ShadowPathSampleV2> {
    let mut ordered = samples.to_vec();
    ordered.sort_by(compare_samples_for_replay);

    let mut kept = Vec::new();
    let mut dropped_for_horizon = false;
    for sample in ordered {
        if sample.age_ms > config.max_horizon_ms {
            dropped_for_horizon = true;
            continue;
        }
        let reason = ShadowPathSamplingReasonV2::from_label(&sample.sampling_reason)
            .unwrap_or(ShadowPathSamplingReasonV2::EventSample);
        let previous_kept_age_ms = kept.last().map(|sample: &ShadowPathSampleV2| sample.age_ms);
        let previous_kept_pnl_bps = kept
            .last()
            .and_then(|sample: &ShadowPathSampleV2| sample.pnl_mark_bps);
        if config.should_keep_sample(
            sample.age_ms,
            sample.pnl_mark_bps.unwrap_or_default(),
            reason,
            previous_kept_age_ms,
            previous_kept_pnl_bps,
        ) {
            kept.push(sample);
        }
    }

    let mut max_points_truncated = false;
    let mut storage_budget_exceeded_for_protected = false;
    if kept.len() > config.max_path_points {
        let protected_count = kept
            .iter()
            .filter(|sample| sample_protected_from_path_cap(sample, config))
            .count();
        let optional_budget = config.max_path_points.saturating_sub(protected_count);
        let mut optional_kept = 0usize;
        let mut capped = Vec::with_capacity(config.max_path_points.max(protected_count));

        for sample in kept {
            if sample_protected_from_path_cap(&sample, config) {
                capped.push(sample);
            } else if optional_kept < optional_budget {
                optional_kept += 1;
                capped.push(sample);
            } else {
                max_points_truncated = true;
            }
        }

        storage_budget_exceeded_for_protected = capped.len() > config.max_path_points;
        kept = capped;
    }
    if (dropped_for_horizon || max_points_truncated || storage_budget_exceeded_for_protected)
        && !kept.is_empty()
    {
        if let Some(last) = kept.last_mut() {
            if dropped_for_horizon || max_points_truncated {
                last.truncated = true;
                last.envelope
                    .limitations
                    .push("PATH_SAMPLER_TRUNCATED_BY_HORIZON_OR_OPTIONAL_MAX_POINTS".to_string());
            }
            if storage_budget_exceeded_for_protected {
                last.envelope.limitations.push(
                    "PATH_SAMPLER_STORAGE_BUDGET_EXCEEDED_PROTECTED_SAMPLES_RETAINED".to_string(),
                );
            }
        }
    }
    kept
}

fn sample_protected_from_path_cap(
    sample: &ShadowPathSampleV2,
    config: &ShadowPathSamplerConfigV2,
) -> bool {
    let reason = ShadowPathSamplingReasonV2::from_label(&sample.sampling_reason)
        .unwrap_or(ShadowPathSamplingReasonV2::EventSample);
    reason.is_must_keep()
        || (config.keep_every_event_sample && reason == ShadowPathSamplingReasonV2::EventSample)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2ArtifactBudgetConfig {
    pub enabled: bool,
    pub rotation_enabled: bool,
    pub max_total_artifact_bytes: u64,
    pub max_file_bytes: u64,
    pub max_rows_per_file: u64,
    pub max_density_rows: u64,
    pub max_stdout_bytes: u64,
    pub max_system_log_bytes: u64,
}

impl Default for ShadowV2ArtifactBudgetConfig {
    fn default() -> Self {
        const GIB: u64 = 1024 * 1024 * 1024;
        const MIB: u64 = 1024 * 1024;
        Self {
            enabled: true,
            rotation_enabled: true,
            max_total_artifact_bytes: 5 * GIB,
            max_file_bytes: 2 * GIB,
            max_rows_per_file: 2_000_000,
            max_density_rows: 250_000,
            max_stdout_bytes: 256 * MIB,
            max_system_log_bytes: 512 * MIB,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2ArtifactRotationManifestRow {
    pub schema: String,
    pub schema_version: u32,
    pub run_id: String,
    pub artifact: String,
    pub logical_path: String,
    pub rotated_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_path: Option<String>,
    pub uncompressed_size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_size_bytes: Option<u64>,
    pub row_count: u64,
    pub hash_algorithm: String,
    pub hash_uncompressed: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash_compressed: Option<String>,
    pub rotation_index: u64,
    pub rotated_at_wall_ms: u64,
}

impl ShadowV2ArtifactRotationManifestRow {
    fn new(
        run_id: impl Into<String>,
        artifact: impl Into<String>,
        logical_path: &Path,
        rotated_path: &Path,
        uncompressed_size_bytes: u64,
        row_count: u64,
        rotation_index: u64,
        hash_uncompressed: impl Into<String>,
    ) -> Self {
        Self {
            schema: SHADOW_V2_ARTIFACT_ROTATION_MANIFEST_SCHEMA.to_string(),
            schema_version: 1,
            run_id: run_id.into(),
            artifact: artifact.into(),
            logical_path: logical_path.display().to_string(),
            rotated_path: rotated_path.display().to_string(),
            compressed_path: None,
            uncompressed_size_bytes,
            compressed_size_bytes: None,
            row_count,
            hash_algorithm: "blake3".to_string(),
            hash_uncompressed: hash_uncompressed.into(),
            hash_compressed: None,
            rotation_index,
            rotated_at_wall_ms: shadow_v2_now_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2SourceRefSummary {
    pub count: usize,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    pub range_hash: Option<String>,
    pub manifest_ref: Option<String>,
}

impl ShadowV2SourceRefSummary {
    fn from_ids(ids: &[String], manifest_ref: Option<String>) -> Self {
        Self {
            count: ids.len(),
            first_id: ids.first().cloned(),
            last_id: ids.last().cloned(),
            range_hash: (!ids.is_empty()).then(|| shadow_v2_source_ref_range_hash(ids)),
            manifest_ref,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2SourceRefManifestRow {
    pub schema: String,
    pub schema_version: u32,
    pub run_id: String,
    pub position_id: String,
    pub source_canonical_high_watermark: String,
    pub canonical_event_stream_ref: String,
    pub source_event_count: usize,
    pub source_event_first_id: Option<String>,
    pub source_event_last_id: Option<String>,
    pub source_event_range_hash: Option<String>,
    pub path_sample_count: usize,
    pub path_sample_first_id: Option<String>,
    pub path_sample_last_id: Option<String>,
    pub path_sample_range_hash: Option<String>,
    pub compact_ref_policy: String,
    pub created_at_wall_ms: u64,
}

impl ShadowV2SourceRefManifestRow {
    fn new(
        high_watermark: &ShadowPositionEventV2,
        canonical_event_stream_ref: impl Into<String>,
        source_event_ids: &[String],
        path_sample_event_ids: &[String],
    ) -> Self {
        let source_summary = ShadowV2SourceRefSummary::from_ids(source_event_ids, None);
        let path_summary = ShadowV2SourceRefSummary::from_ids(path_sample_event_ids, None);
        Self {
            schema: SHADOW_V2_SOURCE_REF_MANIFEST_SCHEMA.to_string(),
            schema_version: 1,
            run_id: high_watermark.envelope.run_id.clone(),
            position_id: high_watermark.envelope.position_id.clone(),
            source_canonical_high_watermark: high_watermark.envelope.event_id.clone(),
            canonical_event_stream_ref: canonical_event_stream_ref.into(),
            source_event_count: source_summary.count,
            source_event_first_id: source_summary.first_id,
            source_event_last_id: source_summary.last_id,
            source_event_range_hash: source_summary.range_hash,
            path_sample_count: path_summary.count,
            path_sample_first_id: path_summary.first_id,
            path_sample_last_id: path_summary.last_id,
            path_sample_range_hash: path_summary.range_hash,
            compact_ref_policy: "COMPACT_RANGE_HASH_NO_REPEATED_FULL_ARRAYS".to_string(),
            created_at_wall_ms: shadow_v2_now_ms(),
        }
    }

    fn manifest_ref(&self) -> String {
        format!(
            "{}:{}:{}",
            SHADOW_V2_SOURCE_REF_MANIFEST_SCHEMA,
            self.position_id,
            self.source_canonical_high_watermark
        )
    }
}

fn shadow_v2_source_ref_range_hash(ids: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    for id in ids {
        hasher.update(id.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PathDensityInputStats {
    duplicate_age_count: usize,
    non_monotonic_input: bool,
}

fn path_density_input_stats(samples: &[ShadowPathSampleV2]) -> PathDensityInputStats {
    let non_monotonic_input = samples
        .windows(2)
        .any(|pair| pair[1].age_ms < pair[0].age_ms);
    let mut ages = samples
        .iter()
        .map(|sample| sample.age_ms)
        .collect::<Vec<_>>();
    let original_len = ages.len();
    ages.sort_unstable();
    ages.dedup();
    PathDensityInputStats {
        duplicate_age_count: original_len.saturating_sub(ages.len()),
        non_monotonic_input,
    }
}

fn evaluate_single_horizon_density(
    ages: &[u64],
    replay_horizon_ms: Option<u64>,
    config: &ShadowPathSamplerConfigV2,
    horizon_ms: u64,
    input_stats: PathDensityInputStats,
) -> ShadowPathHorizonEvaluationV2 {
    let mut limitations = vec![format!("PATH_SAMPLING_MODE={}", config.mode.label())];
    if config.requires_storage_budget {
        limitations.push("PATH_MODE_REQUIRES_STORAGE_BUDGET_BEFORE_VALIDATION_RUN".to_string());
    }
    if input_stats.duplicate_age_count > 0 {
        limitations.push(format!(
            "PATH_DENSITY_DUPLICATE_AGE_COUNT={}",
            input_stats.duplicate_age_count
        ));
    }
    if input_stats.non_monotonic_input {
        limitations.push("PATH_DENSITY_INPUT_NON_MONOTONIC".to_string());
    }
    if ages.is_empty() {
        limitations.push("PATH_DENSITY_NO_PATH_POINTS".to_string());
        return ShadowPathHorizonEvaluationV2 {
            horizon_ms,
            verdict: ShadowPathHorizonVerdictV2::NotEvaluableNoCoverage,
            path_points: 0,
            coverage_points: 0,
            replay_horizon_ms,
            first_path_point_age_ms: None,
            median_interval_ms: None,
            p90_interval_ms: None,
            max_interval_ms: None,
            duplicate_age_count: input_stats.duplicate_age_count,
            non_monotonic_input: input_stats.non_monotonic_input,
            limitations,
        };
    }

    if horizon_ms > config.max_horizon_ms {
        limitations.push("HORIZON_EXCEEDS_CONFIGURED_PATH_MODE".to_string());
        return horizon_not_evaluable(
            ages,
            replay_horizon_ms,
            horizon_ms,
            input_stats,
            limitations,
        );
    }

    if replay_horizon_ms.is_some_and(|replay_horizon| horizon_ms > replay_horizon) {
        limitations.push("HORIZON_EXCEEDS_REPLAY_COVERAGE".to_string());
        return horizon_not_evaluable(
            ages,
            replay_horizon_ms,
            horizon_ms,
            input_stats,
            limitations,
        );
    }

    let covered: Vec<u64> = ages
        .iter()
        .copied()
        .filter(|age_ms| *age_ms <= horizon_ms)
        .collect();
    if covered.is_empty() {
        limitations.push("PATH_DENSITY_NO_POINT_AT_OR_BEFORE_HORIZON".to_string());
        return ShadowPathHorizonEvaluationV2 {
            horizon_ms,
            verdict: ShadowPathHorizonVerdictV2::NotEvaluableNoCoverage,
            path_points: ages.len(),
            coverage_points: 0,
            replay_horizon_ms,
            first_path_point_age_ms: None,
            median_interval_ms: None,
            p90_interval_ms: None,
            max_interval_ms: None,
            duplicate_age_count: input_stats.duplicate_age_count,
            non_monotonic_input: input_stats.non_monotonic_input,
            limitations,
        };
    }

    let intervals = path_intervals_with_origin(&covered);
    let median_interval_ms = percentile_from_sorted(&intervals, 50);
    let p90_interval_ms = percentile_from_sorted(&intervals, 90);
    let max_interval_ms = intervals.iter().copied().max();
    let verdict = match max_interval_ms {
        Some(max_interval) if max_interval <= config.exact_interval_ms => {
            ShadowPathHorizonVerdictV2::EvaluableExact
        }
        Some(max_interval) if max_interval <= config.approx_interval_ms => {
            ShadowPathHorizonVerdictV2::EvaluableApprox
        }
        Some(_) => {
            limitations.push("PATH_DENSITY_INTERVAL_TOO_SPARSE_FOR_APPROX".to_string());
            ShadowPathHorizonVerdictV2::SparseApproxOnly
        }
        None => ShadowPathHorizonVerdictV2::NotEvaluableNoCoverage,
    };

    ShadowPathHorizonEvaluationV2 {
        horizon_ms,
        verdict,
        path_points: ages.len(),
        coverage_points: covered.len(),
        replay_horizon_ms,
        first_path_point_age_ms: covered.first().copied(),
        median_interval_ms,
        p90_interval_ms,
        max_interval_ms,
        duplicate_age_count: input_stats.duplicate_age_count,
        non_monotonic_input: input_stats.non_monotonic_input,
        limitations,
    }
}

fn horizon_not_evaluable(
    ages: &[u64],
    replay_horizon_ms: Option<u64>,
    horizon_ms: u64,
    input_stats: PathDensityInputStats,
    limitations: Vec<String>,
) -> ShadowPathHorizonEvaluationV2 {
    ShadowPathHorizonEvaluationV2 {
        horizon_ms,
        verdict: ShadowPathHorizonVerdictV2::NotEvaluableHorizonExceedsReplay,
        path_points: ages.len(),
        coverage_points: ages.iter().filter(|age_ms| **age_ms <= horizon_ms).count(),
        replay_horizon_ms,
        first_path_point_age_ms: ages.first().copied(),
        median_interval_ms: None,
        p90_interval_ms: None,
        max_interval_ms: None,
        duplicate_age_count: input_stats.duplicate_age_count,
        non_monotonic_input: input_stats.non_monotonic_input,
        limitations,
    }
}

fn path_intervals_with_origin(sorted_ages: &[u64]) -> Vec<u64> {
    let mut previous = 0;
    let mut intervals = Vec::with_capacity(sorted_ages.len());
    for age in sorted_ages {
        intervals.push(age.saturating_sub(previous));
        previous = *age;
    }
    intervals
}

fn percentile_from_sorted(values: &[u64], percentile: u64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let index = ((sorted.len() - 1) as u64 * percentile / 100) as usize;
    sorted.get(index).copied()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowTerminalTruthV2 {
    pub envelope: ShadowV2Envelope,
    pub event_order_key: EventOrderKey,
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

pub fn executable_pnl_bps_from_entry_exit_fills(
    entry_fill: &ShadowEntryFillV2,
    exit_fill: &ShadowExitFillV2,
) -> Option<i32> {
    if entry_fill.fill_status != FillStatus::Filled || exit_fill.fill_status != FillStatus::Filled {
        return None;
    }
    let entry_sol = entry_fill.fill_amount_sol?;
    let exit_sol = exit_fill.fill_amount_sol?;
    if !entry_sol.is_finite() || !exit_sol.is_finite() || entry_sol <= 0.0 {
        return None;
    }
    let pnl_bps = ((exit_sol - entry_sol) / entry_sol) * SHADOW_V2_BPS_DENOMINATOR as f64;
    pnl_bps.is_finite().then_some(pnl_bps.round() as i32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowV2ExecutablePnlLink {
    pub final_pnl_executable_bps: i32,
    pub linked_entry_fill: String,
    pub linked_exit_fill: String,
}

pub fn executable_pnl_link_from_canonical_position_fills(
    stream: &ShadowV2CanonicalEventStream,
    position_id: &str,
    pending_exit_fill: Option<&ShadowExitFillV2>,
) -> Option<ShadowV2ExecutablePnlLink> {
    let entry = stream
        .events_for_position(position_id)
        .into_iter()
        .rev()
        .find_map(|event| match shadow_v2_record_from_event(event) {
            Ok(ShadowV2Record::ShadowEntryFillV2(fill))
                if fill.fill_status == FillStatus::Filled =>
            {
                Some((event.envelope.event_id.clone(), fill))
            }
            _ => None,
        })?;

    let pending_exit = pending_exit_fill
        .filter(|fill| {
            fill.envelope.position_id == position_id && fill.fill_status == FillStatus::Filled
        })
        .map(|fill| (fill.envelope.event_id.clone(), fill.clone()));

    let exit = pending_exit.or_else(|| {
        stream
            .events_for_position(position_id)
            .into_iter()
            .rev()
            .find_map(|event| match shadow_v2_record_from_event(event) {
                Ok(ShadowV2Record::ShadowExitFillV2(fill))
                    if fill.fill_status == FillStatus::Filled =>
                {
                    Some((event.envelope.event_id.clone(), fill))
                }
                _ => None,
            })
    })?;

    let final_pnl_executable_bps = executable_pnl_bps_from_entry_exit_fills(&entry.1, &exit.1)?;
    Some(ShadowV2ExecutablePnlLink {
        final_pnl_executable_bps,
        linked_entry_fill: entry.0,
        linked_exit_fill: exit.0,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowReplayV2 {
    pub envelope: ShadowV2Envelope,
    pub canonical_event_stream_ref: String,
    pub source_canonical_high_watermark: String,
    pub mark_replay_ref: Option<String>,
    pub executable_replay_ref: Option<String>,
    pub coverage_metadata_ref: String,
    pub derived_from_canonical_stream: bool,
    pub canonical_terminal_event_id: Option<String>,
    pub source_event_ids: Vec<String>,
    pub path_sample_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_first_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_last_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_range_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_manifest_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sample_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sample_first_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sample_last_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sample_range_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sample_manifest_ref: Option<String>,
    pub entry_fill_event_id: Option<String>,
    pub exit_attempt_event_id: Option<String>,
    pub exit_fill_event_id: Option<String>,
    pub terminal_truth_event_id: Option<String>,
    pub mark_path_sample_count: usize,
    pub executable_quote_sample_count: usize,
    pub blocked_path_sample_count: usize,
    pub terminal_reason: Option<TerminalReasonV2>,
    pub terminal_pnl_mark_bps: Option<i32>,
    pub terminal_pnl_executable_bps: Option<i32>,
    pub close_age_ms: Option<u64>,
    pub replay_derivation_status: String,
}

impl ShadowReplayV2 {
    pub fn derive_from_canonical_stream(
        mut envelope: ShadowV2Envelope,
        stream: &ShadowV2CanonicalEventStream,
        position_id: &str,
        canonical_event_stream_ref: impl Into<String>,
    ) -> Result<Self, ShadowV2Error> {
        let canonical_event_stream_ref = canonical_event_stream_ref.into();
        let position_events: Vec<_> = stream
            .events_for_position(position_id)
            .into_iter()
            .filter(|event| {
                !matches!(
                    event.event_kind,
                    ShadowPositionEventKindV2::ReplayDerived
                        | ShadowPositionEventKindV2::LifecycleSubEvent
                )
            })
            .collect();
        if position_events.is_empty() {
            return Err(ShadowV2Error::MissingCanonicalPositionEvents {
                position_id: position_id.to_string(),
            });
        }

        let mut source_event_ids = Vec::with_capacity(position_events.len());
        let mut path_sample_event_ids = Vec::new();
        let mut entry_fill_event_id = None;
        let mut exit_attempt_event_id = None;
        let mut exit_fill_event_id = None;
        let mut mark_path_sample_count = 0usize;
        let mut executable_quote_sample_count = 0usize;
        let mut blocked_path_sample_count = 0usize;

        for event in &position_events {
            source_event_ids.push(event.envelope.event_id.clone());
            match event.event_kind {
                ShadowPositionEventKindV2::EntryFill if entry_fill_event_id.is_none() => {
                    entry_fill_event_id = Some(event.envelope.event_id.clone());
                }
                ShadowPositionEventKindV2::PathSample => {
                    path_sample_event_ids.push(event.envelope.event_id.clone());
                    if let ShadowV2Record::ShadowPathSampleV2(sample) =
                        shadow_v2_record_from_event(event)?
                    {
                        if sample.pnl_mark_bps.is_some() {
                            mark_path_sample_count += 1;
                        }
                        if sample.executable_exit_quote.is_some()
                            || sample.pnl_executable_bps.is_some()
                        {
                            executable_quote_sample_count += 1;
                        }
                        if sample.exact_or_approx == "BLOCKED_BY_DATA" {
                            blocked_path_sample_count += 1;
                        }
                    }
                }
                ShadowPositionEventKindV2::ExitAttempt if exit_attempt_event_id.is_none() => {
                    exit_attempt_event_id = Some(event.envelope.event_id.clone());
                }
                ShadowPositionEventKindV2::ExitFill if exit_fill_event_id.is_none() => {
                    exit_fill_event_id = Some(event.envelope.event_id.clone());
                }
                _ => {}
            }
        }

        let terminal = stream.canonical_terminal_event(position_id);
        let terminal_truth = terminal
            .map(|event| match shadow_v2_record_from_event(event)? {
                ShadowV2Record::ShadowTerminalTruthV2(record) => Ok(record),
                _ => Err(ShadowV2Error::Serialization(format!(
                    "canonical terminal event {} did not contain shadow_terminal_truth_v2 payload",
                    event.envelope.event_id
                ))),
            })
            .transpose()?;
        let canonical_terminal_event_id = terminal_truth
            .as_ref()
            .map(|record| record.envelope.event_id.clone());
        let source_canonical_high_watermark = source_event_ids.last().cloned().unwrap_or_default();

        envelope.schema = "shadow_replay_v2".to_string();
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = MeasurementGrade::MarkPriceReplay;
        envelope.temporal_class = if terminal_truth.is_some() {
            TemporalClass::PostExit
        } else {
            TemporalClass::PostEntry
        };
        envelope.clock_domain = ClockDomain::WallClockMs;
        envelope.source_event_id = canonical_terminal_event_id.clone();
        envelope.source_refs.push(format!(
            "canonical_event_stream:{canonical_event_stream_ref}"
        ));
        envelope.source_refs.extend(
            source_event_ids
                .iter()
                .map(|event_id| format!("canonical_event:{event_id}")),
        );
        envelope
            .limitations
            .push("REPLAY_V2_DERIVED_VIEW_NOT_CANONICAL_TRUTH".to_string());
        envelope
            .limitations
            .push("MARK_REPLAY_NOT_EXECUTABLE_FILL".to_string());
        if executable_quote_sample_count > 0 || exit_fill_event_id.is_some() {
            envelope
                .limitations
                .push("EXECUTABLE_REPLAY_LANE_IS_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        }

        let replay_derivation_status = if terminal_truth.is_some() {
            "REPLAY_DERIVED_FROM_CANONICAL_TERMINAL".to_string()
        } else {
            envelope
                .limitations
                .push("REPLAY_DERIVED_WITHOUT_CANONICAL_TERMINAL_TRUTH".to_string());
            "REPLAY_DERIVED_OPEN_OR_BLOCKED".to_string()
        };
        envelope.quality = replay_derivation_status.clone();

        Ok(Self {
            envelope,
            canonical_event_stream_ref: canonical_event_stream_ref.clone(),
            source_canonical_high_watermark,
            mark_replay_ref: (!path_sample_event_ids.is_empty()).then(|| {
                format!(
                    "{canonical_event_stream_ref}#mark_path_samples:{}",
                    position_id
                )
            }),
            executable_replay_ref: (executable_quote_sample_count > 0
                || exit_fill_event_id.is_some())
            .then(|| {
                format!(
                    "{canonical_event_stream_ref}#static_executable_lane:{}",
                    position_id
                )
            }),
            coverage_metadata_ref: format!("{canonical_event_stream_ref}#coverage:{position_id}"),
            derived_from_canonical_stream: true,
            canonical_terminal_event_id: canonical_terminal_event_id.clone(),
            source_event_ids,
            path_sample_event_ids,
            source_event_count: None,
            source_event_first_id: None,
            source_event_last_id: None,
            source_event_range_hash: None,
            source_event_manifest_ref: None,
            path_sample_count: None,
            path_sample_first_id: None,
            path_sample_last_id: None,
            path_sample_range_hash: None,
            path_sample_manifest_ref: None,
            entry_fill_event_id,
            exit_attempt_event_id,
            exit_fill_event_id,
            terminal_truth_event_id: canonical_terminal_event_id,
            mark_path_sample_count,
            executable_quote_sample_count,
            blocked_path_sample_count,
            terminal_reason: terminal_truth.as_ref().map(|record| record.terminal_reason),
            terminal_pnl_mark_bps: terminal_truth
                .as_ref()
                .and_then(|record| record.final_pnl_mark_bps),
            terminal_pnl_executable_bps: terminal_truth
                .as_ref()
                .and_then(|record| record.final_pnl_executable_bps),
            close_age_ms: terminal_truth
                .as_ref()
                .and_then(|record| record.close_age_ms),
            replay_derivation_status,
        })
    }

    fn compact_source_refs(&mut self, manifest_ref: String) {
        let source_summary =
            ShadowV2SourceRefSummary::from_ids(&self.source_event_ids, Some(manifest_ref.clone()));
        let path_summary =
            ShadowV2SourceRefSummary::from_ids(&self.path_sample_event_ids, Some(manifest_ref));

        self.source_event_count = Some(source_summary.count);
        self.source_event_first_id = source_summary.first_id;
        self.source_event_last_id = source_summary.last_id;
        self.source_event_range_hash = source_summary.range_hash;
        self.source_event_manifest_ref = source_summary.manifest_ref;
        self.path_sample_count = Some(path_summary.count);
        self.path_sample_first_id = path_summary.first_id;
        self.path_sample_last_id = path_summary.last_id;
        self.path_sample_range_hash = path_summary.range_hash;
        self.path_sample_manifest_ref = path_summary.manifest_ref;
        self.source_event_ids.clear();
        self.path_sample_event_ids.clear();
        self.envelope
            .source_refs
            .retain(|source_ref| !source_ref.starts_with("canonical_event:"));
        self.envelope.source_refs.push(format!(
            "source_ref_manifest:{}",
            self.source_event_manifest_ref
                .as_deref()
                .unwrap_or("UNKNOWN")
        ));
        self.envelope
            .limitations
            .push("SOURCE_REFS_COMPACTED_TO_MANIFEST_RANGE_HASH".to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowLifecycleV2 {
    pub envelope: ShadowV2Envelope,
    pub canonical_event_stream_ref: String,
    pub source_canonical_high_watermark: String,
    pub derived_from_canonical_stream: bool,
    pub lifecycle_event_type: ShadowLifecycleEventTypeV2,
    pub canonical_position_event_id: Option<String>,
    pub entry_fill_event_id: Option<String>,
    pub exit_attempt_event_id: Option<String>,
    pub exit_fill_event_id: Option<String>,
    pub canonical_terminal_event_id: Option<String>,
    pub terminal_reason: Option<TerminalReasonV2>,
    pub terminal_ts_ms: Option<ClockedTimestamp>,
    pub terminal_slot: Option<u64>,
    pub final_pnl_mark_bps: Option<i32>,
    pub final_pnl_executable_bps: Option<i32>,
    pub close_age_ms: Option<u64>,
    pub duplicate_terminal_handling: String,
    pub reconciliation_status: String,
    pub source_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_first_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_last_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_range_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_manifest_ref: Option<String>,
    pub derived_view_not_canonical_terminal: bool,
}

impl ShadowLifecycleV2 {
    pub fn derive_from_canonical_stream(
        mut envelope: ShadowV2Envelope,
        stream: &ShadowV2CanonicalEventStream,
        position_id: &str,
        canonical_event_stream_ref: impl Into<String>,
    ) -> Result<Self, ShadowV2Error> {
        let canonical_event_stream_ref = canonical_event_stream_ref.into();
        let position_events: Vec<_> = stream
            .events_for_position(position_id)
            .into_iter()
            .filter(|event| {
                !matches!(
                    event.event_kind,
                    ShadowPositionEventKindV2::ReplayDerived
                        | ShadowPositionEventKindV2::LifecycleSubEvent
                )
            })
            .collect();
        if position_events.is_empty() {
            return Err(ShadowV2Error::MissingCanonicalPositionEvents {
                position_id: position_id.to_string(),
            });
        }

        let mut source_event_ids = Vec::with_capacity(position_events.len());
        let mut canonical_position_event_id = None;
        let mut entry_fill_event_id = None;
        let mut exit_attempt_event_id = None;
        let mut exit_fill_event_id = None;

        for event in &position_events {
            source_event_ids.push(event.envelope.event_id.clone());
            match event.event_kind {
                ShadowPositionEventKindV2::PositionCreated
                    if canonical_position_event_id.is_none() =>
                {
                    canonical_position_event_id = Some(event.envelope.event_id.clone());
                }
                ShadowPositionEventKindV2::EntryFill if entry_fill_event_id.is_none() => {
                    entry_fill_event_id = Some(event.envelope.event_id.clone());
                }
                ShadowPositionEventKindV2::ExitAttempt if exit_attempt_event_id.is_none() => {
                    exit_attempt_event_id = Some(event.envelope.event_id.clone());
                }
                ShadowPositionEventKindV2::ExitFill if exit_fill_event_id.is_none() => {
                    exit_fill_event_id = Some(event.envelope.event_id.clone());
                }
                _ => {}
            }
        }

        let terminal = stream.canonical_terminal_event(position_id);
        let terminal_truth = terminal
            .map(|event| match shadow_v2_record_from_event(event)? {
                ShadowV2Record::ShadowTerminalTruthV2(record) => Ok(record),
                _ => Err(ShadowV2Error::Serialization(format!(
                    "canonical terminal event {} did not contain shadow_terminal_truth_v2 payload",
                    event.envelope.event_id
                ))),
            })
            .transpose()?;
        let canonical_terminal_event_id = terminal_truth
            .as_ref()
            .map(|record| record.envelope.event_id.clone());
        let source_canonical_high_watermark = source_event_ids.last().cloned().unwrap_or_default();
        let lifecycle_event_type =
            match terminal_truth.as_ref().map(|record| record.terminal_reason) {
                Some(
                    TerminalReasonV2::BlockedByData
                    | TerminalReasonV2::Failed
                    | TerminalReasonV2::NoFill,
                ) => ShadowLifecycleEventTypeV2::TerminalBlocked,
                Some(_) => ShadowLifecycleEventTypeV2::PositionClosed,
                None => ShadowLifecycleEventTypeV2::PositionOpen,
            };

        envelope.schema = "shadow_lifecycle_v2".to_string();
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = if terminal_truth.is_some() {
            MeasurementGrade::MarkPriceReplay
        } else {
            MeasurementGrade::DiagnosticOnly
        };
        envelope.temporal_class = if terminal_truth.is_some() {
            TemporalClass::PostExit
        } else {
            TemporalClass::PostEntry
        };
        envelope.clock_domain = ClockDomain::WallClockMs;
        envelope.source_event_id = canonical_terminal_event_id.clone();
        envelope.source_refs.push(format!(
            "canonical_event_stream:{canonical_event_stream_ref}"
        ));
        envelope.source_refs.extend(
            source_event_ids
                .iter()
                .map(|event_id| format!("canonical_event:{event_id}")),
        );
        envelope
            .limitations
            .push("LIFECYCLE_V2_DERIVED_VIEW_NOT_CANONICAL_TERMINAL_TRUTH".to_string());
        envelope
            .limitations
            .push("LIFECYCLE_V2_DOES_NOT_IMPLY_LIVE_POSITION_STATE".to_string());
        if terminal_truth.is_none() {
            envelope
                .limitations
                .push("LIFECYCLE_DERIVED_WITHOUT_CANONICAL_TERMINAL_TRUTH".to_string());
        }

        let reconciliation_status = if terminal_truth.is_some() {
            "LIFECYCLE_DERIVED_FROM_CANONICAL_TERMINAL".to_string()
        } else {
            "LIFECYCLE_DERIVED_OPEN_OR_BLOCKED".to_string()
        };
        envelope.quality = reconciliation_status.clone();

        Ok(Self {
            envelope,
            canonical_event_stream_ref,
            source_canonical_high_watermark,
            derived_from_canonical_stream: true,
            lifecycle_event_type,
            canonical_position_event_id,
            entry_fill_event_id,
            exit_attempt_event_id,
            exit_fill_event_id,
            canonical_terminal_event_id,
            terminal_reason: terminal_truth.as_ref().map(|record| record.terminal_reason),
            terminal_ts_ms: terminal_truth
                .as_ref()
                .map(|record| record.terminal_ts_ms.clone()),
            terminal_slot: terminal_truth
                .as_ref()
                .and_then(|record| record.terminal_slot),
            final_pnl_mark_bps: terminal_truth
                .as_ref()
                .and_then(|record| record.final_pnl_mark_bps),
            final_pnl_executable_bps: terminal_truth
                .as_ref()
                .and_then(|record| record.final_pnl_executable_bps),
            close_age_ms: terminal_truth
                .as_ref()
                .and_then(|record| record.close_age_ms),
            duplicate_terminal_handling:
                "DERIVED_LIFECYCLE_VIEW_DOES_NOT_CREATE_CANONICAL_TERMINAL_TRUTH".to_string(),
            reconciliation_status,
            source_event_ids,
            source_event_count: None,
            source_event_first_id: None,
            source_event_last_id: None,
            source_event_range_hash: None,
            source_event_manifest_ref: None,
            derived_view_not_canonical_terminal: true,
        })
    }

    fn compact_source_refs(&mut self, manifest_ref: String) {
        let source_summary =
            ShadowV2SourceRefSummary::from_ids(&self.source_event_ids, Some(manifest_ref));
        self.source_event_count = Some(source_summary.count);
        self.source_event_first_id = source_summary.first_id;
        self.source_event_last_id = source_summary.last_id;
        self.source_event_range_hash = source_summary.range_hash;
        self.source_event_manifest_ref = source_summary.manifest_ref;
        self.source_event_ids.clear();
        self.envelope
            .source_refs
            .retain(|source_ref| !source_ref.starts_with("canonical_event:"));
        self.envelope.source_refs.push(format!(
            "source_ref_manifest:{}",
            self.source_event_manifest_ref
                .as_deref()
                .unwrap_or("UNKNOWN")
        ));
        self.envelope
            .limitations
            .push("SOURCE_REFS_COMPACTED_TO_MANIFEST_RANGE_HASH".to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowReplayLifecycleReconciliationV2 {
    pub replay_event_id: String,
    pub lifecycle_event_id: String,
    pub position_id: String,
    pub exact_join: bool,
    pub fallback_join_used: bool,
    pub ambiguous_join: bool,
    pub canonical_terminal_event_id_match: bool,
    pub terminal_reason_match: bool,
    pub final_pnl_mark_match: bool,
    pub final_pnl_executable_match: bool,
    pub close_age_match: bool,
    pub reconciliation_status: String,
    pub limitations: Vec<String>,
}

pub fn reconcile_replay_lifecycle_v2(
    replay: &ShadowReplayV2,
    lifecycle: &ShadowLifecycleV2,
) -> Result<ShadowReplayLifecycleReconciliationV2, ShadowV2Error> {
    let replay_key = replay.envelope.exact_join_key()?;
    let lifecycle_key = lifecycle.envelope.exact_join_key()?;
    let exact_join = replay_key == lifecycle_key;
    let mut limitations = vec![
        "REPLAY_LIFECYCLE_RECONCILIATION_EXACT_KEY_ONLY".to_string(),
        "NO_FALLBACK_JOIN_ACCEPTED".to_string(),
    ];

    if !exact_join {
        limitations.push("REPLAY_LIFECYCLE_EXACT_JOIN_KEY_MISMATCH".to_string());
        return Ok(ShadowReplayLifecycleReconciliationV2 {
            replay_event_id: replay.envelope.event_id.clone(),
            lifecycle_event_id: lifecycle.envelope.event_id.clone(),
            position_id: replay.envelope.position_id.clone(),
            exact_join: false,
            fallback_join_used: false,
            ambiguous_join: false,
            canonical_terminal_event_id_match: false,
            terminal_reason_match: false,
            final_pnl_mark_match: false,
            final_pnl_executable_match: false,
            close_age_match: false,
            reconciliation_status: "REPLAY_LIFECYCLE_EXACT_JOIN_KEY_MISMATCH".to_string(),
            limitations,
        });
    }

    let canonical_terminal_event_id_match = replay.canonical_terminal_event_id.is_some()
        && replay.canonical_terminal_event_id == lifecycle.canonical_terminal_event_id;
    let terminal_reason_match = replay.terminal_reason == lifecycle.terminal_reason;
    let final_pnl_mark_match = replay.terminal_pnl_mark_bps == lifecycle.final_pnl_mark_bps;
    let final_pnl_executable_match =
        replay.terminal_pnl_executable_bps == lifecycle.final_pnl_executable_bps;
    let close_age_match = replay.close_age_ms == lifecycle.close_age_ms;
    let all_match = canonical_terminal_event_id_match
        && terminal_reason_match
        && final_pnl_mark_match
        && final_pnl_executable_match
        && close_age_match;
    if !all_match {
        limitations.push("REPLAY_LIFECYCLE_DERIVED_FIELD_MISMATCH".to_string());
    }

    Ok(ShadowReplayLifecycleReconciliationV2 {
        replay_event_id: replay.envelope.event_id.clone(),
        lifecycle_event_id: lifecycle.envelope.event_id.clone(),
        position_id: replay.envelope.position_id.clone(),
        exact_join: true,
        fallback_join_used: false,
        ambiguous_join: false,
        canonical_terminal_event_id_match,
        terminal_reason_match,
        final_pnl_mark_match,
        final_pnl_executable_match,
        close_age_match,
        reconciliation_status: if all_match {
            "REPLAY_LIFECYCLE_RECONCILED_FROM_CANONICAL_STREAM".to_string()
        } else {
            "REPLAY_LIFECYCLE_MISMATCH".to_string()
        },
        limitations,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowV2WriteStatus {
    Ok,
    Err(String),
    Skipped(String),
}

impl ShadowV2WriteStatus {
    pub fn is_err(&self) -> bool {
        matches!(self, Self::Err(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowV2ValidationEvidenceStatus {
    Complete,
    CanonicalWriteFailed,
    DerivedArtifactWriteFailed,
    DensityWriteFailed,
    BlockedByData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowV2HarnessAppendOutcome {
    pub canonical_write: ShadowV2WriteStatus,
    pub replay_write: ShadowV2WriteStatus,
    pub lifecycle_write: ShadowV2WriteStatus,
    pub density_write: ShadowV2WriteStatus,
    pub validation_evidence_status: ShadowV2ValidationEvidenceStatus,
}

impl ShadowV2HarnessAppendOutcome {
    fn canonical_failed(error: impl ToString) -> Self {
        Self {
            canonical_write: ShadowV2WriteStatus::Err(error.to_string()),
            replay_write: ShadowV2WriteStatus::Skipped(
                "CANONICAL_WRITE_FAILED_NO_DERIVED_REPLAY".to_string(),
            ),
            lifecycle_write: ShadowV2WriteStatus::Skipped(
                "CANONICAL_WRITE_FAILED_NO_DERIVED_LIFECYCLE".to_string(),
            ),
            density_write: ShadowV2WriteStatus::Skipped(
                "CANONICAL_WRITE_FAILED_NO_DENSITY".to_string(),
            ),
            validation_evidence_status: ShadowV2ValidationEvidenceStatus::CanonicalWriteFailed,
        }
    }

    fn from_writes(
        replay_write: ShadowV2WriteStatus,
        lifecycle_write: ShadowV2WriteStatus,
        density_write: ShadowV2WriteStatus,
    ) -> Self {
        let validation_evidence_status = if replay_write.is_err() || lifecycle_write.is_err() {
            ShadowV2ValidationEvidenceStatus::DerivedArtifactWriteFailed
        } else if density_write.is_err() {
            ShadowV2ValidationEvidenceStatus::DensityWriteFailed
        } else {
            ShadowV2ValidationEvidenceStatus::Complete
        };
        Self {
            canonical_write: ShadowV2WriteStatus::Ok,
            replay_write,
            lifecycle_write,
            density_write,
            validation_evidence_status,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShadowV2ValidationHarnessConfig {
    pub run_id: String,
    pub canonical_event_stream_path: PathBuf,
    pub replay_v2_path: PathBuf,
    pub lifecycle_v2_path: PathBuf,
    pub path_density_v2_path: PathBuf,
    pub source_ref_manifest_v2_path: PathBuf,
    pub artifact_rotation_manifest_v2_path: PathBuf,
    pub canonical_event_stream_ref: String,
    pub path_sampler_config: ShadowPathSamplerConfigV2,
    pub density_horizons_ms: Vec<u64>,
    pub compact_density_enabled: bool,
    pub density_full_stream_enabled: bool,
    pub replay_lifecycle_compact_refs_enabled: bool,
    pub artifact_budget: ShadowV2ArtifactBudgetConfig,
}

impl ShadowV2ValidationHarnessConfig {
    pub fn new(
        run_id: impl Into<String>,
        canonical_event_stream_path: impl Into<PathBuf>,
        replay_v2_path: impl Into<PathBuf>,
        lifecycle_v2_path: impl Into<PathBuf>,
        path_density_v2_path: impl Into<PathBuf>,
    ) -> Self {
        let canonical_event_stream_path = canonical_event_stream_path.into();
        let canonical_event_stream_ref = canonical_event_stream_path.display().to_string();
        let replay_v2_path = replay_v2_path.into();
        let density_full_stream_enabled = shadow_v2_density_full_stream_enabled_from_env();
        let density_horizons_ms = if density_full_stream_enabled {
            shadow_v2_full_density_horizons_ms()
        } else {
            shadow_v2_l2_declared_density_horizons_ms()
        };
        let source_ref_manifest_v2_path =
            replay_v2_path.with_file_name("shadow_source_ref_manifest_v2.jsonl");
        let artifact_rotation_manifest_v2_path =
            replay_v2_path.with_file_name("shadow_artifact_rotation_manifest_v2.jsonl");
        Self {
            run_id: run_id.into(),
            canonical_event_stream_path,
            replay_v2_path,
            lifecycle_v2_path: lifecycle_v2_path.into(),
            path_density_v2_path: path_density_v2_path.into(),
            source_ref_manifest_v2_path,
            artifact_rotation_manifest_v2_path,
            canonical_event_stream_ref,
            path_sampler_config: ShadowPathSamplerConfigV2::standard_120s(),
            density_horizons_ms,
            compact_density_enabled: !density_full_stream_enabled,
            density_full_stream_enabled,
            replay_lifecycle_compact_refs_enabled: true,
            artifact_budget: ShadowV2ArtifactBudgetConfig::default(),
        }
    }

    pub fn from_burnin_config(
        config: &crate::config::ghost_brain_config::ShadowV2BurninConfig,
    ) -> Result<Option<Self>, ShadowV2Error> {
        if !config.enabled {
            return Ok(None);
        }
        config
            .validate()
            .map_err(|error| ShadowV2Error::HarnessConfig {
                reason: error.to_string(),
            })?;
        let run_id = required_shadow_v2_burnin_path_component(
            config.run_namespace.as_deref(),
            "run_namespace",
        )?;
        let canonical_event_stream_path = required_shadow_v2_burnin_path_component(
            config.canonical_event_stream_path.as_deref(),
            "canonical_event_stream_path",
        )?;
        let replay_v2_path = required_shadow_v2_burnin_path_component(
            config.replay_v2_path.as_deref(),
            "replay_v2_path",
        )?;
        let lifecycle_v2_path = required_shadow_v2_burnin_path_component(
            config.lifecycle_v2_path.as_deref(),
            "lifecycle_v2_path",
        )?;
        let path_density_v2_path = required_shadow_v2_burnin_path_component(
            config.path_density_v2_path.as_deref(),
            "path_density_v2_path",
        )?;
        let mut harness_config = Self::new(
            run_id,
            canonical_event_stream_path,
            replay_v2_path,
            lifecycle_v2_path,
            path_density_v2_path,
        );
        harness_config.source_ref_manifest_v2_path = config
            .source_ref_manifest_v2_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| harness_config.source_ref_manifest_v2_path.clone());
        harness_config.compact_density_enabled = config.compact_density_enabled;
        harness_config.density_full_stream_enabled =
            config.density_full_stream_enabled || shadow_v2_density_full_stream_enabled_from_env();
        if harness_config.density_full_stream_enabled {
            harness_config.compact_density_enabled = false;
            harness_config.density_horizons_ms = shadow_v2_full_density_horizons_ms();
        } else {
            harness_config.density_horizons_ms = shadow_v2_l2_declared_density_horizons_ms();
        }
        harness_config.replay_lifecycle_compact_refs_enabled =
            config.replay_lifecycle_compact_refs_enabled;
        harness_config.artifact_budget = ShadowV2ArtifactBudgetConfig {
            enabled: config.artifact_budget_enabled,
            rotation_enabled: config.artifact_rotation_enabled,
            max_total_artifact_bytes: config.max_total_artifact_bytes,
            max_file_bytes: config.max_file_bytes,
            max_rows_per_file: config.max_rows_per_file,
            max_density_rows: config.max_density_rows,
            max_stdout_bytes: config.max_stdout_bytes,
            max_system_log_bytes: config.max_system_log_bytes,
        };
        Ok(Some(harness_config))
    }
}

pub fn shadow_v2_l2_declared_density_horizons_ms() -> Vec<u64> {
    SHADOW_V2_L2_DECLARED_DENSITY_HORIZONS_MS.to_vec()
}

pub fn shadow_v2_full_density_horizons_ms() -> Vec<u64> {
    SHADOW_V2_L2_DECLARED_DENSITY_HORIZONS_MS
        .iter()
        .chain(SHADOW_V2_L2_UNDECLARED_LONG_HORIZONS_MS.iter())
        .copied()
        .collect()
}

fn shadow_v2_density_full_stream_enabled_from_env() -> bool {
    matches!(
        std::env::var(SHADOW_V2_DENSITY_FULL_STREAM_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn shadow_v2_density_compact_flush_event(event_kind: ShadowPositionEventKindV2) -> bool {
    matches!(event_kind, ShadowPositionEventKindV2::TerminalTruth)
}

fn shadow_v2_artifact_budget_error(reason: impl Into<String>) -> ShadowV2Error {
    ShadowV2Error::Io(format!(
        "{SHADOW_V2_ARTIFACT_BUDGET_BLOCKER}: {}",
        reason.into()
    ))
}

fn required_shadow_v2_burnin_path_component(
    value: Option<&str>,
    field_name: &'static str,
) -> Result<String, ShadowV2Error> {
    let trimmed = value.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return Err(ShadowV2Error::HarnessConfig {
            reason: format!("missing {field_name}"),
        });
    }
    Ok(trimmed.to_string())
}

#[derive(Debug)]
pub struct ShadowV2ValidationHarness {
    config: ShadowV2ValidationHarnessConfig,
    canonical_writer: JsonlShadowV2CanonicalWriter,
    canonical_rows_written: u64,
    canonical_active_rows_written: u64,
    replay_rows_written: u64,
    replay_active_rows_written: u64,
    lifecycle_rows_written: u64,
    lifecycle_active_rows_written: u64,
    density_rows_written: u64,
    density_active_rows_written: u64,
    source_ref_manifest_rows_written: u64,
    source_ref_manifest_active_rows_written: u64,
    artifact_rotation_manifest_rows_written: u64,
    rotated_artifact_bytes: u64,
    source_ref_manifest_keys: HashSet<String>,
}

impl ShadowV2ValidationHarness {
    pub fn new(config: ShadowV2ValidationHarnessConfig) -> Result<Self, ShadowV2Error> {
        let canonical_writer =
            JsonlShadowV2CanonicalWriter::new(config.canonical_event_stream_path.clone())?;
        Ok(Self {
            config,
            canonical_writer,
            canonical_rows_written: 0,
            canonical_active_rows_written: 0,
            replay_rows_written: 0,
            replay_active_rows_written: 0,
            lifecycle_rows_written: 0,
            lifecycle_active_rows_written: 0,
            density_rows_written: 0,
            density_active_rows_written: 0,
            source_ref_manifest_rows_written: 0,
            source_ref_manifest_active_rows_written: 0,
            artifact_rotation_manifest_rows_written: 0,
            rotated_artifact_bytes: 0,
            source_ref_manifest_keys: HashSet::new(),
        })
    }

    pub fn append_record(&mut self, record: ShadowV2Record) -> ShadowV2HarnessAppendOutcome {
        if matches!(
            record,
            ShadowV2Record::ShadowReplayV2(_) | ShadowV2Record::ShadowLifecycleV2(_)
        ) {
            return ShadowV2HarnessAppendOutcome::canonical_failed(
                "HARNESS_REJECTS_DERIVED_RECORD_AS_CANONICAL_INPUT",
            );
        }

        let position_id = record.envelope().position_id.clone();
        let event = match self.canonical_writer.stream.prepare_record(record) {
            Ok(event) => event,
            Err(error) => return ShadowV2HarnessAppendOutcome::canonical_failed(error),
        };
        let canonical_path = self.config.canonical_event_stream_path.clone();
        let active_rows = match self.rotate_artifact_if_needed(
            &canonical_path,
            "shadow_position_event_v2",
            self.canonical_active_rows_written,
        ) {
            Ok(active_rows) => active_rows,
            Err(error) => return ShadowV2HarnessAppendOutcome::canonical_failed(error),
        };
        if let Err(error) = self.ensure_artifact_budget_before_write(
            &canonical_path,
            "shadow_position_event_v2",
            active_rows.saturating_add(1),
        ) {
            return ShadowV2HarnessAppendOutcome::canonical_failed(error);
        }
        if let Err(error) = append_jsonl_record(&canonical_path, &event) {
            return ShadowV2HarnessAppendOutcome::canonical_failed(error);
        }
        if let Err(error) = self.canonical_writer.stream.commit_prepared_event(event) {
            return ShadowV2HarnessAppendOutcome::canonical_failed(error);
        }
        self.canonical_rows_written = self.canonical_rows_written.saturating_add(1);
        self.canonical_active_rows_written = active_rows.saturating_add(1);

        let replay_write = self.write_replay_snapshot(&position_id);
        let lifecycle_write = self.write_lifecycle_snapshot(&position_id);
        let density_write = self.write_path_density_snapshots(&position_id);
        ShadowV2HarnessAppendOutcome::from_writes(replay_write, lifecycle_write, density_write)
    }

    pub fn canonical_stream(&self) -> &ShadowV2CanonicalEventStream {
        self.canonical_writer.stream()
    }

    pub fn canonical_event_stream_path(&self) -> &Path {
        self.canonical_writer.path()
    }

    fn write_replay_snapshot(&mut self, position_id: &str) -> ShadowV2WriteStatus {
        let Some(high_watermark) = self.high_watermark_for_position(position_id).cloned() else {
            return ShadowV2WriteStatus::Skipped("NO_CANONICAL_HIGH_WATERMARK".to_string());
        };
        let envelope = derived_snapshot_envelope("shadow_replay_v2", "replay_v2", &high_watermark);
        match ShadowReplayV2::derive_from_canonical_stream(
            envelope,
            self.canonical_writer.stream(),
            position_id,
            self.config.canonical_event_stream_ref.clone(),
        ) {
            Ok(mut replay) => {
                if self.config.replay_lifecycle_compact_refs_enabled {
                    match self.write_source_ref_manifest_snapshot(
                        &high_watermark,
                        &replay.source_event_ids,
                        &replay.path_sample_event_ids,
                    ) {
                        Ok(manifest_ref) => replay.compact_source_refs(manifest_ref),
                        Err(error) => return ShadowV2WriteStatus::Err(error.to_string()),
                    }
                }
                let replay_path = self.config.replay_v2_path.clone();
                let active_rows = match self.rotate_artifact_if_needed(
                    &replay_path,
                    "shadow_replay_v2",
                    self.replay_active_rows_written,
                ) {
                    Ok(active_rows) => active_rows,
                    Err(error) => return ShadowV2WriteStatus::Err(error.to_string()),
                };
                if let Err(error) = self.ensure_artifact_budget_before_write(
                    &replay_path,
                    "shadow_replay_v2",
                    active_rows.saturating_add(1),
                ) {
                    return ShadowV2WriteStatus::Err(error.to_string());
                }
                match append_jsonl_record(&replay_path, &replay) {
                    Ok(()) => {
                        self.replay_rows_written = self.replay_rows_written.saturating_add(1);
                        self.replay_active_rows_written = active_rows.saturating_add(1);
                        ShadowV2WriteStatus::Ok
                    }
                    Err(error) => ShadowV2WriteStatus::Err(error.to_string()),
                }
            }
            Err(error) => ShadowV2WriteStatus::Err(error.to_string()),
        }
    }

    fn write_lifecycle_snapshot(&mut self, position_id: &str) -> ShadowV2WriteStatus {
        let Some(high_watermark) = self.high_watermark_for_position(position_id).cloned() else {
            return ShadowV2WriteStatus::Skipped("NO_CANONICAL_HIGH_WATERMARK".to_string());
        };
        let envelope =
            derived_snapshot_envelope("shadow_lifecycle_v2", "lifecycle_v2", &high_watermark);
        match ShadowLifecycleV2::derive_from_canonical_stream(
            envelope,
            self.canonical_writer.stream(),
            position_id,
            self.config.canonical_event_stream_ref.clone(),
        ) {
            Ok(mut lifecycle) => {
                if self.config.replay_lifecycle_compact_refs_enabled {
                    let (source_event_ids, path_sample_event_ids) =
                        self.canonical_ref_ids_for_position(position_id);
                    match self.write_source_ref_manifest_snapshot(
                        &high_watermark,
                        &source_event_ids,
                        &path_sample_event_ids,
                    ) {
                        Ok(manifest_ref) => lifecycle.compact_source_refs(manifest_ref),
                        Err(error) => return ShadowV2WriteStatus::Err(error.to_string()),
                    }
                }
                let lifecycle_path = self.config.lifecycle_v2_path.clone();
                let active_rows = match self.rotate_artifact_if_needed(
                    &lifecycle_path,
                    "shadow_lifecycle_v2",
                    self.lifecycle_active_rows_written,
                ) {
                    Ok(active_rows) => active_rows,
                    Err(error) => return ShadowV2WriteStatus::Err(error.to_string()),
                };
                if let Err(error) = self.ensure_artifact_budget_before_write(
                    &lifecycle_path,
                    "shadow_lifecycle_v2",
                    active_rows.saturating_add(1),
                ) {
                    return ShadowV2WriteStatus::Err(error.to_string());
                }
                match append_jsonl_record(&lifecycle_path, &lifecycle) {
                    Ok(()) => {
                        self.lifecycle_rows_written = self.lifecycle_rows_written.saturating_add(1);
                        self.lifecycle_active_rows_written = active_rows.saturating_add(1);
                        ShadowV2WriteStatus::Ok
                    }
                    Err(error) => ShadowV2WriteStatus::Err(error.to_string()),
                }
            }
            Err(error) => ShadowV2WriteStatus::Err(error.to_string()),
        }
    }

    fn write_path_density_snapshots(&mut self, position_id: &str) -> ShadowV2WriteStatus {
        let Some(high_watermark) = self.high_watermark_for_position(position_id).cloned() else {
            return ShadowV2WriteStatus::Skipped("NO_CANONICAL_HIGH_WATERMARK".to_string());
        };
        if self.config.compact_density_enabled
            && !shadow_v2_density_compact_flush_event(high_watermark.event_kind)
        {
            return ShadowV2WriteStatus::Skipped(
                "DENSITY_COMPACT_WAITING_FOR_FINAL_SNAPSHOT".to_string(),
            );
        }
        let path_samples = self.path_samples_for_position(position_id);
        let selected_samples =
            select_path_samples_v2(&path_samples, &self.config.path_sampler_config);
        let source_path_sample_event_ids = selected_samples
            .iter()
            .map(|sample| sample.envelope.event_id.clone())
            .collect::<Vec<_>>();
        let truncated = selected_samples.iter().any(|sample| sample.truncated);
        let evaluations = evaluate_path_density_v2(
            &selected_samples,
            &self.config.path_sampler_config,
            &self.config.density_horizons_ms,
        );
        let created_at_wall_ms = shadow_v2_now_ms();
        for evaluation in evaluations {
            let row = ShadowPathDensityV2::from_evaluation(
                &high_watermark,
                self.config.canonical_event_stream_ref.clone(),
                source_path_sample_event_ids.clone(),
                truncated,
                evaluation,
                created_at_wall_ms,
            );
            if self.config.artifact_budget.enabled
                && self.density_rows_written >= self.config.artifact_budget.max_density_rows
            {
                return ShadowV2WriteStatus::Err(format!(
                    "{SHADOW_V2_ARTIFACT_BUDGET_BLOCKER}: shadow_path_density_v2 max_density_rows={} reached",
                    self.config.artifact_budget.max_density_rows
                ));
            }
            let density_path = self.config.path_density_v2_path.clone();
            let active_rows = match self.rotate_artifact_if_needed(
                &density_path,
                "shadow_path_density_v2",
                self.density_active_rows_written,
            ) {
                Ok(active_rows) => active_rows,
                Err(error) => return ShadowV2WriteStatus::Err(error.to_string()),
            };
            if let Err(error) = self.ensure_artifact_budget_before_write(
                &density_path,
                "shadow_path_density_v2",
                active_rows.saturating_add(1),
            ) {
                return ShadowV2WriteStatus::Err(error.to_string());
            }
            if let Err(error) = append_jsonl_record(&density_path, &row) {
                return ShadowV2WriteStatus::Err(error.to_string());
            }
            self.density_rows_written = self.density_rows_written.saturating_add(1);
            self.density_active_rows_written = active_rows.saturating_add(1);
        }
        ShadowV2WriteStatus::Ok
    }

    fn write_source_ref_manifest_snapshot(
        &mut self,
        high_watermark: &ShadowPositionEventV2,
        source_event_ids: &[String],
        path_sample_event_ids: &[String],
    ) -> Result<String, ShadowV2Error> {
        let row = ShadowV2SourceRefManifestRow::new(
            high_watermark,
            self.config.canonical_event_stream_ref.clone(),
            source_event_ids,
            path_sample_event_ids,
        );
        let manifest_ref = row.manifest_ref();
        if self.source_ref_manifest_keys.insert(manifest_ref.clone()) {
            let manifest_path = self.config.source_ref_manifest_v2_path.clone();
            let active_rows = self.rotate_artifact_if_needed(
                &manifest_path,
                SHADOW_V2_SOURCE_REF_MANIFEST_SCHEMA,
                self.source_ref_manifest_active_rows_written,
            )?;
            self.ensure_artifact_budget_before_write(
                &manifest_path,
                SHADOW_V2_SOURCE_REF_MANIFEST_SCHEMA,
                active_rows.saturating_add(1),
            )?;
            append_jsonl_record(&manifest_path, &row)?;
            self.source_ref_manifest_rows_written =
                self.source_ref_manifest_rows_written.saturating_add(1);
            self.source_ref_manifest_active_rows_written = active_rows.saturating_add(1);
        }
        Ok(manifest_ref)
    }

    fn rotate_artifact_if_needed(
        &mut self,
        path: &Path,
        artifact: &str,
        active_rows: u64,
    ) -> Result<u64, ShadowV2Error> {
        if !self.config.artifact_budget.enabled || !self.config.artifact_budget.rotation_enabled {
            return Ok(active_rows);
        }
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(active_rows),
            Err(error) => return Err(error.into()),
        };
        if metadata.len() < self.config.artifact_budget.max_file_bytes
            && active_rows < self.config.artifact_budget.max_rows_per_file
        {
            return Ok(active_rows);
        }
        let (rotated_path, rotation_index) = next_shadow_v2_rotated_jsonl_part_path(path);
        if let Some(parent) = rotated_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(path, &rotated_path)?;
        let hash_uncompressed = blake3_file_hex(&rotated_path)?;
        let row = ShadowV2ArtifactRotationManifestRow::new(
            self.config.run_id.clone(),
            artifact,
            path,
            &rotated_path,
            metadata.len(),
            active_rows,
            rotation_index,
            hash_uncompressed,
        );
        append_jsonl_record(&self.config.artifact_rotation_manifest_v2_path, &row)?;
        self.artifact_rotation_manifest_rows_written = self
            .artifact_rotation_manifest_rows_written
            .saturating_add(1);
        self.rotated_artifact_bytes = self.rotated_artifact_bytes.saturating_add(metadata.len());
        Ok(0)
    }

    fn ensure_artifact_budget_before_write(
        &self,
        path: &Path,
        artifact: &str,
        next_rows_for_file: u64,
    ) -> Result<(), ShadowV2Error> {
        if !self.config.artifact_budget.enabled {
            return Ok(());
        }
        let budget = &self.config.artifact_budget;
        if next_rows_for_file > budget.max_rows_per_file {
            if budget.rotation_enabled {
                return Err(shadow_v2_artifact_budget_error(format!(
                    "{artifact} rows would exceed max_rows_per_file={} after rotation path={}",
                    budget.max_rows_per_file,
                    path.display()
                )));
            } else {
                return Err(shadow_v2_artifact_budget_error(format!(
                    "{artifact} rows would exceed max_rows_per_file={} path={}",
                    budget.max_rows_per_file,
                    path.display()
                )));
            }
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() >= budget.max_file_bytes {
                return Err(shadow_v2_artifact_budget_error(format!(
                    "{artifact} size_bytes={} exceeds max_file_bytes={} path={}",
                    metadata.len(),
                    budget.max_file_bytes,
                    path.display()
                )));
            }
        }
        let total_bytes = self.configured_artifact_size_bytes();
        if total_bytes >= budget.max_total_artifact_bytes {
            return Err(shadow_v2_artifact_budget_error(format!(
                "configured artifact bytes {} exceed max_total_artifact_bytes={}",
                total_bytes, budget.max_total_artifact_bytes
            )));
        }
        Ok(())
    }

    fn configured_artifact_size_bytes(&self) -> u64 {
        [
            self.config.canonical_event_stream_path.as_path(),
            self.config.replay_v2_path.as_path(),
            self.config.lifecycle_v2_path.as_path(),
            self.config.path_density_v2_path.as_path(),
            self.config.source_ref_manifest_v2_path.as_path(),
            self.config.artifact_rotation_manifest_v2_path.as_path(),
        ]
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum::<u64>()
        .saturating_add(self.rotated_artifact_bytes)
    }

    fn canonical_ref_ids_for_position(&self, position_id: &str) -> (Vec<String>, Vec<String>) {
        let mut source_event_ids = Vec::new();
        let mut path_sample_event_ids = Vec::new();
        for event in self
            .canonical_writer
            .stream()
            .events_for_position(position_id)
        {
            if matches!(
                event.event_kind,
                ShadowPositionEventKindV2::ReplayDerived
                    | ShadowPositionEventKindV2::LifecycleSubEvent
            ) {
                continue;
            }
            source_event_ids.push(event.envelope.event_id.clone());
            if event.event_kind == ShadowPositionEventKindV2::PathSample {
                path_sample_event_ids.push(event.envelope.event_id.clone());
            }
        }
        (source_event_ids, path_sample_event_ids)
    }

    fn high_watermark_for_position(&self, position_id: &str) -> Option<&ShadowPositionEventV2> {
        self.canonical_writer
            .stream()
            .events()
            .iter()
            .rev()
            .find(|event| event.envelope.position_id == position_id)
    }

    fn path_samples_for_position(&self, position_id: &str) -> Vec<ShadowPathSampleV2> {
        self.canonical_writer
            .stream()
            .events_for_position(position_id)
            .into_iter()
            .filter(|event| event.event_kind == ShadowPositionEventKindV2::PathSample)
            .filter_map(|event| match shadow_v2_record_from_event(event) {
                Ok(ShadowV2Record::ShadowPathSampleV2(sample)) => Some(sample),
                _ => None,
            })
            .collect()
    }
}

fn derived_snapshot_envelope(
    schema: &str,
    id_prefix: &str,
    high_watermark_event: &ShadowPositionEventV2,
) -> ShadowV2Envelope {
    let mut envelope = high_watermark_event.envelope.clone();
    envelope.schema = schema.to_string();
    envelope.parent_event_id = Some(high_watermark_event.envelope.event_id.clone());
    envelope.source_event_id = Some(high_watermark_event.envelope.event_id.clone());
    envelope.event_id = format!(
        "{id_prefix}:{}:{}",
        high_watermark_event.envelope.position_id, high_watermark_event.envelope.event_id
    );
    envelope.produced_at_ms = shadow_v2_now_ms();
    envelope.produced_at_slot = high_watermark_event.envelope.produced_at_slot;
    envelope.source_refs.push(format!(
        "canonical_high_watermark:{}",
        high_watermark_event.envelope.event_id
    ));
    envelope
        .limitations
        .push("DERIVED_SNAPSHOT_KEYED_BY_CANONICAL_HIGH_WATERMARK".to_string());
    envelope
}

fn shadow_v2_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn shadow_v2_record_from_event(
    event: &ShadowPositionEventV2,
) -> Result<ShadowV2Record, ShadowV2Error> {
    serde_json::from_value(event.payload.clone()).map_err(ShadowV2Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::account_state_core::types::StatePhase;
    use ghost_core::{
        quote_constant_product, CurveFinality, ShadowV2QuoteSide, SHADOW_V2_PRICE_FORMULA_VERSION,
    };
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

    fn solana_transaction_source_event_order_key() -> EventOrderKey {
        EventOrderKey {
            slot: EventOrderComponent::known(42),
            block_time: EventOrderComponent::known(1_785_000_000),
            signature: EventOrderComponent::known("source-tx-sig".to_string()),
            transaction_index_or_unknown: EventOrderComponent::known(2),
            instruction_index_or_unknown: EventOrderComponent::known(1),
            inner_instruction_index_or_unknown: EventOrderComponent::unknown(),
            log_index_or_unknown: EventOrderComponent::not_applicable(),
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
        let mut terminal_order = event_order_key(Some(45), Some(3));
        terminal_order.event_seq_in_process = 9;
        ShadowTerminalTruthV2 {
            envelope,
            event_order_key: terminal_order,
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

    fn harness_config_for_dir(path: &Path) -> ShadowV2ValidationHarnessConfig {
        ShadowV2ValidationHarnessConfig::new(
            "run-a",
            path.join("shadow_position_event_v2.jsonl"),
            path.join("shadow_replay_v2.jsonl"),
            path.join("shadow_lifecycle_v2.jsonl"),
            path.join("shadow_path_density_v2.jsonl"),
        )
    }

    fn closed_canonical_stream_for_pr8_pr9() -> ShadowV2CanonicalEventStream {
        let mut stream = ShadowV2CanonicalEventStream::default();
        stream
            .append_record(ShadowV2Record::ShadowPositionV2(position_record(
                "pos-a",
                "event-position",
            )))
            .unwrap();

        let pool_entry = account_state_pool_sample("pool-event-entry", 1);
        stream
            .append_record(ShadowV2Record::PoolStateSampleV2(pool_entry.clone()))
            .unwrap();

        let mut entry_fill_order = event_order_key(Some(43), Some(2));
        entry_fill_order.event_seq_in_process = 2;
        let entry_config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            250,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );
        let entry_fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-a"),
            entry_fill_order,
            &pool_entry,
            &entry_config,
        );
        assert_eq!(entry_fill.fill_status, FillStatus::Filled);
        stream
            .append_record(ShadowV2Record::ShadowEntryFillV2(entry_fill))
            .unwrap();

        let pool_path = post_entry_pool_sample("pool-event-path", 3, 44, 1);
        stream
            .append_record(ShadowV2Record::PoolStateSampleV2(pool_path.clone()))
            .unwrap();

        let mut path_order_a = event_order_key(Some(45), Some(1));
        path_order_a.event_seq_in_process = 4;
        let path_a = path_sample_with_pnl(
            "path-sample-a",
            1_000,
            200,
            ShadowPathSamplingModeV2::Dense3s,
            ShadowPathSamplingReasonV2::EventSample,
            path_order_a,
        );
        stream
            .append_record(ShadowV2Record::ShadowPathSampleV2(path_a))
            .unwrap();

        let reserves =
            reserves_from_pool_state(&pool_path, ShadowV2PoolPhase::BondingCurve).unwrap();
        let exit_quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Sell,
            reserves,
            10_000_000_000,
            100,
            100,
        )
        .unwrap();
        let mut path_order_b = event_order_key(Some(45), Some(2));
        path_order_b.event_seq_in_process = 5;
        let mut path_b = path_sample_with_pnl(
            "path-sample-b",
            2_000,
            650,
            ShadowPathSamplingModeV2::Dense3s,
            ShadowPathSamplingReasonV2::EventSample,
            path_order_b,
        );
        path_b.attach_static_exit_quote(&exit_quote, pool_path.price_sol_per_token);
        stream
            .append_record(ShadowV2Record::ShadowPathSampleV2(path_b))
            .unwrap();

        let mut exit_attempt_order = event_order_key(Some(46), Some(1));
        exit_attempt_order.event_seq_in_process = 6;
        let mut exit_attempt = ShadowExitAttemptV2::from_mark_path_trigger(
            test_envelope("shadow_exit_attempt_v2", "pos-a", "exit-attempt-a"),
            exit_attempt_order,
            "TARGET",
            clocked(
                "trigger_ts_ms",
                1_785_000_002_123,
                ClockDomain::StreamObservedMs,
            ),
            Some(46),
            "shadow_exit_path_replay_v2",
            Some(600),
            Some(-600),
            Some(3_000),
            false,
            Some("BLOCK_AMBIGUOUS".to_string()),
        );
        exit_attempt.attach_static_exit_model(SHADOW_V2_EXIT_FILL_MODEL_VERSION);
        stream
            .append_record(ShadowV2Record::ShadowExitAttemptV2(exit_attempt))
            .unwrap();

        let pool_exit = post_entry_pool_sample("pool-event-exit", 7, 46, 2);
        stream
            .append_record(ShadowV2Record::PoolStateSampleV2(pool_exit.clone()))
            .unwrap();

        let mut exit_fill_order = event_order_key(Some(47), Some(0));
        exit_fill_order.event_seq_in_process = 8;
        let exit_config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            150,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        );
        let exit_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-a"),
            exit_fill_order,
            &pool_exit,
            &exit_config,
        );
        assert_eq!(exit_fill.fill_status, FillStatus::Filled);
        stream
            .append_record(ShadowV2Record::ShadowExitFillV2(exit_fill))
            .unwrap();

        let mut terminal = terminal_record("pos-a", "event-terminal-a");
        terminal.terminal_reason = TerminalReasonV2::Target;
        terminal.final_pnl_mark_bps = Some(650);
        terminal.final_pnl_executable_bps = Some(580);
        terminal.close_age_ms = Some(2_000);
        terminal.linked_entry_fill = Some("entry-fill-a".to_string());
        terminal.linked_exit_fill = Some("exit-fill-a".to_string());
        stream
            .append_record(ShadowV2Record::ShadowTerminalTruthV2(terminal))
            .unwrap();

        stream
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
            source_write_version: Some(7),
            source_account_pubkey: Some(Pubkey::new_unique()),
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            account_data_len: Some(b"account-data".len() as u64),
            account_data_hash: Some(account_data_hash_blake3(b"account-data")),
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

    fn assert_entry_fill_diagnostic_for_pool_research_blocker(
        pool_state: PoolStateSampleV2,
        event_id: &str,
        expected_blocker: &str,
    ) {
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = pool_state.event_order_key.event_seq_in_process + 1;
        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", event_id),
            fill_order,
            &pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(false));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::DiagnosticSim)
        );
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::DiagnosticOnly
        );
        assert!(fill.fill_price.is_some());
        assert!(
            fill.provenance_blockers
                .contains(&expected_blocker.to_string()),
            "expected provenance blocker {expected_blocker}, got {:?}",
            fill.provenance_blockers
        );
    }

    fn post_entry_pool_sample(
        event_id: &str,
        seq: u64,
        slot: u64,
        tx_index: u32,
    ) -> PoolStateSampleV2 {
        let mut sample = account_state_pool_sample(event_id, seq);
        sample.envelope.temporal_class = TemporalClass::PostEntry;
        sample.envelope.clock_domain = ClockDomain::StreamObservedMs;
        sample.event_order_key.slot = EventOrderComponent::known(slot);
        sample.event_order_key.transaction_index_or_unknown = EventOrderComponent::known(tx_index);
        sample.event_order_key.event_seq_in_process = seq;
        sample.observed_slot = Some(slot.saturating_sub(1));
        sample.staleness_slots = Some(1);
        sample
    }

    fn path_sample_with_pnl(
        event_id: &str,
        age_ms: u64,
        pnl_mark_bps: i32,
        sampling_mode: ShadowPathSamplingModeV2,
        sampling_reason: ShadowPathSamplingReasonV2,
        mut event_order_key: EventOrderKey,
    ) -> ShadowPathSampleV2 {
        event_order_key.observed_at_wall_ms = 1_785_000_000_123_u64.saturating_add(age_ms);
        ShadowPathSampleV2 {
            envelope: test_envelope("shadow_path_sample_v2", "pos-a", event_id),
            event_order_key,
            sampling_mode,
            path_horizon_ms: ShadowPathSamplerConfigV2::for_mode(sampling_mode).max_horizon_ms,
            sample_ts_ms: clocked(
                "sample_ts_ms",
                1_785_000_000_123_i64.saturating_add(age_ms as i64),
                ClockDomain::StreamObservedMs,
            ),
            sample_slot: Some(45),
            age_ms,
            pool_state_ref: "pool-event-path".to_string(),
            mark_price: Some(0.00003),
            executable_exit_quote: None,
            pnl_mark_bps: Some(pnl_mark_bps),
            pnl_executable_bps: None,
            mfe_mark_bps: Some(pnl_mark_bps),
            mae_mark_bps: Some(pnl_mark_bps),
            source_quality: "RESEARCH_READY".to_string(),
            sampling_reason: sampling_reason.label().to_string(),
            exact_or_approx: "EXACT_EVENT_ORDER".to_string(),
            truncated: false,
        }
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
    fn shadow_v2_solana_transaction_source_proof_is_not_complete_chain_order() {
        let order = solana_transaction_source_event_order_key();

        assert!(order.has_complete_solana_transaction_source_proof());
        assert!(order.solana_transaction_source_proof_blockers().is_empty());
        assert!(!order.has_complete_chain_order());
        assert_eq!(
            order.explicit_unknown_chain_order_components(),
            vec!["inner_instruction_index_or_unknown"]
        );
        assert_eq!(
            order.not_applicable_or_derived_chain_order_components(),
            vec!["log_index_or_unknown:NOT_APPLICABLE".to_string()]
        );
    }

    #[test]
    fn shadow_v2_event_seq_does_not_complete_solana_transaction_source_proof() {
        let mut order = explicit_unknown_event_order_key();
        order.event_seq_in_process = 99;

        assert!(order.is_after_process_seq(98));
        assert!(!order.has_complete_solana_transaction_source_proof());
        assert!(!order.has_complete_chain_order());
        let blockers = order.solana_transaction_source_proof_blockers();
        for expected in [
            "TRANSACTION_SOURCE_SIGNATURE_UNKNOWN",
            "TRANSACTION_SOURCE_TRANSACTION_INDEX_UNKNOWN",
            "TRANSACTION_SOURCE_INSTRUCTION_INDEX_UNKNOWN",
        ] {
            assert!(
                blockers.contains(&expected.to_string()),
                "expected blocker {expected}, got {blockers:?}"
            );
        }
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
    fn shadow_v2_event_order_seq_alone_is_not_l2_chain_order() {
        let mut lhs = event_order_key(Some(42), None);
        let mut rhs = event_order_key(Some(42), None);
        lhs.event_seq_in_process = 10;
        rhs.event_seq_in_process = 11;

        assert!(lhs.is_after_process_seq(9));
        assert!(rhs.is_after_process_seq(lhs.event_seq_in_process));
        assert!(!lhs.has_complete_chain_order());
        assert!(!rhs.has_complete_chain_order());
        assert!(lhs.same_slot_ambiguous_with(&rhs));
        assert!(lhs
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_UNKNOWN_BUT_REQUIRED_FOR_RESEARCH".to_string()));
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
    fn shadow_v2_replay_derive_from_canonical_stream_separates_lanes() {
        let stream = closed_canonical_stream_for_pr8_pr9();
        let replay = ShadowReplayV2::derive_from_canonical_stream(
            test_envelope("shadow_replay_v2", "pos-a", "replay-derived-a"),
            &stream,
            "pos-a",
            "shadow_position_event_v2.jsonl",
        )
        .unwrap();

        assert!(replay.derived_from_canonical_stream);
        assert_eq!(replay.envelope.schema, "shadow_replay_v2");
        assert_eq!(replay.envelope.simulation_level, SimulationLevel::MarkOnly);
        assert_eq!(
            replay.envelope.measurement_grade,
            MeasurementGrade::MarkPriceReplay
        );
        assert_eq!(
            replay.replay_derivation_status,
            "REPLAY_DERIVED_FROM_CANONICAL_TERMINAL"
        );
        assert_eq!(
            replay.canonical_terminal_event_id.as_deref(),
            Some("event-terminal-a")
        );
        assert_eq!(
            replay.terminal_truth_event_id.as_deref(),
            Some("event-terminal-a")
        );
        assert_eq!(replay.entry_fill_event_id.as_deref(), Some("entry-fill-a"));
        assert_eq!(
            replay.exit_attempt_event_id.as_deref(),
            Some("exit-attempt-a")
        );
        assert_eq!(replay.exit_fill_event_id.as_deref(), Some("exit-fill-a"));
        assert_eq!(replay.path_sample_event_ids.len(), 2);
        assert_eq!(replay.mark_path_sample_count, 2);
        assert_eq!(replay.executable_quote_sample_count, 1);
        assert_eq!(replay.blocked_path_sample_count, 0);
        assert_eq!(replay.terminal_reason, Some(TerminalReasonV2::Target));
        assert_eq!(replay.terminal_pnl_mark_bps, Some(650));
        assert_eq!(replay.terminal_pnl_executable_bps, Some(580));
        assert_eq!(replay.close_age_ms, Some(2_000));
        assert!(replay.mark_replay_ref.is_some());
        assert!(replay.executable_replay_ref.is_some());
        assert!(replay
            .envelope
            .limitations
            .contains(&"REPLAY_V2_DERIVED_VIEW_NOT_CANONICAL_TRUTH".to_string()));
        assert!(replay
            .envelope
            .limitations
            .contains(&"MARK_REPLAY_NOT_EXECUTABLE_FILL".to_string()));
    }

    #[test]
    fn shadow_v2_lifecycle_derive_from_same_canonical_terminal_truth() {
        let stream = closed_canonical_stream_for_pr8_pr9();
        let lifecycle = ShadowLifecycleV2::derive_from_canonical_stream(
            test_envelope("shadow_lifecycle_v2", "pos-a", "lifecycle-derived-a"),
            &stream,
            "pos-a",
            "shadow_position_event_v2.jsonl",
        )
        .unwrap();

        assert!(lifecycle.derived_from_canonical_stream);
        assert!(lifecycle.derived_view_not_canonical_terminal);
        assert_eq!(lifecycle.envelope.schema, "shadow_lifecycle_v2");
        assert_eq!(
            lifecycle.lifecycle_event_type,
            ShadowLifecycleEventTypeV2::PositionClosed
        );
        assert_eq!(
            lifecycle.canonical_position_event_id.as_deref(),
            Some("event-position")
        );
        assert_eq!(
            lifecycle.canonical_terminal_event_id.as_deref(),
            Some("event-terminal-a")
        );
        assert_eq!(
            lifecycle.entry_fill_event_id.as_deref(),
            Some("entry-fill-a")
        );
        assert_eq!(
            lifecycle.exit_attempt_event_id.as_deref(),
            Some("exit-attempt-a")
        );
        assert_eq!(lifecycle.exit_fill_event_id.as_deref(), Some("exit-fill-a"));
        assert_eq!(lifecycle.terminal_reason, Some(TerminalReasonV2::Target));
        assert_eq!(lifecycle.final_pnl_mark_bps, Some(650));
        assert_eq!(lifecycle.final_pnl_executable_bps, Some(580));
        assert_eq!(lifecycle.close_age_ms, Some(2_000));
        assert_eq!(
            lifecycle.duplicate_terminal_handling,
            "DERIVED_LIFECYCLE_VIEW_DOES_NOT_CREATE_CANONICAL_TERMINAL_TRUTH"
        );
        assert_eq!(
            lifecycle.reconciliation_status,
            "LIFECYCLE_DERIVED_FROM_CANONICAL_TERMINAL"
        );
        assert!(lifecycle
            .envelope
            .limitations
            .contains(&"LIFECYCLE_V2_DERIVED_VIEW_NOT_CANONICAL_TERMINAL_TRUTH".to_string()));
    }

    #[test]
    fn shadow_v2_lifecycle_sub_event_does_not_create_duplicate_terminal_truth() {
        let mut stream = closed_canonical_stream_for_pr8_pr9();
        let lifecycle = ShadowLifecycleV2::derive_from_canonical_stream(
            test_envelope("shadow_lifecycle_v2", "pos-a", "lifecycle-derived-a"),
            &stream,
            "pos-a",
            "shadow_position_event_v2.jsonl",
        )
        .unwrap();

        let lifecycle_event = stream
            .append_record(ShadowV2Record::ShadowLifecycleV2(lifecycle))
            .unwrap();

        assert_eq!(
            lifecycle_event.event_kind,
            ShadowPositionEventKindV2::LifecycleSubEvent
        );
        assert!(!lifecycle_event.is_canonical_terminal());
        assert!(lifecycle_event.canonical_terminal_event_id.is_none());
        assert_eq!(stream.terminal_event_id("pos-a"), Some("event-terminal-a"));

        let duplicate_terminal = stream
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
    }

    #[test]
    fn shadow_v2_replay_lifecycle_reconciliation_uses_exact_join_only() {
        let stream = closed_canonical_stream_for_pr8_pr9();
        let replay = ShadowReplayV2::derive_from_canonical_stream(
            test_envelope("shadow_replay_v2", "pos-a", "replay-derived-a"),
            &stream,
            "pos-a",
            "shadow_position_event_v2.jsonl",
        )
        .unwrap();
        let mut lifecycle = ShadowLifecycleV2::derive_from_canonical_stream(
            test_envelope("shadow_lifecycle_v2", "pos-a", "lifecycle-derived-a"),
            &stream,
            "pos-a",
            "shadow_position_event_v2.jsonl",
        )
        .unwrap();

        let reconciled = reconcile_replay_lifecycle_v2(&replay, &lifecycle).unwrap();
        assert!(reconciled.exact_join);
        assert!(!reconciled.fallback_join_used);
        assert!(!reconciled.ambiguous_join);
        assert!(reconciled.canonical_terminal_event_id_match);
        assert!(reconciled.terminal_reason_match);
        assert!(reconciled.final_pnl_mark_match);
        assert!(reconciled.final_pnl_executable_match);
        assert!(reconciled.close_age_match);
        assert_eq!(
            reconciled.reconciliation_status,
            "REPLAY_LIFECYCLE_RECONCILED_FROM_CANONICAL_STREAM"
        );

        lifecycle.envelope.pool_id = "other-pool".to_string();
        let mismatch = reconcile_replay_lifecycle_v2(&replay, &lifecycle).unwrap();
        assert!(!mismatch.exact_join);
        assert!(!mismatch.fallback_join_used);
        assert_eq!(
            mismatch.reconciliation_status,
            "REPLAY_LIFECYCLE_EXACT_JOIN_KEY_MISMATCH"
        );
        assert!(mismatch
            .limitations
            .contains(&"NO_FALLBACK_JOIN_ACCEPTED".to_string()));
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
        assert_eq!(json["schema"], "shadow_position_event_v2");
        assert_eq!(json["event_kind"], "POSITION_CREATED");
        assert_eq!(json["envelope"]["position_id"], "pos-a");
        assert_eq!(json["canonical_payload_schema"], "shadow_position_v2");
        assert_eq!(
            json["ordering_exemption"],
            "ORDERING_EXEMPT_POSITION_CREATED"
        );
        assert_eq!(writer.stream().events().len(), 1);
    }

    #[test]
    fn shadow_v2_terminal_truth_has_event_order_key() {
        let terminal = terminal_record("pos-a", "event-terminal");
        let record = ShadowV2Record::ShadowTerminalTruthV2(terminal);
        assert!(record.event_order_key().is_some());

        let event = ShadowPositionEventV2::from_record(record).unwrap();
        assert_eq!(event.event_kind, ShadowPositionEventKindV2::TerminalTruth);
        assert!(event.event_kind.requires_event_ordering());
        assert!(event.event_order_key.is_some());
        assert!(event.ordering_exemption.is_none());
        assert_eq!(
            event
                .payload
                .get("record")
                .and_then(|record| record.get("event_order_key"))
                .and_then(|value| value.get("event_seq_in_process"))
                .and_then(Value::as_u64),
            Some(9)
        );
    }

    #[test]
    fn shadow_v2_position_created_ordering_exemption_is_explicit() {
        let event = ShadowPositionEventV2::from_record(ShadowV2Record::ShadowPositionV2(
            position_record("pos-a", "event-position"),
        ))
        .unwrap();

        assert_eq!(event.event_kind, ShadowPositionEventKindV2::PositionCreated);
        assert!(event.event_order_key.is_none());
        assert_eq!(
            event.ordering_exemption.as_deref(),
            Some("ORDERING_EXEMPT_POSITION_CREATED")
        );
        assert!(event.has_explicit_ordering_exemption());
        assert!(!event.event_kind.requires_event_ordering());

        let mut smoke_marker = position_record("pos-smoke", "event-position-smoke");
        smoke_marker.envelope.candidate_id = Some("VALIDATION_SMOKE_MARKER".to_string());
        smoke_marker
            .envelope
            .limitations
            .push("VALIDATION_SMOKE_MARKER_V2".to_string());
        let smoke_event =
            ShadowPositionEventV2::from_record(ShadowV2Record::ShadowPositionV2(smoke_marker))
                .unwrap();
        assert_eq!(
            smoke_event.ordering_exemption.as_deref(),
            Some("ORDERING_EXEMPT_VALIDATION_SMOKE_MARKER")
        );
        assert!(smoke_event.has_explicit_ordering_exemption());
    }

    #[test]
    fn shadow_v2_temporal_audit_fails_missing_required_event_order_key() {
        let mut event = ShadowPositionEventV2::from_record(ShadowV2Record::ShadowTerminalTruthV2(
            terminal_record("pos-a", "event-terminal"),
        ))
        .unwrap();
        event.event_order_key = None;

        assert!(event.event_kind.requires_event_ordering());
        assert!(event.event_order_key.is_none());
        assert!(!event.has_explicit_ordering_exemption());

        let err = ShadowV2CanonicalEventStream::default()
            .commit_prepared_event(event)
            .unwrap_err();
        assert!(matches!(
            err,
            ShadowV2Error::MissingRequiredEventOrderKey {
                event_kind: ShadowPositionEventKindV2::TerminalTruth,
                ..
            }
        ));
    }

    #[test]
    fn shadow_v2_temporal_audit_allows_explicit_position_created_exemption() {
        let event = ShadowPositionEventV2::from_record(ShadowV2Record::ShadowPositionV2(
            position_record("pos-a", "event-position"),
        ))
        .unwrap();

        assert!(!event.event_kind.requires_event_ordering());
        assert!(event.event_order_key.is_none());
        assert!(event.has_explicit_ordering_exemption());
    }

    #[test]
    fn shadow_v2_event_seq_is_monotonic_per_position() {
        let mut stream = ShadowV2CanonicalEventStream::default();

        let mut first = account_state_pool_sample("pool-event-a", 10);
        first.envelope.position_id = "pos-a".to_string();
        stream
            .append_record(ShadowV2Record::PoolStateSampleV2(first))
            .unwrap();

        let mut second = account_state_pool_sample("pool-event-b", 5);
        second.envelope.position_id = "pos-a".to_string();
        let second_event = stream
            .append_record(ShadowV2Record::PoolStateSampleV2(second))
            .unwrap();
        assert_eq!(
            second_event
                .event_order_key
                .as_ref()
                .map(|order| order.event_seq_in_process),
            Some(11)
        );
        assert_eq!(
            second_event
                .payload
                .get("record")
                .and_then(|record| record.get("event_order_key"))
                .and_then(|value| value.get("event_seq_in_process"))
                .and_then(Value::as_u64),
            Some(11)
        );

        let mut other_position = account_state_pool_sample("pool-event-c", 1);
        other_position.envelope.position_id = "pos-b".to_string();
        let other_event = stream
            .append_record(ShadowV2Record::PoolStateSampleV2(other_position))
            .unwrap();
        assert_eq!(
            other_event
                .event_order_key
                .as_ref()
                .map(|order| order.event_seq_in_process),
            Some(1)
        );
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
    fn shadow_v2_validation_harness_writes_canonical_derived_and_density_snapshots() {
        let tmp = tempfile::tempdir().unwrap();
        let config = harness_config_for_dir(tmp.path());
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let outcome = harness.append_record(ShadowV2Record::ShadowPositionV2(position_record(
            "pos-a",
            "event-position",
        )));

        assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(outcome.replay_write, ShadowV2WriteStatus::Ok);
        assert_eq!(outcome.lifecycle_write, ShadowV2WriteStatus::Ok);
        assert_eq!(
            outcome.density_write,
            ShadowV2WriteStatus::Skipped("DENSITY_COMPACT_WAITING_FOR_FINAL_SNAPSHOT".to_string())
        );
        assert_eq!(
            outcome.validation_evidence_status,
            ShadowV2ValidationEvidenceStatus::Complete
        );
        assert_eq!(harness.canonical_stream().events().len(), 1);

        let replay_line =
            std::fs::read_to_string(tmp.path().join("shadow_replay_v2.jsonl")).unwrap();
        let replay: Value = serde_json::from_str(replay_line.lines().next().unwrap()).unwrap();
        assert_eq!(replay["envelope"]["schema"], "shadow_replay_v2");
        assert_eq!(
            replay["envelope"]["event_id"],
            "replay_v2:pos-a:event-position"
        );
        assert_eq!(replay["source_canonical_high_watermark"], "event-position");
        assert_eq!(replay["derived_from_canonical_stream"], true);

        let lifecycle_line =
            std::fs::read_to_string(tmp.path().join("shadow_lifecycle_v2.jsonl")).unwrap();
        let lifecycle: Value =
            serde_json::from_str(lifecycle_line.lines().next().unwrap()).unwrap();
        assert_eq!(lifecycle["envelope"]["schema"], "shadow_lifecycle_v2");
        assert_eq!(
            lifecycle["envelope"]["event_id"],
            "lifecycle_v2:pos-a:event-position"
        );
        assert_eq!(
            lifecycle["source_canonical_high_watermark"],
            "event-position"
        );

        assert!(!tmp.path().join("shadow_path_density_v2.jsonl").exists());
    }

    #[test]
    fn shadow_v2_l2_g_compact_density_emits_latest_declared_horizons_only_on_final() {
        let tmp = tempfile::tempdir().unwrap();
        let config = harness_config_for_dir(tmp.path());
        assert_eq!(
            config.path_sampler_config,
            ShadowPathSamplerConfigV2::standard_120s()
        );
        assert_eq!(config.path_sampler_config.max_horizon_ms, 121_000);
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        for idx in 0..=121 {
            let age_ms = idx as u64 * 1_000;
            let mut order = event_order_key(Some(45), Some(idx as u32));
            order.event_seq_in_process = 10 + idx as u64;
            let sample = path_sample_with_pnl(
                &format!("d3b-path-sample-{idx:03}"),
                age_ms,
                idx,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order,
            );
            let outcome = harness.append_record(ShadowV2Record::ShadowPathSampleV2(sample));
            assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
            assert_eq!(
                outcome.density_write,
                ShadowV2WriteStatus::Skipped(
                    "DENSITY_COMPACT_WAITING_FOR_FINAL_SNAPSHOT".to_string()
                )
            );
        }
        let outcome = harness.append_record(ShadowV2Record::ShadowTerminalTruthV2(
            terminal_record("pos-a", "event-terminal-compact"),
        ));
        assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(outcome.density_write, ShadowV2WriteStatus::Ok);

        let canonical =
            std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl")).unwrap();
        let canonical_rows: Vec<Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(canonical_rows.len(), 123);
        assert!(canonical_rows
            .iter()
            .all(|row| row["schema"] == "shadow_position_event_v2"));
        assert!(canonical_rows
            .iter()
            .take(122)
            .all(|row| row["event_kind"] == "PATH_SAMPLE"));

        let density =
            std::fs::read_to_string(tmp.path().join("shadow_path_density_v2.jsonl")).unwrap();
        let density_rows: Vec<Value> = density
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(density_rows.len(), 5);

        let final_high_watermark = "event-terminal-compact";
        let final_path_sample_id = "d3b-path-sample-121";
        let final_rows: Vec<&Value> = density_rows.iter().collect();

        let by_horizon = final_rows
            .iter()
            .map(|row| (row["horizon_ms"].as_u64().unwrap(), *row))
            .collect::<std::collections::BTreeMap<_, _>>();

        for horizon in [2_000_u64, 3_000, 10_000, 30_000, 120_000] {
            let row = by_horizon.get(&horizon).unwrap();
            assert_eq!(row["schema"], "shadow_path_density_v2");
            assert_eq!(row["source_canonical_high_watermark"], final_high_watermark);
            assert_eq!(row["verdict"], "EVALUABLE_EXACT");
            assert_eq!(row["path_points"], 122);
            assert_eq!(row["replay_horizon_ms"], 121_000);
            assert_eq!(row["max_interval_ms"], 1_000);
            let source_ids = row["source_path_sample_event_ids"].as_array().unwrap();
            assert_eq!(source_ids.len(), 122);
            assert_eq!(source_ids.first().unwrap(), "d3b-path-sample-000");
            assert_eq!(source_ids.last().unwrap(), final_path_sample_id);
        }

        for horizon in [300_000_u64, 500_000] {
            assert!(!by_horizon.contains_key(&horizon));
        }
    }

    #[test]
    fn shadow_v2_l2_g_compact_density_does_not_flush_on_exit_fill() {
        let tmp = tempfile::tempdir().unwrap();
        let config = harness_config_for_dir(tmp.path());
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let sample = path_sample_with_pnl(
            "compact-exit-path-sample",
            1_000,
            10,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::Heartbeat,
            event_order_key(Some(45), Some(1)),
        );
        let sample_outcome = harness.append_record(ShadowV2Record::ShadowPathSampleV2(sample));
        assert_eq!(sample_outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(
            sample_outcome.density_write,
            ShadowV2WriteStatus::Skipped("DENSITY_COMPACT_WAITING_FOR_FINAL_SNAPSHOT".to_string())
        );

        let pool_exit = post_entry_pool_sample("pool-event-exit-compact", 7, 46, 2);
        let exit_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-compact"),
            event_order_key(Some(47), Some(0)),
            &pool_exit,
            &ShadowExitFillModelConfig::bonding_curve(
                10_000_000_000,
                150,
                100,
                SHADOW_V2_EXIT_FILL_MODEL_VERSION,
            ),
        );
        let exit_outcome = harness.append_record(ShadowV2Record::ShadowExitFillV2(exit_fill));
        assert_eq!(exit_outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(
            exit_outcome.density_write,
            ShadowV2WriteStatus::Skipped("DENSITY_COMPACT_WAITING_FOR_FINAL_SNAPSHOT".to_string())
        );
        assert!(!tmp.path().join("shadow_path_density_v2.jsonl").exists());

        let terminal_outcome = harness.append_record(ShadowV2Record::ShadowTerminalTruthV2(
            terminal_record("pos-a", "event-terminal-compact-exit"),
        ));
        assert_eq!(terminal_outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(terminal_outcome.density_write, ShadowV2WriteStatus::Ok);

        let density =
            std::fs::read_to_string(tmp.path().join("shadow_path_density_v2.jsonl")).unwrap();
        assert_eq!(density.lines().count(), 5);
    }

    #[test]
    fn shadow_v2_l2_g_full_density_stream_requires_explicit_opt_in_config() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = harness_config_for_dir(tmp.path());
        config.compact_density_enabled = false;
        config.density_full_stream_enabled = true;
        config.density_horizons_ms = shadow_v2_full_density_horizons_ms();
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let sample = path_sample_with_pnl(
            "full-stream-path-sample",
            1_000,
            10,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::Heartbeat,
            event_order_key(Some(45), Some(1)),
        );
        let outcome = harness.append_record(ShadowV2Record::ShadowPathSampleV2(sample));
        assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(outcome.density_write, ShadowV2WriteStatus::Ok);

        let density =
            std::fs::read_to_string(tmp.path().join("shadow_path_density_v2.jsonl")).unwrap();
        let density_rows: Vec<Value> = density
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(density_rows.len(), 7);
        let horizons = density_rows
            .iter()
            .map(|row| row["horizon_ms"].as_u64().unwrap())
            .collect::<HashSet<_>>();
        assert!(horizons.contains(&300_000));
        assert!(horizons.contains(&500_000));
    }

    #[test]
    fn shadow_v2_l2_g_replay_lifecycle_compact_refs_use_manifest_hashes() {
        let tmp = tempfile::tempdir().unwrap();
        let config = harness_config_for_dir(tmp.path());
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        for record in [
            ShadowV2Record::ShadowPositionV2(position_record("pos-a", "event-position")),
            ShadowV2Record::ShadowTerminalTruthV2(terminal_record("pos-a", "event-terminal")),
        ] {
            let outcome = harness.append_record(record);
            assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        }

        let replay_lines =
            std::fs::read_to_string(tmp.path().join("shadow_replay_v2.jsonl")).unwrap();
        let replay: Value = serde_json::from_str(replay_lines.lines().last().unwrap()).unwrap();
        assert!(replay["source_event_ids"].as_array().unwrap().is_empty());
        assert!(replay["path_sample_event_ids"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(replay["source_event_count"], 2);
        assert_eq!(replay["source_event_first_id"], "event-position");
        assert_eq!(replay["source_event_last_id"], "event-terminal");
        assert!(replay["source_event_range_hash"].as_str().is_some());
        assert!(replay["source_event_manifest_ref"].as_str().is_some());

        let lifecycle_lines =
            std::fs::read_to_string(tmp.path().join("shadow_lifecycle_v2.jsonl")).unwrap();
        let lifecycle: Value =
            serde_json::from_str(lifecycle_lines.lines().last().unwrap()).unwrap();
        assert!(lifecycle["source_event_ids"].as_array().unwrap().is_empty());
        assert_eq!(lifecycle["source_event_count"], 2);
        assert!(lifecycle["source_event_range_hash"].as_str().is_some());

        let manifest_lines =
            std::fs::read_to_string(tmp.path().join("shadow_source_ref_manifest_v2.jsonl"))
                .unwrap();
        let manifest_rows: Vec<Value> = manifest_lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(manifest_rows.iter().any(|row| {
            row["source_canonical_high_watermark"] == "event-terminal"
                && row["source_event_count"] == 2
                && row["source_event_first_id"] == "event-position"
                && row["source_event_last_id"] == "event-terminal"
                && row["source_event_range_hash"].as_str().is_some()
        }));
    }

    #[test]
    fn shadow_v2_l2_g_artifact_budget_breach_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = harness_config_for_dir(tmp.path());
        config.artifact_budget.max_density_rows = 1;
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let sample = path_sample_with_pnl(
            "budget-path-sample",
            1_000,
            10,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::Heartbeat,
            event_order_key(Some(45), Some(1)),
        );
        assert_eq!(
            harness
                .append_record(ShadowV2Record::ShadowPathSampleV2(sample))
                .canonical_write,
            ShadowV2WriteStatus::Ok
        );
        let outcome = harness.append_record(ShadowV2Record::ShadowTerminalTruthV2(
            terminal_record("pos-a", "event-terminal-budget"),
        ));
        assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        match outcome.density_write {
            ShadowV2WriteStatus::Err(error) => {
                assert!(error.contains(SHADOW_V2_ARTIFACT_BUDGET_BLOCKER));
            }
            other => panic!("expected density budget error, got {other:?}"),
        }
        assert_eq!(
            outcome.validation_evidence_status,
            ShadowV2ValidationEvidenceStatus::DensityWriteFailed
        );
    }

    #[test]
    fn shadow_v2_l2_g_rotates_jsonl_artifacts_and_records_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = harness_config_for_dir(tmp.path());
        config.artifact_budget.max_file_bytes = 1;
        config.artifact_budget.max_rows_per_file = 1_000;
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let first = harness.append_record(ShadowV2Record::ShadowPositionV2(position_record(
            "pos-a",
            "event-position-a",
        )));
        assert_eq!(first.canonical_write, ShadowV2WriteStatus::Ok);

        let second = harness.append_record(ShadowV2Record::ShadowPositionV2(position_record(
            "pos-b",
            "event-position-b",
        )));
        assert_eq!(second.canonical_write, ShadowV2WriteStatus::Ok);
        assert_eq!(harness.canonical_stream().events().len(), 2);

        let rotated = tmp
            .path()
            .join("shadow_position_event_v2.part-000001.jsonl");
        assert!(rotated.is_file());
        let active = tmp.path().join("shadow_position_event_v2.jsonl");
        assert!(active.is_file());

        let manifest_lines = std::fs::read_to_string(
            tmp.path()
                .join("shadow_artifact_rotation_manifest_v2.jsonl"),
        )
        .unwrap();
        let rows: Vec<Value> = manifest_lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(rows.iter().any(|row| {
            row["schema"] == SHADOW_V2_ARTIFACT_ROTATION_MANIFEST_SCHEMA
                && row["artifact"] == "shadow_position_event_v2"
                && row["rotated_path"].as_str().is_some_and(|path| {
                    path.ends_with("shadow_position_event_v2.part-000001.jsonl")
                })
                && row["row_count"] == 1
                && row["hash_algorithm"] == "blake3"
                && row["hash_uncompressed"].as_str().is_some()
                && row["compressed_path"].is_null()
        }));
    }

    #[test]
    fn shadow_v2_validation_harness_keeps_canonical_after_derived_write_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let replay_path = tmp.path().join("shadow_replay_v2.jsonl");
        std::fs::create_dir_all(&replay_path).unwrap();
        let config = ShadowV2ValidationHarnessConfig::new(
            "run-a",
            tmp.path().join("shadow_position_event_v2.jsonl"),
            replay_path,
            tmp.path().join("shadow_lifecycle_v2.jsonl"),
            tmp.path().join("shadow_path_density_v2.jsonl"),
        );
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();

        let outcome = harness.append_record(ShadowV2Record::ShadowPositionV2(position_record(
            "pos-a",
            "event-position",
        )));

        assert_eq!(outcome.canonical_write, ShadowV2WriteStatus::Ok);
        assert!(outcome.replay_write.is_err());
        assert_eq!(
            outcome.validation_evidence_status,
            ShadowV2ValidationEvidenceStatus::DerivedArtifactWriteFailed
        );
        assert_eq!(harness.canonical_stream().events().len(), 1);
        assert!(tmp.path().join("shadow_position_event_v2.jsonl").is_file());
    }

    #[test]
    fn shadow_v2_validation_harness_rejects_derived_records_as_canonical_input() {
        let tmp = tempfile::tempdir().unwrap();
        let config = harness_config_for_dir(tmp.path());
        let mut harness = ShadowV2ValidationHarness::new(config).unwrap();
        let stream = closed_canonical_stream_for_pr8_pr9();
        let replay = ShadowReplayV2::derive_from_canonical_stream(
            test_envelope("shadow_replay_v2", "pos-a", "replay-derived-a"),
            &stream,
            "pos-a",
            "canonical-ref",
        )
        .unwrap();

        let outcome = harness.append_record(ShadowV2Record::ShadowReplayV2(replay));

        assert!(outcome.canonical_write.is_err());
        assert_eq!(
            outcome.validation_evidence_status,
            ShadowV2ValidationEvidenceStatus::CanonicalWriteFailed
        );
        assert!(harness.canonical_stream().events().is_empty());
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
        assert_eq!(sample.account_data_len, Some(b"account-data".len() as u64));
        assert!(sample.source_account_pubkey.is_some());
        assert!(sample.source_account_owner_or_program.is_some());
        assert_eq!(sample.source_account_slot, Some(41));
        assert_eq!(sample.source_write_version, Some(7));
        assert!(sample.has_complete_account_state_source_proof());
        assert!(sample.is_research_ready());
    }

    #[test]
    fn shadow_v2_account_state_source_proof_is_separate_from_transaction_order() {
        let mut order_key = explicit_unknown_event_order_key();
        order_key.event_seq_in_process = 1;
        let sample = PoolStateSampleV2::from_account_state_core(
            test_envelope("pool_state_sample_v2", "pos-a", "pool-event-account-proof"),
            order_key,
            &canonical_pool_state(),
            1_785_000_000_250,
            Some(account_data_hash_blake3(b"account-data")),
            TemporalClass::PreDecision,
            ClockDomain::StreamObservedMs,
            6,
        );

        assert!(sample.has_complete_account_state_source_proof());
        assert!(sample.account_state_source_proof_blockers().is_empty());
        assert!(!sample
            .event_order_key
            .has_complete_solana_transaction_source_proof());
        assert!(!sample.event_order_key.has_complete_chain_order());
        let blockers = sample.research_blockers();
        assert!(blockers.contains(&"EVENT_ORDER_SIGNATURE_UNKNOWN".to_string()));
        assert!(blockers.contains(&"EVENT_ORDER_TRANSACTION_INDEX_UNKNOWN".to_string()));
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

        let blockers = sample.research_blockers();
        for expected in [
            "EVENT_ORDER_SIGNATURE_UNKNOWN",
            "EVENT_ORDER_TRANSACTION_INDEX_UNKNOWN",
            "EVENT_ORDER_INSTRUCTION_INDEX_UNKNOWN",
            "EVENT_ORDER_INNER_INSTRUCTION_INDEX_UNKNOWN",
            "EVENT_ORDER_LOG_INDEX_UNKNOWN",
        ] {
            assert!(
                blockers.contains(&expected.to_string()),
                "expected blocker {expected}, got {blockers:?}"
            );
        }
        assert!(sample
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT".to_string()));
        assert!(sample
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_UNKNOWN_BUT_REQUIRED_FOR_RESEARCH".to_string()));
        assert!(sample
            .envelope
            .limitations
            .contains(&"EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string()));

        let mut recorder = PoolStateProvenanceRecorder::default();
        let error = recorder.record_research_sample(sample).unwrap_err();
        assert!(matches!(
            error,
            ShadowV2Error::PoolStateBlocked { blockers, .. }
                if blockers.contains(&"EVENT_ORDER_SIGNATURE_UNKNOWN".to_string())
                    && blockers.contains(&"EVENT_ORDER_TRANSACTION_INDEX_UNKNOWN".to_string())
        ));
    }

    #[test]
    fn shadow_v2_event_order_classifies_non_chain_observed_components() {
        let mut derived = event_order_key(Some(42), Some(1));
        derived.signature = EventOrderComponent::derived();
        derived.transaction_index_or_unknown = EventOrderComponent::not_applicable();
        derived.instruction_index_or_unknown = EventOrderComponent::runtime_local();

        assert!(!derived.has_complete_chain_order());
        assert_eq!(
            derived.explicit_unknown_chain_order_components(),
            Vec::<&str>::new()
        );
        assert_eq!(
            derived.not_applicable_or_derived_chain_order_components(),
            vec![
                "signature:DERIVED".to_string(),
                "transaction_index_or_unknown:NOT_APPLICABLE".to_string(),
                "instruction_index_or_unknown:RUNTIME_LOCAL".to_string(),
            ]
        );

        let serialized = serde_json::to_value(&derived).unwrap();
        assert_eq!(serialized["signature"], "DERIVED");
        assert_eq!(serialized["transaction_index_or_unknown"], "NOT_APPLICABLE");
        assert_eq!(serialized["instruction_index_or_unknown"], "RUNTIME_LOCAL");
        assert!(derived
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_CHAIN_COMPONENT_CLASSIFIED_NOT_CHAIN_OBSERVED".to_string()));
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
    fn shadow_v2_pool_state_recorder_canonicalizes_monotonic_event_sequence() {
        let mut recorder = PoolStateProvenanceRecorder::default();
        recorder
            .record_sample(account_state_pool_sample("pool-event-a", 2))
            .unwrap();

        let validation = recorder
            .record_sample(account_state_pool_sample("pool-event-b", 1))
            .unwrap();

        assert!(validation.research_ready);
        let event = recorder
            .canonical_stream
            .events()
            .iter()
            .find(|event| event.envelope.event_id == "pool-event-b")
            .unwrap();
        assert_eq!(
            event
                .event_order_key
                .as_ref()
                .map(|order| order.event_seq_in_process),
            Some(3)
        );
    }

    #[test]
    fn shadow_v2_entry_fill_static_model_reconstructs_buy_fill_from_pool_state() {
        let pool_state = account_state_pool_sample("pool-event-entry", 1);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            250,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-a"),
            fill_order,
            &pool_state,
            &config,
        );

        let reserves =
            reserves_from_pool_state(&pool_state, ShadowV2PoolPhase::BondingCurve).unwrap();
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Buy,
            reserves,
            config.input_sol_lamports,
            config.fee_bps,
            config.slippage_bps,
        )
        .unwrap();

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.envelope.simulation_level,
            SimulationLevel::FillModelStatic
        );
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::ResearchGradeCandidate
        );
        assert_eq!(fill.envelope.temporal_class, TemporalClass::PostEntry);
        assert_eq!(fill.envelope.clock_domain, ClockDomain::LandingTsMs);
        assert_eq!(fill.fill_price, Some(quote.fill_price_sol_per_token));
        assert_eq!(
            fill.fill_price_source.as_deref(),
            Some(quote.price_source_label())
        );
        assert_eq!(fill.min_out, Some(quote.min_output_amount));
        assert_eq!(fill.own_impact_bps, Some(quote.own_impact_bps));
        assert_eq!(fill.fee_bps, Some(100));
        assert_eq!(fill.slippage_bps, Some(250));
        assert_eq!(
            fill.pool_state_before.as_deref(),
            Some(pool_state.envelope.event_id.as_str())
        );
        assert!(fill
            .pool_state_after
            .as_deref()
            .unwrap()
            .contains(SHADOW_V2_PRICE_FORMULA_VERSION));
        assert!(fill
            .limitations
            .contains(&"FILL_MODEL_STATIC_NOT_LIVE_CONFIRMED".to_string()));
        assert!(fill
            .limitations
            .contains(&"REALIZED_SLIPPAGE_BPS_UNAVAILABLE_IN_L1".to_string()));
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(true));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::ResearchCandidate)
        );
        assert_eq!(fill.slippage_tolerance_bps, Some(250));
        assert_eq!(
            fill.deterministic_price_impact_bps,
            Some(quote.own_impact_bps)
        );
        assert_eq!(fill.realized_slippage_bps, None);
        assert_eq!(fill.quote_fill_divergence_bps, None);
    }

    #[test]
    fn shadow_v2_execution_buy_filled_research_candidate_with_hash() {
        let pool_state = account_state_pool_sample("pool-event-entry-research", 1);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-research"),
            fill_order,
            &pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(true));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::ResearchCandidate)
        );
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::ResearchGradeCandidate
        );
        assert!(fill.provenance_blockers.is_empty());
    }

    #[test]
    fn shadow_v2_execution_quote_fill_divergence_is_none_in_l1() {
        let pool_state = account_state_pool_sample("pool-event-entry-quote-divergence", 1);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope(
                "shadow_entry_fill_v2",
                "pos-a",
                "entry-fill-quote-divergence",
            ),
            fill_order,
            &pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.quote_fill_divergence_bps, None);
        assert!(fill
            .limitations
            .contains(&"QUOTE_FILL_DIVERGENCE_UNAVAILABLE_IN_L1".to_string()));
    }

    #[test]
    fn shadow_v2_execution_realized_slippage_is_none_in_l1() {
        let pool_state = account_state_pool_sample("pool-event-entry-realized-slippage", 1);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope(
                "shadow_entry_fill_v2",
                "pos-a",
                "entry-fill-realized-slippage",
            ),
            fill_order,
            &pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.slippage_tolerance_bps, Some(250));
        assert_eq!(fill.realized_slippage_bps, None);
        assert!(fill
            .limitations
            .contains(&"REALIZED_SLIPPAGE_BPS_UNAVAILABLE_IN_L1".to_string()));
    }

    #[test]
    fn shadow_v2_entry_fill_links_pool_state_when_available() {
        let pool_state = account_state_pool_sample("pool-event-entry-linked", 1);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            250,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-linked"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.pool_state_before.as_deref(),
            Some(pool_state.envelope.event_id.as_str())
        );
        assert!(fill.pool_state_after.is_some());
        assert!(fill.fill_price.is_some());
        assert!(fill.fill_amount_sol.is_some());
        assert!(fill.fill_amount_tokens.is_some());
        assert!(fill.slippage_bps.is_some());
        assert!(fill.own_impact_bps.is_some());
        assert!(fill.fee_bps.is_some());
        assert_eq!(
            fill.reconstruction_status,
            "BUY_FILL_RECONSTRUCTED_BY_L1_EXECUTION_ENGINE"
        );
    }

    #[test]
    fn shadow_v2_execution_buy_filled_diagnostic_without_hash() {
        let mut pool_state = account_state_pool_sample("pool-event-entry-no-hash", 1);
        pool_state.account_data_hash = None;
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            250,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-no-hash"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::DiagnosticOnly
        );
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(false));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::DiagnosticSim)
        );
        assert!(fill.fill_price.is_some());
        assert!(fill.pool_state_after.is_some());
        assert!(fill
            .provenance_blockers
            .contains(&"POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string()));
        assert_eq!(fill.realized_slippage_bps, None);
        assert_eq!(fill.quote_fill_divergence_bps, None);
    }

    #[test]
    fn shadow_v2_execution_pool_research_blockers_downgrade_to_diagnostic_fill() {
        let mut missing_observed_slot =
            account_state_pool_sample("pool-event-missing-observed-slot", 1);
        missing_observed_slot.observed_slot = None;
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            missing_observed_slot,
            "entry-fill-missing-observed-slot",
            "POOL_STATE_SLOT_MISSING",
        );

        let mut empty_signature = account_state_pool_sample("pool-event-empty-signature", 1);
        empty_signature.event_order_key.signature = EventOrderComponent::known(String::new());
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            empty_signature,
            "entry-fill-empty-signature",
            "EVENT_ORDER_SIGNATURE_MISSING",
        );

        let mut missing_pool_observed_time =
            account_state_pool_sample("pool-event-missing-observed-time", 1);
        missing_pool_observed_time.observed_at_wall_ms = 0;
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            missing_pool_observed_time,
            "entry-fill-missing-observed-time",
            "POOL_STATE_OBSERVED_AT_WALL_MS_MISSING",
        );

        let mut missing_order_observed_time =
            account_state_pool_sample("pool-event-missing-order-observed-time", 1);
        missing_order_observed_time
            .event_order_key
            .observed_at_wall_ms = 0;
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            missing_order_observed_time,
            "entry-fill-missing-order-observed-time",
            "EVENT_ORDER_OBSERVED_AT_WALL_MS_MISSING",
        );

        let mut unknown_source = account_state_pool_sample("pool-event-unknown-source", 1);
        unknown_source.source = PoolStateSource::Unknown;
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            unknown_source,
            "entry-fill-unknown-source",
            "POOL_STATE_SOURCE_UNKNOWN",
        );

        let mut diagnostic_source =
            account_state_pool_sample("pool-event-shadowledger-diagnostic", 1);
        diagnostic_source.source = PoolStateSource::ShadowLedgerDiagnostic;
        assert_entry_fill_diagnostic_for_pool_research_blocker(
            diagnostic_source,
            "entry-fill-shadowledger-diagnostic",
            "SHADOW_LEDGER_DIAGNOSTIC_NOT_LIVE_TRUTH",
        );
    }

    #[test]
    fn shadow_v2_execution_min_out_returns_no_fill_without_fill_price() {
        let pool_state = account_state_pool_sample("pool-event-entry-min-out", 1);
        let reserves =
            reserves_from_pool_state(&pool_state, ShadowV2PoolPhase::BondingCurve).unwrap();
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Buy,
            reserves,
            1_000_000_000,
            100,
            250,
        )
        .unwrap();
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;

        let outcome = ShadowV2FillEngine::simulate(ShadowV2ExecutionInput {
            side: ShadowV2ExecutionSide::Buy,
            pool_phase: ShadowV2PoolPhase::BondingCurve,
            pool_state_before: Some(&pool_state),
            boundary_kind: ShadowV2BoundaryKind::EntryBefore,
            event_order_key: fill_order,
            input_amount_raw: Some(1_000_000_000),
            min_out_raw: Some(quote.expected_output_amount + 1),
            fee_bps: Some(100),
            slippage_tolerance_bps: Some(250),
            model_version: SHADOW_V2_ENTRY_FILL_MODEL_VERSION.to_string(),
        });

        assert_eq!(outcome.fill_status, FillStatus::NoFill);
        assert_eq!(
            outcome.no_fill_reason,
            Some(ShadowV2NoFillReason::MinOutNotMet)
        );
        assert_eq!(outcome.fill_price, None);
        assert_eq!(outcome.pool_state_after_derived, None);
        assert_eq!(
            outcome.expected_output_raw,
            Some(quote.expected_output_amount)
        );
        assert_eq!(outcome.min_out_raw, Some(quote.expected_output_amount + 1));
    }

    #[test]
    fn shadow_v2_execution_missing_pool_state_blocks() {
        let outcome = ShadowV2FillEngine::simulate(ShadowV2ExecutionInput {
            side: ShadowV2ExecutionSide::Buy,
            pool_phase: ShadowV2PoolPhase::BondingCurve,
            pool_state_before: None,
            boundary_kind: ShadowV2BoundaryKind::EntryBefore,
            event_order_key: event_order_key(Some(43), Some(2)),
            input_amount_raw: Some(1_000_000_000),
            min_out_raw: None,
            fee_bps: Some(100),
            slippage_tolerance_bps: Some(250),
            model_version: SHADOW_V2_ENTRY_FILL_MODEL_VERSION.to_string(),
        });

        assert_eq!(outcome.fill_status, FillStatus::BlockedByData);
        assert_eq!(outcome.execution_simulation_ready, false);
        assert!(outcome
            .blocked_reasons
            .contains(&"BLOCKED_POOL_STATE_MISSING".to_string()));
        assert!(outcome.fill_price.is_none());
    }

    #[test]
    fn shadow_v2_entry_fill_blocks_missing_reserves_hash_and_bad_temporal_class() {
        let mut pool_state = account_state_pool_sample("pool-event-blocked", 1);
        pool_state.account_data_hash = None;
        pool_state.virtual_sol_reserves = None;
        pool_state.envelope.temporal_class = TemporalClass::Outcome;
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            100,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-blocked"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::BlockedByData
        );
        for expected in [
            "POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME",
            "BLOCKED_POOL_STATE_INCOMPLETE",
            "BUY_POOL_STATE_TEMPORAL_CLASS_NOT_ALLOWED=Outcome",
        ] {
            assert!(
                fill.limitations.contains(&expected.to_string()),
                "expected blocker {expected}, got {:?}",
                fill.limitations
            );
        }
        assert!(fill.fill_price.is_none());
        assert!(fill.pool_state_after.is_none());
    }

    #[test]
    fn shadow_v2_entry_fill_blocks_future_pool_state_by_process_sequence() {
        let pool_state = account_state_pool_sample("pool-event-future", 3);
        let mut fill_order = event_order_key(Some(43), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            100,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-future"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert!(fill
            .limitations
            .contains(&"ENTRY_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY".to_string()));
        assert!(fill.fill_price.is_none());
        assert!(fill.pool_state_after.is_none());
    }

    #[test]
    fn shadow_v2_entry_fill_blocks_same_slot_incomplete_order() {
        let mut pool_state = account_state_pool_sample("pool-event-same-slot-ambiguous", 1);
        pool_state.event_order_key.transaction_index_or_unknown = EventOrderComponent::unknown();
        let mut fill_order = event_order_key(Some(42), Some(2));
        fill_order.event_seq_in_process = 2;
        let config = ShadowEntryFillModelConfig::bonding_curve(
            1_000_000_000,
            100,
            100,
            SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
        );

        let fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope(
                "shadow_entry_fill_v2",
                "pos-a",
                "entry-fill-same-slot-ambiguous",
            ),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert!(fill
            .limitations
            .contains(&"ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(fill.fill_price.is_none());
        assert!(fill.pool_state_after.is_none());
    }

    #[test]
    fn shadow_v2_execution_same_slot_ambiguity_blocks() {
        let mut pool_state = account_state_pool_sample("pool-event-execution-same-slot", 1);
        pool_state.event_order_key.transaction_index_or_unknown = EventOrderComponent::unknown();
        let mut fill_order = event_order_key(Some(42), Some(2));
        fill_order.event_seq_in_process = 2;

        let outcome = ShadowV2FillEngine::simulate(ShadowV2ExecutionInput {
            side: ShadowV2ExecutionSide::Buy,
            pool_phase: ShadowV2PoolPhase::BondingCurve,
            pool_state_before: Some(&pool_state),
            boundary_kind: ShadowV2BoundaryKind::EntryBefore,
            event_order_key: fill_order,
            input_amount_raw: Some(1_000_000_000),
            min_out_raw: None,
            fee_bps: Some(100),
            slippage_tolerance_bps: Some(250),
            model_version: SHADOW_V2_ENTRY_FILL_MODEL_VERSION.to_string(),
        });

        assert_eq!(outcome.fill_status, FillStatus::BlockedByData);
        assert_eq!(outcome.execution_simulation_ready, false);
        assert!(outcome
            .blocked_reasons
            .contains(&"ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(outcome.fill_price.is_none());
    }

    #[test]
    fn shadow_v2_entry_attempt_keeps_decision_mark_quote_and_min_out_separate() {
        let pool_state = account_state_pool_sample("pool-event-attempt", 1);
        let reserves =
            reserves_from_pool_state(&pool_state, ShadowV2PoolPhase::BondingCurve).unwrap();
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Buy,
            reserves,
            1_000_000_000,
            100,
            250,
        )
        .unwrap();
        let mut attempt = ShadowEntryAttemptV2 {
            envelope: test_envelope("shadow_entry_attempt_v2", "pos-a", "entry-attempt-a"),
            event_order_key: event_order_key(Some(42), Some(2)),
            intended_entry_ts_ms: clocked(
                "intended_entry_ts_ms",
                1_785_000_000_250,
                ClockDomain::DecisionTsMs,
            ),
            intended_entry_slot: Some(42),
            intended_price_source: "pool_state_sample_v2".to_string(),
            intended_quote: None,
            decision_mark_price: None,
            entry_quote_price: None,
            entry_quote_tokens_out: None,
            entry_quote_min_out: None,
            simulated_submit_ts_ms: None,
            simulated_landing_slot: None,
            simulated_landing_delay_ms: None,
            entry_failure_mode: None,
            executable_fill_model_version: None,
        };

        attempt.attach_static_entry_quote(&quote, SHADOW_V2_ENTRY_FILL_MODEL_VERSION);

        assert_eq!(
            attempt.decision_mark_price,
            Some(quote.mark_price_sol_per_token)
        );
        assert_eq!(
            attempt.entry_quote_price,
            Some(quote.fill_price_sol_per_token)
        );
        assert_eq!(attempt.intended_quote, Some(quote.fill_price_sol_per_token));
        assert_eq!(
            attempt.entry_quote_tokens_out,
            Some(quote.expected_output_amount)
        );
        assert_eq!(attempt.entry_quote_min_out, Some(quote.min_output_amount));
        assert_eq!(
            attempt.executable_fill_model_version.as_deref(),
            Some(SHADOW_V2_ENTRY_FILL_MODEL_VERSION)
        );
        assert_ne!(attempt.decision_mark_price, attempt.entry_quote_price);
    }

    #[test]
    fn shadow_v2_exit_fill_static_model_reconstructs_sell_fill_from_pool_state() {
        let pool_state = post_entry_pool_sample("pool-event-exit", 2, 44, 1);
        let mut fill_order = event_order_key(Some(45), Some(2));
        fill_order.event_seq_in_process = 3;
        let config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            150,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        );

        let fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-a"),
            fill_order,
            &pool_state,
            &config,
        );

        let reserves =
            reserves_from_pool_state(&pool_state, ShadowV2PoolPhase::BondingCurve).unwrap();
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Sell,
            reserves,
            config.input_token_raw,
            config.fee_bps,
            config.slippage_bps,
        )
        .unwrap();

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.envelope.simulation_level,
            SimulationLevel::FillModelStatic
        );
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::ResearchGradeCandidate
        );
        assert_eq!(fill.envelope.temporal_class, TemporalClass::PostExit);
        assert_eq!(fill.envelope.clock_domain, ClockDomain::LandingTsMs);
        assert_eq!(fill.fill_price, Some(quote.fill_price_sol_per_token));
        assert_eq!(
            fill.fill_price_source.as_deref(),
            Some(quote.price_source_label())
        );
        assert_eq!(fill.min_out, Some(quote.min_output_amount));
        assert_eq!(fill.own_impact_bps, Some(quote.own_impact_bps));
        assert_eq!(fill.fee_bps, Some(100));
        assert_eq!(fill.slippage_bps, Some(150));
        assert_eq!(
            fill.pool_state_before.as_deref(),
            Some(pool_state.envelope.event_id.as_str())
        );
        assert!(fill
            .pool_state_after
            .as_deref()
            .unwrap()
            .contains(SHADOW_V2_PRICE_FORMULA_VERSION));
        assert!(fill
            .limitations
            .contains(&"STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string()));
        assert!(fill
            .limitations
            .contains(&"REALIZED_SLIPPAGE_BPS_UNAVAILABLE_IN_L1".to_string()));
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(true));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::ResearchCandidate)
        );
        assert_eq!(fill.slippage_tolerance_bps, Some(150));
        assert_eq!(
            fill.deterministic_price_impact_bps,
            Some(quote.own_impact_bps)
        );
        assert_eq!(fill.realized_slippage_bps, None);
        assert_eq!(fill.quote_fill_divergence_bps, None);
    }

    #[test]
    fn shadow_v2_exit_fill_links_pool_state_when_available() {
        let pool_state = post_entry_pool_sample("pool-event-exit-linked", 2, 44, 1);
        let mut fill_order = event_order_key(Some(45), Some(2));
        fill_order.event_seq_in_process = 3;
        let config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            150,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        );

        let fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-linked"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.pool_state_before.as_deref(),
            Some(pool_state.envelope.event_id.as_str())
        );
        assert!(fill.pool_state_after.is_some());
        assert!(fill.fill_price.is_some());
        assert!(fill.fill_amount_sol.is_some());
        assert!(fill.fill_amount_tokens.is_some());
        assert!(fill.slippage_bps.is_some());
        assert!(fill.own_impact_bps.is_some());
        assert!(fill.fee_bps.is_some());
        assert_eq!(
            fill.reconstruction_status,
            "SELL_FILL_RECONSTRUCTED_BY_L1_EXECUTION_ENGINE"
        );
    }

    #[test]
    fn shadow_v2_execution_sell_filled_diagnostic_without_hash() {
        let mut pool_state = post_entry_pool_sample("pool-event-exit-no-hash", 2, 44, 1);
        pool_state.account_data_hash = None;
        let mut fill_order = event_order_key(Some(45), Some(2));
        fill_order.event_seq_in_process = 3;
        let config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            150,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        );

        let fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-no-hash"),
            fill_order,
            &pool_state,
            &config,
        );

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::DiagnosticOnly
        );
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(false));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::DiagnosticSim)
        );
        assert!(fill.fill_price.is_some());
        assert!(fill.pool_state_after.is_some());
        assert!(fill
            .provenance_blockers
            .contains(&"POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string()));
        assert!(fill
            .limitations
            .contains(&"STATIC_EXIT_FILL_DOES_NOT_ENABLE_ACTIVE_CLOSE".to_string()));
        assert_eq!(fill.realized_slippage_bps, None);
        assert_eq!(fill.quote_fill_divergence_bps, None);
    }

    #[test]
    fn shadow_v2_exit_fill_blocks_future_pool_state_and_same_slot_ambiguity() {
        let future_pool_state = post_entry_pool_sample("pool-event-exit-future", 4, 45, 1);
        let mut fill_order = event_order_key(Some(45), Some(2));
        fill_order.event_seq_in_process = 3;
        let config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            100,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        );

        let future_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-future"),
            fill_order.clone(),
            &future_pool_state,
            &config,
        );

        assert_eq!(future_fill.fill_status, FillStatus::BlockedByData);
        assert!(future_fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY".to_string()));

        let mut ambiguous_pool_state =
            post_entry_pool_sample("pool-event-exit-ambiguous", 2, 45, 1);
        ambiguous_pool_state
            .event_order_key
            .transaction_index_or_unknown = EventOrderComponent::unknown();
        let ambiguous_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-ambiguous"),
            fill_order,
            &ambiguous_pool_state,
            &config,
        );

        assert_eq!(ambiguous_fill.fill_status, FillStatus::BlockedByData);
        assert!(ambiguous_fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(ambiguous_fill.fill_price.is_none());
        assert!(ambiguous_fill.pool_state_after.is_none());
    }

    #[test]
    fn shadow_v2_exit_fill_can_emit_explicit_no_fill_or_failure_without_price_claim() {
        let pool_state = post_entry_pool_sample("pool-event-exit-no-fill", 2, 44, 1);
        let mut fill_order = event_order_key(Some(45), Some(2));
        fill_order.event_seq_in_process = 3;
        let no_fill_config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            100,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        )
        .with_modeled_failure(ShadowExitFillFailureModeV2::NoFill);

        let no_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-no-fill"),
            fill_order.clone(),
            &pool_state,
            &no_fill_config,
        );

        assert_eq!(no_fill.fill_status, FillStatus::NoFill);
        assert_eq!(no_fill.reconstruction_status, "EXIT_FILL_MODELED_NO_FILL");
        assert!(no_fill.fill_price.is_none());
        assert!(no_fill.pool_state_after.is_none());
        assert_eq!(no_fill.execution_simulation_ready, Some(false));
        assert_eq!(no_fill.research_provenance_ready, Some(false));
        assert_eq!(no_fill.no_fill_reason, None);
        assert_eq!(
            no_fill.fail_reason.as_deref(),
            Some("MODELED_EXIT_NO_FILL_NOT_L1_EXECUTION_SIM")
        );
        assert_eq!(no_fill.expected_output_raw, None);
        assert_eq!(no_fill.output_amount_raw, None);
        assert_eq!(no_fill.deterministic_price_impact_bps, None);
        assert_eq!(
            no_fill.envelope.measurement_grade,
            MeasurementGrade::DiagnosticOnly
        );
        assert!(no_fill
            .limitations
            .contains(&"EXIT_FILL_MODELED_NO_FILL_NOT_LIVE_CONFIRMED".to_string()));
        assert!(no_fill
            .limitations
            .contains(&"MODELED_EXIT_FAILURE_NOT_L1_EXECUTION_SIM".to_string()));

        let failed_config = ShadowExitFillModelConfig::bonding_curve(
            10_000_000_000,
            100,
            100,
            SHADOW_V2_EXIT_FILL_MODEL_VERSION,
        )
        .with_modeled_failure(ShadowExitFillFailureModeV2::Failed);
        let failed = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-failed"),
            fill_order,
            &pool_state,
            &failed_config,
        );

        assert_eq!(failed.fill_status, FillStatus::Failed);
        assert_eq!(failed.reconstruction_status, "EXIT_FILL_MODELED_FAILED");
        assert!(failed.fill_price.is_none());
        assert_eq!(failed.execution_simulation_ready, Some(false));
    }

    #[test]
    fn shadow_v2_terminal_truth_sets_executable_pnl_only_when_exit_fill_executable() {
        let entry_pool_state = account_state_pool_sample("pool-event-entry-terminal", 1);
        let mut entry_fill_order = event_order_key(Some(43), Some(2));
        entry_fill_order.event_seq_in_process = 2;
        let entry_fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-a", "entry-fill-terminal"),
            entry_fill_order,
            &entry_pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        let exit_pool_state = post_entry_pool_sample("pool-event-exit-terminal", 3, 44, 1);
        let mut exit_fill_order = event_order_key(Some(45), Some(2));
        exit_fill_order.event_seq_in_process = 4;
        let exit_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-terminal"),
            exit_fill_order.clone(),
            &exit_pool_state,
            &ShadowExitFillModelConfig::bonding_curve(
                10_000_000_000,
                150,
                100,
                SHADOW_V2_EXIT_FILL_MODEL_VERSION,
            ),
        );
        assert_eq!(entry_fill.fill_status, FillStatus::Filled);
        assert_eq!(exit_fill.fill_status, FillStatus::Filled);

        let executable_pnl = executable_pnl_bps_from_entry_exit_fills(&entry_fill, &exit_fill);
        assert!(executable_pnl.is_some());

        let blocked_exit = ShadowExitFillV2::blocked_without_pool_state(
            test_envelope("shadow_exit_fill_v2", "pos-a", "exit-fill-terminal-blocked"),
            exit_fill_order,
            vec!["EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string()],
        );
        assert_eq!(
            executable_pnl_bps_from_entry_exit_fills(&entry_fill, &blocked_exit),
            None
        );

        let mut terminal = terminal_record("pos-a", "terminal-with-executable-pnl");
        terminal.final_pnl_executable_bps = executable_pnl;
        assert!(terminal.final_pnl_executable_bps.is_some());

        let mut blocked_terminal = terminal_record("pos-a", "terminal-without-executable-pnl");
        blocked_terminal.final_pnl_executable_bps =
            executable_pnl_bps_from_entry_exit_fills(&entry_fill, &blocked_exit);
        assert!(blocked_terminal.final_pnl_executable_bps.is_none());
    }

    #[test]
    fn shadow_v2_executable_pnl_link_requires_same_position_filled_entry_and_exit() {
        let entry_pool_state = account_state_pool_sample("pool-event-entry-link", 1);
        let mut entry_fill_order = event_order_key(Some(43), Some(2));
        entry_fill_order.event_seq_in_process = 2;
        let entry_fill = ShadowEntryFillV2::from_static_buy_model(
            test_envelope("shadow_entry_fill_v2", "pos-link-a", "entry-fill-link"),
            entry_fill_order,
            &entry_pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                1_000_000_000,
                250,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );

        let exit_pool_state = post_entry_pool_sample("pool-event-exit-link", 3, 44, 1);
        let mut exit_fill_order = event_order_key(Some(45), Some(2));
        exit_fill_order.event_seq_in_process = 4;
        let exit_fill = ShadowExitFillV2::from_static_sell_model(
            test_envelope("shadow_exit_fill_v2", "pos-link-a", "exit-fill-link"),
            exit_fill_order.clone(),
            &exit_pool_state,
            &ShadowExitFillModelConfig::bonding_curve(
                10_000_000_000,
                150,
                100,
                SHADOW_V2_EXIT_FILL_MODEL_VERSION,
            ),
        );

        let mut stream = ShadowV2CanonicalEventStream::default();
        stream
            .append_record(ShadowV2Record::ShadowEntryFillV2(entry_fill.clone()))
            .expect("append entry fill");
        let link = executable_pnl_link_from_canonical_position_fills(
            &stream,
            "pos-link-a",
            Some(&exit_fill),
        )
        .expect("executable pnl link");
        assert_eq!(link.linked_entry_fill, "entry-fill-link");
        assert_eq!(link.linked_exit_fill, "exit-fill-link");
        assert_eq!(
            Some(link.final_pnl_executable_bps),
            executable_pnl_bps_from_entry_exit_fills(&entry_fill, &exit_fill)
        );

        let blocked_exit = ShadowExitFillV2::blocked_without_pool_state(
            test_envelope(
                "shadow_exit_fill_v2",
                "pos-link-a",
                "exit-fill-link-blocked",
            ),
            exit_fill_order.clone(),
            vec!["EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string()],
        );
        assert!(executable_pnl_link_from_canonical_position_fills(
            &stream,
            "pos-link-a",
            Some(&blocked_exit)
        )
        .is_none());

        let other_position_exit = ShadowExitFillV2::from_static_sell_model(
            test_envelope(
                "shadow_exit_fill_v2",
                "pos-link-b",
                "exit-fill-other-position",
            ),
            exit_fill_order,
            &exit_pool_state,
            &ShadowExitFillModelConfig::bonding_curve(
                10_000_000_000,
                150,
                100,
                SHADOW_V2_EXIT_FILL_MODEL_VERSION,
            ),
        );
        assert!(executable_pnl_link_from_canonical_position_fills(
            &stream,
            "pos-link-a",
            Some(&other_position_exit)
        )
        .is_none());
    }

    #[test]
    fn shadow_v2_fill_remains_blocked_when_pool_state_missing() {
        let mut entry_fill_order = event_order_key(Some(43), Some(2));
        entry_fill_order.event_seq_in_process = 2;
        let entry_fill = ShadowEntryFillV2::blocked_without_pool_state(
            test_envelope(
                "shadow_entry_fill_v2",
                "pos-a",
                "entry-fill-missing-pool-state",
            ),
            entry_fill_order,
            vec![
                "ENTRY_POOL_STATE_BEFORE_UNAVAILABLE".to_string(),
                "FILL_PRICE_UNAVAILABLE".to_string(),
            ],
        );

        assert_eq!(entry_fill.fill_status, FillStatus::BlockedByData);
        assert!(entry_fill.pool_state_before.is_none());
        assert!(entry_fill.fill_price.is_none());
        assert!(entry_fill
            .limitations
            .contains(&"ENTRY_POOL_STATE_BEFORE_UNAVAILABLE".to_string()));

        let mut exit_fill_order = event_order_key(Some(45), Some(2));
        exit_fill_order.event_seq_in_process = 3;
        let exit_fill = ShadowExitFillV2::blocked_without_pool_state(
            test_envelope(
                "shadow_exit_fill_v2",
                "pos-a",
                "exit-fill-missing-pool-state",
            ),
            exit_fill_order,
            vec![
                "EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string(),
                "FILL_PRICE_UNAVAILABLE".to_string(),
            ],
        );

        assert_eq!(exit_fill.fill_status, FillStatus::BlockedByData);
        assert!(exit_fill.pool_state_before.is_none());
        assert!(exit_fill.fill_price.is_none());
        assert!(exit_fill
            .limitations
            .contains(&"EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string()));
    }

    #[test]
    fn shadow_v2_exit_attempt_requires_tie_break_for_same_slot_ambiguity() {
        let mut attempt = ShadowExitAttemptV2::from_mark_path_trigger(
            test_envelope("shadow_exit_attempt_v2", "pos-a", "exit-attempt-a"),
            explicit_unknown_event_order_key(),
            "TARGET_OR_STOP",
            clocked(
                "trigger_ts_ms",
                1_785_000_001_000,
                ClockDomain::StreamObservedMs,
            ),
            Some(45),
            "path_sampler_v2",
            Some(1_200),
            Some(-600),
            Some(45_000),
            true,
            None,
        );

        let blockers = attempt.research_blockers();
        assert!(blockers.contains(&"EXIT_ATTEMPT_TIE_BREAK_POLICY_MISSING".to_string()));
        assert!(attempt
            .envelope
            .limitations
            .contains(&"EXIT_ATTEMPT_SAME_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK".to_string()));

        attempt.tie_break_policy = Some("BLOCK_AMBIGUOUS".to_string());
        attempt.attach_static_exit_model(SHADOW_V2_EXIT_FILL_MODEL_VERSION);
        assert!(!attempt
            .research_blockers()
            .contains(&"EXIT_ATTEMPT_TIE_BREAK_POLICY_MISSING".to_string()));
        assert_eq!(
            attempt.executable_fill_model_version.as_deref(),
            Some(SHADOW_V2_EXIT_FILL_MODEL_VERSION)
        );
    }

    #[test]
    fn shadow_v2_exit_path_replay_separates_sampled_and_exact_level_hits() {
        let mut order_a = event_order_key(Some(45), Some(1));
        order_a.event_seq_in_process = 10;
        let mut order_b = event_order_key(Some(45), Some(2));
        order_b.event_seq_in_process = 11;
        let mut order_c = event_order_key(Some(46), Some(0));
        order_c.event_seq_in_process = 12;
        let samples = vec![
            path_sample_with_pnl(
                "path-pre-hit",
                1_000,
                200,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
                order_a,
            ),
            path_sample_with_pnl(
                "path-sampled-target",
                2_000,
                650,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
                order_b,
            ),
            path_sample_with_pnl(
                "path-exact-target",
                2_500,
                700,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::LevelHit,
                order_c,
            ),
        ];
        let config = ShadowExitPathReplayConfigV2::new(
            Some(600),
            Some(-500),
            3_000,
            ShadowExitTieBreakPolicyV2::BlockAmbiguous,
        );

        let result = replay_exit_from_path_v2(&samples, &config);

        assert_eq!(
            result.selected_exit.terminal_reason,
            TerminalReasonV2::Target
        );
        assert_eq!(
            result.selected_exit.hit_source,
            ShadowExitHitSourceV2::SampledPath
        );
        assert_eq!(result.selected_exit.age_ms, Some(2_000));
        assert_eq!(
            result
                .sampled_path_hit
                .as_ref()
                .unwrap()
                .path_sample_ref
                .as_deref(),
            Some("path-sampled-target")
        );
        assert_eq!(
            result
                .exact_level_hit
                .as_ref()
                .unwrap()
                .path_sample_ref
                .as_deref(),
            Some("path-exact-target")
        );
        assert_eq!(result.mfe_mark_bps, Some(650));
        assert_eq!(result.mae_mark_bps, Some(200));
        assert_eq!(result.terminal_pnl_mark_bps, Some(650));
    }

    #[test]
    fn shadow_v2_exit_path_replay_blocks_or_tie_breaks_ambiguous_target_stop() {
        let mut target_order = event_order_key(Some(45), Some(1));
        target_order.event_seq_in_process = 10;
        target_order.transaction_index_or_unknown = EventOrderComponent::unknown();
        let mut stop_order = event_order_key(Some(45), Some(2));
        stop_order.event_seq_in_process = 11;
        stop_order.transaction_index_or_unknown = EventOrderComponent::unknown();
        let samples = vec![
            path_sample_with_pnl(
                "path-target-ambiguous",
                2_000,
                700,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
                target_order,
            ),
            path_sample_with_pnl(
                "path-stop-ambiguous",
                2_000,
                -700,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
                stop_order,
            ),
        ];
        let blocked_config = ShadowExitPathReplayConfigV2::new(
            Some(600),
            Some(-600),
            3_000,
            ShadowExitTieBreakPolicyV2::BlockAmbiguous,
        );
        let blocked = replay_exit_from_path_v2(&samples, &blocked_config);

        assert_eq!(
            blocked.selected_exit.terminal_reason,
            TerminalReasonV2::BlockedByData
        );
        assert!(blocked
            .limitations
            .contains(&"EXIT_PATH_TARGET_STOP_ORDER_AMBIGUOUS".to_string()));
        assert!(blocked.selected_exit.same_slot_ambiguity);

        let stop_first_config = ShadowExitPathReplayConfigV2::new(
            Some(600),
            Some(-600),
            3_000,
            ShadowExitTieBreakPolicyV2::StopFirst,
        );
        let stop_first = replay_exit_from_path_v2(&samples, &stop_first_config);

        assert_eq!(
            stop_first.selected_exit.terminal_reason,
            TerminalReasonV2::Stop
        );
        assert!(stop_first.selected_exit.same_slot_ambiguity);
        assert!(stop_first
            .selected_exit
            .limitations
            .contains(&"EXIT_PATH_TARGET_STOP_AMBIGUITY_RESOLVED_STOP_FIRST".to_string()));
    }

    #[test]
    fn shadow_v2_exit_path_replay_timeout_requires_path_coverage() {
        let mut order_a = event_order_key(Some(45), Some(1));
        order_a.event_seq_in_process = 10;
        let mut order_b = event_order_key(Some(45), Some(2));
        order_b.event_seq_in_process = 11;
        let sparse_samples = vec![
            path_sample_with_pnl(
                "path-timeout-a",
                1_000,
                100,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order_a.clone(),
            ),
            path_sample_with_pnl(
                "path-timeout-b",
                2_000,
                150,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order_b.clone(),
            ),
        ];
        let config = ShadowExitPathReplayConfigV2::new(
            Some(600),
            Some(-600),
            3_000,
            ShadowExitTieBreakPolicyV2::BlockAmbiguous,
        );
        let blocked = replay_exit_from_path_v2(&sparse_samples, &config);

        assert_eq!(
            blocked.selected_exit.terminal_reason,
            TerminalReasonV2::BlockedByData
        );
        assert!(blocked
            .limitations
            .contains(&"TIMEOUT_MAX_HOLD_EXCEEDS_REPLAY_HORIZON".to_string()));

        let mut covered_samples = sparse_samples;
        let mut order_c = event_order_key(Some(46), Some(0));
        order_c.event_seq_in_process = 12;
        covered_samples.push(path_sample_with_pnl(
            "path-timeout-c",
            3_000,
            125,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::Terminal,
            order_c,
        ));
        let timeout = replay_exit_from_path_v2(&covered_samples, &config);

        assert_eq!(
            timeout.selected_exit.terminal_reason,
            TerminalReasonV2::Timeout
        );
        assert_eq!(
            timeout.selected_exit.hit_source,
            ShadowExitHitSourceV2::TimeoutPathPoint
        );
        assert_eq!(timeout.terminal_pnl_mark_bps, Some(125));
        assert!(timeout
            .selected_exit
            .limitations
            .contains(&"TIMEOUT_PNL_USES_REAL_PATH_POINT".to_string()));

        let zero_horizon_config = ShadowExitPathReplayConfigV2::new(
            Some(600),
            Some(-600),
            0,
            ShadowExitTieBreakPolicyV2::BlockAmbiguous,
        );
        let zero_horizon = replay_exit_from_path_v2(&covered_samples, &zero_horizon_config);
        assert_eq!(
            zero_horizon.selected_exit.terminal_reason,
            TerminalReasonV2::BlockedByData
        );
        assert!(zero_horizon
            .limitations
            .contains(&"EXIT_PATH_REPLAY_MAX_HOLD_MS_ZERO".to_string()));
    }

    #[test]
    fn shadow_v2_path_sample_reconstructs_mark_pnl_and_attaches_static_exit_quote() {
        let pool_state = post_entry_pool_sample("pool-event-path", 2, 44, 1);
        let reserves =
            reserves_from_pool_state(&pool_state, ShadowV2PoolPhase::BondingCurve).unwrap();
        let quote = quote_constant_product(
            ShadowV2PoolPhase::BondingCurve,
            ShadowV2QuoteSide::Sell,
            reserves,
            10_000_000_000,
            100,
            100,
        )
        .unwrap();
        let entry_mark = pool_state.price_sol_per_token.unwrap() * 0.95;
        let mut sample = ShadowPathSampleV2::from_pool_state_mark(
            test_envelope("shadow_path_sample_v2", "pos-a", "path-sample-a"),
            pool_state.event_order_key.clone(),
            clocked(
                "sample_ts_ms",
                1_785_000_001_000,
                ClockDomain::StreamObservedMs,
            ),
            1_000,
            &pool_state,
            ShadowV2PoolPhase::BondingCurve,
            Some(entry_mark),
            ShadowPathSamplingModeV2::Dense3s,
            ShadowPathSamplingReasonV2::EventSample,
        );

        assert_eq!(sample.envelope.schema, "shadow_path_sample_v2");
        assert_eq!(sample.envelope.simulation_level, SimulationLevel::MarkOnly);
        assert_eq!(
            sample.envelope.measurement_grade,
            MeasurementGrade::MarkPriceReplay
        );
        assert_eq!(sample.sampling_mode, ShadowPathSamplingModeV2::Dense3s);
        assert_eq!(sample.path_horizon_ms, 3_000);
        assert_eq!(sample.pool_state_ref, pool_state.envelope.event_id);
        assert!(sample.mark_price.is_some());
        assert!(sample.pnl_mark_bps.unwrap() > 0);
        assert_eq!(sample.exact_or_approx, "EXACT_EVENT_ORDER");

        sample.attach_static_exit_quote(&quote, Some(entry_mark));

        assert_eq!(
            sample.envelope.simulation_level,
            SimulationLevel::FillModelStatic
        );
        assert_eq!(
            sample.envelope.measurement_grade,
            MeasurementGrade::ResearchGradeCandidate
        );
        assert_eq!(
            sample.executable_exit_quote,
            Some(quote.fill_price_sol_per_token)
        );
        assert!(sample.pnl_executable_bps.is_some());
        assert!(sample
            .envelope
            .limitations
            .contains(&"EXECUTABLE_EXIT_QUOTE_IS_STATIC_MODEL_NOT_LIVE_FILL".to_string()));
    }

    #[test]
    fn shadow_v2_path_density_supports_dense_2s_3s_and_blocks_unsupported_long_horizons() {
        let config = ShadowPathSamplerConfigV2::dense_3s();
        let pool_state = post_entry_pool_sample("pool-event-density", 2, 44, 1);
        let mut samples = Vec::new();
        for (idx, age_ms) in [0_u64, 1_000, 2_000, 3_000].into_iter().enumerate() {
            let mut order = pool_state.event_order_key.clone();
            order.event_seq_in_process = 2 + idx as u64;
            let event_id = format!("path-sample-{idx}");
            samples.push(ShadowPathSampleV2::from_pool_state_mark(
                test_envelope("shadow_path_sample_v2", "pos-a", &event_id),
                order,
                clocked(
                    "sample_ts_ms",
                    1_785_000_001_000 + age_ms as i64,
                    ClockDomain::StreamObservedMs,
                ),
                age_ms,
                &pool_state,
                ShadowV2PoolPhase::BondingCurve,
                Some(0.00003),
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
            ));
        }

        let evaluations = evaluate_path_density_v2(&samples, &config, &[2_000, 3_000, 300_000]);

        assert_eq!(
            evaluations[0].verdict,
            ShadowPathHorizonVerdictV2::EvaluableExact
        );
        assert_eq!(
            evaluations[1].verdict,
            ShadowPathHorizonVerdictV2::EvaluableExact
        );
        assert_eq!(
            evaluations[2].verdict,
            ShadowPathHorizonVerdictV2::NotEvaluableHorizonExceedsReplay
        );
        assert!(evaluations[2]
            .limitations
            .contains(&"HORIZON_EXCEEDS_CONFIGURED_PATH_MODE".to_string()));
    }

    #[test]
    fn shadow_v2_path_density_marks_sparse_and_no_coverage_explicitly() {
        let config = ShadowPathSamplerConfigV2::standard_120s();
        let pool_state = post_entry_pool_sample("pool-event-density-sparse", 2, 44, 1);
        let samples = [10_000_u64, 30_000]
            .into_iter()
            .enumerate()
            .map(|(idx, age_ms)| {
                let event_id = format!("path-sparse-{idx}");
                ShadowPathSampleV2::from_pool_state_mark(
                    test_envelope("shadow_path_sample_v2", "pos-a", &event_id),
                    pool_state.event_order_key.clone(),
                    clocked(
                        "sample_ts_ms",
                        1_785_000_001_000 + age_ms as i64,
                        ClockDomain::StreamObservedMs,
                    ),
                    age_ms,
                    &pool_state,
                    ShadowV2PoolPhase::BondingCurve,
                    Some(0.00003),
                    ShadowPathSamplingModeV2::Standard120s,
                    ShadowPathSamplingReasonV2::Heartbeat,
                )
            })
            .collect::<Vec<_>>();

        let evaluations = evaluate_path_density_v2(&samples, &config, &[3_000, 20_000, 500_000]);

        assert_eq!(
            evaluations[0].verdict,
            ShadowPathHorizonVerdictV2::NotEvaluableNoCoverage
        );
        assert_eq!(
            evaluations[1].verdict,
            ShadowPathHorizonVerdictV2::SparseApproxOnly
        );
        assert_eq!(
            evaluations[2].verdict,
            ShadowPathHorizonVerdictV2::NotEvaluableHorizonExceedsReplay
        );
    }

    #[test]
    fn shadow_v2_path_density_reports_duplicate_and_non_monotonic_input() {
        let config = ShadowPathSamplerConfigV2::standard_120s();
        let mut order_a = event_order_key(Some(45), Some(1));
        order_a.event_seq_in_process = 10;
        let mut order_b = event_order_key(Some(45), Some(2));
        order_b.event_seq_in_process = 11;
        let mut order_c = event_order_key(Some(45), Some(3));
        order_c.event_seq_in_process = 12;
        let samples = vec![
            path_sample_with_pnl(
                "path-density-nonmono-a",
                30_000,
                10,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order_a,
            ),
            path_sample_with_pnl(
                "path-density-nonmono-b",
                10_000,
                20,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order_b,
            ),
            path_sample_with_pnl(
                "path-density-duplicate",
                10_000,
                25,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order_c,
            ),
        ];

        let evaluations = evaluate_path_density_v2(&samples, &config, &[30_000]);

        assert_eq!(evaluations[0].duplicate_age_count, 1);
        assert!(evaluations[0].non_monotonic_input);
        assert!(evaluations[0]
            .limitations
            .contains(&"PATH_DENSITY_INPUT_NON_MONOTONIC".to_string()));
        assert!(evaluations[0]
            .limitations
            .contains(&"PATH_DENSITY_DUPLICATE_AGE_COUNT=1".to_string()));
    }

    #[test]
    fn shadow_v2_standard_120s_sampler_retains_l2_baseline_margin_sample() {
        let config = ShadowPathSamplerConfigV2::standard_120s();
        let mut samples = Vec::new();
        for idx in 0..=121 {
            let mut order = event_order_key(Some(45), Some(idx as u32));
            order.event_seq_in_process = 10 + idx as u64;
            samples.push(path_sample_with_pnl(
                &format!("path-standard-margin-{idx}"),
                idx as u64 * 1_000,
                idx as i32,
                ShadowPathSamplingModeV2::Standard120s,
                ShadowPathSamplingReasonV2::Heartbeat,
                order,
            ));
        }

        let selected = select_path_samples_v2(&samples, &config);
        let evaluations = evaluate_path_density_v2(&selected, &config, &[120_000, 300_000]);

        assert_eq!(config.max_horizon_ms, 121_000);
        assert_eq!(selected.last().map(|sample| sample.age_ms), Some(121_000));
        assert_eq!(
            evaluations[0].verdict,
            ShadowPathHorizonVerdictV2::EvaluableExact
        );
        assert_eq!(evaluations[0].replay_horizon_ms, Some(121_000));
        assert_eq!(
            evaluations[1].verdict,
            ShadowPathHorizonVerdictV2::NotEvaluableHorizonExceedsReplay
        );
        assert!(evaluations[1]
            .limitations
            .contains(&"HORIZON_EXCEEDS_CONFIGURED_PATH_MODE".to_string()));
    }

    #[test]
    fn shadow_v2_path_sampler_dense_keeps_all_event_samples_over_max_points_cap() {
        let dense = ShadowPathSamplerConfigV2 {
            max_path_points: 3,
            ..ShadowPathSamplerConfigV2::dense_3s()
        };
        let standard = ShadowPathSamplerConfigV2::standard_120s();
        let mut samples = Vec::new();
        for idx in 0..5 {
            let mut order = event_order_key(Some(45), Some(idx));
            order.event_seq_in_process = 10 + idx as u64;
            samples.push(path_sample_with_pnl(
                &format!("path-dense-event-{idx}"),
                100 + idx as u64,
                idx as i32,
                ShadowPathSamplingModeV2::Dense3s,
                ShadowPathSamplingReasonV2::EventSample,
                order,
            ));
        }

        let dense_selected = select_path_samples_v2(&samples, &dense);
        let standard_selected = select_path_samples_v2(&samples, &standard);

        assert_eq!(dense_selected.len(), 5);
        assert!(!dense_selected.last().unwrap().truncated);
        assert!(dense_selected
            .last()
            .unwrap()
            .envelope
            .limitations
            .contains(
                &"PATH_SAMPLER_STORAGE_BUDGET_EXCEEDED_PROTECTED_SAMPLES_RETAINED".to_string()
            ));
        assert_eq!(standard_selected.len(), 1);
    }

    #[test]
    fn shadow_v2_path_sampler_modes_define_sampling_policy() {
        let dense = ShadowPathSamplerConfigV2::dense_3s();
        let standard = ShadowPathSamplerConfigV2::standard_120s();
        let long = ShadowPathSamplerConfigV2::long_500s();

        assert_eq!(dense.max_horizon_ms, 3_000);
        assert_eq!(standard.max_horizon_ms, 121_000);
        assert_eq!(long.max_horizon_ms, 500_000);
        assert!(dense.keep_every_event_sample);
        assert!(!standard.keep_every_event_sample);
        assert!(dense.requires_storage_budget);
        assert!(!standard.requires_storage_budget);
        assert!(long.requires_storage_budget);
        assert!(dense.should_keep_sample(
            100,
            10,
            ShadowPathSamplingReasonV2::EventSample,
            None,
            None
        ));
        assert!(standard.should_keep_sample(
            500,
            250,
            ShadowPathSamplingReasonV2::LargePriceDelta,
            Some(100),
            Some(0)
        ));
        assert!(long.should_keep_sample(
            100,
            0,
            ShadowPathSamplingReasonV2::Terminal,
            Some(90),
            Some(0)
        ));
        assert!(!standard.should_keep_sample(
            500,
            10,
            ShadowPathSamplingReasonV2::Heartbeat,
            Some(100),
            Some(0)
        ));
    }
}
