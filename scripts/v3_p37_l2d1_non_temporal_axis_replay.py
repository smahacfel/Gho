#!/usr/bin/env python3
"""P3.7-L2D1 minimal Gatekeeper V2 non-temporal axis replay backend.

This runner is intentionally narrow. It uses the L1R21 manifest as SSOT, then
attempts causal forward replay only for non-temporal R16 axes on rows whose
source run did not already include the tested axis. A row is replayable only
when the logged Gatekeeper V2 trace can baseline-reproduce the observed verdict
and all axis-specific fields are present.

When current artifacts do not satisfy that contract, the runner reports a hard
input-gap decision instead of inferring counterfactual BUY/REJECT results.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import v3_p37_l2a_executable_subset_policy_delta as l2a
import v3_p37_l2c_gatekeeper_v2_axis_replay as l2c


SCHEMA_VERSION = 1
DEFAULT_MANIFEST = l2a.DEFAULT_MANIFEST
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2D1_GATEKEEPER_V2_NON_TEMPORAL_AXIS_REPLAY_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2D1_GATEKEEPER_V2_NON_TEMPORAL_AXIS_REPLAY_20260524.md"
)

NON_TEMPORAL_AXES = [
    "soft_pdd_instead_of_hard_pdd",
    "prosperity_filter_disabled",
    "hhi_hard_fail_relaxed",
    "elapsed_aware_entry_drift",
]

ALL_REQUESTABLE_AXES = NON_TEMPORAL_AXES + ["standard_mode_shorter_window"]

EXPECTED_DENOMINATOR = l2c.EXPECTED_DENOMINATOR
EXPECTED_DIRTY_GOOD = l2c.EXPECTED_DIRTY_GOOD
J4C_NAMESPACE = l2c.J4C_NAMESPACE
R16_R1_NAMESPACE = l2c.R16_R1_NAMESPACE

AXIS_REQUIRED_FIELDS = {
    "soft_pdd_instead_of_hard_pdd": [
        "gatekeeper_gate_trace",
        "pdd_hard_fail",
        "soft_points",
        "max_soft_points",
        "pdd_soft_penalty_points",
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
        "pdd_hard_fail",
        "pdd_entry_drift_pct",
        "pdd_entry_drift_effective_max_pct",
        "pdd_entry_drift_threshold_source",
    ],
}

R16_FORWARD_AXES = set(NON_TEMPORAL_AXES)


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def quality(row: dict[str, Any]) -> str:
    return l2c.quality(row)


def is_dirty_good(row: dict[str, Any]) -> bool:
    return l2c.is_dirty_good(row)


def is_bad(row: dict[str, Any]) -> bool:
    return l2c.is_bad(row)


def observed_verdict(row: dict[str, Any]) -> str:
    return l2c.observed_verdict(row)


def trace(decision: dict[str, Any] | None) -> list[dict[str, Any]]:
    return l2c.gate_trace(decision)


def hard_fail_gates(decision: dict[str, Any] | None) -> list[str]:
    return l2c.hard_gate_fails(trace(decision))


def field_present(value: Any) -> bool:
    return value is not None and value != ""


def missing_fields(decision: dict[str, Any] | None, fields: list[str]) -> list[str]:
    missing = []
    for field in fields:
        if field == "gatekeeper_gate_trace":
            if not trace(decision):
                missing.append(field)
            continue
        if not decision or not field_present(decision.get(field)):
            missing.append(field)
    return missing


def num(decision: dict[str, Any] | None, *fields: str) -> float | None:
    if not decision:
        return None
    return l2a.number_value(decision, *fields)


def gate_failed(decision: dict[str, Any] | None, gate_name: str) -> bool:
    return gate_name in hard_fail_gates(decision)


def hard_fail_gates_after_axis(axis: str, row: dict[str, Any]) -> list[str]:
    decision = row.get("decision")
    remaining = hard_fail_gates(decision)
    forgiven: set[str] = set()

    if axis == "soft_pdd_instead_of_hard_pdd":
        if gate_failed(decision, "pdd") and soft_budget_allows_pdd_penalty(decision):
            forgiven.add("pdd")
    elif axis == "prosperity_filter_disabled":
        if gate_failed(decision, "prosperity"):
            forgiven.add("prosperity")
    elif axis == "hhi_hard_fail_relaxed":
        hhi = num(decision, "hhi")
        if hhi is not None and hhi <= 0.20 and gate_failed(decision, "diversity_hhi_hard_fail"):
            forgiven.add("diversity_hhi_hard_fail")
    elif axis == "elapsed_aware_entry_drift":
        drift = num(decision, "pdd_entry_drift_pct", "entry_drift_pct")
        effective = num(decision, "pdd_entry_drift_effective_max_pct")
        hard_fail = str((decision or {}).get("pdd_hard_fail") or "")
        if (
            gate_failed(decision, "pdd")
            and "ENTRY_DRIFT" in hard_fail
            and drift is not None
            and effective is not None
            and abs(drift) <= effective
        ):
            forgiven.add("pdd")

    return [gate for gate in remaining if gate not in forgiven]


def soft_budget_allows_pdd_penalty(decision: dict[str, Any] | None) -> bool:
    soft_points = num(decision, "soft_points")
    max_soft = num(decision, "max_soft_points")
    pdd_penalty = num(decision, "pdd_soft_penalty_points")
    if soft_points is None or max_soft is None or pdd_penalty is None:
        return False
    return soft_points + pdd_penalty <= max_soft


def variant_verdict(axis: str, row: dict[str, Any]) -> str:
    remaining_hard_fails = hard_fail_gates_after_axis(axis, row)
    if remaining_hard_fails:
        terminal = str((row.get("decision") or {}).get("gatekeeper_terminal_gate") or "")
        return "TIMEOUT" if terminal == "timeout" else "REJECT"
    return "BUY"


def source_axis_contract(axis: str, row: dict[str, Any]) -> str | None:
    namespace = row["namespace"]
    if axis == "standard_mode_shorter_window":
        return "unsupported_temporal_replay_required"
    if namespace == R16_R1_NAMESPACE and axis in R16_FORWARD_AXES:
        return "unsupported_axis_already_applied_in_source_run"
    if namespace != J4C_NAMESPACE:
        return "unsupported_non_baseline_source_namespace"
    return None


def row_status(axis: str, row: dict[str, Any]) -> str:
    source_status = source_axis_contract(axis, row)
    if source_status is not None:
        return source_status

    decision = row.get("decision")
    missing = missing_fields(decision, AXIS_REQUIRED_FIELDS.get(axis, []))
    if missing:
        return "unsupported_missing_fields:" + ",".join(missing)

    support = l2c.row_replay_support(row)
    if not support["baseline_parity"]:
        return "unsupported_baseline_parity_gap"

    return "replay_ready"


def axis_result(axis: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    status_counts: Counter[str] = Counter()
    evaluated = []
    changed_reject_to_buy = 0
    changed_buy_to_reject = 0
    changed_reason_only = 0

    for row in rows:
        status = row_status(axis, row)
        status_counts[status] += 1
        if status != "replay_ready":
            continue

        before = observed_verdict(row)
        after = variant_verdict(axis, row)
        evaluated.append((row, before, after))
        if before != after:
            if before != "BUY" and after == "BUY":
                changed_reject_to_buy += 1
            elif before == "BUY" and after != "BUY":
                changed_buy_to_reject += 1
            else:
                changed_reason_only += 1

    accepted = [row for row, _before, after in evaluated if after == "BUY"]
    accepted_dirty = sum(1 for row in accepted if is_dirty_good(row))
    accepted_bad = sum(1 for row in accepted if is_bad(row))
    evaluated_dirty = sum(1 for row, _before, _after in evaluated if is_dirty_good(row))
    evaluated_bad = sum(1 for row, _before, _after in evaluated if is_bad(row))
    evaluated_count = len(evaluated)

    if axis == "standard_mode_shorter_window":
        final_status = "unsupported_temporal_replay_required"
    elif evaluated_count == 0:
        final_status = "blocked_no_causal_replay_rows"
    elif evaluated_count < len(rows):
        final_status = "partial_evaluated_input_limited"
    else:
        final_status = "evaluated"

    return {
        "axis_status": final_status,
        "row_status_counts": counter_dict(status_counts),
        "axis_evaluable_rows": evaluated_count,
        "unsupported_rows": len(rows) - evaluated_count,
        "variant_buy_rows": len(accepted),
        "accepted_dirty_good": accepted_dirty,
        "accepted_bad": accepted_bad,
        "missed_dirty_good": evaluated_dirty - accepted_dirty,
        "avoided_bad": evaluated_bad - accepted_bad,
        "dirty_good_capture_rate": accepted_dirty / evaluated_dirty if evaluated_dirty else None,
        "bad_accept_rate": accepted_bad / evaluated_bad if evaluated_bad else None,
        "dirty_good_precision": accepted_dirty / len(accepted) if accepted else None,
        "decision_delta_rows": changed_reject_to_buy + changed_buy_to_reject + changed_reason_only,
        "changed_from_reject_to_buy": changed_reject_to_buy,
        "changed_from_buy_to_reject": changed_buy_to_reject,
        "changed_reason_only": changed_reason_only,
    }


def input_support(rows: list[dict[str, Any]]) -> dict[str, Any]:
    gate_trace_counts: Counter[str] = Counter()
    parity_counts: Counter[str] = Counter()
    namespace_parity: Counter[str] = Counter()
    j4c_missing_gate_trace = 0
    r16_replayable = 0

    for row in rows:
        support = l2c.row_replay_support(row)
        gate_trace_counts["gate_trace_present" if support["has_gate_trace"] else "gate_trace_missing"] += 1
        parity_counts["baseline_parity_ok" if support["baseline_parity"] else "baseline_parity_gap"] += 1
        namespace_parity[
            f"{row['namespace']}|{'parity_ok' if support['baseline_parity'] else 'parity_gap'}"
        ] += 1
        if row["namespace"] == J4C_NAMESPACE and not support["has_gate_trace"]:
            j4c_missing_gate_trace += 1
        if row["namespace"] == R16_R1_NAMESPACE and support["baseline_parity"]:
            r16_replayable += 1

    return {
        "gate_trace_counts": counter_dict(gate_trace_counts),
        "baseline_parity_counts": counter_dict(parity_counts),
        "namespace_parity_counts": counter_dict(namespace_parity),
        "j4c_missing_gate_trace_rows": j4c_missing_gate_trace,
        "r16_replayable_rows": r16_replayable,
    }


def validate_manifest_contract(manifest: dict[str, Any], rows: list[dict[str, Any]]) -> list[str]:
    failures = l2c.validate_manifest_contract(manifest, rows)
    return [
        item.replace("BLOCK_L2C_", "BLOCK_L2D1_")
        for item in failures
    ]


def build_report(manifest_path: Path, axes: list[str]) -> dict[str, Any]:
    manifest = l2a.load_manifest(manifest_path)
    rows = l2c.load_denominator_rows(manifest)
    artifact_failures = [
        item.replace("missing_required_artifact", "BLOCK_L2D1_MISSING_REQUIRED_ARTIFACT")
        .replace("artifact_hash_mismatch", "BLOCK_L2D1_ARTIFACT_HASH_MISMATCH")
        for item in l2a.verify_artifacts(manifest)
    ]
    contract_failures = validate_manifest_contract(manifest, rows)
    blockers = sorted(set(artifact_failures + contract_failures))

    quality_counts = Counter(quality(row) for row in rows)
    namespace_counts = Counter(row["namespace"] for row in rows)
    axis_results = {axis: axis_result(axis, rows) for axis in axes}
    evaluated_axes = [
        axis for axis, result in axis_results.items()
        if result["axis_evaluable_rows"] > 0
    ]

    analysis_status = "pass" if not blockers else "fail"
    if blockers:
        final_decision = "BLOCK_L2D1_INPUT_MANIFEST_CONTRACT"
    elif evaluated_axes:
        final_decision = "GO_L2D1_NON_TEMPORAL_AXIS_REPLAY_RESULTS"
    else:
        final_decision = "BLOCK_L2D1_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP"

    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L2D1 Gatekeeper V2 Non-Temporal Axis Replay",
        "manifest_path": str(manifest_path),
        "analysis_status": analysis_status,
        "final_decision": final_decision,
        "blockers": blockers,
        "locked_denominator": {
            "rows": len(rows),
            "quality_counts": counter_dict(quality_counts),
            "namespace_counts": counter_dict(namespace_counts),
            "dirty_good_rate": quality_counts.get("buy_quality_dirty_good", 0) / len(rows)
            if rows else None,
        },
        "input_support": input_support(rows),
        "axes_requested": axes,
        "axes_evaluated": evaluated_axes,
        "axis_results": axis_results,
        "recommended_next_path": (
            "l2d_axis_result_review"
            if evaluated_axes
            else "emit_gatekeeper_v2_replay_contract_fields_for_baseline_rows"
        ),
        "interpretation": {
            "baseline_parity_required": True,
            "forward_axis_replay_requires_baseline_source_rows": True,
            "r16_rows_are_not_forward_causal_for_already_applied_axes": True,
            "standard_mode_requires_temporal_snapshots": True,
            "diagnostic_flags_not_used_as_causal_ablation": True,
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
        "# P3.7-L2D1 Gatekeeper V2 Non-Temporal Axis Replay",
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
    support = report["input_support"]
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
            "## Input Support",
            "",
            f"- gate_trace_counts: `{json.dumps(support['gate_trace_counts'], sort_keys=True)}`",
            f"- baseline_parity_counts: `{json.dumps(support['baseline_parity_counts'], sort_keys=True)}`",
            f"- namespace_parity_counts: `{json.dumps(support['namespace_parity_counts'], sort_keys=True)}`",
            f"- j4c_missing_gate_trace_rows: `{support['j4c_missing_gate_trace_rows']}`",
            f"- r16_replayable_rows: `{support['r16_replayable_rows']}`",
            "",
            "## Axis Results",
            "",
            "| axis | axis_status | axis_evaluable_rows | variant_buy_rows | accepted_dirty_good | accepted_bad |",
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
                    fmt(result["axis_evaluable_rows"]),
                    fmt(result["variant_buy_rows"]),
                    fmt(result["accepted_dirty_good"]),
                    fmt(result["accepted_bad"]),
                ]
            )
            + " |"
        )

    lines.extend(["", "## Row Status Counts Per Axis", ""])
    for axis, result in report["axis_results"].items():
        lines.append(f"- `{axis}`: `{json.dumps(result['row_status_counts'], sort_keys=True)}`")

    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- Causal forward axis replay requires baseline-source rows, complete V2 trace evidence, and baseline parity.",
            "- R16-r1 rows are useful for trace coverage diagnostics, but they already contain the tested R16 bundle axes and cannot prove forward J4C-to-R16 causality by themselves.",
            "- J4C rows are the needed baseline source for forward axis replay, but the current manifest rows do not carry Gatekeeper V2 gate traces.",
            "- `standard_mode_shorter_window` remains unsupported until temporal snapshots are available.",
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
    parser.add_argument("--axes", nargs="*", default=NON_TEMPORAL_AXES)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    unknown_axes = sorted(set(args.axes) - set(ALL_REQUESTABLE_AXES))
    if unknown_axes:
        raise SystemExit(f"unknown axes: {', '.join(unknown_axes)}")

    report = build_report(args.manifest, args.axes)
    l2a.write_json(args.output_json, report)
    l2a.write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    if report["analysis_status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
