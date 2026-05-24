#!/usr/bin/env python3
"""P3.7-L2B manifest-locked offline axis ablation gate.

L2B is allowed to use only the L1R21 locked executable subset. The script
fails closed when the input manifest contract is violated, replay payloads are
not available for the denominator, or the requested L2B policy axes are not
supported by a deterministic counterfactual replay backend.

The report intentionally separates observed J4C/R16-r1 policy delta from
causal axis ablation. Directional diagnostics may be useful for prioritization,
but they are not accepted as an ablation result.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import v3_p37_l2a_executable_subset_policy_delta as l2a


SCHEMA_VERSION = 1
DEFAULT_MANIFEST = l2a.DEFAULT_MANIFEST
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2B_MANIFEST_AXIS_ABLATION_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2B_MANIFEST_AXIS_ABLATION_20260524.md"
)

J4C_NAMESPACE = "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1"
R16_R1_NAMESPACE = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1"
REQUIRED_ALLOWED_RUNS = [J4C_NAMESPACE, R16_R1_NAMESPACE]
EXPECTED_DENOMINATOR = 85
EXPECTED_DIRTY_GOOD = 4

AXIS_VARIANTS = [
    "A0_j4c_baseline",
    "Afull_r16_r1_bundle",
    "A1_standard_mode_shorter_window_only",
    "A2_soft_pdd_instead_of_hard_pdd_only",
    "A3_prosperity_filter_disabled_only",
    "A4_hhi_hard_fail_relaxed_only",
    "A5_elapsed_aware_entry_drift_only",
]

REQUESTED_COUNTERFACTUAL_AXES = [
    "standard_mode_shorter_window",
    "soft_pdd_instead_of_hard_pdd",
    "prosperity_filter_disabled",
    "hhi_hard_fail_relaxed",
    "elapsed_aware_entry_drift",
]

SUPPORTED_COUNTERFACTUAL_AXES: list[str] = []


def row_key(label: dict[str, Any]) -> str:
    for field in ("source_ab_record_id", "ab_record_id", "v3_feature_snapshot_hash", "candidate_id"):
        value = label.get(field)
        if value is not None and str(value):
            return str(value)
    return "unknown"


def has_full_replay_payload(decision: dict[str, Any] | None) -> bool:
    if not decision:
        return False
    required = [
        "v3_materialized_feature_snapshot",
        "v3_policy_config_payload",
        "v3_shadow_verdict",
        "v3_shadow_reason_code",
        "v3_replay_payload_schema_version",
        "v3_materialization_version",
    ]
    return all(decision.get(field) is not None for field in required)


def denominator_rows(manifest: dict[str, Any]) -> list[dict[str, Any]]:
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


def quality(row: dict[str, Any]) -> str:
    return str(row["label"].get("buy_quality_class") or "unknown")


def is_dirty_good(row: dict[str, Any]) -> bool:
    return quality(row) == "buy_quality_dirty_good"


def is_bad(row: dict[str, Any]) -> bool:
    return quality(row) == "buy_quality_bad"


def accepted_metrics(rows: list[dict[str, Any]], accepted_keys: set[str]) -> dict[str, Any]:
    accepted = [row for row in rows if row["row_key"] in accepted_keys]
    accepted_dirty = sum(1 for row in accepted if is_dirty_good(row))
    accepted_bad = sum(1 for row in accepted if is_bad(row))
    total_dirty = sum(1 for row in rows if is_dirty_good(row))
    total_bad = sum(1 for row in rows if is_bad(row))
    return {
        "variant_buy_rows": len(accepted),
        "variant_reject_rows": len(rows) - len(accepted),
        "variant_timeout_rows": 0,
        "variant_pending_rows": 0,
        "accepted_dirty_good": accepted_dirty,
        "accepted_bad": accepted_bad,
        "missed_dirty_good": total_dirty - accepted_dirty,
        "avoided_bad": total_bad - accepted_bad,
        "dirty_good_capture_rate": accepted_dirty / total_dirty if total_dirty else None,
        "bad_accept_rate": accepted_bad / total_bad if total_bad else None,
        "dirty_good_precision": accepted_dirty / len(accepted) if accepted else None,
    }


def observed_anchor_variants(rows: list[dict[str, Any]]) -> dict[str, Any]:
    j4c_keys = {row["row_key"] for row in rows if row["namespace"] == J4C_NAMESPACE}
    r16_keys = {row["row_key"] for row in rows if row["namespace"] == R16_R1_NAMESPACE}
    combined = {row["row_key"] for row in rows}
    anchors = {
        "A0_j4c_baseline": {
            "mode": "observed_anchor_not_counterfactual",
            **accepted_metrics(rows, j4c_keys),
        },
        "Afull_r16_r1_bundle": {
            "mode": "observed_anchor_not_counterfactual",
            **accepted_metrics(rows, r16_keys),
        },
        "combined_allowed_subset": {
            "mode": "observed_dataset_summary_not_counterfactual",
            **accepted_metrics(rows, combined),
        },
    }
    anchors["Afull_r16_r1_bundle"]["delta_vs_J4C"] = {
        "accepted_dirty_good_delta": anchors["Afull_r16_r1_bundle"]["accepted_dirty_good"]
        - anchors["A0_j4c_baseline"]["accepted_dirty_good"],
        "accepted_bad_delta": anchors["Afull_r16_r1_bundle"]["accepted_bad"]
        - anchors["A0_j4c_baseline"]["accepted_bad"],
    }
    return anchors


def f64(decision: dict[str, Any], *fields: str) -> float | None:
    return l2a.number_value(decision, *fields)


def bool_value(value: Any) -> bool | None:
    if isinstance(value, bool):
        return value
    return None


def axis_diagnostic_flags(row: dict[str, Any]) -> dict[str, bool | None]:
    decision = row["decision"] or {}
    hhi = f64(decision, "hhi")
    drift = f64(decision, "pdd_entry_drift_pct", "entry_drift_pct")
    effective_drift = f64(decision, "pdd_entry_drift_effective_max_pct")
    pdd_hard_fail = str(decision.get("pdd_hard_fail") or "")
    first_kill_gate = str(decision.get("gatekeeper_first_kill_gate") or "")
    return {
        "soft_pdd_instead_of_hard_pdd": bool(
            pdd_hard_fail or first_kill_gate == "pdd"
        ),
        "prosperity_filter_disabled": bool_value(
            decision.get("prosperity_filter_enabled")
        )
        is False,
        "prosperity_would_fail_if_enabled": bool_value(
            decision.get("aps_shadow_prosperity_would_pass")
        )
        is False,
        "hhi_hard_fail_relaxed_band_0_10_0_20": (
            hhi is not None and hhi > 0.10 and hhi <= 0.20
        ),
        "hhi_above_r16_hard_fail_0_20": hhi is not None and hhi > 0.20,
        "elapsed_aware_entry_drift_present": effective_drift is not None
        and effective_drift > 5.0,
        "elapsed_aware_drift_would_allow_static5_reject": (
            drift is not None
            and effective_drift is not None
            and abs(drift) > 5.0
            and abs(drift) <= effective_drift
        ),
        "standard_mode_shorter_window": None,
    }


def axis_diagnostic_matrix(rows: list[dict[str, Any]]) -> dict[str, Any]:
    matrix: dict[str, Any] = {}
    for axis in REQUESTED_COUNTERFACTUAL_AXES:
        matrix[axis] = {
            "mode": "diagnostic_flag_not_counterfactual",
            "dirty_good_flagged_rows": 0,
            "bad_flagged_rows": 0,
            "dirty_good_unflagged_rows": 0,
            "bad_unflagged_rows": 0,
            "unknown_rows": 0,
        }

    flag_mapping = {
        "standard_mode_shorter_window": "standard_mode_shorter_window",
        "soft_pdd_instead_of_hard_pdd": "soft_pdd_instead_of_hard_pdd",
        "prosperity_filter_disabled": "prosperity_would_fail_if_enabled",
        "hhi_hard_fail_relaxed": "hhi_hard_fail_relaxed_band_0_10_0_20",
        "elapsed_aware_entry_drift": "elapsed_aware_drift_would_allow_static5_reject",
    }
    for row in rows:
        flags = axis_diagnostic_flags(row)
        for axis, flag_name in flag_mapping.items():
            value = flags[flag_name]
            if value is None:
                matrix[axis]["unknown_rows"] += 1
                continue
            if value and is_dirty_good(row):
                matrix[axis]["dirty_good_flagged_rows"] += 1
            elif value and is_bad(row):
                matrix[axis]["bad_flagged_rows"] += 1
            elif not value and is_dirty_good(row):
                matrix[axis]["dirty_good_unflagged_rows"] += 1
            elif not value and is_bad(row):
                matrix[axis]["bad_unflagged_rows"] += 1
    return matrix


def replay_payload_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    status_counts: Counter[str] = Counter()
    not_evaluable_examples = []
    for row in rows:
        if has_full_replay_payload(row["decision"]):
            status_counts["full_payload_available"] += 1
        else:
            status_counts["full_payload_missing"] += 1
            if len(not_evaluable_examples) < 10:
                not_evaluable_examples.append(
                    {
                        "namespace": row["namespace"],
                        "row_key": row["row_key"],
                        "has_decision_join": row["decision"] is not None,
                    }
                )
    return {
        "ablation_evaluable_rows": status_counts["full_payload_available"],
        "ablation_not_evaluable_rows": status_counts["full_payload_missing"],
        "status_counts": counter_dict(status_counts),
        "not_evaluable_examples": not_evaluable_examples,
    }


def validate_manifest_contract(manifest: dict[str, Any], rows: list[dict[str, Any]]) -> list[str]:
    failures = []
    actual = manifest.get("actual_contract", {})
    allowed = actual.get("allowed_runs", [])
    if allowed != REQUIRED_ALLOWED_RUNS:
        failures.append("BLOCK_L2_INPUT_MANIFEST_CONTRACT:allowed_runs_mismatch")
    if actual.get("buy_quality_denominator_rows") != EXPECTED_DENOMINATOR:
        failures.append("BLOCK_L2_DENOMINATOR_MISMATCH:manifest")
    if actual.get("buy_quality_dirty_good") != EXPECTED_DIRTY_GOOD:
        failures.append("BLOCK_L2_DIRTY_GOOD_MISMATCH:manifest")
    row_denominator = len(rows)
    row_dirty_good = sum(1 for row in rows if is_dirty_good(row))
    if row_denominator != EXPECTED_DENOMINATOR:
        failures.append("BLOCK_L2_DENOMINATOR_MISMATCH:rows")
    if row_dirty_good != EXPECTED_DIRTY_GOOD:
        failures.append("BLOCK_L2_DIRTY_GOOD_MISMATCH:rows")

    l2a_results = [l2a.analyze_run(run) for run in manifest.get("allowed_run_manifests", [])]
    failures.extend(l2a.validate_input_contract(manifest, l2a_results))
    return sorted(set(failures))


def unsupported_axis_variants() -> dict[str, Any]:
    variants = {}
    for variant in AXIS_VARIANTS:
        if variant in ("A0_j4c_baseline", "Afull_r16_r1_bundle"):
            continue
        variants[variant] = {
            "status": "not_evaluated_axis_replay_unsupported",
            "variant_buy_rows": None,
            "accepted_dirty_good": None,
            "accepted_bad": None,
            "missed_dirty_good": None,
            "avoided_bad": None,
            "dirty_good_capture_rate": None,
            "bad_accept_rate": None,
            "dirty_good_precision": None,
        }
    return variants


def build_report(manifest_path: Path) -> dict[str, Any]:
    manifest = l2a.load_manifest(manifest_path)
    rows = denominator_rows(manifest)
    artifact_failures = l2a.verify_artifacts(manifest)
    contract_failures = validate_manifest_contract(manifest, rows)
    replay_summary = replay_payload_summary(rows)
    blockers = sorted(set(artifact_failures + contract_failures))

    if replay_summary["ablation_not_evaluable_rows"]:
        blockers.append("BLOCK_L2B_REPLAY_PAYLOAD_GAP")

    unsupported_axes = [
        axis for axis in REQUESTED_COUNTERFACTUAL_AXES if axis not in SUPPORTED_COUNTERFACTUAL_AXES
    ]
    if unsupported_axes:
        blockers.append("BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED")

    status = "pass" if not blockers else "blocked"
    observed = observed_anchor_variants(rows)
    variant_results = dict(observed)
    variant_results.update(unsupported_axis_variants())

    quality_counts = Counter(quality(row) for row in rows)
    namespace_counts = Counter(row["namespace"] for row in rows)

    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L2B Manifest-Locked Offline Axis Ablation",
        "manifest_path": str(manifest_path),
        "status": status,
        "final_decision": (
            "GO_L2C_TARGETED_AXIS_EXPERIMENT_PREP"
            if status == "pass"
            else "BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED"
            if "BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED" in blockers
            else "BLOCK_L2B_INPUT_CONTRACT"
        ),
        "blockers": sorted(set(blockers)),
        "manifest_contract": manifest.get("actual_contract", {}),
        "locked_denominator": {
            "rows": len(rows),
            "quality_counts": counter_dict(quality_counts),
            "namespace_counts": counter_dict(namespace_counts),
            "small_sample_directional_only": True,
        },
        "replay_payload": replay_summary,
        "counterfactual_axis_support": {
            "requested_axes": REQUESTED_COUNTERFACTUAL_AXES,
            "supported_axes": SUPPORTED_COUNTERFACTUAL_AXES,
            "unsupported_axes": unsupported_axes,
            "reason": "Existing replay payloads are full, but no deterministic Gatekeeper V2 axis replay backend exists for the requested L2B axes.",
        },
        "variant_results": variant_results,
        "axis_diagnostic_matrix": axis_diagnostic_matrix(rows),
        "recommended_next_path": (
            "implement_manifest_locked_gatekeeper_v2_axis_replay_backend"
            if unsupported_axes
            else "l2c_targeted_axis_experiment_prep"
        ),
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
        "# P3.7-L2B Manifest-Locked Offline Axis Ablation",
        "",
        "## Verdict",
        "",
        f"- status: `{report['status']}`",
        f"- final_decision: `{report['final_decision']}`",
        f"- manifest_path: `{report['manifest_path']}`",
        f"- recommended_next_path: `{report['recommended_next_path']}`",
    ]
    for blocker in report["blockers"]:
        lines.append(f"- blocker: `{blocker}`")

    denom = report["locked_denominator"]
    replay = report["replay_payload"]
    support = report["counterfactual_axis_support"]
    lines.extend(
        [
            "",
            "## Locked Denominator",
            "",
            f"- rows: `{denom['rows']}`",
            f"- quality_counts: `{json.dumps(denom['quality_counts'], sort_keys=True)}`",
            f"- namespace_counts: `{json.dumps(denom['namespace_counts'], sort_keys=True)}`",
            f"- small_sample_directional_only: `{denom['small_sample_directional_only']}`",
            "",
            "## Replay Payload Gate",
            "",
            f"- ablation_evaluable_rows: `{replay['ablation_evaluable_rows']}`",
            f"- ablation_not_evaluable_rows: `{replay['ablation_not_evaluable_rows']}`",
            f"- status_counts: `{json.dumps(replay['status_counts'], sort_keys=True)}`",
            "",
            "## Counterfactual Axis Support",
            "",
            f"- requested_axes: `{json.dumps(support['requested_axes'], sort_keys=True)}`",
            f"- supported_axes: `{json.dumps(support['supported_axes'], sort_keys=True)}`",
            f"- unsupported_axes: `{json.dumps(support['unsupported_axes'], sort_keys=True)}`",
            f"- reason: {support['reason']}",
            "",
            "## Variant Results",
            "",
            "| variant | mode/status | accepted_dirty_good | accepted_bad | dirty_good_precision |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    for name, result in report["variant_results"].items():
        mode = result.get("mode") or result.get("status")
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{name}`",
                    f"`{mode}`",
                    fmt(result.get("accepted_dirty_good")),
                    fmt(result.get("accepted_bad")),
                    fmt(result.get("dirty_good_precision")),
                ]
            )
            + " |"
        )

    lines.extend(["", "## Diagnostic Axis Matrix", ""])
    for axis, item in report["axis_diagnostic_matrix"].items():
        lines.append(f"- `{axis}`: `{json.dumps(item, sort_keys=True)}`")

    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- L2B did not produce causal axis ablation because the requested Gatekeeper V2 policy axes are not supported by a deterministic replay backend.",
            "- Observed anchors remain useful: J4C has 0 dirty_good and R16-r1 has 4 dirty_good on the locked executable subset.",
            "- The diagnostic matrix is only directional evidence and must not be used for threshold tuning.",
            "",
            "## Non-Goals",
            "",
        ]
    )
    for non_goal in report["non_goals"]:
        lines.append(f"- `{non_goal}`")
    return "\n".join(lines) + "\n"


def write_json(path: Path, payload: dict[str, Any]) -> None:
    l2a.write_json(path, payload)


def write_text(path: Path, content: str) -> None:
    l2a.write_text(path, content)


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
    write_json(args.output_json, report)
    write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    if report["status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
