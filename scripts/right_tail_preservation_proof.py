#!/usr/bin/env python3
"""PR-RTP-A0 offline right-tail preservation proof.

This script is offline-only. It reuses the row-level TimeStop V2 A2 replay
helpers to test a small predeclared family of right-tail preservation guards.
It never writes runtime logs and must not be imported by runtime code.
"""

from __future__ import annotations

import argparse
import copy
import csv
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

import time_stop_v2_counterfactual_lab as lab


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"

REPORT_DIR = Path("reports/selector")
REPORT_MD = Path("PLANS/AUDYT/RAPORT_RTP_A0_RIGHT_TAIL_PRESERVATION_20260628.md")
ADR_MD = Path("docs/ADR/ADR_8D_RTP_A0_RIGHT_TAIL_PRESERVATION_20260628.md")
SUMMARY_CSV = REPORT_DIR / "rtp_a0_guard_summary.csv"
STABILITY_CSV = REPORT_DIR / "rtp_a0_guard_stability.csv"
TAIL_CSV = REPORT_DIR / "rtp_a0_tail_preservation.csv"
INTERSECTION_CSV = REPORT_DIR / "rtp_a0_fixed_pair_intersection.csv"
R51_PLAN_MD = Path("PLANS/PLAN_R51_FULL_EVIDENCE_LOGGING_ONLY_20260628.md")

VOLUME_LOGS_ROOT = Path("/mnt/HC_Volume_105935807/logs")
LOCAL_LOGS_ROOT = Path("logs")

BENEFICIAL_CLASSES = {"saved_stop", "timeout_improved", "beneficial_exit"}
HARMFUL_CLASSES = {"cut_target", "harmful_exit"}
TARGET_CUT_CLASS = "cut_target"

COSTS_BPS = [0, 50, 100, 150, 200]
EARLY_HORIZONS_MS = [10000, 20000, 30000, 45000]
GUARDS = [
    "G0_NONE",
    "G1_STEADY_EARLY_STRENGTH",
    "G2_RECOVERY_AFTER_EARLY_DRAWDOWN",
    "G3_LOW_VOL_CONTINUATION",
    "G4_DELAYED_DECISION_4000",
    "G5_DELAYED_DECISION_8000",
]


@dataclass(frozen=True)
class Anchor:
    name: str
    mask_name: str
    target_bps: int
    stop_bps: int
    max_hold_ms: int


ANCHORS = [
    Anchor("M0_ALL / 6000 / -6000 / 120000", "M0_ALL", 6000, -6000, 120000),
    Anchor("M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000", "M4_CONFIRM_2_WINDOWS", 10000, -6000, 120000),
    Anchor("M7_CLASS_RESTRICTED / 10000 / -6000 / 60000", "M7_CLASS_RESTRICTED", 10000, -6000, 60000),
]


@dataclass
class ScopeData:
    scope: str
    records: list[dict[str, Any]]
    join_quality: dict[str, Any]
    input_paths: dict[str, str]
    load_stats: list[dict[str, Any]]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--r49-scope", default=R49_SCOPE)
    parser.add_argument("--r50-scope", default=R50_SCOPE)
    parser.add_argument("--local-logs-root", type=Path, default=LOCAL_LOGS_ROOT)
    parser.add_argument("--volume-logs-root", type=Path, default=VOLUME_LOGS_ROOT)
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    return parser.parse_args()


def identity(row: dict[str, Any]) -> tuple[str, str, str, str, int | None]:
    return (
        str(row.get("run_id") or ""),
        str(row.get("session_id") or ""),
        str(row.get("pool_id") or ""),
        str(row.get("base_mint") or ""),
        lab.int_or_none(row.get("entry_ts_ms")),
    )


def choose_shadow_dir(scope: str, local_logs_root: Path, volume_logs_root: Path) -> Path:
    candidates = [
        local_logs_root / "shadow_run" / scope,
        volume_logs_root / "shadow_run" / scope,
    ]
    for candidate in candidates:
        if (candidate / "shadow_exit_replay_v1.jsonl").exists():
            return candidate
    return candidates[0]


def stat_to_dict(stat: Any) -> dict[str, Any]:
    return {
        "path": getattr(stat, "path", ""),
        "rows": getattr(stat, "rows", 0),
        "malformed_rows": getattr(stat, "malformed_rows", 0),
    }


def load_scope(scope: str, local_logs_root: Path, volume_logs_root: Path) -> ScopeData:
    shadow_dir = choose_shadow_dir(scope, local_logs_root, volume_logs_root)
    paths = {
        "shadow_exit_replay": shadow_dir / "shadow_exit_replay_v1.jsonl",
        "shadow_lifecycle": shadow_dir / "shadow_lifecycle.jsonl",
        "probe_shadow_lifecycle": shadow_dir / "probe_shadow_lifecycle.jsonl",
    }
    replay_positions, replay_stats = lab.load_exit_replay_positions(paths["shadow_exit_replay"])
    lifecycle_positions, lifecycle_stats = lab.load_lifecycle_positions(
        paths["shadow_lifecycle"],
        paths["probe_shadow_lifecycle"],
    )
    joined, join_quality = lab.join_lifecycle(replay_positions, lifecycle_positions)
    records = lab.build_position_records(
        replay_positions,
        lifecycle_positions,
        joined,
        6000,
        -6000,
        120000,
        [4000, 8000, 12000],
    )
    lab.assign_chronological_terciles(records)
    return ScopeData(
        scope=scope,
        records=records,
        join_quality=join_quality,
        input_paths={name: str(path) for name, path in paths.items()},
        load_stats=[stat_to_dict(replay_stats), *[stat_to_dict(stat) for stat in lifecycle_stats]],
    )


def exact_join_rate(data: ScopeData) -> float:
    exit_rows = sum(1 for row in data.records if row.get("has_exit_replay"))
    return lab.safe_div(float(data.join_quality.get("exact_join_count") or 0), float(exit_rows))


def classify_from_baseline_result(baseline_result: str, delta_bps: int) -> str:
    if baseline_result == lab.STOP and delta_bps > 0:
        return "saved_stop"
    if baseline_result == lab.TARGET:
        return "cut_target"
    if baseline_result == lab.TIMEOUT and delta_bps > 0:
        return "timeout_improved"
    if delta_bps < 0:
        return "harmful_exit"
    if delta_bps > 0:
        return "beneficial_exit"
    return "neutral_exit"


def no_lookahead_path_stats(replay: dict[str, Any], action_age_ms: int | None, horizon_ms: int) -> dict[str, Any]:
    if action_age_ms is None:
        return {
            "points": 0,
            "cutoff_ms": None,
            "last_pnl_bps": None,
            "max_pnl_bps": None,
            "min_pnl_bps": None,
            "range_bps": None,
            "recovery_from_min_bps": None,
        }
    cutoff_ms = min(action_age_ms, horizon_ms)
    points = [(age, pnl) for age, pnl in lab.path_points(replay) if age <= cutoff_ms]
    if not points:
        return {
            "points": 0,
            "cutoff_ms": cutoff_ms,
            "last_pnl_bps": None,
            "max_pnl_bps": None,
            "min_pnl_bps": None,
            "range_bps": None,
            "recovery_from_min_bps": None,
        }
    last_pnl = points[-1][1]
    max_pnl = max(pnl for _, pnl in points)
    min_pnl = min(pnl for _, pnl in points)
    return {
        "points": len(points),
        "cutoff_ms": cutoff_ms,
        "last_pnl_bps": last_pnl,
        "max_pnl_bps": max_pnl,
        "min_pnl_bps": min_pnl,
        "range_bps": max_pnl - min_pnl,
        "recovery_from_min_bps": last_pnl - min_pnl,
    }


def bool_guard_protects(guard_name: str, stats: dict[str, Any]) -> tuple[bool, str]:
    points = int(stats.get("points") or 0)
    last_pnl = lab.int_or_none(stats.get("last_pnl_bps"))
    max_pnl = lab.int_or_none(stats.get("max_pnl_bps"))
    min_pnl = lab.int_or_none(stats.get("min_pnl_bps"))
    range_bps = lab.int_or_none(stats.get("range_bps"))
    recovery = lab.int_or_none(stats.get("recovery_from_min_bps"))
    if points <= 0 or last_pnl is None or max_pnl is None or min_pnl is None:
        return False, "no_early_path"
    if guard_name == "G1_STEADY_EARLY_STRENGTH":
        protects = max_pnl >= 500 and last_pnl >= 250 and min_pnl >= -300
        return protects, "steady_early_strength" if protects else "steady_early_strength_not_met"
    if guard_name == "G2_RECOVERY_AFTER_EARLY_DRAWDOWN":
        protects = min_pnl <= -300 and recovery is not None and recovery >= 300 and last_pnl >= -100
        return protects, "recovery_after_early_drawdown" if protects else "recovery_not_met"
    if guard_name == "G3_LOW_VOL_CONTINUATION":
        protects = points >= 3 and range_bps is not None and range_bps <= 350 and last_pnl >= 0 and max_pnl >= 100
        return protects, "low_vol_continuation" if protects else "low_vol_continuation_not_met"
    return False, "guard_has_no_boolean_protector"


def delayed_guard_ms(guard_name: str) -> int | None:
    if guard_name == "G4_DELAYED_DECISION_4000":
        return 4000
    if guard_name == "G5_DELAYED_DECISION_8000":
        return 8000
    return None


def update_action_after_delay(action: dict[str, Any], replay: dict[str, Any], delay_ms: int) -> dict[str, Any]:
    out = copy.deepcopy(action)
    if not action.get("supported") or not action.get("action_taken"):
        out["rtp_guard_reason"] = "no_anchor_action"
        return out
    candidate_age = lab.int_or_none(action.get("candidate_age_ms"))
    baseline_exit_age = lab.int_or_none(action.get("baseline_exit_age_ms"))
    max_hold_ms = lab.int_or_none(action.get("max_hold_ms"))
    if candidate_age is None or baseline_exit_age is None or max_hold_ms is None:
        out.update(
            {
                "action_taken": False,
                "classification": "no_active_exit",
                "tsv2_pnl_bps": action.get("baseline_pnl_bps"),
                "delta_bps": 0,
                "delta_after_cost_bps": 0,
                "exclusion_reason": "rtp_delay_missing_age",
                "rtp_guard_protected": True,
                "rtp_guard_reason": "delay_missing_age",
            }
        )
        return out
    delayed_age = candidate_age + delay_ms
    if delayed_age > baseline_exit_age or delayed_age > max_hold_ms:
        out.update(
            {
                "action_taken": False,
                "classification": "no_active_exit",
                "candidate_age_ms": delayed_age,
                "tsv2_pnl_bps": action.get("baseline_pnl_bps"),
                "delta_bps": 0,
                "delta_after_cost_bps": 0,
                "exclusion_reason": "rtp_delay_after_baseline_or_hold",
                "rtp_guard_protected": True,
                "rtp_guard_reason": "delay_after_baseline_or_hold",
            }
        )
        return out
    delayed_pnl, source = lab.last_path_pnl_at_or_before(replay, delayed_age)
    if delayed_pnl is None:
        out.update(
            {
                "action_taken": False,
                "classification": "no_active_exit",
                "candidate_age_ms": delayed_age,
                "tsv2_pnl_bps": action.get("baseline_pnl_bps"),
                "delta_bps": 0,
                "delta_after_cost_bps": 0,
                "exclusion_reason": "rtp_delay_missing_path",
                "rtp_guard_protected": True,
                "rtp_guard_reason": "delay_missing_path",
            }
        )
        return out
    baseline_pnl = int(action.get("baseline_pnl_bps") or 0)
    delta = int(delayed_pnl - baseline_pnl)
    classification = classify_from_baseline_result(str(action.get("baseline_result") or lab.UNKNOWN), delta)
    out.update(
        {
            "action_taken": True,
            "classification": classification,
            "candidate_age_ms": delayed_age,
            "candidate_pnl_bps": delayed_pnl,
            "tsv2_pnl_bps": delayed_pnl,
            "delta_bps": delta,
            "delta_after_cost_bps": delta,
            "mask_action_source": f"rtp_delay_{delay_ms}ms_{source}",
            "exclusion_reason": "",
            "rtp_guard_protected": action.get("classification") == TARGET_CUT_CLASS and classification != TARGET_CUT_CLASS,
            "rtp_guard_reason": f"delay_{delay_ms}ms",
        }
    )
    return out


def apply_guard(
    action: dict[str, Any],
    source_record: dict[str, Any] | None,
    guard_name: str,
    early_horizon_ms: int,
) -> dict[str, Any]:
    out = copy.deepcopy(action)
    out["guard_name"] = guard_name
    out["early_horizon_ms"] = early_horizon_ms
    out["rtp_guard_protected"] = False
    out["rtp_guard_reason"] = "none"
    if guard_name == "G0_NONE":
        return out
    replay = source_record.get("_exit_replay_row") if isinstance(source_record, dict) else None
    if not isinstance(replay, dict):
        out["rtp_guard_reason"] = "missing_replay"
        return out
    delay_ms = delayed_guard_ms(guard_name)
    if delay_ms is not None:
        out = update_action_after_delay(out, replay, delay_ms)
        out["guard_name"] = guard_name
        out["early_horizon_ms"] = early_horizon_ms
        return out
    if not action.get("supported") or not action.get("action_taken"):
        out["rtp_guard_reason"] = "no_anchor_action"
        return out
    stats = no_lookahead_path_stats(replay, lab.int_or_none(action.get("candidate_age_ms")), early_horizon_ms)
    protects, reason = bool_guard_protects(guard_name, stats)
    out.update({f"rtp_{key}": value for key, value in stats.items()})
    out["rtp_guard_reason"] = reason
    if protects:
        out.update(
            {
                "action_taken": False,
                "classification": "no_active_exit",
                "tsv2_pnl_bps": action.get("baseline_pnl_bps"),
                "delta_bps": 0,
                "delta_after_cost_bps": 0,
                "exclusion_reason": "rtp_guard_protected",
                "rtp_guard_protected": action.get("classification") == TARGET_CUT_CLASS,
            }
        )
    return out


def sum_damage(rows: list[dict[str, Any]], classification: str) -> int:
    if classification == TARGET_CUT_CLASS:
        return sum(max(0, -int(row.get("delta_bps") or 0)) for row in rows if row.get("classification") == classification)
    return sum(max(0, int(row.get("delta_bps") or 0)) for row in rows if row.get("classification") == classification)


def tail_preservation_metrics(anchor_actions: list[dict[str, Any]], guarded_actions: list[dict[str, Any]]) -> dict[str, Any]:
    guarded_by_id = {identity(row): row for row in guarded_actions}
    anchor_target_cuts = [
        row for row in anchor_actions
        if row.get("supported") and row.get("classification") == TARGET_CUT_CLASS
    ]
    anchor_target_cuts.sort(key=lambda row: max(0, -int(row.get("delta_bps") or 0)), reverse=True)

    def rate_for_top(pct: float) -> tuple[int, int, float]:
        if not anchor_target_cuts:
            return 0, 0, 1.0
        count = max(1, int(len(anchor_target_cuts) * pct + 0.999999))
        top_rows = anchor_target_cuts[:count]
        protected = 0
        for row in top_rows:
            guarded = guarded_by_id.get(identity(row))
            if guarded is not None and guarded.get("classification") != TARGET_CUT_CLASS:
                protected += 1
        return len(top_rows), protected, lab.safe_div(float(protected), float(len(top_rows)))

    top5_count, top5_protected, top5_rate = rate_for_top(0.05)
    top10_count, top10_protected, top10_rate = rate_for_top(0.10)
    anchor_target_cut_damage = sum_damage(anchor_actions, TARGET_CUT_CLASS)
    guarded_target_cut_damage = sum_damage(guarded_actions, TARGET_CUT_CLASS)
    anchor_saved_stop_damage = sum_damage(anchor_actions, "saved_stop")
    guarded_saved_stop_damage = sum_damage(guarded_actions, "saved_stop")
    anchor_timeout_count = sum(1 for row in anchor_actions if row.get("classification") == "timeout_improved")
    guarded_timeout_count = sum(1 for row in guarded_actions if row.get("classification") == "timeout_improved")
    return {
        "anchor_target_cut_count": len(anchor_target_cuts),
        "anchor_target_cut_damage_bps": anchor_target_cut_damage,
        "guarded_target_cut_damage_bps": guarded_target_cut_damage,
        "target_cut_damage_reduction_bps": anchor_target_cut_damage - guarded_target_cut_damage,
        "target_cut_damage_reduction_rate": lab.safe_div(
            float(anchor_target_cut_damage - guarded_target_cut_damage),
            float(anchor_target_cut_damage),
        ),
        "anchor_saved_stop_damage_bps": anchor_saved_stop_damage,
        "guarded_saved_stop_damage_bps": guarded_saved_stop_damage,
        "saved_stop_damage_retained_rate": (
            lab.safe_div(float(guarded_saved_stop_damage), float(anchor_saved_stop_damage))
            if anchor_saved_stop_damage
            else 1.0
        ),
        "anchor_timeout_improved_count": anchor_timeout_count,
        "guarded_timeout_improved_count": guarded_timeout_count,
        "timeout_improved_count_retained_rate": (
            lab.safe_div(float(guarded_timeout_count), float(anchor_timeout_count))
            if anchor_timeout_count
            else 1.0
        ),
        "top5_target_cut_winner_count": top5_count,
        "top5_target_cut_winners_protected": top5_protected,
        "top5_target_cut_winners_protected_rate": top5_rate,
        "top10_target_cut_winner_count": top10_count,
        "top10_target_cut_winners_protected": top10_protected,
        "top10_target_cut_winners_protected_rate": top10_rate,
    }


def prefixed_cost_metrics(actions: list[dict[str, Any]], cost_bps: int, prefix: str) -> dict[str, Any]:
    metrics = lab.summarize_action_rows(actions, roundtrip_cost_bps=cost_bps)
    keys = [
        "baseline_sum_after_cost_bps",
        "baseline_avg_after_cost_bps",
        "baseline_median_after_cost_bps",
        "tsv2_sum_after_cost_bps",
        "tsv2_avg_after_cost_bps",
        "tsv2_median_after_cost_bps",
        "delta_sum_bps",
        "delta_avg_bps",
        "delta_median_bps",
    ]
    return {f"{prefix}cost{cost_bps}_{key}": metrics.get(key) for key in keys}


def pass_segment_guard(stability_rows: list[dict[str, Any]]) -> bool:
    expected = {"train", "validation", "holdout"}
    seen = {str(row.get("segment")) for row in stability_rows}
    if seen != expected:
        return False
    for row in stability_rows:
        if float(row.get("target_cut_damage_ratio") or 0.0) > 0.20:
            return False
    return True


def evaluate_scope_anchor_guard(
    data: ScopeData,
    anchor: Anchor,
    guard_name: str,
    early_horizon_ms: int,
) -> tuple[dict[str, Any], list[dict[str, Any]], dict[str, Any], list[dict[str, Any]]]:
    records_by_id = {identity(row): row for row in data.records}
    anchor_actions = lab.cell_action_rows(
        data.records,
        anchor.target_bps,
        anchor.stop_bps,
        anchor.max_hold_ms,
        roundtrip_cost_bps=100,
        mask_name=anchor.mask_name,
    )
    guarded_actions = [
        apply_guard(row, records_by_id.get(identity(row)), guard_name, early_horizon_ms)
        for row in anchor_actions
    ]

    cost100 = lab.summarize_action_rows(guarded_actions, roundtrip_cost_bps=100)
    anchor_cost100 = lab.summarize_action_rows(anchor_actions, roundtrip_cost_bps=100)
    cost200 = lab.summarize_action_rows(guarded_actions, roundtrip_cost_bps=200)
    anchor_cost200 = lab.summarize_action_rows(anchor_actions, roundtrip_cost_bps=200)
    tail = tail_preservation_metrics(anchor_actions, guarded_actions)
    stability_rows: list[dict[str, Any]] = []
    for segment in ("train", "validation", "holdout"):
        segment_actions = [row for row in guarded_actions if row.get("segment") == segment]
        metrics = lab.summarize_action_rows(segment_actions, roundtrip_cost_bps=100)
        stability_rows.append(
            {
                "scope": data.scope,
                "anchor": anchor.name,
                "mask_name": anchor.mask_name,
                "target_bps": anchor.target_bps,
                "stop_bps": anchor.stop_bps,
                "max_hold_ms": anchor.max_hold_ms,
                "guard_name": guard_name,
                "early_horizon_ms": early_horizon_ms,
                "segment": segment,
                **metrics,
                "segment_target_cut_damage_guard_pass": float(metrics.get("target_cut_damage_ratio") or 0.0) <= 0.20,
            }
        )
    cost_sensitivity: dict[str, Any] = {}
    for cost in COSTS_BPS:
        cost_sensitivity.update(prefixed_cost_metrics(guarded_actions, cost, "guarded_"))
        cost_sensitivity.update(prefixed_cost_metrics(anchor_actions, cost, "anchor_"))

    target_cut_count_limit = float(cost100.get("target_cut_count_guard_limit") or 0.0)
    target_cut_count_guard_pass = bool(cost100.get("target_cut_count_guard_pass"))
    aggregate_pass = (
        exact_join_rate(data) >= 0.98
        and float(cost100.get("exit_action_precision") or 0.0) >= 0.70
        and float(cost100.get("exit_action_precision_wilson95_lower") or 0.0) >= 0.65
        and int(cost100.get("delta_sum_bps") or 0) > 0
        and float(cost100.get("delta_avg_bps") or 0.0) > 0.0
        and float(cost100.get("target_cut_damage_ratio") or 0.0) <= 0.20
        and target_cut_count_guard_pass
        and tail["saved_stop_damage_retained_rate"] >= 0.70
        and tail["timeout_improved_count_retained_rate"] >= 0.70
        and tail["top10_target_cut_winners_protected_rate"] >= 0.50
        and int(cost100.get("tsv2_sum_after_cost_bps") or 0) > int(anchor_cost100.get("tsv2_sum_after_cost_bps") or 0)
    )
    segment_pass = pass_segment_guard(stability_rows)
    row = {
        "scope": data.scope,
        "anchor": anchor.name,
        "mask_name": anchor.mask_name,
        "target_bps": anchor.target_bps,
        "stop_bps": anchor.stop_bps,
        "max_hold_ms": anchor.max_hold_ms,
        "guard_name": guard_name,
        "early_horizon_ms": early_horizon_ms,
        "positions_with_exit_replay": sum(1 for row in data.records if row.get("has_exit_replay")),
        "positions_with_tsv2_windows": sum(1 for row in data.records if row.get("has_tsv2_windows")),
        "candidate_positions": sum(1 for row in data.records if row.get("has_candidate")),
        "exact_join_rate": exact_join_rate(data),
        "supported_rows": cost100.get("supported_rows"),
        "action_taken_count": cost100.get("action_taken_count"),
        "exit_action_precision": cost100.get("exit_action_precision"),
        "wilson_lower95": cost100.get("exit_action_precision_wilson95_lower"),
        "paired_delta_sum_bps": cost100.get("delta_sum_bps"),
        "paired_delta_avg_bps": cost100.get("delta_avg_bps"),
        "paired_delta_median_bps": cost100.get("delta_median_bps"),
        "baseline_cost100_sum_bps": cost100.get("baseline_sum_after_cost_bps"),
        "tsv2_cost100_sum_bps": cost100.get("tsv2_sum_after_cost_bps"),
        "baseline_cost200_sum_bps": cost200.get("baseline_sum_after_cost_bps"),
        "tsv2_cost200_sum_bps": cost200.get("tsv2_sum_after_cost_bps"),
        "anchor_tsv2_cost100_sum_bps": anchor_cost100.get("tsv2_sum_after_cost_bps"),
        "anchor_tsv2_cost200_sum_bps": anchor_cost200.get("tsv2_sum_after_cost_bps"),
        "cost100_improvement_vs_unguarded_anchor_bps": int(cost100.get("tsv2_sum_after_cost_bps") or 0)
        - int(anchor_cost100.get("tsv2_sum_after_cost_bps") or 0),
        "cost200_improvement_vs_unguarded_anchor_bps": int(cost200.get("tsv2_sum_after_cost_bps") or 0)
        - int(anchor_cost200.get("tsv2_sum_after_cost_bps") or 0),
        "target_cut_damage_ratio": cost100.get("target_cut_damage_ratio"),
        "target_cut_count": cost100.get("target_cut_count"),
        "target_cut_damage_bps": cost100.get("target_cut_damage_bps"),
        "saved_stop_count": cost100.get("saved_stop_count"),
        "saved_stop_damage_bps": cost100.get("saved_stop_damage_bps"),
        "timeout_improved_count": cost100.get("timeout_improved_count"),
        "target_cut_count_guard_limit": target_cut_count_limit,
        "target_cut_count_guard_pass": target_cut_count_guard_pass,
        "max_consecutive_harmful_actions": cost100.get("max_consecutive_harmful_actions"),
        "aggregate_pass": aggregate_pass,
        "segment_target_cut_guard_pass": segment_pass,
        "scope_pass": aggregate_pass and segment_pass,
        **tail,
        **cost_sensitivity,
    }
    tail_row = {
        "scope": data.scope,
        "anchor": anchor.name,
        "guard_name": guard_name,
        "early_horizon_ms": early_horizon_ms,
        **tail,
    }
    return row, stability_rows, tail_row, guarded_actions


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    fieldnames: list[str] = []
    for row in rows:
        for key in row:
            if key not in fieldnames:
                fieldnames.append(key)
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def markdown_table(rows: list[dict[str, Any]], columns: list[str], limit: int = 20) -> str:
    if not rows:
        return "_Brak wierszy._"
    selected = rows[:limit]
    lines = [
        "| " + " | ".join(columns) + " |",
        "| " + " | ".join("---" for _ in columns) + " |",
    ]
    for row in selected:
        values = [str(row.get(column, "")) for column in columns]
        lines.append("| " + " | ".join(value.replace("\n", " ") for value in values) + " |")
    if len(rows) > limit:
        lines.append(f"\n_Pokazano {limit} z {len(rows)} wierszy._")
    return "\n".join(lines)


def build_intersection_rows(summary_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, int], dict[str, dict[str, Any]]] = {}
    for row in summary_rows:
        key = (str(row["anchor"]), str(row["guard_name"]), int(row["early_horizon_ms"]))
        grouped.setdefault(key, {})[str(row["scope"])] = row
    out: list[dict[str, Any]] = []
    for (anchor, guard, horizon), by_scope in sorted(grouped.items()):
        r49 = by_scope.get(R49_SCOPE)
        r50 = by_scope.get(R50_SCOPE)
        r49_pass = bool(r49 and r49.get("scope_pass") is True)
        r50_pass = bool(r50 and r50.get("scope_pass") is True)
        out.append(
            {
                "anchor": anchor,
                "guard_name": guard,
                "early_horizon_ms": horizon,
                "r49_scope_pass": r49_pass,
                "r50_scope_pass": r50_pass,
                "fixed_pair_passing_both": r49_pass and r50_pass,
                "r49_action_precision": r49.get("exit_action_precision") if r49 else "",
                "r50_action_precision": r50.get("exit_action_precision") if r50 else "",
                "r49_paired_delta_sum_bps": r49.get("paired_delta_sum_bps") if r49 else "",
                "r50_paired_delta_sum_bps": r50.get("paired_delta_sum_bps") if r50 else "",
                "r49_target_cut_damage_ratio": r49.get("target_cut_damage_ratio") if r49 else "",
                "r50_target_cut_damage_ratio": r50.get("target_cut_damage_ratio") if r50 else "",
                "r49_cost100_improvement_vs_anchor_bps": r49.get("cost100_improvement_vs_unguarded_anchor_bps") if r49 else "",
                "r50_cost100_improvement_vs_anchor_bps": r50.get("cost100_improvement_vs_unguarded_anchor_bps") if r50 else "",
            }
        )
    return out


def choose_best(summary_rows: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not summary_rows:
        return None
    return max(
        summary_rows,
        key=lambda row: (
            bool(row.get("scope_pass")),
            int(row.get("cost100_improvement_vs_unguarded_anchor_bps") or 0),
            -float(row.get("target_cut_damage_ratio") or 999.0),
            float(row.get("exit_action_precision") or 0.0),
        ),
    )


def write_reports(
    summary_rows: list[dict[str, Any]],
    stability_rows: list[dict[str, Any]],
    tail_rows: list[dict[str, Any]],
    intersection_rows: list[dict[str, Any]],
    scope_data: list[ScopeData],
) -> dict[str, Any]:
    passing = [row for row in intersection_rows if row.get("fixed_pair_passing_both") is True]
    best = choose_best(summary_rows)
    missing_data = any(not data.records for data in scope_data)
    if missing_data:
        verdict = "RTP_BLOCKED_BY_DATA"
    elif passing:
        verdict = "RTP_PROMISING_OFFLINE_ONLY / NEED_FRESH_SCOPE"
    elif any(int(row.get("cost100_improvement_vs_unguarded_anchor_bps") or 0) > 0 for row in summary_rows):
        verdict = "RTP_DIAGNOSTIC_ONLY / NO_RUNTIME"
    else:
        verdict = "RTP_NO_SIGNAL / REJECTED_FOR_RUNTIME"
    r51_decision = "GO_PREPARE_R51_PLAN" if passing else "NO_GO_FOR_R51"

    scope_pass_count = sum(1 for row in summary_rows if row.get("scope_pass") is True)
    scope_rows = [
        {
            "scope": data.scope,
            "positions_with_exit_replay": sum(1 for row in data.records if row.get("has_exit_replay")),
            "positions_with_tsv2_windows": sum(1 for row in data.records if row.get("has_tsv2_windows")),
            "candidate_positions": sum(1 for row in data.records if row.get("has_candidate")),
            "exact_join_rate": exact_join_rate(data),
            "shadow_exit_replay": data.input_paths.get("shadow_exit_replay"),
            "shadow_lifecycle": data.input_paths.get("shadow_lifecycle"),
            "probe_shadow_lifecycle": data.input_paths.get("probe_shadow_lifecycle"),
        }
        for data in scope_data
    ]

    intersection_preview = markdown_table(
        intersection_rows,
        [
            "anchor",
            "guard_name",
            "early_horizon_ms",
            "r49_scope_pass",
            "r50_scope_pass",
            "fixed_pair_passing_both",
            "r49_paired_delta_sum_bps",
            "r50_paired_delta_sum_bps",
        ],
        limit=30,
    )
    best_rows = sorted(
        summary_rows,
        key=lambda row: int(row.get("cost100_improvement_vs_unguarded_anchor_bps") or 0),
        reverse=True,
    )
    best_preview = markdown_table(
        best_rows,
        [
            "scope",
            "anchor",
            "guard_name",
            "early_horizon_ms",
            "scope_pass",
            "exit_action_precision",
            "wilson_lower95",
            "paired_delta_sum_bps",
            "target_cut_damage_ratio",
            "cost100_improvement_vs_unguarded_anchor_bps",
        ],
        limit=20,
    )
    scope_preview = markdown_table(
        scope_rows,
        [
            "scope",
            "positions_with_exit_replay",
            "positions_with_tsv2_windows",
            "candidate_positions",
            "exact_join_rate",
        ],
    )
    best_text = "none"
    if best is not None:
        best_text = (
            f"{best['anchor']} + {best['guard_name']} @ {best['early_horizon_ms']}ms "
            f"on {best['scope']}"
        )

    report = f"""# PR-RTP-A0: Right-Tail Preservation Offline Proof

Data: `2026-06-28`

Final verdict: `{verdict}`

R51 decision: `{r51_decision}`

## Granica runtime

Ten raport jest offline-only. Nie zatwierdza zmian runtime, `shadow_close_only`, active close, BUY/REJECT, Gatekeeper policy, selector runtime, `alpha_31100`, XGBoost ani TX/Jito/live path. Nie dodano nowych masek TSV2 ani nowych progow runtime. Surowe JSONL pozostaja lokalnym dowodem i nie sa przeznaczone do commita.

## Pytanie

Czy stala para `(anchor, guard)` moze ograniczyc ciecie przyszlego prawego ogona, uzywajac tylko no-lookahead early path oraz candidate-time fields?

## Zakres dowodowy

{scope_preview}

## Predeklarowane kotwice

- `M0_ALL / 6000 / -6000 / 120000`
- `M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000`
- `M7_CLASS_RESTRICTED / 10000 / -6000 / 60000`

## Predeklarowane guardy

- `G0_NONE`: brak dodatkowej ochrony.
- `G1_STEADY_EARLY_STRENGTH`: chroni tylko gdy path znany do `min(candidate_action_age, horizon)` ma `max >= 500 bps`, `last >= 250 bps`, `min >= -300 bps`.
- `G2_RECOVERY_AFTER_EARLY_DRAWDOWN`: chroni tylko gdy path ma drawdown `min <= -300 bps`, recovery `last - min >= 300 bps`, `last >= -100 bps`.
- `G3_LOW_VOL_CONTINUATION`: chroni tylko gdy path ma co najmniej 3 punkty, zakres `<= 350 bps`, `last >= 0 bps`, `max >= 100 bps`.
- `G4_DELAYED_DECISION_4000`: symuluje decyzje po 4000 ms, uzywajac tylko stanu dostepnego po opoznieniu.
- `G5_DELAYED_DECISION_8000`: symuluje decyzje po 8000 ms, uzywajac tylko stanu dostepnego po opoznieniu.

Horyzonty early path: `{', '.join(str(value) for value in EARLY_HORIZONS_MS)}` ms.

## Wynik intersection

Passing fixed pairs across R49 and R50: `{len(passing)}`

Scope-pass rows: `{scope_pass_count}`

Best diagnostic row: `{best_text}`

{intersection_preview}

## Najlepsze wiersze diagnostyczne

{best_preview}

## Decyzja

Runtime approval: `false`

Shadow_close_only approval: `false`

R51 GO/NO-GO: `{r51_decision}`

No active close.

No Gatekeeper/BUY/REJECT/selector/TX/Jito/live change.

TSV2 remains diagnostic/logging-only.

ORG/TSV2/EIX/RTP provide no basis for runtime or `shadow_close_only`.

{"Jesli RTP-A0 nie ma stalej pary przechodzacej R49 i R50, kierunek pozostaje TSV2 diagnostic/logging-only. NO_GO_FOR_R51." if not passing else "RTP-A0 znalazl stala pare offline, ale nadal nie zatwierdza runtime. Dopuszczalny jest tylko plan R51 full-evidence logging-only."}

## Outputy

- `{SUMMARY_CSV}`
- `{STABILITY_CSV}`
- `{TAIL_CSV}`
- `{INTERSECTION_CSV}`
- `{ADR_MD}`
"""
    REPORT_MD.parent.mkdir(parents=True, exist_ok=True)
    REPORT_MD.write_text(report, encoding="utf-8")

    adr = f"""# ADR-8D: RTP-A0 right-tail preservation offline proof

Status: {verdict}
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Zakres: PR-RTP-A0

## Decyzja

RTP-A0 zostal wykonany jako offline-only proof. Nie zmieniono runtime.

Final verdict: `{verdict}`

R51 GO/NO-GO: `{r51_decision}`

## Dowody

{scope_preview}

## Wynik

- `passing_fixed_pair_count = {len(passing)}`
- `scope_pass_count = {scope_pass_count}`
- `best_fixed_pair = {best_text}`
- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `raw_jsonl_committed = false`

## Konsekwencje

Brak zgody na runtime, active close lub `shadow_close_only`. Nie bylo zmian Gatekeeper/BUY/REJECT/selector/TX/Jito/live. RTP-A0 jest tylko testem offline right-tail preservation. Jezeli brak stalej pary przechodzacej R49 i R50, obowiazuje `NO_GO_FOR_R51`, TSV2 pozostaje diagnostic/logging-only i nie startujemy nowego runu na podstawie tego wyniku. ORG/TSV2/EIX/RTP nie daja podstaw do runtime ani `shadow_close_only`.
"""
    ADR_MD.parent.mkdir(parents=True, exist_ok=True)
    ADR_MD.write_text(adr, encoding="utf-8")

    if passing:
        R51_PLAN_MD.parent.mkdir(parents=True, exist_ok=True)
        R51_PLAN_MD.write_text(
            """# PLAN_R51_FULL_EVIDENCE_LOGGING_ONLY_20260628

Status: PREPARED_ONLY / NO_RUNTIME

R51 may be considered only as a logging-only evidence run. Requirements:

- no runtime changes
- no active close
- no shadow_close_only
- mandatory gatekeeper_v2_decisions.jsonl
- mandatory materialized feature snapshot
- mandatory shadow_lifecycle.jsonl
- mandatory probe_shadow_lifecycle.jsonl
- mandatory shadow_exit_replay_v1.jsonl
- mandatory lifecycle launcher PASS report
- mandatory immutable manifest with sha256, size, rows
- no active log symlinks to archive volume
- cleanup blocked without verified archive manifest
""",
            encoding="utf-8",
        )

    return {
        "verdict": verdict,
        "r51_decision": r51_decision,
        "passing_fixed_pair_count": len(passing),
        "scope_pass_count": scope_pass_count,
        "best_fixed_pair": best_text,
        "runtime_approval": False,
        "shadow_close_only_approval": False,
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    scopes = [
        load_scope(args.r49_scope, args.local_logs_root, args.volume_logs_root),
        load_scope(args.r50_scope, args.local_logs_root, args.volume_logs_root),
    ]
    summary_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    tail_rows: list[dict[str, Any]] = []
    for data in scopes:
        for anchor in ANCHORS:
            for guard_name in GUARDS:
                for horizon in EARLY_HORIZONS_MS:
                    summary, stability, tail, _guarded_actions = evaluate_scope_anchor_guard(
                        data,
                        anchor,
                        guard_name,
                        horizon,
                    )
                    summary_rows.append(summary)
                    stability_rows.extend(stability)
                    tail_rows.append(tail)
    intersection_rows = build_intersection_rows(summary_rows)
    write_csv(SUMMARY_CSV, summary_rows)
    write_csv(STABILITY_CSV, stability_rows)
    write_csv(TAIL_CSV, tail_rows)
    write_csv(INTERSECTION_CSV, intersection_rows)
    result = write_reports(summary_rows, stability_rows, tail_rows, intersection_rows, scopes)
    return {
        **result,
        "scopes": [args.r49_scope, args.r50_scope],
        "summary_rows": len(summary_rows),
        "stability_rows": len(stability_rows),
        "tail_rows": len(tail_rows),
        "intersection_rows": len(intersection_rows),
        "output_files": [
            str(SUMMARY_CSV),
            str(STABILITY_CSV),
            str(TAIL_CSV),
            str(INTERSECTION_CSV),
            str(REPORT_MD),
            str(ADR_MD),
        ],
    }


def main() -> int:
    args = parse_args()
    result = run(args)
    print(json.dumps(result, sort_keys=True, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
