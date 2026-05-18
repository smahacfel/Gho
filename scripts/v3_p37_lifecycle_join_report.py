#!/usr/bin/env python3
"""
Join P3.7 Outcome Label v2 rows with shadow execution/lifecycle evidence.

This is an offline truth-layer report. It separates market outcome from
executable opportunity and treats unknown execution status as non-success.
Decision logs and label files are never modified.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

import gatekeeper_outcome_labeler as v1


EXECUTION_FEASIBLE_CLEAN = "execution_feasible_clean"
EXECUTION_FEASIBLE_DEGRADED = "execution_feasible_degraded"
EXECUTION_INFEASIBLE = "execution_infeasible"
EXECUTION_UNKNOWN = "execution_unknown"
NO_DISPATCH_EXPECTED = "no_dispatch_expected"


def iter_jsonl(path: Path | None) -> Iterable[dict[str, Any]]:
    if path is None or not path.exists():
        return
    yield from v1.iter_json_objects(path)


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def int_or_none(value: Any) -> int | None:
    return int(value) if isinstance(value, (int, float)) else None


def join_keys(row: dict[str, Any]) -> list[str]:
    keys: list[str] = []
    for field in ("ab_record_id", "join_key", "candidate_id", "execution_candidate_id"):
        value = str_or_none(row.get(field))
        if value:
            keys.append(f"{field}:{value}")

    pool_id = str_or_none(row.get("pool_id"))
    mint = (
        str_or_none(row.get("base_mint"))
        or str_or_none(row.get("mint_id"))
        or str_or_none(row.get("mint"))
    )
    if pool_id and mint:
        keys.append(f"pool_mint:{pool_id}:{mint}")
    return keys


def index_rows(rows: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        for key in join_keys(row):
            indexed.setdefault(key, row)
    return indexed


def best_match(row: dict[str, Any], index: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
    for key in join_keys(row):
        match = index.get(key)
        if match is not None:
            return match
    return None


def key_set(rows: Iterable[dict[str, Any]]) -> set[str]:
    keys: set[str] = set()
    for row in rows:
        keys.update(join_keys(row))
    return keys


def unmatched_artifacts(rows: list[dict[str, Any]], known_keys: set[str]) -> int:
    unmatched = 0
    for row in rows:
        if not any(key in known_keys for key in join_keys(row)):
            unmatched += 1
    return unmatched


def dispatch_expected(decision: dict[str, Any] | None, label: dict[str, Any]) -> bool:
    if decision is None:
        return False
    decision_buy = bool_or_none(decision.get("decision_verdict_buy"))
    if decision_buy is not None:
        return decision_buy
    verdict = str_or_none(decision.get("verdict_type")) or str_or_none(label.get("decision_verdict"))
    return verdict in {"BUY", "EARLY_BUY"}


def decision_ts_ms(decision: dict[str, Any] | None, label: dict[str, Any]) -> int | None:
    if decision:
        for field in ("decision_ts_ms", "ab_t_end_event_ts_ms", "first_seen_ts_ms"):
            value = int_or_none(decision.get(field))
            if value is not None:
                return value
    for field in ("entry_ts_ms",):
        value = int_or_none(label.get(field))
        if value is not None:
            return value
    return None


def classify_success(value: Any) -> bool | None:
    if not isinstance(value, str) or not value:
        return None
    normalized = value.lower()
    if normalized in {"ok", "success", "succeeded", "passed", "accepted", "position_opened", "position_closed"}:
        return True
    if normalized in {"failed", "error", "rejected", "shadow_data_problem", "data_problem", "simulation_failed"}:
        return False
    return None


def execution_evidence_source(
    shadow_entry: dict[str, Any] | None,
    lifecycle: dict[str, Any] | None,
    *,
    expected: bool,
    decision_missing: bool,
) -> str:
    if lifecycle is not None:
        return "shadow_lifecycle"
    if shadow_entry is not None:
        return "shadow_entry"
    if decision_missing or expected:
        return "missing"
    return "proxy_not_available"


def classify_execution(
    *,
    expected: bool,
    decision_missing: bool,
    shadow_entry: dict[str, Any] | None,
    lifecycle: dict[str, Any] | None,
) -> tuple[str, str | None, bool, bool, bool | None, str]:
    source = execution_evidence_source(
        shadow_entry,
        lifecycle,
        expected=expected,
        decision_missing=decision_missing,
    )
    if decision_missing:
        return EXECUTION_UNKNOWN, "missing_decision_row", False, False, None, source

    entry_outcome = classify_success(shadow_entry.get("execution_outcome") if shadow_entry else None)
    dispatch_status = classify_success(lifecycle.get("dispatch_status") if lifecycle else None)
    sim_status = classify_success(lifecycle.get("simulation_outcome") if lifecycle else None)
    record_type = str_or_none(lifecycle.get("record_type") if lifecycle else None)
    error_class = (
        str_or_none(lifecycle.get("error_class") if lifecycle else None)
        or str_or_none(lifecycle.get("classification") if lifecycle else None)
    )
    err = str_or_none(lifecycle.get("err") if lifecycle else None)

    if entry_outcome is False or dispatch_status is False or sim_status is False or error_class or err:
        return (
            EXECUTION_INFEASIBLE,
            error_class or err or "shadow_execution_failed",
            bool(shadow_entry),
            False,
            False,
            source,
        )

    if not expected:
        reason = "dispatch_observed_without_expected" if shadow_entry is not None or lifecycle is not None else None
        return NO_DISPATCH_EXPECTED, reason, False, False, None, source
    if shadow_entry is None and lifecycle is None:
        return (
            EXECUTION_UNKNOWN,
            "dispatch_expected_but_no_shadow_artifacts",
            False,
            False,
            None,
            source,
        )

    shadow_entry_possible = bool(shadow_entry) and entry_outcome is not False
    shadow_exit_possible = record_type == "position_closed"
    if shadow_entry_possible and shadow_exit_possible:
        return EXECUTION_FEASIBLE_CLEAN, None, True, True, True, source
    if shadow_entry_possible or sim_status is True or dispatch_status is True:
        return EXECUTION_FEASIBLE_DEGRADED, "missing_exit_proof", shadow_entry_possible, False, True, source
    return EXECUTION_UNKNOWN, "shadow_artifacts_without_terminal_status", bool(shadow_entry), False, None, source


def decision_quality_class(market: str, execution: str) -> str:
    if market in {"good_clean", "good_dirty"}:
        if execution in {EXECUTION_FEASIBLE_CLEAN, EXECUTION_FEASIBLE_DEGRADED}:
            return "good_executable"
        return "good_not_executable"
    if market in {"bad_clean", "bad_dirty"}:
        return "bad_avoidable"
    if market == "neutral_clean":
        return "neutral"
    return "unknown"


def join_row(
    label: dict[str, Any],
    decision: dict[str, Any] | None,
    shadow_entry: dict[str, Any] | None,
    lifecycle: dict[str, Any] | None,
) -> dict[str, Any]:
    expected = dispatch_expected(decision, label)
    ts = decision_ts_ms(decision, label)
    entry_ts = int_or_none(shadow_entry.get("timestamp_ms") if shadow_entry else None)
    lifecycle_ts = int_or_none(lifecycle.get("timestamp_ms") if lifecycle else None)
    decision_to_sim = lifecycle_ts - ts if lifecycle_ts is not None and ts is not None else None
    decision_to_entry = entry_ts - ts if entry_ts is not None and ts is not None else None
    (
        execution_class,
        no_dispatch_reason,
        entry_possible,
        exit_possible,
        simulation_success,
        evidence_source,
    ) = classify_execution(
        expected=expected,
        decision_missing=decision is None,
        shadow_entry=shadow_entry,
        lifecycle=lifecycle,
    )
    sim_status = (
        str_or_none(lifecycle.get("simulation_outcome") if lifecycle else None)
        or str_or_none(lifecycle.get("dispatch_status") if lifecycle else None)
        or str_or_none(shadow_entry.get("execution_outcome") if shadow_entry else None)
    )
    error_class = (
        str_or_none(lifecycle.get("error_class") if lifecycle else None)
        or str_or_none(lifecycle.get("classification") if lifecycle else None)
        or str_or_none(lifecycle.get("err") if lifecycle else None)
        or (str_or_none(shadow_entry.get("execution_outcome") if shadow_entry else None) if execution_class == EXECUTION_INFEASIBLE else None)
    )

    market_class = str(label.get("market_outcome_class") or "unknown")
    return {
        "ab_record_id": label.get("ab_record_id"),
        "join_key": label.get("join_key"),
        "pool_id": label.get("pool_id"),
        "base_mint": label.get("base_mint"),
        "market_outcome_class": market_class,
        "dispatch_expected": expected,
        "shadow_dispatch_observed": shadow_entry is not None or lifecycle is not None,
        "candidate_id": (lifecycle or shadow_entry or {}).get("candidate_id"),
        "sim_status": sim_status,
        "decision_to_sim_ms": decision_to_sim,
        "entry_materialized_at_ms": entry_ts,
        "decision_to_entry_materialization_ms": decision_to_entry,
        "quote_age_ms": (lifecycle or shadow_entry or {}).get("quote_age_ms"),
        "curve_age_ms": (lifecycle or shadow_entry or {}).get("curve_age_ms"),
        "simulation_success": simulation_success,
        "simulation_error_class": error_class,
        "compute_units": (lifecycle or shadow_entry or {}).get("compute_units"),
        "shadow_entry_possible": entry_possible,
        "shadow_exit_possible": exit_possible,
        "no_dispatch_reason": no_dispatch_reason,
        "unknown_execution_status": execution_class == EXECUTION_UNKNOWN,
        "execution_evidence_source": evidence_source,
        "execution_quality_class": execution_class,
        "decision_quality_class": decision_quality_class(market_class, execution_class),
    }


def build_report(
    decisions_path: Path,
    labels_v2_path: Path,
    shadow_entry_path: Path | None,
    shadow_lifecycle_path: Path | None,
    joined_output_path: Path,
) -> dict[str, Any]:
    decisions = list(iter_jsonl(decisions_path))
    labels = list(iter_jsonl(labels_v2_path))
    shadow_entries = list(iter_jsonl(shadow_entry_path))
    lifecycle_rows = list(iter_jsonl(shadow_lifecycle_path))
    decisions_by_key = index_rows(decisions)
    entries_by_key = index_rows(shadow_entries)
    lifecycle_by_key = index_rows(lifecycle_rows)
    known_label_decision_keys = key_set(labels) | key_set(decisions)

    joined: list[dict[str, Any]] = []
    execution_counts: Counter[str] = Counter()
    decision_quality_counts: Counter[str] = Counter()
    market_counts: Counter[str] = Counter()
    no_dispatch_reasons: Counter[str] = Counter()
    evidence_sources: Counter[str] = Counter()
    unmatched_labels = 0
    dispatch_expected_rows = 0
    shadow_dispatch_observed_rows = 0
    dispatch_observed_without_expected_rows = 0
    dispatch_expected_without_observed_rows = 0
    for label in labels:
        decision = best_match(label, decisions_by_key)
        if decision is None:
            unmatched_labels += 1
        shadow_entry = best_match(label, entries_by_key)
        lifecycle = best_match(label, lifecycle_by_key)
        row = join_row(label, decision, shadow_entry, lifecycle)
        joined.append(row)
        if row["dispatch_expected"]:
            dispatch_expected_rows += 1
        if row["shadow_dispatch_observed"]:
            shadow_dispatch_observed_rows += 1
        if row["shadow_dispatch_observed"] and not row["dispatch_expected"]:
            dispatch_observed_without_expected_rows += 1
        if row["dispatch_expected"] and not row["shadow_dispatch_observed"]:
            dispatch_expected_without_observed_rows += 1
        execution_counts[row["execution_quality_class"]] += 1
        decision_quality_counts[row["decision_quality_class"]] += 1
        market_counts[row["market_outcome_class"]] += 1
        if row["no_dispatch_reason"]:
            no_dispatch_reasons[row["no_dispatch_reason"]] += 1
        evidence_sources[row["execution_evidence_source"]] += 1

    write_jsonl(joined_output_path, joined)
    return {
        "status": "ok",
        "decisions": len(decisions),
        "labels_v2": len(labels),
        "shadow_entry_rows": len(shadow_entries),
        "shadow_lifecycle_rows": len(lifecycle_rows),
        "unmatched_shadow_entry_rows": unmatched_artifacts(shadow_entries, known_label_decision_keys),
        "unmatched_shadow_lifecycle_rows": unmatched_artifacts(lifecycle_rows, known_label_decision_keys),
        "joined_rows": len(joined),
        "unmatched_label_rows": unmatched_labels,
        "dispatch_expected_rows": dispatch_expected_rows,
        "shadow_dispatch_observed_rows": shadow_dispatch_observed_rows,
        "dispatch_observed_without_expected_rows": dispatch_observed_without_expected_rows,
        "dispatch_expected_without_observed_rows": dispatch_expected_without_observed_rows,
        "execution_quality_class_counts": dict(sorted(execution_counts.items())),
        "decision_quality_class_counts": dict(sorted(decision_quality_counts.items())),
        "market_outcome_class_counts": dict(sorted(market_counts.items())),
        "no_dispatch_reason_counts": dict(sorted(no_dispatch_reasons.items())),
        "execution_evidence_source_counts": dict(sorted(evidence_sources.items())),
        "unknown_execution_status_rows": execution_counts.get(EXECUTION_UNKNOWN, 0),
        "output": str(joined_output_path),
    }


def render_markdown(summary: dict[str, Any]) -> str:
    lines = [
        "# P3.7 Execution Feasibility Summary",
        "",
        f"Status: `{summary['status']}`",
        "",
        "## Counts",
        "",
        f"- Decisions: `{summary['decisions']}`",
        f"- Label v2 rows: `{summary['labels_v2']}`",
        f"- Shadow entry rows: `{summary['shadow_entry_rows']}`",
        f"- Shadow lifecycle rows: `{summary['shadow_lifecycle_rows']}`",
        f"- Unmatched shadow entry rows: `{summary['unmatched_shadow_entry_rows']}`",
        f"- Unmatched shadow lifecycle rows: `{summary['unmatched_shadow_lifecycle_rows']}`",
        f"- Joined rows: `{summary['joined_rows']}`",
        f"- Unmatched label rows: `{summary['unmatched_label_rows']}`",
        f"- Dispatch expected rows: `{summary['dispatch_expected_rows']}`",
        f"- Shadow dispatch observed rows: `{summary['shadow_dispatch_observed_rows']}`",
        f"- Dispatch observed without expected rows: `{summary['dispatch_observed_without_expected_rows']}`",
        f"- Dispatch expected without observed rows: `{summary['dispatch_expected_without_observed_rows']}`",
        f"- Unknown execution status rows: `{summary['unknown_execution_status_rows']}`",
        "",
        "## Execution Quality",
        "",
    ]
    for key, value in summary["execution_quality_class_counts"].items():
        lines.append(f"- `{key}`: `{value}`")
    lines.extend(["", "## Decision Quality", ""])
    for key, value in summary["decision_quality_class_counts"].items():
        lines.append(f"- `{key}`: `{value}`")
    lines.extend(["", "## No Dispatch / Unknown / Failure Reasons", ""])
    if summary["no_dispatch_reason_counts"]:
        for key, value in summary["no_dispatch_reason_counts"].items():
            lines.append(f"- `{key}`: `{value}`")
    else:
        lines.append("- none")
    lines.extend(["", "## Execution Evidence Sources", ""])
    for key, value in summary["execution_evidence_source_counts"].items():
        lines.append(f"- `{key}`: `{value}`")
    lines.extend(
        [
            "",
            "## Governance",
            "",
            "- Unknown execution status is not success.",
            "- REJECT/PENDING no-dispatch is `no_dispatch_expected`, not execution failure.",
            "- Market-good rows are not `good_executable` without real shadow entry/lifecycle or simulation evidence.",
            "- `AccountNotFound`, simulation failure, and data-problem lifecycle rows are `execution_infeasible`.",
            "- This report does not mutate decision logs, labels, runtime behavior, Gatekeeper policy, P2, or live execution.",
        ]
    )
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decisions", required=True, type=Path, help="gatekeeper_v2_decisions.jsonl")
    parser.add_argument("--labels-v2", required=True, type=Path, help="P3.7 label v2 JSONL")
    parser.add_argument("--shadow-entry", type=Path, help="shadow_entries.jsonl")
    parser.add_argument("--shadow-lifecycle", type=Path, help="shadow_lifecycle.jsonl")
    parser.add_argument("--output", required=True, type=Path, help="joined execution feasibility JSONL")
    parser.add_argument("--summary-output", required=True, type=Path, help="summary JSON")
    parser.add_argument("--summary-md-output", type=Path, help="summary Markdown")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    summary = build_report(
        args.decisions,
        args.labels_v2,
        args.shadow_entry,
        args.shadow_lifecycle,
        args.output,
    )
    args.summary_output.parent.mkdir(parents=True, exist_ok=True)
    args.summary_output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.summary_md_output:
        args.summary_md_output.parent.mkdir(parents=True, exist_ok=True)
        args.summary_md_output.write_text(render_markdown(summary), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
