#!/usr/bin/env python3
"""Assert fresh-validation readiness for the frozen BUY quality candidate.

The guard validates an existing ``gatekeeper_buy_quality_candidate_v1.json``
report.  It is intentionally read-only: it does not start runtime, change
Gatekeeper, alter execution, tune thresholds, or promote the candidate to
production behavior.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import analyze_gatekeeper_buy_quality_candidate as buy_quality


ARTIFACT = "gatekeeper_buy_quality_candidate_fresh_validation_gate_v1"
DEFAULT_REPORT_NAME = f"{buy_quality.ARTIFACT}.json"
PASS_DECISION = "BUY_QUALITY_CANDIDATE_READY_FOR_POLICY_REVIEW_AFTER_FRESH_VALIDATION"
FAIL_DECISION = "BUY_QUALITY_CANDIDATE_NO_GO_FRESH_VALIDATION_MISSING_OR_FAILED"
PASS_REPORT_STATUSES = {
    "BUY_QUALITY_CANDIDATE_READY_FOR_FRESH_VALIDATION",
    "BUY_QUALITY_CANDIDATE_PROMISING_LOW_VALIDATION_COVERAGE",
}
NON_CLAIM_FALSE_FIELDS = (
    "changes_runtime",
    "changes_gatekeeper",
    "changes_execution",
    "changes_send_path",
    "thresholds_tuned",
    "production_promotion_allowed",
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", default=None, help="Validation scope under reports/selector/<scope>.")
    parser.add_argument("--report", default=None, help="Explicit gatekeeper_buy_quality_candidate_v1.json path.")
    parser.add_argument(
        "--discovery-scope",
        action="append",
        default=[],
        help="Discovery/backtest scope. It cannot satisfy fresh validation.",
    )
    parser.add_argument("--fresh-validation-scope", default=None)
    parser.add_argument("--min-fresh-selected-resolved-rows", type=int, default=100)
    parser.add_argument("--min-support-selected-resolved-rows", type=int, default=75)
    parser.add_argument("--min-selected-precision", type=float, default=0.55)
    parser.add_argument("--min-lift-vs-base-pp", type=float, default=0.10)
    parser.add_argument("--min-lift-vs-current-buy-pp", type=float, default=0.10)
    parser.add_argument("--json", action="store_true")
    return parser


def report_path(args: argparse.Namespace) -> Path:
    if args.report:
        return Path(args.report)
    scope = args.scope or args.fresh_validation_scope
    if not scope:
        raise SystemExit("--scope, --fresh-validation-scope, or --report is required")
    return Path(args.root) / "reports" / "selector" / str(scope) / DEFAULT_REPORT_NAME


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


def close_enough(left: Any, right: float, tolerance: float = 1e-12) -> bool:
    value = as_float(left)
    return value is not None and abs(value - right) <= tolerance


def matrix_row(report: dict[str, Any], run: str) -> dict[str, Any]:
    matrix = report.get("matrix") if isinstance(report.get("matrix"), list) else []
    for row in matrix:
        if isinstance(row, dict) and row.get("run") == run:
            return row
    return {}


def fail_if_metric_below(
    fail_reasons: list[str],
    row: dict[str, Any],
    field: str,
    minimum: float,
    prefix: str,
) -> None:
    value = as_float(row.get(field))
    if value is None:
        fail_reasons.append(f"missing_{prefix}_{field}")
    elif value < minimum:
        fail_reasons.append(f"{prefix}_{field}_too_low:{value:.6f}<{minimum:.6f}")


def fail_if_count_below(
    fail_reasons: list[str],
    row: dict[str, Any],
    field: str,
    minimum: int,
    prefix: str,
) -> None:
    value = as_int(row.get(field))
    if value is None:
        fail_reasons.append(f"missing_{prefix}_{field}")
    elif value < minimum:
        fail_reasons.append(f"{prefix}_{field}_too_low:{value}<{minimum}")


def validate_report(report: dict[str, Any], path: Path, args: argparse.Namespace) -> dict[str, Any]:
    fail_reasons: list[str] = []
    candidate = report.get("candidate") if isinstance(report.get("candidate"), dict) else {}
    claims = report.get("claim_boundaries") if isinstance(report.get("claim_boundaries"), dict) else {}
    train = matrix_row(report, "train")
    fresh = matrix_row(report, "validation")
    discovery_scopes = set(str(scope) for scope in args.discovery_scope)
    fresh_scope = str(args.fresh_validation_scope or report.get("validation_scope") or "")
    train_scope = str(report.get("train_scope") or train.get("scope") or "")
    report_validation_scope = str(report.get("validation_scope") or fresh.get("scope") or "")

    if report.get("artifact") != buy_quality.ARTIFACT:
        fail_reasons.append(f"unexpected_artifact:{report.get('artifact')}")
    if report.get("status") not in PASS_REPORT_STATUSES:
        fail_reasons.append(f"report_status_not_eligible:{report.get('status')}")
    if report.get("business_decision") != "FREEZE_CANDIDATE_FOR_FRESH_VALIDATION_DO_NOT_CHANGE_RUNTIME":
        fail_reasons.append(f"unexpected_business_decision:{report.get('business_decision')}")
    if candidate.get("candidate_id") != buy_quality.CANDIDATE_ID:
        fail_reasons.append(f"unexpected_candidate_id:{candidate.get('candidate_id')}")
    if not close_enough(candidate.get("buyer_hhi_max"), buy_quality.BUYER_HHI_MAX):
        fail_reasons.append(f"unexpected_buyer_hhi_max:{candidate.get('buyer_hhi_max')}")
    if as_int(candidate.get("buy_count_min")) != buy_quality.BUY_COUNT_MIN:
        fail_reasons.append(f"unexpected_buy_count_min:{candidate.get('buy_count_min')}")
    if candidate.get("input_population") != "current_gatekeeper_buy_rows":
        fail_reasons.append(f"unexpected_input_population:{candidate.get('input_population')}")
    if candidate.get("threshold_origin") != "post_r23_r24_exploratory_screen":
        fail_reasons.append(f"unexpected_threshold_origin:{candidate.get('threshold_origin')}")

    if not fresh_scope:
        fail_reasons.append("missing_fresh_validation_scope")
    elif fresh_scope in discovery_scopes:
        fail_reasons.append(f"fresh_validation_scope_overlaps_discovery_scope:{fresh_scope}")
    if report_validation_scope and fresh_scope and report_validation_scope != fresh_scope:
        fail_reasons.append(f"fresh_validation_scope_mismatch:{report_validation_scope}!={fresh_scope}")
    if train_scope and fresh_scope and train_scope == fresh_scope:
        fail_reasons.append(f"train_scope_matches_fresh_validation_scope:{fresh_scope}")

    if not train:
        fail_reasons.append("missing_train_matrix_row")
    if not fresh:
        fail_reasons.append("missing_validation_matrix_row")

    if train:
        fail_if_count_below(
            fail_reasons,
            train,
            "selected_resolved_rows",
            int(args.min_support_selected_resolved_rows),
            "support",
        )
        fail_if_metric_below(
            fail_reasons,
            train,
            "selected_lift_vs_base_pp",
            float(args.min_lift_vs_base_pp),
            "support",
        )
    if fresh:
        fail_if_count_below(
            fail_reasons,
            fresh,
            "selected_resolved_rows",
            int(args.min_fresh_selected_resolved_rows),
            "fresh",
        )
        fail_if_metric_below(
            fail_reasons,
            fresh,
            "selected_precision",
            float(args.min_selected_precision),
            "fresh",
        )
        fail_if_metric_below(
            fail_reasons,
            fresh,
            "selected_lift_vs_base_pp",
            float(args.min_lift_vs_base_pp),
            "fresh",
        )
        fail_if_metric_below(
            fail_reasons,
            fresh,
            "selected_lift_vs_current_buy_pp",
            float(args.min_lift_vs_current_buy_pp),
            "fresh",
        )

    if claims.get("offline_only") is not True:
        fail_reasons.append(f"claim_not_true:offline_only={claims.get('offline_only')!r}")
    if claims.get("diagnostic_only") is not True:
        fail_reasons.append(f"claim_not_true:diagnostic_only={claims.get('diagnostic_only')!r}")
    if claims.get("requires_fresh_validation") is not True:
        fail_reasons.append(f"claim_not_true:requires_fresh_validation={claims.get('requires_fresh_validation')!r}")
    for field in NON_CLAIM_FALSE_FIELDS:
        if claims.get(field) is not False:
            fail_reasons.append(f"claim_not_false:{field}={claims.get(field)!r}")

    status = "FAIL" if fail_reasons else "PASS"
    return {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": PASS_DECISION if status == "PASS" else FAIL_DECISION,
        "report_path": str(path),
        "candidate_id": candidate.get("candidate_id"),
        "train_scope": train_scope or None,
        "fresh_validation_scope": fresh_scope or None,
        "discovery_scopes": sorted(discovery_scopes),
        "thresholds": {
            "buyer_hhi_max": buy_quality.BUYER_HHI_MAX,
            "buy_count_min": buy_quality.BUY_COUNT_MIN,
            "min_fresh_selected_resolved_rows": int(args.min_fresh_selected_resolved_rows),
            "min_support_selected_resolved_rows": int(args.min_support_selected_resolved_rows),
            "min_selected_precision": float(args.min_selected_precision),
            "min_lift_vs_base_pp": float(args.min_lift_vs_base_pp),
            "min_lift_vs_current_buy_pp": float(args.min_lift_vs_current_buy_pp),
        },
        "metrics": {
            "support_selected_resolved_rows": as_int(train.get("selected_resolved_rows")),
            "support_selected_precision": as_float(train.get("selected_precision")),
            "support_selected_lift_vs_base_pp": as_float(train.get("selected_lift_vs_base_pp")),
            "fresh_selected_resolved_rows": as_int(fresh.get("selected_resolved_rows")),
            "fresh_selected_precision": as_float(fresh.get("selected_precision")),
            "fresh_base_positive_rate": as_float(fresh.get("base_positive_rate")),
            "fresh_current_buy_precision": as_float(fresh.get("current_buy_precision")),
            "fresh_selected_lift_vs_base_pp": as_float(fresh.get("selected_lift_vs_base_pp")),
            "fresh_selected_lift_vs_current_buy_pp": as_float(fresh.get("selected_lift_vs_current_buy_pp")),
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


def validate(args: argparse.Namespace) -> dict[str, Any]:
    path = report_path(args)
    try:
        report = read_report(path)
    except FileNotFoundError:
        return {
            "artifact": ARTIFACT,
            "status": "FAIL",
            "business_decision": FAIL_DECISION,
            "report_path": str(path),
            "fail_reasons": [f"report_missing:{path}"],
        }
    except json.JSONDecodeError as exc:
        return {
            "artifact": ARTIFACT,
            "status": "FAIL",
            "business_decision": FAIL_DECISION,
            "report_path": str(path),
            "fail_reasons": [f"report_invalid_json:{path}:{exc}"],
        }
    return validate_report(report, path, args)


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
