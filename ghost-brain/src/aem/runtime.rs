use std::collections::HashMap;

use solana_sdk::pubkey::Pubkey;

use crate::aem::{
    config::AemConfig,
    error::AemError,
    feature_adapter::choose_default_directive,
    hard_safety::DefaultHardSafetyCheck,
    regime::{compute_regime_key_with_config, detect_regime},
    types::*,
};

#[derive(Debug, Clone)]
pub struct OutcomeSample {
    pub price_at_t: Option<f64>,
    pub peak_in_t: Option<f64>,
    pub reclaim_happened: bool,
    pub time_to_reclaim_ms: Option<u64>,
    pub outcome_data_gap: bool,
}

pub trait OutcomeFeatureSource: Send + Sync {
    fn sample_outcome(
        &self,
        position_id: &str,
        decision_ts_unix_ms: u64,
        horizon_ms: u64,
    ) -> Option<OutcomeSample>;
}

#[derive(Debug, Clone)]
struct PositionState {
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    entry_unix_ms: UnixMs,
    entry_price_or_mcap: f64,
    position_epoch: u64,
    tick_count: u64,
    ordinal: u32,
}

#[derive(Debug, Clone)]
struct PendingOutcome {
    due_unix_ms: UnixMs,
    decision: ManagementDecisionEvent,
}

#[derive(Debug, Clone)]
pub struct TickReport {
    pub decision: ManagementDecisionEvent,
    pub apply_result: Option<CommandApplyResult>,
}

pub struct AemRuntime {
    pub cfg: AemConfig,
    hard_safety: Box<dyn HardSafetyCheck>,
    policy: PolicyEngine,
    regime_book: RegimeBook,
    positions: HashMap<PositionId, PositionState>,
    pending_outcomes: HashMap<DecisionEventId, PendingOutcome>,
    positions_seen_total: u32,
    ledger_degraded: bool,
}

impl AemRuntime {
    pub fn new(cfg: AemConfig) -> Self {
        Self {
            cfg,
            hard_safety: Box::new(DefaultHardSafetyCheck),
            policy: PolicyEngine,
            regime_book: RegimeBook::default(),
            positions: HashMap::new(),
            pending_outcomes: HashMap::new(),
            positions_seen_total: 0,
            ledger_degraded: false,
        }
    }

    pub fn set_hard_safety(&mut self, hard_safety: Box<dyn HardSafetyCheck>) {
        self.hard_safety = hard_safety;
    }

    pub fn bootstrap_from_ledger(
        &mut self,
        reader: &dyn AemLedgerReader,
        now_unix_ms: UnixMs,
    ) -> Result<(), AemError> {
        self.cfg.validate()?;
        let window_start = now_unix_ms
            .saturating_sub((self.cfg.replay_window_days as u64).saturating_mul(86_400_000));
        let replay = match reader.replay_pairs_in_window(
            window_start,
            now_unix_ms,
            self.cfg.replay_max_events,
        ) {
            Ok(v) => v,
            Err(e) => {
                self.ledger_degraded = true;
                return Err(e);
            }
        };

        self.regime_book = RegimeBook::rebuild_from_replay(replay, now_unix_ms, &self.cfg)
            .map_err(|e| {
                self.ledger_degraded = true;
                e
            })?;

        let pending = reader.decisions_without_outcome(
            window_start,
            now_unix_ms,
            self.cfg.replay_max_events,
        )?;

        let horizon = self.cfg.derived_time_windows().outcome_horizon_ms;
        for d in pending {
            let due = d.timestamp_decision_unix_ms.saturating_add(horizon);
            self.pending_outcomes.insert(
                d.decision_event_id.clone(),
                PendingOutcome {
                    due_unix_ms: due,
                    decision: d,
                },
            );
        }
        Ok(())
    }

    pub fn register_position(
        &mut self,
        position_id: PositionId,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        entry_unix_ms: UnixMs,
        entry_price_or_mcap: f64,
        position_epoch: u64,
    ) -> Result<(), AemError> {
        if !entry_price_or_mcap.is_finite() || entry_price_or_mcap <= 0.0 {
            return Err(AemError::InvalidData(
                "entry_price_or_mcap must be finite and > 0".to_string(),
            ));
        }
        self.positions_seen_total = self.positions_seen_total.saturating_add(1);
        let ordinal = self.positions_seen_total;
        self.positions.insert(
            position_id,
            PositionState {
                pool_amm_id,
                base_mint,
                entry_unix_ms,
                entry_price_or_mcap,
                position_epoch,
                tick_count: 0,
                ordinal,
            },
        );
        Ok(())
    }

    pub fn unregister_position(&mut self, position_id: &str) -> Result<(), AemError> {
        self.positions.remove(position_id);
        Ok(())
    }

    fn compute_rollout_mode(&self, features: &StateFeatures, ordinal: u32) -> RolloutMode {
        if ordinal <= self.cfg.shadow_positions {
            return RolloutMode::Shadow;
        }

        if features.drawdown_pct >= self.cfg.pilot_drawdown_min_pct
            && (!self.cfg.pilot_requires_stress_low || features.stress_bucket == StressBucket::Low)
        {
            let mut full_live_ok = true;
            if self.cfg.full_live_requires_positive_mean_delta {
                let any_negative = self
                    .regime_book
                    .stats
                    .values()
                    .any(|s| s.n >= self.cfg.n_min_per_key && s.mean_delta_pnl <= 0.0);
                full_live_ok = !any_negative;
            }
            if full_live_ok {
                let any_tail_bad = self
                    .regime_book
                    .stats
                    .values()
                    .filter_map(|s| s.tail_risk_rate)
                    .any(|t| t > self.cfg.full_live_tail_risk_max);
                if !any_tail_bad {
                    return RolloutMode::FullLive;
                }
            }
            return RolloutMode::PilotLive;
        }

        RolloutMode::Shadow
    }

    fn build_command(
        &self,
        features: &StateFeatures,
        action: ActionChosen,
        reason_code: String,
        now_unix_ms: UnixMs,
        position_epoch: u64,
        priority: CommandPriority,
    ) -> ControlCommand {
        let windows = self.cfg.derived_time_windows();
        let ttl = match action {
            ActionChosen::WaitReclaim => windows.reclaim_timeout_ms,
            ActionChosen::Panic => windows.panic_freeze_end_ms.min(windows.reclaim_timeout_ms),
            ActionChosen::SellNow | ActionChosen::Partial => 5_000,
        };
        ControlCommand {
            position_id: features.position_id.clone(),
            action,
            directive: choose_default_directive(action, self.cfg.partial_fraction_bps),
            issued_at_unix_ms: now_unix_ms,
            valid_from_unix_ms: now_unix_ms,
            expires_at_unix_ms: now_unix_ms.saturating_add(ttl),
            position_epoch,
            priority,
            reason_code,
        }
    }

    pub fn on_tick(
        &mut self,
        features: StateFeatures,
        now_unix_ms: UnixMs,
        writer: &dyn AemLedgerWriter,
        trigger: &mut dyn TriggerControlAdapter,
    ) -> Result<(), AemError> {
        let _ = self.on_tick_with_report(features, now_unix_ms, writer, trigger, None, None)?;
        Ok(())
    }

    pub fn on_tick_with_report(
        &mut self,
        features: StateFeatures,
        now_unix_ms: UnixMs,
        writer: &dyn AemLedgerWriter,
        trigger: &mut dyn TriggerControlAdapter,
        emitter: Option<&crate::events::EventEmitter>,
        candidate_id: Option<&str>,
    ) -> Result<Option<TickReport>, AemError> {
        if !self.cfg.enabled {
            return Ok(None);
        }
        let (position_epoch, ordinal, tick_count) = {
            let Some(pos) = self.positions.get_mut(&features.position_id) else {
                return Err(AemError::PositionNotFound(features.position_id));
            };
            pos.tick_count = pos.tick_count.saturating_add(1);
            (pos.position_epoch, pos.ordinal, pos.tick_count)
        };
        if tick_count < self.cfg.min_stabilization_ticks as u64 {
            return Ok(None);
        }

        let rollout = self.compute_rollout_mode(&features, ordinal);
        let regime_tag = detect_regime(&features);
        let regime_key = compute_regime_key_with_config(&features, &self.cfg);

        let (action, reason_code, priority, hard_safety_reason) = if self.ledger_degraded {
            (
                ActionChosen::Partial,
                "ledger_degraded_conservative".to_string(),
                CommandPriority::AemPolicy,
                None,
            )
        } else if let Some(safety) = self.hard_safety.evaluate(&features, now_unix_ms, &self.cfg) {
            (
                safety.action,
                format!("hard_safety::{:?}", safety.reason_code),
                CommandPriority::HardSafety,
                Some(safety.reason_code),
            )
        } else {
            let policy = self.policy.decide(
                &features,
                regime_tag,
                &regime_key,
                &self.regime_book,
                &self.cfg,
            );
            (
                policy.action_chosen,
                policy.reason_code,
                CommandPriority::AemPolicy,
                None,
            )
        };

        let command = self.build_command(
            &features,
            action,
            reason_code.clone(),
            now_unix_ms,
            position_epoch,
            priority,
        );

        let decision_event_id = format!(
            "{}:{}:{}",
            features.position_id, position_epoch, command.issued_at_unix_ms
        );
        let decision = ManagementDecisionEvent {
            decision_event_id: decision_event_id.clone(),
            position_id: features.position_id.clone(),
            position_epoch,
            timestamp_decision_unix_ms: now_unix_ms,
            regime_key: regime_key.clone(),
            regime_tag: regime_tag.clone(),
            action_chosen: action,
            features_snapshot: features.clone(),
            hard_safety_triggered: priority == CommandPriority::HardSafety,
            hard_safety_reason_code: hard_safety_reason.clone(),
            control_command: command.clone(),
            rollout_mode: rollout,
        };

        if let (Some(e), Some(cid)) = (emitter, candidate_id) {
            let features_summary =
                serde_json::to_value(&features).unwrap_or(serde_json::Value::Null);
            e.emit_aem_tick(
                &cid.to_string(),
                &features.position_id,
                &format!("{:?}", regime_key),
                &format!("{:?}", regime_tag),
                features_summary,
                &format!("{:?}", rollout),
                hard_safety_reason.clone().map(|r| format!("{:?}", r)),
                features.drawdown_pct,
                features.unrealized_pnl_pct,
            );

            let decision_json = serde_json::to_value(&decision).unwrap_or(serde_json::Value::Null);
            e.emit_management_decision(
                &cid.to_string(),
                &features.position_id,
                decision_json,
                None,
                Some(decision.decision_event_id.clone()),
            );
        }

        let apply_result = if rollout != RolloutMode::Shadow {
            Some(trigger.apply_control_command(&command, now_unix_ms))
        } else {
            None
        };
        writer.append_decision(&decision)?;

        self.pending_outcomes.insert(
            decision_event_id,
            PendingOutcome {
                due_unix_ms: now_unix_ms
                    .saturating_add(self.cfg.derived_time_windows().outcome_horizon_ms),
                decision: decision.clone(),
            },
        );

        Ok(Some(TickReport {
            decision,
            apply_result,
        }))
    }

    pub fn flush_due_outcomes(
        &mut self,
        now_unix_ms: UnixMs,
        feature_source: &dyn OutcomeFeatureSource,
        writer: &dyn AemLedgerWriter,
        emitter: Option<&crate::events::EventEmitter>,
        get_candidate_id: impl Fn(&str) -> Option<String>,
    ) -> Result<Vec<ManagementOutcomeEvent>, AemError> {
        let mut emitted = Vec::new();
        let due_ids: Vec<DecisionEventId> = self
            .pending_outcomes
            .iter()
            .filter_map(|(id, p)| {
                if p.due_unix_ms <= now_unix_ms {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        for decision_id in due_ids {
            let Some(pending) = self.pending_outcomes.remove(&decision_id) else {
                continue;
            };
            let decision = pending.decision;
            let horizon = self.cfg.derived_time_windows().outcome_horizon_ms;
            let sample = feature_source.sample_outcome(
                &decision.position_id,
                decision.timestamp_decision_unix_ms,
                horizon,
            );

            let price_at_decision = decision.features_snapshot.current_price_or_mcap;
            let (price_at_t, peak_in_t, reclaim_happened, time_to_reclaim_ms, outcome_data_gap) =
                if let Some(s) = sample {
                    (
                        s.price_at_t,
                        s.peak_in_t,
                        s.reclaim_happened,
                        s.time_to_reclaim_ms,
                        s.outcome_data_gap,
                    )
                } else {
                    (None, None, false, None, true)
                };

            let delta = price_at_t
                .filter(|v| {
                    v.is_finite() && price_at_decision.is_finite() && price_at_decision > 0.0
                })
                .map(|pt| (pt - price_at_decision) / price_at_decision)
                .unwrap_or(0.0);

            let counterfactual_delta_pnl = match decision.action_chosen {
                ActionChosen::SellNow => 0.0,
                ActionChosen::WaitReclaim => delta,
                ActionChosen::Partial => delta * 0.5,
                ActionChosen::Panic => delta.min(0.0),
            };
            let tail_loss_flag = decision.action_chosen == ActionChosen::WaitReclaim
                && price_at_t
                    .map(|v| v < price_at_decision * 0.60)
                    .unwrap_or(false);

            let outcome = ManagementOutcomeEvent {
                outcome_event_id: format!("outcome:{}", decision_id),
                decision_event_id: decision_id.clone(),
                position_id: decision.position_id.clone(),
                timestamp_outcome_unix_ms: now_unix_ms,
                price_at_decision,
                price_at_t,
                peak_in_t,
                reclaim_happened,
                time_to_reclaim_ms,
                counterfactual_delta_pnl,
                tail_loss_flag,
                outcome_data_gap,
                outcome_reason_code: if outcome_data_gap {
                    "outcome_data_gap".to_string()
                } else {
                    "ok".to_string()
                },
            };

            writer.append_outcome(&outcome)?;
            if !outcome.outcome_data_gap {
                let _ = self.regime_book.update_from_outcome(
                    &decision,
                    &outcome,
                    now_unix_ms,
                    &self.cfg,
                );
            }
            if let Some(e) = emitter {
                if let Some(cid) = get_candidate_id(&decision.position_id) {
                    let outcome_json =
                        serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null);
                    e.emit_management_outcome(
                        &cid,
                        &decision.position_id,
                        outcome_json,
                        Some(decision.decision_event_id.clone()),
                    );
                }
            }
            emitted.push(outcome);
        }

        Ok(emitted)
    }
}
