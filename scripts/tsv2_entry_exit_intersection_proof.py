#!/usr/bin/env python3
"""PR-TSV2-EIX-A0 offline entry+exit intersection proof.

This script is intentionally offline/read-only for Ghost runtime inputs. It
does not modify Gatekeeper, selector, execution, configs, logs, sidecars, or
runtime behavior. Raw JSONL logs are used only as local evidence and are not
copied into report outputs.

The proof is fail-closed: entry+exit intersections are evaluated only when a
scope has both pre-entry Gatekeeper/materialized decision evidence and TSV2
exit evidence. Missing pre-entry evidence is reported as a research result,
not papered over with lifecycle proxies.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Mapping


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"

REPORT_DIR = Path("reports/selector")
ORG_A0_SCOPE = "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2"
ORG_A0_THRESHOLD_CSV = REPORT_DIR / ORG_A0_SCOPE / "organic_candidate_policy_thresholds.csv"

SUMMARY_CSV = REPORT_DIR / "tsv2_entry_exit_intersection_summary.csv"
STABILITY_CSV = REPORT_DIR / "tsv2_entry_exit_intersection_stability.csv"
COST_CSV = REPORT_DIR / "tsv2_entry_exit_intersection_cost_sensitivity.csv"
TAIL_CSV = REPORT_DIR / "tsv2_entry_exit_intersection_tail_audit.csv"
THRESHOLD_CSV = REPORT_DIR / "tsv2_entry_exit_intersection_threshold_manifest.csv"
REPORT_MD = Path("PLANS/AUDYT/RAPORT_TSV2_ENTRY_EXIT_INTERSECTION_20260628.md")
ADR_MD = Path("docs/ADR/ADR_8D_TSV2_ENTRY_EXIT_INTERSECTION_20260628.md")

ENTRY_COHORTS = ("S1_F5", "C1", "C2", "C3", "C4")
EXIT_MASKS = (
    "M4_CONFIRM_2_WINDOWS",
    "M5_DELAY_4000MS_CONFIRM",
    "M6_DELAY_8000MS_CONFIRM",
    "M7_CLASS_RESTRICTED",
)
EXIT_CELLS = (
    (7500, -6000, 60000),
    (10000, -6000, 60000),
    (7500, -6000, 120000),
    (10000, -6000, 120000),
)
COSTS_BPS = (0, 50, 100, 150, 200)

S1_RULES: tuple[tuple[str, str, float, str], ...] = (
    ("current_market_cap_sol", ">=", 30.2, "ORG-A0 S1/F5 fixed floor"),
    ("bonding_progress_pct", ">=", 36.5, "ORG-A0 S1/F5 fixed floor"),
    ("price_change_ratio", ">=", 1.012, "ORG-A0 S1/F5 fixed floor"),
    ("buy_count", ">=", 8.0, "ORG-A0 S1/F5 fixed floor"),
    ("sol_buy_ratio", ">=", 0.520, "ORG-A0 S1/F5 fixed floor"),
)

VERDICT_MISSING = "MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED"
VERDICT_REJECTED = "NO_ENTRY_EXIT_FIXED_RULE / REJECTED_FOR_RUNTIME"
VERDICT_INCONCLUSIVE = "ENTRY_EXIT_INTERSECTION_INCONCLUSIVE / NO_RUNTIME"
VERDICT_PROMISING = "PROMISING_ENTRY_EXIT_INTERSECTION_OFFLINE / NEED_FRESH_SCOPE_PREDECLARED_VALIDATION / NO_RUNTIME"


@dataclass(frozen=True)
class ScopeEvidence:
    scope: str
    shadow_lifecycle: Path | None
    probe_lifecycle: Path | None
    exit_replay: Path | None
    decision_log: Path | None
    a2_summary: Path | None
    a2_cost: Path | None
    a2_stability: Path | None
    replay_rows: int | None

    @property
    def has_pre_entry(self) -> bool:
        return self.decision_log is not None

    @property
    def has_exit_evidence(self) -> bool:
        return self.exit_replay is not None and self.shadow_lifecycle is not None

    @property
    def has_a2_reports(self) -> bool:
        return self.a2_summary is not None and self.a2_cost is not None and self.a2_stability is not None

    @property
    def can_evaluate_entry_exit(self) -> bool:
        return self.has_pre_entry and self.has_exit_evidence


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Offline PR-TSV2-EIX-A0 entry+exit intersection proof.",
    )
    parser.add_argument("--r49-scope", default=R49_SCOPE)
    parser.add_argument("--r50-scope", default=R50_SCOPE)
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    parser.add_argument("--local-logs-root", type=Path, default=Path("logs"))
    parser.add_argument("--volume-logs-root", type=Path, default=Path("/mnt/HC_Volume_105935807/logs"))
    return parser.parse_args()


def first_existing(paths: Iterable[Path]) -> Path | None:
    seen: set[str] = set()
    for path in paths:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        if path.exists() and path.is_file():
            return path
    return None


def file_size(path: Path | None) -> int:
    if path is None:
        return 0
    try:
        return path.stat().st_size
    except OSError:
        return 0


def line_count(path: Path | None) -> int | None:
    if path is None:
        return None
    count = 0
    with path.open("rb") as f:
        for _ in f:
            count += 1
    return count


def choose_decision_log(scope: str, local_logs_root: Path, volume_logs_root: Path) -> Path | None:
    bases = [
        local_logs_root / "rollout" / scope / "decisions" / scope,
        volume_logs_root / "rollout" / scope / "decisions" / scope,
    ]
    candidates: list[Path] = []
    for base in bases:
        if base.exists():
            candidates.extend(base.glob("**/gatekeeper_v2_decisions.jsonl"))
    if not candidates:
        return None
    unique: dict[str, Path] = {}
    for path in candidates:
        try:
            unique[str(path.resolve())] = path
        except OSError:
            unique[str(path)] = path
    paths = list(unique.values())
    paths.sort(key=lambda p: ("/v2.5/v25_shadow/" not in str(p), "/v2.2/legacy_live/" not in str(p), str(p)))
    return paths[0]


def discover_scope(scope: str, reports_dir: Path, local_logs_root: Path, volume_logs_root: Path) -> ScopeEvidence:
    shadow_bases = [
        local_logs_root / "shadow_run" / scope,
        volume_logs_root / "shadow_run" / scope,
    ]
    shadow_lifecycle = first_existing(base / "shadow_lifecycle.jsonl" for base in shadow_bases)
    probe_lifecycle = first_existing(base / "probe_shadow_lifecycle.jsonl" for base in shadow_bases)
    exit_replay = first_existing(base / "shadow_exit_replay_v1.jsonl" for base in shadow_bases)
    report_base = reports_dir / scope
    decision_log = choose_decision_log(scope, local_logs_root, volume_logs_root)
    a2_summary = first_existing([report_base / "time_stop_v2_mask_summary_a2.csv"])
    a2_cost = first_existing([report_base / "time_stop_v2_mask_cost_sensitivity_a2.csv"])
    a2_stability = first_existing([report_base / "time_stop_v2_mask_stability_a2.csv"])
    replay_rows = line_count(exit_replay) if exit_replay is not None else None
    return ScopeEvidence(
        scope=scope,
        shadow_lifecycle=shadow_lifecycle,
        probe_lifecycle=probe_lifecycle,
        exit_replay=exit_replay,
        decision_log=decision_log,
        a2_summary=a2_summary,
        a2_cost=a2_cost,
        a2_stability=a2_stability,
        replay_rows=replay_rows,
    )


def read_csv_by_key(path: Path | None) -> dict[tuple[str, int, int, int], dict[str, str]]:
    if path is None:
        return {}
    out: dict[tuple[str, int, int, int], dict[str, str]] = {}
    with path.open(newline="") as f:
        reader = csv.DictReader(f)
        for row in reader:
            try:
                key = (
                    row["mask_name"],
                    int(row["target_bps"]),
                    int(row["stop_bps"]),
                    int(row["max_hold_ms"]),
                )
            except (KeyError, ValueError):
                continue
            out[key] = row
    return out


def read_csv_rows(path: Path | None) -> list[dict[str, str]]:
    if path is None:
        return []
    with path.open(newline="") as f:
        return list(csv.DictReader(f))


def as_float(row: Mapping[str, str], field: str) -> float:
    raw = row.get(field, "")
    if raw in ("", None):
        return 0.0
    try:
        value = float(raw)
    except ValueError:
        return 0.0
    return value if math.isfinite(value) else 0.0


def as_int(row: Mapping[str, str], field: str) -> int:
    return int(round(as_float(row, field)))


def as_bool(row: Mapping[str, str], field: str) -> bool:
    return str(row.get(field, "")).strip().lower() == "true"


def safe_div(num: float, den: float) -> float | str:
    if den == 0:
        return ""
    return num / den


def fixed_key(mask_name: str, target_bps: int, stop_bps: int, max_hold_ms: int) -> tuple[str, int, int, int]:
    return mask_name, target_bps, stop_bps, max_hold_ms


def cell_label(mask_name: str, target_bps: int, stop_bps: int, max_hold_ms: int) -> str:
    return f"{mask_name}/{target_bps}/{stop_bps}/{max_hold_ms}"


def fixed_rule_count() -> int:
    return len(ENTRY_COHORTS) * len(EXIT_MASKS) * len(EXIT_CELLS)


def evidence_blocker(evidences: Iterable[ScopeEvidence]) -> str:
    missing: list[str] = []
    for evidence in evidences:
        if evidence.decision_log is None:
            missing.append(f"{evidence.scope}:missing_gatekeeper_v2_decisions_jsonl")
        if evidence.exit_replay is None:
            missing.append(f"{evidence.scope}:missing_shadow_exit_replay_v1_jsonl")
        if evidence.shadow_lifecycle is None:
            missing.append(f"{evidence.scope}:missing_shadow_lifecycle_jsonl")
    return ",".join(missing)


def summary_from_a2(scope: str, key: tuple[str, int, int, int], row: Mapping[str, str], replay_rows: int | None) -> dict[str, object]:
    supported = as_int(row, "cost100_supported_rows")
    exact = as_int(row, "cost100_exact_rows")
    target_count = as_int(row, "cost100_baseline_target_count")
    stop_count = as_int(row, "cost100_baseline_stop_count")
    timeout_count = as_int(row, "cost100_baseline_timeout_count")
    mask_name, target_bps, stop_bps, max_hold_ms = key
    return {
        "scope": scope,
        "row_type": "mask_only_tsv2_without_entry_filter",
        "entry_cohort": "NONE",
        "mask_name": mask_name,
        "target_bps": target_bps,
        "stop_bps": stop_bps,
        "max_hold_ms": max_hold_ms,
        "evaluable": True,
        "blocking_reason": "",
        "retained_count": supported,
        "retained_pct": safe_div(float(supported), float(replay_rows or 0)),
        "exact_join_rate": safe_div(float(exact), float(supported)),
        "target_rate": safe_div(float(target_count), float(supported)),
        "stop_rate": safe_div(float(stop_count), float(supported)),
        "timeout_rate": safe_div(float(timeout_count), float(supported)),
        "negative_timeout_rate": "",
        "avg_pnl_bps": row.get("cost100_tsv2_avg_after_cost_bps", ""),
        "median_pnl_bps": row.get("cost100_tsv2_median_after_cost_bps", ""),
        "sum_pnl_bps": row.get("cost100_tsv2_sum_after_cost_bps", ""),
        "paired_delta_sum_bps": row.get("cost100_delta_sum_bps", ""),
        "paired_delta_avg_bps": row.get("cost100_delta_avg_bps", ""),
        "paired_delta_median_bps": row.get("cost100_delta_median_bps", ""),
        "exit_action_precision": row.get("cost100_exit_action_precision", ""),
        "wilson_lower95": row.get("cost100_exit_action_precision_wilson95_lower", ""),
        "target_cut_damage_ratio": row.get("cost100_target_cut_damage_ratio", ""),
        "target_cut_count": row.get("cost100_target_cut_count", ""),
        "saved_stop_count": row.get("cost100_saved_stop_count", ""),
        "timeout_improved_count": row.get("cost100_timeout_improved_count", ""),
        "target_cut_count_guard_pass": row.get("cost100_target_cut_count_guard_pass", ""),
        "aggregate_target_cut_guard_pass": row.get("cost100_aggregate_target_cut_damage_guard_pass", ""),
        "segment_target_cut_guard_pass": row.get("cost100_segment_target_cut_damage_guard_pass", ""),
        "max_consecutive_losses": "",
        "max_consecutive_harmful_actions": row.get("cost100_max_consecutive_harmful_actions", ""),
        "passes_acceptance": False,
        "acceptance_failures": "not_entry_exit_intersection;diagnostic_mask_only_baseline",
        "metric_source": "existing_a2_mask_summary_csv",
    }


def broad_from_a2(scope: str, target_bps: int, stop_bps: int, max_hold_ms: int, row: Mapping[str, str], replay_rows: int | None) -> dict[str, object]:
    supported = as_int(row, "cost100_supported_rows")
    exact = as_int(row, "cost100_exact_rows")
    target_count = as_int(row, "cost100_baseline_target_count")
    stop_count = as_int(row, "cost100_baseline_stop_count")
    timeout_count = as_int(row, "cost100_baseline_timeout_count")
    return {
        "scope": scope,
        "row_type": "broad_acted_baseline_from_a2",
        "entry_cohort": "BROAD_ACTED",
        "mask_name": "NONE",
        "target_bps": target_bps,
        "stop_bps": stop_bps,
        "max_hold_ms": max_hold_ms,
        "evaluable": True,
        "blocking_reason": "",
        "retained_count": supported,
        "retained_pct": safe_div(float(supported), float(replay_rows or 0)),
        "exact_join_rate": safe_div(float(exact), float(supported)),
        "target_rate": safe_div(float(target_count), float(supported)),
        "stop_rate": safe_div(float(stop_count), float(supported)),
        "timeout_rate": safe_div(float(timeout_count), float(supported)),
        "negative_timeout_rate": "",
        "avg_pnl_bps": row.get("cost100_baseline_avg_after_cost_bps", ""),
        "median_pnl_bps": row.get("cost100_baseline_median_after_cost_bps", ""),
        "sum_pnl_bps": row.get("cost100_baseline_sum_after_cost_bps", ""),
        "paired_delta_sum_bps": "",
        "paired_delta_avg_bps": "",
        "paired_delta_median_bps": "",
        "exit_action_precision": "",
        "wilson_lower95": "",
        "target_cut_damage_ratio": "",
        "target_cut_count": "",
        "saved_stop_count": "",
        "timeout_improved_count": "",
        "target_cut_count_guard_pass": "",
        "aggregate_target_cut_guard_pass": "",
        "segment_target_cut_guard_pass": "",
        "max_consecutive_losses": "",
        "max_consecutive_harmful_actions": "",
        "passes_acceptance": False,
        "acceptance_failures": "baseline_only",
        "metric_source": "existing_a2_mask_summary_csv_baseline_columns",
    }


def missing_entry_row(
    scope: str,
    entry_cohort: str,
    mask_name: str,
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    blocker: str,
    row_type: str,
) -> dict[str, object]:
    return {
        "scope": scope,
        "row_type": row_type,
        "entry_cohort": entry_cohort,
        "mask_name": mask_name,
        "target_bps": target_bps,
        "stop_bps": stop_bps,
        "max_hold_ms": max_hold_ms,
        "evaluable": False,
        "blocking_reason": blocker,
        "retained_count": "",
        "retained_pct": "",
        "exact_join_rate": "",
        "target_rate": "",
        "stop_rate": "",
        "timeout_rate": "",
        "negative_timeout_rate": "",
        "avg_pnl_bps": "",
        "median_pnl_bps": "",
        "sum_pnl_bps": "",
        "paired_delta_sum_bps": "",
        "paired_delta_avg_bps": "",
        "paired_delta_median_bps": "",
        "exit_action_precision": "",
        "wilson_lower95": "",
        "target_cut_damage_ratio": "",
        "target_cut_count": "",
        "saved_stop_count": "",
        "timeout_improved_count": "",
        "target_cut_count_guard_pass": "",
        "aggregate_target_cut_guard_pass": "",
        "segment_target_cut_guard_pass": "",
        "max_consecutive_losses": "",
        "max_consecutive_harmful_actions": "",
        "passes_acceptance": False,
        "acceptance_failures": blocker,
        "metric_source": "blocked_before_metric_calculation",
    }


def build_summary_rows(evidences: list[ScopeEvidence]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    blocker = evidence_blocker(evidences)
    for evidence in evidences:
        a2_summary = read_csv_by_key(evidence.a2_summary)
        for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
            baseline_row = None
            for mask_name in EXIT_MASKS:
                baseline_row = a2_summary.get(fixed_key(mask_name, target_bps, stop_bps, max_hold_ms))
                if baseline_row is not None:
                    break
            if baseline_row is not None:
                rows.append(broad_from_a2(evidence.scope, target_bps, stop_bps, max_hold_ms, baseline_row, evidence.replay_rows))
            else:
                rows.append(
                    missing_entry_row(
                        evidence.scope,
                        "BROAD_ACTED",
                        "NONE",
                        target_bps,
                        stop_bps,
                        max_hold_ms,
                        "missing_a2_baseline_row",
                        "broad_acted_baseline_from_a2",
                    )
                )
        for entry_cohort in ENTRY_COHORTS:
            for mask_name in EXIT_MASKS:
                for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                    rows.append(
                        missing_entry_row(
                            evidence.scope,
                            entry_cohort,
                            mask_name,
                            target_bps,
                            stop_bps,
                            max_hold_ms,
                            blocker or "entry_exit_calculation_not_implemented_for_runtime_safety_review",
                            "entry_exit_intersection",
                        )
                    )
        for mask_name in EXIT_MASKS:
            for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                key = fixed_key(mask_name, target_bps, stop_bps, max_hold_ms)
                row = a2_summary.get(key)
                if row is None:
                    rows.append(
                        missing_entry_row(
                            evidence.scope,
                            "NONE",
                            mask_name,
                            target_bps,
                            stop_bps,
                            max_hold_ms,
                            "missing_a2_mask_summary_row",
                            "mask_only_tsv2_without_entry_filter",
                        )
                    )
                    continue
                rows.append(summary_from_a2(evidence.scope, key, row, evidence.replay_rows))
    return rows


def build_cost_rows(evidences: list[ScopeEvidence]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    blocker = evidence_blocker(evidences)
    for evidence in evidences:
        a2_cost = read_csv_by_key(evidence.a2_cost)
        for mask_name in EXIT_MASKS:
            for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                key = fixed_key(mask_name, target_bps, stop_bps, max_hold_ms)
                row = a2_cost.get(key)
                for cost in COSTS_BPS:
                    if row is None:
                        rows.append(
                            {
                                "scope": evidence.scope,
                                "row_type": "mask_only_tsv2_without_entry_filter",
                                "entry_cohort": "NONE",
                                "mask_name": mask_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "roundtrip_cost_bps": cost,
                                "evaluable": False,
                                "blocking_reason": "missing_a2_cost_row",
                                "paired_delta_sum_bps": "",
                                "paired_delta_avg_bps": "",
                                "paired_delta_median_bps": "",
                                "absolute_baseline_pnl_bps": "",
                                "absolute_tsv2_pnl_bps": "",
                            }
                        )
                        continue
                    rows.append(
                        {
                            "scope": evidence.scope,
                            "row_type": "mask_only_tsv2_without_entry_filter",
                            "entry_cohort": "NONE",
                            "mask_name": mask_name,
                            "target_bps": target_bps,
                            "stop_bps": stop_bps,
                            "max_hold_ms": max_hold_ms,
                            "roundtrip_cost_bps": cost,
                            "evaluable": True,
                            "blocking_reason": "",
                            "paired_delta_sum_bps": row.get(f"paired_delta_sum_cost{cost}", row.get(f"paired_delta_cost{cost}", "")),
                            "paired_delta_avg_bps": row.get(f"paired_delta_avg_cost{cost}", ""),
                            "paired_delta_median_bps": row.get(f"paired_delta_median_cost{cost}", ""),
                            "absolute_baseline_pnl_bps": row.get(f"absolute_baseline_pnl_cost{cost}", ""),
                            "absolute_tsv2_pnl_bps": row.get(f"absolute_tsv2_pnl_cost{cost}", ""),
                        }
                    )
        for entry_cohort in ENTRY_COHORTS:
            for mask_name in EXIT_MASKS:
                for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                    for cost in COSTS_BPS:
                        rows.append(
                            {
                                "scope": evidence.scope,
                                "row_type": "entry_exit_intersection",
                                "entry_cohort": entry_cohort,
                                "mask_name": mask_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "roundtrip_cost_bps": cost,
                                "evaluable": False,
                                "blocking_reason": blocker,
                                "paired_delta_sum_bps": "",
                                "paired_delta_avg_bps": "",
                                "paired_delta_median_bps": "",
                                "absolute_baseline_pnl_bps": "",
                                "absolute_tsv2_pnl_bps": "",
                            }
                        )
    return rows


def build_stability_rows(evidences: list[ScopeEvidence]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    blocker = evidence_blocker(evidences)
    allowed_keys = {fixed_key(mask, t, s, h) for mask in EXIT_MASKS for t, s, h in EXIT_CELLS}
    for evidence in evidences:
        for row in read_csv_rows(evidence.a2_stability):
            key = fixed_key(
                row.get("mask_name", ""),
                as_int(row, "target_bps"),
                as_int(row, "stop_bps"),
                as_int(row, "max_hold_ms"),
            )
            if key not in allowed_keys:
                continue
            rows.append(
                {
                    "scope": evidence.scope,
                    "row_type": "mask_only_tsv2_without_entry_filter",
                    "entry_cohort": "NONE",
                    "mask_name": key[0],
                    "target_bps": key[1],
                    "stop_bps": key[2],
                    "max_hold_ms": key[3],
                    "segment": row.get("segment", ""),
                    "evaluable": True,
                    "blocking_reason": "",
                    "retained_count": row.get("supported_rows", ""),
                    "paired_delta_sum_bps": row.get("delta_sum_bps", ""),
                    "paired_delta_avg_bps": row.get("delta_avg_bps", ""),
                    "paired_delta_median_bps": row.get("delta_median_bps", ""),
                    "exit_action_precision": row.get("exit_action_precision", ""),
                    "wilson_lower95": row.get("exit_action_precision_wilson95_lower", ""),
                    "target_cut_damage_ratio": row.get("target_cut_damage_ratio", ""),
                    "target_cut_count": row.get("target_cut_count", ""),
                    "saved_stop_count": row.get("saved_stop_count", ""),
                    "timeout_improved_count": row.get("timeout_improved_count", ""),
                    "max_consecutive_harmful_actions": row.get("max_consecutive_harmful_actions", ""),
                    "segment_acceptance_pass": False,
                    "acceptance_failures": "not_entry_exit_intersection;diagnostic_mask_only_baseline",
                }
            )
        for entry_cohort in ENTRY_COHORTS:
            for mask_name in EXIT_MASKS:
                for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                    for segment in ("train", "validation", "holdout"):
                        rows.append(
                            {
                                "scope": evidence.scope,
                                "row_type": "entry_exit_intersection",
                                "entry_cohort": entry_cohort,
                                "mask_name": mask_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "segment": segment,
                                "evaluable": False,
                                "blocking_reason": blocker,
                                "retained_count": "",
                                "paired_delta_sum_bps": "",
                                "paired_delta_avg_bps": "",
                                "paired_delta_median_bps": "",
                                "exit_action_precision": "",
                                "wilson_lower95": "",
                                "target_cut_damage_ratio": "",
                                "target_cut_count": "",
                                "saved_stop_count": "",
                                "timeout_improved_count": "",
                                "max_consecutive_harmful_actions": "",
                                "segment_acceptance_pass": False,
                                "acceptance_failures": blocker,
                            }
                        )
    return rows


def build_tail_rows(evidences: list[ScopeEvidence]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    blocker = evidence_blocker(evidences)
    for evidence in evidences:
        for entry_cohort in ENTRY_COHORTS:
            for mask_name in EXIT_MASKS:
                for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
                    for removal in ("top5_positive_removed", "top10_positive_removed"):
                        rows.append(
                            {
                                "scope": evidence.scope,
                                "entry_cohort": entry_cohort,
                                "mask_name": mask_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "tail_scenario": removal,
                                "evaluable": False,
                                "blocking_reason": blocker,
                                "retained_count_after_tail_removal": "",
                                "sum_pnl_bps_after_tail_removal": "",
                                "avg_pnl_bps_after_tail_removal": "",
                                "median_pnl_bps_after_tail_removal": "",
                                "tail_dependency_flag": "unknown_missing_entry_join_evidence",
                            }
                        )
    return rows


def build_threshold_rows() -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for field, op, value, source in S1_RULES:
        rows.append(
            {
                "family": "entry",
                "cohort": "S1_F5",
                "field_or_rule": field,
                "operator": op,
                "threshold": value,
                "source": source,
                "used_in_eix": True,
                "notes": "fixed ORG-A0 S1/F5 rule",
            }
        )
    if ORG_A0_THRESHOLD_CSV.exists():
        for row in read_csv_rows(ORG_A0_THRESHOLD_CSV):
            stage = row.get("stage", "")
            used = str(row.get("used", "")).lower() == "true"
            if stage not in {"C1", "C2", "C3", "C4"}:
                continue
            rows.append(
                {
                    "family": "entry",
                    "cohort": stage,
                    "field_or_rule": row.get("field", ""),
                    "operator": "<=" if row.get("direction") == "cap" else ">=",
                    "threshold": row.get("threshold", ""),
                    "source": f"ORG-A0 {row.get('source', '')}; {ORG_A0_THRESHOLD_CSV}",
                    "used_in_eix": used,
                    "notes": f"profile={row.get('profile', '')}; quantile={row.get('quantile', '')}; no R50 retuning",
                }
            )
    else:
        rows.append(
            {
                "family": "entry",
                "cohort": "C1-C4",
                "field_or_rule": "ORG-A0 threshold CSV",
                "operator": "missing",
                "threshold": "",
                "source": str(ORG_A0_THRESHOLD_CSV),
                "used_in_eix": False,
                "notes": "missing evidence: existing ORG-A0 threshold manifest unavailable",
            }
        )
    for mask_name in EXIT_MASKS:
        rows.append(
            {
                "family": "exit_mask",
                "cohort": "TSV2",
                "field_or_rule": mask_name,
                "operator": "fixed_mask",
                "threshold": "",
                "source": "PR-TSV2-A2 predeclared mask",
                "used_in_eix": True,
                "notes": "no new masks; no R49/R50 tuning",
            }
        )
    for target_bps, stop_bps, max_hold_ms in EXIT_CELLS:
        rows.append(
            {
                "family": "exit_cell",
                "cohort": "TSV2",
                "field_or_rule": f"target={target_bps},stop={stop_bps},hold={max_hold_ms}",
                "operator": "fixed_cell",
                "threshold": "",
                "source": "PR-TSV2-EIX-A0 predeclared fixed cell list",
                "used_in_eix": True,
                "notes": "no other grid evaluated",
            }
        )
    return rows


def write_csv(path: Path, rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fields: list[str] = []
    for row in rows:
        for key in row:
            if key not in fields:
                fields.append(key)
    with path.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def evidence_rows(evidences: Iterable[ScopeEvidence]) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for evidence in evidences:
        for name, path in (
            ("shadow_lifecycle", evidence.shadow_lifecycle),
            ("probe_shadow_lifecycle", evidence.probe_lifecycle),
            ("shadow_exit_replay_v1", evidence.exit_replay),
            ("gatekeeper_v2_decisions", evidence.decision_log),
            ("a2_mask_summary", evidence.a2_summary),
            ("a2_cost_sensitivity", evidence.a2_cost),
            ("a2_stability", evidence.a2_stability),
        ):
            rows.append(
                {
                    "scope": evidence.scope,
                    "artifact": name,
                    "exists": path is not None,
                    "path": str(path) if path is not None else "",
                    "size_bytes": file_size(path),
                }
            )
    return rows


def format_bool(value: bool) -> str:
    return "true" if value else "false"


def evaluate_final(summary_rows: list[dict[str, object]], evidences: list[ScopeEvidence]) -> dict[str, object]:
    entry_exit_rows = [row for row in summary_rows if row.get("row_type") == "entry_exit_intersection"]
    evaluable_entry_exit = [row for row in entry_exit_rows if row.get("evaluable") is True]
    passing_rows = [row for row in evaluable_entry_exit if row.get("passes_acceptance") is True]
    blocker = evidence_blocker(evidences)
    if blocker:
        verdict = VERDICT_MISSING
    elif not passing_rows:
        verdict = VERDICT_REJECTED
    else:
        verdict = VERDICT_PROMISING
    return {
        "final_verdict": verdict,
        "fixed_rules_tested_count": fixed_rule_count(),
        "entry_exit_scope_rows": len(entry_exit_rows),
        "entry_exit_evaluable_rows": len(evaluable_entry_exit),
        "passing_fixed_rules_count": len(passing_rows),
        "best_fixed_rule": "none" if not passing_rows else cell_label(
            str(passing_rows[0]["mask_name"]),
            int(passing_rows[0]["target_bps"]),
            int(passing_rows[0]["stop_bps"]),
            int(passing_rows[0]["max_hold_ms"]),
        ),
        "blocking_reason": blocker,
        "runtime_approval": False,
        "shadow_close_only_approval": False,
        "raw_jsonl_committed": False,
    }


def markdown_table(rows: list[dict[str, object]], fields: list[str]) -> str:
    out = ["| " + " | ".join(fields) + " |", "| " + " | ".join(["---"] * len(fields)) + " |"]
    for row in rows:
        out.append("| " + " | ".join(str(row.get(field, "")) for field in fields) + " |")
    return "\n".join(out)


def selected_mask_rows(summary_rows: list[dict[str, object]]) -> list[dict[str, object]]:
    wanted = [
        ("r49_m4", R49_SCOPE, "M4_CONFIRM_2_WINDOWS", 10000, -6000, 120000),
        ("r49_m7", R49_SCOPE, "M7_CLASS_RESTRICTED", 10000, -6000, 60000),
        ("r50_m4", R50_SCOPE, "M4_CONFIRM_2_WINDOWS", 10000, -6000, 120000),
        ("r50_m7", R50_SCOPE, "M7_CLASS_RESTRICTED", 10000, -6000, 60000),
    ]
    out: list[dict[str, object]] = []
    for label, scope, mask, target, stop, hold in wanted:
        for row in summary_rows:
            if (
                row.get("row_type") == "mask_only_tsv2_without_entry_filter"
                and row.get("scope") == scope
                and row.get("mask_name") == mask
                and int(row.get("target_bps") or 0) == target
                and int(row.get("stop_bps") or 0) == stop
                and int(row.get("max_hold_ms") or 0) == hold
            ):
                copied = dict(row)
                copied["label"] = label
                out.append(copied)
                break
    return out


def write_reports(
    summary_rows: list[dict[str, object]],
    threshold_rows: list[dict[str, object]],
    evidences: list[ScopeEvidence],
    result: Mapping[str, object],
) -> None:
    REPORT_MD.parent.mkdir(parents=True, exist_ok=True)
    ADR_MD.parent.mkdir(parents=True, exist_ok=True)

    evidence_md = markdown_table(evidence_rows(evidences), ["scope", "artifact", "exists", "size_bytes", "path"])
    masks_md = markdown_table(
        selected_mask_rows(summary_rows),
        [
            "label",
            "scope",
            "mask_name",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "retained_count",
            "paired_delta_sum_bps",
            "exit_action_precision",
            "wilson_lower95",
            "target_cut_damage_ratio",
        ],
    )
    thresholds_md = markdown_table(
        threshold_rows[:18],
        ["family", "cohort", "field_or_rule", "operator", "threshold", "source", "used_in_eix"],
    )

    report = f"""# PR-TSV2-EIX-A0: Offline entry+exit intersection proof

Date: `2026-06-28`

Status: `{result['final_verdict']}`

## Runtime Boundary

This is offline-only research evidence. It does not approve runtime changes, `shadow_close_only`, active close, BUY/REJECT changes, Gatekeeper policy changes, selector runtime changes, `v25_confidence`, V3 promotion, `alpha_31100`, XGBoost, TX builder/sender/Jito/live path changes, new masks, new thresholds, or R50 retuning.

Raw JSONL logs are local evidence only and must not be committed.

No R51 is approved from this result.

## Scopes

- R49: `{R49_SCOPE}`
- R50: `{R50_SCOPE}`

## Evidence Inventory

{evidence_md}

## Fixed Rule Space

- Entry cohorts: `{', '.join(ENTRY_COHORTS)}`
- Exit masks: `{', '.join(EXIT_MASKS)}`
- Exit cells: `{'; '.join(str(cell) for cell in EXIT_CELLS)}`
- Fixed entry+exit rules requested: `{result['fixed_rules_tested_count']}`
- Evaluable entry+exit rows: `{result['entry_exit_evaluable_rows']}`
- Passing fixed rules: `{result['passing_fixed_rules_count']}`
- Best fixed rule: `{result['best_fixed_rule']}`

## Threshold Manifest Preview

{thresholds_md}

Full manifest: `{THRESHOLD_CSV}`

## Diagnostic Mask-Only Baseline

The rows below are existing A2 mask-only diagnostics without entry filtering. They are included as baselines only and are not entry+exit intersection proof.

{masks_md}

## Blocking Evidence

`{result['blocking_reason']}`

The R49 scope has local lifecycle and exit replay evidence, but no local `gatekeeper_v2_decisions.jsonl` or equivalent materialized pre-entry feature rows were found in the clean worktree or the log volume. Lifecycle/replay rows do not contain the ORG-A0 pre-entry feature set required for S1_F5/C1/C2/C3/C4 filtering. The script therefore does not create proxy features from lifecycle fields and does not tune thresholds on R50.

Rescue audit also checked `/root/Gho`, `/root/Gho-tsv2-a1-a2-clean`, `/tmp`, `logs/rollout/**`, `logs/shadow_run/**`, `reports/selector/**`, `PLANS/AUDYT/**`, ORG-A0 intermediate artifacts, and joined/inventory/threshold CSV candidates. It found R49 lifecycle/entry/probe/event artifacts, but no R49 decision-time ORG-A0 feature surface. Existing R49 A2 attribution reports explicitly mark the pre-entry fields as `missing evidence: field unavailable`.

This means the EIX hypothesis is not numerically falsified. It is unevaluable in the current local evidence set because the R49 pre-entry feature surface is unavailable.

## Output Files

- `{SUMMARY_CSV}`
- `{STABILITY_CSV}`
- `{COST_CSV}`
- `{TAIL_CSV}`
- `{THRESHOLD_CSV}`
- `{ADR_MD}`

## Final Verdict

`{result['final_verdict']}`

No runtime approval.
No `shadow_close_only` approval.
No active close.
No R51.

If the missing R49 pre-entry decision evidence cannot be recovered, active TSV2/ORG entry+exit intersection is closed as unevaluable for runtime in this evidence set. Any later retry must keep this fixed manifest and still cannot add masks, thresholds, or R49/R50 retuning.

## POST-EIX Contingency

- Do not start R51 from this result.
- Do not promote TSV2, ORG-A0, or an entry+exit intersection into runtime.
- Keep TSV2/ORG as diagnostic/logging-only evidence.
- The only rescue path is archival evidence recovery: recover the original R49 `gatekeeper_v2_decisions.jsonl` or an equivalent materialized pre-entry feature snapshot, then rerun this same fixed EIX script without changing masks, thresholds, or target/stop/hold cells.
- If the original R49 pre-entry evidence cannot be recovered, close EIX as `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`.
"""
    REPORT_MD.write_text(report)

    adr = f"""# ADR-8D: TSV2 entry+exit intersection offline proof

Status: {result['final_verdict']}
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Autor/Agent: Codex
Zakres: PR-TSV2-EIX-A0
Poziom ryzyka: MEDIUM

## 1. Decision

PR-TSV2-EIX-A0 was implemented as an offline-only proof script and report set. No runtime path was changed.

Final verdict: `{result['final_verdict']}`

## 2. Scope

- R49: `{R49_SCOPE}`
- R50: `{R50_SCOPE}`

## 3. Evidence

{evidence_md}

## 4. Fixed Inputs

- Entry cohorts: `{', '.join(ENTRY_COHORTS)}`
- Exit masks: `{', '.join(EXIT_MASKS)}`
- Exit cells: `{'; '.join(str(cell) for cell in EXIT_CELLS)}`
- New masks: `false`
- New thresholds: `false`
- R50 retuning: `false`

## 5. Result

- `fixed_rules_tested_count = {result['fixed_rules_tested_count']}`
- `entry_exit_evaluable_rows = {result['entry_exit_evaluable_rows']}`
- `passing_fixed_rules_count = {result['passing_fixed_rules_count']}`
- `best_fixed_rule = {result['best_fixed_rule']}`
- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `raw_jsonl_committed = false`

Blocking reason: `{result['blocking_reason']}`

Reason: R49 pre-entry feature surface unavailable.

EIX hypothesis not falsified numerically. It is blocked by missing decision-time pre-entry evidence.

## 6. Consequences

The current local evidence set cannot prove a fixed ORG-A0 entry cohort plus TSV2 exit mask/cell intersection across R49 and R50, because R49 pre-entry decision/materialized feature evidence is missing. The proof remains offline-only and gives no basis for runtime change, `shadow_close_only`, active close, Gatekeeper policy change, selector runtime change, alpha hook, XGBoost, or TX/Jito/live path change.

No R51 is approved from this result.

POST-EIX contingency: recover original R49 pre-entry decision/materialized feature evidence and rerun the same fixed EIX script, or close EIX as `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`. Do not add masks, thresholds, R50 retuning, runtime changes, active close, or `shadow_close_only`.
"""
    ADR_MD.write_text(adr)


def main() -> None:
    args = parse_args()
    global REPORT_DIR
    REPORT_DIR = args.reports_dir

    evidences = [
        discover_scope(args.r49_scope, args.reports_dir, args.local_logs_root, args.volume_logs_root),
        discover_scope(args.r50_scope, args.reports_dir, args.local_logs_root, args.volume_logs_root),
    ]
    summary_rows = build_summary_rows(evidences)
    stability_rows = build_stability_rows(evidences)
    cost_rows = build_cost_rows(evidences)
    tail_rows = build_tail_rows(evidences)
    threshold_rows = build_threshold_rows()
    result = evaluate_final(summary_rows, evidences)

    write_csv(SUMMARY_CSV, summary_rows)
    write_csv(STABILITY_CSV, stability_rows)
    write_csv(COST_CSV, cost_rows)
    write_csv(TAIL_CSV, tail_rows)
    write_csv(THRESHOLD_CSV, threshold_rows)
    write_reports(summary_rows, threshold_rows, evidences, result)

    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
