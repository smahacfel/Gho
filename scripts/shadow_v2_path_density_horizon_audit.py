#!/usr/bin/env python3
from __future__ import annotations

import csv
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

from shadow_v2_offline_audit_common import density_rows, emit, parser, position_id


DECLARED_L2_BASELINE_HORIZONS_MS = [2_000, 3_000, 10_000, 30_000, 120_000]
UNDECLARED_LONG_HORIZONS_MS = [300_000, 500_000]
DEFAULT_RETENTION_MARGIN_MS = 1_000
L2_D2_PASS_VERDICT = "L2_D2_DENSITY_RETENTION_READY_FOR_L2_F"
L2_D3_CONTRACT_FIXTURE_PASS_VERDICT = "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED"
PR55_CONTRACT_FIXTURE_VERDICT = "PR55_AS_L2_D3_CONTRACT_FIXTURE_ACCEPTED"
PR55_RUNTIME_DENSITY_NOT_ACCEPTED_VERDICT = "PR55_AS_RUNTIME_DENSITY_EMISSION_PROOF_NOT_ACCEPTED"
L2_D3B_NEXT_STAGE = "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF"


VERDICTS = [
    "EVALUABLE_EXACT",
    "EVALUABLE_APPROX",
    "SPARSE_APPROX_ONLY",
    "NOT_EVALUABLE_NO_COVERAGE",
    "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
]

EVALUABLE_VERDICTS = {"EVALUABLE_EXACT", "EVALUABLE_APPROX"}
NON_EVALUABLE_VERDICTS = {
    "SPARSE_APPROX_ONLY",
    "NOT_EVALUABLE_NO_COVERAGE",
    "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
}


def int_csv(value: str) -> list[int]:
    parsed: list[int] = []
    for raw in value.split(","):
        item = raw.strip()
        if not item:
            continue
        parsed.append(int(item))
    return parsed


def finite_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    return None


def percentile(values: Iterable[Any], pct: int) -> int | float | None:
    numeric = sorted(v for v in values if isinstance(v, (int, float)) and not isinstance(v, bool))
    if not numeric:
        return None
    if len(numeric) == 1:
        return numeric[0]
    rank = (len(numeric) - 1) * (pct / 100)
    lower = int(rank)
    upper = min(lower + 1, len(numeric) - 1)
    weight = rank - lower
    return numeric[lower] * (1 - weight) + numeric[upper] * weight


def row_limitations(row: dict[str, Any]) -> list[str]:
    values = row.get("limitations")
    if isinstance(values, list):
        return [str(value) for value in values]
    return []


def row_identity(row: dict[str, Any], idx: int) -> str:
    return position_id(row) or str(row.get("position_id") or f"row:{idx}")


def has_limitation(row: dict[str, Any], needle: str) -> bool:
    return any(needle in value for value in row_limitations(row))


def csv_value(value: Any) -> str:
    if isinstance(value, (list, dict)):
        return json.dumps(value, sort_keys=True)
    if value is None:
        return ""
    return str(value)


def horizon_metrics(
    horizon: int,
    group: list[dict[str, Any]],
    *,
    declared_horizons: set[int],
    undeclared_horizons: set[int],
    required_replay_horizon_ms: int,
) -> dict[str, Any]:
    counts = Counter(str(row.get("verdict") or "UNKNOWN") for row in group)
    eligible_positions = {
        row_identity(row, idx)
        for idx, row in enumerate(group)
    }
    evaluable_positions = {
        row_identity(row, idx)
        for idx, row in enumerate(group)
        if str(row.get("verdict") or "UNKNOWN") in EVALUABLE_VERDICTS
    }
    retention_gap_count = 0
    for row in group:
        replay_horizon_ms = finite_int(row.get("replay_horizon_ms"))
        if replay_horizon_ms is None or replay_horizon_ms < required_replay_horizon_ms:
            retention_gap_count += 1
    censored_count = sum(
        1
        for row in group
        if row.get("truncated") is True
        or has_limitation(row, "TRUNCATED")
        or has_limitation(row, "CENSORED")
    )
    horizon_unmatured_count = sum(1 for row in group if has_limitation(row, "HORIZON_UNMATURED"))
    duplicate_sample_count = sum(finite_int(row.get("duplicate_age_count")) or 0 for row in group)
    non_monotonic_sample_count = sum(1 for row in group if row.get("non_monotonic_input") is True)

    declared = horizon in declared_horizons
    undeclared = horizon in undeclared_horizons or not declared
    path_coverage_gap_count = (
        counts["NOT_EVALUABLE_NO_COVERAGE"]
        + counts["NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY"]
    )
    sparse_or_unknown_count = counts["SPARSE_APPROX_ONLY"] + counts["UNKNOWN"]
    if declared:
        if path_coverage_gap_count:
            horizon_verdict = "FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT"
            l2_baseline_blocker = True
            positive_claim_allowed = False
        elif sparse_or_unknown_count:
            horizon_verdict = "FAILED_DECLARED_HORIZON_INCOMPLETE"
            l2_baseline_blocker = True
            positive_claim_allowed = False
        elif retention_gap_count:
            horizon_verdict = "FAILED_RETENTION_GAP"
            l2_baseline_blocker = True
            positive_claim_allowed = False
        else:
            horizon_verdict = "PASS"
            l2_baseline_blocker = False
            positive_claim_allowed = True
    else:
        horizon_verdict = "NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE"
        l2_baseline_blocker = False
        positive_claim_allowed = False

    coverage_ratio = (
        len(evaluable_positions) / len(eligible_positions)
        if eligible_positions
        else 0.0
    )
    return {
        "horizon_ms": horizon,
        "horizon_scope": "DECLARED_L2_BASELINE" if declared else "UNDECLARED_FOR_L2_BASELINE",
        "total_density_rows": len(group),
        "eligible_positions": len(eligible_positions),
        "evaluable_positions": len(evaluable_positions),
        "coverage_ratio": coverage_ratio,
        **{f"{value}_count": counts[value] for value in VERDICTS},
        "UNKNOWN_count": counts["UNKNOWN"],
        "samples_per_position_p50": percentile((row.get("path_points") for row in group), 50),
        "samples_per_position_p90": percentile((row.get("path_points") for row in group), 90),
        "coverage_points_p50": percentile((row.get("coverage_points") for row in group), 50),
        "coverage_points_p90": percentile((row.get("coverage_points") for row in group), 90),
        "max_gap_ms_p90": percentile((row.get("max_interval_ms") for row in group), 90),
        "max_gap_ms_max": max(
            (
                value
                for value in (finite_int(row.get("max_interval_ms")) for row in group)
                if value is not None
            ),
            default=None,
        ),
        "replay_horizon_ms_min": min(
            (
                value
                for value in (finite_int(row.get("replay_horizon_ms")) for row in group)
                if value is not None
            ),
            default=None,
        ),
        "replay_horizon_ms_p50": percentile((row.get("replay_horizon_ms") for row in group), 50),
        "duplicate_sample_count": duplicate_sample_count,
        "non_monotonic_sample_count": non_monotonic_sample_count,
        "censored_count": censored_count,
        "horizon_unmatured_count": horizon_unmatured_count,
        "retention_gap_count": retention_gap_count,
        "path_sample_coverage_gap_count": path_coverage_gap_count,
        "sparse_or_unknown_count": sparse_or_unknown_count,
        "positive_research_claim_allowed": positive_claim_allowed,
        "l2_baseline_blocker": l2_baseline_blocker,
        "verdict": horizon_verdict,
        "undeclared_default_status": (
            "NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE" if undeclared else None
        ),
    }


def missing_declared_horizon_metrics(horizon: int) -> dict[str, Any]:
    return {
        "horizon_ms": horizon,
        "horizon_scope": "DECLARED_L2_BASELINE",
        "total_density_rows": 0,
        "eligible_positions": 0,
        "evaluable_positions": 0,
        "coverage_ratio": 0.0,
        **{f"{value}_count": 0 for value in VERDICTS},
        "UNKNOWN_count": 0,
        "samples_per_position_p50": None,
        "samples_per_position_p90": None,
        "coverage_points_p50": None,
        "coverage_points_p90": None,
        "max_gap_ms_p90": None,
        "max_gap_ms_max": None,
        "replay_horizon_ms_min": None,
        "replay_horizon_ms_p50": None,
        "duplicate_sample_count": 0,
        "non_monotonic_sample_count": 0,
        "censored_count": 0,
        "horizon_unmatured_count": 0,
        "retention_gap_count": 0,
        "path_sample_coverage_gap_count": 0,
        "sparse_or_unknown_count": 0,
        "positive_research_claim_allowed": False,
        "l2_baseline_blocker": True,
        "verdict": "FAILED_MISSING_DECLARED_HORIZON",
        "undeclared_default_status": None,
    }


def main() -> int:
    p = parser("Offline Shadow V2 path density horizon retention audit")
    p.add_argument(
        "--declared-horizons-ms",
        default=",".join(str(value) for value in DECLARED_L2_BASELINE_HORIZONS_MS),
        help="Comma-separated horizons that are required for the first L2 baseline.",
    )
    p.add_argument(
        "--undeclared-horizons-ms",
        default=",".join(str(value) for value in UNDECLARED_LONG_HORIZONS_MS),
        help="Comma-separated horizons reported as non-blocking undeclared baseline horizons.",
    )
    p.add_argument(
        "--retention-margin-ms",
        type=int,
        default=DEFAULT_RETENTION_MARGIN_MS,
        help="Required replay/retention margin beyond the max declared horizon.",
    )
    p.add_argument(
        "--output-csv",
        help="Optional metric,value,notes summary CSV path for L2-D2 artifacts.",
    )
    p.add_argument(
        "--pass-verdict",
        default=L2_D2_PASS_VERDICT,
        help="Final verdict to emit when all declared density/retention gates pass.",
    )
    args = p.parse_args()
    declared_horizons = set(int_csv(args.declared_horizons_ms))
    undeclared_horizons = set(int_csv(args.undeclared_horizons_ms)) - declared_horizons
    max_declared_horizon_ms = max(declared_horizons) if declared_horizons else 0
    retention_margin_ms = max(args.retention_margin_ms, 0)
    required_replay_horizon_ms = max_declared_horizon_ms + retention_margin_ms

    rows, malformed = density_rows(args.scope_root)
    by_horizon: dict[int, list[dict]] = defaultdict(list)
    unknown_horizon = 0
    for row in rows:
        horizon = row.get("horizon_ms")
        if isinstance(horizon, int):
            by_horizon[horizon].append(row)
        else:
            unknown_horizon += 1
    per_horizon = []
    missing_declared_horizons = sorted(declared_horizons - set(by_horizon))
    retention_blockers = 0
    density_blockers = 0
    path_coverage_blockers = 0
    for horizon in sorted(by_horizon):
        item = horizon_metrics(
            horizon,
            by_horizon[horizon],
            declared_horizons=declared_horizons,
            undeclared_horizons=undeclared_horizons,
            required_replay_horizon_ms=required_replay_horizon_ms,
        )
        if horizon in declared_horizons:
            if item["path_sample_coverage_gap_count"]:
                path_coverage_blockers += 1
            elif item["retention_gap_count"]:
                retention_blockers += 1
            elif item["l2_baseline_blocker"]:
                density_blockers += 1
        per_horizon.append(item)
    per_horizon.extend(
        missing_declared_horizon_metrics(horizon)
        for horizon in missing_declared_horizons
    )
    per_horizon.sort(key=lambda item: item["horizon_ms"])

    declared_present_count = len(declared_horizons & set(by_horizon))
    undeclared_present_count = len(set(by_horizon) - declared_horizons)
    if malformed or unknown_horizon:
        verdict = "BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE"
    elif missing_declared_horizons or density_blockers:
        verdict = "BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE"
    elif path_coverage_blockers:
        verdict = "BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT"
    elif retention_blockers:
        verdict = "BLOCKED_RETENTION_CONTRACT_INSUFFICIENT"
    else:
        verdict = args.pass_verdict

    density_contract_fixture_pass = verdict == L2_D3_CONTRACT_FIXTURE_PASS_VERDICT
    if density_contract_fixture_pass:
        for item in per_horizon:
            item["positive_research_claim_allowed"] = False
    result = {
        "audit": "path_density_horizon_retention_repair",
        "scope_root": args.scope_root,
        "declared_supported_horizons_ms": sorted(declared_horizons),
        "undeclared_horizons_ms": sorted(undeclared_horizons),
        "unsupported_horizons_ms": sorted(undeclared_horizons),
        "max_declared_horizon_ms": max_declared_horizon_ms,
        "retention_margin_ms": retention_margin_ms,
        "retention_contract_ms": required_replay_horizon_ms,
        "required_replay_coverage_ms": required_replay_horizon_ms,
        "required_replay_horizon_ms": required_replay_horizon_ms,
        "density_rows": len(rows),
        "malformed_density_rows": malformed,
        "unknown_horizon_rows": unknown_horizon,
        "horizon_count": len(by_horizon),
        "declared_horizon_count": len(declared_horizons),
        "declared_horizon_present_count": declared_present_count,
        "declared_horizon_missing_count": len(missing_declared_horizons),
        "missing_declared_horizons_ms": missing_declared_horizons,
        "declared_horizon_density_blocker_count": density_blockers,
        "declared_horizon_path_coverage_blocker_count": path_coverage_blockers,
        "declared_horizon_retention_blocker_count": retention_blockers,
        "undeclared_horizon_present_count": undeclared_present_count,
        "undeclared_horizons_block_l2_baseline": False,
        "undeclared_horizons_positive_research_claim_allowed": False,
        "per_horizon": per_horizon,
        "density_audit_verdict": verdict,
        "verdict": verdict,
        "pass_verdict": args.pass_verdict,
        "density_contract_fixture_pass": density_contract_fixture_pass,
        "pr55_contract_fixture_verdict": (
            PR55_CONTRACT_FIXTURE_VERDICT if density_contract_fixture_pass else None
        ),
        "pr55_runtime_density_verdict": (
            PR55_RUNTIME_DENSITY_NOT_ACCEPTED_VERDICT if density_contract_fixture_pass else None
        ),
        "density_fixture_l2_f_allowed_next": False,
        "runtime_density_emission_proof": False,
        "next_stage": L2_D3B_NEXT_STAGE if density_contract_fixture_pass else None,
        "l2_f_allowed_next": verdict == args.pass_verdict and not density_contract_fixture_pass,
        "runtime_decision_behavior_changes": False,
        "runtime_evidence_schema_changes": False,
        "new_provider_streams": "NONE",
        "runtime_approval": False,
        "research_grade": False,
        "live_equivalence": False,
        "strategy_research_unblocked": False,
        "shadow_close_only": False,
        "active_close": False,
    }
    if args.output_csv:
        write_summary_csv(result, Path(args.output_csv))
    emit(result, args.pretty)
    return 0


def write_summary_csv(result: dict[str, Any], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    rows: list[dict[str, str]] = []

    def add(metric: str, value: Any, notes: str = "") -> None:
        rows.append({"metric": metric, "value": csv_value(value), "notes": notes})

    for metric in [
        "density_audit_verdict",
        "declared_supported_horizons_ms",
        "unsupported_horizons_ms",
        "retention_contract_ms",
        "required_replay_coverage_ms",
        "required_replay_horizon_ms",
        "pass_verdict",
        "retention_margin_ms",
        "density_rows",
        "malformed_density_rows",
        "unknown_horizon_rows",
        "declared_horizon_present_count",
        "declared_horizon_missing_count",
        "declared_horizon_density_blocker_count",
        "declared_horizon_path_coverage_blocker_count",
        "declared_horizon_retention_blocker_count",
        "density_contract_fixture_pass",
        "pr55_contract_fixture_verdict",
        "pr55_runtime_density_verdict",
        "density_fixture_l2_f_allowed_next",
        "runtime_density_emission_proof",
        "next_stage",
        "runtime_approval",
        "research_grade",
        "live_equivalence",
        "strategy_research_unblocked",
        "shadow_close_only",
        "active_close",
    ]:
        add(metric, result.get(metric), "aggregate")

    for horizon in result.get("per_horizon", []):
        horizon_ms = horizon.get("horizon_ms")
        notes = f"horizon_ms={horizon_ms};scope={horizon.get('horizon_scope')}"
        for metric in [
            "horizon_ms",
            "eligible_positions",
            "evaluable_positions",
            "coverage_ratio",
            "samples_per_position_p50",
            "samples_per_position_p90",
            "max_gap_ms_p90",
            "max_gap_ms_max",
            "duplicate_sample_count",
            "non_monotonic_sample_count",
            "censored_count",
            "horizon_unmatured_count",
            "retention_gap_count",
            "path_sample_coverage_gap_count",
            "verdict",
            "positive_research_claim_allowed",
            "l2_baseline_blocker",
        ]:
            add(f"horizon_{horizon_ms}_{metric}", horizon.get(metric), notes)

    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=["metric", "value", "notes"])
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    raise SystemExit(main())
