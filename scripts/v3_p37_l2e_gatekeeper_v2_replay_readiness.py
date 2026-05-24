#!/usr/bin/env python3
"""P3.7-L2E Gatekeeper V2 replay-input contract readiness audit.

This is not an axis ablation runner. It verifies whether a manifest-locked
denominator carries the additive Gatekeeper V2 replay-input contract required
by future causal L2 replay:

* non-temporal axes need a complete V2 gate trace, phase pass vector, soft
  budget/PDD/prosperity/HHI diagnostics, and baseline-ready verdict context.
* temporal axes such as standard-mode shorter-window need explicit decision
  evaluation snapshots; final MFS payloads are not enough.

The L1R21 manifest remains the only input SSOT.
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
REPLAY_INPUT_SCHEMA_VERSION = 1
DEFAULT_MANIFEST = l2a.DEFAULT_MANIFEST
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2E_GATEKEEPER_V2_REPLAY_INPUT_CONTRACT_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2E_GATEKEEPER_V2_REPLAY_INPUT_CONTRACT_20260524.md"
)

EXPECTED_DENOMINATOR = l2c.EXPECTED_DENOMINATOR
EXPECTED_DIRTY_GOOD = l2c.EXPECTED_DIRTY_GOOD

NON_TEMPORAL_AXES = [
    "soft_pdd_instead_of_hard_pdd",
    "prosperity_filter_disabled",
    "hhi_hard_fail_relaxed",
    "elapsed_aware_entry_drift",
]
TEMPORAL_AXES = ["standard_mode_shorter_window"]

CONTRACT_FIELDS = [
    "gatekeeper_v2_replay_input_schema_version",
    "gatekeeper_v2_replay_ready_non_temporal",
    "gatekeeper_v2_replay_ready_temporal",
    "gatekeeper_v2_replay_missing_fields",
    "gatekeeper_v2_phase_pass_vector",
    "observed_mode",
    "observed_window_ms",
    "observed_stage",
]


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def present(value: Any) -> bool:
    return value is not None and value != "" and value != []


def decision(row: dict[str, Any]) -> dict[str, Any]:
    raw = row.get("decision")
    return raw if isinstance(raw, dict) else {}


def schema_status(raw: dict[str, Any]) -> str:
    version = raw.get("gatekeeper_v2_replay_input_schema_version")
    if version is None:
        return "missing_v2_replay_contract"
    if version != REPLAY_INPUT_SCHEMA_VERSION:
        return f"unsupported_v2_replay_contract_version:{version}"
    return "v2_replay_contract_v1"


def explicit_missing_fields(raw: dict[str, Any]) -> list[str]:
    value = raw.get("gatekeeper_v2_replay_missing_fields")
    if isinstance(value, list):
        return [str(item) for item in value if str(item)]
    return []


def missing_contract_fields(raw: dict[str, Any]) -> list[str]:
    return [field for field in CONTRACT_FIELDS if field not in raw]


def non_temporal_ready(raw: dict[str, Any]) -> bool:
    return (
        raw.get("gatekeeper_v2_replay_input_schema_version") == REPLAY_INPUT_SCHEMA_VERSION
        and raw.get("gatekeeper_v2_replay_ready_non_temporal") is True
    )


def temporal_ready(raw: dict[str, Any]) -> bool:
    return (
        raw.get("gatekeeper_v2_replay_input_schema_version") == REPLAY_INPUT_SCHEMA_VERSION
        and raw.get("gatekeeper_v2_replay_ready_temporal") is True
        and present(raw.get("decision_eval_snapshots"))
    )


def row_readiness(row: dict[str, Any]) -> dict[str, Any]:
    raw = decision(row)
    missing = missing_contract_fields(raw)
    explicit_missing = explicit_missing_fields(raw)
    nt_ready = non_temporal_ready(raw)
    t_ready = temporal_ready(raw)
    return {
        "schema_status": schema_status(raw),
        "missing_contract_fields": missing,
        "explicit_missing_fields": explicit_missing,
        "non_temporal_ready": nt_ready,
        "temporal_ready": t_ready,
        "standard_axis_status": "replay_ready" if t_ready else "unsupported_temporal_snapshots_missing",
        "non_temporal_axis_status": "replay_ready" if nt_ready else "unsupported_v2_replay_input_gap",
    }


def input_support(rows: list[dict[str, Any]]) -> dict[str, Any]:
    schema_counts: Counter[str] = Counter()
    non_temporal_counts: Counter[str] = Counter()
    temporal_counts: Counter[str] = Counter()
    missing_contract_counts: Counter[str] = Counter()
    explicit_missing_counts: Counter[str] = Counter()
    namespace_non_temporal_counts: Counter[str] = Counter()
    namespace_temporal_counts: Counter[str] = Counter()

    for row in rows:
        readiness = row_readiness(row)
        namespace = str(row.get("namespace") or "unknown")
        schema_counts[readiness["schema_status"]] += 1
        nt_key = "ready" if readiness["non_temporal_ready"] else "not_ready"
        t_key = "ready" if readiness["temporal_ready"] else "not_ready"
        non_temporal_counts[nt_key] += 1
        temporal_counts[t_key] += 1
        namespace_non_temporal_counts[f"{namespace}|{nt_key}"] += 1
        namespace_temporal_counts[f"{namespace}|{t_key}"] += 1
        for field in readiness["missing_contract_fields"]:
            missing_contract_counts[field] += 1
        for field in readiness["explicit_missing_fields"]:
            explicit_missing_counts[field] += 1

    return {
        "schema_status_counts": counter_dict(schema_counts),
        "non_temporal_ready_counts": counter_dict(non_temporal_counts),
        "temporal_ready_counts": counter_dict(temporal_counts),
        "missing_contract_field_counts": counter_dict(missing_contract_counts),
        "explicit_missing_field_counts": counter_dict(explicit_missing_counts),
        "namespace_non_temporal_ready_counts": counter_dict(namespace_non_temporal_counts),
        "namespace_temporal_ready_counts": counter_dict(namespace_temporal_counts),
    }


def validate_manifest_contract(manifest: dict[str, Any], rows: list[dict[str, Any]]) -> list[str]:
    failures = l2c.validate_manifest_contract(manifest, rows)
    return [item.replace("BLOCK_L2C_", "BLOCK_L2E_") for item in failures]


def build_report(manifest_path: Path) -> dict[str, Any]:
    manifest = l2a.load_manifest(manifest_path)
    rows = l2c.load_denominator_rows(manifest)
    artifact_failures = [
        item.replace("missing_required_artifact", "BLOCK_L2E_MISSING_REQUIRED_ARTIFACT")
        .replace("artifact_hash_mismatch", "BLOCK_L2E_ARTIFACT_HASH_MISMATCH")
        for item in l2a.verify_artifacts(manifest)
    ]
    contract_failures = validate_manifest_contract(manifest, rows)
    blockers = sorted(set(artifact_failures + contract_failures))

    quality_counts = Counter(l2c.quality(row) for row in rows)
    namespace_counts = Counter(row["namespace"] for row in rows)
    support = input_support(rows)
    non_temporal_ready_rows = support["non_temporal_ready_counts"].get("ready", 0)
    temporal_ready_rows = support["temporal_ready_counts"].get("ready", 0)

    analysis_status = "pass" if not blockers else "fail"
    if blockers:
        final_decision = "BLOCK_L2E_INPUT_MANIFEST_CONTRACT"
        recommended_next_path = "repair_l1r21_manifest_contract"
    elif non_temporal_ready_rows == len(rows) and temporal_ready_rows == len(rows):
        final_decision = "GO_L2D_FULL_GATEKEEPER_V2_AXIS_REPLAY_READY"
        recommended_next_path = "rerun_l2d_axis_replay"
    elif non_temporal_ready_rows == len(rows):
        final_decision = "GO_L2D_NON_TEMPORAL_REPLAY_READY_TEMPORAL_BLOCKED"
        recommended_next_path = "rerun_l2d_non_temporal_axes_keep_standard_axis_blocked"
    else:
        final_decision = "BLOCK_L2E_HISTORICAL_ROWS_MISSING_V22_REPLAY_CONTRACT"
        recommended_next_path = "run_r17_replay_ready_diagnostic_after_l2e_instrumentation"

    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L2E Gatekeeper V2 Replay Input Contract",
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
        "replay_input_contract": {
            "runtime_schema_version": REPLAY_INPUT_SCHEMA_VERSION,
            "contract_fields": CONTRACT_FIELDS,
            "non_temporal_axes": NON_TEMPORAL_AXES,
            "temporal_axes": TEMPORAL_AXES,
            "standard_mode_requires_decision_eval_snapshots": True,
        },
        "input_support": support,
        "axis_readiness": {
            axis: {
                "axis_kind": "non_temporal",
                "ready_rows": non_temporal_ready_rows,
                "blocked_rows": len(rows) - non_temporal_ready_rows,
                "axis_status": "replay_ready" if non_temporal_ready_rows == len(rows)
                else "unsupported_v2_replay_input_gap",
            }
            for axis in NON_TEMPORAL_AXES
        }
        | {
            axis: {
                "axis_kind": "temporal",
                "ready_rows": temporal_ready_rows,
                "blocked_rows": len(rows) - temporal_ready_rows,
                "axis_status": "replay_ready" if temporal_ready_rows == len(rows)
                else "unsupported_temporal_snapshots_missing",
            }
            for axis in TEMPORAL_AXES
        },
        "recommended_next_path": recommended_next_path,
        "interpretation": {
            "v3_replay_payload_is_not_gatekeeper_v2_axis_replay_input": True,
            "historical_l1r21_rows_precede_l2e_contract": True,
            "future_runs_must_emit_contract_at_decision_time": True,
            "temporal_axis_requires_decision_eval_snapshots": True,
            "diagnostic_flags_not_used_as_causal_ablation": True,
        },
        "non_goals": [
            "no_runtime_started_by_this_script",
            "no_threshold_tuning",
            "no_phase_b",
            "no_p2_live",
            "no_new_runs_added_to_manifest",
            "no_causal_axis_claim",
        ],
    }


def fmt(value: Any) -> str:
    return l2a.fmt(value)


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L2E Gatekeeper V2 Replay Input Contract",
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
            "## Replay Input Support",
            "",
            f"- schema_status_counts: `{json.dumps(support['schema_status_counts'], sort_keys=True)}`",
            f"- non_temporal_ready_counts: `{json.dumps(support['non_temporal_ready_counts'], sort_keys=True)}`",
            f"- temporal_ready_counts: `{json.dumps(support['temporal_ready_counts'], sort_keys=True)}`",
            f"- missing_contract_field_counts: `{json.dumps(support['missing_contract_field_counts'], sort_keys=True)}`",
            f"- explicit_missing_field_counts: `{json.dumps(support['explicit_missing_field_counts'], sort_keys=True)}`",
            "",
            "## Axis Readiness",
            "",
            "| axis | kind | status | ready_rows | blocked_rows |",
            "| --- | --- | --- | ---: | ---: |",
        ]
    )
    for axis, readiness in report["axis_readiness"].items():
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{axis}`",
                    f"`{readiness['axis_kind']}`",
                    f"`{readiness['axis_status']}`",
                    fmt(readiness["ready_rows"]),
                    fmt(readiness["blocked_rows"]),
                ]
            )
            + " |"
        )

    lines.extend(
        [
            "",
            "## Contract",
            "",
            f"- runtime_schema_version: `{report['replay_input_contract']['runtime_schema_version']}`",
            f"- non_temporal_axes: `{json.dumps(report['replay_input_contract']['non_temporal_axes'])}`",
            f"- temporal_axes: `{json.dumps(report['replay_input_contract']['temporal_axes'])}`",
            "- `standard_mode_shorter_window` remains blocked until `decision_eval_snapshots` are emitted.",
            "",
            "## Interpretation",
            "",
            "- L2E adds the future-run replay-input contract; it does not claim that historical L1R21 rows satisfy it.",
            "- Full V3 replay payload is not sufficient for Gatekeeper V2 causal axis replay.",
            "- Non-temporal axes require explicit V2 trace/phase/soft-budget/PDD/prosperity/HHI diagnostics at terminal decision time.",
            "- Temporal standard-window replay requires explicit decision-evaluation snapshots, not final MFS snapshots.",
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
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    report = build_report(args.manifest)
    l2a.write_json(args.output_json, report)
    l2a.write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    if report["analysis_status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
