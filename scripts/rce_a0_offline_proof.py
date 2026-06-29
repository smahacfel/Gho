#!/usr/bin/env python3
"""PR-RCE-A0 offline Regime-Confirmed Entry proof.

Offline-only. This script reads existing JSONL evidence, writes reports, and
does not change runtime, Gatekeeper, BUY/REJECT, selector policy, TX/Jito/live
paths, alpha hooks, sidecars, active close, or shadow_close_only.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import json
import math
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Mapping

sys.dont_write_bytecode = True

import time_stop_v2_counterfactual_lab as lab


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"
R51_SCOPE = "shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1"

REPORT_DIR = Path("reports/selector")
SUMMARY_CSV = REPORT_DIR / "rce_a0_summary.csv"
COST_CSV = REPORT_DIR / "rce_a0_cost_sensitivity.csv"
STABILITY_CSV = REPORT_DIR / "rce_a0_stability.csv"
TAIL_CSV = REPORT_DIR / "rce_a0_tail_audit.csv"
THRESHOLD_CSV = REPORT_DIR / "rce_a0_threshold_manifest.csv"
REPORT_MD = Path("PLANS/AUDYT/RAPORT_RCE_A0_OFFLINE_PROOF_20260629.md")
ADR_MD = Path("docs/ADR/ADR_8D_RCE_A0_RESULT_20260629.md")

LOCAL_LOGS_ROOT = Path("logs")
VOLUME_LOGS_ROOT = Path("/mnt/HC_Volume_105935807/logs")

TEMPLATES = (
    "T1_BREAKOUT_RETEST_RECLAIM",
    "T2_STAIRSTEP_CONTINUATION",
    "T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY",
)
TARGET_BPS_GRID = (600, 900, 1200)
STOP_BPS_GRID = (-250, -400, -600)
MAX_HOLD_MS_GRID = (10000, 20000, 30000)
COSTS_BPS = (100, 200)
SEGMENTS = ("train", "validation", "holdout")

VERDICT_BLOCKED = "RCE_BLOCKED_BY_DATA"
VERDICT_REJECTED = "RCE_REJECTED"
VERDICT_SINGLE = "RCE_PROMISING_SINGLE_SCOPE_ONLY"
VERDICT_TWO_SCOPE = "RCE_PROMISING_TWO_SCOPE_OFFLINE_ONLY"


@dataclass(frozen=True)
class ScopeEvidence:
    scope: str
    decision_log: Path | None
    exit_replay: Path | None
    shadow_lifecycle: Path | None
    probe_lifecycle: Path | None
    has_pre_entry_path_summary_v1: bool
    has_session_regime_snapshot_v1: bool
    sampled_decision_rows: int

    @property
    def has_full_surface(self) -> bool:
        return (
            self.decision_log is not None
            and self.exit_replay is not None
            and self.has_pre_entry_path_summary_v1
            and self.has_session_regime_snapshot_v1
        )


@dataclass
class JoinedScope:
    scope: str
    evidence: ScopeEvidence
    decision_rows: int
    replay_rows: int
    joined_records: list[dict[str, Any]]
    unjoined_replay_rows: int
    join_rate: float
    malformed_decision_rows: int
    malformed_replay_rows: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline PR-RCE-A0 proof.")
    parser.add_argument(
        "--scope",
        action="append",
        default=None,
        help="Scope to evaluate. Can be repeated. Defaults to R49/R50/R51 evidence check.",
    )
    parser.add_argument("--local-logs-root", type=Path, default=LOCAL_LOGS_ROOT)
    parser.add_argument("--volume-logs-root", type=Path, default=VOLUME_LOGS_ROOT)
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    return parser.parse_args()


def safe_div(num: float, den: float) -> float:
    return num / den if den else 0.0


def as_float(value: Any) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if math.isfinite(parsed) else None


def as_int(value: Any) -> int | None:
    parsed = as_float(value)
    if parsed is None:
        return None
    return int(round(parsed))


def median(values: list[int]) -> float:
    return float(statistics.median(values)) if values else 0.0


def mean(values: list[int]) -> float:
    return float(sum(values) / len(values)) if values else 0.0


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


def choose_decision_log(scope: str, local_logs_root: Path, volume_logs_root: Path) -> Path | None:
    bases = [
        local_logs_root / "rollout" / scope / "decisions" / scope,
        volume_logs_root / "rollout" / scope / "decisions" / scope,
    ]
    paths: list[Path] = []
    for base in bases:
        if base.exists():
            paths.extend(base.glob("**/gatekeeper_v2_decisions.jsonl"))
    paths.sort(key=lambda p: ("/v2.5/v25_shadow/" not in str(p), str(p)))
    return paths[0] if paths else None


def shadow_bases(scope: str, local_logs_root: Path, volume_logs_root: Path) -> list[Path]:
    return [
        local_logs_root / "shadow_run" / scope,
        volume_logs_root / "shadow_run" / scope,
    ]


def nested_dict(row: Mapping[str, Any], *path: str) -> dict[str, Any] | None:
    current: Any = row
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current if isinstance(current, dict) else None


def sample_rce_surface(path: Path | None, max_rows: int = 50) -> tuple[bool, bool, int]:
    if path is None:
        return False, False, 0
    has_path = False
    has_regime = False
    sampled = 0
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            if sampled >= max_rows:
                break
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(row, dict):
                continue
            sampled += 1
            snapshot = nested_dict(row, "materialized_feature_snapshot") or nested_dict(
                row, "v3_materialized_feature_snapshot"
            )
            if snapshot is None:
                continue
            has_path = has_path or isinstance(snapshot.get("pre_entry_path_summary_v1"), dict)
            has_regime = has_regime or isinstance(snapshot.get("session_regime_snapshot_v1"), dict)
            if has_path and has_regime:
                break
    return has_path, has_regime, sampled


def discover_evidence(scope: str, local_logs_root: Path, volume_logs_root: Path) -> ScopeEvidence:
    decision_log = choose_decision_log(scope, local_logs_root, volume_logs_root)
    has_path, has_regime, sampled = sample_rce_surface(decision_log)
    bases = shadow_bases(scope, local_logs_root, volume_logs_root)
    return ScopeEvidence(
        scope=scope,
        decision_log=decision_log,
        exit_replay=first_existing(base / "shadow_exit_replay_v1.jsonl" for base in bases),
        shadow_lifecycle=first_existing(base / "shadow_lifecycle.jsonl" for base in bases),
        probe_lifecycle=first_existing(base / "probe_shadow_lifecycle.jsonl" for base in bases),
        has_pre_entry_path_summary_v1=has_path,
        has_session_regime_snapshot_v1=has_regime,
        sampled_decision_rows=sampled,
    )


def parse_join_ts(row: Mapping[str, Any]) -> int | None:
    for field in (
        "decision_ts_ms",
        "timestamp_ms",
        "ab_t_end_event_ts_ms",
        "event_ts_ms",
        "first_seen_ts_ms",
    ):
        value = as_int(row.get(field))
        if value is not None:
            return value
    join_key = row.get("join_key")
    if isinstance(join_key, str):
        suffix = join_key.rsplit(":", 1)[-1]
        if suffix.isdigit():
            return int(suffix)
    return None


def load_decisions(path: Path) -> tuple[int, int, dict[tuple[str, str], list[dict[str, Any]]]]:
    rows = 0
    malformed = 0
    index: dict[tuple[str, str], list[dict[str, Any]]] = {}
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if not isinstance(row, dict):
                malformed += 1
                continue
            rows += 1
            pool_id = row.get("pool_id")
            base_mint = row.get("base_mint")
            ts_ms = parse_join_ts(row)
            snapshot = nested_dict(row, "materialized_feature_snapshot") or nested_dict(
                row, "v3_materialized_feature_snapshot"
            )
            if not pool_id or not base_mint or ts_ms is None or snapshot is None:
                continue
            path_summary = snapshot.get("pre_entry_path_summary_v1")
            regime = snapshot.get("session_regime_snapshot_v1")
            if not isinstance(path_summary, dict) or not isinstance(regime, dict):
                continue
            index.setdefault((str(pool_id), str(base_mint)), []).append(
                {
                    "ts_ms": ts_ms,
                    "path": path_summary,
                    "regime": regime,
                }
            )
    for records in index.values():
        records.sort(key=lambda item: int(item["ts_ms"]))
    return rows, malformed, index


def load_replay(path: Path) -> tuple[int, int, list[dict[str, Any]]]:
    rows = 0
    malformed = 0
    records: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if not isinstance(row, dict):
                malformed += 1
                continue
            rows += 1
            records.append(row)
    return rows, malformed, records


def latest_before(records: list[dict[str, Any]], entry_ts_ms: int) -> dict[str, Any] | None:
    timestamps = [int(row["ts_ms"]) for row in records]
    idx = bisect.bisect_right(timestamps, entry_ts_ms) - 1
    return records[idx] if idx >= 0 else None


def segment_for(index: int, total: int) -> str:
    ratio = index / total if total else 0.0
    if ratio < 1 / 3:
        return "train"
    if ratio < 2 / 3:
        return "validation"
    return "holdout"


def join_scope(evidence: ScopeEvidence) -> JoinedScope:
    if evidence.decision_log is None or evidence.exit_replay is None:
        return JoinedScope(evidence.scope, evidence, 0, 0, [], 0, 0.0, 0, 0)
    decision_rows, decision_malformed, index = load_decisions(evidence.decision_log)
    replay_rows, replay_malformed, replays = load_replay(evidence.exit_replay)
    joined: list[dict[str, Any]] = []
    unjoined = 0
    replays.sort(key=lambda row: (as_int(row.get("entry_ts_ms")) or 0, str(row.get("pool_id") or "")))
    for replay in replays:
        pool_id = replay.get("pool_id")
        base_mint = replay.get("base_mint")
        entry_ts_ms = as_int(replay.get("entry_ts_ms"))
        if not pool_id or not base_mint or entry_ts_ms is None:
            unjoined += 1
            continue
        decision = latest_before(index.get((str(pool_id), str(base_mint)), []), entry_ts_ms)
        if decision is None:
            unjoined += 1
            continue
        joined.append(
            {
                "scope": evidence.scope,
                "entry_ts_ms": entry_ts_ms,
                "replay": replay,
                "path": decision["path"],
                "regime": decision["regime"],
            }
        )
    for idx, row in enumerate(joined):
        row["segment"] = segment_for(idx, len(joined))
    return JoinedScope(
        scope=evidence.scope,
        evidence=evidence,
        decision_rows=decision_rows,
        replay_rows=replay_rows,
        joined_records=joined,
        unjoined_replay_rows=unjoined,
        join_rate=safe_div(float(len(joined)), float(replay_rows)),
        malformed_decision_rows=decision_malformed,
        malformed_replay_rows=replay_malformed,
    )


def nf(row: Mapping[str, Any], field: str, default: float | None = None) -> float | None:
    value = as_float(row.get(field))
    return default if value is None else value


def nint(row: Mapping[str, Any], field: str, default: int | None = None) -> int | None:
    value = as_int(row.get(field))
    return default if value is None else value


def le(value: float | None, threshold: float) -> bool:
    return value is not None and value <= threshold


def ge(value: float | int | None, threshold: float) -> bool:
    return value is not None and float(value) >= threshold


def t1(record: Mapping[str, Any]) -> bool:
    path = record["path"]
    regime = record["regime"]
    impulse = ge(nf(path, "pre_entry_ret_10s"), 500.0) or ge(nf(path, "pre_entry_mfe_10s"), 900.0)
    controlled_pullback = ge(nf(path, "pullback_depth_bps"), 100.0) and le(
        nf(path, "pullback_depth_bps"), 900.0
    )
    reclaim = ge(nf(path, "reclaim_fraction"), 0.45) and ge(nf(path, "reclaim_bps"), 150.0)
    concentration_ok = le(nf(regime, "top3_signer_volume_ratio_drift"), 0.10) and ge(
        nf(regime, "unique_ratio_drift"), -0.15
    )
    return impulse and controlled_pullback and reclaim and concentration_ok


def t2(record: Mapping[str, Any]) -> bool:
    path = record["path"]
    return (
        ge(nint(path, "higher_low_count"), 2)
        and ge(nint(path, "above_0bps_dwell_ms"), 10_000)
        and ge(nint(path, "above_300bps_dwell_ms"), 3_000)
        and ge(nf(path, "pre_entry_ret_20s"), 300.0)
        and ge(nf(path, "pre_entry_mae_20s"), -600.0)
    )


def t3(record: Mapping[str, Any]) -> bool:
    path = record["path"]
    regime = record["regime"]
    hot_session = ge(nf(regime, "session_pool_rate_5m"), 10.0)
    reclaim = ge(nf(path, "reclaim_fraction"), 0.50) and ge(nf(path, "reclaim_bps"), 200.0)
    toxicity_decay = ge(nf(regime, "same_ms_tx_ratio_decay"), 0.0) and ge(
        nf(regime, "burst_ratio_decay"), 0.0
    )
    concentration_ok = le(nf(regime, "top3_signer_volume_ratio_drift"), 0.05)
    return hot_session and reclaim and toxicity_decay and concentration_ok


TEMPLATE_FUNCS: dict[str, Callable[[Mapping[str, Any]], bool]] = {
    "T1_BREAKOUT_RETEST_RECLAIM": t1,
    "T2_STAIRSTEP_CONTINUATION": t2,
    "T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY": t3,
}


def max_consecutive_losses(pnls: list[int]) -> int:
    best = current = 0
    for pnl in pnls:
        if pnl < 0:
            current += 1
            best = max(best, current)
        else:
            current = 0
    return best


def simulate(records: list[dict[str, Any]], target_bps: int, stop_bps: int, max_hold_ms: int) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for record in records:
        baseline = lab.simulate_baseline_cached(record["replay"], target_bps, stop_bps, max_hold_ms)
        if baseline is None:
            continue
        out.append(
            {
                "scope": record["scope"],
                "segment": record["segment"],
                "entry_ts_ms": record["entry_ts_ms"],
                "result": baseline.result,
                "pnl_bps": int(baseline.pnl_bps),
            }
        )
    return out


def tail_after_removal(pnls: list[int], fraction: float) -> dict[str, Any]:
    positives = sorted([pnl for pnl in pnls if pnl > 0], reverse=True)
    remove_count = min(len(positives), math.ceil(len(pnls) * fraction)) if pnls else 0
    removed = positives[:remove_count]
    removed_left = removed.copy()
    remaining: list[int] = []
    for pnl in sorted(pnls, reverse=True):
        if pnl > 0 and removed_left and pnl == removed_left[0]:
            removed_left.pop(0)
        else:
            remaining.append(pnl)
    return {
        "removed_count": remove_count,
        "removed_sum_bps": int(sum(removed)),
        "remaining_sum_bps": int(sum(remaining)),
        "remaining_median_bps": median(remaining),
    }


def metrics(rows: list[dict[str, Any]], cost_bps: int) -> dict[str, Any]:
    gross = [int(row["pnl_bps"]) for row in rows]
    after_cost = [pnl - cost_bps for pnl in gross]
    nonloss = sum(1 for pnl in after_cost if pnl >= 0)
    count = len(after_cost)
    return {
        "count": count,
        "target_count": sum(1 for row in rows if row["result"] == lab.TARGET),
        "stop_count": sum(1 for row in rows if row["result"] == lab.STOP),
        "timeout_count": sum(1 for row in rows if row["result"] == lab.TIMEOUT),
        "negative_timeout_count": sum(
            1 for row in rows if row["result"] == lab.TIMEOUT and int(row["pnl_bps"]) < 0
        ),
        "sum_gross_bps": int(sum(gross)),
        "avg_gross_bps": mean(gross),
        "median_gross_bps": median(gross),
        f"sum_cost{cost_bps}_bps": int(sum(after_cost)),
        f"avg_cost{cost_bps}_bps": mean(after_cost),
        f"median_cost{cost_bps}_bps": median(after_cost),
        f"precision_cost{cost_bps}": safe_div(float(nonloss), float(count)),
        f"wilson_lower95_cost{cost_bps}": lab.wilson_lower_bound(nonloss, count),
        f"max_consecutive_losses_cost{cost_bps}": max_consecutive_losses(after_cost),
    }


def internal_holdout_pass(stability_rows: list[dict[str, Any]], key: tuple[str, str, int, int, int]) -> bool:
    rows = [
        row
        for row in stability_rows
        if (
            row["scope"],
            row["template"],
            int(row["target_bps"]),
            int(row["stop_bps"]),
            int(row["max_hold_ms"]),
        )
        == key
    ]
    if {row["segment"] for row in rows} != set(SEGMENTS):
        return False
    return all(
        int(row["selected_count"] or 0) > 0
        and int(row["cost100_sum_pnl_bps"] or 0) > 0
        and float(row["median_cost100_bps"] or 0.0) >= 0.0
        and float(row["precision_cost100"] or 0.0) >= 0.55
        for row in rows
    )


def evaluate_scope(scope_data: JoinedScope) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    summary_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    tail_rows: list[dict[str, Any]] = []
    for template in TEMPLATES:
        selected = [row for row in scope_data.joined_records if TEMPLATE_FUNCS[template](row)]
        for target_bps in TARGET_BPS_GRID:
            for stop_bps in STOP_BPS_GRID:
                for max_hold_ms in MAX_HOLD_MS_GRID:
                    evaluated = simulate(selected, target_bps, stop_bps, max_hold_ms)
                    m100 = metrics(evaluated, 100)
                    m200 = metrics(evaluated, 200)
                    cost100_pnls = [int(row["pnl_bps"]) - 100 for row in evaluated]
                    tail5 = tail_after_removal(cost100_pnls, 0.05)
                    tail10 = tail_after_removal(cost100_pnls, 0.10)
                    key = (scope_data.scope, template, target_bps, stop_bps, max_hold_ms)
                    for segment in SEGMENTS:
                        segment_rows = [row for row in evaluated if row["segment"] == segment]
                        sm100 = metrics(segment_rows, 100)
                        sm200 = metrics(segment_rows, 200)
                        stability_rows.append(
                            {
                                "scope": scope_data.scope,
                                "template": template,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "segment": segment,
                                "selected_count": len(segment_rows),
                                "precision_cost100": sm100["precision_cost100"],
                                "wilson_lower95_cost100": sm100["wilson_lower95_cost100"],
                                "cost100_sum_pnl_bps": sm100["sum_cost100_bps"],
                                "median_cost100_bps": sm100["median_cost100_bps"],
                                "cost200_sum_pnl_bps": sm200["sum_cost200_bps"],
                                "median_cost200_bps": sm200["median_cost200_bps"],
                            }
                        )
                    holdout_ok = internal_holdout_pass(stability_rows, key)
                    failures = []
                    if scope_data.join_rate < 0.98:
                        failures.append("join_quality_lt_98pct")
                    if len(evaluated) < 250:
                        failures.append("selected_count_lt_250")
                    if m100["precision_cost100"] < 0.65:
                        failures.append("precision_cost100_lt_65pct")
                    if m100["wilson_lower95_cost100"] < 0.60:
                        failures.append("wilson_lower95_lt_60pct")
                    if m100["sum_cost100_bps"] <= 0:
                        failures.append("cost100_sum_not_positive")
                    if m200["sum_cost200_bps"] < 0:
                        failures.append("cost200_sum_negative")
                    if m100["median_cost100_bps"] < 0:
                        failures.append("median_cost100_negative")
                    if tail5["remaining_sum_bps"] < 0:
                        failures.append("top5_tail_removed_negative")
                    if not holdout_ok:
                        failures.append("internal_holdout_fail")
                    passes = not failures
                    summary_rows.append(
                        {
                            "scope": scope_data.scope,
                            "template": template,
                            "target_bps": target_bps,
                            "stop_bps": stop_bps,
                            "max_hold_ms": max_hold_ms,
                            "selected_count": len(evaluated),
                            "selected_pct": safe_div(float(len(evaluated)), float(scope_data.replay_rows)),
                            "exact_join_rate": scope_data.join_rate,
                            "target_rate": safe_div(float(m100["target_count"]), float(len(evaluated))),
                            "stop_rate": safe_div(float(m100["stop_count"]), float(len(evaluated))),
                            "timeout_rate": safe_div(float(m100["timeout_count"]), float(len(evaluated))),
                            "negative_timeout_rate": safe_div(float(m100["negative_timeout_count"]), float(len(evaluated))),
                            "avg_pnl_bps": m100["avg_gross_bps"],
                            "median_pnl_bps": m100["median_gross_bps"],
                            "sum_pnl_bps": m100["sum_gross_bps"],
                            "precision_cost100": m100["precision_cost100"],
                            "wilson_lower95": m100["wilson_lower95_cost100"],
                            "cost100_sum_pnl_bps": m100["sum_cost100_bps"],
                            "cost100_avg_pnl_bps": m100["avg_cost100_bps"],
                            "median_cost100": m100["median_cost100_bps"],
                            "cost200_sum_pnl_bps": m200["sum_cost200_bps"],
                            "cost200_avg_pnl_bps": m200["avg_cost200_bps"],
                            "median_cost200": m200["median_cost200_bps"],
                            "result_after_removing_top_5pct_positive": tail5["remaining_sum_bps"],
                            "result_after_removing_top_10pct_positive": tail10["remaining_sum_bps"],
                            "internal_holdout_pass": holdout_ok,
                            "passes_single_scope": passes,
                            "acceptance_failures": ";".join(failures),
                        }
                    )
                    for cost, metric in ((100, m100), (200, m200)):
                        cost_rows.append(
                            {
                                "scope": scope_data.scope,
                                "template": template,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "cost_bps": cost,
                                "selected_count": len(evaluated),
                                "sum_pnl_after_cost_bps": metric[f"sum_cost{cost}_bps"],
                                "avg_pnl_after_cost_bps": metric[f"avg_cost{cost}_bps"],
                                "median_pnl_after_cost_bps": metric[f"median_cost{cost}_bps"],
                                "precision": metric[f"precision_cost{cost}"],
                                "wilson_lower95": metric[f"wilson_lower95_cost{cost}"],
                                "max_consecutive_losses": metric[f"max_consecutive_losses_cost{cost}"],
                            }
                        )
                    for fraction, tail in ((0.05, tail5), (0.10, tail10)):
                        tail_rows.append(
                            {
                                "scope": scope_data.scope,
                                "template": template,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "cost_bps": 100,
                                "top_positive_fraction_removed": fraction,
                                "selected_count": len(evaluated),
                                "removed_count": tail["removed_count"],
                                "removed_sum_bps": tail["removed_sum_bps"],
                                "remaining_sum_bps": tail["remaining_sum_bps"],
                                "remaining_median_bps": tail["remaining_median_bps"],
                            }
                        )
    return summary_rows, cost_rows, stability_rows, tail_rows


def threshold_manifest_rows() -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = [
        {
            "family": "template",
            "name": "T1_BREAKOUT_RETEST_RECLAIM",
            "rule": "impulse + controlled pullback + reclaim + concentration not worsening",
            "source": "PR-RCE-A0 predeclared",
            "runtime_input": False,
        },
        {
            "family": "template",
            "name": "T2_STAIRSTEP_CONTINUATION",
            "rule": "higher_low_count>=2 + dwell above levels + MAE guard",
            "source": "PR-RCE-A0 predeclared",
            "runtime_input": False,
        },
        {
            "family": "template",
            "name": "T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY",
            "rule": "hot session rate + reclaim + toxicity decay + concentration guard",
            "source": "PR-RCE-A0 predeclared",
            "runtime_input": False,
        },
    ]
    for target in TARGET_BPS_GRID:
        rows.append({"family": "exit_grid", "name": "target_bps", "rule": str(target), "source": "fixed", "runtime_input": False})
    for stop in STOP_BPS_GRID:
        rows.append({"family": "exit_grid", "name": "stop_bps", "rule": str(stop), "source": "fixed", "runtime_input": False})
    for hold in MAX_HOLD_MS_GRID:
        rows.append({"family": "exit_grid", "name": "max_hold_ms", "rule": str(hold), "source": "fixed", "runtime_input": False})
    for forbidden in (
        "final_pnl",
        "target",
        "stop",
        "timeout",
        "path_after_decision_horizon",
        "pool_id_as_feature",
        "mint_as_feature",
        "signer_id_as_feature",
    ):
        rows.append({"family": "forbidden", "name": forbidden, "rule": "not_used", "source": "leakage_guard", "runtime_input": False})
    return rows


def write_csv(path: Path, rows: list[Mapping[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fields: list[str] = []
    for row in rows:
        for key in row:
            if key not in fields:
                fields.append(key)
    if not fields:
        fields = ["empty"]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow(dict(row))


def evidence_rows(evidences: list[ScopeEvidence]) -> list[dict[str, Any]]:
    return [
        {
            "scope": evidence.scope,
            "has_decision_log": evidence.decision_log is not None,
            "has_exit_replay": evidence.exit_replay is not None,
            "has_shadow_lifecycle": evidence.shadow_lifecycle is not None,
            "has_probe_shadow_lifecycle": evidence.probe_lifecycle is not None,
            "has_pre_entry_path_summary_v1": evidence.has_pre_entry_path_summary_v1,
            "has_session_regime_snapshot_v1": evidence.has_session_regime_snapshot_v1,
            "sampled_decision_rows": evidence.sampled_decision_rows,
            "full_rce_surface": evidence.has_full_surface,
            "decision_log": str(evidence.decision_log or ""),
            "exit_replay": str(evidence.exit_replay or ""),
        }
        for evidence in evidences
    ]


def best_rows(rows: list[dict[str, Any]], limit: int = 8) -> list[dict[str, Any]]:
    return sorted(
        rows,
        key=lambda row: (
            row.get("passes_single_scope") is True,
            int(row.get("cost100_sum_pnl_bps") or 0),
            float(row.get("precision_cost100") or 0.0),
            int(row.get("selected_count") or 0),
        ),
        reverse=True,
    )[:limit]


def markdown_table(rows: list[Mapping[str, Any]], fields: list[str]) -> str:
    out = ["| " + " | ".join(fields) + " |", "| " + " | ".join(["---"] * len(fields)) + " |"]
    for row in rows:
        out.append("| " + " | ".join(str(row.get(field, "")) for field in fields) + " |")
    return "\n".join(out)


def final_verdict(evidences: list[ScopeEvidence], summary_rows: list[dict[str, Any]]) -> dict[str, Any]:
    full_scopes = [evidence.scope for evidence in evidences if evidence.has_full_surface]
    passing_by_rule: dict[tuple[str, int, int, int], set[str]] = {}
    for row in summary_rows:
        if row.get("passes_single_scope") is True:
            key = (
                str(row["template"]),
                int(row["target_bps"]),
                int(row["stop_bps"]),
                int(row["max_hold_ms"]),
            )
            passing_by_rule.setdefault(key, set()).add(str(row["scope"]))
    passing_two_scope = [key for key, scopes in passing_by_rule.items() if len(scopes) >= 2]
    passing_single = [key for key, scopes in passing_by_rule.items() if scopes]
    if not full_scopes:
        verdict = VERDICT_BLOCKED
    elif passing_two_scope:
        verdict = VERDICT_TWO_SCOPE
    elif passing_single:
        verdict = VERDICT_SINGLE
    else:
        verdict = VERDICT_REJECTED
    best = best_rows(summary_rows, 1)
    return {
        "verdict": verdict,
        "full_surface_scope_count": len(full_scopes),
        "full_surface_scopes": ";".join(full_scopes),
        "passing_fixed_rule_count": len(passing_two_scope),
        "single_scope_passing_rule_count": len(passing_single),
        "best_rule": "" if not best else f"{best[0]['template']}/{best[0]['target_bps']}/{best[0]['stop_bps']}/{best[0]['max_hold_ms']}",
        "best_scope": "" if not best else best[0]["scope"],
        "runtime_approval": False,
        "shadow_close_only_approval": False,
    }


def write_reports(
    evidences: list[ScopeEvidence],
    joined_scopes: list[JoinedScope],
    summary_rows: list[dict[str, Any]],
    result: Mapping[str, Any],
) -> None:
    REPORT_MD.parent.mkdir(parents=True, exist_ok=True)
    ADR_MD.parent.mkdir(parents=True, exist_ok=True)
    evidence_md = markdown_table(
        evidence_rows(evidences),
        [
            "scope",
            "has_decision_log",
            "has_exit_replay",
            "has_pre_entry_path_summary_v1",
            "has_session_regime_snapshot_v1",
            "full_rce_surface",
        ],
    )
    best_md = markdown_table(
        best_rows(summary_rows, 8),
        [
            "scope",
            "template",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "selected_count",
            "precision_cost100",
            "wilson_lower95",
            "cost100_sum_pnl_bps",
            "median_cost100",
            "passes_single_scope",
            "acceptance_failures",
        ],
    )
    if result["verdict"] == VERDICT_BLOCKED:
        recommendation = "GO_R51_LOGGING_ONLY wymaga osobnej zgody; bez niej NO_GO_CLOSE_PROJECT."
    elif result["verdict"] == VERDICT_REJECTED:
        recommendation = "NO_GO_CLOSE_PROJECT."
    else:
        recommendation = "NO_RUNTIME. Fresh independent validation nadal wymagana."
    report = f"""# PR-RCE-A0: Offline proof

Data: `2026-06-29`

Final verdict: `{result['verdict']}`

## Decyzja

{recommendation}

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

## Boundary

To jest offline-only proof. Skrypt nie zmienia runtime, Gatekeepera, BUY/REJECT, selector runtime, `v25_confidence`, V3 promotion, TX/Jito/live path, `shadow_close_only`, active close, `alpha_31100` ani XGBoost.

## Evidence

{evidence_md}

Full RCE surface scopes: `{result['full_surface_scopes']}`

Full RCE surface scope count: `{result['full_surface_scope_count']}`

## Templates

- `T1_BREAKOUT_RETEST_RECLAIM`
- `T2_STAIRSTEP_CONTINUATION`
- `T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY`

## Fixed grid

- target_bps: `{', '.join(str(x) for x in TARGET_BPS_GRID)}`
- stop_bps: `{', '.join(str(x) for x in STOP_BPS_GRID)}`
- max_hold_ms: `{', '.join(str(x) for x in MAX_HOLD_MS_GRID)}`
- costs_bps: `{', '.join(str(x) for x in COSTS_BPS)}`

## Best rows

{best_md if summary_rows else "Brak metryk: full RCE evidence surface jest niedostepny."}

## Acceptance

Passing fixed rules across two scopes: `{result['passing_fixed_rule_count']}`

Single-scope passing rules: `{result['single_scope_passing_rule_count']}`

Best rule: `{result['best_rule']}` on `{result['best_scope']}`

## Required next step

Istniejace R49/R50 logs nie zawieraja wymaganej RCE surface. Jedyny dopuszczalny nastepny krok to osobno zatwierdzony R51 logging-only scope. Bez zgody sponsora na jeden taki scope projektowy trading edge search nalezy zamknac.
"""
    REPORT_MD.write_text(report, encoding="utf-8")
    adr = f"""# ADR-8D: PR-RCE-A0 offline result

Status: {result['verdict']} / NO_RUNTIME
Typ: ADR-8D / offline research result
Data: 2026-06-29
Zakres: PR-RCE-A0
Poziom ryzyka: LOW runtime risk / MEDIUM evidence risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Final verdict: `{result['verdict']}`

{recommendation}

## 2. Runtime boundary

Nie zatwierdzono:

- runtime change,
- BUY/REJECT change,
- Gatekeeper policy change,
- selector runtime change,
- `shadow_close_only`,
- active close,
- TX/Jito/live path change,
- `alpha_31100`,
- XGBoost.

## 3. Evidence

{evidence_md}

## 4. Result

Full RCE surface scope count: `{result['full_surface_scope_count']}`

Passing fixed rules across two scopes: `{result['passing_fixed_rule_count']}`

Single-scope passing rules: `{result['single_scope_passing_rule_count']}`

Best rule: `{result['best_rule']}`

## 5. Files

- `scripts/rce_a0_offline_proof.py`
- `{SUMMARY_CSV}`
- `{COST_CSV}`
- `{STABILITY_CSV}`
- `{TAIL_CSV}`
- `{THRESHOLD_CSV}`
- `{REPORT_MD}`
"""
    ADR_MD.write_text(adr, encoding="utf-8")


def main() -> int:
    args = parse_args()
    scopes = args.scope or [R49_SCOPE, R50_SCOPE, R51_SCOPE]
    global REPORT_DIR, SUMMARY_CSV, COST_CSV, STABILITY_CSV, TAIL_CSV, THRESHOLD_CSV
    REPORT_DIR = args.reports_dir
    SUMMARY_CSV = REPORT_DIR / "rce_a0_summary.csv"
    COST_CSV = REPORT_DIR / "rce_a0_cost_sensitivity.csv"
    STABILITY_CSV = REPORT_DIR / "rce_a0_stability.csv"
    TAIL_CSV = REPORT_DIR / "rce_a0_tail_audit.csv"
    THRESHOLD_CSV = REPORT_DIR / "rce_a0_threshold_manifest.csv"

    evidences = [discover_evidence(scope, args.local_logs_root, args.volume_logs_root) for scope in scopes]
    full_evidence = [evidence for evidence in evidences if evidence.has_full_surface]
    joined_scopes = [join_scope(evidence) for evidence in full_evidence]
    summary_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    tail_rows: list[dict[str, Any]] = []
    for scope_data in joined_scopes:
        scope_summary, scope_cost, scope_stability, scope_tail = evaluate_scope(scope_data)
        summary_rows.extend(scope_summary)
        cost_rows.extend(scope_cost)
        stability_rows.extend(scope_stability)
        tail_rows.extend(scope_tail)
    result = final_verdict(evidences, summary_rows)

    if not summary_rows:
        summary_rows = [
            {
                "verdict": result["verdict"],
                "blocking_reason": "missing_pre_entry_path_summary_v1_or_session_regime_snapshot_v1",
                "runtime_approval": False,
                "shadow_close_only_approval": False,
            }
        ]
    write_csv(SUMMARY_CSV, summary_rows)
    write_csv(COST_CSV, cost_rows)
    write_csv(STABILITY_CSV, stability_rows)
    write_csv(TAIL_CSV, tail_rows)
    write_csv(THRESHOLD_CSV, threshold_manifest_rows())
    write_reports(evidences, joined_scopes, [] if result["verdict"] == VERDICT_BLOCKED else summary_rows, result)

    print(f"Final verdict: {result['verdict']}")
    print(f"Full RCE surface scopes: {result['full_surface_scopes']}")
    print(f"Passing fixed rules: {result['passing_fixed_rule_count']}")
    if result["verdict"] == VERDICT_BLOCKED:
        print("Recommendation: GO_R51_LOGGING_ONLY requires separate approval; otherwise NO_GO_CLOSE_PROJECT")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
