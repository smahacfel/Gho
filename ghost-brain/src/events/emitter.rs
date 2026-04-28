//! EventEmitter — convenience wrapper for emitting typed execution events.
//!
//! Backends and pipeline components use `EventEmitter` to emit events
//! without needing to construct `EventEnvelope` + `EventKind` manually.
//!
//! The emitter holds `run_id`, `lane`, and an `EventWriter`, and provides
//! typed methods like `emit_stress_changed()`, `emit_oracle_stale()`, etc.

use std::sync::{Arc, Mutex};
use tracing::{debug, error, warn};

use crate::execution::backend::{
    CandidateId, CommandId, ExecutionStressSnapshot, FillStatus, Lane, OrderId, OrderSide,
    PositionId, QuoteId, StressBucket,
};

use super::schema::*;
use super::writer::{EventWriter, EventWriterConfig};

// ─── EventEmitter ───────────────────────────────────────────────────────────

/// High-level event emitter that wraps `EventWriter` with context.
pub struct EventEmitter {
    writer: Arc<Mutex<EventWriter>>,
    run_id: String,
    lane: Lane,
}

impl EventEmitter {
    /// Create a new EventEmitter. Initializes the underlying EventWriter.
    pub fn new(config: EventWriterConfig, run_id: String, lane: Lane) -> std::io::Result<Self> {
        let writer = EventWriter::new(config, run_id.clone())?;
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            run_id,
            lane,
        })
    }

    /// Create an EventEmitter that wraps an existing EventWriter (for shared use in DualBackend).
    pub fn with_shared_writer(writer: Arc<Mutex<EventWriter>>, run_id: String, lane: Lane) -> Self {
        Self {
            writer,
            run_id,
            lane,
        }
    }

    /// Get the run_id.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Get the lane.
    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// Get a clone of the shared writer Arc (for DualBackend sharing).
    pub fn shared_writer(&self) -> Arc<Mutex<EventWriter>> {
        Arc::clone(&self.writer)
    }

    /// Flush all buffered events to disk.
    pub fn flush(&self) -> std::io::Result<()> {
        self.writer.lock().unwrap().flush()
    }

    /// Total events written.
    pub fn total_events_written(&self) -> u64 {
        self.writer.lock().unwrap().total_events_written()
    }

    /// Current unix timestamp in milliseconds.
    pub fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Envelope helper ─────────────────────────────────────────────────

    fn make_envelope(&self, candidate_id: &CandidateId) -> EventEnvelope {
        let now_ms = Self::now_ms();
        EventEnvelope::new(self.run_id.clone(), self.lane, candidate_id.clone(), now_ms)
    }

    /// Create a new envelope with explicit event time.
    pub fn make_envelope_at(
        &self,
        candidate_id: &CandidateId,
        event_time_ms: u64,
    ) -> EventEnvelope {
        EventEnvelope::new(
            self.run_id.clone(),
            self.lane,
            candidate_id.clone(),
            event_time_ms,
        )
    }

    fn emit(&self, event: ExecutionEvent) {
        if let Ok(mut w) = self.writer.lock() {
            if let Err(e) = w.write_event(&event) {
                error!(error = %e, kind = event.kind.type_name(), "EventEmitter: failed to write event");
            }
        }
    }

    /// Emit a fully-constructed event (used by pipeline instrumentation hooks).
    pub fn emit_raw(&self, event: ExecutionEvent) {
        self.emit(event);
    }

    // ── Typed emitters ──────────────────────────────────────────────────

    /// Emit a CandidateEvent (Gatekeeper PASS).
    pub fn emit_candidate(
        &self,
        candidate_id: &CandidateId,
        mcap_snapshot: Option<f64>,
        price_snapshot: Option<f64>,
        verdict: &str,
        flags: Vec<String>,
        source: &str,
    ) {
        let env = self.make_envelope(candidate_id);
        self.emit(ExecutionEvent::new(
            env,
            EventKind::Candidate(CandidatePayload {
                mcap_snapshot,
                price_snapshot,
                gatekeeper_verdict: verdict.to_string(),
                gatekeeper_flags: flags,
                source: source.to_string(),
            }),
        ));
    }

    /// Emit an EntrySubmittedEvent.
    pub fn emit_entry_submitted(
        &self,
        candidate_id: &CandidateId,
        order_id: &OrderId,
        amount_lamports: u64,
        min_tokens_out: u64,
        planned_delay_ms: Option<u64>,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.order_id = Some(order_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::EntrySubmitted(EntrySubmittedPayload {
                side: OrderSide::Entry,
                planned_delay_ms,
                send_params: None,
                amount_lamports,
                min_tokens_out,
            }),
        ));
    }

    /// Emit an EntryFilledEvent.
    pub fn emit_entry_filled(
        &self,
        candidate_id: &CandidateId,
        order_id: &OrderId,
        fill_time_ms: u64,
        fill_price: f64,
        fill_qty: u64,
        quote_id_used: &QuoteId,
        status: FillStatus,
        latency_ms: u64,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.order_id = Some(order_id.clone());
        env.quote_id = Some(quote_id_used.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::EntryFilled(EntryFilledPayload {
                fill_time_ms,
                fill_price_effective: fill_price,
                fill_qty,
                quote_id_used: quote_id_used.clone(),
                status,
                latency_ms,
            }),
        ));
    }

    /// Emit a PositionOpenedEvent.
    pub fn emit_position_opened(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        entry_price: f64,
        entry_time_ms: u64,
        epoch_id: u64,
        size_tokens: u64,
        size_sol: u64,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        env.position_epoch = Some(epoch_id);
        self.emit(ExecutionEvent::new(
            env,
            EventKind::PositionOpened(PositionOpenedPayload {
                entry_price,
                entry_time_ms,
                epoch_id,
                size_tokens,
                size_sol,
            }),
        ));
    }

    /// Emit a PositionClosedEvent.
    pub fn emit_position_closed(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        final_pnl: f64,
        final_pnl_pct: f64,
        entry_value_sol: Option<f64>,
        exit_value_sol: Option<f64>,
        gross_pnl_sol: Option<f64>,
        net_pnl_sol: Option<f64>,
        estimated_costs_sol: Option<f64>,
        duration_ms: u64,
        reason: CloseReason,
        total_exits: u32,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::PositionClosed(PositionClosedPayload {
                final_pnl,
                final_pnl_pct,
                entry_value_sol,
                exit_value_sol,
                gross_pnl_sol,
                net_pnl_sol,
                estimated_costs_sol,
                duration_ms,
                reason,
                total_exits,
            }),
        ));
    }

    /// Emit an ExitSubmittedEvent.
    pub fn emit_exit_submitted(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        order_id: &OrderId,
        fraction_bps: u16,
        command_ref: Option<CommandId>,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        env.order_id = Some(order_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ExitSubmitted(ExitSubmittedPayload {
                fraction_bps,
                command_ref,
            }),
        ));
    }

    /// Emit an ExitFilledEvent.
    pub fn emit_exit_filled(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        order_id: &OrderId,
        fill_price: f64,
        fill_qty: u64,
        realized_pnl_delta: f64,
        status: FillStatus,
        is_partial: bool,
        remaining_qty: u64,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        env.order_id = Some(order_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ExitFilled(ExitFilledPayload {
                fill_price,
                fill_qty,
                realized_pnl_delta,
                status,
                is_partial,
                remaining_qty,
            }),
        ));
    }

    /// Emit an ExecutionStressChangedEvent (on bucket transition).
    pub fn emit_stress_changed(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        previous_bucket: StressBucket,
        new_bucket: StressBucket,
        snapshot: &ExecutionStressSnapshot,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        debug!(
            position_id = %position_id,
            from = ?previous_bucket,
            to = ?new_bucket,
            "EventEmitter: stress bucket transition"
        );
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ExecutionStressChanged(ExecutionStressChangedPayload {
                previous_bucket,
                new_bucket,
                snapshot: snapshot.clone(),
            }),
        ));
    }

    /// Emit an OracleStaleEvent when oracle data is overdue.
    pub fn emit_oracle_stale(
        &self,
        candidate_id: &CandidateId,
        stale_age_ms: u64,
        threshold_ms: u64,
    ) {
        let env = self.make_envelope(candidate_id);
        warn!(
            stale_age_ms = stale_age_ms,
            threshold_ms = threshold_ms,
            "EventEmitter: oracle stale detected"
        );
        self.emit(ExecutionEvent::new(
            env,
            EventKind::OracleStale(OracleStalePayload {
                stale_age_ms,
                threshold_ms,
            }),
        ));
    }

    /// Emit a LedgerDegradedEvent.
    pub fn emit_ledger_degraded(
        &self,
        candidate_id: &CandidateId,
        reason: &str,
        conservative_mode: bool,
    ) {
        let env = self.make_envelope(candidate_id);
        self.emit(ExecutionEvent::new(
            env,
            EventKind::LedgerDegraded(LedgerDegradedPayload {
                reason: reason.to_string(),
                conservative_mode,
            }),
        ));
    }

    /// Emit an AemTickEvent.
    pub fn emit_aem_tick(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        regime_key: &str,
        regime_tag: &str,
        features_summary: serde_json::Value,
        rollout_mode: &str,
        hard_safety_state: Option<String>,
        drawdown_pct: f64,
        unrealized_pnl_pct: f64,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::AemTick(AemTickPayload {
                regime_key: regime_key.to_string(),
                regime_tag: regime_tag.to_string(),
                features_summary,
                rollout_mode: rollout_mode.to_string(),
                hard_safety_state,
                drawdown_pct,
                unrealized_pnl_pct,
            }),
        ));
    }

    /// Emit a ControlCommandIssuedEvent.
    pub fn emit_control_command_issued(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        command_id: &CommandId,
        directive: &str,
        fraction_bps: Option<u16>,
        freeze_until_ms: Option<u64>,
        valid_from_ms: u64,
        expires_at_ms: u64,
        epoch: u64,
        priority: &str,
        reason_code: &str,
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        env.command_id = Some(command_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ControlCommandIssued(ControlCommandIssuedPayload {
                directive: directive.to_string(),
                fraction_bps,
                freeze_until_ms,
                issued_at_ms: now_ms,
                valid_from_ms,
                expires_at_ms,
                epoch,
                priority: priority.to_string(),
                reason_code: reason_code.to_string(),
            }),
        ));
    }

    /// Emit a ControlCommandAppliedEvent.
    pub fn emit_control_command_applied(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        command_id: &CommandId,
        accepted: bool,
        reject_reason: Option<String>,
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        env.command_id = Some(command_id.clone());
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ControlCommandApplied(ControlCommandAppliedPayload {
                accepted,
                reject_reason,
                applied_at_ms: now_ms,
            }),
        ));
    }

    /// Emit a ManagementDecisionEvent.
    pub fn emit_management_decision(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        decision: serde_json::Value,
        counterfactual_basis_quote_id: Option<QuoteId>,
        command_id: Option<CommandId>,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        if let Some(cid) = command_id {
            env.command_id = Some(cid);
        }
        if let Some(ref qid) = counterfactual_basis_quote_id {
            env.quote_id = Some(qid.clone());
        }
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ManagementDecision(ManagementDecisionPayload {
                decision,
                counterfactual_basis_quote_id,
            }),
        ));
    }

    /// Emit a ManagementOutcomeEvent.
    pub fn emit_management_outcome(
        &self,
        candidate_id: &CandidateId,
        position_id: &PositionId,
        outcome: serde_json::Value,
        command_id: Option<CommandId>,
    ) {
        let mut env = self.make_envelope(candidate_id);
        env.position_id = Some(position_id.clone());
        if let Some(cid) = command_id {
            env.command_id = Some(cid);
        }
        self.emit(ExecutionEvent::new(
            env,
            EventKind::ManagementOutcome(ManagementOutcomePayload { outcome }),
        ));
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_emitter() -> (EventEmitter, TempDir) {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            enable_optional_events: true,
            ..Default::default()
        };
        let emitter = EventEmitter::new(config, "test-run".to_string(), Lane::Paper).unwrap();
        (emitter, tmp)
    }

    #[test]
    fn test_emit_candidate() {
        let (emitter, _tmp) = make_emitter();
        emitter.emit_candidate(
            &"cand-1".to_string(),
            Some(50000.0),
            Some(0.001),
            "PASS",
            vec!["ok".to_string()],
            "grpc",
        );
        emitter.flush().unwrap();
        assert_eq!(emitter.total_events_written(), 1);
    }

    #[test]
    fn test_emit_stress_changed() {
        let (emitter, _tmp) = make_emitter();
        emitter.emit_stress_changed(
            &"cand-2".to_string(),
            &"pos-1".to_string(),
            StressBucket::Low,
            StressBucket::High,
            &ExecutionStressSnapshot::default(),
        );
        emitter.flush().unwrap();
        assert_eq!(emitter.total_events_written(), 1);
    }

    #[test]
    fn test_emit_oracle_stale() {
        let (emitter, _tmp) = make_emitter();
        emitter.emit_oracle_stale(&"cand-3".to_string(), 2500, 1500);
        emitter.flush().unwrap();
        assert_eq!(emitter.total_events_written(), 1);
    }

    #[test]
    fn test_emit_full_lifecycle() {
        let (emitter, _tmp) = make_emitter();
        let cid = "lifecycle-cand".to_string();
        let pid = "pos-lc".to_string();
        let oid = "ord-lc".to_string();
        let qid = "q-lc".to_string();

        emitter.emit_candidate(&cid, None, None, "PASS", vec![], "test");
        emitter.emit_entry_submitted(&cid, &oid, 10_000_000, 1000, Some(300));
        emitter.emit_entry_filled(&cid, &oid, 1000, 0.001, 1000, &qid, FillStatus::Filled, 300);
        emitter.emit_position_opened(&cid, &pid, 0.001, 1000, 1, 1000, 10_000_000);
        emitter.emit_exit_submitted(&cid, &pid, &oid, 10000, None);
        emitter.emit_exit_filled(
            &cid,
            &pid,
            &oid,
            0.002,
            1000,
            0.001,
            FillStatus::Filled,
            false,
            0,
        );
        emitter.emit_position_closed(
            &cid,
            &pid,
            0.001,
            100.0,
            Some(0.001),
            Some(0.002),
            Some(0.001),
            Some(0.001),
            Some(0.0),
            5000,
            CloseReason::Target,
            1,
        );

        emitter.flush().unwrap();
        assert_eq!(emitter.total_events_written(), 7);
    }

    #[test]
    fn test_shared_writer() {
        let (emitter1, _tmp) = make_emitter();
        let shared = emitter1.shared_writer();
        let emitter2 = EventEmitter::with_shared_writer(shared, "test-run".to_string(), Lane::Live);

        emitter1.emit_candidate(&"cand-a".to_string(), None, None, "PASS", vec![], "test");
        emitter2.emit_candidate(&"cand-b".to_string(), None, None, "PASS", vec![], "test");

        emitter1.flush().unwrap();
        // Both emitters share the same writer, so total = 2
        assert_eq!(emitter1.total_events_written(), 2);
        assert_eq!(emitter2.total_events_written(), 2);
    }

    #[test]
    fn test_emit_candidate_on_shadow_lane() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            enable_optional_events: true,
            ..Default::default()
        };
        let emitter = EventEmitter::new(config, "shadow-run".to_string(), Lane::Shadow).unwrap();

        emitter.emit_candidate(
            &"cand-shadow".to_string(),
            None,
            None,
            "PASS",
            vec![],
            "test",
        );
        emitter.flush().unwrap();
        assert_eq!(emitter.total_events_written(), 1);
    }
}
