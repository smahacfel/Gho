#!/usr/bin/env python3
"""Assert selector smoke regression gates from offline audit artifacts.

This script is intentionally read-only.  It validates already-produced audit
JSON and JSONL evidence; it does not start runtime, alter Gatekeeper behavior,
or promote shadow evidence to execution.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


ARTIFACT = "selector_regression_gates_v1"
DEFAULT_AUDIT_NAME = "buy_simulation_coverage_audit_v1.json"
SELECTED_FALLBACK_SOURCE = "selected_fallback_route_execution_handoff"
BCV2_META_STATUS = "BCV2_META_READY_BY_PROTOCOL_SCHEMA"
BCV2_LOAD_NOT_REQUIRED = "BCV2_LOAD_NOT_REQUIRED"

FORBIDDEN_MARKERS = (
    "AccountNotFound",
    "unsupported_legacy_buy_layout_requires_bcv2",
    "ResourceExhausted",
    "LEGACY_BC_V2_TAIL_RESOLVER_FAILED",
    "UNKNOWN_UNCLASSIFIED",
)
FORBIDDEN_GATES = (
    *FORBIDDEN_MARKERS,
    "can_unlock_execution=true",
    "missing_on_rpc_precheck for bonding_curve_v2",
    "selected_route_kind=None for selected_fallback_route_execution_handoff",
    "primary_route_bcv2_missing fatal after final handoff",
    "BCV2 meta-only applied to normal bonding_curve",
)


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def json_text(row: dict[str, Any]) -> str:
    return json.dumps(row, ensure_ascii=False, sort_keys=True)


def as_list(value: Any) -> list[Any]:
    if value in (None, ""):
        return []
    if isinstance(value, list):
        return value
    return [value]


def row_source(row: dict[str, Any]) -> str:
    path = row.get("_source_path")
    line = row.get("_source_line")
    if path and line:
        return f"{path}:{line}"
    if path:
        return str(path)
    return "<inline>"


def default_audit_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / DEFAULT_AUDIT_NAME


def default_jsonl_paths(root: Path, scope: str) -> list[Path]:
    paths: list[Path] = []
    rollout = root / "logs" / "rollout" / scope
    if rollout.exists():
        for name in ("gatekeeper_v2_decisions.jsonl", "gatekeeper_v2_buys.jsonl"):
            paths.extend(sorted((rollout / "decisions").rglob(name)))
    for path in (
        root / "logs" / "shadow_run" / f"{scope}-buys.jsonl",
        root / "logs" / "shadow_run" / scope / "shadow_entries.jsonl",
        root / "logs" / "shadow_run" / scope / "shadow_lifecycle.jsonl",
    ):
        if path.exists():
            paths.append(path)
    return paths


def load_rows(paths: Iterable[Path]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in paths:
        for index, row in enumerate(read_jsonl(path), 1):
            row = dict(row)
            row["_source_path"] = str(path)
            row["_source_line"] = index
            rows.append(row)
    return rows


def metric_int(metrics: dict[str, Any], key: str) -> int:
    value = metrics.get(key)
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(round(value))
    if isinstance(value, str) and value.strip():
        return int(float(value))
    return 0


def attempted_rows(metrics: dict[str, Any]) -> int:
    explicit = metrics.get("shadow_simulation_attempted_rows")
    if explicit is not None:
        return metric_int(metrics, "shadow_simulation_attempted_rows")
    buy_rows = metric_int(metrics, "buy_rows")
    coverage = float(metrics.get("simulation_attempt_coverage") or 0.0)
    return int(round(buy_rows * coverage))


def success_rows(metrics: dict[str, Any]) -> int:
    return metric_int(metrics, "shadow_simulated_rows")


def contains_marker(row: dict[str, Any], marker: str) -> bool:
    if marker == "can_unlock_execution=true":
        return row.get("can_unlock_execution") is True or "can_unlock_execution=true" in json_text(row)
    return marker in json_text(row)


def is_selected_fallback(row: dict[str, Any]) -> bool:
    values = {
        row.get("route_account_manifest_source"),
        row.get("selected_route_source"),
        row.get("final_manifest_source"),
        row.get("source"),
    }
    return SELECTED_FALLBACK_SOURCE in values


def selected_route_kind_missing_for_fallback(row: dict[str, Any]) -> bool:
    if not is_selected_fallback(row):
        return False
    value = row.get("selected_route_kind")
    return value in (None, "", "None", "null")


def missing_on_rpc_precheck_for_bcv2(row: dict[str, Any]) -> bool:
    text = json_text(row)
    if "missing_on_rpc_precheck" in text and "bonding_curve_v2" in text:
        return True
    for entry in manifest_entries(row):
        role = entry.get("role") or entry.get("account_role") or entry.get("name")
        if role == "bonding_curve_v2" and any(
            entry.get(field) == "missing_on_rpc_precheck"
            for field in (
                "precheck_rpc_load_status",
                "rpc_load_status",
                "builder_required_curve_account_ready_reason",
            )
        ):
            return True
    return False


def primary_route_bcv2_missing_fatal_after_final_handoff(row: dict[str, Any]) -> bool:
    if not is_selected_fallback(row):
        return False
    fatal_values: list[Any] = []
    for field in (
        "fatal_reasons_after_final_manifest_validation",
        "selected_route_handoff_fatal_reasons",
        "route_handoff_fatal_reasons",
        "final_handoff_fatal_reasons",
    ):
        fatal_values.extend(as_list(row.get(field)))
    return any("primary_route_bcv2_missing" in str(value) for value in fatal_values)


def manifest_entries(row: dict[str, Any]) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for field in (
        "simulation_account_manifest",
        "selected_route_account_manifest",
        "account_manifest",
        "final_manifest_accounts",
    ):
        raw = row.get(field)
        if isinstance(raw, list):
            entries.extend(item for item in raw if isinstance(item, dict))
    return entries


def normal_bonding_curve_marked_meta_only(row: dict[str, Any]) -> bool:
    if row.get("bonding_curve_load_status") == BCV2_LOAD_NOT_REQUIRED:
        return True
    if row.get("bonding_curve_meta_status") == BCV2_META_STATUS:
        return True
    for entry in manifest_entries(row):
        role = entry.get("role") or entry.get("account_role") or entry.get("name")
        if role != "bonding_curve":
            continue
        if entry.get("precheck_rpc_load_status") == BCV2_LOAD_NOT_REQUIRED:
            return True
        if entry.get("rpc_load_status") == BCV2_LOAD_NOT_REQUIRED:
            return True
        if entry.get("builder_required_curve_account_ready_reason") == BCV2_META_STATUS:
            return True
    return False


def count_forbidden_rows(rows: list[dict[str, Any]]) -> tuple[dict[str, int], dict[str, list[str]]]:
    counts: Counter[str] = Counter()
    samples: dict[str, list[str]] = {}

    for row in rows:
        marker_checks = {
            **{marker: contains_marker(row, marker) for marker in FORBIDDEN_MARKERS},
            "can_unlock_execution=true": contains_marker(row, "can_unlock_execution=true"),
            "missing_on_rpc_precheck for bonding_curve_v2": missing_on_rpc_precheck_for_bcv2(row),
            "selected_route_kind=None for selected_fallback_route_execution_handoff": (
                selected_route_kind_missing_for_fallback(row)
            ),
            "primary_route_bcv2_missing fatal after final handoff": (
                primary_route_bcv2_missing_fatal_after_final_handoff(row)
            ),
            "BCV2 meta-only applied to normal bonding_curve": normal_bonding_curve_marked_meta_only(row),
        }
        for marker, present in marker_checks.items():
            if present:
                counts[marker] += 1
                samples.setdefault(marker, []).append(row_source(row))

    return dict(counts), {key: values[:10] for key, values in samples.items()}


def audit_marker_count(audit: dict[str, Any], marker: str) -> int:
    if marker == "UNKNOWN_UNCLASSIFIED":
        failure = (audit.get("failure_classes") or {}).get("UNKNOWN_UNCLASSIFIED") or {}
        return metric_int(failure, "count")
    if marker == "LEGACY_BC_V2_TAIL_RESOLVER_FAILED":
        failure = (audit.get("failure_classes") or {}).get(marker) or {}
        return metric_int(failure, "count")
    return metric_int(audit.get("critical_regression_markers") or {}, marker)


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    audit_path = args.audit_json or default_audit_path(root, args.scope)
    audit = read_json(audit_path)
    jsonl_paths = list(args.jsonl or [])
    if not jsonl_paths and not args.audit_only:
        jsonl_paths = default_jsonl_paths(root, args.scope)
    rows = load_rows(jsonl_paths)

    metrics = audit.get("metrics") or {}
    buy_rows = metric_int(metrics, "buy_rows")
    attempted = attempted_rows(metrics)
    success = success_rows(metrics)
    not_executable = metric_int(metrics, "not_executable_route_rows")
    target_rows = math.ceil(buy_rows * args.min_attempt_coverage) if buy_rows else 0
    attempt_coverage = attempted / buy_rows if buy_rows else 0.0

    forbidden_counts, forbidden_samples = count_forbidden_rows(rows)
    for marker in FORBIDDEN_MARKERS:
        count = audit_marker_count(audit, marker)
        if count:
            forbidden_counts[marker] = max(forbidden_counts.get(marker, 0), count)

    fail_reasons: list[str] = []
    if buy_rows <= 0:
        fail_reasons.append("buy_rows_zero")
    if args.require_not_executable_zero and not_executable != 0:
        fail_reasons.append("not_executable_route_rows_nonzero")
    if args.require_attempted_equals_buy and attempted != buy_rows:
        fail_reasons.append("attempted_rows_not_equal_buy_rows")
    if attempted < target_rows:
        fail_reasons.append("attempt_coverage_below_minimum")
    for marker, count in sorted(forbidden_counts.items()):
        if count > 0:
            fail_reasons.append(f"forbidden_marker_present:{marker}")

    return {
        "artifact": ARTIFACT,
        "schema_version": 1,
        "status": "PASS" if not fail_reasons else "FAIL",
        "scope": args.scope,
        "audit_json": str(audit_path),
        "jsonl_paths": [str(path) for path in jsonl_paths],
        "metrics": {
            "buy_rows": buy_rows,
            "attempted_rows": attempted,
            "success_rows": success,
            "not_executable_route_rows": not_executable,
            "attempt_coverage": attempt_coverage,
            "min_attempt_coverage": args.min_attempt_coverage,
            "target_attempted_rows": target_rows,
            "attempted_equals_buy_rows": attempted == buy_rows,
        },
        "forbidden_markers": {
            "counts": {key: forbidden_counts.get(key, 0) for key in sorted(set(forbidden_counts) | set(FORBIDDEN_GATES))},
            "samples": forbidden_samples,
        },
        "config_guard": {
            "bcv2_meta_only_role": "bonding_curve_v2",
            "normal_bonding_curve_load_required": forbidden_counts.get(
                "BCV2 meta-only applied to normal bonding_curve",
                0,
            )
            == 0,
        },
        "claim_boundaries": {
            "offline_assert_only": True,
            "active_execution_changed": False,
            "send_path_changed": False,
            "gatekeeper_changed": False,
            "can_unlock_execution_allowed": False,
        },
        "fail_reasons": sorted(set(fail_reasons)),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--audit-json", type=Path)
    parser.add_argument("--jsonl", type=Path, action="append", default=[])
    parser.add_argument("--audit-only", action="store_true")
    parser.add_argument("--min-attempt-coverage", type=float, default=0.95)
    parser.add_argument("--require-attempted-equals-buy", action="store_true")
    parser.add_argument("--require-not-executable-zero", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
