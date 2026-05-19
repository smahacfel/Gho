#!/usr/bin/env python3
"""Label P3.7 shadow lifecycle/on-chain recovery rows.

The labeler keeps market outcome, execution verification, truth-gap quality,
and final buy-quality as separate axes. It does not promote speculative
snapshot truth to finalized proof.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics as st
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1
JOIN_METADATA_FIELDS = (
    "ab_record_id",
    "v3_feature_snapshot_hash",
    "v3_policy_config_hash",
    "decision_plane",
    "rollout_namespace",
)
GOOD_EXECUTION_CLASSES = {
    "shadow_onchain_finalized_verified",
    "shadow_onchain_confirmed_verified",
    "live_confirmed_verified",
}
USABLE_EXECUTION_CLASSES = GOOD_EXECUTION_CLASSES | {
    "shadow_onchain_snapshot_verified",
    "shadow_onchain_speculative_snapshot_verified",
    "shadow_onchain_degraded",
}


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            try:
                row = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=False))
            fh.write("\n")


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


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
        return {"count": 0, "min": None, "p50": None, "p90": None, "p99": None, "max": None, "mean": None}
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


def classify_market(row: dict[str, Any]) -> tuple[str, str | None]:
    if row.get("analysis_status") != "ok":
        return "market_unknown", "analysis_status_not_ok"
    if row.get("truth_status") != "resolved":
        return "market_unknown", "truth_status_not_resolved"
    pnl_pct = finite_float(nested(row, "shadow", "final_pnl_pct"))
    if pnl_pct is None:
        return "market_unknown", "missing_final_pnl_pct"
    if pnl_pct > 0.0:
        return "market_good_clean", None
    if pnl_pct < 0.0:
        return "market_bad_clean", None
    return "market_neutral", None


def finality_values(row: dict[str, Any]) -> tuple[str | None, list[str]]:
    entry_finality = nested(row, "onchain", "entry", "curve_finality")
    exit_values: list[str] = []
    fills = row.get("exit_fills")
    if isinstance(fills, list):
        for fill in fills:
            if isinstance(fill, dict) and isinstance(fill.get("onchain_curve_finality"), str):
                exit_values.append(fill["onchain_curve_finality"])
    return entry_finality if isinstance(entry_finality, str) else None, exit_values


def summarize_finality(values: list[str]) -> str | None:
    if not values:
        return None
    unique = sorted(set(values))
    if len(unique) == 1:
        return unique[0]
    return ",".join(unique)


def classify_execution(row: dict[str, Any]) -> tuple[str, str | None]:
    if row.get("analysis_status") != "ok":
        return "shadow_execution_unknown", "analysis_status_not_ok"
    if row.get("truth_status") != "resolved":
        return "shadow_execution_unknown", "truth_status_not_resolved"
    if row.get("truth_source") not in {"canonical_account_state_snapshot", "diag_account_update_relay"}:
        return "shadow_onchain_degraded", "truth_source_noncanonical_or_unknown"
    execution_outcome = nested(row, "shadow", "execution_outcome")
    if execution_outcome != "shadow_simulated":
        if isinstance(execution_outcome, str) and "error" in execution_outcome.lower():
            return "shadow_execution_infeasible", "shadow_execution_error"
        return "shadow_execution_unknown", "shadow_execution_outcome_not_simulated"

    entry_finality, exit_finalities = finality_values(row)
    all_finalities = [value.lower() for value in ([entry_finality] if entry_finality else []) + exit_finalities]
    if not all_finalities:
        return "shadow_onchain_degraded", "missing_curve_finality"
    if any(value == "speculative" for value in all_finalities):
        return "shadow_onchain_speculative_snapshot_verified", None
    if all(value == "finalized" for value in all_finalities):
        return "shadow_onchain_finalized_verified", None
    if all(value in {"confirmed", "finalized"} for value in all_finalities):
        return "shadow_onchain_confirmed_verified", None
    if all_finalities:
        return "shadow_onchain_degraded", "nonstandard_curve_finality"
    return "shadow_execution_unknown", "missing_curve_finality"


def classify_entry_gap(gap_ms: int | None, clean_ms: int, degraded_ms: int) -> str:
    if gap_ms is None:
        return "truth_gap_unknown"
    if gap_ms <= clean_ms:
        return "truth_gap_clean"
    if gap_ms <= degraded_ms:
        return "truth_gap_degraded_acceptable"
    return "truth_gap_too_large"


def classify_exit_gap(
    gap_ms: int | None,
    close_reason: str | None,
    clean_ms: int,
    timestop_acceptable_ms: int,
    other_acceptable_ms: int,
) -> str:
    if gap_ms is None:
        return "truth_gap_unknown"
    if gap_ms <= clean_ms:
        return "truth_gap_clean"
    acceptable = timestop_acceptable_ms if close_reason == "TimeStop" else other_acceptable_ms
    if gap_ms <= acceptable:
        return "truth_gap_degraded_acceptable"
    return "truth_gap_too_large"


def worst_gap_class(entry_class: str, exit_class: str) -> str:
    order = {
        "truth_gap_clean": 0,
        "truth_gap_degraded_acceptable": 1,
        "truth_gap_unknown": 2,
        "truth_gap_too_large": 3,
    }
    return max((entry_class, exit_class), key=lambda value: order.get(value, 4))


def classify_drift(value: float | None, acceptable_abs_pct: float) -> str:
    if value is None:
        return "drift_unknown"
    if abs(value) <= acceptable_abs_pct:
        return "drift_acceptable"
    return "drift_degraded"


def build_label(row: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    market_class, market_unknown_reason = classify_market(row)
    execution_class, execution_unknown_reason = classify_execution(row)
    entry_finality, exit_finalities = finality_values(row)
    entry_gap = finite_int(nested(row, "onchain", "entry", "match_delta_ms"))
    entry_gap_abs = abs(entry_gap) if entry_gap is not None else None
    exit_gap = finite_int(nested(row, "onchain", "exit", "max_abs_truth_gap_ms"))
    close_reason = row.get("close_reason") if isinstance(row.get("close_reason"), str) else None
    entry_gap_class = classify_entry_gap(
        entry_gap_abs,
        args.entry_truth_gap_clean_ms,
        args.entry_truth_gap_degraded_acceptable_ms,
    )
    exit_gap_class = classify_exit_gap(
        exit_gap,
        close_reason,
        args.exit_truth_gap_clean_ms,
        args.exit_truth_gap_timestop_acceptable_ms,
        args.exit_truth_gap_other_acceptable_ms,
    )
    truth_gap_class = worst_gap_class(entry_gap_class, exit_gap_class)
    gatekeeper_context = nested(row, "timing", "gatekeeper_buy_context_found") is True
    entry_drift_exec = finite_float(nested(row, "drift_pct", "entry_vs_onchain_executable"))
    exit_drift_exec = finite_float(nested(row, "drift_pct", "exit_vs_onchain_executable"))
    entry_drift_spot = finite_float(nested(row, "drift_pct", "entry_vs_onchain_spot"))
    exit_drift_spot = finite_float(nested(row, "drift_pct", "exit_vs_onchain_spot"))
    entry_drift_class = classify_drift(entry_drift_exec, args.entry_drift_acceptable_abs_pct)
    exit_drift_class = classify_drift(exit_drift_exec, args.exit_drift_acceptable_abs_pct)

    unknown_reasons = [
        reason
        for reason in (market_unknown_reason, execution_unknown_reason)
        if reason is not None
    ]
    degraded_reasons: list[str] = []
    if execution_class == "shadow_onchain_speculative_snapshot_verified":
        degraded_reasons.append("speculative_curve_finality")
    elif execution_class == "shadow_onchain_degraded":
        degraded_reasons.append(execution_unknown_reason or "execution_verification_degraded")
    if not gatekeeper_context:
        degraded_reasons.append("missing_gatekeeper_buy_context")
    for name, klass in (("entry", entry_gap_class), ("exit", exit_gap_class)):
        if klass == "truth_gap_degraded_acceptable":
            degraded_reasons.append(f"{name}_truth_gap_degraded_acceptable")
        elif klass == "truth_gap_too_large":
            degraded_reasons.append(f"{name}_truth_gap_too_large")
        elif klass == "truth_gap_unknown":
            degraded_reasons.append(f"{name}_truth_gap_unknown")
    if entry_drift_class == "drift_degraded":
        degraded_reasons.append("entry_drift_degraded")
    elif entry_drift_class == "drift_unknown":
        degraded_reasons.append("entry_drift_unknown")
    if exit_drift_class == "drift_degraded":
        degraded_reasons.append("exit_drift_degraded")
    elif exit_drift_class == "drift_unknown":
        degraded_reasons.append("exit_drift_unknown")

    if execution_class == "shadow_execution_infeasible":
        buy_quality = "buy_quality_not_executable"
    elif market_class == "market_unknown" or execution_class == "shadow_execution_unknown":
        buy_quality = "buy_quality_unknown"
    elif market_class == "market_neutral":
        buy_quality = "buy_quality_neutral"
    elif market_class == "market_bad_clean":
        buy_quality = "buy_quality_bad"
    elif (
        market_class == "market_good_clean"
        and execution_class in GOOD_EXECUTION_CLASSES
        and truth_gap_class == "truth_gap_clean"
        and gatekeeper_context
        and entry_drift_class == "drift_acceptable"
        and exit_drift_class == "drift_acceptable"
    ):
        buy_quality = "buy_quality_good"
    elif (
        market_class == "market_good_clean"
        and execution_class in USABLE_EXECUTION_CLASSES
        and truth_gap_class in {"truth_gap_clean", "truth_gap_degraded_acceptable"}
    ):
        buy_quality = "buy_quality_dirty_good"
    else:
        buy_quality = "buy_quality_unknown"
        if truth_gap_class in {"truth_gap_too_large", "truth_gap_unknown"}:
            unknown_reasons.append(truth_gap_class)

    if buy_quality in {"buy_quality_unknown", "buy_quality_not_executable"}:
        label_quality = "unknown" if buy_quality == "buy_quality_unknown" else "not_executable"
    elif degraded_reasons or buy_quality == "buy_quality_dirty_good":
        label_quality = "degraded"
    else:
        label_quality = "clean"

    label = {
        "schema_version": SCHEMA_VERSION,
        "candidate_id": row.get("candidate_id"),
        "position_id": row.get("position_id"),
        "pool_id": row.get("pool_id"),
        "base_mint": row.get("mint_id"),
        "close_reason": close_reason,
        "analysis_status": row.get("analysis_status"),
        "truth_status": row.get("truth_status"),
        "truth_source": row.get("truth_source"),
        "sample_price_state": row.get("sample_price_state"),
        "market_outcome_class": market_class,
        "execution_verification_class": execution_class,
        "truth_gap_class": truth_gap_class,
        "buy_quality_class": buy_quality,
        "gatekeeper_buy_context_found": gatekeeper_context,
        "decision_ts_ms": nested(row, "timing", "decision_ts_ms"),
        "entry_execution_ts_ms": nested(row, "timing", "entry_execution_ts_ms"),
        "close_ts_ms": nested(row, "timing", "close_ts_ms"),
        "position_duration_ms": nested(row, "timing", "position_duration_ms"),
        "decision_to_execution_ms": nested(row, "timing", "decision_to_execution_ms"),
        "detection_to_execution_ms": nested(row, "timing", "detection_to_execution_ms"),
        "curve_finality_entry": entry_finality,
        "curve_finality_exit": summarize_finality(exit_finalities),
        "entry_truth_gap_ms": entry_gap_abs,
        "exit_truth_gap_ms": exit_gap,
        "entry_truth_gap_class": entry_gap_class,
        "exit_truth_gap_class": exit_gap_class,
        "entry_drift_vs_onchain_executable_pct": entry_drift_exec,
        "exit_drift_vs_onchain_executable_pct": exit_drift_exec,
        "entry_drift_vs_onchain_spot_pct": entry_drift_spot,
        "exit_drift_vs_onchain_spot_pct": exit_drift_spot,
        "entry_drift_class": entry_drift_class,
        "exit_drift_class": exit_drift_class,
        "final_pnl_sol": nested(row, "shadow", "final_pnl_sol"),
        "final_pnl_pct": nested(row, "shadow", "final_pnl_pct"),
        "gross_pnl_sol": nested(row, "shadow", "gross_pnl_sol"),
        "net_pnl_sol": nested(row, "shadow", "net_pnl_sol"),
        "estimated_costs_sol": nested(row, "shadow", "estimated_costs_sol"),
        "total_exits": nested(row, "shadow", "total_exits"),
        "label_quality": label_quality,
        "unknown_reason": ";".join(sorted(set(unknown_reasons))) if unknown_reasons else None,
        "degraded_reasons": sorted(set(degraded_reasons)),
    }
    for field in JOIN_METADATA_FIELDS:
        label[field] = row.get(field) if isinstance(row.get(field), str) else None
    return label


def build_labels(rows: list[dict[str, Any]], args: argparse.Namespace) -> list[dict[str, Any]]:
    return [build_label(row, args) for row in rows]


def build_summary(labels: list[dict[str, Any]], *, source_path: Path, output_path: Path, args: argparse.Namespace) -> dict[str, Any]:
    counts = {
        "analysis_status_counts": Counter(str(row.get("analysis_status") or "unknown") for row in labels),
        "truth_status_counts": Counter(str(row.get("truth_status") or "unknown") for row in labels),
        "market_outcome_class_counts": Counter(str(row.get("market_outcome_class") or "unknown") for row in labels),
        "execution_verification_class_counts": Counter(str(row.get("execution_verification_class") or "unknown") for row in labels),
        "truth_gap_class_counts": Counter(str(row.get("truth_gap_class") or "unknown") for row in labels),
        "entry_truth_gap_class_counts": Counter(str(row.get("entry_truth_gap_class") or "unknown") for row in labels),
        "exit_truth_gap_class_counts": Counter(str(row.get("exit_truth_gap_class") or "unknown") for row in labels),
        "buy_quality_class_counts": Counter(str(row.get("buy_quality_class") or "unknown") for row in labels),
        "label_quality_counts": Counter(str(row.get("label_quality") or "unknown") for row in labels),
        "close_reason_counts": Counter(str(row.get("close_reason") or "unknown") for row in labels),
        "curve_finality_entry_counts": Counter(str(row.get("curve_finality_entry") or "unknown") for row in labels),
        "curve_finality_exit_counts": Counter(str(row.get("curve_finality_exit") or "unknown") for row in labels),
    }
    degraded_reasons = Counter(
        reason
        for row in labels
        for reason in row.get("degraded_reasons", [])
        if isinstance(reason, str)
    )
    by_context = {
        "gatekeeper_context_rows": sum(1 for row in labels if row.get("gatekeeper_buy_context_found") is True),
        "no_gatekeeper_context_rows": sum(1 for row in labels if row.get("gatekeeper_buy_context_found") is not True),
    }
    close_reason_buy_quality: dict[str, dict[str, int]] = defaultdict(dict)
    for row in labels:
        close_reason = str(row.get("close_reason") or "unknown")
        quality = str(row.get("buy_quality_class") or "unknown")
        close_reason_buy_quality.setdefault(close_reason, {})
        close_reason_buy_quality[close_reason][quality] = close_reason_buy_quality[close_reason].get(quality, 0) + 1

    summary = {
        "schema_version": SCHEMA_VERSION,
        "source": str(source_path),
        "output": str(output_path),
        "rows_total": len(labels),
        "all_lifecycle_rows": len(labels),
        "thresholds": {
            "entry_truth_gap_clean_ms": args.entry_truth_gap_clean_ms,
            "entry_truth_gap_degraded_acceptable_ms": args.entry_truth_gap_degraded_acceptable_ms,
            "exit_truth_gap_clean_ms": args.exit_truth_gap_clean_ms,
            "exit_truth_gap_timestop_acceptable_ms": args.exit_truth_gap_timestop_acceptable_ms,
            "exit_truth_gap_other_acceptable_ms": args.exit_truth_gap_other_acceptable_ms,
            "entry_drift_acceptable_abs_pct": args.entry_drift_acceptable_abs_pct,
            "exit_drift_acceptable_abs_pct": args.exit_drift_acceptable_abs_pct,
        },
        "gatekeeper_context_split": by_context,
        "close_reason_by_buy_quality": {
            key: dict(sorted(value.items())) for key, value in sorted(close_reason_buy_quality.items())
        },
        "degraded_reason_counts": counter_dict(degraded_reasons),
        "entry_truth_gap_ms": distribution(row.get("entry_truth_gap_ms") for row in labels),
        "exit_truth_gap_ms": distribution(row.get("exit_truth_gap_ms") for row in labels),
        "entry_abs_drift_vs_onchain_executable_pct": distribution(
            (row.get("entry_drift_vs_onchain_executable_pct") for row in labels), abs_values=True
        ),
        "exit_abs_drift_vs_onchain_executable_pct": distribution(
            (row.get("exit_drift_vs_onchain_executable_pct") for row in labels), abs_values=True
        ),
        "decision_to_execution_ms": distribution(row.get("decision_to_execution_ms") for row in labels),
        "detection_to_execution_ms": distribution(row.get("detection_to_execution_ms") for row in labels),
        "phase_f_label_status": "accepted" if phase_f_acceptance(labels) else "not_accepted",
    }
    for key, counter in counts.items():
        summary[key] = counter_dict(counter)
    return summary


def phase_f_acceptance(labels: list[dict[str, Any]]) -> bool:
    market = Counter(row.get("market_outcome_class") for row in labels)
    execution = Counter(row.get("execution_verification_class") for row in labels)
    buy_quality = Counter(row.get("buy_quality_class") for row in labels)
    return (
        len(labels) > 0
        and market.get("market_good_clean", 0) > 0
        and market.get("market_bad_clean", 0) > 0
        and execution.get("shadow_onchain_speculative_snapshot_verified", 0) > 0
        and buy_quality.get("buy_quality_dirty_good", 0) > 0
        and buy_quality.get("buy_quality_bad", 0) > 0
    )


def render_markdown(summary: dict[str, Any]) -> str:
    lines: list[str] = []
    lines.append("# P3.7 Shadow Lifecycle Labels - Buy Heavy Rerun")
    lines.append("")
    lines.append(f"Source: `{summary['source']}`")
    lines.append(f"Output: `{summary['output']}`")
    lines.append(f"Phase F label status: `{summary['phase_f_label_status']}`")
    lines.append("")
    lines.append("## Counts")
    lines.append("")
    for key in (
        "rows_total",
        "all_lifecycle_rows",
        "analysis_status_counts",
        "truth_status_counts",
        "market_outcome_class_counts",
        "execution_verification_class_counts",
        "truth_gap_class_counts",
        "entry_truth_gap_class_counts",
        "exit_truth_gap_class_counts",
        "buy_quality_class_counts",
        "label_quality_counts",
        "close_reason_counts",
        "curve_finality_entry_counts",
        "curve_finality_exit_counts",
        "gatekeeper_context_split",
        "close_reason_by_buy_quality",
        "degraded_reason_counts",
    ):
        lines.append(f"- `{key}`: `{json.dumps(summary[key], ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Distributions")
    lines.append("")
    for key in (
        "entry_truth_gap_ms",
        "exit_truth_gap_ms",
        "entry_abs_drift_vs_onchain_executable_pct",
        "exit_abs_drift_vs_onchain_executable_pct",
        "decision_to_execution_ms",
        "detection_to_execution_ms",
    ):
        lines.append(f"- `{key}`: `{json.dumps(summary[key], ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Thresholds")
    lines.append("")
    lines.append(f"- `thresholds`: `{json.dumps(summary['thresholds'], ensure_ascii=False, sort_keys=True)}`")
    lines.append("")
    lines.append("## Interpretation")
    lines.append("")
    lines.append("- Market outcome, execution verification, truth-gap quality, and buy-quality are separate axes.")
    lines.append("- Speculative curve finality is classified as `shadow_onchain_speculative_snapshot_verified`, not finalized proof.")
    lines.append("- `buy_quality_dirty_good` is the conservative positive class for speculative/degraded but usable rows.")
    lines.append("- Rows without Gatekeeper BUY context remain labeled, but are separated in `gatekeeper_context_split`.")
    lines.append("- Phase B remains blocked until feature availability is audited on these labels.")
    return "\n".join(lines).rstrip() + "\n"


def write_markdown(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_markdown(summary), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shadow-onchain-lifecycle", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--summary-output", required=True, type=Path)
    parser.add_argument("--summary-md-output", required=True, type=Path)
    parser.add_argument("--entry-truth-gap-clean-ms", type=int, default=1500)
    parser.add_argument("--entry-truth-gap-degraded-acceptable-ms", type=int, default=10000)
    parser.add_argument("--exit-truth-gap-clean-ms", type=int, default=5000)
    parser.add_argument("--exit-truth-gap-timestop-acceptable-ms", type=int, default=45000)
    parser.add_argument("--exit-truth-gap-other-acceptable-ms", type=int, default=15000)
    parser.add_argument("--entry-drift-acceptable-abs-pct", type=float, default=15.0)
    parser.add_argument("--exit-drift-acceptable-abs-pct", type=float, default=5.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    rows = list(iter_jsonl(args.shadow_onchain_lifecycle))
    labels = build_labels(rows, args)
    write_jsonl(args.output, labels)
    summary = build_summary(labels, source_path=args.shadow_onchain_lifecycle, output_path=args.output, args=args)
    write_json(args.summary_output, summary)
    write_markdown(args.summary_md_output, summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
