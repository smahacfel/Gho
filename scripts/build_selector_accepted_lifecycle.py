#!/usr/bin/env python3
"""Project existing shadow-onchain lifecycle rows into accepted_lifecycle_v1."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def build_accepted_lifecycle(
    *,
    lifecycle_report: Path,
    pnl_target_net_pct: float,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    input_rows = list(common.iter_json_objects(lifecycle_report))
    rows = [
        common.project_accepted_lifecycle_row(row, pnl_target_net_pct=pnl_target_net_pct)
        for row in input_rows
    ]
    status_counts = Counter(str(row.get("analysis_status") or "unknown") for row in rows)
    lifecycle_status_counts = Counter(str(row.get("lifecycle_status") or "unknown") for row in rows)
    r1_counts = Counter(str(row.get("r1_label") or row.get("r1_excluded_reason") or "gray") for row in rows)
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "accepted_lifecycle_v1",
        "status": "ok" if rows else "no_rows",
        "input_lifecycle_report": str(lifecycle_report),
        "rows_read": len(input_rows),
        "rows_written": len(rows),
        "pnl_target_net_pct": pnl_target_net_pct,
        "analysis_status_counts": common.counter_dict(status_counts),
        "lifecycle_status_counts": common.counter_dict(lifecycle_status_counts),
        "r1_counts": common.counter_dict(r1_counts),
        "source_contract": "projection_of_existing_shadow_onchain_lifecycle_report_not_new_labeler",
    }
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lifecycle-report", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument(
        "--pnl-target-net-pct",
        required=True,
        type=float,
        help="R1 target threshold; required to avoid hidden constants.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, manifest = build_accepted_lifecycle(
        lifecycle_report=args.lifecycle_report,
        pnl_target_net_pct=args.pnl_target_net_pct,
    )
    common.write_jsonl(args.output, rows)
    if args.manifest_output:
        common.write_json(args.manifest_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
