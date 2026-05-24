#!/usr/bin/env python3
"""P3.7-R17A replay-readiness failure breakdown audit.

This is an offline audit for R17 decision logs. It does not evaluate policy and
does not infer buy quality. Its only job is to explain why
gatekeeper_v2_replay_ready_temporal is false for rows that otherwise carry
decision_eval_snapshots.
"""

from __future__ import annotations

import argparse
import json
import statistics
from collections import Counter
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python 3.10 fallback only
    import tomli as tomllib  # type: ignore


SCHEMA_VERSION = 1
DEFAULT_CONFIG = Path("configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml")
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_R17A_REPLAY_READINESS_FAILURE_BREAKDOWN_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_R17A_REPLAY_READINESS_FAILURE_BREAKDOWN_20260524.md"
)
REQUIRED_TARGETS_MS = [2000, 5000, 7000]
SNAPSHOT_REQUIRED_FIELDS = [
    "gatekeeper_gate_trace",
    "phase_pass_vector",
    "pdd_diagnostics",
    "prosperity_diagnostics",
    "hhi_diversity_diagnostics",
]


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            raw = line.strip()
            if not raw:
                continue
            row = json.loads(raw)
            if isinstance(row, dict):
                row["_line_number"] = line_number
                rows.append(row)
    return rows


def get_path(data: dict[str, Any], keys: list[str], default: Any = None) -> Any:
    value: Any = data
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            return default
        value = value[key]
    return value


def resolve_path(raw: str, base: Path) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (base / path).resolve()


def derive_decision_log_from_config(config_path: Path) -> Path:
    config = load_toml(config_path)
    raw_dir = get_path(config, ["oracle", "decision_log_path"])
    if not raw_dir:
        raise SystemExit("decision log not provided and oracle.decision_log_path missing")
    decision_dir = resolve_path(str(raw_dir), config_path.parent)
    candidates = sorted(
        decision_dir.glob("**/gatekeeper_v2_decisions.jsonl"),
        key=lambda item: item.stat().st_mtime if item.exists() else 0,
        reverse=True,
    )
    if not candidates:
        raise SystemExit(f"no gatekeeper_v2_decisions.jsonl found under {decision_dir}")
    return candidates[0]


def present(value: Any) -> bool:
    return value is not None and value != "" and value != []


def as_bool(value: Any) -> bool:
    return value is True


def explicit_missing_fields(row: dict[str, Any]) -> list[str]:
    value = row.get("gatekeeper_v2_replay_missing_fields")
    if not isinstance(value, list):
        return []
    return [str(item) for item in value if str(item)]


def snapshots(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = row.get("decision_eval_snapshots")
    if not isinstance(raw, list):
        return []
    return [item for item in raw if isinstance(item, dict)]


def snapshot_target(snapshot: dict[str, Any]) -> int | None:
    value = snapshot.get("snapshot_target_ms")
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def snapshot_actual_elapsed(snapshot: dict[str, Any]) -> int | None:
    value = snapshot.get("snapshot_actual_elapsed_ms", snapshot.get("elapsed_ms"))
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def snapshot_drift(snapshot: dict[str, Any]) -> int | None:
    value = snapshot.get("snapshot_drift_ms")
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def snapshot_has_terminal(snapshot: dict[str, Any], terminal_target_ms: int | None) -> bool:
    if snapshot.get("snapshot_source") == "terminal":
        return True
    target = snapshot_target(snapshot)
    return terminal_target_ms is not None and target == terminal_target_ms


def snapshot_by_target(items: list[dict[str, Any]]) -> dict[int, dict[str, Any]]:
    result: dict[int, dict[str, Any]] = {}
    for snapshot in items:
        target = snapshot_target(snapshot)
        if target is not None and target not in result:
            result[target] = snapshot
    return result


def terminal_snapshot(items: list[dict[str, Any]], terminal_target_ms: int | None) -> dict[str, Any] | None:
    for snapshot in items:
        if snapshot_has_terminal(snapshot, terminal_target_ms):
            return snapshot
    return None


def observation_duration_ms(row: dict[str, Any], items: list[dict[str, Any]]) -> int | None:
    for key in ["observation_duration_ms", "observation_window_ms", "observed_window_ms"]:
        value = row.get(key)
        if value is not None:
            try:
                return int(value)
            except (TypeError, ValueError):
                pass
    terminal = terminal_snapshot(items, None)
    if terminal:
        return snapshot_actual_elapsed(terminal)
    actuals = [snapshot_actual_elapsed(item) for item in items]
    actuals = [value for value in actuals if value is not None]
    return max(actuals) if actuals else None


def verdict_type(row: dict[str, Any]) -> str:
    for key in ["verdict_type", "legacy_live_verdict_type", "v3_shadow_verdict", "verdict"]:
        value = row.get(key)
        if value:
            return str(value)
    return "unknown"


def reason_code(row: dict[str, Any]) -> str:
    for key in ["reason_code", "decision_reason", "gatekeeper_first_kill_reason"]:
        value = row.get(key)
        if value:
            return str(value)
    return verdict_type(row)


def classify_missing_target(row: dict[str, Any], target_ms: int, duration_ms: int | None) -> tuple[str, str]:
    reason = reason_code(row)
    verdict = verdict_type(row)
    if duration_ms is not None and duration_ms < target_ms:
        return "early_close_before_targets", "natural_not_applicable"
    if reason == "TIMEOUT_PHASE1_NO_DATA" or verdict == "TIMEOUT_PHASE1_NO_DATA":
        return "timeout_no_data_before_targets", "natural_not_applicable"
    if reason == "TIMEOUT_PHASE1_INSUFFICIENT" or verdict == "TIMEOUT_PHASE1_INSUFFICIENT":
        return "insufficient_sample_before_target", "natural_not_applicable"
    return f"missing_{target_ms}_snapshot", "runtime_emission_bug"


def classify_row(
    row: dict[str, Any],
    required_targets_ms: list[int],
    terminal_target_ms: int | None,
    max_drift_ms: int,
) -> dict[str, Any]:
    items = snapshots(row)
    by_target = snapshot_by_target(items)
    terminal = terminal_snapshot(items, terminal_target_ms)
    duration_ms = observation_duration_ms(row, items)
    explicit_missing = explicit_missing_fields(row)
    temporal_ready = as_bool(row.get("gatekeeper_v2_replay_ready_temporal"))

    has_target = {str(target): target in by_target for target in required_targets_ms}
    target_drift = {
        str(target): snapshot_drift(by_target[target])
        for target in required_targets_ms
        if target in by_target
    }
    target_actual_elapsed = {
        str(target): snapshot_actual_elapsed(by_target[target])
        for target in required_targets_ms
        if target in by_target
    }

    missing_reasons: list[str] = []
    root_cause_classes: list[str] = []
    for target in required_targets_ms:
        if target not in by_target:
            reason, root_class = classify_missing_target(row, target, duration_ms)
            missing_reasons.append(reason)
            root_cause_classes.append(root_class)

    if not items:
        missing_reasons.append("missing_decision_eval_snapshots")
        root_cause_classes.append("runtime_emission_bug")
    if terminal is None:
        missing_reasons.append("missing_terminal_snapshot")
        root_cause_classes.append("runtime_emission_bug")

    payload_missing_fields: Counter[str] = Counter()
    for snapshot in items:
        for field in SNAPSHOT_REQUIRED_FIELDS:
            if field not in snapshot or snapshot.get(field) is None:
                payload_missing_fields[field] += 1
    if payload_missing_fields:
        for field in sorted(payload_missing_fields):
            missing_reasons.append(f"missing_{field}_in_snapshot")
        root_cause_classes.append("payload_missing")

    drift_exceeded_targets: list[int] = []
    for target, snapshot in by_target.items():
        drift = snapshot_drift(snapshot)
        if drift is not None and drift > max_drift_ms:
            drift_exceeded_targets.append(target)
    if drift_exceeded_targets and not missing_reasons and not temporal_ready:
        missing_reasons.append("snapshot_drift_exceeded")
        root_cause_classes.append("contract_too_strict")

    if temporal_ready:
        primary_reason = "ready"
        root_cause_class = "ready"
    elif missing_reasons:
        priority = [
            ("payload_missing", "missing_gatekeeper_gate_trace_in_snapshot"),
            ("payload_missing", "missing_phase_pass_vector_in_snapshot"),
            ("payload_missing", "missing_pdd_diagnostics_in_snapshot"),
            ("payload_missing", "missing_prosperity_diagnostics_in_snapshot"),
            ("payload_missing", "missing_hhi_diversity_diagnostics_in_snapshot"),
            ("runtime_emission_bug", "missing_terminal_snapshot"),
            ("runtime_emission_bug", "missing_decision_eval_snapshots"),
            ("natural_not_applicable", "early_close_before_targets"),
            ("natural_not_applicable", "timeout_no_data_before_targets"),
            ("natural_not_applicable", "insufficient_sample_before_target"),
            ("contract_too_strict", "snapshot_drift_exceeded"),
        ]
        primary_reason = missing_reasons[0]
        root_cause_class = root_cause_classes[0] if root_cause_classes else "unknown"
        for wanted_class, wanted_reason in priority:
            if wanted_reason in missing_reasons:
                primary_reason = wanted_reason
                root_cause_class = wanted_class
                break
        if (
            primary_reason.startswith("missing_")
            and primary_reason.endswith("_snapshot")
            and primary_reason.removeprefix("missing_").removesuffix("_snapshot").isdigit()
        ):
            root_cause_class = "runtime_emission_bug"
    else:
        primary_reason = "unknown_temporal_readiness_gap"
        root_cause_class = "unknown"

    return {
        "line_number": row.get("_line_number"),
        "ab_record_id": row.get("ab_record_id"),
        "verdict_type": verdict_type(row),
        "reason_code": reason_code(row),
        "observation_duration_ms": duration_ms,
        "gatekeeper_v2_replay_ready_non_temporal": as_bool(row.get("gatekeeper_v2_replay_ready_non_temporal")),
        "gatekeeper_v2_replay_ready_temporal": temporal_ready,
        "gatekeeper_v2_replay_missing_fields": explicit_missing,
        "has_decision_eval_snapshots": bool(items),
        "snapshot_count": len(items),
        "has_terminal_snapshot": terminal is not None,
        "has_required_target_snapshot": has_target,
        "snapshot_drift_ms_by_target": target_drift,
        "snapshot_actual_elapsed_ms_by_target": target_actual_elapsed,
        "payload_missing_field_counts": dict(sorted(payload_missing_fields.items())),
        "drift_exceeded_targets": sorted(drift_exceeded_targets),
        "temporal_readiness_reason": primary_reason,
        "temporal_readiness_root_cause_class": root_cause_class,
    }


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def stats(values: list[int]) -> dict[str, float | int | None]:
    if not values:
        return {"count": 0, "min": None, "max": None, "mean": None, "p50": None, "p90": None}
    ordered = sorted(values)
    def percentile(pct: float) -> int:
        idx = int(round((len(ordered) - 1) * pct))
        return ordered[idx]
    return {
        "count": len(values),
        "min": min(values),
        "max": max(values),
        "mean": round(statistics.mean(values), 2),
        "p50": percentile(0.50),
        "p90": percentile(0.90),
    }


def final_decision(summary: dict[str, Any]) -> tuple[str, str]:
    if summary["unknown_rows"] > 0:
        return "R17A_AUDIT_GAP", "repair_r17a_unknown_classification"
    if summary["runtime_emission_bug_rows"] > 0 or summary["payload_missing_rows"] > 0:
        return "R17B_SNAPSHOT_EMISSION_FIX_REQUIRED", "repair_snapshot_emission_or_payload"
    return "SNAPSHOT_SIDE_PASS_GO_E1", "start_p3_7_e1_pumpfun_executable_route_support_matrix"


def build_report(
    decision_log: Path,
    config_path: Path | None,
    required_targets_ms: list[int],
    terminal_target_ms: int | None,
    max_drift_ms: int,
) -> dict[str, Any]:
    rows = read_jsonl(decision_log)
    row_reports = [
        classify_row(row, required_targets_ms, terminal_target_ms, max_drift_ms)
        for row in rows
    ]
    reason_counts = Counter(item["temporal_readiness_reason"] for item in row_reports)
    root_counts = Counter(item["temporal_readiness_root_cause_class"] for item in row_reports)
    not_ready_reports = [
        item for item in row_reports if not item["gatekeeper_v2_replay_ready_temporal"]
    ]
    not_ready_reason_counts = Counter(
        item["temporal_readiness_reason"] for item in not_ready_reports
    )
    not_ready_root_counts = Counter(
        item["temporal_readiness_root_cause_class"] for item in not_ready_reports
    )
    verdict_counts = Counter(item["verdict_type"] for item in row_reports)
    non_temporal_counts = Counter(str(item["gatekeeper_v2_replay_ready_non_temporal"]).lower() for item in row_reports)
    temporal_counts = Counter(str(item["gatekeeper_v2_replay_ready_temporal"]).lower() for item in row_reports)
    snapshot_count_values = [int(item["snapshot_count"]) for item in row_reports]
    drift_values: list[int] = []
    target_coverage: Counter[str] = Counter()
    for item in row_reports:
        for target, present_value in item["has_required_target_snapshot"].items():
            if present_value:
                target_coverage[target] += 1
        for drift in item["snapshot_drift_ms_by_target"].values():
            if drift is not None:
                drift_values.append(int(drift))
    drift_exceeded_rows = sum(1 for item in row_reports if item["drift_exceeded_targets"])
    terminal_present_rows = sum(1 for item in row_reports if item["has_terminal_snapshot"])
    terminal_only_rows = sum(
        1
        for item in row_reports
        if item["has_terminal_snapshot"] and int(item["snapshot_count"]) == 1
    )
    missing_payload_row_counts: Counter[str] = Counter()
    for item in row_reports:
        missing_fields = item["payload_missing_field_counts"]
        for field in SNAPSHOT_REQUIRED_FIELDS:
            if missing_fields.get(field, 0) > 0:
                missing_payload_row_counts[field] += 1

    total_rows = len(row_reports)
    temporal_ready_rows = temporal_counts.get("true", 0)
    temporal_not_ready_rows = total_rows - temporal_ready_rows
    summary = {
        "total_rows": total_rows,
        "temporal_ready_rows": temporal_ready_rows,
        "temporal_not_ready_rows": temporal_not_ready_rows,
        "non_temporal_ready_counts": counter_dict(non_temporal_counts),
        "temporal_ready_counts": counter_dict(temporal_counts),
        "reason_counts": counter_dict(reason_counts),
        "root_cause_class_counts": counter_dict(root_counts),
        "temporal_not_ready_reason_counts": counter_dict(not_ready_reason_counts),
        "temporal_not_ready_root_cause_class_counts": counter_dict(not_ready_root_counts),
        "runtime_emission_bug_rows": not_ready_root_counts.get("runtime_emission_bug", 0),
        "natural_not_applicable_rows": not_ready_root_counts.get("natural_not_applicable", 0),
        "contract_too_strict_rows": not_ready_root_counts.get("contract_too_strict", 0),
        "payload_missing_rows": not_ready_root_counts.get("payload_missing", 0),
        "payload_gap_rows": not_ready_root_counts.get("payload_missing", 0),
        "unknown_rows": not_ready_root_counts.get("unknown", 0),
        "unknown_temporal_readiness_gap_rows": not_ready_reason_counts.get(
            "unknown_temporal_readiness_gap", 0
        ),
        "snapshot_2000_present_rows": target_coverage.get("2000", 0),
        "snapshot_5000_present_rows": target_coverage.get("5000", 0),
        "snapshot_7000_present_rows": target_coverage.get("7000", 0),
        "snapshot_terminal_present_rows": terminal_present_rows,
        "missing_2000_snapshot_rows": total_rows - target_coverage.get("2000", 0),
        "missing_5000_snapshot_rows": total_rows - target_coverage.get("5000", 0),
        "missing_7000_snapshot_rows": total_rows - target_coverage.get("7000", 0),
        "missing_terminal_snapshot_rows": total_rows - terminal_present_rows,
        "snapshot_drift_exceeded_rows": drift_exceeded_rows,
        "missing_gate_trace_rows": missing_payload_row_counts.get("gatekeeper_gate_trace", 0),
        "missing_phase_vector_rows": missing_payload_row_counts.get("phase_pass_vector", 0),
        "missing_pdd_diagnostics_rows": missing_payload_row_counts.get("pdd_diagnostics", 0),
        "missing_prosperity_diagnostics_rows": missing_payload_row_counts.get("prosperity_diagnostics", 0),
        "missing_hhi_diagnostics_rows": missing_payload_row_counts.get("hhi_diversity_diagnostics", 0),
        "early_close_before_target_rows": not_ready_reason_counts.get("early_close_before_targets", 0),
        "timeout_no_data_before_target_rows": not_ready_reason_counts.get("timeout_no_data_before_targets", 0),
        "insufficient_sample_before_target_rows": not_ready_reason_counts.get("insufficient_sample_before_target", 0),
        "terminal_only_rows": terminal_only_rows,
        "snapshot_target_coverage": dict(sorted(target_coverage.items(), key=lambda item: int(item[0]))),
        "snapshot_count_stats": stats(snapshot_count_values),
        "snapshot_drift_stats_ms": stats(drift_values),
        "verdict_counts": counter_dict(verdict_counts),
    }
    decision, next_path = final_decision(summary)
    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-R17A Replay-Readiness Failure Breakdown Audit",
        "config_path": str(config_path) if config_path else None,
        "decision_log": str(decision_log),
        "required_targets_ms": required_targets_ms,
        "terminal_target_ms": terminal_target_ms,
        "max_drift_ms": max_drift_ms,
        "summary": summary,
        "final_decision": decision,
        "recommended_next_path": next_path,
        "rows": row_reports,
    }


def markdown_table(rows: list[list[Any]]) -> str:
    if not rows:
        return ""
    header = rows[0]
    body = rows[1:]
    lines = [
        "| " + " | ".join(str(item) for item in header) + " |",
        "| " + " | ".join("---" for _ in header) + " |",
    ]
    for row in body:
        lines.append("| " + " | ".join(str(item) for item in row) + " |")
    return "\n".join(lines)


def write_markdown(report: dict[str, Any], output: Path) -> None:
    summary = report["summary"]
    reason_rows = [["Reason", "Rows"]] + [
        [key, value] for key, value in summary["temporal_not_ready_reason_counts"].items()
    ]
    root_rows = [["Root cause class", "Rows"]] + [
        [key, value] for key, value in summary["temporal_not_ready_root_cause_class_counts"].items()
    ]
    target_rows = [["Target ms", "Rows with snapshot"]] + [
        [key, value] for key, value in summary["snapshot_target_coverage"].items()
    ]
    verdict_rows = [["Verdict", "Rows"]] + [
        [key, value] for key, value in summary["verdict_counts"].items()
    ]

    text = f"""# RAPORT P3.7 R17A Replay-Readiness Failure Breakdown - 2026-05-24

## Verdict

Final decision: **{report["final_decision"]}**

Recommended next path: `{report["recommended_next_path"]}`

This audit is offline only. It does not change thresholds, policy, route support, runtime behavior, P2/live, or Phase B.

## Inputs

- decision log: `{report["decision_log"]}`
- config: `{report["config_path"]}`
- required temporal targets: `{report["required_targets_ms"]}`
- terminal target: `{report["terminal_target_ms"]}`
- max drift threshold: `{report["max_drift_ms"]} ms`

## Summary

- total rows: `{summary["total_rows"]}`
- temporal ready rows: `{summary["temporal_ready_rows"]}`
- temporal not-ready rows: `{summary["temporal_not_ready_rows"]}`
- runtime emission bug rows: `{summary["runtime_emission_bug_rows"]}`
- natural not-applicable rows: `{summary["natural_not_applicable_rows"]}`
- contract too strict rows: `{summary["contract_too_strict_rows"]}`
- payload gap rows: `{summary["payload_gap_rows"]}`
- unknown rows: `{summary["unknown_rows"]}`
- missing 2000 snapshot rows: `{summary["missing_2000_snapshot_rows"]}`
- missing 5000 snapshot rows: `{summary["missing_5000_snapshot_rows"]}`
- missing 7000 snapshot rows: `{summary["missing_7000_snapshot_rows"]}`
- missing terminal snapshot rows: `{summary["missing_terminal_snapshot_rows"]}`
- missing gate trace rows: `{summary["missing_gate_trace_rows"]}`
- missing phase vector rows: `{summary["missing_phase_vector_rows"]}`
- missing PDD diagnostics rows: `{summary["missing_pdd_diagnostics_rows"]}`
- missing prosperity diagnostics rows: `{summary["missing_prosperity_diagnostics_rows"]}`
- missing HHI diagnostics rows: `{summary["missing_hhi_diagnostics_rows"]}`
- early close before target rows: `{summary["early_close_before_target_rows"]}`
- timeout/no-data before target rows: `{summary["timeout_no_data_before_target_rows"]}`
- insufficient sample before target rows: `{summary["insufficient_sample_before_target_rows"]}`
- terminal-only rows: `{summary["terminal_only_rows"]}`
- unknown temporal readiness gap rows: `{summary["unknown_temporal_readiness_gap_rows"]}`
- rows with snapshot drift above threshold: `{summary["snapshot_drift_exceeded_rows"]}`

## Reason Counts

Counts below are for `gatekeeper_v2_replay_ready_temporal=false` rows.

{markdown_table(reason_rows)}

## Root Cause Classes

Counts below are for `gatekeeper_v2_replay_ready_temporal=false` rows.

{markdown_table(root_rows)}

## Snapshot Target Coverage

{markdown_table(target_rows)}

Snapshot count stats:

```json
{json.dumps(summary["snapshot_count_stats"], indent=2, sort_keys=True)}
```

Snapshot drift stats:

```json
{json.dumps(summary["snapshot_drift_stats_ms"], indent=2, sort_keys=True)}
```

## Verdict Counts

{markdown_table(verdict_rows)}

## Interpretation

`temporal_ready=false` is not automatically a runtime bug. R17A separates:

- missing terminal/payload fields as runtime or payload problems;
- early terminal and no-data rows as natural non-applicability;
- sparse/no-data checkpoint rows as natural non-applicability unless payload or terminal data is missing.

L2D2 remains blocked unless a future manifest has both replay-ready inputs and an executable lifecycle-labeled denominator.
"""
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(text.rstrip() + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decision-log", type=Path)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--required-target-ms", type=int, nargs="*", default=REQUIRED_TARGETS_MS)
    parser.add_argument("--terminal-target-ms", type=int, default=10_000)
    parser.add_argument("--max-drift-ms", type=int, default=2_000)
    parser.add_argument("--json", action="store_true", help="Print report JSON to stdout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    decision_log = args.decision_log or derive_decision_log_from_config(args.config)
    report = build_report(
        decision_log=decision_log,
        config_path=args.config,
        required_targets_ms=sorted(set(args.required_target_ms)),
        terminal_target_ms=args.terminal_target_ms,
        max_drift_ms=args.max_drift_ms,
    )
    args.output_json.parent.mkdir(parents=True, exist_ok=True)
    args.output_json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    write_markdown(report, args.output_md)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
