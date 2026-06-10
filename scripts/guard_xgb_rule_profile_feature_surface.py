#!/usr/bin/env python3
"""Guard decision-log coverage for the R22 XGB-derived rule profile.

This script is read-only. It verifies that Gatekeeper decision logs contain the
decision-time metrics needed to evaluate the R22 shadow-only rule profile after
the run. It does not evaluate BUY/REJECT policy and cannot influence runtime.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "xgb_rule_profile_feature_surface_guard_v1"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
REQUIRED_FIELDS = (
    "buy_ratio_min",
    "flipper_presence_ratio",
    "flip_ratio_10s",
    "early_slot_volume_dominance_buy",
    "hhi_delta_t2_t0",
    "dev_paperhand_latency_ms",
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True, help="Runtime rollout scope.")
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--decision-plane", default=None)
    parser.add_argument("--min-present-rate", type=float, default=0.80)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def default_output(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"{ARTIFACT}.json"


def decision_paths(root: Path, scope: str, decision_plane: str | None) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / scope / "decisions" / scope
    if not decisions_root.exists():
        return []
    paths = sorted(decisions_root.rglob(DECISION_FILE))
    if decision_plane:
        needle = f"/{decision_plane}/"
        paths = [path for path in paths if needle in path.as_posix()]
    return paths


def present(value: Any) -> bool:
    return common.float_or_none(value) is not None


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    paths = decision_paths(root, args.scope, args.decision_plane)
    field_present: Counter[str] = Counter()
    field_numeric: Counter[str] = Counter()
    row_count = 0
    sample_paths = [str(path) for path in paths[:10]]

    for path in paths:
        for row in common.iter_json_objects(path):
            row_count += 1
            for field in REQUIRED_FIELDS:
                if row.get(field) not in (None, ""):
                    field_present[field] += 1
                if present(row.get(field)):
                    field_numeric[field] += 1

    field_coverage = {
        field: (field_numeric[field] / row_count if row_count else 0.0)
        for field in REQUIRED_FIELDS
    }
    missing_fields = [
        field for field, rate in field_coverage.items() if rate < args.min_present_rate
    ]
    fail_reasons: list[str] = []
    if not paths:
        fail_reasons.append("no_gatekeeper_v2_decision_logs_found")
    if row_count <= 0:
        fail_reasons.append("no_decision_rows")
    for field in missing_fields:
        fail_reasons.append(
            f"{field}_coverage_below_min:{field_coverage[field]:.6f}<{args.min_present_rate:.6f}"
        )

    status = "PASS" if not fail_reasons else "FAIL"
    return {
        "schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "status": status,
        "scope": args.scope,
        "decision_plane": args.decision_plane,
        "decision_log_paths": sample_paths,
        "decision_log_path_count": len(paths),
        "decision_rows": row_count,
        "required_fields": list(REQUIRED_FIELDS),
        "min_present_rate": args.min_present_rate,
        "field_present_counts": dict(field_present),
        "field_numeric_counts": dict(field_numeric),
        "field_coverage": field_coverage,
        "fail_reasons": fail_reasons,
        "non_claims": {
            "changes_gatekeeper_decision": False,
            "changes_execution": False,
            "production_promotion_allowed": False,
            "runtime_sidecar_required": False,
        },
    }


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    root = Path(args.root)
    output = args.output or default_output(root, args.scope)
    common.write_json(output, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    else:
        print(f"{report['status']} {output}")
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
