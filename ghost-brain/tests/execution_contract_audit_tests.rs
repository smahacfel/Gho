use std::fs;
use std::io::Write;
use std::path::PathBuf;

use ghost_brain::events::{
    AemTickPayload, CandidatePayload, CloseReason, ControlCommandAppliedPayload,
    ControlCommandIssuedPayload, EntryFilledPayload, EntrySubmittedPayload, EventEnvelope,
    EventKind, EventValidator, ExecutionEvent, ExitFilledPayload, ExitSubmittedPayload,
    ManagementDecisionPayload, ManagementOutcomePayload, PositionClosedPayload,
    PositionOpenedPayload,
};
use ghost_brain::execution::{FillStatus, Lane, OrderSide};
use tempfile::NamedTempFile;

#[test]
fn test_pipeline_has_no_dry_run_branching_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let execution_src = fs::read_to_string(root.join("src/pipeline/execution.rs"))
        .expect("read pipeline/execution.rs");
    let jito_src = fs::read_to_string(root.join("src/pipeline/jito_processor.rs"))
        .expect("read pipeline/jito_processor.rs");

    assert!(
        execution_src.contains("match execution_mode"),
        "startup dispatch should use explicit match execution_mode"
    );
    assert!(
        execution_src.contains("ExecutionMode::Shadow"),
        "execution pipeline must keep a dedicated first-class shadow branch"
    );
    assert!(
        !execution_src.contains("if matches!(execution_mode, ExecutionMode::Paper)"),
        "paper startup branch should be handled in execution_mode match"
    );
    assert!(
        !execution_src.contains("dry_run"),
        "execution pipeline should not branch on dry_run"
    );
    assert!(
        !jito_src.contains("dry_run"),
        "jito processor should not branch on dry_run"
    );
}

#[test]
fn test_prepared_entry_contract_replaces_pending_generation_placeholder() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let execution_src = fs::read_to_string(root.join("src/pipeline/execution.rs"))
        .expect("read pipeline/execution.rs");
    let jito_src = fs::read_to_string(root.join("src/pipeline/jito_processor.rs"))
        .expect("read pipeline/jito_processor.rs");

    assert!(
        !execution_src.contains("pending-generation"),
        "execution pipeline must not emit placeholder order ids"
    );
    assert!(
        !jito_src.contains("pending-generation"),
        "legacy jito helper must not emit placeholder order ids"
    );
    assert!(
        execution_src.contains("submit_prepared_entry("),
        "live/paper execution path should submit the frozen prepared-entry contract"
    );
    assert!(
        jito_src.contains("submit_prepared_entry("),
        "jito helper should submit the frozen prepared-entry contract"
    );
}

#[test]
fn test_live_worker_has_no_second_quote_truth_path() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let live_src =
        fs::read_to_string(root.join("src/execution/live.rs")).expect("read execution/live.rs");
    let jito_src = fs::read_to_string(root.join("src/pipeline/jito_processor.rs"))
        .expect("read pipeline/jito_processor.rs");

    assert!(
        !live_src.contains("resolve_quote_ref_with_provider"),
        "live worker must not re-resolve quote/timing after pipeline prep"
    );
    assert!(
        live_src.contains("pub attempt: ExecutionAttemptContext"),
        "live worker requests should carry the shared execution attempt context"
    );
    assert!(
        live_src.contains("let predicted_slot = req")
            && live_src.contains(".timing")
            && live_src.contains(".predicted_slot"),
        "live jito worker should consume prepared timing metadata instead of inventing local slots"
    );
    assert!(
        !live_src.contains("12345"),
        "live backend must not contain placeholder predicted slots"
    );
    assert!(
        !jito_src.contains("12345"),
        "jito processor must not contain placeholder predicted slots"
    );
}

#[test]
fn test_prepared_entry_contract_freezes_identity_and_timing_fields() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let backend_src = fs::read_to_string(root.join("src/execution/backend.rs"))
        .expect("read execution/backend.rs");

    assert!(
        backend_src.contains("pub struct PreparedQuoteRef"),
        "prepared quote contract should be defined explicitly"
    );
    assert!(
        backend_src.contains("pub struct PreparedEntryExecution"),
        "prepared entry contract should be defined explicitly"
    );
    for required_field in [
        "pub order_id: OrderId",
        "pub candidate: CandidateRef",
        "pub submit_time_ms: u64",
        "pub position_epoch: u64",
        "pub quote: PreparedQuoteRef",
        "pub timing_source: EntryTimingSource",
        "pub predicted_slot: Option<u64>",
        "pub quote_ts_ms: u64",
        "pub slot: Option<u64>",
        "pub quote_price_ref: Option<f64>",
        "pub price_source: EntryPriceSource",
        "pub is_stale: bool",
        "pub stale_age_ms: u64",
        "pub stale_policy: EntryStalePolicy",
    ] {
        assert!(
            backend_src.contains(required_field),
            "prepared-entry contract missing required field: {required_field}"
        );
    }
}

#[test]
fn test_timeline_includes_aem_decision_and_outcome_without_validator_violations() {
    let run_id = "run-contract".to_string();
    let lane = Lane::Paper;
    let candidate_id = "cand-1".to_string();
    let position_id = "pos-1".to_string();
    let entry_order_id = "entry-1".to_string();
    let exit_order_id = "exit-1".to_string();
    let quote_id = "1_1000_1".to_string();
    let command_id = "cmd-1".to_string();

    let mut events: Vec<ExecutionEvent> = Vec::new();

    events.push(ExecutionEvent::new(
        EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1000),
        EventKind::Candidate(CandidatePayload {
            mcap_snapshot: None,
            price_snapshot: Some(1.0),
            gatekeeper_verdict: "PASS".to_string(),
            gatekeeper_flags: vec!["position_limit_ok".to_string()],
            source: "test".to_string(),
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1010);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::EntrySubmitted(EntrySubmittedPayload {
            side: OrderSide::Entry,
            planned_delay_ms: None,
            send_params: None,
            amount_lamports: 1_000_000,
            min_tokens_out: 1_000,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1020);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::EntryFilled(EntryFilledPayload {
            fill_time_ms: 1020,
            fill_price_effective: 1.0,
            fill_qty: 1_000,
            quote_id_used: quote_id.clone(),
            status: FillStatus::Confirmed,
            latency_ms: 10,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1030);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::PositionOpened(PositionOpenedPayload {
            entry_price: 1.0,
            entry_time_ms: 1030,
            epoch_id: 1,
            size_tokens: 1_000,
            size_sol: 1_000_000,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1040);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::AemTick(AemTickPayload {
            regime_key: "{\"k\":\"v\"}".to_string(),
            regime_tag: "Balanced".to_string(),
            features_summary: serde_json::json!({"drawdown_pct": 1.2}),
            rollout_mode: "PilotLive".to_string(),
            hard_safety_state: None,
            drawdown_pct: 1.2,
            unrealized_pnl_pct: 3.4,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1050);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    env.command_id = Some(command_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ManagementDecision(ManagementDecisionPayload {
            decision: serde_json::json!({"decision_event_id": command_id, "action": "ForceExitAll"}),
            counterfactual_basis_quote_id: Some(quote_id.clone()),
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1060);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    env.command_id = Some("cmd-1".to_string());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ControlCommandIssued(ControlCommandIssuedPayload {
            directive: "ForceExitAll".to_string(),
            fraction_bps: Some(10_000),
            freeze_until_ms: None,
            issued_at_ms: 1060,
            valid_from_ms: 1060,
            expires_at_ms: 1160,
            epoch: 1,
            priority: "AemPolicy".to_string(),
            reason_code: "test_reason".to_string(),
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1070);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    env.command_id = Some("cmd-1".to_string());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ControlCommandApplied(ControlCommandAppliedPayload {
            accepted: true,
            reject_reason: None,
            applied_at_ms: 1070,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1080);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(exit_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    env.command_id = Some("cmd-1".to_string());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ExitSubmitted(ExitSubmittedPayload {
            fraction_bps: 10_000,
            command_ref: Some("cmd-1".to_string()),
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1090);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(exit_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    env.command_id = Some("cmd-1".to_string());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ExitFilled(ExitFilledPayload {
            fill_price: 1.05,
            fill_qty: 1_000,
            realized_pnl_delta: 0.05,
            status: FillStatus::Confirmed,
            is_partial: false,
            remaining_qty: 0,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1100);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.order_id = Some(entry_order_id.clone());
    env.quote_id = Some(quote_id.clone());
    events.push(ExecutionEvent::new(
        env,
        EventKind::PositionClosed(PositionClosedPayload {
            final_pnl: 0.05,
            final_pnl_pct: 5.0,
            entry_value_sol: None,
            exit_value_sol: None,
            gross_pnl_sol: None,
            net_pnl_sol: None,
            estimated_costs_sol: None,
            duration_ms: 100,
            reason: CloseReason::Default,
            total_exits: 1,
        }),
    ));

    let mut env = EventEnvelope::new(run_id.clone(), lane, candidate_id.clone(), 1110);
    env.position_id = Some(position_id.clone());
    env.position_epoch = Some(1);
    env.command_id = Some("cmd-1".to_string());
    events.push(ExecutionEvent::new(
        env,
        EventKind::ManagementOutcome(ManagementOutcomePayload {
            outcome: serde_json::json!({"result": "ok", "decision_event_id": "cmd-1"}),
        }),
    ));

    let mut file = NamedTempFile::new().expect("tmp file");
    for event in events {
        writeln!(
            file,
            "{}",
            serde_json::to_string(&event).expect("serialize")
        )
        .expect("write line");
    }

    let metrics = EventValidator::validate_jsonl(file.path()).expect("validate jsonl");
    assert!(
        metrics.invariant_violations.is_empty(),
        "expected no invariant violations, got: {:?}",
        metrics.invariant_violations
    );
    assert_eq!(metrics.valid_trajectories, 1);
}
