//! Event Schema — all structured event types for the execution pipeline.
//!
//! # Design
//!
//! Every event is wrapped in an `ExecutionEvent` = `EventEnvelope` + `EventKind`.
//! The envelope carries global identifiers (run_id, lane, candidate_id, etc.)
//! while the kind-specific payload carries the domain data.
//!
//! # Minimal Event Set (12 kinds)
//!
//! ```text
//! Candidate → EntrySubmitted → EntryFilled → PositionOpened
//!   → (AemTick)* → ControlCommandIssued → ControlCommandApplied
//!   → ExitSubmitted → ExitFilled → PositionClosed
//!   → ManagementDecision → ManagementOutcome
//! ```

use serde::{Deserialize, Serialize};

use crate::execution::backend::{
    CandidateId, CommandId, ExecutionStressSnapshot, FillStatus, Lane, OrderId, OrderSide,
    PositionId, QuoteId, StressBucket,
};

// ─── Envelope (present on EVERY event) ──────────────────────────────────────

/// Global identifiers present on every event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// UUID of the bot run session.
    pub run_id: String,
    /// Which lane this event belongs to.
    pub lane: Lane,
    /// Candidate identifier: `{mint}_{pool_amm_id}_{first_seen_slot}`.
    pub candidate_id: CandidateId,
    /// Position identifier (present from PositionOpened onwards).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
    /// Position epoch counter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_epoch: Option<u64>,
    /// Unique event ID (UUID or monotonic).
    pub event_id: String,
    /// Unix ms when this event was created.
    pub event_time_ms: u64,
    /// Solana slot (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    /// Reference to the quote used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_id: Option<QuoteId>,
    /// Reference to AEM command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<CommandId>,
    /// Reference to order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<OrderId>,
}

impl EventEnvelope {
    /// Create a minimal envelope with required fields.
    pub fn new(run_id: String, lane: Lane, candidate_id: CandidateId, event_time_ms: u64) -> Self {
        Self {
            run_id,
            lane,
            candidate_id,
            position_id: None,
            position_epoch: None,
            event_id: Self::generate_event_id(),
            event_time_ms,
            slot: None,
            quote_id: None,
            command_id: None,
            order_id: None,
        }
    }

    /// Derive a new envelope from an existing one (same run_id, lane, candidate_id)
    /// with a fresh event_id and updated time.
    pub fn derive(&self, event_time_ms: u64) -> Self {
        Self {
            run_id: self.run_id.clone(),
            lane: self.lane,
            candidate_id: self.candidate_id.clone(),
            position_id: self.position_id.clone(),
            position_epoch: self.position_epoch,
            event_id: Self::generate_event_id(),
            event_time_ms,
            slot: self.slot,
            quote_id: self.quote_id.clone(),
            command_id: self.command_id.clone(),
            order_id: self.order_id.clone(),
        }
    }

    fn generate_event_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

// ─── ExecutionEvent ─────────────────────────────────────────────────────────

/// A single event in the execution timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub envelope: EventEnvelope,
    pub kind: EventKind,
}

impl ExecutionEvent {
    pub fn new(envelope: EventEnvelope, kind: EventKind) -> Self {
        Self { envelope, kind }
    }
}

// ─── EventKind ──────────────────────────────────────────────────────────────

/// Discriminated union of all event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum EventKind {
    // ── Ingest / selector evidence ──────────────────────────────────────
    NewPoolDetected(NewPoolDetectedPayload),
    PoolTransaction(PoolTransactionPayload),

    // ── Mandatory (12) ──────────────────────────────────────────────────
    Candidate(CandidatePayload),
    EntrySubmitted(EntrySubmittedPayload),
    EntryFilled(EntryFilledPayload),
    PositionOpened(PositionOpenedPayload),
    AemTick(AemTickPayload),
    ControlCommandIssued(ControlCommandIssuedPayload),
    ControlCommandApplied(ControlCommandAppliedPayload),
    ExitSubmitted(ExitSubmittedPayload),
    ExitFilled(ExitFilledPayload),
    PositionClosed(PositionClosedPayload),
    ManagementDecision(ManagementDecisionPayload),
    ManagementOutcome(ManagementOutcomePayload),

    // ── Optional (3) ────────────────────────────────────────────────────
    ExecutionStressChanged(ExecutionStressChangedPayload),
    OracleStale(OracleStalePayload),
    LedgerDegraded(LedgerDegradedPayload),
}

impl EventKind {
    /// Returns the event type name as a string (for logging / filtering).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::NewPoolDetected(_) => "NewPoolDetected",
            Self::PoolTransaction(_) => "PoolTransaction",
            Self::Candidate(_) => "Candidate",
            Self::EntrySubmitted(_) => "EntrySubmitted",
            Self::EntryFilled(_) => "EntryFilled",
            Self::PositionOpened(_) => "PositionOpened",
            Self::AemTick(_) => "AemTick",
            Self::ControlCommandIssued(_) => "ControlCommandIssued",
            Self::ControlCommandApplied(_) => "ControlCommandApplied",
            Self::ExitSubmitted(_) => "ExitSubmitted",
            Self::ExitFilled(_) => "ExitFilled",
            Self::PositionClosed(_) => "PositionClosed",
            Self::ManagementDecision(_) => "ManagementDecision",
            Self::ManagementOutcome(_) => "ManagementOutcome",
            Self::ExecutionStressChanged(_) => "ExecutionStressChanged",
            Self::OracleStale(_) => "OracleStale",
            Self::LedgerDegraded(_) => "LedgerDegraded",
        }
    }

    /// Whether this is an optional event (stress/oracle/ledger).
    pub fn is_optional(&self) -> bool {
        matches!(
            self,
            Self::PoolTransaction(_)
                | Self::ExecutionStressChanged(_)
                | Self::OracleStale(_)
                | Self::LedgerDegraded(_)
        )
    }
}

// ─── Payloads ───────────────────────────────────────────────────────────────

/// Durable birth/create event for selector dataset construction.
///
/// This is an ingest evidence event, not a Gatekeeper decision or lifecycle
/// label. It intentionally contains only identity/provenance available at
/// `NewPoolDetected` time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPoolDetectedPayload {
    /// Explicit marker consumed by offline selector universe builders.
    pub is_birth_event: bool,
    /// Pool AMM ID.
    pub pool_amm_id: String,
    /// Alias used by selector normalizers.
    pub pool_id: String,
    /// Base token mint.
    pub base_mint: String,
    /// Alias used by selector normalizers.
    pub mint_id: String,
    /// Quote token mint.
    pub quote_mint: String,
    /// Pump.fun bonding curve address.
    pub bonding_curve: String,
    /// Creator/deployer wallet address when available from ingest.
    pub creator: String,
    /// AMM program ID.
    pub amm_program: String,
    /// Source transaction signature for the create/birth event.
    pub signature: String,
    /// Selector birth timestamp in epoch milliseconds.
    pub birth_ts_ms: u64,
    /// Alias retained for historical artifact normalizers.
    pub timestamp_ms: u64,
    /// Chain slot when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_slot: Option<u64>,
    /// Local wall-clock timestamp of launcher detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_wall_ts_ms: Option<u64>,
    /// Effective chain/event timestamp if provided by ingest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_event_ts_ms: Option<u64>,
    /// Source label for offline provenance.
    pub source: String,
}

/// Durable transaction-flow evidence for selector feature snapshots.
///
/// This is not a Gatekeeper verdict, execution attempt, lifecycle event, or
/// denominator source.  Offline selector builders may join it to an existing
/// birth/candidate universe by mint + pool/bonding-curve identity and by
/// decision-time cutoffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolTransactionPayload {
    /// Payload schema marker for downstream artifact audits.
    pub schema_version: String,
    /// Pool AMM ID normalized for selector joins.
    pub pool_amm_id: String,
    /// Alias used by selector normalizers.
    pub pool_id: String,
    /// Source pool id before runtime remapping, if it differs from `pool_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pool_amm_id: Option<String>,
    /// Base token mint, when known at emit time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_mint: Option<String>,
    /// Alias used by selector normalizers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mint_id: Option<String>,
    /// Alias used by selector normalizers for historical event shapes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_mint: Option<String>,
    /// Quote token mint, if known. Pump.fun primary quote is wrapped SOL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_mint: Option<String>,
    /// Bonding curve / pool identity used for strict joins.
    pub bonding_curve: String,
    /// Source transaction signature.
    pub signature: String,
    /// Chain slot when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_slot: Option<u64>,
    /// Alias retained for downstream normalizers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    /// Transaction index in slot when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,
    /// Stable event ordinal inside the source transaction when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ordinal: Option<u32>,
    /// Parser-side outer instruction index when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_instruction_index: Option<u32>,
    /// Parser-side inner instruction group when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_group_index: Option<u32>,
    /// Selector event timestamp in epoch milliseconds.
    pub event_ts_ms: u64,
    /// Alias retained for historical normalizers.
    pub timestamp_ms: u64,
    /// Runtime arrival timestamp in epoch milliseconds.
    pub arrival_ts_ms: u64,
    /// Source label for offline provenance.
    pub source: String,
    /// `buy` or `sell`.
    pub side: String,
    /// Explicit boolean side for historical normalizers.
    pub is_buy: bool,
    /// True when the source transaction succeeded.
    #[serde(default = "default_pool_transaction_success")]
    pub success: bool,
    /// Parsed error code if the source transaction failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Primary signer / trader wallet observed by ingest.
    pub signer: String,
    /// Alias for feature rollups that use wallet identity.
    pub wallet: String,
    /// SOL-denominated trade amount used by flow features.
    pub quote_amount_sol: f64,
    /// Alias accepted by selector feature normalizers.
    pub volume_sol: f64,
    /// Canonical SOL amount in lamports when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sol_amount_lamports: Option<u64>,
    /// Canonical token amount in base units when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_amount_units: Option<u64>,
    /// Updated base reserve, if available from ingest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_base: Option<f64>,
    /// Updated quote/SOL reserve, if available from ingest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_quote: Option<f64>,
    /// Updated price, if available from ingest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_quote: Option<f64>,
    /// Virtual tokens remaining in bonding curve, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v_tokens_in_bonding_curve: Option<f64>,
    /// Virtual SOL remaining in bonding curve, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v_sol_in_bonding_curve: Option<f64>,
    /// Market cap in SOL, if available from ingest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap_sol: Option<f64>,
    /// Curve progress is not inferred here. It is populated only if an
    /// upstream source already supplied an authoritative value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_progress_pct: Option<f64>,
    /// Availability status for `curve_progress_pct`.
    pub curve_progress_status: String,
    /// Finality tier of the curve state used for this transaction.
    pub curve_finality: String,
    /// True when the parser had curve data, false for telemetry-only flow.
    pub curve_data_known: bool,
    /// Route/account contract status carried as evidence only.
    pub execution_account_contract_status: String,
    /// Optional reason when the route/account contract is incomplete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_account_contract_reason: Option<String>,
}

const fn default_pool_transaction_success() -> bool {
    true
}

/// 1. CandidateEvent — Gatekeeper PASS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePayload {
    /// Market-cap snapshot at detection time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcap_snapshot: Option<f64>,
    /// Price snapshot at detection time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_snapshot: Option<f64>,
    /// Gatekeeper verdict string (e.g. "PASS").
    pub gatekeeper_verdict: String,
    /// Flags from gatekeeper filters.
    pub gatekeeper_flags: Vec<String>,
    /// Detection source (e.g. "pump_portal", "grpc").
    pub source: String,
}

/// 2. EntrySubmittedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrySubmittedPayload {
    /// Side is always Entry for this event.
    pub side: OrderSide,
    /// Planned fill delay (paper only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_delay_ms: Option<u64>,
    /// Live send params (tip, slippage, etc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_params: Option<serde_json::Value>,
    /// Amount in lamports.
    pub amount_lamports: u64,
    /// Minimum tokens out.
    pub min_tokens_out: u64,
}

/// 3. EntryFilledEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryFilledPayload {
    /// Unix ms when the fill occurred.
    pub fill_time_ms: u64,
    /// Effective fill price.
    pub fill_price_effective: f64,
    /// Number of tokens received.
    pub fill_qty: u64,
    /// Quote used for fill resolution.
    pub quote_id_used: QuoteId,
    /// Fill status.
    pub status: FillStatus,
    /// submit → fill latency (ms).
    pub latency_ms: u64,
}

/// 4. PositionOpenedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionOpenedPayload {
    /// Entry price in SOL per token.
    pub entry_price: f64,
    /// Unix ms when position was opened.
    pub entry_time_ms: u64,
    /// Epoch counter.
    pub epoch_id: u64,
    /// Size in tokens.
    pub size_tokens: u64,
    /// Size in SOL (lamports).
    pub size_sol: u64,
}

/// 5. AemTickEvent (decision ticks only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AemTickPayload {
    /// Serialized regime key.
    pub regime_key: String,
    /// Serialized regime tag.
    pub regime_tag: String,
    /// Features summary blob.
    pub features_summary: serde_json::Value,
    /// AEM rollout mode string.
    pub rollout_mode: String,
    /// Hard safety state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_safety_state: Option<String>,
    /// Current drawdown percent.
    pub drawdown_pct: f64,
    /// Current unrealized PnL percent.
    pub unrealized_pnl_pct: f64,
}

/// 6. ControlCommandIssuedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommandIssuedPayload {
    /// Directive: WAIT, PARTIAL, PANIC, SELL_NOW, NOOP
    pub directive: String,
    /// Fraction in bps (for PARTIAL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction_bps: Option<u16>,
    /// Freeze until timestamp (for WAIT).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeze_until_ms: Option<u64>,
    /// When the command was issued.
    pub issued_at_ms: u64,
    /// Start of validity window.
    pub valid_from_ms: u64,
    /// End of validity window.
    pub expires_at_ms: u64,
    /// Epoch this command belongs to.
    pub epoch: u64,
    /// Priority: Default, AemPolicy, HardSafety
    pub priority: String,
    /// Human-readable reason code.
    pub reason_code: String,
}

/// 7. ControlCommandAppliedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommandAppliedPayload {
    /// Whether the command was accepted.
    pub accepted: bool,
    /// If rejected, the reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<String>,
    /// When the command was applied.
    pub applied_at_ms: u64,
}

/// 8. ExitSubmittedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitSubmittedPayload {
    /// Fraction of position to exit (bps).
    pub fraction_bps: u16,
    /// Reference to the AEM command that triggered this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_ref: Option<CommandId>,
}

/// 9. ExitFilledEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitFilledPayload {
    /// Fill price.
    pub fill_price: f64,
    /// Tokens sold.
    pub fill_qty: u64,
    /// Realized PnL delta for this exit.
    pub realized_pnl_delta: f64,
    /// Fill status.
    pub status: FillStatus,
    /// Was this a partial exit?
    pub is_partial: bool,
    /// Remaining quantity after exit.
    pub remaining_qty: u64,
}

/// 10. PositionClosedEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionClosedPayload {
    /// Final PnL (SOL).
    pub final_pnl: f64,
    /// Final PnL as percentage.
    pub final_pnl_pct: f64,
    /// Entry notional for the closed position (SOL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_value_sol: Option<f64>,
    /// Exit notional for the closed position (SOL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_value_sol: Option<f64>,
    /// Gross PnL before any paper/live execution costs (SOL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gross_pnl_sol: Option<f64>,
    /// Net PnL after explicit or placeholder execution costs (SOL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_pnl_sol: Option<f64>,
    /// Total explicit or placeholder execution costs used for net PnL (SOL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_costs_sol: Option<f64>,
    /// Position duration in ms.
    pub duration_ms: u64,
    /// Reason for closure.
    pub reason: CloseReason,
    /// Total number of exit events.
    pub total_exits: u32,
}

/// Why a position was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseReason {
    Panic,
    StopLoss,
    TimeStop,
    Target,
    Manual,
    Default,
    HardSafety,
}

impl Default for CloseReason {
    fn default() -> Self {
        Self::Default
    }
}

/// 11. ManagementDecisionEvent (reuses existing AEM types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementDecisionPayload {
    /// Serialized management decision (from aem::types).
    pub decision: serde_json::Value,
    /// Reference to the counterfactual quote used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterfactual_basis_quote_id: Option<QuoteId>,
}

/// 12. ManagementOutcomeEvent (reuses existing AEM types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementOutcomePayload {
    /// Serialized management outcome (from aem::types).
    pub outcome: serde_json::Value,
}

// ─── Optional payloads ─────────────────────────────────────────────────────

/// ExecutionStressChanged — emitted when stress bucket transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStressChangedPayload {
    pub previous_bucket: StressBucket,
    pub new_bucket: StressBucket,
    pub snapshot: ExecutionStressSnapshot,
}

/// OracleStale — emitted when quote staleness exceeds threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleStalePayload {
    pub stale_age_ms: u64,
    pub threshold_ms: u64,
}

/// LedgerDegraded — emitted on ShadowLedger data quality issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerDegradedPayload {
    pub reason: String,
    pub conservative_mode: bool,
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::backend::Lane;

    fn make_envelope() -> EventEnvelope {
        EventEnvelope::new(
            "run-123".to_string(),
            Lane::Paper,
            "mint_pool_slot".to_string(),
            1700000000000,
        )
    }

    #[test]
    fn test_event_envelope_required_fields() {
        let env = make_envelope();
        assert_eq!(env.run_id, "run-123");
        assert_eq!(env.lane, Lane::Paper);
        assert!(!env.event_id.is_empty());
        assert_eq!(env.event_time_ms, 1700000000000);
    }

    #[test]
    fn test_event_envelope_derive() {
        let env = make_envelope();
        let derived = env.derive(1700000001000);
        assert_eq!(derived.run_id, env.run_id);
        assert_eq!(derived.lane, env.lane);
        assert_eq!(derived.candidate_id, env.candidate_id);
        assert_ne!(derived.event_id, env.event_id); // fresh ID
        assert_eq!(derived.event_time_ms, 1700000001000);
    }

    #[test]
    fn test_execution_event_serialization() {
        let env = make_envelope();
        let event = ExecutionEvent::new(
            env,
            EventKind::Candidate(CandidatePayload {
                mcap_snapshot: Some(50000.0),
                price_snapshot: Some(0.001),
                gatekeeper_verdict: "PASS".to_string(),
                gatekeeper_flags: vec!["liquidity_ok".to_string()],
                source: "pump_portal".to_string(),
            }),
        );

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"Candidate\""));
        assert!(json.contains("\"gatekeeper_verdict\":\"PASS\""));
        assert!(json.contains("\"pump_portal\""));
    }

    #[test]
    fn test_event_envelope_shadow_lane_round_trip() {
        let env = EventEnvelope::new(
            "run-shadow".to_string(),
            Lane::Shadow,
            "cand-shadow".to_string(),
            1700000002000,
        );

        let json = serde_json::to_string(&env).expect("serialize");
        assert!(json.contains("\"lane\":\"shadow\""));

        let parsed: EventEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.lane, Lane::Shadow);
    }

    #[test]
    fn test_event_kind_type_name() {
        let kind = EventKind::EntrySubmitted(EntrySubmittedPayload {
            side: OrderSide::Entry,
            planned_delay_ms: Some(300),
            send_params: None,
            amount_lamports: 10_000_000,
            min_tokens_out: 1000,
        });
        assert_eq!(kind.type_name(), "EntrySubmitted");
        assert!(!kind.is_optional());

        let birth_kind = EventKind::NewPoolDetected(NewPoolDetectedPayload {
            is_birth_event: true,
            pool_amm_id: "pool".to_string(),
            pool_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            mint_id: "mint".to_string(),
            quote_mint: "SOL".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            amm_program: "pumpfun".to_string(),
            signature: "sig".to_string(),
            birth_ts_ms: 1_700_000_000_000,
            timestamp_ms: 1_700_000_000_000,
            event_slot: Some(123),
            detected_wall_ts_ms: Some(1_700_000_000_100),
            chain_event_ts_ms: Some(1_700_000_000_000),
            source: "seer_new_pool_detected".to_string(),
        });
        assert_eq!(birth_kind.type_name(), "NewPoolDetected");
        assert!(!birth_kind.is_optional());

        let tx_kind = EventKind::PoolTransaction(PoolTransactionPayload {
            schema_version: "v1".to_string(),
            pool_amm_id: "pool".to_string(),
            pool_id: "pool".to_string(),
            source_pool_amm_id: None,
            base_mint: Some("mint".to_string()),
            mint_id: Some("mint".to_string()),
            token_mint: Some("mint".to_string()),
            quote_mint: Some("SOL".to_string()),
            bonding_curve: "pool".to_string(),
            signature: "sig-tx".to_string(),
            event_slot: Some(124),
            slot: Some(124),
            tx_index: Some(1),
            event_ordinal: Some(2),
            outer_instruction_index: Some(3),
            inner_group_index: Some(4),
            event_ts_ms: 1_700_000_001_000,
            timestamp_ms: 1_700_000_001_000,
            arrival_ts_ms: 1_700_000_001_001,
            source: "grpc_global_stream".to_string(),
            side: "buy".to_string(),
            is_buy: true,
            success: true,
            error_code: None,
            signer: "wallet".to_string(),
            wallet: "wallet".to_string(),
            quote_amount_sol: 0.42,
            volume_sol: 0.42,
            sol_amount_lamports: Some(420_000_000),
            token_amount_units: Some(123),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            curve_progress_pct: None,
            curve_progress_status: "unavailable_missing_curve_state_source".to_string(),
            curve_finality: "speculative".to_string(),
            curve_data_known: false,
            execution_account_contract_status: "route_account_manifest_incomplete".to_string(),
            execution_account_contract_reason: Some(
                "route_account_manifest_incomplete:missing_global_config".to_string(),
            ),
        });
        assert_eq!(tx_kind.type_name(), "PoolTransaction");
        assert!(tx_kind.is_optional());
    }

    #[test]
    fn test_optional_event_flag() {
        let kind = EventKind::OracleStale(OracleStalePayload {
            stale_age_ms: 2000,
            threshold_ms: 1500,
        });
        assert!(kind.is_optional());
        assert_eq!(kind.type_name(), "OracleStale");
    }

    #[test]
    fn test_full_timeline_serializable() {
        // Verify that all event kinds can be serialized to JSON
        let env = make_envelope();
        let events = vec![
            ExecutionEvent::new(
                env.derive(0),
                EventKind::NewPoolDetected(NewPoolDetectedPayload {
                    is_birth_event: true,
                    pool_amm_id: "pool".to_string(),
                    pool_id: "pool".to_string(),
                    base_mint: "mint".to_string(),
                    mint_id: "mint".to_string(),
                    quote_mint: "SOL".to_string(),
                    bonding_curve: "curve".to_string(),
                    creator: "creator".to_string(),
                    amm_program: "pumpfun".to_string(),
                    signature: "sig".to_string(),
                    birth_ts_ms: 1,
                    timestamp_ms: 1,
                    event_slot: Some(1),
                    detected_wall_ts_ms: Some(2),
                    chain_event_ts_ms: Some(1),
                    source: "seer_new_pool_detected".to_string(),
                }),
            ),
            ExecutionEvent::new(
                env.derive(1),
                EventKind::PoolTransaction(PoolTransactionPayload {
                    schema_version: "v1".to_string(),
                    pool_amm_id: "pool".to_string(),
                    pool_id: "pool".to_string(),
                    source_pool_amm_id: None,
                    base_mint: Some("mint".to_string()),
                    mint_id: Some("mint".to_string()),
                    token_mint: Some("mint".to_string()),
                    quote_mint: Some("SOL".to_string()),
                    bonding_curve: "pool".to_string(),
                    signature: "sig-tx".to_string(),
                    event_slot: Some(1),
                    slot: Some(1),
                    tx_index: Some(1),
                    event_ordinal: Some(1),
                    outer_instruction_index: None,
                    inner_group_index: None,
                    event_ts_ms: 1,
                    timestamp_ms: 1,
                    arrival_ts_ms: 1,
                    source: "grpc_global_stream".to_string(),
                    side: "buy".to_string(),
                    is_buy: true,
                    success: true,
                    error_code: None,
                    signer: "wallet".to_string(),
                    wallet: "wallet".to_string(),
                    quote_amount_sol: 1.0,
                    volume_sol: 1.0,
                    sol_amount_lamports: Some(1_000_000_000),
                    token_amount_units: Some(100),
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    v_tokens_in_bonding_curve: None,
                    v_sol_in_bonding_curve: None,
                    market_cap_sol: None,
                    curve_progress_pct: None,
                    curve_progress_status: "unavailable_missing_curve_state_source".to_string(),
                    curve_finality: "speculative".to_string(),
                    curve_data_known: false,
                    execution_account_contract_status: "route_account_manifest_incomplete"
                        .to_string(),
                    execution_account_contract_reason: Some(
                        "route_account_manifest_incomplete:missing_global_config".to_string(),
                    ),
                }),
            ),
            ExecutionEvent::new(
                env.derive(2),
                EventKind::Candidate(CandidatePayload {
                    mcap_snapshot: None,
                    price_snapshot: None,
                    gatekeeper_verdict: "PASS".to_string(),
                    gatekeeper_flags: vec![],
                    source: "grpc".to_string(),
                }),
            ),
            ExecutionEvent::new(
                env.derive(3),
                EventKind::EntrySubmitted(EntrySubmittedPayload {
                    side: OrderSide::Entry,
                    planned_delay_ms: None,
                    send_params: None,
                    amount_lamports: 1000,
                    min_tokens_out: 10,
                }),
            ),
            ExecutionEvent::new(
                env.derive(4),
                EventKind::EntryFilled(EntryFilledPayload {
                    fill_time_ms: 3,
                    fill_price_effective: 0.001,
                    fill_qty: 1000,
                    quote_id_used: "q-1".to_string(),
                    status: FillStatus::Filled,
                    latency_ms: 300,
                }),
            ),
            ExecutionEvent::new(
                env.derive(5),
                EventKind::PositionOpened(PositionOpenedPayload {
                    entry_price: 0.001,
                    entry_time_ms: 4,
                    epoch_id: 1,
                    size_tokens: 1000,
                    size_sol: 1000000,
                }),
            ),
            ExecutionEvent::new(
                env.derive(6),
                EventKind::AemTick(AemTickPayload {
                    regime_key: "key".to_string(),
                    regime_tag: "tag".to_string(),
                    features_summary: serde_json::json!({}),
                    rollout_mode: "Shadow".to_string(),
                    hard_safety_state: None,
                    drawdown_pct: 0.0,
                    unrealized_pnl_pct: 0.0,
                }),
            ),
            ExecutionEvent::new(
                env.derive(7),
                EventKind::ControlCommandIssued(ControlCommandIssuedPayload {
                    directive: "WAIT".to_string(),
                    fraction_bps: None,
                    freeze_until_ms: Some(100),
                    issued_at_ms: 6,
                    valid_from_ms: 6,
                    expires_at_ms: 1006,
                    epoch: 1,
                    priority: "AemPolicy".to_string(),
                    reason_code: "test".to_string(),
                }),
            ),
            ExecutionEvent::new(
                env.derive(8),
                EventKind::ControlCommandApplied(ControlCommandAppliedPayload {
                    accepted: true,
                    reject_reason: None,
                    applied_at_ms: 7,
                }),
            ),
            ExecutionEvent::new(
                env.derive(9),
                EventKind::ExitSubmitted(ExitSubmittedPayload {
                    fraction_bps: 10000,
                    command_ref: None,
                }),
            ),
            ExecutionEvent::new(
                env.derive(10),
                EventKind::ExitFilled(ExitFilledPayload {
                    fill_price: 0.002,
                    fill_qty: 1000,
                    realized_pnl_delta: 0.001,
                    status: FillStatus::Filled,
                    is_partial: false,
                    remaining_qty: 0,
                }),
            ),
            ExecutionEvent::new(
                env.derive(11),
                EventKind::PositionClosed(PositionClosedPayload {
                    final_pnl: 0.001,
                    final_pnl_pct: 100.0,
                    entry_value_sol: Some(0.001),
                    exit_value_sol: Some(0.002),
                    gross_pnl_sol: Some(0.001),
                    net_pnl_sol: Some(0.001),
                    estimated_costs_sol: Some(0.0),
                    duration_ms: 5000,
                    reason: CloseReason::Target,
                    total_exits: 1,
                }),
            ),
            ExecutionEvent::new(
                env.derive(12),
                EventKind::ManagementDecision(ManagementDecisionPayload {
                    decision: serde_json::json!({"action": "SELL_NOW"}),
                    counterfactual_basis_quote_id: None,
                }),
            ),
            ExecutionEvent::new(
                env.derive(13),
                EventKind::ManagementOutcome(ManagementOutcomePayload {
                    outcome: serde_json::json!({"result": "success"}),
                }),
            ),
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.is_empty());
            // Verify roundtrip
            let _: ExecutionEvent = serde_json::from_str(&json).unwrap();
        }
        assert_eq!(events.len(), 14);
    }

    #[test]
    fn test_close_reason_serialization() {
        let reasons = vec![
            CloseReason::Panic,
            CloseReason::StopLoss,
            CloseReason::TimeStop,
            CloseReason::Target,
            CloseReason::Manual,
            CloseReason::Default,
            CloseReason::HardSafety,
        ];
        for reason in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            let back: CloseReason = serde_json::from_str(&json).unwrap();
            assert_eq!(back, reason);
        }
    }
}
