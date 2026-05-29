//! Comparison Report Generator — lane-aware execution analysis.
//!
//! Reads JSONL event files produced by `EventWriter` and generates
//! a summary report comparing execution lanes without collapsing Shadow into Paper.
//!
//! Metrics computed:
//! - Latency: fill_time - submit_time (median, p95, p99 per lane)
//! - Slippage: fill_price vs quote_price per lane
//! - Failure rate: FillStatus::Failed count per lane
//! - Decision concordance: % of commands identical across lanes

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::events::schema::{EventKind, ExecutionEvent};
use crate::execution::backend::{FillStatus, Lane};

// ─── Report types ───────────────────────────────────────────────────────────

/// Summary comparison report for a dual-mode run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub run_id: String,
    pub total_events: u64,
    pub paper: LaneReport,
    pub live: LaneReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<LaneReport>,
    /// Percentage of candidates that were sampled into the live lane.
    pub live_sampling_rate: f64,
    /// Decision concordance: % of positions where both lanes made identical commands.
    pub decision_concordance_pct: Option<f64>,
    /// Position timeline closure quality across lanes.
    pub no_gap_pct: Option<f64>,
    /// Safety compliance: WAIT commands not emitted under HIGH stress/stale contexts.
    pub safety_compliance_pct: Option<f64>,
    /// Anti-zombie command reject rate (epoch/ttl/priority rejects).
    pub anti_zombie_reject_rate_pct: Option<f64>,
}

/// Per-lane performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaneReport {
    pub lane: String,
    pub total_entries: u64,
    pub total_fills: u64,
    pub total_exits: u64,
    pub positions_opened: u64,
    pub positions_closed: u64,
    /// Latency stats (fill_time - submit_time) in ms.
    pub latency: LatencyStats,
    /// Slippage stats (fill_price / quote_price - 1.0) in bps.
    pub slippage_bps: SlippageStats,
    /// Fill failure rate (FillStatus::Failed / total fills).
    pub failure_rate_pct: f64,
    pub failed_count: u64,
    /// Stress event count.
    pub stress_changes: u64,
    /// Oracle stale event count.
    pub oracle_stale_events: u64,
    /// Aggregate close/PnL evidence for this lane.
    pub close_economics: CloseEconomicsStats,
}

/// Latency distribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LatencyStats {
    pub count: u64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub mean_ms: f64,
}

/// Slippage distribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlippageStats {
    pub count: u64,
    pub min_bps: f64,
    pub max_bps: f64,
    pub median_bps: f64,
    pub p95_bps: f64,
    pub mean_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloseEconomicsStats {
    pub positions_with_economics: u64,
    pub total_entry_value_sol: f64,
    pub total_exit_value_sol: f64,
    pub total_gross_pnl_sol: f64,
    pub total_net_pnl_sol: f64,
    pub total_estimated_costs_sol: f64,
}

// ─── Generator ──────────────────────────────────────────────────────────────

/// Generates a ComparisonReport from a JSONL event log file.
pub fn generate_comparison_report(
    jsonl_path: &Path,
) -> Result<ComparisonReport, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(jsonl_path)?;
    let reader = BufReader::new(file);

    let mut total_events = 0u64;
    let mut run_id = String::new();

    // Collectors per lane
    let mut paper_latencies: Vec<f64> = Vec::new();
    let mut live_latencies: Vec<f64> = Vec::new();
    let mut shadow_latencies: Vec<f64> = Vec::new();
    let mut paper_slippages: Vec<f64> = Vec::new();
    let mut live_slippages: Vec<f64> = Vec::new();
    let mut shadow_slippages: Vec<f64> = Vec::new();
    let mut quote_price_by_order: HashMap<(Lane, String), f64> = HashMap::new();
    let mut commands_by_lane_candidate: HashMap<(Lane, String), Vec<String>> = HashMap::new();
    let mut wait_commands_total = 0u64;
    let mut wait_commands_safety_violations = 0u64;
    let mut recent_high_stress_or_stale: HashMap<(Lane, String), bool> = HashMap::new();
    let mut total_applied_commands = 0u64;
    let mut anti_zombie_rejects = 0u64;

    let mut paper_counts = LaneCounts::default();
    let mut live_counts = LaneCounts::default();
    let mut shadow_counts = LaneCounts::default();
    let mut paper_close_economics = CloseEconomicsStats::default();
    let mut live_close_economics = CloseEconomicsStats::default();
    let mut shadow_close_economics = CloseEconomicsStats::default();

    let mut all_candidates: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut live_candidates: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let event: ExecutionEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Skipping malformed JSONL line");
                continue;
            }
        };

        total_events += 1;
        if run_id.is_empty() {
            run_id = event.envelope.run_id.clone();
        }

        if matches!(&event.kind, EventKind::NewPoolDetected(_)) {
            continue;
        }

        let lane = event.envelope.lane;
        let counts = match lane {
            Lane::Paper | Lane::Single => &mut paper_counts,
            Lane::Live => &mut live_counts,
            Lane::Shadow => &mut shadow_counts,
        };

        // Track per candidate
        all_candidates.insert(event.envelope.candidate_id.clone());
        if lane == Lane::Live {
            live_candidates.insert(event.envelope.candidate_id.clone());
        }

        match &event.kind {
            EventKind::Candidate(_) => {
                counts.candidates += 1;
            }
            EventKind::EntrySubmitted(payload) => {
                counts.entries += 1;
                if let Some(ref order_id) = event.envelope.order_id {
                    if let Some(price_ref) = payload
                        .send_params
                        .as_ref()
                        .and_then(|p| p.get("quote_price_ref"))
                        .and_then(|v| v.as_f64())
                    {
                        quote_price_by_order.insert((lane, order_id.clone()), price_ref);
                    }
                }
            }
            EventKind::EntryFilled(payload) => {
                counts.fills += 1;
                if payload.status == FillStatus::Failed {
                    counts.failed += 1;
                }
                // Record latency
                let lat = payload.latency_ms as f64;
                match lane {
                    Lane::Paper | Lane::Single => paper_latencies.push(lat),
                    Lane::Live => live_latencies.push(lat),
                    Lane::Shadow => shadow_latencies.push(lat),
                }
                if let Some(ref order_id) = event.envelope.order_id {
                    if let Some(quote_ref_price) =
                        quote_price_by_order.get(&(lane, order_id.clone()))
                    {
                        if *quote_ref_price > 0.0 {
                            let slippage_bps = ((payload.fill_price_effective / *quote_ref_price)
                                - 1.0)
                                * 10_000.0;
                            match lane {
                                Lane::Paper | Lane::Single => paper_slippages.push(slippage_bps),
                                Lane::Live => live_slippages.push(slippage_bps),
                                Lane::Shadow => shadow_slippages.push(slippage_bps),
                            }
                        }
                    }
                }
            }
            EventKind::ExitSubmitted(_) => {
                counts.exits += 1;
            }
            EventKind::ExitFilled(payload) => {
                counts.fills += 1;
                if payload.status == FillStatus::Failed {
                    counts.failed += 1;
                }
            }
            EventKind::PositionOpened(_) => {
                counts.positions_opened += 1;
            }
            EventKind::PositionClosed(payload) => {
                counts.positions_closed += 1;
                let economics = match lane {
                    Lane::Paper | Lane::Single => &mut paper_close_economics,
                    Lane::Live => &mut live_close_economics,
                    Lane::Shadow => &mut shadow_close_economics,
                };
                if payload.entry_value_sol.is_some()
                    && payload.exit_value_sol.is_some()
                    && payload.gross_pnl_sol.is_some()
                    && payload.net_pnl_sol.is_some()
                    && payload.estimated_costs_sol.is_some()
                {
                    economics.positions_with_economics += 1;
                }
                economics.total_entry_value_sol += payload.entry_value_sol.unwrap_or(0.0);
                economics.total_exit_value_sol += payload.exit_value_sol.unwrap_or(0.0);
                economics.total_gross_pnl_sol += payload.gross_pnl_sol.unwrap_or(0.0);
                economics.total_net_pnl_sol += payload.net_pnl_sol.unwrap_or(0.0);
                economics.total_estimated_costs_sol += payload.estimated_costs_sol.unwrap_or(0.0);
            }
            EventKind::ExecutionStressChanged(payload) => {
                counts.stress_changes += 1;
                if payload.new_bucket == crate::execution::backend::StressBucket::High {
                    recent_high_stress_or_stale
                        .insert((lane, event.envelope.candidate_id.clone()), true);
                }
            }
            EventKind::OracleStale(_) => {
                counts.oracle_stale += 1;
                recent_high_stress_or_stale
                    .insert((lane, event.envelope.candidate_id.clone()), true);
            }
            EventKind::ControlCommandIssued(payload) => {
                commands_by_lane_candidate
                    .entry((lane, event.envelope.candidate_id.clone()))
                    .or_default()
                    .push(format!("{}:{:?}", payload.directive, payload.fraction_bps));
                if payload.directive.eq_ignore_ascii_case("wait") {
                    wait_commands_total += 1;
                    if recent_high_stress_or_stale
                        .get(&(lane, event.envelope.candidate_id.clone()))
                        .copied()
                        .unwrap_or(false)
                    {
                        wait_commands_safety_violations += 1;
                    }
                }
            }
            EventKind::ControlCommandApplied(payload) => {
                total_applied_commands += 1;
                if !payload.accepted {
                    if let Some(reason) = payload.reject_reason.as_deref() {
                        if matches!(reason, "epoch_mismatch" | "ttl_expired" | "priority_lock") {
                            anti_zombie_rejects += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Compute stats
    let paper_lane = build_lane_report(
        "paper",
        &paper_counts,
        &mut paper_latencies,
        &mut paper_slippages,
        &paper_close_economics,
    );
    let live_lane = build_lane_report(
        "live",
        &live_counts,
        &mut live_latencies,
        &mut live_slippages,
        &live_close_economics,
    );
    let shadow_lane = build_lane_report(
        "shadow",
        &shadow_counts,
        &mut shadow_latencies,
        &mut shadow_slippages,
        &shadow_close_economics,
    );

    let live_sampling_rate = if all_candidates.is_empty() {
        0.0
    } else {
        live_candidates.len() as f64 / all_candidates.len() as f64
    };

    info!(
        run_id = %run_id,
        total_events = total_events,
        paper_entries = paper_counts.entries,
        live_entries = live_counts.entries,
        shadow_entries = shadow_counts.entries,
        live_sampling_pct = format!("{:.1}%", live_sampling_rate * 100.0),
        "Comparison report generated"
    );

    let mut concordance_matches = 0u64;
    let mut concordance_total = 0u64;
    let mut seen_candidates = std::collections::HashSet::new();
    for ((lane, candidate_id), _) in &commands_by_lane_candidate {
        if *lane == Lane::Live && seen_candidates.insert(candidate_id.clone()) {
            let paper = commands_by_lane_candidate
                .get(&(Lane::Paper, candidate_id.clone()))
                .or_else(|| commands_by_lane_candidate.get(&(Lane::Single, candidate_id.clone())));
            let live = commands_by_lane_candidate.get(&(Lane::Live, candidate_id.clone()));
            if let (Some(paper_seq), Some(live_seq)) = (paper, live) {
                concordance_total += 1;
                if paper_seq == live_seq {
                    concordance_matches += 1;
                }
            }
        }
    }
    let decision_concordance_pct = if concordance_total == 0 {
        None
    } else {
        Some(concordance_matches as f64 / concordance_total as f64 * 100.0)
    };

    let total_positions_opened = paper_counts.positions_opened + live_counts.positions_opened;
    let total_positions_closed = paper_counts.positions_closed + live_counts.positions_closed;
    let no_gap_pct = if total_positions_opened == 0 {
        None
    } else {
        Some(total_positions_closed as f64 / total_positions_opened as f64 * 100.0)
    };

    let safety_compliance_pct = if wait_commands_total == 0 {
        None
    } else {
        Some(
            (wait_commands_total.saturating_sub(wait_commands_safety_violations)) as f64
                / wait_commands_total as f64
                * 100.0,
        )
    };
    let anti_zombie_reject_rate_pct = if total_applied_commands == 0 {
        None
    } else {
        Some(anti_zombie_rejects as f64 / total_applied_commands as f64 * 100.0)
    };

    Ok(ComparisonReport {
        run_id,
        total_events,
        paper: paper_lane,
        live: live_lane,
        shadow: (shadow_counts.entries > 0
            || shadow_counts.fills > 0
            || shadow_counts.exits > 0
            || shadow_counts.positions_opened > 0
            || shadow_counts.positions_closed > 0
            || shadow_counts.stress_changes > 0
            || shadow_counts.oracle_stale > 0)
            .then_some(shadow_lane),
        live_sampling_rate,
        decision_concordance_pct,
        no_gap_pct,
        safety_compliance_pct,
        anti_zombie_reject_rate_pct,
    })
}

/// Generate a report from all JSONL files in a directory (handles rotation).
pub fn generate_comparison_report_from_dir(
    dir: &Path,
) -> Result<ComparisonReport, Box<dyn std::error::Error>> {
    let mut all_events: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            let file = std::fs::File::open(&path)?;
            for line in BufReader::new(file).lines() {
                all_events.push(line?);
            }
        }
    }

    // Write to temp file and then parse
    let tmp = std::env::temp_dir().join("comparison_report_tmp.jsonl");
    std::fs::write(&tmp, all_events.join("\n"))?;
    let result = generate_comparison_report(&tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

// ─── Internal helpers ───────────────────────────────────────────────────────

#[derive(Default)]
struct LaneCounts {
    candidates: u64,
    entries: u64,
    fills: u64,
    exits: u64,
    failed: u64,
    positions_opened: u64,
    positions_closed: u64,
    stress_changes: u64,
    oracle_stale: u64,
}

fn build_lane_report(
    lane_name: &str,
    counts: &LaneCounts,
    latencies: &mut Vec<f64>,
    slippages: &mut Vec<f64>,
    close_economics: &CloseEconomicsStats,
) -> LaneReport {
    let failure_rate = if counts.fills > 0 {
        counts.failed as f64 / counts.fills as f64 * 100.0
    } else {
        0.0
    };

    LaneReport {
        lane: lane_name.to_string(),
        total_entries: counts.entries,
        total_fills: counts.fills,
        total_exits: counts.exits,
        positions_opened: counts.positions_opened,
        positions_closed: counts.positions_closed,
        latency: compute_percentile_stats(latencies),
        slippage_bps: compute_slippage_stats(slippages),
        failure_rate_pct: failure_rate,
        failed_count: counts.failed,
        stress_changes: counts.stress_changes,
        oracle_stale_events: counts.oracle_stale,
        close_economics: close_economics.clone(),
    }
}

fn compute_percentile_stats(values: &mut Vec<f64>) -> LatencyStats {
    if values.is_empty() {
        return LatencyStats::default();
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    let sum: f64 = values.iter().sum();

    LatencyStats {
        count: n as u64,
        min_ms: values[0],
        max_ms: values[n - 1],
        median_ms: percentile(values, 0.50),
        p95_ms: percentile(values, 0.95),
        p99_ms: percentile(values, 0.99),
        mean_ms: sum / n as f64,
    }
}

fn compute_slippage_stats(values: &mut Vec<f64>) -> SlippageStats {
    if values.is_empty() {
        return SlippageStats::default();
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    let sum: f64 = values.iter().sum();

    SlippageStats {
        count: n as u64,
        min_bps: values[0],
        max_bps: values[n - 1],
        median_bps: percentile(values, 0.50),
        p95_bps: percentile(values, 0.95),
        mean_bps: sum / n as f64,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::schema::*;
    use crate::execution::backend::*;
    use tempfile::TempDir;

    fn write_events_to_file(events: &[ExecutionEvent], dir: &Path) -> std::path::PathBuf {
        let path = dir.join("test_events.jsonl");
        let content: Vec<String> = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        std::fs::write(&path, content.join("\n")).unwrap();
        path
    }

    fn make_entry_submitted(lane: Lane, cid: &str) -> ExecutionEvent {
        let mut env = EventEnvelope::new("run-1".to_string(), lane, cid.to_string(), 1000);
        env.order_id = Some("ord-1".to_string());
        ExecutionEvent::new(
            env,
            EventKind::EntrySubmitted(EntrySubmittedPayload {
                side: OrderSide::Entry,
                planned_delay_ms: Some(300),
                send_params: None,
                amount_lamports: 10_000_000,
                min_tokens_out: 1000,
            }),
        )
    }

    fn make_entry_filled(
        lane: Lane,
        cid: &str,
        latency: u64,
        status: FillStatus,
    ) -> ExecutionEvent {
        let mut env =
            EventEnvelope::new("run-1".to_string(), lane, cid.to_string(), 1000 + latency);
        env.order_id = Some("ord-1".to_string());
        ExecutionEvent::new(
            env,
            EventKind::EntryFilled(EntryFilledPayload {
                fill_time_ms: 1000 + latency,
                fill_price_effective: 0.001,
                fill_qty: 1000,
                quote_id_used: "q-1".to_string(),
                status,
                latency_ms: latency,
            }),
        )
    }

    fn make_position_closed(
        lane: Lane,
        cid: &str,
        entry_value_sol: f64,
        exit_value_sol: f64,
        gross_pnl_sol: f64,
        net_pnl_sol: f64,
        estimated_costs_sol: f64,
    ) -> ExecutionEvent {
        let mut env = EventEnvelope::new("run-1".to_string(), lane, cid.to_string(), 2_000);
        env.position_id = Some(format!("pos-{cid}"));
        ExecutionEvent::new(
            env,
            EventKind::PositionClosed(PositionClosedPayload {
                final_pnl: gross_pnl_sol,
                final_pnl_pct: if entry_value_sol > 0.0 {
                    (gross_pnl_sol / entry_value_sol) * 100.0
                } else {
                    0.0
                },
                entry_value_sol: Some(entry_value_sol),
                exit_value_sol: Some(exit_value_sol),
                gross_pnl_sol: Some(gross_pnl_sol),
                net_pnl_sol: Some(net_pnl_sol),
                estimated_costs_sol: Some(estimated_costs_sol),
                duration_ms: 1_000,
                reason: CloseReason::Target,
                total_exits: 1,
            }),
        )
    }

    #[test]
    fn test_comparison_report_paper_only() {
        let tmp = TempDir::new().unwrap();

        let events = vec![
            make_entry_submitted(Lane::Paper, "cand-1"),
            make_entry_filled(Lane::Paper, "cand-1", 300, FillStatus::Filled),
            make_entry_submitted(Lane::Paper, "cand-2"),
            make_entry_filled(Lane::Paper, "cand-2", 250, FillStatus::Filled),
        ];

        let path = write_events_to_file(&events, tmp.path());
        let report = generate_comparison_report(&path).unwrap();

        assert_eq!(report.run_id, "run-1");
        assert_eq!(report.total_events, 4);
        assert_eq!(report.paper.total_entries, 2);
        assert_eq!(report.paper.total_fills, 2);
        assert_eq!(report.paper.latency.count, 2);
        assert_eq!(report.paper.failure_rate_pct, 0.0);

        // Live should be empty
        assert_eq!(report.live.total_entries, 0);
        assert_eq!(report.live.total_fills, 0);
        assert_eq!(report.live_sampling_rate, 0.0);
    }

    #[test]
    fn test_comparison_report_dual_mode() {
        let tmp = TempDir::new().unwrap();

        let events = vec![
            // Paper lane events
            make_entry_submitted(Lane::Paper, "cand-1"),
            make_entry_filled(Lane::Paper, "cand-1", 300, FillStatus::Filled),
            make_entry_submitted(Lane::Paper, "cand-2"),
            make_entry_filled(Lane::Paper, "cand-2", 250, FillStatus::Filled),
            // Live lane events (sampled candidate)
            make_entry_submitted(Lane::Live, "cand-1"),
            make_entry_filled(Lane::Live, "cand-1", 450, FillStatus::Filled),
        ];

        let path = write_events_to_file(&events, tmp.path());
        let report = generate_comparison_report(&path).unwrap();

        assert_eq!(report.paper.total_entries, 2);
        assert_eq!(report.paper.total_fills, 2);
        assert_eq!(report.live.total_entries, 1);
        assert_eq!(report.live.total_fills, 1);

        // cand-1 is in both lanes, cand-2 is paper-only → 50% sampling
        assert!((report.live_sampling_rate - 0.5).abs() < 0.01);

        // Paper median latency = 300 (sorted: [250, 300], p=0.5 -> idx=(0.5*1).round()=1 -> 300)
        assert_eq!(report.paper.latency.median_ms, 300.0);
        assert_eq!(report.live.latency.median_ms, 450.0);
    }

    #[test]
    fn test_comparison_report_with_failures() {
        let tmp = TempDir::new().unwrap();

        let events = vec![
            make_entry_submitted(Lane::Paper, "cand-1"),
            make_entry_filled(Lane::Paper, "cand-1", 300, FillStatus::Filled),
            make_entry_submitted(Lane::Paper, "cand-2"),
            make_entry_filled(Lane::Paper, "cand-2", 200, FillStatus::Failed),
        ];

        let path = write_events_to_file(&events, tmp.path());
        let report = generate_comparison_report(&path).unwrap();

        assert_eq!(report.paper.total_fills, 2);
        assert_eq!(report.paper.failed_count, 1);
        assert!((report.paper.failure_rate_pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_comparison_report_tracks_shadow_lane_separately() {
        let tmp = TempDir::new().unwrap();

        let events = vec![
            make_entry_submitted(Lane::Shadow, "cand-shadow"),
            make_entry_filled(Lane::Shadow, "cand-shadow", 175, FillStatus::Filled),
            make_entry_submitted(Lane::Paper, "cand-paper"),
            make_entry_filled(Lane::Paper, "cand-paper", 250, FillStatus::Filled),
        ];

        let path = write_events_to_file(&events, tmp.path());
        let report = generate_comparison_report(&path).unwrap();

        let shadow = report.shadow.expect("shadow lane report");
        assert_eq!(shadow.lane, "shadow");
        assert_eq!(shadow.total_entries, 1);
        assert_eq!(shadow.total_fills, 1);
        assert_eq!(report.paper.total_entries, 1);
        assert_eq!(report.live.total_entries, 0);
    }

    #[test]
    fn test_comparison_report_tracks_shadow_close_economics() {
        let tmp = TempDir::new().unwrap();

        let events = vec![
            make_entry_submitted(Lane::Shadow, "cand-shadow"),
            make_entry_filled(Lane::Shadow, "cand-shadow", 175, FillStatus::Filled),
            make_position_closed(Lane::Shadow, "cand-shadow", 1.0, 1.5, 0.5, 0.5, 0.0),
        ];

        let path = write_events_to_file(&events, tmp.path());
        let report = generate_comparison_report(&path).unwrap();

        let shadow = report.shadow.expect("shadow lane report");
        assert_eq!(shadow.close_economics.positions_with_economics, 1);
        assert!((shadow.close_economics.total_entry_value_sol - 1.0).abs() < 1e-9);
        assert!((shadow.close_economics.total_exit_value_sol - 1.5).abs() < 1e-9);
        assert!((shadow.close_economics.total_gross_pnl_sol - 0.5).abs() < 1e-9);
        assert!((shadow.close_economics.total_net_pnl_sol - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_computation() {
        let mut vals = vec![100.0, 200.0, 300.0, 400.0, 500.0];
        let stats = compute_percentile_stats(&mut vals);
        assert_eq!(stats.count, 5);
        assert_eq!(stats.min_ms, 100.0);
        assert_eq!(stats.max_ms, 500.0);
        assert_eq!(stats.median_ms, 300.0);
        assert_eq!(stats.mean_ms, 300.0);
    }
}
