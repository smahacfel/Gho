#!/usr/bin/env python3
"""P3.7-L1R20 fail-closed preflight for L2 executable-subset inputs.

L1R19 found a historical executable labeled subset, but that is not permission
to run L2 over the full R16 route universe. This preflight locks L2 inputs to
explicitly allowed historical namespaces and reports every excluded denominator.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import v3_p37_l1r19_executable_universe_discovery as l1r19


SCHEMA_VERSION = 1
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/"
    "RAPORT_P3_7_L1R20_L2_EXECUTABLE_SUBSET_PREFLIGHT_20260524.md"
)
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/"
    "RAPORT_P3_7_L1R20_L2_EXECUTABLE_SUBSET_PREFLIGHT_20260524.json"
)

DEFAULT_ALLOWED_L2_NAMESPACES = (
    "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1",
)

DEFAULT_HARD_BLOCKED_NAMESPACES = (
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-route-bcv2-source",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r12-bcv2-provenance",
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver",
)


def ratio(numerator: int, denominator: int) -> float | None:
    if denominator <= 0:
        return None
    return numerator / denominator


def index_runs(discovery_report: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {str(run["namespace"]): run for run in discovery_report.get("runs", [])}


def sum_field(runs: list[dict[str, Any]], field: str) -> int:
    return sum(int(run.get(field) or 0) for run in runs)


def classify_exclusion(
    run: dict[str, Any],
    *,
    allowed_namespaces: set[str],
    hard_blocked_namespaces: set[str],
) -> str:
    namespace = str(run.get("namespace") or "")
    if namespace in allowed_namespaces:
        return "allowed_l2_input"
    if namespace in hard_blocked_namespaces:
        return "hard_blocked_unsupported_route_universe"
    if int(run.get("buy_quality_denominator_rows") or 0) <= 0:
        return "excluded_no_buy_quality_denominator"
    return "excluded_not_in_allowed_l2_universe"


def build_preflight_report_from_discovery(
    discovery_report: dict[str, Any],
    *,
    l2_input_namespaces: list[str] | None = None,
    allowed_namespaces: set[str] | None = None,
    hard_blocked_namespaces: set[str] | None = None,
    allow_unsupported_override: bool = False,
) -> dict[str, Any]:
    allowed = allowed_namespaces or set(DEFAULT_ALLOWED_L2_NAMESPACES)
    hard_blocked = hard_blocked_namespaces or set(DEFAULT_HARD_BLOCKED_NAMESPACES)
    requested_namespaces = list(l2_input_namespaces or DEFAULT_ALLOWED_L2_NAMESPACES)
    runs_by_namespace = index_runs(discovery_report)

    missing_requested = [ns for ns in requested_namespaces if ns not in runs_by_namespace]
    requested_runs = [runs_by_namespace[ns] for ns in requested_namespaces if ns in runs_by_namespace]
    blocked_requested = [
        run
        for run in requested_runs
        if str(run.get("namespace") or "") in hard_blocked
        and not allow_unsupported_override
    ]
    disallowed_requested = [
        run
        for run in requested_runs
        if str(run.get("namespace") or "") not in allowed
        and str(run.get("namespace") or "") not in hard_blocked
        and not allow_unsupported_override
    ]
    unusable_requested = [
        run
        for run in requested_runs
        if int(run.get("buy_quality_denominator_rows") or 0) <= 0
        and not allow_unsupported_override
    ]

    exclusions = []
    for run in discovery_report.get("runs", []):
        namespace = str(run.get("namespace") or "")
        if namespace in requested_namespaces and namespace in allowed:
            continue
        exclusion_class = classify_exclusion(
            run,
            allowed_namespaces=allowed,
            hard_blocked_namespaces=hard_blocked,
        )
        if exclusion_class == "allowed_l2_input":
            continue
        exclusions.append(
            {
                "namespace": namespace,
                "exclusion_class": exclusion_class,
                "decision_rows_total": int(run.get("decision_rows_total") or 0),
                "route_non_executable_rows": int(run.get("route_non_executable_rows") or 0),
                "execution_feasibility_reject_rows": int(
                    run.get("execution_feasibility_reject_rows") or 0
                ),
                "buy_quality_denominator_rows": int(
                    run.get("buy_quality_denominator_rows") or 0
                ),
                "dirty_good_rows": int(run.get("buy_quality_dirty_good") or 0),
            }
        )

    input_totals = {
        "total_rows": sum_field(requested_runs, "decision_rows_total"),
        "executable_eligible_rows": sum_field(requested_runs, "route_executable_rows"),
        "excluded_non_executable_rows_within_allowed_runs": sum_field(
            requested_runs,
            "route_non_executable_rows",
        ),
        "buy_quality_denominator_rows": sum_field(requested_runs, "buy_quality_denominator_rows"),
        "buy_quality_bad": sum_field(requested_runs, "buy_quality_bad"),
        "buy_quality_dirty_good": sum_field(requested_runs, "buy_quality_dirty_good"),
        "buy_quality_good": sum_field(requested_runs, "buy_quality_good"),
        "buy_quality_not_executable": sum_field(requested_runs, "buy_quality_not_executable"),
        "lifecycle_labeled_rows": sum_field(requested_runs, "lifecycle_labeled_rows"),
        "feature_join_executable_labeled_rows": sum_field(
            requested_runs,
            "feature_join_executable_labeled_rows",
        ),
    }
    input_totals["dirty_good_rate"] = ratio(
        input_totals["buy_quality_dirty_good"],
        input_totals["buy_quality_denominator_rows"],
    )
    input_totals["usable_label_rate"] = ratio(
        input_totals["buy_quality_denominator_rows"],
        input_totals["lifecycle_labeled_rows"],
    )

    excluded_totals = {
        "excluded_runs": len(exclusions),
        "excluded_decision_rows_total": sum(item["decision_rows_total"] for item in exclusions),
        "excluded_non_executable_rows": sum(item["route_non_executable_rows"] for item in exclusions),
        "excluded_unsupported_route_rows": sum(
            item["execution_feasibility_reject_rows"] for item in exclusions
        ),
        "excluded_buy_quality_denominator_rows": sum(
            item["buy_quality_denominator_rows"] for item in exclusions
        ),
        "excluded_dirty_good_rows": sum(item["dirty_good_rows"] for item in exclusions),
    }

    blockers = []
    if missing_requested:
        blockers.append("requested_l2_namespace_missing_from_discovery_report")
    if blocked_requested:
        blockers.append("requested_l2_namespace_is_hard_blocked")
    if disallowed_requested:
        blockers.append("requested_l2_namespace_not_allowed")
    if unusable_requested:
        blockers.append("requested_l2_namespace_has_no_buy_quality_denominator")
    if input_totals["buy_quality_denominator_rows"] <= 0:
        blockers.append("l2_buy_quality_denominator_empty")

    preflight_status = "pass" if not blockers else "fail"
    final_decision = (
        "GO_L2_EXECUTABLE_SUBSET_LOCKED"
        if preflight_status == "pass"
        else "BLOCK_L2_INPUT_UNIVERSE_CONTRACT"
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L1R20 L2 Executable-Subset Preflight",
        "source_l1r19_decision": discovery_report.get("final_decision"),
        "preflight_status": preflight_status,
        "final_decision": final_decision,
        "blockers": blockers,
        "allowed_l2_namespaces": sorted(allowed),
        "requested_l2_namespaces": requested_namespaces,
        "missing_requested_l2_namespaces": missing_requested,
        "blocked_requested_l2_namespaces": [
            str(run.get("namespace") or "") for run in blocked_requested
        ],
        "disallowed_requested_l2_namespaces": [
            str(run.get("namespace") or "") for run in disallowed_requested
        ],
        "unusable_requested_l2_namespaces": [
            str(run.get("namespace") or "") for run in unusable_requested
        ],
        "input_totals": input_totals,
        "excluded_totals": excluded_totals,
        "excluded_runs": exclusions,
        "l2_input_runs": requested_runs,
        "override_used": allow_unsupported_override,
        "guardrails": [
            "GO_L2_EXECUTABLE_SUBSET is not GO_LIVE_POLICY.",
            "GO_L2_EXECUTABLE_SUBSET is not GO_FULL_R16_ROUTE_UNIVERSE.",
            "Non-executable rows remain outside buy-quality denominators.",
            "Hard-blocked R16-r3..r13 namespaces cannot enter L2 without explicit override.",
        ],
    }


def build_preflight_report(
    config_paths: list[Path],
    *,
    l2_input_namespaces: list[str] | None = None,
    allow_unsupported_override: bool = False,
) -> dict[str, Any]:
    discovery = l1r19.build_discovery_report(config_paths)
    return build_preflight_report_from_discovery(
        discovery,
        l2_input_namespaces=l2_input_namespaces,
        allow_unsupported_override=allow_unsupported_override,
    )


def fmt(value: Any) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.4f}"
    return str(value)


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L1R20 L2 Executable-Subset Preflight",
        "",
        "## Verdict",
        "",
        f"- source_l1r19_decision: `{report['source_l1r19_decision']}`",
        f"- preflight_status: `{report['preflight_status']}`",
        f"- final_decision: `{report['final_decision']}`",
        f"- override_used: `{report['override_used']}`",
    ]
    if report["blockers"]:
        for blocker in report["blockers"]:
            lines.append(f"- blocker: `{blocker}`")
    lines.extend(
        [
            "",
            "## Scope Lock",
            "",
            "- L2 input universe is restricted to historical executable lifecycle-labeled namespaces.",
            "- This report does not change scoring, thresholds, route fallback, live/P2, IWIM, or Gatekeeper policy.",
            "- Full R16 route universe remains blocked unless explicitly overridden outside this default preflight.",
            "",
            "## Allowed L2 Namespaces",
            "",
        ]
    )
    for namespace in report["allowed_l2_namespaces"]:
        lines.append(f"- `{namespace}`")
    lines.extend(["", "## Input Totals", ""])
    for key, value in report["input_totals"].items():
        lines.append(f"- {key}: `{fmt(value)}`")
    lines.extend(["", "## Excluded Totals", ""])
    for key, value in report["excluded_totals"].items():
        lines.append(f"- {key}: `{fmt(value)}`")
    lines.extend(
        [
            "",
            "## Requested L2 Inputs",
            "",
            "| namespace | decisions | route_exec | route_non_exec | lifecycle_labels | buy_denominator | bad | dirty_good | good | not_exec |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for run in report["l2_input_runs"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{run['namespace']}`",
                    fmt(run.get("decision_rows_total")),
                    fmt(run.get("route_executable_rows")),
                    fmt(run.get("route_non_executable_rows")),
                    fmt(run.get("lifecycle_labeled_rows")),
                    fmt(run.get("buy_quality_denominator_rows")),
                    fmt(run.get("buy_quality_bad")),
                    fmt(run.get("buy_quality_dirty_good")),
                    fmt(run.get("buy_quality_good")),
                    fmt(run.get("buy_quality_not_executable")),
                ]
            )
            + " |"
        )
    lines.extend(
        [
            "",
            "## Excluded Runs",
            "",
            "| namespace | class | decisions | route_non_exec | unsupported_route | buy_denominator | dirty_good |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in report["excluded_runs"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['namespace']}`",
                    f"`{row['exclusion_class']}`",
                    fmt(row["decision_rows_total"]),
                    fmt(row["route_non_executable_rows"]),
                    fmt(row["execution_feasibility_reject_rows"]),
                    fmt(row["buy_quality_denominator_rows"]),
                    fmt(row["dirty_good_rows"]),
                ]
            )
            + " |"
        )
    lines.extend(["", "## Guardrails", ""])
    for guardrail in report["guardrails"]:
        lines.append(f"- {guardrail}")
    return "\n".join(lines) + "\n"


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        action="append",
        dest="configs",
        help="Discovery config path. Defaults to L1R19 J4C/R16/L1 config set.",
    )
    parser.add_argument(
        "--l2-input-namespace",
        action="append",
        dest="l2_input_namespaces",
        help="Namespace proposed as L2 input. Defaults to J4C + R16-r1.",
    )
    parser.add_argument(
        "--allow-unsupported-override",
        action="store_true",
        help="Explicitly allow non-default namespaces. Default fails closed.",
    )
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    config_paths = [Path(path) for path in (args.configs or l1r19.DEFAULT_CONFIGS)]
    report = build_preflight_report(
        config_paths,
        l2_input_namespaces=args.l2_input_namespaces,
        allow_unsupported_override=args.allow_unsupported_override,
    )
    write_json(args.output_json, report)
    write_text(args.output_md, render_markdown(report))
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    if report["preflight_status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
