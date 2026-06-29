//! Shadow Burnin Simulation V2 contract types.
//!
//! These types are intentionally inert. PR1 defines the schema and validation
//! vocabulary only; no runtime writer, lifecycle path, replay path, BUY/REJECT
//! policy, selector, TX/Jito path, shadow_close_only path, or active close path
//! consumes these records yet.

use serde::{Deserialize, Serialize};

pub const SHADOW_V2_SIMULATION_CONTRACT_VERSION: &str = "shadow_burnin_simulation_v2_20260629";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventOrderKey {
    pub slot: Option<u64>,
    pub block_time: Option<i64>,
    pub signature: Option<String>,
    pub transaction_index_or_unknown: Option<u32>,
    pub instruction_index_or_unknown: Option<u32>,
    pub inner_instruction_index_or_unknown: Option<u32>,
    pub log_index_or_unknown: Option<u32>,
    pub event_seq_in_process: u64,
    pub observed_at_wall_ms: u64,
}

impl EventOrderKey {
    pub fn has_complete_chain_order(&self) -> bool {
        self.slot.is_some()
            && self.signature.is_some()
            && self.transaction_index_or_unknown.is_some()
            && self.instruction_index_or_unknown.is_some()
            && self.inner_instruction_index_or_unknown.is_some()
            && self.log_index_or_unknown.is_some()
    }

    pub fn same_slot_ambiguous_with(&self, other: &Self) -> bool {
        self.slot.is_some()
            && self.slot == other.slot
            && (!self.has_complete_chain_order() || !other.has_complete_chain_order())
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

    fn event_order_key(slot: Option<u64>, tx_index: Option<u32>) -> EventOrderKey {
        EventOrderKey {
            slot,
            block_time: Some(1_785_000_000),
            signature: Some("sig".to_string()),
            transaction_index_or_unknown: tx_index,
            instruction_index_or_unknown: Some(0),
            inner_instruction_index_or_unknown: Some(0),
            log_index_or_unknown: Some(0),
            event_seq_in_process: 7,
            observed_at_wall_ms: 1_785_000_000_123,
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
}
