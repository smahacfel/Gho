#!/usr/bin/env python3
"""Summarize P3.7 shadow-onchain lifecycle recovery reports."""

from __future__ import annotations

import argparse
import json
import math
import statistics as st
from collections import Counter
from pathlib import Path
from typing import Any, Iterable


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if isinstance(obj, dict):
                yield obj


def nested(row: dict[str, Any], *keys: str) -> Any:
    value: Any = row
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def finite_float(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def finite_int(value: Any) -> int | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return int(value)
    return None


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def percentile(sorted_values: list[float], pct: float) -> float | None:
    if not sorted_values:
        return None
    if len(sorted_values) == 1:
        return sorted_values[0]
    rank = (len(sorted_values) - 1) * pct
    lower = math.floor(rank)
    upper = math.ceil(rank)
    if lower == upper:
        return sorted_values[int(rank)]
    weight = rank - lower
    return sorted_values[lower] * (1.0 - weight) + sorted_values[upper] * weight


def distribution(values: Iterable[float | int | None], *, abs_values: bool = False) -> dict[str, Any]:
    cleaned: list[float] = []
    for value in values:
        number = finite_float(value)
        if number is None:
            continue
        cleaned.append(abs(number) if abs_values else number)
    if not cleaned:
        return {
            "count": 0,
            "min": None,
            "p50": None,
            "p90": None,
            "p99": None,
            "max": None,
            "mean": None,
        }
    ordered = sorted(cleaned)
    return {
        "count": len(ordered),
        "min": ordered[0],
        "p50": percentile(ordered, 0.50),
        "p90": percentile(ordered, 0.90),
        "p99": percentile(ordered, 0.99),
        "max": ordered[-1],
        "mean": st.fmean(ordered),
    }


def proof_class_for_finality(finality: str | None) -> str:
    normalized = (finality or "").strip().lower()
    if normalized == "finalized":
        return "shadow_onchain_finalized_verified"
    if normalized == "confirmed":
        return "shadow_onchain_confirmed_verified"
    if normalized == "speculative":
        return "shadow_onchain_speculative_snapshot_verified"
    if normalized:
        return "shadow_onchain_snapshot_verified_non_final"
    return "shadow_onchain_degraded_unknown_finality"


def pnl_bucket(final_pnl_pct: Any) -> str:
    value = finite_float(final_pnl_pct)
    if value is None:
        return "unknown"
    if value > 0.0:
        return "positive"
    if value < 0.0:
        return "negative"
    return "neutral"


def summarize_rows(rows: list[dict[str, Any]], *, namespace: str, config: str | None, input_path: Path) -> dict[str, Any]:
    analysis_status = Counter(str(row.get("analysis_status") or "unknown") for row in rows)
    truth_status = Counter(str(row.get("truth_status") or "unknown") for row in rows)
    truth_source = Counter(str(row.get("truth_source") or "unknown") for row in rows)
    entry_finality = Counter(
        str(nested(row, "onchain", "entry", "curve_finality") or "unknown") for row in rows
    )
    entry_proof = Counter(
        proof_class_for_finality(nested(row, "onchain", "entry", "curve_finality")) for row in rows
    )
    exit_finality: Counter[str] = Counter()
    exit_proof: Counter[str] = Counter()
    exit_fill_count = 0
    exit_fill_gap_values: list[int] = []
    for row in rows:
        fills = row.get("exit_fills")
        if not isinstance(fills, list):
            continue
        for fill in fills:
            if not isinstance(fill, dict):
                continue
            exit_fill_count += 1
            finality = fill.get("onchain_curve_finality")
            exit_finality[str(finality or "unknown")] += 1
            exit_proof[proof_class_for_finality(finality if isinstance(finality, str) else None)] += 1
            gap = finite_int(fill.get("onchain_match_delta_ms"))
            if gap is not None:
                exit_fill_gap_values.append(abs(gap))

    close_reason = Counter(str(row.get("close_reason") or "unknown") for row in rows)
    shadow_execution = Counter(str(nested(row, "shadow", "execution_outcome") or "unknown") for row in rows)
    pnl_counts = Counter(pnl_bucket(nested(row, "shadow", "final_pnl_pct")) for row in rows)
    gatekeeper_buy_context_found = sum(
        1 for row in rows if nested(row, "timing", "gatekeeper_buy_context_found") is True
    )
    gatekeeper_buy_context_missing = len(rows) - gatekeeper_buy_context_found
    position_closed_rows = sum(1 for row in rows if row.get("position_id") or nested(row, "timing", "close_ts_ms"))

    entry_truth_gaps = [abs(value) for value in (finite_int(nested(row, "onchain", "entry", "match_delta_ms")) for row in rows) if value is not None]
    exit_truth_gap_max_per_position = [
        value
        for value in (
            finite_int(nested(row, "onchain", "exit", "max_abs_truth_gap_ms")) for row in rows
        )
        if value is not None
    ]
    entry_drift = [
        value
        for value in (
            finite_float(nested(row, "drift_pct", "entry_vs_onchain_executable")) for row in rows
        )
        if value is not None
    ]
    exit_drift = [
        value
        for value in (
            finite_float(nested(row, "drift_pct", "exit_vs_onchain_executable")) for row in rows
        )
        if value is not None
    ]
    decision_to_execution_ms = [
        value
        for value in (finite_int(nested(row, "timing", "decision_to_execution_ms")) for row in rows)
        if value is not None
    ]
    detection_to_execution_ms = [
        value
        for value in (finite_int(nested(row, "timing", "detection_to_execution_ms")) for row in rows)
        if value is not None
    ]

    acceptance_checks = {
        "rows_total_gt_0": len(rows) > 0,
        "analysis_status_ok_gt_0": analysis_status.get("ok", 0) > 0,
        "truth_status_resolved_gt_0": truth_status.get("resolved", 0) > 0,
        "position_closed_gt_0": position_closed_rows > 0,
        "gatekeeper_buy_context_found_gt_0": gatekeeper_buy_context_found > 0,
        "entry_truth_gap_distribution_present": bool(entry_truth_gaps),
        "exit_truth_gap_distribution_present": bool(exit_truth_gap_max_per_position or exit_fill_gap_values),
        "curve_finality_distribution_present": bool(rows) and bool(entry_finality),
        "pnl_positive_or_negative_counted": pnl_counts.get("positive", 0) + pnl_counts.get("negative", 0) > 0,
    }

    return {
        "schema_version": 1,
        "namespace": namespace,
        "config": config,
        "input": str(input_path),
        "rows_total": len(rows),
        "analysis_status_counts": counter_dict(analysis_status),
        "truth_status_counts": counter_dict(truth_status),
        "truth_source_counts": counter_dict(truth_source),
        "curve_finality_entry_counts": counter_dict(entry_finality),
        "curve_finality_exit_fill_counts": counter_dict(exit_finality),
        "execution_verification_class_hint_entry_counts": counter_dict(entry_proof),
        "execution_verification_class_hint_exit_fill_counts": counter_dict(exit_proof),
        "position_closed_rows": position_closed_rows,
        "exit_filled_rows": exit_fill_count,
        "final_pnl_pct_counts": counter_dict(pnl_counts),
        "close_reason_counts": counter_dict(close_reason),
        "gatekeeper_buy_context_found_count": gatekeeper_buy_context_found,
        "gatekeeper_buy_context_missing_count": gatekeeper_buy_context_missing,
        "shadow_execution_outcome_counts": counter_dict(shadow_execution),
        "entry_truth_gap_ms": distribution(entry_truth_gaps),
        "exit_truth_gap_max_per_position_ms": distribution(exit_truth_gap_max_per_position),
        "exit_truth_gap_fill_ms": distribution(exit_fill_gap_values),
        "entry_drift_vs_onchain_executable_pct": distribution(entry_drift),
        "entry_abs_drift_vs_onchain_executable_pct": distribution(entry_drift, abs_values=True),
        "exit_drift_vs_onchain_executable_pct": distribution(exit_drift),
        "exit_abs_drift_vs_onchain_executable_pct": distribution(exit_drift, abs_values=True),
        "decision_to_execution_ms": distribution(decision_to_execution_ms),
        "detection_to_execution_ms": distribution(detection_to_execution_ms),
        "acceptance_checks": acceptance_checks,
        "phase_e_recovery_status": "accepted" if all(acceptance_checks.values()) else "not_accepted",
    }


def fmt_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


def fmt_dist(value: dict[str, Any]) -> str:
    return fmt_json(value)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_markdown(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    lines.append("# P3.7 Shadow-Onchain Lifecycle Recovery")
    lines.append("")
    lines.append(f"Namespace: `{payload['namespace']}`")
    lines.append(f"Config: `{payload.get('config') or 'unknown'}`")
    lines.append(f"Input: `{payload['input']}`")
    lines.append(f"Phase E recovery status: `{payload['phase_e_recovery_status']}`")
    lines.append("")
    lines.append("## Acceptance Checks")
    lines.append("")
    for key, value in payload["acceptance_checks"].items():
        lines.append(f"- `{key}`: `{str(value).lower()}`")
    lines.append("")
    lines.append("## Counts")
    lines.append("")
    lines.append(f"- `rows_total`: `{payload['rows_total']}`")
    lines.append(f"- `analysis_status_counts`: `{fmt_json(payload['analysis_status_counts'])}`")
    lines.append(f"- `truth_status_counts`: `{fmt_json(payload['truth_status_counts'])}`")
    lines.append(f"- `truth_source_counts`: `{fmt_json(payload['truth_source_counts'])}`")
    lines.append(f"- `curve_finality_entry_counts`: `{fmt_json(payload['curve_finality_entry_counts'])}`")
    lines.append(f"- `curve_finality_exit_fill_counts`: `{fmt_json(payload['curve_finality_exit_fill_counts'])}`")
    lines.append(f"- `execution_verification_class_hint_entry_counts`: `{fmt_json(payload['execution_verification_class_hint_entry_counts'])}`")
    lines.append(f"- `execution_verification_class_hint_exit_fill_counts`: `{fmt_json(payload['execution_verification_class_hint_exit_fill_counts'])}`")
    lines.append(f"- `position_closed_rows`: `{payload['position_closed_rows']}`")
    lines.append(f"- `exit_filled_rows`: `{payload['exit_filled_rows']}`")
    lines.append(f"- `final_pnl_pct_counts`: `{fmt_json(payload['final_pnl_pct_counts'])}`")
    lines.append(f"- `close_reason_counts`: `{fmt_json(payload['close_reason_counts'])}`")
    lines.append(f"- `gatekeeper_buy_context_found_count`: `{payload['gatekeeper_buy_context_found_count']}`")
    lines.append(f"- `gatekeeper_buy_context_missing_count`: `{payload['gatekeeper_buy_context_missing_count']}`")
    lines.append(f"- `shadow_execution_outcome_counts`: `{fmt_json(payload['shadow_execution_outcome_counts'])}`")
    lines.append("")
    lines.append("## Distributions")
    lines.append("")
    lines.append(f"- `entry_truth_gap_ms`: `{fmt_dist(payload['entry_truth_gap_ms'])}`")
    lines.append(f"- `exit_truth_gap_max_per_position_ms`: `{fmt_dist(payload['exit_truth_gap_max_per_position_ms'])}`")
    lines.append(f"- `exit_truth_gap_fill_ms`: `{fmt_dist(payload['exit_truth_gap_fill_ms'])}`")
    lines.append(f"- `entry_drift_vs_onchain_executable_pct`: `{fmt_dist(payload['entry_drift_vs_onchain_executable_pct'])}`")
    lines.append(f"- `entry_abs_drift_vs_onchain_executable_pct`: `{fmt_dist(payload['entry_abs_drift_vs_onchain_executable_pct'])}`")
    lines.append(f"- `exit_drift_vs_onchain_executable_pct`: `{fmt_dist(payload['exit_drift_vs_onchain_executable_pct'])}`")
    lines.append(f"- `exit_abs_drift_vs_onchain_executable_pct`: `{fmt_dist(payload['exit_abs_drift_vs_onchain_executable_pct'])}`")
    lines.append(f"- `decision_to_execution_ms`: `{fmt_dist(payload['decision_to_execution_ms'])}`")
    lines.append(f"- `detection_to_execution_ms`: `{fmt_dist(payload['detection_to_execution_ms'])}`")
    lines.append("")
    lines.append("## Scope Notes")
    lines.append("")
    lines.append("- This report summarizes shadow-onchain lifecycle recovery only.")
    lines.append("- Shadow lifecycle proof is not live inclusion and does not prove strategy edge.")
    lines.append("- Non-finalized finality values are snapshot/degraded proof hints, not finalized proof.")
    lines.append("- Phase B feature prototype remains blocked until labeler and feature availability audit are complete.")
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path, help="shadow_onchain_lifecycle_report JSONL input.")
    parser.add_argument("--namespace", required=True, help="Artifact namespace being summarized.")
    parser.add_argument("--config", help="Config path used to produce the report.")
    parser.add_argument("--output-md", required=True, type=Path, help="Markdown report output path.")
    parser.add_argument("--output-json", type=Path, help="Optional JSON summary output path.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    rows = list(iter_jsonl(args.input))
    payload = summarize_rows(rows, namespace=args.namespace, config=args.config, input_path=args.input)
    if args.output_json is not None:
        write_json(args.output_json, payload)
    write_markdown(args.output_md, payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
