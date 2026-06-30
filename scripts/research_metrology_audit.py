#!/usr/bin/env python3
"""P0 research metrology audit.

Offline-only. This script audits simulator, replay, metric, horizon, and
configuration-measurement contracts for the ORG/TSV2/EIX/RTP/RUG/RCE research
line. It reads existing reports and local JSONL evidence, writes compact CSV/MD
reports, and never mutates runtime state or raw logs.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Mapping

sys.dont_write_bytecode = True


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"
R51_SCOPE = "shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1"
R48_R2_SCOPE = "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2"

REPORT_DIR = Path("reports/selector")
SUMMARY_CSV = REPORT_DIR / "research_metrology_audit_summary.csv"
FIXTURES_CSV = REPORT_DIR / "research_metrology_audit_simulation_fixtures.csv"
CONFIG_CSV = REPORT_DIR / "research_metrology_audit_config_sensitivity.csv"
HORIZON_CSV = REPORT_DIR / "research_metrology_audit_horizon_sensitivity.csv"
METRIC_CSV = REPORT_DIR / "research_metrology_audit_metric_consistency.csv"
RECON_CSV = REPORT_DIR / "research_metrology_audit_replay_lifecycle_reconciliation.csv"
REPORT_MD = Path("PLANS/AUDYT/RAPORT_RESEARCH_METROLOGY_AUDIT_20260629.md")
ADR_MD = Path("docs/ADR/ADR_8D_RESEARCH_METROLOGY_AUDIT_20260629.md")

LOCAL_LOGS_ROOT = Path("logs")
VOLUME_LOGS_ROOT = Path("/mnt/HC_Volume_105935807/logs")
HORIZONS_MS = (1000, 2000, 3000, 5000, 10000, 20000, 30000, 60000, 120000, 300000, 500000)
TIE_VARIANTS = (
    "stop_first_on_equal_timestamp",
    "target_first_on_equal_timestamp",
    "ambiguous_tie_excluded",
    "path_approx_fallback_only",
    "exact_levels_only",
)
EVALUABLE_COVERAGE_MIN = 0.80
FLOAT_TOLERANCE = 1e-6


@dataclass(frozen=True)
class ExitParams:
    target_bps: int
    stop_bps: int
    max_hold_ms: int


@dataclass
class ReplayRow:
    scope: str
    path: Path
    line_no: int
    run_id: str
    session_id: str
    pool_id: str
    base_mint: str
    entry_ts_ms: int | None
    candidate_id: str
    path_bps: list[tuple[int, int]]
    first_hit_ms: dict[int, int]
    close_age_ms: int | None
    horizon_ms: int | None
    last_pnl_bps: int | None
    quality: str
    malformed_reason: str = ""

    @property
    def key(self) -> tuple[str, str, str, str, int | None]:
        return (self.run_id, self.session_id, self.pool_id, self.base_mint, self.entry_ts_ms)

    @property
    def candidate_key(self) -> tuple[str, str, str]:
        return (self.run_id, self.pool_id, self.base_mint)


@dataclass
class LifecycleTerminal:
    scope: str
    path: Path
    line_no: int
    record_type: str
    run_id: str
    session_id: str
    pool_id: str
    base_mint: str
    candidate_id: str
    position_id: str
    close_reason: str
    final_pnl_bps: int | None
    final_pnl_pct: float | None
    duration_ms: int | None
    timestamp_ms: int | None
    entry_ts_from_candidate: int | None

    @property
    def key(self) -> tuple[str, str, str, str, int | None]:
        return (
            self.run_id,
            self.session_id,
            self.pool_id,
            self.base_mint,
            self.entry_ts_from_candidate,
        )

    @property
    def candidate_key(self) -> tuple[str, str, str]:
        return (self.run_id, self.pool_id, self.base_mint)


@dataclass
class ScopeEvidence:
    scope: str
    exit_replay: Path | None = None
    shadow_lifecycle: Path | None = None
    probe_lifecycle: Path | None = None
    decision_logs: list[Path] = field(default_factory=list)
    active_partial: bool = False


@dataclass
class AuditState:
    summary_rows: list[dict[str, Any]] = field(default_factory=list)
    fixture_rows: list[dict[str, Any]] = field(default_factory=list)
    config_rows: list[dict[str, Any]] = field(default_factory=list)
    horizon_rows: list[dict[str, Any]] = field(default_factory=list)
    metric_rows: list[dict[str, Any]] = field(default_factory=list)
    recon_rows: list[dict[str, Any]] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    failures: list[str] = field(default_factory=list)
    verdict: str = "METROLOGY_PASS_WITH_WARNINGS"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline P0 research metrology audit.")
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    parser.add_argument("--local-logs-root", type=Path, default=LOCAL_LOGS_ROOT)
    parser.add_argument("--volume-logs-root", type=Path, default=VOLUME_LOGS_ROOT)
    parser.add_argument(
        "--scope",
        action="append",
        default=None,
        help="Restrict raw replay/lifecycle audit to this scope. Can be repeated.",
    )
    return parser.parse_args()


def as_float(value: Any) -> float | None:
    if value is None or isinstance(value, bool):
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


def safe_div(num: float, den: float) -> float:
    return num / den if den else 0.0


def wilson_lower_bound(successes: int, total: int, z: float = 1.959963984540054) -> float:
    if total <= 0:
        return 0.0
    phat = successes / total
    denom = 1 + z * z / total
    centre = phat + z * z / (2 * total)
    margin = z * math.sqrt((phat * (1 - phat) + z * z / (4 * total)) / total)
    return max(0.0, (centre - margin) / denom)


def max_consecutive(values: Iterable[bool]) -> int:
    best = 0
    current = 0
    for value in values:
        if value:
            current += 1
            best = max(best, current)
        else:
            current = 0
    return best


def median(values: list[int]) -> float:
    return float(statistics.median(values)) if values else 0.0


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def read_jsonl(path: Path) -> Iterable[tuple[int, dict[str, Any] | None, str]]:
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line_no, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                yield line_no, None, str(exc)
                continue
            if not isinstance(row, dict):
                yield line_no, None, "row_not_object"
                continue
            yield line_no, row, ""


def first_existing(paths: Iterable[Path]) -> Path | None:
    for path in paths:
        if path.exists() and path.is_file():
            return path
    return None


def scope_roots(scope: str, local_logs_root: Path, volume_logs_root: Path) -> tuple[list[Path], list[Path]]:
    rollout_roots = [local_logs_root / "rollout" / scope, volume_logs_root / "rollout" / scope]
    shadow_roots = [local_logs_root / "shadow_run" / scope, volume_logs_root / "shadow_run" / scope]
    return rollout_roots, shadow_roots


def discover_scopes(local_logs_root: Path, volume_logs_root: Path) -> dict[str, ScopeEvidence]:
    scopes: dict[str, ScopeEvidence] = {}
    for root in (
        local_logs_root / "shadow_run",
        volume_logs_root / "shadow_run",
        local_logs_root / "rollout",
        volume_logs_root / "rollout",
    ):
        if not root.exists():
            continue
        for child in root.iterdir():
            if child.is_dir():
                scopes.setdefault(child.name, ScopeEvidence(scope=child.name))
    for scope, evidence in scopes.items():
        rollout_roots, shadow_roots = scope_roots(scope, local_logs_root, volume_logs_root)
        evidence.exit_replay = first_existing(root / "shadow_exit_replay_v1.jsonl" for root in shadow_roots)
        evidence.shadow_lifecycle = first_existing(root / "shadow_lifecycle.jsonl" for root in shadow_roots)
        evidence.probe_lifecycle = first_existing(root / "probe_shadow_lifecycle.jsonl" for root in shadow_roots)
        decision_logs: list[Path] = []
        for root in rollout_roots:
            decisions = root / "decisions"
            if decisions.exists():
                decision_logs.extend(sorted(decisions.glob("**/gatekeeper_v2_decisions.jsonl")))
        evidence.decision_logs = decision_logs
        post_manifest = first_existing(root / "post_run_manifest.json" for root in rollout_roots)
        evidence.active_partial = scope == R51_SCOPE and post_manifest is None
    return scopes


def infer_exit_params(scope: str) -> ExitParams:
    if "target12-stop6" in scope:
        return ExitParams(target_bps=1200, stop_bps=-600, max_hold_ms=45000)
    if "target24-stop3" in scope:
        return ExitParams(target_bps=2400, stop_bps=-300, max_hold_ms=45000)
    if "target50-stop50" in scope:
        return ExitParams(target_bps=5000, stop_bps=-5000, max_hold_ms=120000)
    if "target60-stop60" in scope:
        return ExitParams(target_bps=6000, stop_bps=-6000, max_hold_ms=120000)
    return ExitParams(target_bps=6000, stop_bps=-6000, max_hold_ms=120000)


def normalize_path(raw_path: Any) -> list[tuple[int, int]]:
    normalized: list[tuple[int, int]] = []
    if not isinstance(raw_path, list):
        return normalized
    for point in raw_path:
        if not isinstance(point, (list, tuple)) or len(point) < 2:
            continue
        ts = as_int(point[0])
        pnl = as_int(point[1])
        if ts is None or pnl is None:
            continue
        normalized.append((ts, pnl))
    return normalized


def normalize_first_hits(raw_hits: Any) -> dict[int, int]:
    hits: dict[int, int] = {}
    if not isinstance(raw_hits, dict):
        return hits
    for key, value in raw_hits.items():
        level = as_int(key)
        ts = as_int(value)
        if level is not None and ts is not None:
            hits[level] = ts
    return hits


def load_replay_rows(scope: str, path: Path) -> tuple[list[ReplayRow], int]:
    rows: list[ReplayRow] = []
    malformed = 0
    for line_no, row, error in read_jsonl(path):
        if row is None:
            malformed += 1
            continue
        path_bps = normalize_path(row.get("path_bps"))
        first_hit = normalize_first_hits(row.get("first_hit_ms"))
        base_mint = str(row.get("base_mint") or row.get("mint_id") or "")
        rows.append(
            ReplayRow(
                scope=scope,
                path=path,
                line_no=line_no,
                run_id=str(row.get("run_id") or scope),
                session_id=str(row.get("session_id") or ""),
                pool_id=str(row.get("pool_id") or ""),
                base_mint=base_mint,
                entry_ts_ms=as_int(row.get("entry_ts_ms")),
                candidate_id=str(row.get("candidate_id") or ""),
                path_bps=path_bps,
                first_hit_ms=first_hit,
                close_age_ms=as_int(row.get("close_age_ms")),
                horizon_ms=as_int(row.get("horizon_ms")),
                last_pnl_bps=as_int(row.get("last_pnl_bps")),
                quality=str(row.get("quality") or ""),
            )
        )
    return rows, malformed


def parse_candidate_entry_ts(candidate_id: str) -> int | None:
    if not candidate_id:
        return None
    suffix = candidate_id.rsplit("_", 1)[-1]
    if suffix.isdigit():
        return int(suffix)
    return None


def load_lifecycle_terminals(scope: str, path: Path) -> tuple[list[LifecycleTerminal], int]:
    terminals: list[LifecycleTerminal] = []
    malformed = 0
    for line_no, row, error in read_jsonl(path):
        if row is None:
            malformed += 1
            continue
        record_type = str(row.get("record_type") or "")
        if record_type not in {"position_closed", "exit_filled"}:
            continue
        candidate_id = str(row.get("candidate_id") or "")
        final_pnl_pct = as_float(row.get("final_pnl_pct"))
        final_pnl_bps = as_int(row.get("final_pnl_bps"))
        if final_pnl_bps is None and final_pnl_pct is not None:
            final_pnl_bps = int(round(final_pnl_pct * 100))
        terminals.append(
            LifecycleTerminal(
                scope=scope,
                path=path,
                line_no=line_no,
                record_type=record_type,
                run_id=str(row.get("run_id") or scope),
                session_id=str(row.get("session_id") or ""),
                pool_id=str(row.get("pool_id") or ""),
                base_mint=str(row.get("base_mint") or row.get("mint_id") or ""),
                candidate_id=candidate_id,
                position_id=str(row.get("position_id") or ""),
                close_reason=str(row.get("close_reason") or ""),
                final_pnl_bps=final_pnl_bps,
                final_pnl_pct=final_pnl_pct,
                duration_ms=as_int(row.get("duration_ms")),
                timestamp_ms=as_int(row.get("timestamp_ms")),
                entry_ts_from_candidate=parse_candidate_entry_ts(candidate_id),
            )
        )
    return terminals, malformed


def simulate_exit(
    path_bps: list[tuple[int, int]],
    first_hit_ms: dict[int, int],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    variant: str = "target_first_on_equal_timestamp",
) -> dict[str, Any]:
    target_ts = first_hit_ms.get(target_bps)
    stop_ts = first_hit_ms.get(stop_bps)
    used_exact = target_ts is not None or stop_ts is not None
    if variant == "path_approx_fallback_only":
        used_exact = False
        target_ts = None
        stop_ts = None
    if variant == "exact_levels_only" and not used_exact:
        return {
            "result": "NOT_EVALUABLE",
            "pnl_bps": None,
            "exit_age_ms": None,
            "source": "exact_levels_missing",
            "tie": False,
        }
    if not used_exact:
        for ts, pnl in path_bps:
            if ts > max_hold_ms:
                break
            if target_ts is None and pnl >= target_bps:
                target_ts = ts
            if stop_ts is None and pnl <= stop_bps:
                stop_ts = ts
            if target_ts is not None and stop_ts is not None:
                break
    candidates: list[tuple[int, str, int]] = []
    if target_ts is not None and target_ts <= max_hold_ms:
        candidates.append((target_ts, "TARGET", target_bps))
    if stop_ts is not None and stop_ts <= max_hold_ms:
        candidates.append((stop_ts, "STOP", stop_bps))
    tie = len(candidates) == 2 and candidates[0][0] == candidates[1][0]
    if tie and variant == "ambiguous_tie_excluded":
        return {
            "result": "AMBIGUOUS_TIE_EXCLUDED",
            "pnl_bps": None,
            "exit_age_ms": target_ts,
            "source": "exact_levels" if used_exact else "path_approx",
            "tie": True,
        }
    if candidates:
        if tie:
            preferred = "STOP" if variant == "stop_first_on_equal_timestamp" else "TARGET"
            candidates.sort(key=lambda item: (item[0], 0 if item[1] == preferred else 1))
        else:
            candidates.sort(key=lambda item: item[0])
        ts, result, pnl = candidates[0]
        return {
            "result": result,
            "pnl_bps": pnl,
            "exit_age_ms": ts,
            "source": "exact_levels" if used_exact else "path_approx",
            "tie": tie,
        }
    timeout_pnl = None
    timeout_age = max_hold_ms
    for ts, pnl in path_bps:
        if ts <= max_hold_ms:
            timeout_pnl = pnl
            timeout_age = ts
        else:
            break
    if timeout_pnl is None:
        timeout_pnl = path_bps[-1][1] if path_bps else None
        timeout_age = path_bps[-1][0] if path_bps else None
    return {
        "result": "TIMEOUT",
        "pnl_bps": timeout_pnl,
        "exit_age_ms": timeout_age,
        "source": "exact_levels" if used_exact else "path_approx",
        "tie": False,
    }


def run_simulator_fixtures(state: AuditState) -> None:
    fixtures = [
        {
            "fixture": "target_before_stop",
            "path": [(0, 0), (1000, 1200), (2000, -600)],
            "first_hits": {1000: 1000, -500: 2000},
            "expected_target_first": "TARGET",
        },
        {
            "fixture": "stop_before_target",
            "path": [(0, 0), (1000, -600), (2000, 1200)],
            "first_hits": {1000: 2000, -500: 1000},
            "expected_target_first": "STOP",
        },
        {
            "fixture": "target_and_stop_same_timestamp",
            "path": [(0, 0), (1000, 1200), (1000, -600)],
            "first_hits": {1000: 1000, -500: 1000},
            "expected_target_first": "TARGET",
        },
        {
            "fixture": "sparse_path_timeout",
            "path": [(0, 0), (9000, 100), (25000, 200)],
            "first_hits": {},
            "expected_target_first": "TIMEOUT",
        },
        {
            "fixture": "no_path_point_before_max_hold",
            "path": [(60000, 100)],
            "first_hits": {},
            "expected_target_first": "TIMEOUT",
        },
        {
            "fixture": "missing_exact_levels",
            "path": [(0, 0), (1000, 1100)],
            "first_hits": {},
            "expected_target_first": "TARGET",
        },
        {
            "fixture": "exact_levels_vs_path_approximation",
            "path": [(0, 0), (1000, 900), (2000, 1100)],
            "first_hits": {1000: 2500},
            "expected_target_first": "TARGET",
        },
        {
            "fixture": "max_hold_shorter_than_first_hit",
            "path": [(0, 0), (20000, 1200)],
            "first_hits": {1000: 20000},
            "max_hold_ms": 10000,
            "expected_target_first": "TIMEOUT",
        },
        {
            "fixture": "max_hold_longer_than_replay_horizon",
            "path": [(0, 0), (30000, 100)],
            "first_hits": {},
            "max_hold_ms": 120000,
            "expected_target_first": "TIMEOUT",
        },
        {
            "fixture": "path_not_monotonic",
            "path": [(0, 0), (2000, 100), (1000, 1200)],
            "first_hits": {},
            "expected_target_first": "TARGET",
        },
        {
            "fixture": "malformed_first_hit_ms",
            "path": [(0, 0), (1000, -600)],
            "first_hits": {"bad": "bad"},
            "expected_target_first": "STOP",
        },
        {
            "fixture": "negative_timeout_classification",
            "path": [(0, 0), (9000, -100)],
            "first_hits": {},
            "expected_target_first": "TIMEOUT",
        },
        {
            "fixture": "positive_timeout_classification",
            "path": [(0, 0), (9000, 100)],
            "first_hits": {},
            "expected_target_first": "TIMEOUT",
        },
    ]
    for fixture in fixtures:
        first_hits = normalize_first_hits(fixture["first_hits"])
        path_bps = normalize_path(fixture["path"])
        max_hold = int(fixture.get("max_hold_ms", 10000))
        for variant in TIE_VARIANTS:
            result = simulate_exit(path_bps, first_hits, 1000, -500, max_hold, variant)
            pnl = result.get("pnl_bps")
            timeout_class = ""
            if result["result"] == "TIMEOUT" and pnl is not None:
                timeout_class = "positive_timeout" if pnl >= 0 else "negative_timeout"
            pass_fixture = (
                variant != "exact_levels_only"
                or fixture["fixture"] not in {"missing_exact_levels", "sparse_path_timeout", "no_path_point_before_max_hold"}
                or result["result"] == "NOT_EVALUABLE"
            )
            if variant == "target_first_on_equal_timestamp":
                pass_fixture = result["result"] == fixture["expected_target_first"]
            state.fixture_rows.append(
                {
                    "fixture": fixture["fixture"],
                    "variant": variant,
                    "target_bps": 1000,
                    "stop_bps": -500,
                    "max_hold_ms": max_hold,
                    "result": result["result"],
                    "pnl_bps": pnl if pnl is not None else "",
                    "exit_age_ms": result.get("exit_age_ms") or "",
                    "source": result.get("source"),
                    "tie": result.get("tie"),
                    "timeout_class": timeout_class,
                    "pass": pass_fixture,
                    "notes": "synthetic_contract_fixture",
                }
            )
    failed = [row for row in state.fixture_rows if str(row["pass"]) != "True"]
    if failed:
        state.failures.append(f"simulation_fixture_failures={len(failed)}")


def summarize_replay_metrics(rows: list[ReplayRow], params: ExitParams, variant: str) -> dict[str, Any]:
    results: list[dict[str, Any]] = []
    for row in rows:
        result = simulate_exit(row.path_bps, row.first_hit_ms, params.target_bps, params.stop_bps, params.max_hold_ms, variant)
        if result["result"] in {"NOT_EVALUABLE", "AMBIGUOUS_TIE_EXCLUDED"}:
            continue
        results.append(result)
    pnl_values = [int(r["pnl_bps"]) for r in results if r.get("pnl_bps") is not None]
    target_count = sum(1 for r in results if r["result"] == "TARGET")
    stop_count = sum(1 for r in results if r["result"] == "STOP")
    timeout_count = sum(1 for r in results if r["result"] == "TIMEOUT")
    negative_timeout_count = sum(1 for r in results if r["result"] == "TIMEOUT" and (r.get("pnl_bps") or 0) < 0)
    return {
        "evaluated_rows": len(results),
        "target_count": target_count,
        "stop_count": stop_count,
        "timeout_count": timeout_count,
        "target_rate": safe_div(target_count, len(results)),
        "stop_rate": safe_div(stop_count, len(results)),
        "timeout_rate": safe_div(timeout_count, len(results)),
        "negative_timeout_rate": safe_div(negative_timeout_count, timeout_count),
        "sum_pnl_bps": sum(pnl_values),
        "avg_pnl_bps": safe_div(sum(pnl_values), len(pnl_values)),
        "median_pnl_bps": median(pnl_values),
        "cost100_sum_pnl_bps": sum(p - 100 for p in pnl_values),
        "cost200_sum_pnl_bps": sum(p - 200 for p in pnl_values),
        "max_consecutive_losses_cost100": max_consecutive((p - 100) < 0 for p in pnl_values),
        "top5_removed_sum_cost100": tail_removed_sum(pnl_values, 0.05, 100),
        "top10_removed_sum_cost100": tail_removed_sum(pnl_values, 0.10, 100),
    }


def tail_removed_sum(values: list[int], fraction: float, cost_bps: int) -> int:
    adjusted = [v - cost_bps for v in values]
    positives = sorted([v for v in adjusted if v > 0], reverse=True)
    remove_count = int(math.ceil(len(positives) * fraction)) if positives else 0
    remove_sum = sum(positives[:remove_count])
    return sum(adjusted) - remove_sum


def run_tie_sensitivity(state: AuditState, scope: str, rows: list[ReplayRow], params: ExitParams) -> dict[str, dict[str, Any]]:
    metrics_by_variant: dict[str, dict[str, Any]] = {}
    signs: dict[str, int] = {}
    for variant in TIE_VARIANTS:
        metrics = summarize_replay_metrics(rows, params, variant)
        metrics_by_variant[variant] = metrics
        sum_pnl = float(metrics["cost100_sum_pnl_bps"])
        signs[variant] = 1 if sum_pnl > 0 else -1 if sum_pnl < 0 else 0
        state.metric_rows.append(
            {
                "check_type": "tie_break_sensitivity",
                "scope_or_file": scope,
                "row_id": variant,
                "metric": "cost100_sum_pnl_bps",
                "reported_value": "",
                "recomputed_value": sum_pnl,
                "absolute_diff": "",
                "status": "INFO",
                "notes": json.dumps(metrics, sort_keys=True),
            }
        )
    non_zero_signs = {sign for sign in signs.values() if sign != 0}
    if len(non_zero_signs) > 1:
        state.warnings.append(f"tie_break_sign_flip_scope={scope}")
    return metrics_by_variant


def reconcile_scope(state: AuditState, evidence: ScopeEvidence, rows: list[ReplayRow], malformed_replay: int) -> None:
    lifecycle_paths = [p for p in [evidence.shadow_lifecycle, evidence.probe_lifecycle] if p]
    terminals: list[LifecycleTerminal] = []
    malformed_lifecycle = 0
    for path in lifecycle_paths:
        loaded, malformed = load_lifecycle_terminals(evidence.scope, path)
        terminals.extend(loaded)
        malformed_lifecycle += malformed
    exact_index: dict[tuple[str, str, str, str, int | None], list[LifecycleTerminal]] = defaultdict(list)
    fallback_index: dict[tuple[str, str, str], list[LifecycleTerminal]] = defaultdict(list)
    for terminal in terminals:
        exact_index[terminal.key].append(terminal)
        fallback_index[terminal.candidate_key].append(terminal)
    params = infer_exit_params(evidence.scope)
    exact_join = 0
    fallback_join = 0
    missing_join = 0
    duplicate_exact = 0
    close_reason_mismatch = 0
    pnl_large_diff = 0
    duration_large_diff = 0
    joined = 0
    for row in rows:
        candidates = exact_index.get(row.key, [])
        join_type = "exact"
        if len(candidates) == 1:
            terminal = candidates[0]
            exact_join += 1
        elif len(candidates) > 1:
            terminal = candidates[-1]
            duplicate_exact += 1
            exact_join += 1
        else:
            fallback_candidates = fallback_index.get(row.candidate_key, [])
            if len(fallback_candidates) == 1:
                terminal = fallback_candidates[0]
                fallback_join += 1
                join_type = "fallback"
            elif len(fallback_candidates) > 1:
                terminal = fallback_candidates[-1]
                fallback_join += 1
                join_type = "fallback_ambiguous"
            else:
                missing_join += 1
                continue
        joined += 1
        replay = simulate_exit(row.path_bps, row.first_hit_ms, params.target_bps, params.stop_bps, params.max_hold_ms)
        replay_result = replay["result"]
        close_reason = normalize_close_reason(terminal.close_reason)
        if replay_result in {"TARGET", "STOP", "TIMEOUT"} and close_reason and close_reason != replay_result:
            close_reason_mismatch += 1
        replay_pnl = replay.get("pnl_bps")
        if replay_pnl is not None and terminal.final_pnl_bps is not None:
            if abs(int(replay_pnl) - int(terminal.final_pnl_bps)) > 500:
                pnl_large_diff += 1
        replay_age = replay.get("exit_age_ms")
        if replay_age is not None and terminal.duration_ms is not None:
            if abs(int(replay_age) - int(terminal.duration_ms)) > 5000:
                duration_large_diff += 1
    status = "PASS"
    notes = []
    if fallback_join or missing_join or duplicate_exact:
        status = "WARN"
        notes.append("join_quality_not_exact")
    if close_reason_mismatch and safe_div(close_reason_mismatch, max(joined, 1)) > 0.20:
        status = "WARN"
        notes.append("close_reason_replay_mismatch_gt_20pct")
    if evidence.active_partial:
        status = "ACTIVE_PARTIAL"
        notes.append("scope_is_currently_running")
    state.recon_rows.append(
        {
            "scope": evidence.scope,
            "exit_replay_path": evidence.exit_replay or "",
            "shadow_lifecycle_path": evidence.shadow_lifecycle or "",
            "probe_lifecycle_path": evidence.probe_lifecycle or "",
            "replay_rows": len(rows),
            "malformed_replay_rows": malformed_replay,
            "lifecycle_terminal_rows": len(terminals),
            "malformed_lifecycle_rows": malformed_lifecycle,
            "exact_join_count": exact_join,
            "fallback_join_count": fallback_join,
            "missing_join_count": missing_join,
            "duplicate_exact_join_keys": duplicate_exact,
            "exact_join_rate": safe_div(exact_join, len(rows)),
            "fallback_join_rate": safe_div(fallback_join, len(rows)),
            "close_reason_mismatch_count": close_reason_mismatch,
            "close_reason_mismatch_rate": safe_div(close_reason_mismatch, max(joined, 1)),
            "pnl_large_diff_count": pnl_large_diff,
            "duration_large_diff_count": duration_large_diff,
            "status": status,
            "notes": ";".join(notes),
        }
    )
    if status in {"WARN", "ACTIVE_PARTIAL"}:
        state.warnings.append(f"replay_lifecycle_{status.lower()}={evidence.scope}")


def normalize_close_reason(reason: str) -> str:
    reason = (reason or "").strip().lower()
    if reason in {"target", "takeprofit", "take_profit"}:
        return "TARGET"
    if reason in {"stoploss", "stop_loss", "stop"}:
        return "STOP"
    if reason in {"timestop", "time_stop", "timeout", "maxhold", "max_hold"}:
        return "TIMEOUT"
    return ""


def run_horizon_sensitivity(state: AuditState, scope: str, rows: list[ReplayRow]) -> None:
    for horizon in HORIZONS_MS:
        total = len(rows)
        if total == 0:
            coverage = 0.0
            status = "NOT_EVALUABLE"
            pnl_values: list[int] = []
        else:
            supported: list[ReplayRow] = []
            pnl_values = []
            for row in rows:
                max_path_ms = max((p[0] for p in row.path_bps), default=-1)
                row_horizon = row.horizon_ms if row.horizon_ms is not None else max_path_ms
                if max_path_ms >= horizon and row_horizon >= horizon:
                    supported.append(row)
                    pnl = pnl_at_horizon(row.path_bps, horizon)
                    if pnl is not None:
                        pnl_values.append(pnl)
            coverage = safe_div(len(supported), total)
            status = "EVALUABLE" if coverage >= EVALUABLE_COVERAGE_MIN else "NOT_EVALUABLE"
        state.horizon_rows.append(
            {
                "scope": scope,
                "horizon_ms": horizon,
                "replay_rows": total,
                "supported_rows": len(pnl_values),
                "coverage_pct": coverage,
                "status": status,
                "avg_pnl_bps": safe_div(sum(pnl_values), len(pnl_values)),
                "median_pnl_bps": median(pnl_values),
                "sum_pnl_bps": sum(pnl_values),
                "notes": "coverage_below_80pct_marked_not_evaluable" if status == "NOT_EVALUABLE" else "",
            }
        )
    long_not_eval = [
        row
        for row in state.horizon_rows
        if row["scope"] == scope and int(row["horizon_ms"]) >= 300000 and row["status"] == "NOT_EVALUABLE"
    ]
    if long_not_eval:
        state.warnings.append(f"long_horizon_not_evaluable={scope}")


def pnl_at_horizon(path_bps: list[tuple[int, int]], horizon_ms: int) -> int | None:
    best: int | None = None
    for ts, pnl in path_bps:
        if ts <= horizon_ms:
            best = pnl
        else:
            break
    return best


def check_metric_consistency(state: AuditState) -> None:
    csv_paths = [
        *REPORT_DIR.glob("**/time_stop_v2_mask_summary_a2.csv"),
        *REPORT_DIR.glob("**/time_stop_v2_noharm_summary_v1.csv"),
        REPORT_DIR / "rtp_a0_guard_summary.csv",
        REPORT_DIR / "rug_markup_a0_summary.csv",
        REPORT_DIR / "rce_a0_summary.csv",
        REPORT_DIR / "tsv2_a3_two_scope_fixed_cell_intersection.csv",
        *REPORT_DIR.glob("**/organic_candidate_policy_summary.csv"),
    ]
    seen: set[Path] = set()
    for path in csv_paths:
        if path in seen or not path.exists():
            continue
        seen.add(path)
        with path.open(newline="", encoding="utf-8") as handle:
            reader = csv.DictReader(handle)
            for idx, row in enumerate(reader, 1):
                if idx > 5000:
                    break
                run_row_metric_checks(state, path, idx, row)


def run_row_metric_checks(state: AuditState, path: Path, idx: int, row: Mapping[str, str]) -> None:
    row_id = row.get("scope") or row.get("mask_name") or row.get("policy") or str(idx)
    checks = 0
    failures = 0

    def record(metric: str, reported: Any, recomputed: Any, status: str, notes: str = "") -> None:
        nonlocal checks, failures
        checks += 1
        if status == "FAIL":
            failures += 1
        diff = ""
        rf = as_float(reported)
        cf = as_float(recomputed)
        if rf is not None and cf is not None:
            diff = abs(rf - cf)
        state.metric_rows.append(
            {
                "check_type": "csv_internal_consistency",
                "scope_or_file": str(path),
                "row_id": row_id,
                "metric": metric,
                "reported_value": reported,
                "recomputed_value": recomputed,
                "absolute_diff": diff,
                "status": status,
                "notes": notes,
            }
        )

    # avg = sum / count checks.
    for prefix in ("cost100_", "cost200_", "gross_", "guarded_cost100_", "anchor_cost100_"):
        count = as_float(row.get(prefix + "eligible_count") or row.get(prefix + "supported_rows") or row.get("retained_count"))
        sum_value = as_float(row.get(prefix + "sum_pnl_bps") or row.get(prefix + "baseline_sum_after_cost_bps"))
        avg_value = as_float(row.get(prefix + "avg_pnl_bps") or row.get(prefix + "baseline_avg_after_cost_bps"))
        if count and sum_value is not None and avg_value is not None:
            recomputed = sum_value / count
            status = "PASS" if abs(recomputed - avg_value) <= 1e-3 else "FAIL"
            record(prefix + "avg_equals_sum_over_count", avg_value, recomputed, status)

    # target/stop/timeout rates.
    for prefix in ("gross_", "cost100_"):
        target_rate = as_float(row.get(prefix + "target_rate"))
        stop_rate = as_float(row.get(prefix + "stop_rate"))
        timeout_rate = as_float(row.get(prefix + "timeout_rate"))
        if target_rate is not None and stop_rate is not None and timeout_rate is not None:
            total_rate = target_rate + stop_rate + timeout_rate
            status = "PASS" if abs(total_rate - 1.0) <= 1e-6 else "FAIL"
            record(prefix + "rates_sum_to_one", 1.0, total_rate, status)

    # TSV2 precision denominator.
    beneficial = as_int(row.get("cost100_beneficial_exit_count"))
    harmful = as_int(row.get("cost100_harmful_exit_count"))
    precision = as_float(row.get("cost100_exit_action_precision"))
    if beneficial is not None and harmful is not None and precision is not None:
        recomputed = safe_div(beneficial, beneficial + harmful)
        status = "PASS" if abs(recomputed - precision) <= 1e-9 else "FAIL"
        record("cost100_exit_action_precision", precision, recomputed, status)
        wilson_reported = as_float(row.get("cost100_exit_action_precision_wilson95_lower"))
        if wilson_reported is not None:
            wilson = wilson_lower_bound(beneficial, beneficial + harmful)
            status = "PASS" if abs(wilson - wilson_reported) <= 1e-9 else "FAIL"
            record("cost100_exit_action_precision_wilson95_lower", wilson_reported, wilson, status)

    # RUG nonloss precision Wilson.
    precision_cost100 = as_float(row.get("precision_cost100"))
    wilson_cost100 = as_float(row.get("wilson_lower95_cost100"))
    retained = as_int(row.get("retained_count"))
    if precision_cost100 is not None and wilson_cost100 is not None and retained:
        successes = int(round(precision_cost100 * retained))
        wilson = wilson_lower_bound(successes, retained)
        status = "PASS" if abs(wilson - wilson_cost100) <= 0.005 else "FAIL"
        record("wilson_lower95_cost100_approx", wilson_cost100, wilson, status, "successes_rounded_from_precision")

    if failures:
        state.failures.append(f"metric_consistency_failures={path}:{idx}:{failures}")


def run_config_sensitivity(state: AuditState) -> None:
    add_config_rows_from_tsv2_a3(state)
    add_config_rows_from_rug(state)
    add_config_rows_from_rtp(state)
    add_config_rows_from_org(state)
    add_config_rows_from_rce(state)


def add_config_rows_from_tsv2_a3(state: AuditState) -> None:
    path = REPORT_DIR / "tsv2_a3_two_scope_fixed_cell_intersection.csv"
    if not path.exists():
        state.config_rows.append(missing_config_row("TSV2_A3", path))
        return
    passing = 0
    rows = 0
    sign_flip_like = 0
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows += 1
            if str(row.get("passing_both")).lower() == "true":
                passing += 1
            r49 = as_float(row.get("r49_delta_sum_bps")) or 0.0
            r50 = as_float(row.get("r50_delta_sum_bps")) or 0.0
            if r49 * r50 < 0:
                sign_flip_like += 1
    state.config_rows.append(
        {
            "family": "TSV2_A3",
            "source_file": str(path),
            "rows": rows,
            "pass_count": passing,
            "positive_count": "",
            "negative_count": "",
            "sign_flip_count": sign_flip_like,
            "robust_negative_conclusion": passing == 0,
            "status": "PASS" if passing == 0 else "WARN",
            "notes": "fixed_cell_passing_both_count_zero" if passing == 0 else "fixed_cell_exists_review_needed",
        }
    )


def add_config_rows_from_rug(state: AuditState) -> None:
    path = REPORT_DIR / "rug_markup_a0_summary.csv"
    if not path.exists():
        state.config_rows.append(missing_config_row("RUG_MARKUP_A0", path))
        return
    rows = positives = passes = 0
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows += 1
            cost100 = as_float(row.get("cost100_sum_pnl_bps")) or 0.0
            cost200 = as_float(row.get("cost200_sum_pnl_bps")) or 0.0
            median100 = as_float(row.get("cost100_median_pnl_bps")) or -1.0
            if cost100 > 0 and cost200 > 0:
                positives += 1
            if str(row.get("passes_promising_gate")).lower() == "true" or (
                cost100 > 0 and cost200 > 0 and median100 >= 0
            ):
                passes += 1
    state.config_rows.append(
        {
            "family": "RUG_MARKUP_A0",
            "source_file": str(path),
            "rows": rows,
            "pass_count": passes,
            "positive_count": positives,
            "negative_count": rows - positives,
            "sign_flip_count": "",
            "robust_negative_conclusion": passes == 0,
            "status": "PASS" if passes == 0 else "WARN",
            "notes": "no_promising_row_under_fixed_grid" if passes == 0 else "positive_rows_exist_review_tail_dependency",
        }
    )


def add_config_rows_from_rtp(state: AuditState) -> None:
    path = REPORT_DIR / "rtp_a0_guard_summary.csv"
    if not path.exists():
        state.config_rows.append(missing_config_row("RTP_A0", path))
        return
    rows = scope_pass = aggregate_pass = 0
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows += 1
            if str(row.get("scope_pass")).lower() == "true":
                scope_pass += 1
            if str(row.get("aggregate_pass")).lower() == "true":
                aggregate_pass += 1
    state.config_rows.append(
        {
            "family": "RTP_A0",
            "source_file": str(path),
            "rows": rows,
            "pass_count": scope_pass,
            "positive_count": aggregate_pass,
            "negative_count": rows - aggregate_pass,
            "sign_flip_count": "",
            "robust_negative_conclusion": scope_pass == 0,
            "status": "PASS" if scope_pass == 0 else "WARN",
            "notes": "diagnostic_signal_possible_but_no_scope_pass" if aggregate_pass else "no_guard_pass",
        }
    )


def add_config_rows_from_org(state: AuditState) -> None:
    paths = list(REPORT_DIR.glob("**/organic_candidate_policy_summary.csv"))
    if not paths:
        state.config_rows.append(missing_config_row("ORG_A0", Path("reports/selector/**/organic_candidate_policy_summary.csv")))
        return
    total_rows = passes = positives = 0
    for path in paths:
        with path.open(newline="", encoding="utf-8") as handle:
            for row in csv.DictReader(handle):
                total_rows += 1
                cost100 = as_float(row.get("cost100_sum_pnl_bps")) or 0.0
                median100 = as_float(row.get("cost100_median_pnl_bps")) or -1.0
                if cost100 > 0:
                    positives += 1
                if cost100 > 0 and median100 >= 0:
                    passes += 1
    state.config_rows.append(
        {
            "family": "ORG_A0",
            "source_file": ";".join(str(p) for p in paths),
            "rows": total_rows,
            "pass_count": passes,
            "positive_count": positives,
            "negative_count": total_rows - positives,
            "sign_flip_count": "",
            "robust_negative_conclusion": passes == 0,
            "status": "PASS" if passes == 0 else "WARN",
            "notes": "median_gate_blocks_positive_tail_rows" if positives else "no_positive_cost100_rows",
        }
    )


def add_config_rows_from_rce(state: AuditState) -> None:
    path = REPORT_DIR / "rce_a0_summary.csv"
    status = "PASS" if path.exists() else "WARN"
    verdict = ""
    if path.exists():
        with path.open(newline="", encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle))
        verdict = rows[0].get("verdict", "") if rows else ""
    state.config_rows.append(
        {
            "family": "RCE_A0",
            "source_file": str(path),
            "rows": 1 if path.exists() else 0,
            "pass_count": 0,
            "positive_count": 0,
            "negative_count": 0,
            "sign_flip_count": "",
            "robust_negative_conclusion": verdict == "RCE_BLOCKED_BY_DATA",
            "status": status,
            "notes": verdict or "missing_summary",
        }
    )


def missing_config_row(family: str, path: Path) -> dict[str, Any]:
    return {
        "family": family,
        "source_file": str(path),
        "rows": 0,
        "pass_count": 0,
        "positive_count": "",
        "negative_count": "",
        "sign_flip_count": "",
        "robust_negative_conclusion": False,
        "status": "MISSING",
        "notes": "source_report_missing",
    }


def run_missing_metrics_inventory(state: AuditState) -> None:
    missing_metrics = [
        ("slot/block position", "partial", "some lifecycle slot fields exist but not uniformly in replay summaries"),
        ("actual landing latency", "missing_or_partial", "shadow lifecycle has timestamps; real live landing latency is not available"),
        ("priority fee/Jito tip", "missing_or_partial", "not consistently present in replay-level CSVs"),
        ("path sample density", "present", "shadow_exit_replay_v1 has path_points_written/heartbeat_ms/truncated"),
        ("same-slot target/stop ambiguity", "missing_or_partial", "first_hit_ms is ms-level; same-slot ordering not guaranteed"),
        ("entry slippage", "missing_or_partial", "entry_price exists; quote/fill divergence not consistently captured"),
        ("exit slippage", "missing_or_partial", "exit lifecycle price exists; simulated replay slippage not equivalent"),
        ("tx ordering within slot", "missing", "not available in replay CSVs"),
        ("quote/fill divergence", "missing_or_partial", "not consistently captured for shadow replay"),
        ("MFE/MAE before entry", "missing_or_partial", "RCE pre-entry surface only after new logging; old scopes incomplete"),
        ("MFE/MAE after entry", "present", "shadow_exit_replay_v1 has mfe_bps/mae_bps"),
        ("time-to-MFE", "present", "shadow_exit_replay_v1 has time_to_mfe_ms"),
        ("time-to-MAE", "present", "shadow_exit_replay_v1 has time_to_mae_ms"),
        ("follow-through after impulse", "missing_or_partial", "requires RCE surface/fresh decision rows"),
        ("session heat", "missing_or_partial", "RCE session_regime_snapshot_v1 required for new scopes"),
        ("false-negative missed winners", "missing", "rejected opportunities without replay path are not fully represented"),
        ("opportunity cost", "missing", "not measured by current shadow replay reports"),
    ]
    for metric, availability, notes in missing_metrics:
        state.summary_rows.append(
            {
                "dimension": "missing_metrics_inventory",
                "item": metric,
                "status": availability,
                "severity": "WARN" if availability != "present" else "INFO",
                "details": notes,
            }
        )


def decide_verdict(state: AuditState) -> None:
    if any("simulation_fixture_failures" in failure for failure in state.failures):
        state.verdict = "SIMULATION_CONTRACT_UNSTABLE"
        return
    if any("metric_consistency_failures" in failure for failure in state.failures):
        state.verdict = "METRIC_CONSISTENCY_FAILED"
        return
    severe_recon = [
        row
        for row in state.recon_rows
        if row.get("status") == "WARN" and float(row.get("exact_join_rate") or 0.0) < 0.80
    ]
    if severe_recon:
        state.verdict = "REPLAY_LIFECYCLE_MISMATCH"
        return
    unstable_config = [row for row in state.config_rows if row.get("status") == "WARN" and row.get("pass_count") not in ("", 0, "0")]
    if unstable_config:
        state.verdict = "CONFIG_SENSITIVITY_UNSTABLE"
        return
    all_long_not_eval = [
        row
        for row in state.horizon_rows
        if int(row.get("horizon_ms") or 0) >= 300000 and row.get("status") == "NOT_EVALUABLE"
    ]
    if all_long_not_eval:
        # This is a surface limitation, but it does not by itself invalidate
        # conclusions drawn at <=120000ms horizons.
        state.warnings.append("long_horizon_surface_insufficient_300000_500000")
    state.verdict = "METROLOGY_PASS_WITH_WARNINGS" if state.warnings else "METROLOGY_PASS"


def add_summary_rows(state: AuditState) -> None:
    state.summary_rows.extend(
        [
            {
                "dimension": "final_verdict",
                "item": "research_metrology_audit",
                "status": state.verdict,
                "severity": "FAIL" if state.verdict not in {"METROLOGY_PASS", "METROLOGY_PASS_WITH_WARNINGS"} else "WARN",
                "details": ";".join(state.failures or state.warnings[:8]),
            },
            {
                "dimension": "runtime_boundary",
                "item": "runtime_change",
                "status": "false",
                "severity": "INFO",
                "details": "audit is offline-only; no runtime, BUY/REJECT, Gatekeeper, selector, TX/Jito/live path changes",
            },
            {
                "dimension": "raw_log_commit_boundary",
                "item": "raw_jsonl_logs",
                "status": "not_committed",
                "severity": "INFO",
                "details": "script reads local JSONL evidence but outputs only compact CSV/MD reports",
            },
        ]
    )
    for warning in state.warnings:
        state.summary_rows.append(
            {
                "dimension": "warning",
                "item": warning.split("=", 1)[0],
                "status": "WARN",
                "severity": "WARN",
                "details": warning,
            }
        )
    for failure in state.failures:
        state.summary_rows.append(
            {
                "dimension": "failure",
                "item": failure.split("=", 1)[0],
                "status": "FAIL",
                "severity": "FAIL",
                "details": failure,
            }
        )


def write_markdown(state: AuditState, audited_scopes: list[str]) -> None:
    REPORT_MD.parent.mkdir(parents=True, exist_ok=True)
    ADR_MD.parent.mkdir(parents=True, exist_ok=True)
    severe = state.verdict not in {"METROLOGY_PASS", "METROLOGY_PASS_WITH_WARNINGS"}
    decision = (
        "Poprzednie negatywne wyniki nalezy zdegradowac do INCONCLUSIVE_MEASUREMENT_FAILURE."
        if severe
        else "Poprzednie negatywne wyniki pozostaja wazne w audytowanych horyzontach i przy jawnych ograniczeniach pomiaru."
    )
    horizon_note = (
        "Nie wolno inferowac wnioskow dla 300000/500000 ms, jezeli coverage jest NOT_EVALUABLE."
    )
    report = f"""# P0 Research Metrology Audit - 2026-06-29

## Status

Final verdict: **{state.verdict}**

Decision: {decision}

Runtime approval: **false**
Shadow close approval: **false**
Active close approval: **false**

## Zakres

Audyt jest offline-only. Nie uruchamia nowych runow, nie zmienia runtime, nie
zmienia BUY/REJECT, Gatekeepera, selectora ani TX/Jito/live path. Skrypt czyta
lokalne JSONL evidence i istniejace CSV/MD raporty, ale outputs to tylko
kompaktowe raporty CSV/MD.

Audytowane scopes raw replay/lifecycle:

{chr(10).join(f'- `{scope}`' for scope in audited_scopes)}

## Wynik po wymiarach

- Simulator fixtures: sprawdzone w `reports/selector/research_metrology_audit_simulation_fixtures.csv`.
- Tie-break sensitivity: zapis w `research_metrology_audit_metric_consistency.csv`.
- Replay/lifecycle reconciliation: `research_metrology_audit_replay_lifecycle_reconciliation.csv`.
- Metric consistency: `research_metrology_audit_metric_consistency.csv`.
- Config sensitivity: `research_metrology_audit_config_sensitivity.csv`.
- Horizon sensitivity: `research_metrology_audit_horizon_sensitivity.csv`.
- Missing metrics inventory: `research_metrology_audit_summary.csv`.

## Najwazniejsze ostrzezenia

{chr(10).join(f'- {warning}' for warning in state.warnings[:30]) or '- Brak ostrzezen.'}

## Failure flags

{chr(10).join(f'- {failure}' for failure in state.failures) or '- Brak twardych failure flags.'}

## Interpretacja

{decision}

{horizon_note}

R51, jezeli jest aktywny, jest traktowany jako `ACTIVE_PARTIAL`; jego brak
post-run manifestu nie jest uzywany jako negatywny wynik strategii.

## Pliki wynikowe

- `reports/selector/research_metrology_audit_summary.csv`
- `reports/selector/research_metrology_audit_simulation_fixtures.csv`
- `reports/selector/research_metrology_audit_config_sensitivity.csv`
- `reports/selector/research_metrology_audit_horizon_sensitivity.csv`
- `reports/selector/research_metrology_audit_metric_consistency.csv`
- `reports/selector/research_metrology_audit_replay_lifecycle_reconciliation.csv`
"""
    REPORT_MD.write_text(report, encoding="utf-8")

    adr = f"""# ADR-8D: Research Metrology Audit 2026-06-29

## Status

IMPLEMENTED / OFFLINE_AUDIT / {state.verdict}

## Kontekst

ORG-A0, TSV2 A1/A2/A3, EIX, RTP-A0, RUG-MARKUP-A0 i RCE-A0 sa liniami
badawczymi opartymi o replay, lifecycle, CSV metryki i konfiguracje scope.
Przed dalsza interpretacja wynikow potrzebny jest P0 audit metrologiczny:
czy symulator, lifecycle join, metryki, horyzonty i konfiguracje nie robia z
wynikow artefaktu pomiarowego.

## Decyzja

Dodano offline-only audit:

- `scripts/research_metrology_audit.py`
- `reports/selector/research_metrology_audit_*.csv`
- `PLANS/AUDYT/RAPORT_RESEARCH_METROLOGY_AUDIT_20260629.md`

Final verdict: **{state.verdict}**

Runtime approval: **false**
Shadow close approval: **false**
Active close approval: **false**

## Konsekwencje

{decision}

{horizon_note}

Raw JSONL logs pozostaja lokalnym evidence i nie sa committowane.

## Guardrails

- no runtime change
- no BUY/REJECT change
- no Gatekeeper policy change
- no selector runtime change
- no TX/Jito/live path change
- no cleanup
- no raw JSONL commit
"""
    ADR_MD.write_text(adr, encoding="utf-8")


def main() -> int:
    args = parse_args()
    state = AuditState()
    run_simulator_fixtures(state)

    evidence_map = discover_scopes(args.local_logs_root, args.volume_logs_root)
    requested_scopes = args.scope or [
        R48_R2_SCOPE,
        R49_SCOPE,
        R50_SCOPE,
        R51_SCOPE,
    ]
    audited_scopes: list[str] = []
    for scope in requested_scopes:
        evidence = evidence_map.get(scope, ScopeEvidence(scope=scope))
        if not evidence.exit_replay:
            state.recon_rows.append(
                {
                    "scope": scope,
                    "exit_replay_path": "",
                    "shadow_lifecycle_path": evidence.shadow_lifecycle or "",
                    "probe_lifecycle_path": evidence.probe_lifecycle or "",
                    "replay_rows": 0,
                    "malformed_replay_rows": 0,
                    "lifecycle_terminal_rows": 0,
                    "malformed_lifecycle_rows": 0,
                    "exact_join_count": 0,
                    "fallback_join_count": 0,
                    "missing_join_count": 0,
                    "duplicate_exact_join_keys": 0,
                    "exact_join_rate": 0,
                    "fallback_join_rate": 0,
                    "close_reason_mismatch_count": 0,
                    "close_reason_mismatch_rate": 0,
                    "pnl_large_diff_count": 0,
                    "duration_large_diff_count": 0,
                    "status": "MISSING_REPLAY",
                    "notes": "shadow_exit_replay_v1_unavailable",
                }
            )
            state.warnings.append(f"missing_exit_replay={scope}")
            continue
        audited_scopes.append(scope)
        rows, malformed = load_replay_rows(scope, evidence.exit_replay)
        params = infer_exit_params(scope)
        run_tie_sensitivity(state, scope, rows, params)
        reconcile_scope(state, evidence, rows, malformed)
        run_horizon_sensitivity(state, scope, rows)

    check_metric_consistency(state)
    run_config_sensitivity(state)
    run_missing_metrics_inventory(state)
    decide_verdict(state)
    add_summary_rows(state)

    write_csv(
        SUMMARY_CSV,
        state.summary_rows,
        ["dimension", "item", "status", "severity", "details"],
    )
    write_csv(
        FIXTURES_CSV,
        state.fixture_rows,
        [
            "fixture",
            "variant",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "result",
            "pnl_bps",
            "exit_age_ms",
            "source",
            "tie",
            "timeout_class",
            "pass",
            "notes",
        ],
    )
    write_csv(
        CONFIG_CSV,
        state.config_rows,
        [
            "family",
            "source_file",
            "rows",
            "pass_count",
            "positive_count",
            "negative_count",
            "sign_flip_count",
            "robust_negative_conclusion",
            "status",
            "notes",
        ],
    )
    write_csv(
        HORIZON_CSV,
        state.horizon_rows,
        [
            "scope",
            "horizon_ms",
            "replay_rows",
            "supported_rows",
            "coverage_pct",
            "status",
            "avg_pnl_bps",
            "median_pnl_bps",
            "sum_pnl_bps",
            "notes",
        ],
    )
    write_csv(
        METRIC_CSV,
        state.metric_rows,
        [
            "check_type",
            "scope_or_file",
            "row_id",
            "metric",
            "reported_value",
            "recomputed_value",
            "absolute_diff",
            "status",
            "notes",
        ],
    )
    write_csv(
        RECON_CSV,
        state.recon_rows,
        [
            "scope",
            "exit_replay_path",
            "shadow_lifecycle_path",
            "probe_lifecycle_path",
            "replay_rows",
            "malformed_replay_rows",
            "lifecycle_terminal_rows",
            "malformed_lifecycle_rows",
            "exact_join_count",
            "fallback_join_count",
            "missing_join_count",
            "duplicate_exact_join_keys",
            "exact_join_rate",
            "fallback_join_rate",
            "close_reason_mismatch_count",
            "close_reason_mismatch_rate",
            "pnl_large_diff_count",
            "duration_large_diff_count",
            "status",
            "notes",
        ],
    )
    write_markdown(state, audited_scopes)
    print(f"Final verdict: {state.verdict}")
    print(f"Warnings: {len(state.warnings)}")
    print(f"Failures: {len(state.failures)}")
    for warning in state.warnings[:10]:
        print(f"WARN: {warning}")
    for failure in state.failures[:10]:
        print(f"FAIL: {failure}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
