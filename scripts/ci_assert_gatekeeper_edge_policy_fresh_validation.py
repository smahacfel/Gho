#!/usr/bin/env python3
"""Assert fresh-validation readiness for the Gatekeeper edge policy fork.

This is a second-level guard over ``gatekeeper_edge_policy_fork_v1.json``
reports.  Existing discovery runs may support the hypothesis, but promotion
requires a separate fresh-validation scope.  The script is read-only and does
not modify runtime, Gatekeeper policy, execution, send path, configs, or
thresholds.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import ci_assert_gatekeeper_edge_policy_fork as fork_gate


ARTIFACT = "gatekeeper_edge_policy_fresh_validation_gate_v1"
PASS_DECISION = "EDGE_POLICY_FORK_READY_FOR_CONFIG_GATED_POLICY_REVIEW"
FAIL_DECISION = "EDGE_POLICY_FORK_NO_GO_FRESH_VALIDATION_MISSING_OR_FAILED"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", action="append", default=[], help="Selector scope report to validate.")
    parser.add_argument("--report", action="append", default=[], help="Explicit gatekeeper_edge_policy_fork_v1.json path.")
    parser.add_argument(
        "--discovery-scope",
        action="append",
        default=[],
        help="Scope used for discovery/backtest; it cannot satisfy fresh validation.",
    )
    parser.add_argument("--fresh-validation-scope", default=None)
    parser.add_argument("--min-pass-reports", type=int, default=3)
    parser.add_argument("--min-supporting-pass-reports", type=int, default=2)
    parser.add_argument("--min-opportunity-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-opportunity-resolved-rows", type=int, default=100)
    parser.add_argument("--min-opportunity-precision", type=float, default=0.0)
    parser.add_argument("--min-opportunity-label-coverage", type=float, default=0.20)
    parser.add_argument("--accept-label-coverage-warning", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser


def report_path_for_scope(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / fork_gate.DEFAULT_REPORT_NAME


def report_inputs(args: argparse.Namespace) -> list[Path]:
    root = Path(args.root)
    paths = [report_path_for_scope(root, scope) for scope in args.scope]
    paths.extend(Path(path) for path in args.report)
    return list(dict.fromkeys(paths))


def gate_args(args: argparse.Namespace, scope: str | None) -> argparse.Namespace:
    return argparse.Namespace(
        root=args.root,
        scope=scope,
        report=None,
        min_opportunity_lift_pp=args.min_opportunity_lift_pp,
        min_opportunity_resolved_rows=args.min_opportunity_resolved_rows,
        min_opportunity_precision=args.min_opportunity_precision,
        min_opportunity_label_coverage=args.min_opportunity_label_coverage,
        accept_label_coverage_warning=args.accept_label_coverage_warning,
        json=False,
    )


def validate_path(path: Path, args: argparse.Namespace) -> dict[str, Any]:
    try:
        report = fork_gate.read_report(path)
    except FileNotFoundError:
        return {
            "status": "FAIL",
            "scope": None,
            "report_path": str(path),
            "fail_reasons": [f"report_missing:{path}"],
        }
    except json.JSONDecodeError as exc:
        return {
            "status": "FAIL",
            "scope": None,
            "report_path": str(path),
            "fail_reasons": [f"report_invalid_json:{path}:{exc}"],
        }
    scope = str(report.get("selector_scope") or "")
    return fork_gate.validate_report(report, path, gate_args(args, scope or None))


def validate(args: argparse.Namespace) -> dict[str, Any]:
    fail_reasons: list[str] = []
    paths = report_inputs(args)
    if not paths:
        fail_reasons.append("no_policy_fork_reports_provided")

    reports = [validate_path(path, args) for path in paths]
    pass_reports = [report for report in reports if report.get("status") == "PASS"]
    pass_scopes = {str(report.get("scope") or "") for report in pass_reports}
    discovery_scopes = set(str(scope) for scope in args.discovery_scope)
    fresh_scope = str(args.fresh_validation_scope or "")
    fresh_report = next((report for report in reports if str(report.get("scope") or "") == fresh_scope), None)
    supporting_pass_reports = [
        report
        for report in pass_reports
        if str(report.get("scope") or "") != fresh_scope
    ]

    for report in reports:
        if report.get("status") != "PASS":
            scope = report.get("scope") or report.get("report_path")
            for reason in report.get("fail_reasons") or []:
                fail_reasons.append(f"policy_fork_report_failed:{scope}:{reason}")

    if not fresh_scope:
        fail_reasons.append("missing_fresh_validation_scope")
    elif fresh_scope in discovery_scopes:
        fail_reasons.append(f"fresh_validation_scope_overlaps_discovery_scope:{fresh_scope}")
    elif fresh_report is None:
        fail_reasons.append(f"fresh_validation_report_missing:{fresh_scope}")
    elif fresh_report.get("status") != "PASS":
        fail_reasons.append(f"fresh_validation_report_failed:{fresh_scope}")

    if len(pass_reports) < int(args.min_pass_reports):
        fail_reasons.append(f"pass_report_count_too_low:{len(pass_reports)}<{int(args.min_pass_reports)}")
    if len(supporting_pass_reports) < int(args.min_supporting_pass_reports):
        fail_reasons.append(
            f"supporting_pass_report_count_too_low:{len(supporting_pass_reports)}<"
            f"{int(args.min_supporting_pass_reports)}"
        )
    if fresh_scope and fresh_scope not in pass_scopes:
        fail_reasons.append(f"fresh_validation_scope_not_passed:{fresh_scope}")

    status = "FAIL" if fail_reasons else "PASS"
    return {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": PASS_DECISION if status == "PASS" else FAIL_DECISION,
        "fresh_validation_scope": fresh_scope or None,
        "discovery_scopes": sorted(discovery_scopes),
        "pass_report_count": len(pass_reports),
        "supporting_pass_report_count": len(supporting_pass_reports),
        "validated_reports": reports,
        "thresholds": {
            "min_pass_reports": int(args.min_pass_reports),
            "min_supporting_pass_reports": int(args.min_supporting_pass_reports),
            "min_opportunity_lift_pp": float(args.min_opportunity_lift_pp),
            "min_opportunity_resolved_rows": int(args.min_opportunity_resolved_rows),
            "min_opportunity_precision": float(args.min_opportunity_precision),
            "min_opportunity_label_coverage": float(args.min_opportunity_label_coverage),
            "accept_label_coverage_warning": bool(args.accept_label_coverage_warning),
        },
        "non_claims": {
            "runtime_changed": False,
            "gatekeeper_changed": False,
            "execution_changed": False,
            "send_path_changed": False,
            "thresholds_tuned": False,
            "production_promotion_allowed": False,
        },
        "fail_reasons": fail_reasons,
    }


def main() -> int:
    args = build_parser().parse_args()
    result = validate(args)
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    elif result["status"] == "PASS":
        print(f"PASS {result['fresh_validation_scope']}")
    else:
        print(f"FAIL: {', '.join(result['fail_reasons'])}")
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
