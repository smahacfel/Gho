#!/usr/bin/env python3
"""Assert offline Gatekeeper edge policy-fork validation gates.

This guard validates an already-built ``gatekeeper_edge_policy_fork_v1.json``.
It is intentionally read-only: it does not start runtime, change Gatekeeper,
alter execution, tune thresholds, or promote the fork to production behavior.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ARTIFACT = "gatekeeper_edge_policy_fork_v1"
DEFAULT_REPORT_NAME = f"{ARTIFACT}.json"
EXPECTED_BUSINESS_DECISION = "OFFLINE_POLICY_FORK_EDGE_FOUND_REQUIRES_SHADOW_VALIDATION"
REQUIRED_STATUSES = (
    "GK_EDGE_POLICY_FORK_OFFLINE_ONLY",
    "GK_EDGE_POLICY_FORK_R2_OPPORTUNITY_CONFIRMED_OFFLINE",
    "GK_EDGE_POLICY_FORK_R2_OPPORTUNITY_NOT_EXECUTION_SAFE",
    "GK_EDGE_POLICY_FORK_REQUIRES_FRESH_VALIDATION",
    "GK_EDGE_POLICY_FORK_NO_RUNTIME_GO",
)
FORBIDDEN_STATUSES = (
    "GK_EDGE_POLICY_FORK_NO_STABLE_R2_OPPORTUNITY",
    "GK_EDGE_POLICY_FORK_JOIN_SCOPE_MISMATCH_WARNING",
)
LABEL_COVERAGE_WARNING = "GK_EDGE_POLICY_FORK_LABEL_COVERAGE_WARNING"
NON_CLAIM_FIELDS = (
    "runtime_changed",
    "gatekeeper_changed",
    "execution_changed",
    "send_path_changed",
    "thresholds_tuned",
    "production_promotion_allowed",
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", default=None, help="Selector scope under reports/selector/<scope>.")
    parser.add_argument("--report", default=None, help="Explicit gatekeeper_edge_policy_fork_v1.json path.")
    parser.add_argument("--min-opportunity-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-opportunity-resolved-rows", type=int, default=100)
    parser.add_argument("--min-opportunity-precision", type=float, default=0.0)
    parser.add_argument("--min-opportunity-label-coverage", type=float, default=0.20)
    parser.add_argument(
        "--accept-label-coverage-warning",
        action="store_true",
        help="Allow low label coverage when the report explicitly marks the limitation.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def report_path(args: argparse.Namespace) -> Path:
    if args.report:
        return Path(args.report)
    if not args.scope:
        raise SystemExit("--scope or --report is required")
    return Path(args.root) / "reports" / "selector" / str(args.scope) / DEFAULT_REPORT_NAME


def read_report(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def as_float(value: Any) -> float | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return float(int(value))
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        return float(value)
    return None


def as_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(round(value))
    if isinstance(value, str):
        return int(float(value))
    return None


def validate_report(report: dict[str, Any], path: Path, args: argparse.Namespace) -> dict[str, Any]:
    fail_reasons: list[str] = []
    statuses = set(str(status) for status in report.get("policy_fork_statuses") or [])
    metrics = report.get("global_metrics") if isinstance(report.get("global_metrics"), dict) else {}
    non_claims = report.get("non_claims") if isinstance(report.get("non_claims"), dict) else {}
    artifact = report.get("artifact")
    business_decision = report.get("business_decision")

    if artifact != ARTIFACT:
        fail_reasons.append(f"unexpected_artifact:{artifact}")
    if report.get("status") != "PASS":
        fail_reasons.append(f"report_status_not_pass:{report.get('status')}")
    if business_decision != EXPECTED_BUSINESS_DECISION:
        fail_reasons.append(f"unexpected_business_decision:{business_decision}")

    for status in REQUIRED_STATUSES:
        if status not in statuses:
            fail_reasons.append(f"missing_required_status:{status}")
    for status in FORBIDDEN_STATUSES:
        if status in statuses:
            fail_reasons.append(f"forbidden_status:{status}")

    label_warning_present = LABEL_COVERAGE_WARNING in statuses
    if label_warning_present and not args.accept_label_coverage_warning:
        fail_reasons.append("label_coverage_warning_requires_explicit_acceptance")

    lift = as_float(metrics.get("policy_fork_would_allow_lift_vs_base_rate_pp"))
    resolved_rows = as_int(metrics.get("policy_fork_would_allow_resolved_rows"))
    precision = as_float(metrics.get("policy_fork_would_allow_precision"))
    label_coverage = as_float(metrics.get("policy_fork_would_allow_label_coverage"))

    if lift is None:
        fail_reasons.append("missing_policy_fork_would_allow_lift")
    elif lift < float(args.min_opportunity_lift_pp):
        fail_reasons.append(
            f"policy_fork_would_allow_lift_too_low:{lift:.6f}<"
            f"{float(args.min_opportunity_lift_pp):.6f}"
        )
    if resolved_rows is None:
        fail_reasons.append("missing_policy_fork_would_allow_resolved_rows")
    elif resolved_rows < int(args.min_opportunity_resolved_rows):
        fail_reasons.append(
            f"policy_fork_would_allow_resolved_rows_too_low:{resolved_rows}<"
            f"{int(args.min_opportunity_resolved_rows)}"
        )
    if precision is None:
        fail_reasons.append("missing_policy_fork_would_allow_precision")
    elif precision < float(args.min_opportunity_precision):
        fail_reasons.append(
            f"policy_fork_would_allow_precision_too_low:{precision:.6f}<"
            f"{float(args.min_opportunity_precision):.6f}"
        )
    if label_coverage is None:
        fail_reasons.append("missing_policy_fork_would_allow_label_coverage")
    elif label_coverage < float(args.min_opportunity_label_coverage) and not args.accept_label_coverage_warning:
        fail_reasons.append(
            f"policy_fork_would_allow_label_coverage_too_low:{label_coverage:.6f}<"
            f"{float(args.min_opportunity_label_coverage):.6f}"
        )

    for field in NON_CLAIM_FIELDS:
        value = non_claims.get(field)
        if value is not False:
            fail_reasons.append(f"non_claim_not_false:{field}={value!r}")

    result = {
        "artifact": "gatekeeper_edge_policy_fork_ci_gate_v1",
        "status": "FAIL" if fail_reasons else "PASS",
        "scope": report.get("selector_scope") or args.scope,
        "report_path": str(path),
        "business_decision": business_decision,
        "thresholds": {
            "min_opportunity_lift_pp": float(args.min_opportunity_lift_pp),
            "min_opportunity_resolved_rows": int(args.min_opportunity_resolved_rows),
            "min_opportunity_precision": float(args.min_opportunity_precision),
            "min_opportunity_label_coverage": float(args.min_opportunity_label_coverage),
            "accept_label_coverage_warning": bool(args.accept_label_coverage_warning),
        },
        "metrics": {
            "base_positive_rate": metrics.get("base_positive_rate"),
            "policy_fork_would_allow_precision": precision,
            "policy_fork_would_allow_lift_vs_base_rate_pp": lift,
            "policy_fork_would_allow_resolved_rows": resolved_rows,
            "policy_fork_would_allow_label_coverage": label_coverage,
            "policy_fork_would_allow_rows": metrics.get("policy_fork_would_allow_rows"),
            "resolved_rows": metrics.get("resolved_rows"),
        },
        "label_coverage_warning_accepted": bool(label_warning_present and args.accept_label_coverage_warning),
        "fail_reasons": fail_reasons,
    }
    return result


def main() -> int:
    args = build_parser().parse_args()
    path = report_path(args)
    report = read_report(path)
    result = validate_report(report, path, args)
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    elif result["status"] == "PASS":
        print(f"PASS {path}")
    else:
        print(f"FAIL {path}: {', '.join(result['fail_reasons'])}")
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
