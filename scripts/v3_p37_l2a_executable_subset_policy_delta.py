#!/usr/bin/env python3
"""P3.7-L2A executable-subset policy delta analysis.

The L1R21 manifest is the only source of truth for L2A inputs. This script
fails closed on namespace, denominator, dirty-good, artifact hash, and
non-executable denominator mismatches before producing any policy-delta report.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import v3_p37_mfs_lifecycle_join_key_audit as join_audit


SCHEMA_VERSION = 1
DEFAULT_MANIFEST = Path(
    "PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json"
)
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2A_EXECUTABLE_SUBSET_POLICY_DELTA_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L2A_EXECUTABLE_SUBSET_POLICY_DELTA_20260524.md"
)

EXPECTED_DENOMINATOR = 85
EXPECTED_DIRTY_GOOD = 4


def sha256_file(path: Path) -> str | None:
    if not path.exists() or not path.is_file():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    yield from join_audit.iter_jsonl(path)


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def is_non_executable_label(row: dict[str, Any]) -> bool:
    status = str(row.get("execution_feasibility_status") or "")
    return row.get("buy_quality_class") == "buy_quality_not_executable" or status.startswith(
        "not_executable"
    )


def string_value(row: dict[str, Any], *fields: str) -> str:
    for field in fields:
        value = row.get(field)
        if value is None:
            continue
        text = str(value)
        if text:
            return text
    return "unknown"


def bool_bucket(value: Any) -> str:
    if value is True:
        return "true"
    if value is False:
        return "false"
    if value is None:
        return "unknown"
    return str(value)


def number_value(row: dict[str, Any], *fields: str) -> float | None:
    for field in fields:
        value = row.get(field)
        if value is None or value == "":
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return None


def numeric_bucket(value: float | None, buckets: tuple[tuple[float, str], ...]) -> str:
    if value is None:
        return "unavailable"
    for upper, name in buckets:
        if value <= upper:
            return name
    return buckets[-1][1] if buckets and buckets[-1][0] == float("inf") else "gt_max"


def drift_bucket(row: dict[str, Any]) -> str:
    value = number_value(row, "pdd_entry_drift_pct", "entry_drift_pct")
    if value is not None:
        value = abs(value)
    return numeric_bucket(
        value,
        (
            (3.0, "abs_pct_0_3"),
            (6.0, "abs_pct_3_6"),
            (10.0, "abs_pct_6_10"),
            (15.0, "abs_pct_10_15"),
            (float("inf"), "abs_pct_gt_15"),
        ),
    )


def hhi_bucket(row: dict[str, Any]) -> str:
    return numeric_bucket(
        number_value(row, "hhi"),
        (
            (0.10, "lte_0_10"),
            (0.155, "0_10_0_155"),
            (0.20, "0_155_0_20"),
            (0.35, "0_20_0_35"),
            (float("inf"), "gt_0_35"),
        ),
    )


def pct_bucket(row: dict[str, Any], field: str) -> str:
    return numeric_bucket(
        number_value(row, field),
        (
            (0.70, "lte_0_70"),
            (0.90, "0_70_0_90"),
            (0.95, "0_90_0_95"),
            (float("inf"), "gt_0_95"),
        ),
    )


def same_ms_bucket(row: dict[str, Any]) -> str:
    return numeric_bucket(
        number_value(row, "same_ms_tx_ratio"),
        (
            (0.25, "lte_0_25"),
            (0.50, "0_25_0_50"),
            (0.75, "0_50_0_75"),
            (float("inf"), "gt_0_75"),
        ),
    )


def verdict(row: dict[str, Any] | None) -> str:
    if not row:
        return "unjoined"
    if row.get("decision_verdict_buy") is True or row.get("legacy_live_verdict_buy") is True:
        return "BUY"
    explicit = string_value(row, "verdict_type", "legacy_live_verdict_type")
    if explicit != "unknown":
        return explicit
    reason = string_value(row, "reason_code", "decision_reason")
    if reason.startswith("TIMEOUT"):
        return "TIMEOUT"
    if reason.startswith("REJECT") or reason.startswith("HARD_FAIL"):
        return "REJECT"
    return "unknown"


def load_manifest(path: Path) -> dict[str, Any]:
    manifest = read_json(path)
    if manifest.get("manifest_status") != "pass":
        raise ValueError("L1R21 manifest_status is not pass")
    if manifest.get("final_decision") != "GO_L2_INPUT_MANIFEST_LOCKED":
        raise ValueError("L1R21 final_decision is not GO_L2_INPUT_MANIFEST_LOCKED")
    return manifest


def verify_artifacts(manifest: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    for run in manifest.get("allowed_run_manifests", []):
        for artifact in run.get("artifacts", []):
            path = Path(str(artifact.get("path") or ""))
            if not artifact.get("exists") or not path.exists():
                failures.append(f"missing_required_artifact:{run.get('namespace')}:{path}")
                continue
            expected_hash = artifact.get("sha256")
            if expected_hash:
                actual_hash = sha256_file(path)
                if actual_hash != expected_hash:
                    failures.append(
                        f"artifact_hash_mismatch:{run.get('namespace')}:{path}"
                    )
    return failures


def label_artifacts(run: dict[str, Any]) -> list[Path]:
    return [
        Path(str(artifact["path"]))
        for artifact in run.get("artifacts", [])
        if artifact.get("role") == "lifecycle_label_file"
    ]


def decision_artifacts(run: dict[str, Any]) -> list[Path]:
    return [
        Path(str(artifact["path"]))
        for artifact in run.get("artifacts", [])
        if artifact.get("role") == "decision"
    ]


def feature_availability_artifacts(run: dict[str, Any]) -> list[Path]:
    return [
        Path(str(artifact["path"]))
        for artifact in run.get("artifacts", [])
        if artifact.get("role") == "feature_availability_file"
    ]


def build_decision_index(paths: list[Path]) -> dict[str, dict[str, Any]]:
    index: dict[str, dict[str, Any]] = {}
    for path in paths:
        for row in iter_jsonl(path):
            for key in (
                row.get("ab_record_id"),
                row.get("source_ab_record_id"),
                row.get("v3_feature_snapshot_hash"),
                row.get("candidate_id"),
            ):
                if key is not None and str(key):
                    index.setdefault(str(key), row)
    return index


def match_decision(label: dict[str, Any], index: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
    for key in (
        label.get("source_ab_record_id"),
        label.get("ab_record_id"),
        label.get("v3_feature_snapshot_hash"),
        label.get("candidate_id"),
    ):
        if key is not None and str(key) in index:
            return index[str(key)]
    return None


def load_run_rows(run: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    labels = [row for path in label_artifacts(run) for row in iter_jsonl(path)]
    decision_index = build_decision_index(decision_artifacts(run))
    joined = []
    for label in labels:
        decision = match_decision(label, decision_index)
        joined.append({"label": label, "decision": decision})
    return labels, joined


def observed_namespaces(rows: list[dict[str, Any]]) -> list[str]:
    namespaces: set[str] = set()
    for row in rows:
        for field in ("rollout_namespace", "run_id"):
            value = row.get(field)
            if value is not None and str(value):
                namespaces.add(str(value))
    return sorted(namespaces)


def feature_availability_summary(run: dict[str, Any]) -> dict[str, Any]:
    summaries = []
    for path in feature_availability_artifacts(run):
        payload = read_json(path)
        summaries.append(
            {
                "path": str(path),
                "feature_availability_status": payload.get("feature_availability_status"),
                "reason": payload.get("reason") or payload.get("feature_availability_reason"),
                "rows_total": payload.get("rows_total"),
                "buy_quality_denominator_rows": payload.get("buy_quality_denominator_rows"),
                "execution_feasibility_reject_rows": payload.get(
                    "execution_feasibility_reject_rows"
                ),
                "diagnostic_minimums": payload.get("diagnostic_minimums", {}),
            }
        )
    return {"files": summaries}


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def bump(counter: Counter[str], value: str) -> None:
    counter[value if value else "unknown"] += 1


def analyze_joined_rows(joined: list[dict[str, Any]]) -> dict[str, Any]:
    counters: dict[str, Counter[str]] = defaultdict(Counter)
    by_quality: dict[str, dict[str, Counter[str]]] = defaultdict(lambda: defaultdict(Counter))
    joined_decision_rows = 0
    for item in joined:
        label = item["label"]
        decision = item["decision"]
        quality = string_value(label, "buy_quality_class")
        if decision is not None:
            joined_decision_rows += 1

        bump(counters["buy_quality_class"], quality)
        bump(counters["market_outcome_class"], string_value(label, "market_outcome_class"))
        bump(counters["close_reason"], string_value(label, "close_reason"))
        bump(counters["label_quality"], string_value(label, "label_quality"))
        bump(counters["decision_verdict"], verdict(decision))
        bump(counters["reason_code"], string_value(decision or {}, "reason_code", "legacy_live_verdict_type", "decision_reason"))
        bump(counters["first_kill_gate"], string_value(decision or {}, "gatekeeper_first_kill_gate"))
        bump(counters["terminal_gate"], string_value(decision or {}, "gatekeeper_terminal_gate", "legacy_live_verdict_type"))
        bump(counters["pdd_entry_drift_bucket"], drift_bucket(decision or {}))
        bump(counters["hhi_bucket"], hhi_bucket(decision or {}))
        bump(counters["top3_volume_pct_bucket"], pct_bucket(decision or {}, "top3_volume_pct"))
        bump(counters["same_ms_tx_ratio_bucket"], same_ms_bucket(decision or {}))
        bump(counters["alpha_actionable"], bool_bucket((decision or {}).get("alpha_actionable")))
        bump(counters["prosperity_actionable"], bool_bucket((decision or {}).get("prosperity_actionable")))
        bump(counters["prosperity_filter_enabled"], bool_bucket((decision or {}).get("prosperity_filter_enabled")))
        bump(counters["aps_shadow_prosperity_would_pass"], bool_bucket((decision or {}).get("aps_shadow_prosperity_would_pass")))
        bump(counters["pdd_hard_fail"], string_value(decision or {}, "pdd_hard_fail"))
        bump(counters["pdd_soft_flags"], string_value(decision or {}, "pdd_soft_flags"))
        bump(counters["pdd_spike_detected"], bool_bucket((decision or {}).get("pdd_spike_detected")))
        bump(counters["pdd_ramping_detected"], bool_bucket((decision or {}).get("pdd_ramping_detected")))
        bump(counters["pdd_price_anchor_available"], bool_bucket((decision or {}).get("pdd_price_anchor_available")))

        by_quality[quality]["reason_code"][string_value(decision or {}, "reason_code", "legacy_live_verdict_type", "decision_reason")] += 1
        by_quality[quality]["first_kill_gate"][string_value(decision or {}, "gatekeeper_first_kill_gate")] += 1
        by_quality[quality]["pdd_hard_fail"][string_value(decision or {}, "pdd_hard_fail")] += 1
        by_quality[quality]["hhi_bucket"][hhi_bucket(decision or {})] += 1

    return {
        "joined_decision_rows": joined_decision_rows,
        "decision_join_rate": joined_decision_rows / len(joined) if joined else None,
        "distributions": {key: counter_dict(value) for key, value in sorted(counters.items())},
        "by_quality": {
            quality: {key: counter_dict(counter) for key, counter in sorted(groups.items())}
            for quality, groups in sorted(by_quality.items())
        },
    }


def validate_input_contract(manifest: dict[str, Any], run_results: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    allowed = set(manifest["actual_contract"]["allowed_runs"])
    blocked = set(manifest["actual_contract"]["blocked_runs"])
    denominator = sum(result["buy_quality_denominator_rows"] for result in run_results)
    dirty_good = sum(result["buy_quality_dirty_good"] for result in run_results)
    if denominator != EXPECTED_DENOMINATOR:
        failures.append("BLOCK_L2_DENOMINATOR_MISMATCH")
    if dirty_good != EXPECTED_DIRTY_GOOD:
        failures.append("BLOCK_L2_DIRTY_GOOD_MISMATCH")
    for result in run_results:
        namespace = result["namespace"]
        if namespace not in allowed:
            failures.append(f"BLOCK_L2_UNKNOWN_NAMESPACE:{namespace}")
        if namespace in blocked:
            failures.append(f"BLOCK_L2_BLOCKED_NAMESPACE_PRESENT:{namespace}")
        if result["non_executable_rows_in_denominator"] > 0:
            failures.append(f"BLOCK_L2_NON_EXECUTABLE_IN_DENOMINATOR:{namespace}")
        for observed_namespace in result.get("observed_label_namespaces", []):
            if observed_namespace in blocked:
                failures.append(f"BLOCK_L2_BLOCKED_NAMESPACE_PRESENT:{observed_namespace}")
            if observed_namespace not in allowed:
                failures.append(f"BLOCK_L2_UNKNOWN_NAMESPACE:{observed_namespace}")
    return sorted(set(failures))


def analyze_run(run: dict[str, Any]) -> dict[str, Any]:
    labels, joined = load_run_rows(run)
    denominator_rows = [row for row in labels if not is_non_executable_label(row)]
    non_executable_rows = [row for row in labels if is_non_executable_label(row)]
    quality_counts = Counter(str(row.get("buy_quality_class") or "unknown") for row in denominator_rows)
    analysis = analyze_joined_rows([item for item in joined if not is_non_executable_label(item["label"])])
    return {
        "namespace": run["namespace"],
        "observed_label_namespaces": observed_namespaces(labels),
        "label_rows_total": len(labels),
        "buy_quality_denominator_rows": len(denominator_rows),
        "buy_quality_bad": quality_counts.get("buy_quality_bad", 0),
        "buy_quality_dirty_good": quality_counts.get("buy_quality_dirty_good", 0),
        "buy_quality_good": quality_counts.get("buy_quality_good", 0),
        "buy_quality_not_executable": sum(
            1 for row in non_executable_rows if row.get("buy_quality_class") == "buy_quality_not_executable"
        ),
        "non_executable_rows_in_denominator": sum(
            1 for row in denominator_rows if is_non_executable_label(row)
        ),
        "dirty_good_rate": (
            quality_counts.get("buy_quality_dirty_good", 0) / len(denominator_rows)
            if denominator_rows
            else None
        ),
        "analysis": analysis,
        "feature_availability": feature_availability_summary(run),
    }


def combined_result(run_results: list[dict[str, Any]]) -> dict[str, Any]:
    combined: dict[str, Any] = {
        "namespace": "combined_allowed_subset",
        "label_rows_total": sum(row["label_rows_total"] for row in run_results),
        "buy_quality_denominator_rows": sum(row["buy_quality_denominator_rows"] for row in run_results),
        "buy_quality_bad": sum(row["buy_quality_bad"] for row in run_results),
        "buy_quality_dirty_good": sum(row["buy_quality_dirty_good"] for row in run_results),
        "buy_quality_good": sum(row["buy_quality_good"] for row in run_results),
        "buy_quality_not_executable": sum(row["buy_quality_not_executable"] for row in run_results),
    }
    combined["dirty_good_rate"] = (
        combined["buy_quality_dirty_good"] / combined["buy_quality_denominator_rows"]
        if combined["buy_quality_denominator_rows"]
        else None
    )
    return combined


def policy_delta(run_results: list[dict[str, Any]]) -> dict[str, Any]:
    by_namespace = {row["namespace"]: row for row in run_results}
    j4c = by_namespace.get(
        "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1"
    )
    r16 = by_namespace.get("shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1")
    if not j4c or not r16:
        return {"status": "unavailable", "reason": "required J4C/R16-r1 runs missing"}
    dirty_good_delta = r16["buy_quality_dirty_good"] - j4c["buy_quality_dirty_good"]
    bad_delta = r16["buy_quality_bad"] - j4c["buy_quality_bad"]
    dirty_rate_delta = (r16["dirty_good_rate"] or 0.0) - (j4c["dirty_good_rate"] or 0.0)
    candidate_axes = []
    if dirty_good_delta > 0:
        candidate_axes.extend(
            [
                "standard_mode_shorter_window",
                "soft_pdd_instead_of_hard_pdd",
                "prosperity_filter_disabled",
                "hhi_hard_fail_relaxed",
                "elapsed_aware_entry_drift",
            ]
        )
    return {
        "status": "computed_unpaired_delta",
        "note": "J4C and R16-r1 are different sampled universes; this is directional policy delta, not causal attribution.",
        "j4c_dirty_good": j4c["buy_quality_dirty_good"],
        "r16_r1_dirty_good": r16["buy_quality_dirty_good"],
        "dirty_good_delta": dirty_good_delta,
        "j4c_bad": j4c["buy_quality_bad"],
        "r16_r1_bad": r16["buy_quality_bad"],
        "bad_delta": bad_delta,
        "j4c_dirty_good_rate": j4c["dirty_good_rate"],
        "r16_r1_dirty_good_rate": r16["dirty_good_rate"],
        "dirty_good_rate_delta": dirty_rate_delta,
        "l2b_candidate_axes": candidate_axes,
    }


def build_report(manifest_path: Path) -> dict[str, Any]:
    manifest = load_manifest(manifest_path)
    artifact_failures = verify_artifacts(manifest)
    run_results = [analyze_run(run) for run in manifest.get("allowed_run_manifests", [])]
    contract_failures = validate_input_contract(manifest, run_results)
    blockers = sorted(set(artifact_failures + contract_failures))
    status = "pass" if not blockers else "fail"
    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L2A Executable Subset Policy Delta Analysis",
        "manifest_path": str(manifest_path),
        "status": status,
        "final_decision": (
            "GO_L2B_AXIS_ABLATION_PREP"
            if status == "pass"
            else "BLOCK_L2A_INPUT_MANIFEST_CONTRACT"
        ),
        "blockers": blockers,
        "manifest_contract": manifest.get("actual_contract", {}),
        "run_results": run_results,
        "combined_allowed_subset": combined_result(run_results),
        "policy_delta": policy_delta(run_results),
        "non_goals": [
            "no_runtime",
            "no_threshold_tuning",
            "no_phase_b",
            "no_p2_live",
            "no_new_runs",
            "no_full_r16_route_universe",
        ],
    }


def fmt(value: Any) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.4f}"
    return str(value)


def render_distribution_section(lines: list[str], title: str, distributions: dict[str, dict[str, int]]) -> None:
    lines.extend(["", f"## {title}", ""])
    for name in (
        "buy_quality_class",
        "decision_verdict",
        "reason_code",
        "first_kill_gate",
        "terminal_gate",
        "pdd_entry_drift_bucket",
        "hhi_bucket",
        "top3_volume_pct_bucket",
        "same_ms_tx_ratio_bucket",
        "pdd_hard_fail",
        "pdd_soft_flags",
        "alpha_actionable",
        "prosperity_filter_enabled",
        "aps_shadow_prosperity_would_pass",
        "pdd_spike_detected",
        "pdd_ramping_detected",
    ):
        lines.append(f"- {name}: `{json.dumps(distributions.get(name, {}), sort_keys=True)}`")


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L2A Executable Subset Policy Delta Analysis",
        "",
        "## Verdict",
        "",
        f"- status: `{report['status']}`",
        f"- final_decision: `{report['final_decision']}`",
        f"- manifest_path: `{report['manifest_path']}`",
    ]
    for blocker in report["blockers"]:
        lines.append(f"- blocker: `{blocker}`")
    lines.extend(
        [
            "",
            "## Input Contract",
            "",
            f"- buy_quality_denominator_rows: `{report['manifest_contract'].get('buy_quality_denominator_rows')}`",
            f"- buy_quality_dirty_good: `{report['manifest_contract'].get('buy_quality_dirty_good')}`",
            f"- dirty_good_rate: `{fmt(report['manifest_contract'].get('dirty_good_rate'))}`",
            f"- allowed_runs: `{json.dumps(report['manifest_contract'].get('allowed_runs', []), sort_keys=True)}`",
            f"- blocked_runs_count: `{len(report['manifest_contract'].get('blocked_runs', []))}`",
            "",
            "## Per-Run Delta",
            "",
            "| namespace | denominator | bad | dirty_good | good | dirty_good_rate | decision_join_rate |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in report["run_results"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['namespace']}`",
                    fmt(row["buy_quality_denominator_rows"]),
                    fmt(row["buy_quality_bad"]),
                    fmt(row["buy_quality_dirty_good"]),
                    fmt(row["buy_quality_good"]),
                    fmt(row["dirty_good_rate"]),
                    fmt(row["analysis"]["decision_join_rate"]),
                ]
            )
            + " |"
        )
    combined = report["combined_allowed_subset"]
    delta = report["policy_delta"]
    lines.extend(
        [
            "",
            "## Combined Allowed Subset",
            "",
            f"- buy_quality_denominator_rows: `{combined['buy_quality_denominator_rows']}`",
            f"- buy_quality_bad: `{combined['buy_quality_bad']}`",
            f"- buy_quality_dirty_good: `{combined['buy_quality_dirty_good']}`",
            f"- buy_quality_good: `{combined['buy_quality_good']}`",
            f"- dirty_good_rate: `{fmt(combined['dirty_good_rate'])}`",
            "",
            "## Policy Delta",
            "",
            f"- status: `{delta.get('status')}`",
            f"- note: {delta.get('note')}",
            f"- dirty_good_delta: `{delta.get('dirty_good_delta')}`",
            f"- bad_delta: `{delta.get('bad_delta')}`",
            f"- dirty_good_rate_delta: `{fmt(delta.get('dirty_good_rate_delta'))}`",
            f"- l2b_candidate_axes: `{json.dumps(delta.get('l2b_candidate_axes', []), sort_keys=True)}`",
        ]
    )
    for row in report["run_results"]:
        render_distribution_section(
            lines,
            f"Distributions: {row['namespace']}",
            row["analysis"]["distributions"],
        )
        lines.extend(["", f"### Feature Availability: {row['namespace']}", ""])
        for item in row["feature_availability"]["files"]:
            minimums = item.get("diagnostic_minimums", {})
            lines.append(
                "- "
                + f"`{item['path']}` status=`{item.get('feature_availability_status')}` "
                + f"rows_total=`{item.get('rows_total')}` "
                + f"dirty_good_with_features=`{minimums.get('dirty_good_with_features')}` "
                + f"bad_with_features=`{minimums.get('bad_with_features')}`"
            )
    lines.extend(["", "## Non-Goals", ""])
    for non_goal in report["non_goals"]:
        lines.append(f"- `{non_goal}`")
    return "\n".join(lines) + "\n"


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


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
