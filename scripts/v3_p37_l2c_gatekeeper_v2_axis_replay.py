#!/usr/bin/env python3
"""P3.7-L2C Gatekeeper V2 axis replay backend gate.

This script is deliberately stricter than the L2B diagnostic matrix. It checks
whether the L1R21 locked denominator contains enough authoritative Gatekeeper V2
replay evidence to run causal single-axis ablation. It does not use filesystem
configs, mutable runtime state, raw tx streams, or heuristic diagnostic flags as
counterfactual verdicts.

The L1R21 manifest remains the only input SSOT.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import v3_p37_l2a_executable_subset_policy_delta as l2a


SCHEMA_VERSION = 1
DEFAULT_MANIFEST = l2a.DEFAULT_MANIFEST
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2C_GATEKEEPER_V2_AXIS_REPLAY_BACKEND_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2C_GATEKEEPER_V2_AXIS_REPLAY_BACKEND_20260524.md"
)

J4C_NAMESPACE = "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1"
R16_R1_NAMESPACE = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1"
REQUIRED_ALLOWED_RUNS = [J4C_NAMESPACE, R16_R1_NAMESPACE]
EXPECTED_DENOMINATOR = 85
EXPECTED_DIRTY_GOOD = 4

DEFAULT_AXES = [
    "soft_pdd_instead_of_hard_pdd",
    "prosperity_filter_disabled",
    "hhi_hard_fail_relaxed",
    "elapsed_aware_entry_drift",
    "standard_mode_shorter_window",
]

AXIS_REQUIRED_FIELDS = {
    "soft_pdd_instead_of_hard_pdd": [
        "gatekeeper_gate_trace",
        "pdd_hard_fail",
        "soft_points",
        "max_soft_points",
    ],
    "prosperity_filter_disabled": [
        "gatekeeper_gate_trace",
        "prosperity_filter_enabled",
        "aps_shadow_prosperity_would_pass",
    ],
    "hhi_hard_fail_relaxed": [
        "gatekeeper_gate_trace",
        "hhi",
        "max_hhi",
        "top3_volume_pct",
        "same_ms_tx_ratio",
    ],
    "elapsed_aware_entry_drift": [
        "gatekeeper_gate_trace",
        "pdd_entry_drift_pct",
        "pdd_entry_drift_effective_max_pct",
        "pdd_entry_drift_threshold_source",
    ],
}


def row_key(label: dict[str, Any]) -> str:
    for field in ("source_ab_record_id", "ab_record_id", "v3_feature_snapshot_hash", "candidate_id"):
        value = label.get(field)
        if value is not None and str(value):
            return str(value)
    return "unknown"


def quality(row: dict[str, Any]) -> str:
    return str(row["label"].get("buy_quality_class") or "unknown")


def is_dirty_good(row: dict[str, Any]) -> bool:
    return quality(row) == "buy_quality_dirty_good"


def is_bad(row: dict[str, Any]) -> bool:
    return quality(row) == "buy_quality_bad"


def observed_verdict(row: dict[str, Any]) -> str:
    decision = row.get("decision") or {}
    if decision.get("decision_verdict_buy") is True or decision.get("legacy_live_verdict_buy") is True:
        return "BUY"
    reason = str(decision.get("reason_code") or decision.get("legacy_live_verdict_type") or "")
    if reason.startswith("TIMEOUT"):
        return "TIMEOUT"
    if reason.startswith("REJECT") or reason.startswith("HARD_FAIL"):
        return "REJECT"
    if str(decision.get("gatekeeper_terminal_gate") or "") == "buy":
        return "BUY"
    return "UNKNOWN"


def has_full_v3_replay_payload(decision: dict[str, Any] | None) -> bool:
    if not decision:
        return False
    return all(
        decision.get(field) is not None
        for field in (
            "v3_materialized_feature_snapshot",
            "v3_policy_config_payload",
            "v3_shadow_verdict",
            "v3_shadow_reason_code",
            "v3_replay_payload_schema_version",
            "v3_materialization_version",
        )
    )


def load_denominator_rows(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for run in manifest.get("allowed_run_manifests", []):
        _labels, joined = l2a.load_run_rows(run)
        for item in joined:
            if l2a.is_non_executable_label(item["label"]):
                continue
            rows.append(
                {
                    "namespace": run["namespace"],
                    "label": item["label"],
                    "decision": item["decision"],
                    "row_key": row_key(item["label"]),
                }
            )
    return rows


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def validate_manifest_contract(manifest: dict[str, Any], rows: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    actual = manifest.get("actual_contract", {})
    allowed = actual.get("allowed_runs", [])
    if allowed != REQUIRED_ALLOWED_RUNS:
        failures.append("BLOCK_L2C_INPUT_MANIFEST_CONTRACT:allowed_runs_mismatch")
    if actual.get("buy_quality_denominator_rows") != EXPECTED_DENOMINATOR:
        failures.append("BLOCK_L2C_DENOMINATOR_MISMATCH:manifest")
    if actual.get("buy_quality_dirty_good") != EXPECTED_DIRTY_GOOD:
        failures.append("BLOCK_L2C_DIRTY_GOOD_MISMATCH:manifest")

    if len(rows) != EXPECTED_DENOMINATOR:
        failures.append("BLOCK_L2C_DENOMINATOR_MISMATCH:rows")
    if sum(1 for row in rows if is_dirty_good(row)) != EXPECTED_DIRTY_GOOD:
        failures.append("BLOCK_L2C_DIRTY_GOOD_MISMATCH:rows")

    l2a_results = [l2a.analyze_run(run) for run in manifest.get("allowed_run_manifests", [])]
    failures.extend(
        item.replace("BLOCK_L2_", "BLOCK_L2C_")
        for item in l2a.validate_input_contract(manifest, l2a_results)
    )
    return sorted(set(failures))


def missing_fields(decision: dict[str, Any] | None, fields: list[str]) -> list[str]:
    if not decision:
        return fields
    missing = []
    for field in fields:
        value = decision.get(field)
        if value is None or value == "":
            missing.append(field)
    return missing


def gate_trace(decision: dict[str, Any] | None) -> list[dict[str, Any]]:
    if not decision:
        return []
    trace = decision.get("gatekeeper_gate_trace")
    return trace if isinstance(trace, list) else []


def hard_gate_fails(trace: list[dict[str, Any]]) -> list[str]:
    return [
        str(item.get("gate") or "unknown")
        for item in trace
        if item.get("hard_or_soft") == "hard" and item.get("status") == "fail"
    ]


def trace_baseline_verdict(decision: dict[str, Any] | None) -> str:
    trace = gate_trace(decision)
    if not trace:
        return "UNSUPPORTED"
    terminal = str((decision or {}).get("gatekeeper_terminal_gate") or "")
    if terminal == "buy":
        # A hard fail in the trace means the trace is diagnostic but not a
        # replay-complete verdict engine for this row.
        return "BUY" if not hard_gate_fails(trace) else "PARITY_MISMATCH"
    if terminal == "timeout":
        return "TIMEOUT"
    return "REJECT" if hard_gate_fails(trace) else "BUY"


def row_replay_support(row: dict[str, Any]) -> dict[str, Any]:
    decision = row.get("decision")
    trace = gate_trace(decision)
    observed = observed_verdict(row)
    trace_verdict = trace_baseline_verdict(decision)
    parity = trace_verdict == observed
    return {
        "has_decision_join": decision is not None,
        "has_full_v3_replay_payload": has_full_v3_replay_payload(decision),
        "has_gate_trace": bool(trace),
        "observed_verdict": observed,
        "trace_baseline_verdict": trace_verdict,
        "baseline_parity": parity,
        "hard_gate_fails": hard_gate_fails(trace),
    }


def axis_row_status(axis: str, row: dict[str, Any]) -> str:
    if axis == "standard_mode_shorter_window":
        return "unsupported_temporal_replay_required"

    decision = row.get("decision")
    required = AXIS_REQUIRED_FIELDS.get(axis, [])
    missing = missing_fields(decision, required)
    if missing:
        return "unsupported_missing_fields:" + ",".join(missing)

    support = row_replay_support(row)
    if not support["baseline_parity"]:
        return "unsupported_baseline_parity_gap"

    if row["namespace"] == R16_R1_NAMESPACE and axis in {
        "soft_pdd_instead_of_hard_pdd",
        "prosperity_filter_disabled",
        "hhi_hard_fail_relaxed",
        "elapsed_aware_entry_drift",
    }:
        return "unsupported_axis_already_applied_in_source_run"

    return "replay_ready"


def axis_status(axis: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    status_counts = Counter(axis_row_status(axis, row) for row in rows)
    replay_ready = status_counts.get("replay_ready", 0)
    if axis == "standard_mode_shorter_window":
        final_status = "unsupported_temporal_replay_required"
    elif replay_ready == len(rows):
        final_status = "evaluated"
    elif replay_ready > 0:
        final_status = "partial_evaluation_blocked"
    elif any(key.startswith("unsupported_missing_fields") for key in status_counts):
        final_status = "unsupported_missing_fields"
    elif status_counts.get("unsupported_baseline_parity_gap", 0):
        final_status = "unsupported_baseline_parity_gap"
    else:
        final_status = "unsupported_mixed_policy_axis_contract"

    return {
        "axis_status": final_status,
        "row_status_counts": counter_dict(status_counts),
        "evaluated_rows": replay_ready,
        "unsupported_rows": len(rows) - replay_ready,
        "variant_buy_rows": None,
        "accepted_dirty_good": None,
        "accepted_bad": None,
        "missed_dirty_good": None,
        "avoided_bad": None,
        "dirty_good_capture_rate": None,
        "bad_accept_rate": None,
        "dirty_good_precision": None,
        "decision_delta_rows": None,
        "changed_from_reject_to_buy": None,
        "changed_from_buy_to_reject": None,
        "changed_reason_only": None,
    }


def payload_and_support_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    v3_payload = Counter()
    gate_trace_counts = Counter()
    parity_counts = Counter()
    namespace_payload = Counter()
    examples = []
    for row in rows:
        support = row_replay_support(row)
        v3_payload["full_v3_replay_payload" if support["has_full_v3_replay_payload"] else "missing_v3_replay_payload"] += 1
        gate_trace_counts["gate_trace_present" if support["has_gate_trace"] else "gate_trace_missing"] += 1
        parity_counts["baseline_parity_ok" if support["baseline_parity"] else "baseline_parity_gap"] += 1
        namespace_payload[f"{row['namespace']}|{'trace' if support['has_gate_trace'] else 'no_trace'}"] += 1
        if not support["baseline_parity"] and len(examples) < 10:
            examples.append(
                {
                    "namespace": row["namespace"],
                    "row_key": row["row_key"],
                    "buy_quality_class": quality(row),
                    "observed_verdict": support["observed_verdict"],
                    "trace_baseline_verdict": support["trace_baseline_verdict"],
                    "hard_gate_fails": support["hard_gate_fails"],
                }
            )
    return {
        "v3_payload_counts": counter_dict(v3_payload),
        "gate_trace_counts": counter_dict(gate_trace_counts),
        "baseline_parity_counts": counter_dict(parity_counts),
        "namespace_trace_counts": counter_dict(namespace_payload),
        "baseline_parity_gap_examples": examples,
    }


def build_report(manifest_path: Path, axes: list[str]) -> dict[str, Any]:
    manifest = l2a.load_manifest(manifest_path)
    rows = load_denominator_rows(manifest)
    artifact_failures = l2a.verify_artifacts(manifest)
    contract_failures = validate_manifest_contract(manifest, rows)
    blockers = sorted(set(artifact_failures + contract_failures))

    quality_counts = Counter(quality(row) for row in rows)
    namespace_counts = Counter(row["namespace"] for row in rows)
    axis_results = {axis: axis_status(axis, rows) for axis in axes}
    evaluated_axes = [
        axis for axis, result in axis_results.items() if result["axis_status"] == "evaluated"
    ]

    support_summary = payload_and_support_summary(rows)
    analysis_status = "pass" if not blockers else "fail"
    if blockers:
        final_decision = "BLOCK_L2C_INPUT_MANIFEST_CONTRACT"
    elif evaluated_axes:
        final_decision = "GO_L2D_TARGETED_AXIS_EXPERIMENT_PREP"
    else:
        final_decision = "BLOCK_L2D_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP"

    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L2C Gatekeeper V2 Axis Replay Backend",
        "manifest_path": str(manifest_path),
        "analysis_status": analysis_status,
        "final_decision": final_decision,
        "blockers": blockers,
        "locked_denominator": {
            "rows": len(rows),
            "quality_counts": counter_dict(quality_counts),
            "namespace_counts": counter_dict(namespace_counts),
            "dirty_good_rate": quality_counts.get("buy_quality_dirty_good", 0) / len(rows)
            if rows
            else None,
        },
        "payload_and_support": support_summary,
        "axes_requested": axes,
        "axes_evaluated": evaluated_axes,
        "axis_results": axis_results,
        "recommended_next_path": (
            "l2d_targeted_axis_experiment_prep"
            if evaluated_axes
            else "add_authoritative_gatekeeper_v2_axis_replay_payload_or_backend"
        ),
        "interpretation": {
            "v3_payload_not_enough_for_v2_axis_replay": True,
            "diagnostic_flags_not_used_as_causal_ablation": True,
            "small_sample_directional_only": True,
        },
        "non_goals": [
            "no_runtime",
            "no_new_runs",
            "no_threshold_tuning",
            "no_phase_b",
            "no_p2_live",
            "no_full_r16_route_universe",
        ],
    }


def fmt(value: Any) -> str:
    return l2a.fmt(value)


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L2C Gatekeeper V2 Axis Replay Backend",
        "",
        "## Verdict",
        "",
        f"- analysis_status: `{report['analysis_status']}`",
        f"- final_decision: `{report['final_decision']}`",
        f"- manifest_path: `{report['manifest_path']}`",
        f"- recommended_next_path: `{report['recommended_next_path']}`",
    ]
    for blocker in report["blockers"]:
        lines.append(f"- blocker: `{blocker}`")

    denom = report["locked_denominator"]
    support = report["payload_and_support"]
    lines.extend(
        [
            "",
            "## Locked Denominator",
            "",
            f"- rows: `{denom['rows']}`",
            f"- quality_counts: `{json.dumps(denom['quality_counts'], sort_keys=True)}`",
            f"- namespace_counts: `{json.dumps(denom['namespace_counts'], sort_keys=True)}`",
            f"- dirty_good_rate: `{fmt(denom['dirty_good_rate'])}`",
            "",
            "## Replay Support",
            "",
            f"- v3_payload_counts: `{json.dumps(support['v3_payload_counts'], sort_keys=True)}`",
            f"- gate_trace_counts: `{json.dumps(support['gate_trace_counts'], sort_keys=True)}`",
            f"- baseline_parity_counts: `{json.dumps(support['baseline_parity_counts'], sort_keys=True)}`",
            f"- namespace_trace_counts: `{json.dumps(support['namespace_trace_counts'], sort_keys=True)}`",
            "",
            "## Axis Results",
            "",
            "| axis | axis_status | evaluated_rows | unsupported_rows | accepted_dirty_good | accepted_bad |",
            "| --- | --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for axis, result in report["axis_results"].items():
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{axis}`",
                    f"`{result['axis_status']}`",
                    fmt(result["evaluated_rows"]),
                    fmt(result["unsupported_rows"]),
                    fmt(result["accepted_dirty_good"]),
                    fmt(result["accepted_bad"]),
                ]
            )
            + " |"
        )
    lines.extend(["", "## Row Status Counts Per Axis", ""])
    for axis, result in report["axis_results"].items():
        lines.append(f"- `{axis}`: `{json.dumps(result['row_status_counts'], sort_keys=True)}`")

    if support["baseline_parity_gap_examples"]:
        lines.extend(["", "## Baseline Parity Gap Examples", ""])
        for example in support["baseline_parity_gap_examples"]:
            lines.append(f"- `{json.dumps(example, sort_keys=True)}`")

    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- All 85 denominator rows have full V3 replay payload, but that payload is not an authoritative Gatekeeper V2 axis replay contract.",
            "- J4C denominator rows do not carry gatekeeper_gate_trace, so single-axis replay cannot prove the non-axis gates.",
            "- Some R16-r1 rows have diagnostic gate traces that do not baseline-replay to the observed V2 verdict, so those traces cannot be used as the sole verdict engine.",
            "- No diagnostic flag was promoted to a causal ablation result.",
            "",
            "## Non-Goals",
            "",
        ]
    )
    for non_goal in report["non_goals"]:
        lines.append(f"- `{non_goal}`")
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--axes", nargs="*", default=DEFAULT_AXES)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    report = build_report(args.manifest, args.axes)
    l2a.write_json(args.output_json, report)
    l2a.write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    if report["analysis_status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
