#!/usr/bin/env python3
"""Audit join-key coverage for a P3.7 V3/MFS + lifecycle collection run."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_config_path, resolve_runtime_path


SCHEMA_VERSION = 1
DECISION_FILE_NAMES = ("gatekeeper_v2_decisions.jsonl", "gatekeeper_v2_buys.jsonl")
FIELD_GROUPS = {
    "ab_record_id": ("ab_record_id",),
    "candidate_id": ("candidate_id", "execution_candidate_id"),
    "position_id": ("position_id",),
    "pool_id": ("pool_id",),
    "mint": ("base_mint", "mint_id", "mint"),
    "decision_ts_ms": ("decision_ts_ms", "ab_t_end_event_ts_ms", "timestamp_ms", "timestamp"),
    "observation_start_ts_ms": ("observation_start_ts_ms", "ab_t0_event_ts_ms", "first_seen_ts_ms"),
    "observation_end_ts_ms": ("observation_end_ts_ms", "ab_t_end_event_ts_ms"),
    "feature_snapshot_hash": ("v3_feature_snapshot_hash", "feature_snapshot_hash"),
    "v3_policy_config_hash": ("v3_policy_config_hash", "config_hash"),
    "decision_plane": ("decision_plane",),
    "rollout_namespace": ("rollout_namespace", "rollout_profile"),
    "v3_replay_payload": ("v3_replay_payload_schema_version", "v3_replay_payload"),
}


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if isinstance(obj, dict):
                yield obj


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def first_present(row: dict[str, Any], fields: tuple[str, ...]) -> Any:
    for field in fields:
        value = row.get(field)
        if value is None:
            continue
        if isinstance(value, str) and value == "":
            continue
        return value
    return None


def value_set(row: dict[str, Any], fields: tuple[str, ...]) -> set[str]:
    value = first_present(row, fields)
    if value is None:
        return set()
    return {str(value)}


def artifact_summary(name: str, path: Path) -> dict[str, Any]:
    rows = list(iter_jsonl(path))
    field_counts: dict[str, int] = {}
    identifiers: dict[str, set[str]] = defaultdict(set)
    for group, fields in FIELD_GROUPS.items():
        count = 0
        for row in rows:
            values = value_set(row, fields)
            if values:
                count += 1
                identifiers[group].update(values)
        field_counts[group] = count
    return {
        "name": name,
        "path": str(path),
        "exists": path.exists(),
        "rows": len(rows),
        "field_counts": field_counts,
        "field_coverage_pct": {
            field: round((count / len(rows) * 100.0), 3) if rows else 0.0
            for field, count in field_counts.items()
        },
        "_identifiers": {key: values for key, values in identifiers.items()},
    }


def resolve_paths(config_path: Path) -> dict[str, list[Path]]:
    resolved = resolve_config_path(config_path)
    config = load_toml(resolved)
    paths: dict[str, list[Path]] = defaultdict(list)

    decision_root = resolve_runtime_path(
        resolved,
        config.get("oracle", {}).get("decision_log_path", "logs/decisions"),
    )
    root = Path(decision_root)
    for name in DECISION_FILE_NAMES:
        direct = root / name
        if direct.exists():
            paths["decision"].append(direct)
        if root.exists():
            paths["decision"].extend(path for path in root.rglob(name) if path.is_file() and path != direct)

    trigger_path = config.get("trigger", {}).get("shadow_run", {}).get("output_path")
    if trigger_path:
        paths["shadow_transport"].append(Path(resolve_runtime_path(resolved, trigger_path)))

    shadow = config.get("execution", {}).get("shadow", {})
    entry_path = shadow.get("entry_log_path")
    if entry_path:
        paths["shadow_entry"].append(Path(resolve_runtime_path(resolved, entry_path)))
    lifecycle_path = shadow.get("lifecycle_log_path")
    if lifecycle_path:
        paths["shadow_lifecycle"].append(Path(resolve_runtime_path(resolved, lifecycle_path)))
    return {key: sorted(set(value)) for key, value in paths.items()}


def intersection_counts(summaries: list[dict[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for group in ("ab_record_id", "candidate_id", "pool_id", "mint"):
        sets = [
            summary.get("_identifiers", {}).get(group, set())
            for summary in summaries
            if summary.get("rows", 0) > 0
        ]
        if not sets:
            result[group] = {"artifacts_with_rows": 0, "common_values": 0}
            continue
        common = set.intersection(*sets) if len(sets) > 1 else set(sets[0])
        result[group] = {
            "artifacts_with_rows": len(sets),
            "common_values": len(common),
            "per_artifact_values": [len(values) for values in sets],
        }
    return result


def clean_summary(summary: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in summary.items() if key != "_identifiers"}


def readiness(report: dict[str, Any]) -> dict[str, Any]:
    decision_rows = sum(item["rows"] for item in report["artifacts"].get("decision", []))
    v3_payload_rows = sum(
        item["field_counts"].get("v3_replay_payload", 0)
        for item in report["artifacts"].get("decision", [])
    )
    shadow_entry_rows = sum(item["rows"] for item in report["artifacts"].get("shadow_entry", []))
    lifecycle_rows = sum(item["rows"] for item in report["artifacts"].get("shadow_lifecycle", []))
    transport_rows = sum(item["rows"] for item in report["artifacts"].get("shadow_transport", []))
    candidate_common = report["cross_artifact_intersections"].get("candidate_id", {}).get("common_values", 0)
    status = "ready_for_lifecycle_feature_join"
    reasons: list[str] = []
    if decision_rows <= 0:
        status = "not_ready"
        reasons.append("missing_decision_rows")
    if v3_payload_rows <= 0:
        status = "not_ready"
        reasons.append("missing_v3_replay_payload_rows")
    if transport_rows <= 0:
        status = "not_ready"
        reasons.append("missing_shadow_transport_rows")
    if shadow_entry_rows <= 0:
        status = "not_ready"
        reasons.append("missing_shadow_entry_rows")
    if lifecycle_rows <= 0:
        status = "not_ready"
        reasons.append("missing_shadow_lifecycle_rows")
    if candidate_common <= 0:
        status = "degraded" if status != "not_ready" else status
        reasons.append("no_common_candidate_id_across_nonempty_artifacts")
    return {
        "status": status,
        "reasons": reasons,
        "decision_rows": decision_rows,
        "v3_payload_rows": v3_payload_rows,
        "shadow_transport_rows": transport_rows,
        "shadow_entry_rows": shadow_entry_rows,
        "shadow_lifecycle_rows": lifecycle_rows,
    }


def build_report(config_path: Path) -> dict[str, Any]:
    resolved = resolve_config_path(config_path)
    paths = resolve_paths(resolved)
    artifacts: dict[str, list[dict[str, Any]]] = {}
    for artifact_type, artifact_paths in sorted(paths.items()):
        artifacts[artifact_type] = []
        for path in artifact_paths:
            summary = artifact_summary(artifact_type, path)
            artifacts[artifact_type].append(clean_summary(summary))
    with_ids = [artifact_summary(path_type, path) for path_type, values in sorted(paths.items()) for path in values]
    report = {
        "schema_version": SCHEMA_VERSION,
        "config_path": str(resolved),
        "artifacts": artifacts,
        "cross_artifact_intersections": intersection_counts(with_ids),
    }
    report["readiness"] = readiness(report)
    return report


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-J MFS Lifecycle Join-Key Audit",
        "",
        f"- config: `{report['config_path']}`",
        f"- readiness: `{report['readiness']['status']}`",
        f"- readiness_reasons: `{json.dumps(report['readiness']['reasons'], ensure_ascii=False)}`",
        "",
        "## Artifact Coverage",
        "",
        "| artifact | rows | candidate_id | ab_record_id | pool_id | mint | v3_payload | feature_hash |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for artifact_type, items in report["artifacts"].items():
        for item in items:
            counts = item["field_counts"]
            lines.append(
                f"| `{artifact_type}` | {item['rows']} | {counts.get('candidate_id', 0)} | "
                f"{counts.get('ab_record_id', 0)} | {counts.get('pool_id', 0)} | "
                f"{counts.get('mint', 0)} | {counts.get('v3_replay_payload', 0)} | "
                f"{counts.get('feature_snapshot_hash', 0)} |"
            )
    lines.extend(["", "## Cross-Artifact Intersections", ""])
    for key, value in report["cross_artifact_intersections"].items():
        lines.append(f"- `{key}`: `{json.dumps(value, ensure_ascii=False, sort_keys=True)}`")
    lines.extend(
        [
            "",
            "## Governance",
            "",
            "- This audit measures join-key coverage only.",
            "- It does not infer lifecycle truth, strategy edge, or live inclusion.",
        ]
    )
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--output-json", required=True, type=Path)
    parser.add_argument("--output-md", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_report(args.config)
    write_json(args.output_json, report)
    args.output_md.parent.mkdir(parents=True, exist_ok=True)
    args.output_md.write_text(render_markdown(report), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
