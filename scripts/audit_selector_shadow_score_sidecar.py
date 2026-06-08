#!/usr/bin/env python3
"""Audit runtime selector shadow score sidecar emission.

This audit is read-only.  It checks that selector_shadow_score_v1.jsonl is
emitted as an additive diagnostic sidecar for terminal Gatekeeper decisions and
that the sidecar keeps its non-claim boundaries.  It does not compare runtime
scores to offline P3K/P3J scores; that is the separate parity audit.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "selector_shadow_score_sidecar_audit_v1"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
SCORE_FILE = "selector_shadow_score_v1.jsonl"
EXPECTED_SCHEMA = "selector_shadow_score_v1"
EXPECTED_SCORE_VERSION = "selector_shadow_score_combined_simple_v1"
EXPECTED_CANDIDATE_ID = "combined:simple_feature_score_v1"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True, help="Runtime rollout scope.")
    parser.add_argument("--root", default="/root/Gho", help="Repository/runtime root.")
    parser.add_argument(
        "--decision-plane",
        default=None,
        help="Optional decision plane filter, e.g. legacy_live or v25_shadow.",
    )
    parser.add_argument(
        "--min-score-coverage",
        type=float,
        default=0.95,
        help="Minimum score rows / decision rows coverage.",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Optional JSON output path. Defaults under reports/selector/<scope>.",
    )
    parser.add_argument("--json", action="store_true", help="Print JSON report.")
    return parser


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def default_output(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"{ARTIFACT}.json"


def decision_dirs(root: Path, scope: str, decision_plane: str | None) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / scope / "decisions" / scope
    if not decisions_root.exists():
        return []
    paths = sorted(decisions_root.rglob(DECISION_FILE))
    if decision_plane:
        paths = [path for path in paths if f"/{decision_plane}/" in path.as_posix()]
    return [path.parent for path in paths]


def is_numeric(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def valid_reason_vector(row: dict[str, Any]) -> bool:
    reason = row.get("reason_vector")
    if not isinstance(reason, dict):
        return False
    return all(isinstance(reason.get(key), list) for key in ("positive", "negative", "missing"))


def claim_boundary_ok(row: dict[str, Any]) -> bool:
    boundaries = row.get("claim_boundaries")
    if not isinstance(boundaries, dict):
        return False
    expected = {
        "diagnostic_only": True,
        "shadow_only": True,
        "production_promotion_allowed": False,
        "gatekeeper_tuning_started": False,
        "changes_gatekeeper_decision": False,
        "changes_execution": False,
        "send_path_changed": False,
    }
    return all(boundaries.get(key) is value for key, value in expected.items())


def audit_plane(decision_dir: Path) -> dict[str, Any]:
    decision_path = decision_dir / DECISION_FILE
    score_path = decision_dir / SCORE_FILE
    decisions = read_jsonl(decision_path)
    scores = read_jsonl(score_path) if score_path.exists() else []

    schema_versions: Counter[str] = Counter()
    score_versions: Counter[str] = Counter()
    candidate_ids: Counter[str] = Counter()
    verdict_types: Counter[str] = Counter()
    validity_statuses: Counter[str] = Counter()
    mapping_statuses: Counter[str] = Counter()
    threshold_true_counts: Counter[str] = Counter()

    numeric_score_rows = 0
    claim_boundary_violation_rows = 0
    decision_influence_claim_rows = 0
    execution_influence_claim_rows = 0
    send_path_changed_claim_rows = 0
    malformed_reason_vector_rows = 0
    missing_validity_status_rows = 0
    missing_feature_mapping_status_rows = 0
    score_min: float | None = None
    score_max: float | None = None

    for row in scores:
        schema_versions[str(row.get("schema_version"))] += 1
        score_versions[str(row.get("score_version"))] += 1
        candidate_ids[str(row.get("score_candidate_id"))] += 1
        verdict_types[str(row.get("gatekeeper_verdict_type"))] += 1
        validity = row.get("score_validity_status")
        if validity:
            validity_statuses[str(validity)] += 1
        else:
            missing_validity_status_rows += 1

        feature_availability = row.get("feature_availability")
        mapping_status = (
            feature_availability.get("feature_mapping_status")
            if isinstance(feature_availability, dict)
            else None
        )
        if mapping_status:
            mapping_statuses[str(mapping_status)] += 1
        else:
            missing_feature_mapping_status_rows += 1

        score = row.get("selector_shadow_score")
        if is_numeric(score):
            numeric_score_rows += 1
            score = float(score)
            score_min = score if score_min is None else min(score_min, score)
            score_max = score if score_max is None else max(score_max, score)

        if not claim_boundary_ok(row):
            claim_boundary_violation_rows += 1

        boundaries = row.get("claim_boundaries")
        if isinstance(boundaries, dict):
            decision_influence_claim_rows += int(boundaries.get("changes_gatekeeper_decision") is True)
            execution_influence_claim_rows += int(boundaries.get("changes_execution") is True)
            send_path_changed_claim_rows += int(boundaries.get("send_path_changed") is True)

        if not valid_reason_vector(row):
            malformed_reason_vector_rows += 1

        thresholds = row.get("thresholds")
        if isinstance(thresholds, dict):
            for key, value in thresholds.items():
                if value is True:
                    threshold_true_counts[str(key)] += 1

    decision_rows = len(decisions)
    score_rows = len(scores)
    coverage = (score_rows / decision_rows) if decision_rows else 0.0
    return {
        "decision_dir": str(decision_dir),
        "decision_path": str(decision_path),
        "score_path": str(score_path),
        "decision_rows": decision_rows,
        "score_rows": score_rows,
        "score_coverage": coverage,
        "score_coverage_raw": f"{score_rows}/{decision_rows}",
        "schema_versions": dict(schema_versions),
        "score_versions": dict(score_versions),
        "score_candidate_ids": dict(candidate_ids),
        "gatekeeper_verdict_type_counts": dict(verdict_types),
        "numeric_score_rows": numeric_score_rows,
        "score_min": score_min,
        "score_max": score_max,
        "score_validity_status_counts": dict(validity_statuses),
        "feature_mapping_status_counts": dict(mapping_statuses),
        "threshold_true_counts": dict(threshold_true_counts),
        "claim_boundary_violation_rows": claim_boundary_violation_rows,
        "decision_influence_claim_rows": decision_influence_claim_rows,
        "execution_influence_claim_rows": execution_influence_claim_rows,
        "send_path_changed_claim_rows": send_path_changed_claim_rows,
        "malformed_reason_vector_rows": malformed_reason_vector_rows,
        "missing_validity_status_rows": missing_validity_status_rows,
        "missing_feature_mapping_status_rows": missing_feature_mapping_status_rows,
    }


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    planes = [audit_plane(path) for path in decision_dirs(root, args.scope, args.decision_plane)]

    fail_reasons: list[str] = []
    if not planes:
        fail_reasons.append("no_decision_dirs_found")

    total_decisions = sum(int(plane["decision_rows"]) for plane in planes)
    total_scores = sum(int(plane["score_rows"]) for plane in planes)
    total_numeric = sum(int(plane["numeric_score_rows"]) for plane in planes)
    total_claim_boundary_violations = sum(
        int(plane["claim_boundary_violation_rows"]) for plane in planes
    )
    total_decision_claims = sum(int(plane["decision_influence_claim_rows"]) for plane in planes)
    total_execution_claims = sum(int(plane["execution_influence_claim_rows"]) for plane in planes)
    total_send_claims = sum(int(plane["send_path_changed_claim_rows"]) for plane in planes)
    total_malformed_reason = sum(int(plane["malformed_reason_vector_rows"]) for plane in planes)
    total_missing_validity = sum(int(plane["missing_validity_status_rows"]) for plane in planes)
    total_missing_mapping = sum(
        int(plane["missing_feature_mapping_status_rows"]) for plane in planes
    )
    total_coverage = (total_scores / total_decisions) if total_decisions else 0.0

    if total_decisions > 0 and total_coverage < args.min_score_coverage:
        fail_reasons.append(
            f"score_coverage {total_scores}/{total_decisions} < {args.min_score_coverage:.2f}"
        )
    if total_numeric <= 0:
        fail_reasons.append("numeric_score_rows=0")
    if total_claim_boundary_violations:
        fail_reasons.append(f"claim_boundary_violation_rows={total_claim_boundary_violations}")
    if total_decision_claims:
        fail_reasons.append(f"decision_influence_claim_rows={total_decision_claims}")
    if total_execution_claims:
        fail_reasons.append(f"execution_influence_claim_rows={total_execution_claims}")
    if total_send_claims:
        fail_reasons.append(f"send_path_changed_claim_rows={total_send_claims}")
    if total_malformed_reason:
        fail_reasons.append(f"malformed_reason_vector_rows={total_malformed_reason}")
    if total_missing_validity:
        fail_reasons.append(f"missing_validity_status_rows={total_missing_validity}")
    if total_missing_mapping:
        fail_reasons.append(f"missing_feature_mapping_status_rows={total_missing_mapping}")

    for plane in planes:
        if EXPECTED_SCHEMA not in plane["schema_versions"]:
            fail_reasons.append(f"{plane['decision_dir']}: missing schema {EXPECTED_SCHEMA}")
        if EXPECTED_SCORE_VERSION not in plane["score_versions"]:
            fail_reasons.append(
                f"{plane['decision_dir']}: missing score_version {EXPECTED_SCORE_VERSION}"
            )
        if EXPECTED_CANDIDATE_ID not in plane["score_candidate_ids"]:
            fail_reasons.append(
                f"{plane['decision_dir']}: missing score_candidate_id {EXPECTED_CANDIDATE_ID}"
            )

    report = {
        "artifact": ARTIFACT,
        "status": "PASS" if not fail_reasons else "FAIL",
        "scope": args.scope,
        "decision_plane_filter": args.decision_plane,
        "min_score_coverage": args.min_score_coverage,
        "decision_rows": total_decisions,
        "score_rows": total_scores,
        "score_coverage": total_coverage,
        "score_coverage_raw": f"{total_scores}/{total_decisions}",
        "numeric_score_rows": total_numeric,
        "claim_boundary_violation_rows": total_claim_boundary_violations,
        "decision_influence_claim_rows": total_decision_claims,
        "execution_influence_claim_rows": total_execution_claims,
        "send_path_changed_claim_rows": total_send_claims,
        "malformed_reason_vector_rows": total_malformed_reason,
        "missing_validity_status_rows": total_missing_validity,
        "missing_feature_mapping_status_rows": total_missing_mapping,
        "planes": planes,
        "fail_reasons": fail_reasons,
        "claim_boundaries": {
            "diagnostic_only": True,
            "shadow_only": True,
            "production_promotion_allowed": False,
            "changes_gatekeeper_decision": False,
            "changes_execution": False,
            "send_path_changed": False,
        },
    }

    output = Path(args.output) if args.output else default_output(root, args.scope)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    report["output"] = str(output)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            f"{report['status']} score_rows={report['score_coverage_raw']} "
            f"numeric={report['numeric_score_rows']} output={report['output']}"
        )
        for reason in report["fail_reasons"]:
            print(f"FAIL_REASON {reason}")
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
