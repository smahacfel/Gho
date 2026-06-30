#!/usr/bin/env python3
"""Audit Shadow V2 PR13 legacy downgrade enforcement.

The audit checks downgrade metadata and documentation labels only. It does not
read raw run JSONL, start validation runs, delete V1 artifacts, or upgrade V1
evidence to live-equivalent truth.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path


SCHEMA = "shadow_v2_legacy_downgrade_audit_v1"
DEFAULT_MATRIX = Path("reports/selector/shadow_v2_legacy_downgrade_matrix.csv")
DEFAULT_DOCS = [
    Path("PLANS/AUDYT/RAPORT_SHADOW_FIDELITY_DOWNGRADE_IMPACT_20260629.md"),
    Path("PLANS/AUDYT/RAPORT_SHADOW_V2_LEGACY_DOWNGRADE_ENFORCEMENT_PR13_20260630.md"),
    Path("docs/ADR/ADR_8D_SHADOW_BURNIN_V2_PR12_PR13_VALIDATION_DOWNGRADE_20260630.md"),
]

REQUIRED_COLUMNS = {
    "report_family",
    "downgraded_from",
    "allowed_use",
    "blocked_use",
    "required_label",
    "upgrade_condition",
}

REQUIRED_LABELS = {
    "ORG-A0": "OFFLINE_PATH_LABEL_ONLY",
    "R48_R2_exit_matrix": "MARK_PRICE_REPLAY_ONLY",
    "TSV2_A1_A2_A3": "DIAGNOSTIC_ONLY",
    "EIX": "DATA_BLOCKED",
    "RTP_A0": "DIAGNOSTIC_ONLY",
    "RUG_MARKUP_A0": "COMPONENT_REPLAY_ONLY",
    "RCE_A0": "BLOCKED_BY_MISSING_SURFACE",
    "R51": "ACTIVE_PARTIAL_DIAGNOSTIC_ONLY",
    "Shadow_V1_lifecycle": "LIFECYCLE_V1_NOT_CANONICAL",
    "shadow_exit_replay_v1": "MARK_PRICE_REPLAY_ONLY",
}

FORBIDDEN_ALLOWED_USE_PHRASES = {
    "live-equivalent",
    "live equivalent",
    "live pnl proof",
    "runtime approval",
    "executable fill proof",
    "real landing outcome",
}

REQUIRED_DOC_PHRASES = {
    "Previous reports must not be cited as proof of live PnL",
    "V1 never live-equivalent",
    "R51 remains ACTIVE_PARTIAL_DIAGNOSTIC_ONLY",
}


def load_matrix(path: Path) -> tuple[list[dict[str, str]], list[str]]:
    if not path.exists():
        return [], [f"missing downgrade matrix: {path}"]

    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        missing = REQUIRED_COLUMNS.difference(reader.fieldnames or [])
        if missing:
            return [], [f"{path} missing columns: {sorted(missing)}"]
        return [{key: (row.get(key) or "").strip() for key in REQUIRED_COLUMNS} for row in reader], []


def validate_matrix(path: Path) -> tuple[dict[str, str], list[str]]:
    rows, errors = load_matrix(path)
    blockers = list(errors)
    by_family = {row["report_family"]: row for row in rows}

    for family, expected_label in REQUIRED_LABELS.items():
        row = by_family.get(family)
        if row is None:
            blockers.append(f"missing downgrade row: {family}")
            continue
        if row["required_label"] != expected_label:
            blockers.append(
                f"{family}: required_label must be {expected_label}, got {row['required_label']}"
            )
        if not row["downgraded_from"]:
            blockers.append(f"{family}: downgraded_from is empty")
        if not row["allowed_use"]:
            blockers.append(f"{family}: allowed_use is empty")
        if not row["blocked_use"]:
            blockers.append(f"{family}: blocked_use is empty")
        if not row["upgrade_condition"]:
            blockers.append(f"{family}: upgrade_condition is empty")

        allowed_lower = row["allowed_use"].lower()
        for phrase in FORBIDDEN_ALLOWED_USE_PHRASES:
            if phrase in allowed_lower:
                blockers.append(f"{family}: allowed_use contains forbidden phrase {phrase}")

    label_by_family = {
        row["report_family"]: row["required_label"]
        for row in rows
        if row.get("report_family") and row.get("required_label")
    }
    return label_by_family, blockers


def validate_docs(paths: list[Path]) -> list[str]:
    blockers: list[str] = []
    combined = ""
    for path in paths:
        if not path.exists():
            blockers.append(f"missing downgrade documentation: {path}")
            continue
        combined += "\n" + path.read_text(encoding="utf-8")

    for phrase in REQUIRED_DOC_PHRASES:
        if phrase not in combined:
            blockers.append(f"missing required downgrade phrase: {phrase}")
    return blockers


def audit(matrix: Path, docs: list[Path]) -> dict[str, object]:
    label_by_family, matrix_blockers = validate_matrix(matrix)
    doc_blockers = validate_docs(docs)
    blockers = matrix_blockers + doc_blockers
    return {
        "schema": SCHEMA,
        "matrix_path": str(matrix),
        "doc_paths": [str(path) for path in docs],
        "required_family_count": len(REQUIRED_LABELS),
        "validated_family_count": len(label_by_family),
        "labels": dict(sorted(label_by_family.items())),
        "status": "PASS" if not blockers else "BLOCKED",
        "blockers": blockers,
        "v1_live_equivalent_allowed": False,
        "raw_jsonl_read": False,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Validate PR13 legacy downgrade enforcement.")
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--doc", type=Path, action="append", dest="docs")
    parser.add_argument("--strict", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    docs = args.docs if args.docs else DEFAULT_DOCS
    result = audit(args.matrix, docs)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 1 if args.strict and result["blockers"] else 0


if __name__ == "__main__":
    sys.exit(main())
