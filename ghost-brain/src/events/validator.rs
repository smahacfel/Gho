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
    has_terminal_schema_violation: bool,
    entry_fill_statuses: Vec<FillStatus>,
    commands_issued: HashSet<String>,
    commands_applied: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PositionLifecycleKey {
    run_id: String,
    lane: Lane,
    position_id: String,
    position_epoch: u64,
}

#[derive(Debug, Default)]
struct PositionLifecycleState {
    candidate_id: CandidateId,
    opened_count: usize,
    closed_count: usize,
    unresolved_count: usize,
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
        let mut position_lifecycles: HashMap<PositionLifecycleKey, PositionLifecycleState> =
            HashMap::new();
        let mut cross_lane: HashMap<(String, CandidateId), CrossLaneState> = HashMap::new();

        let mut entry_submitted_orders: HashSet<(String, Lane, String)> = HashSet::new();
        let mut exit_submitted_orders: HashSet<(String, Lane, String)> = HashSet::new();
        let mut exit_order_to_position: HashMap<(String, Lane, String), String> = HashMap::new();
        let mut seen_position_opened: HashSet<(String, Lane, String, u64)> = HashSet::new();
        let mut opened_position_ids: HashSet<(String, Lane, String)> = HashSet::new();
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
                    match (
                        event.envelope.position_id.as_ref(),
                        event.envelope.position_epoch,
                    ) {
                        (Some(position_id), Some(epoch)) => {
                            opened_position_ids.insert((run_id.clone(), lane, position_id.clone()));
                            let seen_key = (run_id.clone(), lane, position_id.clone(), epoch);
                            if !seen_position_opened.insert(seen_key) {
                                metrics.invariant_violations.push(Self::violation(
                                    &run_id,
                                    lane,
                                    &candidate_id,
                                    &format!(
                                        "join:PositionOpened repeated for position_id {} epoch {}",
                                        position_id, epoch
                                    ),
                                ));
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

                            let lifecycle = position_lifecycles
                                .entry(PositionLifecycleKey {
                                    run_id: run_id.clone(),
                                    lane,
                                    position_id: position_id.clone(),
                                    position_epoch: epoch,
                                })
                                .or_insert_with(|| PositionLifecycleState {
                                    candidate_id: candidate_id.clone(),
                                    ..Default::default()
                                });
                            if lifecycle.candidate_id != candidate_id {
                                metrics.invariant_violations.push(Self::violation(
                                    &run_id,
                                    lane,
                                    &candidate_id,
                                    "join:PositionOpened candidate mismatch for position epoch",
                                ));
                            }
                            lifecycle.opened_count = lifecycle.opened_count.saturating_add(1);
                        }
                        (None, _) => metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:PositionOpened missing position_id",
                        )),
                        (_, None) => {}
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
                    Self::record_position_terminal(
                        &mut position_lifecycles,
                        event,
                        &run_id,
                        lane,
                        &candidate_id,
                        true,
                        &mut metrics,
                    );
                }
                EventKind::ShadowPositionUnresolved(payload) => {
                    Self::record_position_terminal(
                        &mut position_lifecycles,
                        event,
                        &run_id,
                        lane,
                        &candidate_id,
                        false,
                        &mut metrics,
                    );
                    if !matches!(lane, Lane::Shadow) {
                        trajectory.has_terminal_schema_violation = true;
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "timeline:ShadowPositionUnresolved is only valid for shadow lane",
                        ));
                    }
                    if event.envelope.position_id.is_none() {
                        trajectory.has_terminal_schema_violation = true;
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ShadowPositionUnresolved missing position_id",
                        ));
                    }
                    if event.envelope.position_epoch.is_none() {
                        trajectory.has_terminal_schema_violation = true;
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "join:ShadowPositionUnresolved missing position_epoch",
                        ));
                    }
                    if payload.net_pnl_authoritative {
                        trajectory.has_terminal_schema_violation = true;
                        metrics.invariant_violations.push(Self::violation(
                            &run_id,
                            lane,
                            &candidate_id,
                            "schema:ShadowPositionUnresolved cannot claim authoritative net PnL",
                        ));
                    }
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
                                if !opened_position_ids.contains(&(
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
                    } else if let (true, Some(position_id)) =
                        (matched_exit_submit, event.envelope.position_id.as_ref())
                    {
                        if !opened_position_ids.contains(&(
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

        for (key, lifecycle) in &position_lifecycles {
            if lifecycle.opened_count != 1 {
                metrics.invariant_violations.push(Self::violation(
                    &key.run_id,
                    key.lane,
                    &lifecycle.candidate_id,
                    &format!(
                        "timeline:position_id {} epoch {} requires exactly one PositionOpened",
                        key.position_id, key.position_epoch
                    ),
                ));
            }
            let terminal_count = lifecycle
                .closed_count
                .saturating_add(lifecycle.unresolved_count);
            if lifecycle.opened_count == 1 && terminal_count == 0 {
                metrics.invariant_violations.push(Self::violation(
                    &key.run_id,
                    key.lane,
                    &lifecycle.candidate_id,
                    &format!(
                        "timeline:PositionOpened without terminal for position_id {} epoch {}",
                        key.position_id, key.position_epoch
                    ),
                ));
            }
            if terminal_count > 1 {
                metrics.invariant_violations.push(Self::violation(
                    &key.run_id,
                    key.lane,
                    &lifecycle.candidate_id,
                    &format!(
                        "timeline:position_id {} epoch {} requires exactly one terminal PositionClosed or ShadowPositionUnresolved",
                        key.position_id, key.position_epoch
                    ),
                ));
            }
        }

        let invalid_trajectory_keys: HashSet<(String, Lane, CandidateId)> = metrics
            .invariant_violations
            .iter()
            .map(|violation| {
                (
                    violation.run_id.clone(),
                    violation.lane,
                    violation.candidate_id.clone(),
                )
            })
            .collect();

        for trajectory in trajectories.values() {
            let trajectory_key = (
                trajectory.run_id.clone(),
                trajectory.lane,
                trajectory.candidate_id.clone(),
            );
            let mut valid = !trajectory.has_terminal_schema_violation
                && !invalid_trajectory_keys.contains(&trajectory_key);

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

    #[allow(clippy::too_many_arguments)]
    fn record_position_terminal(
        position_lifecycles: &mut HashMap<PositionLifecycleKey, PositionLifecycleState>,
        event: &ExecutionEvent,
        run_id: &str,
        lane: Lane,
        candidate_id: &str,
        is_closed: bool,
        metrics: &mut ValidatorMetrics,
    ) {
        let terminal_label = if is_closed {
            "PositionClosed"
        } else {
            "ShadowPositionUnresolved"
        };
        let Some(position_id) = event.envelope.position_id.as_ref() else {
            metrics.invariant_violations.push(Self::violation(
                run_id,
                lane,
                candidate_id,
                &format!("join:{terminal_label} missing position_id"),
            ));
            return;
        };
        let Some(position_epoch) = event.envelope.position_epoch else {
            metrics.invariant_violations.push(Self::violation(
                run_id,
                lane,
                candidate_id,
                &format!("join:{terminal_label} missing position_epoch"),
            ));
            return;
        };
        let lifecycle = position_lifecycles
            .entry(PositionLifecycleKey {
                run_id: run_id.to_string(),
                lane,
                position_id: position_id.clone(),
                position_epoch,
            })
            .or_insert_with(|| PositionLifecycleState {
                candidate_id: candidate_id.to_string(),
                ..Default::default()
            });
        if lifecycle.opened_count == 0 {
            metrics.invariant_violations.push(Self::violation(
                run_id,
                lane,
                candidate_id,
                &format!(
                    "timeline:{terminal_label} without PositionOpened exact match for position_id {position_id} epoch {position_epoch}"
                ),
            ));
        }
        if lifecycle.candidate_id != candidate_id {
            metrics.invariant_violations.push(Self::violation(
                run_id,
                lane,
                candidate_id,
                &format!(
                    "join:{terminal_label} candidate mismatch for position_id {position_id} epoch {position_epoch}"
                ),
            ));
        }
        if is_closed {
            lifecycle.closed_count = lifecycle.closed_count.saturating_add(1);
        } else {
            lifecycle.unresolved_count = lifecycle.unresolved_count.saturating_add(1);
        }
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
        PoolTransactionPayload, PositionClosedPayload, PositionOpenedPayload,
        ShadowPositionUnresolvedPayload, ShadowUnresolvedReason,
    };
    use crate::execution::backend::{FillStatus, OrderSide};
    use std::io::Write;
    use tempfile::NamedTempFile;
    use trigger::{PriceTruthSource, PriceTruthStatus};

    fn write_events(file: &mut NamedTempFile, events: Vec<ExecutionEvent>) {
        for event in events {
            writeln!(
                file,
                "{}",
                serde_json::to_string(&event).expect("serialize")
            )
            .expect("write event");
        }
    }

    fn shadow_lifecycle_with_unresolved(
        unresolved_lane: Lane,
        include_open: bool,
        include_close: bool,
    ) -> Vec<ExecutionEvent> {
        let run_id = "r-shadow-unresolved".to_string();
        let candidate_id = "candidate-shadow-unresolved".to_string();
        let position_id = "position-shadow-unresolved".to_string();
        let position_epoch = 7;
        let entry_order_id = "entry-shadow-unresolved".to_string();

        let mut entry_env =
            EventEnvelope::new(run_id.clone(), Lane::Shadow, candidate_id.clone(), 100);
        entry_env.order_id = Some(entry_order_id.clone());
        entry_env.position_id = Some(position_id.clone());
        entry_env.position_epoch = Some(position_epoch);

        let mut events = vec![
            ExecutionEvent::new(
                EventEnvelope::new(run_id.clone(), Lane::Shadow, candidate_id.clone(), 99),
                EventKind::Candidate(CandidatePayload {
                    mcap_snapshot: None,
                    price_snapshot: None,
                    gatekeeper_verdict: "PASS".to_string(),
                    gatekeeper_flags: vec![],
                    source: "test".to_string(),
                }),
            ),
            ExecutionEvent::new(
                entry_env.derive(100),
                EventKind::EntrySubmitted(EntrySubmittedPayload {
                    side: OrderSide::Entry,
                    planned_delay_ms: None,
                    send_params: None,
                    amount_lamports: 10,
                    min_tokens_out: 10,
                }),
            ),
            ExecutionEvent::new(
                entry_env.derive(101),
                EventKind::EntryFilled(EntryFilledPayload {
                    fill_time_ms: 101,
                    fill_price_effective: 1.0,
                    fill_qty: 10,
                    quote_id_used: "0_101_1".to_string(),
                    status: FillStatus::Filled,
                    latency_ms: 1,
                }),
            ),
        ];

        if include_open {
            events.push(ExecutionEvent::new(
                entry_env.derive(102),
                EventKind::PositionOpened(PositionOpenedPayload {
                    entry_price: 1.0,
                    entry_time_ms: 102,
                    epoch_id: position_epoch,
                    size_tokens: 10,
                    size_sol: 10,
                }),
            ));
        }

        let mut unresolved_env =
            EventEnvelope::new(run_id.clone(), unresolved_lane, candidate_id.clone(), 103);
        unresolved_env.position_id = Some(position_id.clone());
        unresolved_env.position_epoch = Some(position_epoch);
        unresolved_env.order_id = Some("shadow-unresolved-action".to_string());
        events.push(ExecutionEvent::new(
            unresolved_env,
            EventKind::ShadowPositionUnresolved(ShadowPositionUnresolvedPayload {
                reason: ShadowUnresolvedReason::BlockedByData,
                action_id: "shadow-unresolved-action".to_string(),
                policy_id: "position_manager_lite_exit_policy_v1".to_string(),
                policy_version: 1,
                policy_config_hash: "hash".to_string(),
                remaining_qty: 10,
                recovery_elapsed_ms: 5_000,
                truth_status: PriceTruthStatus::Stale,
                truth_source: PriceTruthSource::CanonicalAccountStateSnapshot,
                truth_slot: Some(103),
                truth_timestamp_ms: Some(103),
                truth_age_ms: Some(5_000),
                truth_detail: Some("stale".to_string()),
                source_snapshot_id: "snapshot".to_string(),
                execution_cost_coverage: "unmodeled".to_string(),
                net_pnl_authoritative: false,
            }),
        ));

        if include_close {
            events.push(ExecutionEvent::new(
                entry_env.derive(104),
                EventKind::PositionClosed(PositionClosedPayload {
                    final_pnl: 1.0,
                    final_pnl_pct: 10.0,
                    entry_value_sol: Some(10.0),
                    exit_value_sol: Some(11.0),
                    gross_pnl_sol: Some(1.0),
                    net_pnl_sol: Some(1.0),
                    estimated_costs_sol: Some(0.0),
                    duration_ms: 1_000,
                    reason: crate::events::schema::CloseReason::Target,
                    total_exits: 1,
                }),
            ));
        }

        events
    }

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

    #[test]
    fn validator_accepts_shadow_unresolved_as_the_only_terminal() {
        let mut file = NamedTempFile::new().expect("tmp file");
        write_events(
            &mut file,
            shadow_lifecycle_with_unresolved(Lane::Shadow, true, false),
        );

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(
            metrics.invariant_violations.is_empty(),
            "valid unresolved trajectory was rejected: {:?}",
            metrics.invariant_violations
        );
        assert_eq!(metrics.valid_trajectories, 1);
    }

    #[test]
    fn validator_rejects_unresolved_outside_shadow_lane() {
        let mut file = NamedTempFile::new().expect("tmp file");
        write_events(
            &mut file,
            shadow_lifecycle_with_unresolved(Lane::Live, true, false),
        );

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation
                .reason
                .contains("ShadowPositionUnresolved is only valid for shadow lane")
        }));
        assert_eq!(metrics.valid_trajectories, 0);
    }

    #[test]
    fn validator_rejects_unresolved_without_position_opened() {
        let mut file = NamedTempFile::new().expect("tmp file");
        write_events(
            &mut file,
            shadow_lifecycle_with_unresolved(Lane::Shadow, false, false),
        );

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation
                .reason
                .contains("ShadowPositionUnresolved without PositionOpened")
        }));
        assert_eq!(metrics.valid_trajectories, 0);
    }

    #[test]
    fn validator_rejects_close_and_unresolved_for_one_position() {
        let mut file = NamedTempFile::new().expect("tmp file");
        write_events(
            &mut file,
            shadow_lifecycle_with_unresolved(Lane::Shadow, true, true),
        );

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation.reason.contains(
                "requires exactly one terminal PositionClosed or ShadowPositionUnresolved",
            )
        }));
        assert_eq!(metrics.valid_trajectories, 0);
    }

    #[test]
    fn validator_rejects_terminal_for_wrong_position() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut events = shadow_lifecycle_with_unresolved(Lane::Shadow, true, false);
        events.last_mut().expect("terminal").envelope.position_id = Some("wrong-position".into());
        write_events(&mut file, events);

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation
                .reason
                .contains("without PositionOpened exact match")
        }));
    }

    #[test]
    fn validator_rejects_terminal_for_wrong_epoch() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut events = shadow_lifecycle_with_unresolved(Lane::Shadow, true, false);
        events.last_mut().expect("terminal").envelope.position_epoch = Some(99);
        write_events(&mut file, events);

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation
                .reason
                .contains("without PositionOpened exact match")
        }));
    }

    #[test]
    fn validator_rejects_duplicate_terminal_for_exact_position_epoch() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut events = shadow_lifecycle_with_unresolved(Lane::Shadow, true, false);
        events.push(events.last().expect("terminal").clone());
        write_events(&mut file, events);

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(metrics.invariant_violations.iter().any(|violation| {
            violation.reason.contains(
                "requires exactly one terminal PositionClosed or ShadowPositionUnresolved",
            )
        }));
    }

    #[test]
    fn validator_accepts_two_sequential_epochs_for_one_candidate() {
        let mut file = NamedTempFile::new().expect("tmp file");
        let mut events = shadow_lifecycle_with_unresolved(Lane::Shadow, true, false);
        let terminal = events.last().expect("terminal").clone();
        let mut second_open_env = terminal.envelope.derive(104);
        second_open_env.position_epoch = Some(8);
        events.push(ExecutionEvent::new(
            second_open_env,
            EventKind::PositionOpened(PositionOpenedPayload {
                entry_price: 1.0,
                entry_time_ms: 104,
                epoch_id: 8,
                size_tokens: 10,
                size_sol: 10,
            }),
        ));
        let mut second_terminal = terminal;
        second_terminal.envelope = second_terminal.envelope.derive(105);
        second_terminal.envelope.position_epoch = Some(8);
        events.push(second_terminal);
        write_events(&mut file, events);

        let metrics = EventValidator::validate_jsonl(file.path()).expect("validate");
        assert!(
            metrics.invariant_violations.is_empty(),
            "valid sequential epochs rejected: {:?}",
            metrics.invariant_violations
        );
        assert_eq!(metrics.valid_trajectories, 1);
    }
}
