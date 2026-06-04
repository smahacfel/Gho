use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::events::schema::{EventKind, ExecutionEvent};
use crate::execution::backend::{CandidateId, FillStatus, Lane};

const DEFAULT_MAX_QUOTE_AGE_MS: u64 = 1_500;

#[derive(Debug, Default)]
pub struct ValidatorMetrics {
    pub total_events: usize,
    pub valid_trajectories: usize,
    pub invariant_violations: Vec<InvariantViolation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantViolation {
    pub run_id: String,
    pub lane: Lane,
    pub candidate_id: CandidateId,
    pub reason: String,
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[run={}, lane={}, candidate={}] {}",
            self.run_id, self.lane, self.candidate_id, self.reason
        )
    }
}

#[derive(Debug, Default)]
struct CandidateTrajectory {
    run_id: String,
    lane: Lane,
    candidate_id: CandidateId,
    has_candidate: bool,
    has_entry_submitted: bool,
    has_position_opened: bool,
    has_position_closed: bool,
    entry_fill_statuses: Vec<FillStatus>,
    commands_issued: HashSet<String>,
    commands_applied: HashSet<String>,
}

#[derive(Debug, Default)]
struct CrossLaneState {
    lanes: HashSet<Lane>,
    order_ids_by_lane: HashMap<Lane, HashSet<String>>,
    position_ids_by_lane: HashMap<Lane, HashSet<String>>,
}

pub struct EventValidator;

impl EventValidator {
    pub fn validate_jsonl<P: AsRef<Path>>(path: P) -> anyhow::Result<ValidatorMetrics> {
        let mut metrics = ValidatorMetrics::default();
        let events = Self::read_events(path, &mut metrics)?;

        let mut trajectories: HashMap<(String, Lane, CandidateId), CandidateTrajectory> =
            HashMap::new();
        let mut cross_lane: HashMap<(String, CandidateId), CrossLaneState> = HashMap::new();

        let mut entry_submitted_orders: HashSet<(String, Lane, String)> = HashSet::new();
        let mut exit_submitted_orders: HashSet<(String, Lane, String)> = HashSet::new();
        let mut exit_order_to_position: HashMap<(String, Lane, String), String> = HashMap::new();
        let mut seen_position_opened: HashSet<(String, Lane, String)> = HashSet::new();
        let mut position_epoch_by_id: HashMap<(String, Lane, String), u64> = HashMap::new();
        let mut epoch_start_by_candidate: HashMap<(String, Lane, CandidateId, u64), String> =
            HashMap::new();

        for event in &events {
            let run_id = event.envelope.run_id.clone();
            let lane = event.envelope.lane;
            let candidate_id = event.envelope.candidate_id.clone();

            if run_id.is_empty() {
                metrics.invariant_violations.push(Self::violation(
                    &run_id,
                    lane,
                    &candidate_id,
                    "schema:run_id empty",
                ));
            }
            if candidate_id.is_empty() {
                metrics.invariant_violations.push(Self::violation(
                    &run_id,
                    lane,
                    &candidate_id,
                    "schema:candidate_id empty",
                ));
            }

            if matches!(
                &event.kind,
                EventKind::NewPoolDetected(_) | EventKind::PoolTransaction(_)
            ) {
                continue;
            }

            if let Some(ref quote_id) = event.envelope.quote_id {
                if let Some(violation) = Self::quote_freshness_violation(
                    quote_id,
                    event.envelope.event_time_ms,
                    &run_id,
                    lane,
                    &candidate_id,
                ) {
                    metrics.invariant_violations.push(violation);
                }
            }

            let t_key = (run_id.clone(), lane, candidate_id.clone());
            let trajectory = trajectories
                .entry(t_key)
                .or_insert_with(|| CandidateTrajectory {
                    run_id: run_id.clone(),
                    lane,
                    candidate_id: candidate_id.clone(),
                    ..Default::default()
                });

            let c_key = (run_id.clone(), candidate_id.clone());
            let lane_state = cross_lane.entry(c_key).or_default();
            lane_state.lanes.insert(lane);
            if let Some(ref order_id) = event.envelope.order_id {
                lane_state
                    .order_ids_by_lane
                    .entry(lane)
                    .or_default()
                    .insert(order_id.clone());
            }
            if let Some(ref position_id) = event.envelope.position_id {
                lane_state
                    .position_ids_by_lane
                    .entry(lane)
                    .or_default()
                    .insert(position_id.clone());
            }

            match &event.kind {
                EventKind::Candidate(_) => {
                    trajectory.has_candidate = true;
                }
                EventKind::EntrySubmitted(_) => {
                    trajectory.has_entry_submitted = true;
                    match event.envelope.order_id.as_ref() {
                        Some(order_id) => {
                            entry_submitted_orders.insert((run_id.clone(), lane, order_id.clone()));
                        }
                        None => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:EntrySubmitted missing order_id",
                        )),
                    }
                }
                EventKind::EntryFilled(payload) => {
                    trajectory.entry_fill_statuses.push(payload.status);
                    if let Some(violation) = Self::quote_freshness_violation(
                        &payload.quote_id_used,
                        event.envelope.event_time_ms,
                        &run_id,
                        lane,
                        &candidate_id,
                    ) {
                        metrics.invariant_violations.push(violation);
                    }
                    match event.envelope.order_id.as_ref() {
                        Some(order_id)
                            if entry_submitted_orders.contains(&(
                                run_id.clone(),
                                lane,
                                order_id.clone(),
                            )) => {}
                        Some(order_id) => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            &format!(
                                "join:EntryFilled order_id {} has no EntrySubmitted",
                                order_id
                            ),
                        )),
                        None => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:EntryFilled missing order_id",
                        )),
                    }
                }
                EventKind::PositionOpened(payload) => {
                    trajectory.has_position_opened = true;
                    match event.envelope.position_id.as_ref() {
                        Some(position_id) => {
                            let key = (run_id.clone(), lane, position_id.clone());
                            if !seen_position_opened.insert(key.clone()) {
                                metrics.invariant_violations.push(Self::violation(
                                    &run_id,
                                    lane,
                                    &candidate_id,
                                    &format!(
                                        "join:PositionOpened repeated for position_id {}",
                                        position_id
                                    ),
                                ));
                            }
                            if let Some(epoch) = event.envelope.position_epoch {
                                if let Some(previous_epoch) =
                                    position_epoch_by_id.insert(key.clone(), epoch)
                                {
                                    if previous_epoch != epoch {
                                        metrics.invariant_violations.push(Self::violation(
                                            &run_id,
                                            lane,
                                            &candidate_id,
                                            &format!(
                                                "join:position_epoch changed for position_id {} ({} -> {})",
                                                position_id, previous_epoch, epoch
                                            ),
                                        ));
                                    }
                                }

                                let epoch_key = (run_id.clone(), lane, candidate_id.clone(), epoch);
                                if let Some(previous_position) =
                                    epoch_start_by_candidate.insert(epoch_key, position_id.clone())
                                {
                                    if previous_position != *position_id {
                                        metrics.invariant_violations.push(Self::violation(
                                            &run_id,
                                            lane,
                                            &candidate_id,
                                            &format!(
                                                "join:duplicate epoch start {} for candidate maps to multiple positions ({} vs {})",
                                                epoch, previous_position, position_id
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                        None => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:PositionOpened missing position_id",
                        )),
                    }

                    if event.envelope.position_epoch.is_none() {
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:PositionOpened missing position_epoch",
                        ));
                    }
                    if let Some(epoch) = event.envelope.position_epoch {
                        if epoch != payload.epoch_id {
                            metrics.invariant_violations.push(Self::violation(
                                &run_id,
                                lane,
                                &candidate_id,
                                "join:PositionOpened envelope.position_epoch != payload.epoch_id",
                            ));
                        }
                    }
                }
                EventKind::PositionClosed(_) => {
                    trajectory.has_position_closed = true;
                }
                EventKind::ControlCommandIssued(_) => {
                    if let Some(command_id) = event.envelope.command_id.clone() {
                        trajectory.commands_issued.insert(command_id);
                    } else {
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ControlCommandIssued missing command_id",
                        ));
                    }
                }
                EventKind::ControlCommandApplied(payload) => {
                    if let Some(command_id) = event.envelope.command_id.clone() {
                        trajectory.commands_applied.insert(command_id);
                    } else {
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ControlCommandApplied missing command_id",
                        ));
                    }

                    if !payload.accepted {
                        let allowed = ["epoch_mismatch", "ttl_expired", "priority_lock"];
                        let valid = payload
                            .reject_reason
                            .as_ref()
                            .map(|r| allowed.contains(&r.as_str()))
                            .unwrap_or(false);
                        if !valid {
                            metrics.invariant_violations.push(Self::violation(
                                &run_id,
                                lane,
                                &candidate_id,
                                "join:ControlCommandApplied invalid reject_reason",
                            ));
                        }
                    }
                }
                EventKind::ExitSubmitted(_) => match event.envelope.order_id.as_ref() {
                    Some(order_id) => {
                        exit_submitted_orders.insert((run_id.clone(), lane, order_id.clone()));
                        match event.envelope.position_id.as_ref() {
                            Some(position_id) => {
                                exit_order_to_position.insert(
                                    (run_id.clone(), lane, order_id.clone()),
                                    position_id.clone(),
                                );
                                if !seen_position_opened.contains(&(
                                    run_id.clone(),
                                    lane,
                                    position_id.clone(),
                                )) {
                                    metrics.invariant_violations.push(Self::violation(
                                        &run_id,
                                        lane,
                                        &candidate_id,
                                        &format!(
                                            "join:ExitSubmitted references unknown position_id {}",
                                            position_id
                                        ),
                                    ));
                                }
                            }
                            None => metrics.invariant_violations.push(Self::violation(
                                &run_id,
                                lane,
                                &candidate_id,
                                "join:ExitSubmitted missing position_id",
                            )),
                        }
                    }
                    None => metrics.invariant_violations.push(Self::violation(
                        &run_id,
                        lane,
                        &candidate_id,
                        "join:ExitSubmitted missing order_id",
                    )),
                },
                EventKind::ExitFilled(_) => {
                    let mut matched_exit_submit = false;
                    match event.envelope.order_id.as_ref() {
                        Some(order_id)
                            if exit_submitted_orders.contains(&(
                                run_id.clone(),
                                lane,
                                order_id.clone(),
                            )) =>
                        {
                            matched_exit_submit = true;
                            if let Some(filled_position_id) = event.envelope.position_id.as_ref() {
                                if let Some(submitted_position_id) = exit_order_to_position.get(&(
                                    run_id.clone(),
                                    lane,
                                    order_id.clone(),
                                )) {
                                    if submitted_position_id != filled_position_id {
                                        metrics.invariant_violations.push(Self::violation(
                                                    &run_id,
                                                    lane,
                                                    &candidate_id,
                                                    &format!(
                                                        "join:ExitFilled order {} position mismatch (submitted={} filled={})",
                                                        order_id, submitted_position_id, filled_position_id
                                                    ),
                                                ));
                                    }
                                }
                            }
                        }
                        Some(order_id) => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            &format!("join:ExitFilled order_id {} has no ExitSubmitted", order_id),
                        )),
                        None => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ExitFilled missing order_id",
                        )),
                    }
                    if event.envelope.position_id.is_none() {
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ExitFilled missing position_id",
                        ));
                    } else if matched_exit_submit {
                        let position_id =
                            event.envelope.position_id.as_ref().expect("checked above");
                        if !seen_position_opened.contains(&(
                            run_id.clone(),
                            lane,
                            position_id.clone(),
                        )) {
                            metrics.invariant_violations.push(Self::violation(
                                &run_id,
                                lane,
                                &candidate_id,
                                &format!(
                                    "join:ExitFilled references unknown position_id {}",
                                    position_id
                                ),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        for trajectory in trajectories.values() {
            let mut valid = true;

            if !trajectory.has_candidate {
                metrics.invariant_violations.push(Self::violation(
                    &trajectory.run_id,
                    trajectory.lane,
                    &trajectory.candidate_id,
                    "timeline:missing Candidate",
                ));
                valid = false;
            }

            if trajectory.has_candidate && !trajectory.has_entry_submitted {
                metrics.invariant_violations.push(Self::violation(
                    &trajectory.run_id,
                    trajectory.lane,
                    &trajectory.candidate_id,
                    "timeline:Candidate without EntrySubmitted",
                ));
                valid = false;
            }

            if trajectory.has_entry_submitted && trajectory.entry_fill_statuses.is_empty() {
                metrics.invariant_violations.push(Self::violation(
                    &trajectory.run_id,
                    trajectory.lane,
                    &trajectory.candidate_id,
                    "timeline:EntrySubmitted without EntryFilled",
                ));
                valid = false;
            }

            let has_successful_entry = trajectory
                .entry_fill_statuses
                .iter()
                .any(|s| matches!(s, FillStatus::Filled | FillStatus::Confirmed));

            if has_successful_entry && !trajectory.has_position_opened {
                metrics.invariant_violations.push(Self::violation(
                    &trajectory.run_id,
                    trajectory.lane,
                    &trajectory.candidate_id,
                    "timeline:successful EntryFilled without PositionOpened",
                ));
                valid = false;
            }

            if trajectory.has_position_opened && !trajectory.has_position_closed {
                metrics.invariant_violations.push(Self::violation(
                    &trajectory.run_id,
                    trajectory.lane,
                    &trajectory.candidate_id,
                    "timeline:PositionOpened without PositionClosed",
                ));
                valid = false;
            }

            for command_id in &trajectory.commands_issued {
                if !trajectory.commands_applied.contains(command_id) {
                    metrics.invariant_violations.push(Self::violation(
                        &trajectory.run_id,
                        trajectory.lane,
                        &trajectory.candidate_id,
                        &format!(
                            "join:ControlCommandIssued {} has no ControlCommandApplied",
                            command_id
                        ),
                    ));
                    valid = false;
                }
            }

            for command_id in &trajectory.commands_applied {
                if !trajectory.commands_issued.contains(command_id) {
                    metrics.invariant_violations.push(Self::violation(
                        &trajectory.run_id,
                        trajectory.lane,
                        &trajectory.candidate_id,
                        &format!(
                            "join:ControlCommandApplied {} has no ControlCommandIssued",
                            command_id
                        ),
                    ));
                    valid = false;
                }
            }

            if valid {
                metrics.valid_trajectories += 1;
            }
        }

        for ((run_id, candidate_id), state) in &cross_lane {
            let lane_pairs = [
                (Lane::Paper, Lane::Live),
                (Lane::Paper, Lane::Shadow),
                (Lane::Live, Lane::Shadow),
            ];
            for (left_lane, right_lane) in lane_pairs {
                if state.lanes.contains(&left_lane) && state.lanes.contains(&right_lane) {
                    let left_orders = state.order_ids_by_lane.get(&left_lane);
                    let right_orders = state.order_ids_by_lane.get(&right_lane);
                    if let (Some(left), Some(right)) = (left_orders, right_orders) {
                        if left.iter().any(|id| right.contains(id)) {
                            metrics.invariant_violations.push(Self::violation(
                                run_id,
                                Lane::Single,
                                candidate_id,
                                &format!(
                                    "lane:order_id overlap between {} and {}",
                                    left_lane, right_lane
                                ),
                            ));
                        }
                    }

                    let left_positions = state.position_ids_by_lane.get(&left_lane);
                    let right_positions = state.position_ids_by_lane.get(&right_lane);
                    if let (Some(left), Some(right)) = (left_positions, right_positions) {
                        if left.iter().any(|id| right.contains(id)) {
                            metrics.invariant_violations.push(Self::violation(
                                run_id,
                                Lane::Single,
                                candidate_id,
                                &format!(
                                    "lane:position_id overlap between {} and {}",
                                    left_lane, right_lane
                                ),
                            ));
                        }
                    }
                }
            }
        }

        Ok(metrics)
    }

    pub fn validate_timeline<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<InvariantViolation>> {
        let metrics = Self::validate_jsonl(path)?;
        Ok(metrics
            .invariant_violations
            .into_iter()
            .filter(|v| v.reason.starts_with("timeline:"))
            .collect())
    }

    pub fn validate_joins<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<InvariantViolation>> {
        let metrics = Self::validate_jsonl(path)?;
        Ok(metrics
            .invariant_violations
            .into_iter()
            .filter(|v| {
                v.reason.starts_with("join:")
                    || v.reason.starts_with("lane:")
                    || v.reason.starts_with("quote:")
            })
            .collect())
    }

    fn read_events<P: AsRef<Path>>(
        path: P,
        metrics: &mut ValidatorMetrics,
    ) -> anyhow::Result<Vec<ExecutionEvent>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for (line_num, line_res) in reader.lines().enumerate() {
            let line = line_res?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<ExecutionEvent>(&line) {
                Ok(event) => {
                    metrics.total_events += 1;
                    events.push(event);
                }
                Err(e) => {
                    metrics.invariant_violations.push(InvariantViolation {
                        run_id: "UNKNOWN".to_string(),
                        lane: Lane::Single,
                        candidate_id: "UNKNOWN".to_string(),
                        reason: format!("parse:error at line {}: {}", line_num + 1, e),
                    });
                }
            }
        }

        Ok(events)
    }

    fn violation(run_id: &str, lane: Lane, candidate_id: &str, reason: &str) -> InvariantViolation {
        InvariantViolation {
            run_id: run_id.to_string(),
            lane,
            candidate_id: candidate_id.to_string(),
            reason: reason.to_string(),
        }
    }

    fn extract_quote_ts(quote_id: &str) -> Option<u64> {
        let mut parts = quote_id.split('_');
        let _slot = parts.next()?;
        parts.next()?.parse::<u64>().ok()
    }

    fn quote_freshness_violation(
        quote_id: &str,
        event_time_ms: u64,
        run_id: &str,
        lane: Lane,
        candidate_id: &str,
    ) -> Option<InvariantViolation> {
        let ts = Self::extract_quote_ts(quote_id)?;
        let age_ms = event_time_ms.saturating_sub(ts);
        if age_ms > DEFAULT_MAX_QUOTE_AGE_MS {
            Some(Self::violation(
                run_id,
                lane,
                candidate_id,
                &format!(
                    "quote:stale quote_id={} age_ms={} max_age_ms={}",
                    quote_id, age_ms, DEFAULT_MAX_QUOTE_AGE_MS
                ),
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::schema::{
        CandidatePayload, EntryFilledPayload, EntrySubmittedPayload, EventEnvelope, EventKind,
        PoolTransactionPayload,
    };
    use crate::execution::backend::{FillStatus, OrderSide};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validator_happy_path() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut env = EventEnvelope::new("r1".into(), Lane::Paper, "c1".into(), 100);
        env.order_id = Some("ord-1".into());

        let events = vec![
            ExecutionEvent::new(
                EventEnvelope::new("r1".into(), Lane::Paper, "c1".into(), 99),
                EventKind::Candidate(CandidatePayload {
                    mcap_snapshot: None,
                    price_snapshot: None,
                    gatekeeper_verdict: "PASS".into(),
                    gatekeeper_flags: vec![],
                    source: "test".into(),
                }),
            ),
            ExecutionEvent::new(
                env.derive(100),
                EventKind::EntrySubmitted(EntrySubmittedPayload {
                    side: OrderSide::Entry,
                    planned_delay_ms: None,
                    send_params: None,
                    amount_lamports: 10,
                    min_tokens_out: 10,
                }),
            ),
            ExecutionEvent::new(
                env.derive(101),
                EventKind::EntryFilled(EntryFilledPayload {
                    fill_time_ms: 101,
                    fill_price_effective: 1.0,
                    fill_qty: 10,
                    quote_id_used: "0_101_1".into(),
                    status: FillStatus::Failed,
                    latency_ms: 1,
                }),
            ),
        ];

        for e in events {
            writeln!(file, "{}", serde_json::to_string(&e).expect("serialize"))
                .expect("write event");
        }

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert_eq!(metrics.invariant_violations.len(), 0);
        assert_eq!(metrics.valid_trajectories, 1);
    }

    #[test]
    fn test_validator_treats_pool_transaction_as_evidence_only() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let event = ExecutionEvent::new(
            EventEnvelope::new("r1".into(), Lane::Paper, "mint:pool:100".into(), 100),
            EventKind::PoolTransaction(PoolTransactionPayload {
                schema_version: "v1".to_string(),
                pool_amm_id: "pool".to_string(),
                pool_id: "pool".to_string(),
                source_pool_amm_id: None,
                base_mint: Some("mint".to_string()),
                mint_id: Some("mint".to_string()),
                token_mint: Some("mint".to_string()),
                quote_mint: Some("So11111111111111111111111111111111111111112".to_string()),
                bonding_curve: "pool".to_string(),
                signature: "sig-tx".to_string(),
                event_slot: Some(1),
                slot: Some(1),
                tx_index: Some(0),
                event_ordinal: Some(0),
                outer_instruction_index: None,
                inner_group_index: None,
                event_ts_ms: 100,
                timestamp_ms: 100,
                arrival_ts_ms: 101,
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
                execution_account_contract_status: "route_account_manifest_incomplete".to_string(),
                execution_account_contract_reason: Some(
                    "route_account_manifest_incomplete:missing_global_config".to_string(),
                ),
            }),
        );

        writeln!(
            file,
            "{}",
            serde_json::to_string(&event).expect("serialize")
        )
        .expect("write event");

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert_eq!(metrics.invariant_violations.len(), 0);
        assert_eq!(metrics.valid_trajectories, 0);
    }

    #[test]
    fn test_validator_missing_fill() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut env = EventEnvelope::new("r1".into(), Lane::Paper, "c1".into(), 100);
        env.order_id = Some("ord-1".into());

        let event = ExecutionEvent::new(
            env.derive(100),
            EventKind::EntrySubmitted(EntrySubmittedPayload {
                side: OrderSide::Entry,
                planned_delay_ms: None,
                send_params: None,
                amount_lamports: 10,
                min_tokens_out: 10,
            }),
        );

        writeln!(
            file,
            "{}",
            serde_json::to_string(&event).expect("serialize")
        )
        .expect("write event");

        let timeline = EventValidator::validate_timeline(file.path()).expect("validate timeline");
        assert!(!timeline.is_empty());
        assert!(timeline[0].reason.starts_with("timeline:"));
    }

    #[test]
    fn test_validator_detects_live_shadow_order_overlap() {
        let mut file = NamedTempFile::new().expect("tmp file");

        let mut live_env = EventEnvelope::new("r1".into(), Lane::Live, "c1".into(), 100);
        live_env.order_id = Some("ord-shared".into());
        let mut shadow_env = EventEnvelope::new("r1".into(), Lane::Shadow, "c1".into(), 101);
        shadow_env.order_id = Some("ord-shared".into());

        let events = vec![
            ExecutionEvent::new(
                live_env,
                EventKind::EntrySubmitted(EntrySubmittedPayload {
                    side: OrderSide::Entry,
                    planned_delay_ms: None,
                    send_params: None,
                    amount_lamports: 10,
                    min_tokens_out: 10,
                }),
            ),
            ExecutionEvent::new(
                shadow_env,
                EventKind::EntrySubmitted(EntrySubmittedPayload {
                    side: OrderSide::Entry,
                    planned_delay_ms: None,
                    send_params: None,
                    amount_lamports: 10,
                    min_tokens_out: 10,
                }),
            ),
        ];

        for event in events {
            writeln!(
                file,
                "{}",
                serde_json::to_string(&event).expect("serialize")
            )
            .expect("write event");
        }

        let joins = EventValidator::validate_joins(file.path()).expect("validate joins");
        assert!(
            joins
                .iter()
                .any(|violation| violation.reason.contains("overlap between live and shadow")),
            "expected live/shadow overlap violation, got: {joins:?}"
        );
    }
}
