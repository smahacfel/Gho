use std::collections::HashMap;
use std::sync::Mutex;

use solana_sdk::pubkey::Pubkey;

use super::config::AemConfig;
use super::runtime::{AemRuntime, OutcomeFeatureSource, OutcomeSample};
use super::types::*;

#[derive(Default)]
struct InMemoryLedger {
    decisions: Mutex<Vec<ManagementDecisionEvent>>,
    outcomes: Mutex<Vec<ManagementOutcomeEvent>>,
    replay_error: Mutex<Option<String>>,
}

impl InMemoryLedger {
    fn with_replay_error(msg: &str) -> Self {
        Self {
            decisions: Mutex::new(Vec::new()),
            outcomes: Mutex::new(Vec::new()),
            replay_error: Mutex::new(Some(msg.to_string())),
        }
    }

    fn decision_count(&self) -> usize {
        self.decisions.lock().expect("decisions mutex").len()
    }

    fn outcome_count(&self) -> usize {
        self.outcomes.lock().expect("outcomes mutex").len()
    }

    fn last_decision(&self) -> ManagementDecisionEvent {
        self.decisions
            .lock()
            .expect("decisions mutex")
            .last()
            .cloned()
            .expect("decision exists")
    }

    fn seed_pair(&self, pair: ReplayPair) {
        self.append_decision(&pair.decision)
            .expect("append decision");
        self.append_outcome(&pair.outcome).expect("append outcome");
    }
}

impl AemLedgerWriter for InMemoryLedger {
    fn append_decision(
        &self,
        event: &ManagementDecisionEvent,
    ) -> Result<(), super::error::AemError> {
        self.decisions
            .lock()
            .expect("decisions mutex")
            .push(event.clone());
        Ok(())
    }

    fn append_outcome(&self, event: &ManagementOutcomeEvent) -> Result<(), super::error::AemError> {
        self.outcomes
            .lock()
            .expect("outcomes mutex")
            .push(event.clone());
        Ok(())
    }

    fn append_time_index(&self, _idx: &TimeIndexRecord) -> Result<(), super::error::AemError> {
        Ok(())
    }

    fn append_regime_index(&self, _idx: &RegimeIndexRecord) -> Result<(), super::error::AemError> {
        Ok(())
    }
}

impl AemLedgerReader for InMemoryLedger {
    fn replay_pairs_in_window(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ReplayPair>, super::error::AemError> {
        if let Some(msg) = self
            .replay_error
            .lock()
            .expect("replay_error mutex")
            .clone()
        {
            return Err(super::error::AemError::LedgerDegraded(msg));
        }

        let decisions = self.decisions.lock().expect("decisions mutex").clone();
        let outcomes = self.outcomes.lock().expect("outcomes mutex").clone();
        let mut decision_map = HashMap::new();
        for d in decisions {
            decision_map.insert(d.decision_event_id.clone(), d);
        }

        let mut pairs = Vec::new();
        for o in outcomes {
            if o.timestamp_outcome_unix_ms < window_start_unix_ms
                || o.timestamp_outcome_unix_ms > window_end_unix_ms
            {
                continue;
            }
            if let Some(d) = decision_map.get(&o.decision_event_id) {
                if d.timestamp_decision_unix_ms < window_start_unix_ms
                    || d.timestamp_decision_unix_ms > window_end_unix_ms
                {
                    continue;
                }
                pairs.push(ReplayPair {
                    decision: d.clone(),
                    outcome: o.clone(),
                });
            }
            if pairs.len() >= max_events {
                break;
            }
        }

        Ok(pairs)
    }

    fn decisions_without_outcome(
        &self,
        window_start_unix_ms: UnixMs,
        window_end_unix_ms: UnixMs,
        max_events: usize,
    ) -> Result<Vec<ManagementDecisionEvent>, super::error::AemError> {
        let decisions = self.decisions.lock().expect("decisions mutex").clone();
        let outcomes = self.outcomes.lock().expect("outcomes mutex").clone();

        let mut with_outcome = std::collections::HashSet::new();
        for o in outcomes {
            with_outcome.insert(o.decision_event_id);
        }

        let mut pending = Vec::new();
        for d in decisions {
            if d.timestamp_decision_unix_ms < window_start_unix_ms
                || d.timestamp_decision_unix_ms > window_end_unix_ms
            {
                continue;
            }
            if !with_outcome.contains(&d.decision_event_id) {
                pending.push(d);
            }
            if pending.len() >= max_events {
                break;
            }
        }
        Ok(pending)
    }
}

#[derive(Default)]
struct MockTrigger {
    applied: Vec<ControlCommand>,
}

impl TriggerControlAdapter for MockTrigger {
    fn apply_control_command(
        &mut self,
        cmd: &ControlCommand,
        _now_unix_ms: UnixMs,
    ) -> CommandApplyResult {
        self.applied.push(cmd.clone());
        CommandApplyResult {
            accepted: true,
            reject_reason: None,
        }
    }

    fn get_execution_stress(&self, _position_id: &str) -> Option<ExecutionStressSnapshot> {
        None
    }

    fn register_position_epoch(&mut self, _position_id: &str, _position_epoch: u64) {}

    fn unregister_position_epoch(&mut self, _position_id: &str) {}
}

#[derive(Default)]
struct MockOutcomeSource {
    by_position_and_ts: HashMap<(String, u64), OutcomeSample>,
}

impl MockOutcomeSource {
    fn insert(&mut self, position_id: &str, decision_ts: u64, sample: OutcomeSample) {
        self.by_position_and_ts
            .insert((position_id.to_string(), decision_ts), sample);
    }
}

impl OutcomeFeatureSource for MockOutcomeSource {
    fn sample_outcome(
        &self,
        position_id: &str,
        decision_ts_unix_ms: u64,
        _horizon_ms: u64,
    ) -> Option<OutcomeSample> {
        self.by_position_and_ts
            .get(&(position_id.to_string(), decision_ts_unix_ms))
            .cloned()
    }
}

fn make_position_id(mint: Pubkey) -> String {
    format!("pool:{}:{}", mint, 1_700_000_000_000u64)
}

fn mk_features(position_id: &str, mint: Pubkey) -> StateFeatures {
    StateFeatures {
        position_id: position_id.to_string(),
        pool_amm_id: Pubkey::new_unique(),
        base_mint: mint,
        entry_price_or_mcap: 100.0,
        current_price_or_mcap: 55.0,
        peak_since_entry: 100.0,
        drawdown_pct: 45.0,
        unrealized_pnl_pct: -45.0,
        slope_pct_per_s: -0.20,
        volatility_proxy: Some(0.1),
        reclaim_flag: ReclaimFlag::None,
        time_since_entry_s: 35,
        time_since_last_peak_s: 20,
        requeue_count: 0,
        send_fail_count: 0,
        relax_count: 0,
        oracle_stale_age_ms: 10,
        last_sell_attempt_age_ms: Some(100),
        stress_bucket: StressBucket::Low,
    }
}

fn register(runtime: &mut AemRuntime, position_id: &str, mint: Pubkey, now_ms: u64) {
    runtime
        .register_position(
            position_id.to_string(),
            Pubkey::new_unique(),
            mint,
            now_ms,
            100.0,
            1,
        )
        .expect("register position");
}

fn decision_template(
    position_id: &str,
    decision_ts: u64,
    action: ActionChosen,
    features: &StateFeatures,
) -> ManagementDecisionEvent {
    let regime_key = super::regime::compute_regime_key(features);
    ManagementDecisionEvent {
        decision_event_id: format!("{}:1:{}", position_id, decision_ts),
        position_id: position_id.to_string(),
        position_epoch: 1,
        timestamp_decision_unix_ms: decision_ts,
        regime_key,
        regime_tag: super::regime::detect_regime(features),
        action_chosen: action,
        features_snapshot: features.clone(),
        hard_safety_triggered: false,
        hard_safety_reason_code: None,
        control_command: ControlCommand {
            position_id: position_id.to_string(),
            action,
            directive: CommandDirective::Noop,
            issued_at_unix_ms: decision_ts,
            valid_from_unix_ms: decision_ts,
            expires_at_unix_ms: decision_ts + 5000,
            position_epoch: 1,
            priority: CommandPriority::AemPolicy,
            reason_code: "seed".to_string(),
        },
        rollout_mode: RolloutMode::FullLive,
    }
}

fn outcome_template(
    decision: &ManagementDecisionEvent,
    ts: u64,
    delta: f64,
    tail: bool,
) -> ManagementOutcomeEvent {
    ManagementOutcomeEvent {
        outcome_event_id: format!("outcome:{}", decision.decision_event_id),
        decision_event_id: decision.decision_event_id.clone(),
        position_id: decision.position_id.clone(),
        timestamp_outcome_unix_ms: ts,
        price_at_decision: 1.0,
        price_at_t: Some(1.0 + delta),
        peak_in_t: Some(1.0 + delta.max(0.0)),
        reclaim_happened: delta > 0.0,
        time_to_reclaim_ms: if delta > 0.0 { Some(20_000) } else { None },
        counterfactual_delta_pnl: delta,
        tail_loss_flag: tail,
        outcome_data_gap: false,
        outcome_reason_code: "ok".to_string(),
    }
}

fn seed_ci_pass_stats(ledger: &InMemoryLedger, features: &StateFeatures, now_ms: u64) {
    for i in 0..20u64 {
        let d = decision_template(
            &features.position_id,
            now_ms - 80_000 - i,
            ActionChosen::WaitReclaim,
            features,
        );
        let o = outcome_template(&d, now_ms - 70_000 - i, 0.05, false);
        ledger.seed_pair(ReplayPair {
            decision: d,
            outcome: o,
        });
    }

    for i in 0..2u64 {
        let d_sell = decision_template(
            &features.position_id,
            now_ms - 60_000 - i,
            ActionChosen::SellNow,
            features,
        );
        let o_sell = outcome_template(&d_sell, now_ms - 59_000 - i, 0.0, false);
        ledger.seed_pair(ReplayPair {
            decision: d_sell,
            outcome: o_sell,
        });

        let d_partial = decision_template(
            &features.position_id,
            now_ms - 50_000 - i,
            ActionChosen::Partial,
            features,
        );
        let o_partial = outcome_template(&d_partial, now_ms - 49_000 - i, 0.01, false);
        ledger.seed_pair(ReplayPair {
            decision: d_partial,
            outcome: o_partial,
        });
    }
}

#[test]
fn test_ssot_time_windows_from_t_s() {
    let cfg = AemConfig::default();
    let dw = cfg.derived_time_windows();
    assert_eq!(dw.outcome_horizon_ms, 120_000);
    assert_eq!(dw.reclaim_timeout_ms, 90_000);
    assert_eq!(dw.panic_freeze_start_ms, 20_400);
    assert_eq!(dw.panic_freeze_end_ms, 60_000);
    assert!(dw.panic_freeze_end_ms <= dw.reclaim_timeout_ms);
}

#[test]
fn test_deterministic_decision_for_same_input() {
    let now_ms = 1_700_000_100_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut rt1 = AemRuntime::new(cfg.clone());
    let mut rt2 = AemRuntime::new(cfg);
    register(&mut rt1, &position_id, mint, now_ms - 1_000);
    register(&mut rt2, &position_id, mint, now_ms - 1_000);

    let ledger1 = InMemoryLedger::default();
    let ledger2 = InMemoryLedger::default();
    let mut trigger1 = MockTrigger::default();
    let mut trigger2 = MockTrigger::default();

    rt1.on_tick(features.clone(), now_ms, &ledger1, &mut trigger1)
        .expect("tick 1");
    rt2.on_tick(features, now_ms, &ledger2, &mut trigger2)
        .expect("tick 2");

    let d1 = ledger1.last_decision();
    let d2 = ledger2.last_decision();

    assert_eq!(d1.action_chosen, d2.action_chosen);
    assert_eq!(d1.decision_event_id, d2.decision_event_id);
    assert_eq!(
        d1.control_command.expires_at_unix_ms,
        d2.control_command.expires_at_unix_ms
    );
    assert_eq!(d1.control_command.priority, d2.control_command.priority);
}

#[test]
fn test_hard_safety_overrides_wait_reclaim() {
    let now_ms = 1_700_000_200_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.drawdown_pct = 80.0;
    features.unrealized_pnl_pct = -80.0;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    let mut trigger = MockTrigger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Panic);
    assert!(decision.hard_safety_triggered);
    assert_ne!(decision.action_chosen, ActionChosen::WaitReclaim);
}

#[test]
fn test_wait_reclaim_ci_pass_path() {
    let now_ms = 1_700_000_300_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    seed_ci_pass_stats(&ledger, &features, now_ms);
    runtime
        .bootstrap_from_ledger(&ledger, now_ms)
        .expect("bootstrap");

    register(&mut runtime, &position_id, mint, now_ms - 2_000);
    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::WaitReclaim);
    assert!(matches!(
        decision.control_command.directive,
        CommandDirective::FreezePanic
    ));
    assert_eq!(decision.rollout_mode, RolloutMode::FullLive);
}

#[test]
fn test_wait_veto_when_stress_not_low() {
    let now_ms = 1_700_000_400_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.stress_bucket = StressBucket::Med;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert_eq!(
        decision.control_command.reason_code,
        "fallback_stress_not_low"
    );
}

#[test]
fn test_wait_veto_when_oracle_stale() {
    let now_ms = 1_700_000_500_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.oracle_stale_age_ms = 1_500;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert!(decision.hard_safety_triggered);
    assert_eq!(
        decision.hard_safety_reason_code,
        Some(SafetyReasonCode::OracleStaleHard)
    );
}

#[test]
fn test_ci_fail_falls_back_to_partial() {
    let now_ms = 1_700_000_600_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert_eq!(decision.control_command.reason_code, "fallback_n_too_low");
    assert!(matches!(
        decision.control_command.directive,
        CommandDirective::ForceExitFractionBps { fraction_bps: 5000 }
    ));
}

#[test]
fn test_freeze_panic_ttl_is_bounded_by_reclaim_timeout() {
    let now_ms = 1_700_000_700_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let mut runtime = AemRuntime::new(cfg.clone());
    let ledger = InMemoryLedger::default();
    seed_ci_pass_stats(&ledger, &features, now_ms);
    runtime
        .bootstrap_from_ledger(&ledger, now_ms)
        .expect("bootstrap");
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    let ttl = decision
        .control_command
        .expires_at_unix_ms
        .saturating_sub(decision.control_command.issued_at_unix_ms);

    assert_eq!(decision.action_chosen, ActionChosen::WaitReclaim);
    assert_eq!(ttl, cfg.derived_time_windows().reclaim_timeout_ms);
}

#[test]
fn test_replay_build_is_deterministic_after_restart() {
    let now_ms = 1_700_000_800_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let ledger_seed = InMemoryLedger::default();
    seed_ci_pass_stats(&ledger_seed, &features, now_ms);

    let mut rt1 = AemRuntime::new(cfg.clone());
    let mut rt2 = AemRuntime::new(cfg);
    rt1.bootstrap_from_ledger(&ledger_seed, now_ms)
        .expect("bootstrap rt1");
    rt2.bootstrap_from_ledger(&ledger_seed, now_ms)
        .expect("bootstrap rt2");

    register(&mut rt1, &position_id, mint, now_ms - 1_000);
    register(&mut rt2, &position_id, mint, now_ms - 1_000);

    let ledger1 = InMemoryLedger::default();
    let ledger2 = InMemoryLedger::default();
    let mut trigger1 = MockTrigger::default();
    let mut trigger2 = MockTrigger::default();

    rt1.on_tick(features.clone(), now_ms, &ledger1, &mut trigger1)
        .expect("tick1");
    rt2.on_tick(features, now_ms, &ledger2, &mut trigger2)
        .expect("tick2");

    assert_eq!(
        ledger1.last_decision().action_chosen,
        ledger2.last_decision().action_chosen
    );
    assert_eq!(
        ledger1.last_decision().control_command.reason_code,
        ledger2.last_decision().control_command.reason_code
    );
}

#[test]
fn test_crash_recovery_rebuilds_pending_and_flushes_outcome() {
    let now_ms = 1_700_000_900_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let ledger = InMemoryLedger::default();
    let mut first_runtime = AemRuntime::new(cfg.clone());
    register(&mut first_runtime, &position_id, mint, now_ms - 1_000);
    let mut trigger = MockTrigger::default();
    first_runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick before crash");
    assert_eq!(ledger.decision_count(), 1);
    assert_eq!(ledger.outcome_count(), 0);

    let mut second_runtime = AemRuntime::new(cfg.clone());
    second_runtime
        .bootstrap_from_ledger(&ledger, now_ms + 500)
        .expect("bootstrap after crash");

    let decision = ledger.last_decision();
    let mut outcome_source = MockOutcomeSource::default();
    outcome_source.insert(
        &position_id,
        decision.timestamp_decision_unix_ms,
        OutcomeSample {
            price_at_t: Some(58.0),
            peak_in_t: Some(62.0),
            reclaim_happened: true,
            time_to_reclaim_ms: Some(40_000),
            outcome_data_gap: false,
        },
    );

    second_runtime
        .flush_due_outcomes(
            now_ms + cfg.derived_time_windows().outcome_horizon_ms + 1,
            &outcome_source,
            &ledger,
            None,
            |_pos_id: &str| -> Option<String> { None },
        )
        .expect("flush outcomes");

    assert_eq!(ledger.outcome_count(), 1);
    let outcome = ledger
        .outcomes
        .lock()
        .expect("outcomes mutex")
        .last()
        .cloned()
        .expect("outcome exists");
    assert_eq!(outcome.decision_event_id, decision.decision_event_id);
    assert!(!outcome.outcome_data_gap);
}

#[test]
fn test_v_dump_reclaim_prefers_wait_when_ci_passes() {
    let now_ms = 1_700_001_000_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.drawdown_pct = 50.0;
    features.unrealized_pnl_pct = -50.0;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let ledger = InMemoryLedger::default();
    seed_ci_pass_stats(&ledger, &features, now_ms);

    let mut runtime = AemRuntime::new(cfg);
    runtime
        .bootstrap_from_ledger(&ledger, now_ms)
        .expect("bootstrap");
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    assert_eq!(
        ledger.last_decision().action_chosen,
        ActionChosen::WaitReclaim
    );
}

#[test]
fn test_dead_slide_falls_back_to_sell_now() {
    let now_ms = 1_700_001_100_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.drawdown_pct = 65.0;
    features.unrealized_pnl_pct = -65.0;
    features.slope_pct_per_s = -1.2;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    assert_eq!(ledger.last_decision().action_chosen, ActionChosen::SellNow);
}

#[test]
fn test_oracle_stale_chaos_is_conservative() {
    let now_ms = 1_700_001_200_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.oracle_stale_age_ms = 20_000;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert!(decision.hard_safety_triggered);
}

#[test]
fn test_requeue_spiral_chaos_blocks_wait() {
    let now_ms = 1_700_001_300_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let mut features = mk_features(&position_id, mint);
    features.stress_bucket = StressBucket::High;
    features.requeue_count = 6;
    features.send_fail_count = 2;
    features.relax_count = 2;

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    let decision = ledger.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert_eq!(
        decision.hard_safety_reason_code,
        Some(SafetyReasonCode::StressHigh)
    );
}

#[test]
fn test_rollout_shadow_logs_without_trigger_apply() {
    let now_ms = 1_700_001_400_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 10;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    let mut trigger = MockTrigger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    assert_eq!(ledger.last_decision().rollout_mode, RolloutMode::Shadow);
    assert!(trigger.applied.is_empty());
}

#[test]
fn test_rollout_pilot_live_when_tail_risk_blocks_full_live() {
    let now_ms = 1_700_001_500_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let ledger = InMemoryLedger::default();
    seed_ci_pass_stats(&ledger, &features, now_ms);
    let bad_decision = decision_template(
        &position_id,
        now_ms - 30_000,
        ActionChosen::Partial,
        &features,
    );
    let bad_outcome = outcome_template(&bad_decision, now_ms - 29_000, -0.50, true);
    ledger.seed_pair(ReplayPair {
        decision: bad_decision,
        outcome: bad_outcome,
    });

    let mut runtime = AemRuntime::new(cfg);
    runtime
        .bootstrap_from_ledger(&ledger, now_ms)
        .expect("bootstrap");
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    assert_eq!(ledger.last_decision().rollout_mode, RolloutMode::PilotLive);
    assert_eq!(trigger.applied.len(), 1);
}

#[test]
fn test_rollout_full_live_applies_trigger() {
    let now_ms = 1_700_001_600_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let mut runtime = AemRuntime::new(cfg);
    let ledger = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);

    let mut trigger = MockTrigger::default();
    runtime
        .on_tick(features, now_ms, &ledger, &mut trigger)
        .expect("tick");

    assert_eq!(ledger.last_decision().rollout_mode, RolloutMode::FullLive);
    assert_eq!(trigger.applied.len(), 1);
}

#[test]
fn test_ledger_degraded_forces_conservative_shadow_behavior() {
    let now_ms = 1_700_001_700_000u64;
    let mint = Pubkey::new_unique();
    let position_id = make_position_id(mint);
    let features = mk_features(&position_id, mint);

    let mut cfg = AemConfig::default();
    cfg.min_stabilization_ticks = 1;
    cfg.shadow_positions = 0;

    let mut runtime = AemRuntime::new(cfg);
    let broken_ledger = InMemoryLedger::with_replay_error("corrupted");
    assert!(runtime
        .bootstrap_from_ledger(&broken_ledger, now_ms)
        .is_err());

    let clean_writer = InMemoryLedger::default();
    register(&mut runtime, &position_id, mint, now_ms - 1_000);
    let mut trigger = MockTrigger::default();

    runtime
        .on_tick(features, now_ms, &clean_writer, &mut trigger)
        .expect("tick");

    let decision = clean_writer.last_decision();
    assert_eq!(decision.action_chosen, ActionChosen::Partial);
    assert_eq!(
        decision.control_command.reason_code,
        "ledger_degraded_conservative"
    );
}
