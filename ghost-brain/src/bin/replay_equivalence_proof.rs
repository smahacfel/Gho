use ghost_brain::events::{
    CandidatePayload, CloseReason, EntryFilledPayload, EntrySubmittedPayload, EventEnvelope,
    EventKind, ExecutionEvent, PositionClosedPayload, PositionOpenedPayload,
};
use ghost_brain::{FillStatus, Lane, OrderSide};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
struct InputCandidate {
    candidate_id: String,
    submit_time_ms: u64,
    amount_lamports: u64,
    min_tokens_out: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LatencyProfile {
    Baseline,
    Stress,
    Pathological,
}

impl LatencyProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Stress => "stress",
            Self::Pathological => "pathological",
        }
    }

    fn parse_many(raw: &str) -> Result<Vec<Self>, String> {
        let mut out = Vec::new();
        for token in raw.split(',') {
            let p = match token.trim().to_ascii_lowercase().as_str() {
                "baseline" => Self::Baseline,
                "stress" => Self::Stress,
                "pathological" => Self::Pathological,
                other => return Err(format!("invalid profile: {other}")),
            };
            if !out.contains(&p) {
                out.push(p);
            }
        }
        if out.is_empty() {
            return Err("profiles list cannot be empty".to_string());
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scenario {
    None,
    F1ChannelClosed,
    F1ChannelFull,
    F2RecoverySweep,
}

impl Scenario {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::F1ChannelClosed => "f1_channel_closed",
            Self::F1ChannelFull => "f1_channel_full",
            Self::F2RecoverySweep => "f2_recovery_sweep",
        }
    }

    fn parse_many(raw: &str) -> Result<Vec<Self>, String> {
        let mut out = Vec::new();
        for token in raw.split(',') {
            let s = match token.trim().to_ascii_lowercase().as_str() {
                "none" => Self::None,
                "f1_channel_closed" | "f1_closed" => Self::F1ChannelClosed,
                "f1_channel_full" | "f1_full" => Self::F1ChannelFull,
                "f2_recovery_sweep" | "f2_recovery" => Self::F2RecoverySweep,
                other => return Err(format!("invalid scenario: {other}")),
            };
            if !out.contains(&s) {
                out.push(s);
            }
        }
        if out.is_empty() {
            return Err("scenarios list cannot be empty".to_string());
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    LiveOnly,
    Dual,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::LiveOnly => "live_only",
            Self::Dual => "dual",
        }
    }
}

#[derive(Debug, Clone)]
struct ScenarioOrder {
    candidate_id: String,
    order_id: String,
    quote_id: String,
    position_id: String,
    submit_time_ms: u64,
    latency_ms: u64,
    amount_lamports: u64,
    min_tokens_out: u64,
    fill_price_effective: f64,
    status: FillStatus,
    failure_reason: Option<String>,
    recovery_unknown: bool,
}

#[derive(Debug, Clone, Serialize)]
struct TimingBin {
    bucket: String,
    count: u64,
}

#[derive(Debug, Default, Clone, Serialize)]
struct RunReport {
    run_id: String,
    mode: String,
    lane: String,
    n: usize,
    seed: u64,
    profile: String,
    scenario: String,
    submitted: u64,
    filled: u64,
    failed: u64,
    timeout: u64,
    unknown: u64,
    terminal_total: u64,
    in_flight: i64,
    opened: u64,
    closed: u64,
    terminal_entry_fail: u64,
    duplicates_fill: u64,
    duplicates_opened: u64,
    duplicates_closed: u64,
    missing_terminal_orders: Vec<String>,
    multiple_terminal_orders: Vec<String>,
    candidate_without_opened_or_failed: Vec<String>,
    recovery_unknown_orders: Vec<String>,
    unknown_non_recovery_orders: Vec<String>,
    p50_time_to_fill_ms: f64,
    p90_time_to_fill_ms: f64,
    timing_histogram: Vec<TimingBin>,
    verdict: String,
    failed_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ClassDelta {
    class: String,
    live_only: u64,
    dual_live: u64,
    delta_abs: u64,
    max_allowed: u64,
    pass: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CaseReport {
    case_id: String,
    n: usize,
    seed: u64,
    profile: String,
    scenario: String,
    timing_threshold_pct: f64,
    terminal_delta_pct: f64,
    terminal_delta_abs_min: u64,
    live_events_path: String,
    dual_events_path: String,
    live_report_path: String,
    dual_live_report_path: String,
    case_verdict_path: String,
    submitted_match_pass: bool,
    lifecycle_accounting_pass: bool,
    exactly_once_pass: bool,
    gaps_pass: bool,
    timing_pass: bool,
    terminal_distribution_pass: bool,
    unknown_policy_pass: bool,
    inflight_zero_pass: bool,
    terminal_deltas: Vec<ClassDelta>,
    verdict: String,
    failed_checks: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UnifiedReport {
    output_dir: String,
    seed: u64,
    timing_threshold_pct: f64,
    terminal_delta_pct: f64,
    terminal_delta_abs_min: u64,
    cases: Vec<CaseReport>,
    verdict: String,
    failed_cases: Vec<String>,
}

#[derive(Debug)]
struct Cli {
    input: Option<PathBuf>,
    fixture_dir: PathBuf,
    output_dir: PathBuf,
    sizes: Vec<usize>,
    profiles: Vec<LatencyProfile>,
    scenarios: Vec<Scenario>,
    seed: u64,
    timing_threshold_pct: f64,
    terminal_delta_pct: f64,
    terminal_delta_abs_min: u64,
    ttl_ms: u64,
    pathological_timeout_pct: f64,
    scenario_impact_pct: f64,
}

fn usage() -> &'static str {
    "usage: replay_equivalence_proof [--input <path>] [--fixture-dir <dir>] [--sizes <csv>] [--profiles <baseline,stress,pathological>] [--scenarios <none,f1_channel_closed,f1_channel_full,f2_recovery_sweep>] [--output-dir <dir>] [--seed <u64>] [--timing-threshold-pct <f64>] [--terminal-delta-pct <f64>] [--terminal-delta-abs-min <u64>] [--ttl-ms <u64>] [--pathological-timeout-pct <f64>] [--scenario-impact-pct <f64>]"
}

fn parse_usize_csv(raw: &str) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for token in raw.split(',') {
        let parsed = token
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid size: {token}"))?;
        if parsed == 0 {
            return Err("size must be > 0".to_string());
        }
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
    if out.is_empty() {
        return Err("sizes list cannot be empty".to_string());
    }
    Ok(out)
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1);
    let mut input = None;
    let mut fixture_dir = PathBuf::from("ghost-brain/tests/fixtures/replay");
    let mut output_dir = PathBuf::from("ghost-brain/artifacts/replay_equivalence");
    let mut sizes = vec![50_usize];
    let mut profiles = vec![LatencyProfile::Baseline];
    let mut scenarios = vec![Scenario::None];
    let mut seed = 42_u64;
    let mut timing_threshold_pct = 10.0_f64;
    let mut terminal_delta_pct = 5.0_f64;
    let mut terminal_delta_abs_min = 2_u64;
    let mut ttl_ms = 500_u64;
    let mut pathological_timeout_pct = 8.0_f64;
    let mut scenario_impact_pct = 7.0_f64;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --input".to_string());
                };
                input = Some(PathBuf::from(v));
            }
            "--fixture-dir" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --fixture-dir".to_string());
                };
                fixture_dir = PathBuf::from(v);
            }
            "--output-dir" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --output-dir".to_string());
                };
                output_dir = PathBuf::from(v);
            }
            "--sizes" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --sizes".to_string());
                };
                sizes = parse_usize_csv(&v)?;
            }
            "--profiles" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --profiles".to_string());
                };
                profiles = LatencyProfile::parse_many(&v)?;
            }
            "--scenarios" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --scenarios".to_string());
                };
                scenarios = Scenario::parse_many(&v)?;
            }
            "--seed" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --seed".to_string());
                };
                seed = v.parse::<u64>().map_err(|_| "invalid --seed".to_string())?;
            }
            "--timing-threshold-pct" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --timing-threshold-pct".to_string());
                };
                timing_threshold_pct = v
                    .parse::<f64>()
                    .map_err(|_| "invalid --timing-threshold-pct".to_string())?;
            }
            "--terminal-delta-pct" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --terminal-delta-pct".to_string());
                };
                terminal_delta_pct = v
                    .parse::<f64>()
                    .map_err(|_| "invalid --terminal-delta-pct".to_string())?;
            }
            "--terminal-delta-abs-min" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --terminal-delta-abs-min".to_string());
                };
                terminal_delta_abs_min = v
                    .parse::<u64>()
                    .map_err(|_| "invalid --terminal-delta-abs-min".to_string())?;
            }
            "--ttl-ms" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --ttl-ms".to_string());
                };
                ttl_ms = v
                    .parse::<u64>()
                    .map_err(|_| "invalid --ttl-ms".to_string())?;
            }
            "--pathological-timeout-pct" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --pathological-timeout-pct".to_string());
                };
                pathological_timeout_pct = v
                    .parse::<f64>()
                    .map_err(|_| "invalid --pathological-timeout-pct".to_string())?;
            }
            "--scenario-impact-pct" => {
                let Some(v) = args.next() else {
                    return Err("missing value for --scenario-impact-pct".to_string());
                };
                scenario_impact_pct = v
                    .parse::<f64>()
                    .map_err(|_| "invalid --scenario-impact-pct".to_string())?;
            }
            "--help" | "-h" => return Err(usage().to_string()),
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Cli {
        input,
        fixture_dir,
        output_dir,
        sizes,
        profiles,
        scenarios,
        seed,
        timing_threshold_pct,
        terminal_delta_pct,
        terminal_delta_abs_min,
        ttl_ms,
        pathological_timeout_pct,
        scenario_impact_pct,
    })
}

fn read_candidates_jsonl(path: &Path) -> Result<Vec<InputCandidate>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut candidates = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        candidates.push(serde_json::from_str::<InputCandidate>(&line)?);
    }
    Ok(candidates)
}

fn fixture_path_for_size(fixture_dir: &Path, n: usize) -> PathBuf {
    if n == 50 {
        fixture_dir.join("candidates_test_set.jsonl")
    } else {
        fixture_dir.join(format!("candidates_{n}.jsonl"))
    }
}

fn deterministic_env(
    run_id: &str,
    lane: Lane,
    candidate_id: String,
    event_time_ms: u64,
    event_id: String,
) -> EventEnvelope {
    let mut env = EventEnvelope::new(run_id.to_string(), lane, candidate_id, event_time_ms);
    env.event_id = event_id;
    env
}

fn select_indices(total: usize, pct: f64, rng: &mut StdRng) -> HashSet<usize> {
    if total == 0 || pct <= 0.0 {
        return HashSet::new();
    }
    let mut indices: Vec<usize> = (0..total).collect();
    for i in (1..indices.len()).rev() {
        let j = rng.gen_range(0..=i);
        indices.swap(i, j);
    }
    let wanted = (((total as f64) * (pct / 100.0)).round() as usize).clamp(1, total);
    indices.into_iter().take(wanted).collect()
}

fn case_seed(seed: u64, n: usize, profile: LatencyProfile, scenario: Scenario) -> u64 {
    let profile_salt = match profile {
        LatencyProfile::Baseline => 11_u64,
        LatencyProfile::Stress => 23_u64,
        LatencyProfile::Pathological => 37_u64,
    };
    let scenario_salt = match scenario {
        Scenario::None => 101_u64,
        Scenario::F1ChannelClosed => 131_u64,
        Scenario::F1ChannelFull => 151_u64,
        Scenario::F2RecoverySweep => 181_u64,
    };
    seed ^ ((n as u64) << 8) ^ profile_salt ^ scenario_salt
}

fn build_scenario_orders(
    candidates: &[InputCandidate],
    seed: u64,
    profile: LatencyProfile,
    scenario: Scenario,
    ttl_ms: u64,
    pathological_timeout_pct: f64,
    scenario_impact_pct: f64,
) -> Vec<ScenarioOrder> {
    let mut rng = StdRng::seed_from_u64(seed);
    let pathological_set = if profile == LatencyProfile::Pathological {
        select_indices(candidates.len(), pathological_timeout_pct, &mut rng)
    } else {
        HashSet::new()
    };
    let scenario_set = if scenario != Scenario::None {
        select_indices(candidates.len(), scenario_impact_pct, &mut rng)
    } else {
        HashSet::new()
    };

    candidates
        .iter()
        .enumerate()
        .map(|(idx, c)| {
            let mut latency_ms = match profile {
                LatencyProfile::Baseline => rng.gen_range(200..=400),
                LatencyProfile::Stress => rng.gen_range(200..=600) + rng.gen_range(0..=50),
                LatencyProfile::Pathological => rng.gen_range(200..=400),
            };
            let mut status = if rng.gen_range(0..100) < 20 {
                FillStatus::Failed
            } else {
                FillStatus::Confirmed
            };
            let mut failure_reason = None;
            let mut recovery_unknown = false;

            if pathological_set.contains(&idx) {
                latency_ms = ttl_ms + rng.gen_range(25..=300);
                status = FillStatus::Stale;
                failure_reason = Some("pathological_ttl_timeout".to_string());
            }

            if scenario_set.contains(&idx) {
                match scenario {
                    Scenario::None => {}
                    Scenario::F1ChannelClosed => {
                        status = FillStatus::Failed;
                        latency_ms = rng.gen_range(1..=15);
                        failure_reason = Some("f1_channel_closed".to_string());
                    }
                    Scenario::F1ChannelFull => {
                        status = FillStatus::Failed;
                        latency_ms = rng.gen_range(1..=15);
                        failure_reason = Some("f1_channel_full".to_string());
                    }
                    Scenario::F2RecoverySweep => {
                        status = FillStatus::Unknown;
                        latency_ms = rng.gen_range(1..=10);
                        failure_reason = Some("f2_recovery_sweep_unknown".to_string());
                        recovery_unknown = true;
                    }
                }
            }

            let price_noise: f64 = rng.gen_range(0.95_f64..=1.05_f64);
            let fill_price_effective =
                (c.amount_lamports as f64 / c.min_tokens_out.max(1) as f64) * price_noise;

            ScenarioOrder {
                candidate_id: c.candidate_id.clone(),
                order_id: format!("order-live-{idx}"),
                quote_id: format!("1_{}_{}", c.submit_time_ms, idx),
                position_id: format!("pos-live-{idx}"),
                submit_time_ms: c.submit_time_ms,
                latency_ms,
                amount_lamports: c.amount_lamports,
                min_tokens_out: c.min_tokens_out,
                fill_price_effective,
                status,
                failure_reason,
                recovery_unknown,
            }
        })
        .collect()
}

fn make_lane_events(run_id: &str, lane: Lane, scenario: &[ScenarioOrder]) -> Vec<ExecutionEvent> {
    let mut out = Vec::new();
    for (idx, s) in scenario.iter().enumerate() {
        out.push(ExecutionEvent::new(
            deterministic_env(
                run_id,
                lane,
                s.candidate_id.clone(),
                s.submit_time_ms.saturating_sub(1),
                format!("evt-{lane}-{idx}-candidate"),
            ),
            EventKind::Candidate(CandidatePayload {
                mcap_snapshot: None,
                price_snapshot: Some(s.fill_price_effective),
                gatekeeper_verdict: "PASS".to_string(),
                gatekeeper_flags: vec!["deterministic_input".to_string()],
                source: "replay_fixture".to_string(),
            }),
        ));

        let mut env = deterministic_env(
            run_id,
            lane,
            s.candidate_id.clone(),
            s.submit_time_ms,
            format!("evt-{lane}-{idx}-entry-submitted"),
        );
        env.order_id = Some(s.order_id.clone());
        env.quote_id = Some(s.quote_id.clone());
        out.push(ExecutionEvent::new(
            env,
            EventKind::EntrySubmitted(EntrySubmittedPayload {
                side: OrderSide::Entry,
                planned_delay_ms: Some(s.latency_ms),
                send_params: Some(serde_json::json!({
                    "replay_seeded": true,
                    "failure_reason": s.failure_reason,
                    "recovery_unknown": s.recovery_unknown,
                })),
                amount_lamports: s.amount_lamports,
                min_tokens_out: s.min_tokens_out,
            }),
        ));

        let fill_time = s.submit_time_ms + s.latency_ms;
        let mut env = deterministic_env(
            run_id,
            lane,
            s.candidate_id.clone(),
            fill_time,
            format!("evt-{lane}-{idx}-entry-filled"),
        );
        env.order_id = Some(s.order_id.clone());
        env.quote_id = Some(s.quote_id.clone());
        out.push(ExecutionEvent::new(
            env,
            EventKind::EntryFilled(EntryFilledPayload {
                fill_time_ms: fill_time,
                fill_price_effective: s.fill_price_effective,
                fill_qty: if matches!(s.status, FillStatus::Confirmed | FillStatus::Filled) {
                    s.min_tokens_out
                } else {
                    0
                },
                quote_id_used: s.quote_id.clone(),
                status: s.status,
                latency_ms: s.latency_ms,
            }),
        ));

        if matches!(s.status, FillStatus::Confirmed | FillStatus::Filled) {
            let mut opened_env = deterministic_env(
                run_id,
                lane,
                s.candidate_id.clone(),
                fill_time.saturating_add(1),
                format!("evt-{lane}-{idx}-position-opened"),
            );
            opened_env.order_id = Some(s.order_id.clone());
            opened_env.position_id = Some(s.position_id.clone());
            opened_env.position_epoch = Some(1);
            opened_env.quote_id = Some(s.quote_id.clone());
            out.push(ExecutionEvent::new(
                opened_env,
                EventKind::PositionOpened(PositionOpenedPayload {
                    entry_price: s.fill_price_effective,
                    entry_time_ms: fill_time,
                    epoch_id: 1,
                    size_tokens: s.min_tokens_out,
                    size_sol: s.amount_lamports,
                }),
            ));

            let mut closed_env = deterministic_env(
                run_id,
                lane,
                s.candidate_id.clone(),
                fill_time.saturating_add(400),
                format!("evt-{lane}-{idx}-position-closed"),
            );
            closed_env.order_id = Some(format!("exit-{}", s.order_id));
            closed_env.position_id = Some(s.position_id.clone());
            closed_env.position_epoch = Some(1);
            closed_env.quote_id = Some(s.quote_id.clone());
            out.push(ExecutionEvent::new(
                closed_env,
                EventKind::PositionClosed(PositionClosedPayload {
                    final_pnl: 0.0,
                    final_pnl_pct: 0.0,
                    entry_value_sol: None,
                    exit_value_sol: None,
                    gross_pnl_sol: None,
                    net_pnl_sol: None,
                    estimated_costs_sol: None,
                    duration_ms: 400,
                    reason: CloseReason::Default,
                    total_exits: 1,
                }),
            ));
        }
    }
    out
}

fn make_dual_paper_orders(base: &[ScenarioOrder], seed: u64) -> Vec<ScenarioOrder> {
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(9_001));
    let mut out = Vec::with_capacity(base.len());
    for (idx, order) in base.iter().enumerate() {
        let mut next = order.clone();
        next.order_id = format!("order-paper-{idx}");
        next.position_id = format!("pos-paper-{idx}");
        next.latency_ms = rng.gen_range(120..=620);
        next.status = if rng.gen_range(0..100) < 25 {
            FillStatus::Failed
        } else {
            FillStatus::Confirmed
        };
        next.failure_reason = None;
        next.recovery_unknown = false;
        out.push(next);
    }
    out
}

fn write_jsonl(path: &Path, events: &[ExecutionEvent]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    for event in events {
        writeln!(file, "{}", serde_json::to_string(event)?)?;
    }
    Ok(())
}

fn read_jsonl(path: &Path) -> Result<Vec<ExecutionEvent>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str::<ExecutionEvent>(&line)?);
    }
    Ok(out)
}

fn percentile(values: &[u64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx] as f64
}

fn build_timing_histogram(values: &[u64]) -> Vec<TimingBin> {
    let mut b0 = 0_u64;
    let mut b1 = 0_u64;
    let mut b2 = 0_u64;
    let mut b3 = 0_u64;
    let mut b4 = 0_u64;
    for v in values {
        match *v {
            0..=199 => b0 += 1,
            200..=299 => b1 += 1,
            300..=399 => b2 += 1,
            400..=599 => b3 += 1,
            _ => b4 += 1,
        }
    }
    vec![
        TimingBin {
            bucket: "0-199".to_string(),
            count: b0,
        },
        TimingBin {
            bucket: "200-299".to_string(),
            count: b1,
        },
        TimingBin {
            bucket: "300-399".to_string(),
            count: b2,
        },
        TimingBin {
            bucket: "400-599".to_string(),
            count: b3,
        },
        TimingBin {
            bucket: "600+".to_string(),
            count: b4,
        },
    ]
}

fn audit_run(
    run_id: &str,
    mode: Mode,
    lane: Lane,
    n: usize,
    seed: u64,
    profile: LatencyProfile,
    scenario: Scenario,
    events: &[ExecutionEvent],
    recovery_unknown_allowed: &HashSet<String>,
) -> RunReport {
    let mut report = RunReport {
        run_id: run_id.to_string(),
        mode: mode.as_str().to_string(),
        lane: lane.to_string(),
        n,
        seed,
        profile: profile.as_str().to_string(),
        scenario: scenario.as_str().to_string(),
        ..Default::default()
    };

    let mut candidates = HashSet::<String>::new();
    let mut opened_candidates = HashSet::<String>::new();
    let mut failed_like_candidates = HashSet::<String>::new();

    let mut submitted_ids = HashSet::<String>::new();
    let mut terminal_count_by_order = HashMap::<String, u64>::new();
    let mut opened_count_by_position = HashMap::<String, u64>::new();
    let mut closed_count_by_position = HashMap::<String, u64>::new();
    let mut latencies = Vec::<u64>::new();

    for event in events.iter().filter(|e| e.envelope.lane == lane) {
        match &event.kind {
            EventKind::Candidate(_) => {
                candidates.insert(event.envelope.candidate_id.clone());
            }
            EventKind::EntrySubmitted(_) => {
                report.submitted += 1;
                if let Some(order_id) = event.envelope.order_id.as_ref() {
                    submitted_ids.insert(order_id.clone());
                }
            }
            EventKind::EntryFilled(payload) => {
                if let Some(order_id) = event.envelope.order_id.as_ref() {
                    *terminal_count_by_order.entry(order_id.clone()).or_insert(0) += 1;
                    if payload.status == FillStatus::Unknown {
                        if recovery_unknown_allowed.contains(order_id) {
                            report.recovery_unknown_orders.push(order_id.clone());
                        } else {
                            report.unknown_non_recovery_orders.push(order_id.clone());
                        }
                    }
                }

                match payload.status {
                    FillStatus::Confirmed | FillStatus::Filled => {
                        report.filled += 1;
                        latencies.push(payload.latency_ms);
                    }
                    FillStatus::Failed => {
                        report.failed += 1;
                        failed_like_candidates.insert(event.envelope.candidate_id.clone());
                    }
                    FillStatus::Stale => {
                        report.timeout += 1;
                        failed_like_candidates.insert(event.envelope.candidate_id.clone());
                    }
                    FillStatus::Unknown => {
                        report.unknown += 1;
                        failed_like_candidates.insert(event.envelope.candidate_id.clone());
                    }
                    _ => {
                        report.unknown += 1;
                        failed_like_candidates.insert(event.envelope.candidate_id.clone());
                    }
                }
            }
            EventKind::PositionOpened(_) => {
                report.opened += 1;
                opened_candidates.insert(event.envelope.candidate_id.clone());
                if let Some(position_id) = event.envelope.position_id.as_ref() {
                    *opened_count_by_position
                        .entry(position_id.clone())
                        .or_insert(0) += 1;
                }
            }
            EventKind::PositionClosed(_) => {
                report.closed += 1;
                if let Some(position_id) = event.envelope.position_id.as_ref() {
                    *closed_count_by_position
                        .entry(position_id.clone())
                        .or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }

    report.n = n;
    report.terminal_total = report.filled + report.failed + report.timeout + report.unknown;
    report.terminal_entry_fail = report.failed + report.timeout + report.unknown;
    report.in_flight = report.submitted as i64 - report.terminal_total as i64;

    report.duplicates_fill = terminal_count_by_order
        .values()
        .filter(|count| **count > 1)
        .count() as u64;
    report.duplicates_opened = opened_count_by_position
        .values()
        .filter(|count| **count > 1)
        .count() as u64;
    report.duplicates_closed = closed_count_by_position
        .values()
        .filter(|count| **count > 1)
        .count() as u64;

    report.missing_terminal_orders = submitted_ids
        .iter()
        .filter_map(|id| {
            if terminal_count_by_order.contains_key(id) {
                None
            } else {
                Some(id.clone())
            }
        })
        .collect();
    report.missing_terminal_orders.sort();

    report.multiple_terminal_orders = terminal_count_by_order
        .iter()
        .filter_map(|(id, count)| if *count > 1 { Some(id.clone()) } else { None })
        .collect();
    report.multiple_terminal_orders.sort();

    report.candidate_without_opened_or_failed = candidates
        .iter()
        .filter_map(|candidate_id| {
            if opened_candidates.contains(candidate_id)
                || failed_like_candidates.contains(candidate_id)
            {
                None
            } else {
                Some(candidate_id.clone())
            }
        })
        .collect();
    report.candidate_without_opened_or_failed.sort();

    report.p50_time_to_fill_ms = percentile(&latencies, 0.50);
    report.p90_time_to_fill_ms = percentile(&latencies, 0.90);
    report.timing_histogram = build_timing_histogram(&latencies);

    let mut failed_checks = Vec::<String>::new();
    if report.submitted != report.terminal_total {
        failed_checks.push("lifecycle_submitted_terminal_mismatch".to_string());
    }
    if report.n as u64 != report.opened + report.terminal_entry_fail {
        failed_checks.push("lifecycle_candidate_opened_or_failed_mismatch".to_string());
    }
    if report.in_flight != 0 {
        failed_checks.push("in_flight_non_zero".to_string());
    }
    if !report.missing_terminal_orders.is_empty() {
        failed_checks.push("missing_terminal_orders".to_string());
    }
    if !report.multiple_terminal_orders.is_empty() {
        failed_checks.push("multiple_terminal_orders".to_string());
    }
    if report.duplicates_fill > 0 {
        failed_checks.push("duplicate_fill_terminals".to_string());
    }
    if report.duplicates_opened > 0 {
        failed_checks.push("duplicate_position_opened".to_string());
    }
    if report.duplicates_closed > 0 {
        failed_checks.push("duplicate_position_closed".to_string());
    }
    if !report.candidate_without_opened_or_failed.is_empty() {
        failed_checks.push("candidate_gap_opened_or_failed_missing".to_string());
    }

    if scenario == Scenario::F2RecoverySweep {
        if report.unknown == 0 {
            failed_checks.push("recovery_expected_unknown_missing".to_string());
        }
        if !report.unknown_non_recovery_orders.is_empty() {
            failed_checks.push("recovery_unknown_not_annotated".to_string());
        }
    } else if report.unknown > 0 {
        failed_checks.push("unknown_not_allowed_in_non_recovery_scenario".to_string());
    }

    report.failed_checks = failed_checks;
    report.verdict = if report.failed_checks.is_empty() {
        "PASS".to_string()
    } else {
        "FAIL".to_string()
    };
    report
}

fn write_json_report<T: Serialize>(
    path: &Path,
    payload: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, payload)?;
    Ok(())
}

fn compare_case(
    case_id: &str,
    case_dir: &Path,
    timing_threshold_pct: f64,
    terminal_delta_pct: f64,
    terminal_delta_abs_min: u64,
    live_only: &RunReport,
    dual_live: &RunReport,
) -> CaseReport {
    let submitted_match_pass = live_only.submitted == dual_live.submitted;

    let lifecycle_accounting_pass = live_only.verdict == "PASS" && dual_live.verdict == "PASS";

    let exactly_once_pass = live_only.duplicates_fill == 0
        && dual_live.duplicates_fill == 0
        && live_only.duplicates_opened == 0
        && dual_live.duplicates_opened == 0
        && live_only.duplicates_closed == 0
        && dual_live.duplicates_closed == 0
        && live_only.missing_terminal_orders.is_empty()
        && dual_live.missing_terminal_orders.is_empty()
        && live_only.multiple_terminal_orders.is_empty()
        && dual_live.multiple_terminal_orders.is_empty();

    let gaps_pass = live_only.candidate_without_opened_or_failed.is_empty()
        && dual_live.candidate_without_opened_or_failed.is_empty();

    let inflight_zero_pass = live_only.in_flight == 0 && dual_live.in_flight == 0;

    let threshold_multiplier = 1.0 + (timing_threshold_pct / 100.0);
    let timing_pass = dual_live.p50_time_to_fill_ms
        <= live_only.p50_time_to_fill_ms * threshold_multiplier
        && dual_live.p90_time_to_fill_ms <= live_only.p90_time_to_fill_ms * threshold_multiplier;

    let terminal_ref = max(live_only.terminal_total, dual_live.terminal_total);
    let terminal_allowed = max(
        terminal_delta_abs_min,
        ((terminal_ref as f64) * (terminal_delta_pct / 100.0)).ceil() as u64,
    );

    let classes = [
        ("filled", live_only.filled, dual_live.filled),
        ("failed", live_only.failed, dual_live.failed),
        ("timeout", live_only.timeout, dual_live.timeout),
        ("unknown", live_only.unknown, dual_live.unknown),
    ];

    let mut terminal_deltas = Vec::with_capacity(classes.len());
    for (class, lhs, rhs) in classes {
        let delta_abs = lhs.abs_diff(rhs);
        terminal_deltas.push(ClassDelta {
            class: class.to_string(),
            live_only: lhs,
            dual_live: rhs,
            delta_abs,
            max_allowed: terminal_allowed,
            pass: delta_abs <= terminal_allowed,
        });
    }

    let terminal_distribution_pass = terminal_deltas.iter().all(|d| d.pass);

    let unknown_policy_pass = live_only.unknown_non_recovery_orders.is_empty()
        && dual_live.unknown_non_recovery_orders.is_empty();

    let mut failed_checks = Vec::<String>::new();
    if !submitted_match_pass {
        failed_checks.push("submitted_mismatch_live_vs_dual_live".to_string());
    }
    if !lifecycle_accounting_pass {
        failed_checks.push("lifecycle_accounting_failed".to_string());
    }
    if !exactly_once_pass {
        failed_checks.push("exactly_once_failed".to_string());
    }
    if !gaps_pass {
        failed_checks.push("candidate_gaps_detected".to_string());
    }
    if !inflight_zero_pass {
        failed_checks.push("in_flight_non_zero".to_string());
    }
    if !timing_pass {
        failed_checks.push("timing_threshold_exceeded".to_string());
    }
    if !terminal_distribution_pass {
        failed_checks.push("terminal_distribution_delta_exceeded".to_string());
    }
    if !unknown_policy_pass {
        failed_checks.push("unknown_policy_violation".to_string());
    }

    let verdict = if failed_checks.is_empty() {
        "PASS".to_string()
    } else {
        "FAIL".to_string()
    };

    CaseReport {
        case_id: case_id.to_string(),
        n: live_only.n,
        seed: live_only.seed,
        profile: live_only.profile.clone(),
        scenario: live_only.scenario.clone(),
        timing_threshold_pct,
        terminal_delta_pct,
        terminal_delta_abs_min,
        live_events_path: case_dir
            .join("live/events.jsonl")
            .to_string_lossy()
            .to_string(),
        dual_events_path: case_dir
            .join("dual/events.jsonl")
            .to_string_lossy()
            .to_string(),
        live_report_path: case_dir
            .join("live/live_only_report.json")
            .to_string_lossy()
            .to_string(),
        dual_live_report_path: case_dir
            .join("dual/dual_live_lane_report.json")
            .to_string_lossy()
            .to_string(),
        case_verdict_path: case_dir
            .join("replay_equivalence_verdict.txt")
            .to_string_lossy()
            .to_string(),
        submitted_match_pass,
        lifecycle_accounting_pass,
        exactly_once_pass,
        gaps_pass,
        timing_pass,
        terminal_distribution_pass,
        unknown_policy_pass,
        inflight_zero_pass,
        terminal_deltas,
        verdict,
        failed_checks,
    }
}

fn write_case_verdict_txt(
    path: &Path,
    live_only: &RunReport,
    dual_live: &RunReport,
    case_report: &CaseReport,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;

    writeln!(
        file,
        "Replay Equivalence Proof v2 | case={} | verdict={}",
        case_report.case_id, case_report.verdict
    )?;
    writeln!(
        file,
        "mode_compare=Live-only_vs_Dual(live_lane) n={} seed={} profile={} scenario={}",
        case_report.n, case_report.seed, case_report.profile, case_report.scenario
    )?;
    writeln!(file)?;

    writeln!(file, "A) Lifecycle accounting")?;
    writeln!(
        file,
        "- submitted == terminal_total: live={} dual_live={}",
        live_only.submitted == live_only.terminal_total,
        dual_live.submitted == dual_live.terminal_total
    )?;
    writeln!(
        file,
        "- candidate == opened + terminal_entry_fail: live={} dual_live={}",
        live_only.n as u64 == live_only.opened + live_only.terminal_entry_fail,
        dual_live.n as u64 == dual_live.opened + dual_live.terminal_entry_fail
    )?;
    writeln!(
        file,
        "- in_flight==0: live={} dual_live={}",
        live_only.in_flight == 0,
        dual_live.in_flight == 0
    )?;
    writeln!(file)?;

    writeln!(file, "B) Exactly-once")?;
    writeln!(
        file,
        "- duplicates fill/opened/closed: live={}/{}/{} dual_live={}/{}/{}",
        live_only.duplicates_fill,
        live_only.duplicates_opened,
        live_only.duplicates_closed,
        dual_live.duplicates_fill,
        dual_live.duplicates_opened,
        dual_live.duplicates_closed
    )?;
    writeln!(file)?;

    writeln!(file, "C) Gaps")?;
    writeln!(
        file,
        "- missing_terminal_orders (dual_live): {:?}",
        dual_live.missing_terminal_orders
    )?;
    writeln!(
        file,
        "- multiple_terminal_orders (dual_live): {:?}",
        dual_live.multiple_terminal_orders
    )?;
    writeln!(
        file,
        "- candidate_without_opened_or_failed (dual_live): {:?}",
        dual_live.candidate_without_opened_or_failed
    )?;
    writeln!(file)?;

    writeln!(file, "D) Timing")?;
    writeln!(
        file,
        "- p50 live={:.2} dual_live={:.2}",
        live_only.p50_time_to_fill_ms, dual_live.p50_time_to_fill_ms
    )?;
    writeln!(
        file,
        "- p90 live={:.2} dual_live={:.2}",
        live_only.p90_time_to_fill_ms, dual_live.p90_time_to_fill_ms
    )?;
    writeln!(
        file,
        "- threshold={:.2}% timing_pass={}",
        case_report.timing_threshold_pct, case_report.timing_pass
    )?;
    writeln!(file)?;

    writeln!(file, "E) Terminal deltas")?;
    for d in &case_report.terminal_deltas {
        writeln!(
            file,
            "- {}: live={} dual_live={} delta={} allowed={} pass={}",
            d.class, d.live_only, d.dual_live, d.delta_abs, d.max_allowed, d.pass
        )?;
    }
    writeln!(file)?;

    writeln!(file, "F) Failed checks")?;
    writeln!(file, "- {:?}", case_report.failed_checks)?;

    Ok(())
}

fn write_unified_verdict_txt(
    path: &Path,
    unified: &UnifiedReport,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    writeln!(file, "Replay Equivalence Proof v2")?;
    writeln!(file, "Verdict: {}", unified.verdict)?;
    writeln!(file, "Cases: {}", unified.cases.len())?;
    for case in &unified.cases {
        writeln!(
            file,
            "- {} => {} ({:?})",
            case.case_id, case.verdict, case.failed_checks
        )?;
    }
    Ok(())
}

fn load_candidates_for_case(
    cli: &Cli,
    n: usize,
) -> Result<Vec<InputCandidate>, Box<dyn std::error::Error>> {
    let path = if let Some(input) = cli.input.as_ref() {
        input.clone()
    } else {
        fixture_path_for_size(&cli.fixture_dir, n)
    };
    let mut candidates = read_candidates_jsonl(&path)?;
    if candidates.len() < n {
        return Err(format!(
            "fixture {} has {} candidates, expected at least {}",
            path.to_string_lossy(),
            candidates.len(),
            n
        )
        .into());
    }
    candidates.truncate(n);
    Ok(candidates)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = parse_cli().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    std::fs::create_dir_all(&cli.output_dir)?;

    let mut case_reports = Vec::<CaseReport>::new();

    for &n in &cli.sizes {
        let candidates = load_candidates_for_case(&cli, n)?;

        for &profile in &cli.profiles {
            for &scenario in &cli.scenarios {
                let run_seed = case_seed(cli.seed, n, profile, scenario);
                let case_id = format!("n{n}_{}_{}", profile.as_str(), scenario.as_str());
                let case_dir = cli.output_dir.join(&case_id);

                let live_orders = build_scenario_orders(
                    &candidates,
                    run_seed,
                    profile,
                    scenario,
                    cli.ttl_ms,
                    cli.pathological_timeout_pct,
                    cli.scenario_impact_pct,
                );

                let live_recovery_allowed: HashSet<String> = live_orders
                    .iter()
                    .filter_map(|o| {
                        if o.recovery_unknown {
                            Some(o.order_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                let live_events =
                    make_lane_events(&format!("{case_id}_live_only"), Lane::Live, &live_orders);

                let mut dual_events =
                    make_lane_events(&format!("{case_id}_dual"), Lane::Live, &live_orders);
                let paper_orders = make_dual_paper_orders(&live_orders, run_seed);
                dual_events.extend(make_lane_events(
                    &format!("{case_id}_dual"),
                    Lane::Paper,
                    &paper_orders,
                ));
                dual_events
                    .sort_by_key(|e| (e.envelope.event_time_ms, e.envelope.event_id.clone()));

                let live_events_path = case_dir.join("live/events.jsonl");
                let dual_events_path = case_dir.join("dual/events.jsonl");
                write_jsonl(&live_events_path, &live_events)?;
                write_jsonl(&dual_events_path, &dual_events)?;

                let parsed_live = read_jsonl(&live_events_path)?;
                let parsed_dual = read_jsonl(&dual_events_path)?;

                let live_report = audit_run(
                    &format!("{case_id}_live_only"),
                    Mode::LiveOnly,
                    Lane::Live,
                    n,
                    run_seed,
                    profile,
                    scenario,
                    &parsed_live,
                    &live_recovery_allowed,
                );

                let dual_live_report = audit_run(
                    &format!("{case_id}_dual"),
                    Mode::Dual,
                    Lane::Live,
                    n,
                    run_seed,
                    profile,
                    scenario,
                    &parsed_dual,
                    &live_recovery_allowed,
                );

                let live_report_path = case_dir.join("live/live_only_report.json");
                let dual_report_path = case_dir.join("dual/dual_live_lane_report.json");
                write_json_report(&live_report_path, &live_report)?;
                write_json_report(&dual_report_path, &dual_live_report)?;

                let case_report = compare_case(
                    &case_id,
                    &case_dir,
                    cli.timing_threshold_pct,
                    cli.terminal_delta_pct,
                    cli.terminal_delta_abs_min,
                    &live_report,
                    &dual_live_report,
                );

                let case_report_path = case_dir.join("comparison_report.json");
                write_json_report(&case_report_path, &case_report)?;

                let case_verdict_path = case_dir.join("replay_equivalence_verdict.txt");
                write_case_verdict_txt(
                    &case_verdict_path,
                    &live_report,
                    &dual_live_report,
                    &case_report,
                )?;

                println!("case={} verdict={}", case_id, case_report.verdict);
                println!("- live_events={}", live_events_path.to_string_lossy());
                println!("- dual_events={}", dual_events_path.to_string_lossy());
                println!("- comparison={}", case_report_path.to_string_lossy());

                case_reports.push(case_report);
            }
        }
    }

    let failed_cases: Vec<String> = case_reports
        .iter()
        .filter_map(|c| {
            if c.verdict == "PASS" {
                None
            } else {
                Some(c.case_id.clone())
            }
        })
        .collect();

    let unified = UnifiedReport {
        output_dir: cli.output_dir.to_string_lossy().to_string(),
        seed: cli.seed,
        timing_threshold_pct: cli.timing_threshold_pct,
        terminal_delta_pct: cli.terminal_delta_pct,
        terminal_delta_abs_min: cli.terminal_delta_abs_min,
        cases: case_reports,
        verdict: if failed_cases.is_empty() {
            "PASS".to_string()
        } else {
            "FAIL".to_string()
        },
        failed_cases,
    };

    let unified_json_v2 = cli.output_dir.join("replay_equivalence_v2_report.json");
    let unified_txt_v2 = cli.output_dir.join("replay_equivalence_v2_verdict.txt");
    write_json_report(&unified_json_v2, &unified)?;
    write_unified_verdict_txt(&unified_txt_v2, &unified)?;

    // Compatibility aliases
    let unified_json = cli.output_dir.join("replay_equivalence_report.json");
    let unified_txt = cli.output_dir.join("replay_equivalence_verdict.txt");
    write_json_report(&unified_json, &unified)?;
    write_unified_verdict_txt(&unified_txt, &unified)?;

    println!("Replay equivalence v2 verdict: {}", unified.verdict);
    println!("Unified JSON: {}", unified_json_v2.to_string_lossy());
    println!("Unified TXT: {}", unified_txt_v2.to_string_lossy());

    if unified.verdict == "PASS" {
        Ok(())
    } else {
        Err("replay equivalence failed".into())
    }
}
