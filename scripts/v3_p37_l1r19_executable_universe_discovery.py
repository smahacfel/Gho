#!/usr/bin/env python3
"""P3.7-L1R19 executable-universe discovery across J4C/R16/L1 artifacts.

This is an offline denominator audit. It does not start runtime, rebuild labels,
or change policy. Older artifacts may not carry L1R18 execution-feasibility
fields, so successful entries and lifecycle labels are treated as positive
execution evidence and marked as inferred.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

import v3_p37_mfs_lifecycle_join_key_audit as join_audit


SCHEMA_VERSION = 1
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/"
    "RAPORT_P3_7_L1R19_EXECUTABLE_UNIVERSE_DISCOVERY_ROUTE_SUPPORT_DECISION_20260523.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/"
    "RAPORT_P3_7_L1R19_EXECUTABLE_UNIVERSE_DISCOVERY_ROUTE_SUPPORT_DECISION_20260523.md"
)

DEFAULT_CONFIGS = (
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-route-bcv2-source.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r12-bcv2-provenance.toml",
    "configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver.toml",
)

PROBE_LABEL_NAMES = (
    "probe_p3_7_shadow_lifecycle_labels.jsonl",
    "p3_7_probe_shadow_lifecycle_labels.jsonl",
)
ACTIVE_LABEL_NAMES = ("active_p3_7_shadow_lifecycle_labels.jsonl",)
COMBINED_LABEL_NAMES = (
    "p3_7_shadow_lifecycle_labels.jsonl",
    "r16_r4_shadow_lifecycle_labels.jsonl",
    "shadow_lifecycle_labels.jsonl",
)
FEATURE_AVAILABILITY_NAMES = (
    "p3_7_shadow_lifecycle_feature_availability.json",
    "p3_7_probe_shadow_lifecycle_feature_availability.json",
    "r16_r4_shadow_lifecycle_feature_availability.json",
    "shadow_lifecycle_feature_availability.json",
)


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    yield from join_audit.iter_jsonl(path)


def read_json(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def count_rows(paths: list[Path]) -> int:
    return sum(1 for path in paths for _ in iter_jsonl(path))


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def ratio(numerator: int, denominator: int) -> float | None:
    if denominator <= 0:
        return None
    return numerator / denominator


def first_existing(run_dir: Path, names: tuple[str, ...]) -> Path | None:
    for name in names:
        path = run_dir / name
        if path.exists():
            return path
    return None


def selected_label_files(run_dir: Path) -> list[Path]:
    probe = first_existing(run_dir, PROBE_LABEL_NAMES)
    active = first_existing(run_dir, ACTIVE_LABEL_NAMES)
    if probe or active:
        return [path for path in (probe, active) if path is not None]
    combined = first_existing(run_dir, COMBINED_LABEL_NAMES)
    return [combined] if combined is not None else []


def load_labels(run_dir: Path) -> tuple[list[dict[str, Any]], list[str]]:
    paths = selected_label_files(run_dir)
    rows = [row for path in paths for row in iter_jsonl(path)]
    return rows, [str(path) for path in paths]


def is_execution_feasibility_reject_label(row: dict[str, Any]) -> bool:
    status = str(row.get("execution_feasibility_status") or "")
    return (
        row.get("buy_quality_class") == "buy_quality_not_executable"
        or status.startswith("not_executable")
    )


def label_metrics(run_dir: Path) -> dict[str, Any]:
    rows, paths = load_labels(run_dir)
    counts = Counter(str(row.get("buy_quality_class") or "unknown") for row in rows)
    executable_rows = [row for row in rows if not is_execution_feasibility_reject_label(row)]
    return {
        "label_files": paths,
        "lifecycle_labeled_rows": len(rows),
        "buy_quality_denominator_rows": len(executable_rows),
        "buy_quality_bad": counts.get("buy_quality_bad", 0),
        "buy_quality_dirty_good": counts.get("buy_quality_dirty_good", 0),
        "buy_quality_good": counts.get("buy_quality_good", 0),
        "buy_quality_not_executable": counts.get("buy_quality_not_executable", 0),
        "buy_quality_class_counts": counter_dict(counts),
    }


def feature_join_executable_labeled_rows(run_dir: Path, fallback_denominator: int) -> tuple[int, list[str]]:
    values: list[int] = []
    paths: list[str] = []
    for name in FEATURE_AVAILABILITY_NAMES:
        path = run_dir / name
        payload = read_json(path)
        if not payload:
            continue
        paths.append(str(path))
        denominator = payload.get("buy_quality_denominator_rows")
        if isinstance(denominator, int):
            values.append(denominator)
            continue
        rows_total = payload.get("rows_total")
        rejects = payload.get("execution_feasibility_reject_rows")
        if isinstance(rows_total, int):
            if isinstance(rejects, int):
                values.append(max(0, rows_total - rejects))
            else:
                values.append(rows_total)
    if not values:
        return 0, paths
    return min(max(values), fallback_denominator), paths


def run_dir_from_paths(paths: dict[str, list[Path]]) -> Path | None:
    for key in (
        "probe_selection",
        "probe_transport",
        "probe_entry",
        "shadow_transport",
        "shadow_entry",
        "shadow_lifecycle",
    ):
        values = paths.get(key, [])
        if values:
            return values[0].parent
    return None


def config_namespace(config_path: Path, paths: dict[str, list[Path]]) -> str:
    try:
        resolved = join_audit.resolve_config_path(config_path)
        config = join_audit.load_toml(resolved)
        namespace = config.get("p37_shadow_probe", {}).get("namespace")
        if namespace:
            return str(namespace)
    except Exception:
        pass
    run_dir = run_dir_from_paths(paths)
    return run_dir.name if run_dir else config_path.stem


def execution_feasibility_from_join(join_report: dict[str, Any]) -> dict[str, Any]:
    execution = join_report.get("execution_feasibility", {})
    return {
        "decision_rows_total": int(execution.get("decision_rows_total") or 0),
        "probe_selected_rows": int(execution.get("probe_selected_rows") or 0),
        "route_executable_rows_native": int(execution.get("route_executable_rows") or 0),
        "route_non_executable_rows": int(execution.get("route_non_executable_rows") or 0),
        "execution_feasibility_reject_rows": int(
            execution.get("execution_feasibility_reject_rows") or 0
        ),
        "active_buy_execution_infeasible_rows": int(
            execution.get("active_buy_execution_infeasible_rows") or 0
        ),
        "successful_entry_rows_native": int(execution.get("successful_entry_rows") or 0),
        "lifecycle_eligible_rows_native": int(execution.get("lifecycle_eligible_rows") or 0),
        "probe_execution_feasibility_status_counts": execution.get(
            "probe_execution_feasibility_status_counts",
            {},
        ),
        "active_shadow_execution_feasibility_status_counts": execution.get(
            "active_shadow_execution_feasibility_status_counts",
            {},
        ),
    }


def build_run_summary(config_path: Path) -> dict[str, Any]:
    resolved = join_audit.resolve_config_path(config_path)
    paths = join_audit.resolve_paths(resolved)
    run_dir = run_dir_from_paths(paths)
    if run_dir is None:
        return {
            "config_path": str(resolved),
            "namespace": resolved.stem,
            "audit_evidence_status": "audit_gap",
            "audit_gap_reasons": ["no_shadow_run_artifact_paths_resolved"],
        }

    join_report = join_audit.build_report(resolved)
    execution = execution_feasibility_from_join(join_report)
    labels = label_metrics(run_dir)
    feature_join_rows, feature_paths = feature_join_executable_labeled_rows(
        run_dir,
        labels["buy_quality_denominator_rows"],
    )

    active_buy_rows = count_rows(paths.get("shadow_transport", []))
    simulation_error_entry_rows = int(
        join_report.get("probe_entry_materialization", {}).get("simulation_error_entry_rows", 0)
    ) + int(
        join_report.get("active_shadow_dispatch_diagnostics", {}).get(
            "active_shadow_runtime_simulation_error_rows",
            0,
        )
    )

    successful_entry_rows = max(
        execution["successful_entry_rows_native"],
        labels["lifecycle_labeled_rows"],
    )
    lifecycle_eligible_rows = max(
        execution["lifecycle_eligible_rows_native"],
        labels["lifecycle_labeled_rows"],
    )
    route_executable_rows = execution["route_executable_rows_native"]
    executable_evidence = "native_execution_feasibility"
    if route_executable_rows == 0 and successful_entry_rows > 0:
        route_executable_rows = successful_entry_rows
        executable_evidence = "inferred_from_successful_entry_or_lifecycle_label"

    candidate_denominator = execution["probe_selected_rows"] + active_buy_rows
    lifecycle_labeled_rows = labels["lifecycle_labeled_rows"]
    buy_quality_denominator_rows = labels["buy_quality_denominator_rows"]

    gap_reasons: list[str] = []
    if candidate_denominator > 0 and route_executable_rows == 0 and execution["route_non_executable_rows"] == 0:
        gap_reasons.append("no_execution_feasibility_or_successful_entry_evidence")
    if (
        lifecycle_labeled_rows == 0
        and successful_entry_rows == 0
        and candidate_denominator > 0
        and execution["route_non_executable_rows"] == 0
        and execution["execution_feasibility_reject_rows"] == 0
    ):
        gap_reasons.append("no_lifecycle_label_or_successful_entry_evidence")

    if route_executable_rows > 0 and lifecycle_labeled_rows > 0 and buy_quality_denominator_rows > 0:
        audit_evidence_status = "usable_executable_labeled_subset"
    elif route_executable_rows > 0 and lifecycle_labeled_rows == 0:
        audit_evidence_status = "executable_without_lifecycle_labels"
    elif gap_reasons:
        audit_evidence_status = "audit_gap"
    elif route_executable_rows == 0 and candidate_denominator > 0:
        audit_evidence_status = "execution_route_support_blocked"
    else:
        audit_evidence_status = "empty_or_no_candidates"

    summary = {
        "namespace": config_namespace(resolved, paths),
        "config_path": str(resolved),
        "run_dir": str(run_dir),
        "audit_evidence_status": audit_evidence_status,
        "audit_gap_reasons": gap_reasons,
        "decision_rows_total": execution["decision_rows_total"],
        "probe_selected_rows": execution["probe_selected_rows"],
        "active_buy_rows": active_buy_rows,
        "route_executable_rows": route_executable_rows,
        "route_executable_evidence": executable_evidence,
        "route_non_executable_rows": execution["route_non_executable_rows"],
        "execution_feasibility_reject_rows": execution["execution_feasibility_reject_rows"],
        "active_buy_execution_infeasible_rows": execution[
            "active_buy_execution_infeasible_rows"
        ],
        "successful_entry_rows": successful_entry_rows,
        "simulation_error_entry_rows": simulation_error_entry_rows,
        "lifecycle_eligible_rows": lifecycle_eligible_rows,
        "lifecycle_labeled_rows": lifecycle_labeled_rows,
        "buy_quality_denominator_rows": buy_quality_denominator_rows,
        "buy_quality_bad": labels["buy_quality_bad"],
        "buy_quality_dirty_good": labels["buy_quality_dirty_good"],
        "buy_quality_good": labels["buy_quality_good"],
        "buy_quality_not_executable": labels["buy_quality_not_executable"],
        "feature_join_executable_labeled_rows": feature_join_rows,
        "execution_feasibility_rate": ratio(route_executable_rows, candidate_denominator),
        "entry_materialization_rate": ratio(successful_entry_rows, route_executable_rows),
        "lifecycle_label_rate": ratio(lifecycle_labeled_rows, successful_entry_rows),
        "usable_label_rate": ratio(buy_quality_denominator_rows, lifecycle_labeled_rows),
        "candidate_denominator_rows": candidate_denominator,
        "label_files": labels["label_files"],
        "feature_availability_files": feature_paths,
        "buy_quality_class_counts": labels["buy_quality_class_counts"],
        "probe_execution_feasibility_status_counts": execution[
            "probe_execution_feasibility_status_counts"
        ],
        "active_shadow_execution_feasibility_status_counts": execution[
            "active_shadow_execution_feasibility_status_counts"
        ],
    }
    return summary


def final_decision(run_summaries: list[dict[str, Any]]) -> tuple[str, list[str]]:
    usable = [
        row
        for row in run_summaries
        if row.get("route_executable_rows", 0) > 0
        and row.get("lifecycle_labeled_rows", 0) > 0
        and row.get("buy_quality_denominator_rows", 0) > 0
    ]
    if usable:
        return "GO_L2_EXECUTABLE_SUBSET", [
            "At least one existing run has executable lifecycle-labeled rows with a non-empty buy-quality denominator.",
            "L2 remains scoped to that executable labeled subset; non-executable rows stay in execution-feasibility reporting.",
        ]
    gaps = [row for row in run_summaries if row.get("audit_evidence_status") == "audit_gap"]
    if gaps:
        return "BLOCK_L2_AUDIT_GAP", [
            "No usable executable labeled subset was found and some runs lack enough execution/label evidence for a denominator claim.",
        ]
    return "BLOCK_L2_ROUTE_SUPPORT_REQUIRED", [
        "No existing run has usable executable lifecycle-labeled rows.",
        "Route support expansion or explicit executable-route scoping is required before L2.",
    ]


def build_discovery_report(config_paths: list[Path]) -> dict[str, Any]:
    existing_configs = [path for path in config_paths if path.exists()]
    missing_configs = [str(path) for path in config_paths if not path.exists()]
    runs = [build_run_summary(path) for path in existing_configs]
    decision, reasons = final_decision(runs)
    totals = {
        "configs_considered": len(existing_configs),
        "configs_missing": len(missing_configs),
        "runs_with_usable_executable_labeled_subset": sum(
            1 for row in runs if row["audit_evidence_status"] == "usable_executable_labeled_subset"
        ),
        "runs_execution_route_support_blocked": sum(
            1 for row in runs if row["audit_evidence_status"] == "execution_route_support_blocked"
        ),
        "runs_executable_without_lifecycle_labels": sum(
            1 for row in runs if row["audit_evidence_status"] == "executable_without_lifecycle_labels"
        ),
        "runs_with_audit_gap": sum(1 for row in runs if row["audit_evidence_status"] == "audit_gap"),
        "total_route_executable_rows": sum(row.get("route_executable_rows", 0) for row in runs),
        "total_lifecycle_labeled_rows": sum(row.get("lifecycle_labeled_rows", 0) for row in runs),
        "total_buy_quality_denominator_rows": sum(
            row.get("buy_quality_denominator_rows", 0) for row in runs
        ),
        "total_buy_quality_dirty_good": sum(row.get("buy_quality_dirty_good", 0) for row in runs),
        "total_buy_quality_good": sum(row.get("buy_quality_good", 0) for row in runs),
        "total_execution_feasibility_reject_rows": sum(
            row.get("execution_feasibility_reject_rows", 0) for row in runs
        ),
    }
    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L1R19 Executable Universe Discovery / Route Support Decision",
        "final_decision": decision,
        "final_decision_reasons": reasons,
        "totals": totals,
        "missing_configs": missing_configs,
        "runs": runs,
    }


def fmt(value: Any) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.4f}"
    return str(value)


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L1R19 Executable Universe Discovery / Route Support Decision",
        "",
        "## Verdict",
        "",
        f"- final_decision: `{report['final_decision']}`",
    ]
    for reason in report["final_decision_reasons"]:
        lines.append(f"- reason: {reason}")
    lines.extend(
        [
            "",
            "## Scope",
            "",
            "- Offline denominator audit only.",
            "- No runtime, no threshold changes, no route fallback implementation, no collection, no P2/live.",
            "- Successful entries/lifecycle labels in older artifacts are treated as inferred executable evidence when L1R18-native route fields are absent.",
            "",
            "## Totals",
            "",
        ]
    )
    for key, value in report["totals"].items():
        lines.append(f"- {key}: `{value}`")
    if report["missing_configs"]:
        lines.extend(["", "## Missing Configs", ""])
        for path in report["missing_configs"]:
            lines.append(f"- `{path}`")
    lines.extend(
        [
            "",
            "## Per-Run Denominators",
            "",
            "| namespace | status | decisions | probe_selected | active_buy | route_exec | route_non_exec | exec_reject | active_buy_infeasible | success_entry | sim_error_entry | lifecycle_eligible | lifecycle_labels | buy_denominator | bad | dirty_good | good | not_exec | feature_join_exec_labels | exec_rate | entry_rate | lifecycle_rate | usable_label_rate | evidence |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
        ]
    )
    for row in report["runs"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['namespace']}`",
                    f"`{row['audit_evidence_status']}`",
                    fmt(row.get("decision_rows_total")),
                    fmt(row.get("probe_selected_rows")),
                    fmt(row.get("active_buy_rows")),
                    fmt(row.get("route_executable_rows")),
                    fmt(row.get("route_non_executable_rows")),
                    fmt(row.get("execution_feasibility_reject_rows")),
                    fmt(row.get("active_buy_execution_infeasible_rows")),
                    fmt(row.get("successful_entry_rows")),
                    fmt(row.get("simulation_error_entry_rows")),
                    fmt(row.get("lifecycle_eligible_rows")),
                    fmt(row.get("lifecycle_labeled_rows")),
                    fmt(row.get("buy_quality_denominator_rows")),
                    fmt(row.get("buy_quality_bad")),
                    fmt(row.get("buy_quality_dirty_good")),
                    fmt(row.get("buy_quality_good")),
                    fmt(row.get("buy_quality_not_executable")),
                    fmt(row.get("feature_join_executable_labeled_rows")),
                    fmt(row.get("execution_feasibility_rate")),
                    fmt(row.get("entry_materialization_rate")),
                    fmt(row.get("lifecycle_label_rate")),
                    fmt(row.get("usable_label_rate")),
                    f"`{row.get('route_executable_evidence')}`",
                ]
            )
            + " |"
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- `GO_L2_EXECUTABLE_SUBSET` does not promote full R16 route support.",
            "- It only means historical artifacts contain a non-empty executable lifecycle-labeled denominator suitable for scoped L2 analysis.",
            "- Rows with `no_executable_route_account_set` remain outside buy-quality denominators and must stay in execution-feasibility reporting.",
        ]
    )
    return "\n".join(lines) + "\n"


def write_json(path: Path, report: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        action="append",
        dest="configs",
        help="Config path to include. Defaults to known J4C/R16/L1 configs.",
    )
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--json", action="store_true", help="Print JSON report to stdout.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    config_paths = [Path(path) for path in (args.configs or DEFAULT_CONFIGS)]
    report = build_discovery_report(config_paths)
    write_json(args.output_json, report)
    write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
