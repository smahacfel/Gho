#!/usr/bin/env python3
"""Analyze TimeStop V2 window delta zero-fraction.

The script reads shadow lifecycle JSONL files, extracts `time_stop_v2_window`
rows, and estimates whether the configured `window_ms` is too narrow by
measuring how often each emitted window metric has no movement.

For larger candidate windows it groups consecutive TimeStop V2 windows per
position. This can evaluate multiples of the captured base cadence, e.g. a run
captured at 4000 ms can be evaluated as 4000, 8000, 12000 ms, etc. It cannot
recover evidence for windows smaller than the runtime capture cadence.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


WINDOW_RECORD = "time_stop_v2_window"


@dataclass(frozen=True)
class MetricSpec:
    key: str
    label: str
    unit: str
    kind: str
    zero_epsilon: float
    key_metric: bool = True


@dataclass
class JsonlLoadStats:
    invalid_json_lines: int = 0
    invalid_json_examples: list[dict[str, Any]] | None = None

    def add_invalid(self, path: Path, line_no: int, error: json.JSONDecodeError) -> None:
        self.invalid_json_lines += 1
        if self.invalid_json_examples is None:
            self.invalid_json_examples = []
        if len(self.invalid_json_examples) < 10:
            self.invalid_json_examples.append(
                {
                    "path": str(path),
                    "line_no": line_no,
                    "error": str(error),
                }
            )


METRICS: tuple[MetricSpec, ...] = (
    MetricSpec(
        "time_stop_v2_tx_delta_window",
        "total_tx_delta",
        "tx",
        "count_delta",
        0.0,
    ),
    MetricSpec(
        "time_stop_v2_volume_delta_sol_window",
        "total_volume_sol_delta",
        "SOL",
        "sum_delta",
        1e-12,
    ),
    MetricSpec(
        "time_stop_v2_price_delta_pct_window",
        "price_delta_pct",
        "pct",
        "pct_delta",
        1e-9,
    ),
    MetricSpec(
        "time_stop_v2_mcap_delta_pct_window",
        "mcap_delta_pct",
        "pct",
        "pct_delta",
        1e-9,
    ),
    MetricSpec(
        "time_stop_v2_bonding_delta_pct_window",
        "bonding_progress_delta_pct",
        "pct",
        "pct_delta",
        1e-9,
    ),
    MetricSpec(
        "time_stop_v2_avg_volume_per_tx_sol_window",
        "avg_volume_per_tx_sol_window",
        "SOL/tx",
        "window_value",
        1e-12,
        key_metric=False,
    ),
)

EXPECTED_BUT_NOT_EMITTED: tuple[str, ...] = (
    "total_buyers_delta",
    "unique_buyers_delta",
    "total_unique_buyers_delta",
)


def numeric(value: Any) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if math.isfinite(parsed) else None


def read_jsonl(
    path: Path,
    stats: JsonlLoadStats,
    *,
    strict_json: bool,
) -> Iterable[dict[str, Any]]:
    with path.open("r", encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                if strict_json:
                    raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}") from exc
                stats.add_invalid(path, line_no, exc)
                continue
            if isinstance(row, dict):
                row["_source_path"] = str(path)
                row["_line_no"] = line_no
                yield row


def resolve_scope_paths(root: Path, scope: str) -> list[Path]:
    base = root / "logs" / "shadow_run" / scope
    return [base / "shadow_lifecycle.jsonl", base / "probe_shadow_lifecycle.jsonl"]


def load_window_rows(
    paths: list[Path],
    *,
    strict_json: bool,
) -> tuple[list[dict[str, Any]], JsonlLoadStats]:
    stats = JsonlLoadStats()
    rows: list[dict[str, Any]] = []
    for path in paths:
        if not path.exists():
            continue
        for row in read_jsonl(path, stats, strict_json=strict_json):
            if row.get("record_type") == WINDOW_RECORD:
                rows.append(row)
    rows.sort(
        key=lambda row: (
            str(row.get("position_id") or ""),
            row.get("time_stop_v2_window_index")
            if row.get("time_stop_v2_window_index") is not None
            else 10**12,
            row.get("timestamp_ms") or 0,
            row.get("_line_no") or 0,
        )
    )
    return rows, stats


def infer_base_window_ms(rows: list[dict[str, Any]]) -> int | None:
    by_position: dict[str, list[int]] = defaultdict(list)
    for row in rows:
        position_id = row.get("position_id")
        scheduled = row.get("time_stop_v2_scheduled_check_ms")
        if isinstance(position_id, str) and isinstance(scheduled, int):
            by_position[position_id].append(scheduled)

    diffs: list[int] = []
    for values in by_position.values():
        values = sorted(set(values))
        for previous, current in zip(values, values[1:]):
            diff = current - previous
            if diff > 0:
                diffs.append(diff)
    if not diffs:
        return None
    return int(statistics.median(diffs))


def is_zero(value: float, epsilon: float) -> bool:
    return abs(value) <= epsilon


def chunk_position_rows(rows: list[dict[str, Any]], multiple: int) -> list[list[dict[str, Any]]]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        position_id = row.get("position_id")
        if isinstance(position_id, str):
            grouped[position_id].append(row)

    chunks: list[list[dict[str, Any]]] = []
    for position_rows in grouped.values():
        position_rows.sort(
            key=lambda row: (
                row.get("time_stop_v2_window_index")
                if row.get("time_stop_v2_window_index") is not None
                else 10**12,
                row.get("timestamp_ms") or 0,
                row.get("_line_no") or 0,
            )
        )
        for offset in range(0, len(position_rows), multiple):
            chunk = position_rows[offset : offset + multiple]
            if chunk:
                chunks.append(chunk)
    return chunks


def chunk_metric_values(chunk: list[dict[str, Any]], metric: MetricSpec) -> list[float]:
    values = [numeric(row.get(metric.key)) for row in chunk]
    return [value for value in values if value is not None]


def chunk_is_zero(chunk: list[dict[str, Any]], metric: MetricSpec) -> bool | None:
    values = chunk_metric_values(chunk, metric)
    if not values:
        return None
    return all(is_zero(value, metric.zero_epsilon) for value in values)


def summarize_metric_for_multiple(
    rows: list[dict[str, Any]],
    metric: MetricSpec,
    multiple: int,
) -> dict[str, Any]:
    chunks = chunk_position_rows(rows, multiple)
    total_chunks = len(chunks)
    present_chunks = 0
    zero_chunks = 0
    nonzero_chunks = 0
    abs_values: list[float] = []

    for chunk in chunks:
        values = chunk_metric_values(chunk, metric)
        if not values:
            continue
        present_chunks += 1
        abs_values.extend(abs(value) for value in values)
        if all(is_zero(value, metric.zero_epsilon) for value in values):
            zero_chunks += 1
        else:
            nonzero_chunks += 1

    missing_chunks = total_chunks - present_chunks
    zero_fraction_all = zero_chunks / total_chunks if total_chunks else None
    missing_fraction_all = missing_chunks / total_chunks if total_chunks else None
    zero_or_missing_fraction_all = (
        (zero_chunks + missing_chunks) / total_chunks if total_chunks else None
    )
    zero_fraction_present = zero_chunks / present_chunks if present_chunks else None

    return {
        "metric": metric.label,
        "field": metric.key,
        "kind": metric.kind,
        "unit": metric.unit,
        "candidate_multiple": multiple,
        "candidate_chunks": total_chunks,
        "present_chunks": present_chunks,
        "missing_chunks": missing_chunks,
        "zero_chunks": zero_chunks,
        "nonzero_chunks": nonzero_chunks,
        "zero_fraction_all": zero_fraction_all,
        "zero_fraction_present": zero_fraction_present,
        "missing_fraction_all": missing_fraction_all,
        "zero_or_missing_fraction_all": zero_or_missing_fraction_all,
        "median_abs_observed_value": statistics.median(abs_values) if abs_values else None,
        "mean_abs_observed_value": statistics.fmean(abs_values) if abs_values else None,
        "max_abs_observed_value": max(abs_values) if abs_values else None,
    }


def summarize_statuses(rows: list[dict[str, Any]]) -> dict[str, Any]:
    statuses = Counter(str(row.get("time_stop_v2_status") or "missing") for row in rows)
    subreasons = Counter(str(row.get("time_stop_v2_subreason") or "missing") for row in rows)
    sources = Counter(Path(str(row.get("_source_path") or "")).name for row in rows)
    return {
        "status_counts": dict(statuses),
        "subreason_counts": dict(subreasons),
        "source_file_counts": dict(sources),
    }


def build_recommendations(
    metric_rows: list[dict[str, Any]],
    base_window_ms: int | None,
    acceptable_zero_fraction: float,
    max_missing_fraction: float,
) -> dict[str, Any]:
    by_metric: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in metric_rows:
        by_metric[row["metric"]].append(row)

    metric_recommendations: list[dict[str, Any]] = []
    for metric in METRICS:
        rows = sorted(by_metric.get(metric.label, []), key=lambda row: row["candidate_multiple"])
        selected: dict[str, Any] | None = None
        for row in rows:
            zero_or_missing = row.get("zero_or_missing_fraction_all")
            missing = row.get("missing_fraction_all")
            if zero_or_missing is None or missing is None:
                continue
            if zero_or_missing <= acceptable_zero_fraction and missing <= max_missing_fraction:
                selected = row
                break
        metric_recommendations.append(
            {
                "metric": metric.label,
                "key_metric": metric.key_metric,
                "recommended_multiple": selected.get("candidate_multiple") if selected else None,
                "recommended_window_ms": (
                    selected.get("candidate_multiple") * base_window_ms
                    if selected and base_window_ms
                    else None
                ),
                "meets_threshold": selected is not None,
                "reason": "first_window_meeting_zero_or_missing_and_missing_thresholds"
                if selected
                else "no_candidate_window_met_thresholds",
            }
        )

    key_recs = [row for row in metric_recommendations if row["key_metric"]]
    valid_windows = [
        row["recommended_window_ms"]
        for row in key_recs
        if isinstance(row.get("recommended_window_ms"), int)
    ]
    overall = max(valid_windows) if len(valid_windows) == len(key_recs) else None
    return {
        "metric_recommendations": metric_recommendations,
        "overall_recommended_window_ms_for_key_metrics": overall,
        "overall_rule": (
            "max of per-key-metric first acceptable windows; null means at least one key "
            "metric did not meet thresholds within tested multiples"
        ),
    }


def fraction(value: Any) -> str:
    if value is None:
        return "-"
    return f"{float(value) * 100:.1f}%"


def number(value: Any, digits: int = 3) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        return f"{value:.{digits}f}"
    return str(value)


def render_markdown(report: dict[str, Any]) -> str:
    lines: list[str] = []
    meta = report["metadata"]
    lines.append("# TimeStop V2 Window Zero-Fraction Report")
    lines.append("")
    lines.append(f"- input_paths: `{', '.join(meta['input_paths'])}`")
    lines.append(f"- window_rows: `{meta['window_rows']}`")
    lines.append(f"- positions: `{meta['positions']}`")
    lines.append(f"- invalid_json_lines_skipped: `{meta['invalid_json_lines']}`")
    lines.append(f"- inferred_base_window_ms: `{meta['base_window_ms']}`")
    lines.append(f"- tested_window_ms: `{meta['tested_window_ms']}`")
    lines.append(
        "- thresholds: "
        f"`zero_or_missing <= {meta['acceptable_zero_fraction']:.2f}`, "
        f"`missing <= {meta['max_missing_fraction']:.2f}`"
    )
    lines.append("")
    lines.append("## Status Mix")
    lines.append("")
    lines.append(f"- status_counts: `{report['status_summary']['status_counts']}`")
    lines.append(f"- subreason_counts: `{report['status_summary']['subreason_counts']}`")
    lines.append("")
    lines.append("## Metric Zero-Fraction")
    lines.append("")
    lines.append(
        "| metric | window_ms | chunks | present | zero_all | missing_all | zero_or_missing | median_abs | max_abs |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for row in report["metric_zero_fraction"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row["metric"]),
                    number(row.get("candidate_window_ms"), 0),
                    number(row.get("candidate_chunks"), 0),
                    number(row.get("present_chunks"), 0),
                    fraction(row.get("zero_fraction_all")),
                    fraction(row.get("missing_fraction_all")),
                    fraction(row.get("zero_or_missing_fraction_all")),
                    number(row.get("median_abs_observed_value"), 4),
                    number(row.get("max_abs_observed_value"), 4),
                ]
            )
            + " |"
        )
    lines.append("")
    lines.append("## Recommendation")
    lines.append("")
    rec = report["recommendation"]
    overall = rec["overall_recommended_window_ms_for_key_metrics"]
    lines.append(f"- overall_recommended_window_ms_for_key_metrics: `{overall}`")
    lines.append(f"- rule: `{rec['overall_rule']}`")
    lines.append("")
    lines.append("| metric | key_metric | recommended_window_ms | meets_threshold | reason |")
    lines.append("|---|---:|---:|---:|---|")
    for row in rec["metric_recommendations"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row["metric"]),
                    str(row["key_metric"]),
                    number(row.get("recommended_window_ms"), 0),
                    str(row["meets_threshold"]),
                    str(row["reason"]),
                ]
            )
            + " |"
        )
    if report["unavailable_expected_metrics"]:
        lines.append("")
        lines.append("## Unavailable Expected Metrics")
        lines.append("")
        for metric in report["unavailable_expected_metrics"]:
            lines.append(f"- `{metric}`: not emitted in TimeStop V2 window rows")
    lines.append("")
    lines.append("## Interpretation Notes")
    lines.append("")
    lines.append(
        "- `zero_all` counts numeric zero deltas over all candidate chunks. Missing deltas are not silently treated as zero."
    )
    lines.append(
        "- Invalid JSONL lines are skipped by default and counted in `invalid_json_lines_skipped`; use `--strict-json` to fail fast."
    )
    lines.append(
        "- `zero_or_missing` is the stricter operational noise estimate: zero numeric deltas plus missing metric chunks."
    )
    lines.append(
        "- Candidate windows larger than the runtime cadence are synthetic groups of consecutive per-position windows."
    )
    lines.append(
        "- The script cannot validate a window smaller than the captured runtime cadence."
    )
    return "\n".join(lines)


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    if args.scope:
        paths = resolve_scope_paths(root, args.scope)
    else:
        paths = [path.resolve() for path in args.paths]
    paths = [path for path in paths if path.exists()]
    if not paths:
        raise SystemExit("no input lifecycle paths found")

    rows, load_stats = load_window_rows(paths, strict_json=args.strict_json)
    if not rows:
        raise SystemExit("no time_stop_v2_window rows found")

    base_window_ms = args.window_ms or infer_base_window_ms(rows)
    tested_window_ms: list[int | None] = []
    metric_rows: list[dict[str, Any]] = []
    for multiple in range(1, args.max_multiple + 1):
        candidate_window_ms = multiple * base_window_ms if base_window_ms else None
        tested_window_ms.append(candidate_window_ms)
        for metric in METRICS:
            summary = summarize_metric_for_multiple(rows, metric, multiple)
            summary["candidate_window_ms"] = candidate_window_ms
            metric_rows.append(summary)

    positions = {
        row.get("position_id")
        for row in rows
        if isinstance(row.get("position_id"), str)
    }
    report = {
        "metadata": {
            "input_paths": [str(path) for path in paths],
            "window_rows": len(rows),
            "positions": len(positions),
            "invalid_json_lines": load_stats.invalid_json_lines,
            "invalid_json_examples": load_stats.invalid_json_examples or [],
            "base_window_ms": base_window_ms,
            "tested_window_ms": tested_window_ms,
            "max_multiple": args.max_multiple,
            "acceptable_zero_fraction": args.acceptable_zero_fraction,
            "max_missing_fraction": args.max_missing_fraction,
            "zero_definition": "abs(value) <= per_metric_zero_epsilon",
        },
        "status_summary": summarize_statuses(rows),
        "metric_zero_fraction": metric_rows,
        "recommendation": build_recommendations(
            metric_rows,
            base_window_ms,
            args.acceptable_zero_fraction,
            args.max_missing_fraction,
        ),
        "unavailable_expected_metrics": list(EXPECTED_BUT_NOT_EMITTED),
    }
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="Lifecycle JSONL paths. Ignored when --scope is provided.",
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--scope",
        help="Resolve logs/shadow_run/<scope>/shadow_lifecycle.jsonl and probe_shadow_lifecycle.jsonl.",
    )
    parser.add_argument(
        "--window-ms",
        type=int,
        help="Base TimeStop V2 window cadence. Defaults to median scheduled-check diff.",
    )
    parser.add_argument(
        "--max-multiple",
        type=int,
        default=6,
        help="Evaluate base window and this many consecutive-window multiples.",
    )
    parser.add_argument(
        "--acceptable-zero-fraction",
        type=float,
        default=0.30,
        help="Target maximum zero_or_missing fraction for recommendation.",
    )
    parser.add_argument(
        "--max-missing-fraction",
        type=float,
        default=0.15,
        help="Maximum missing fraction accepted for recommendation.",
    )
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")
    parser.add_argument(
        "--strict-json",
        action="store_true",
        help="Fail on invalid JSONL instead of skipping and reporting bad lines.",
    )
    parser.add_argument("--output-json", type=Path, help="Write JSON report.")
    parser.add_argument("--output-md", type=Path, help="Write markdown report.")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.max_multiple < 1:
        raise SystemExit("--max-multiple must be >= 1")
    report = build_report(args)

    if args.output_json:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        args.output_json.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    if args.output_md:
        args.output_md.parent.mkdir(parents=True, exist_ok=True)
        args.output_md.write_text(render_markdown(report) + "\n", encoding="utf-8")

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_markdown(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
