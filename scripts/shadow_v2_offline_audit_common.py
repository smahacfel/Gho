#!/usr/bin/env python3
"""Common offline helpers for Shadow V2 PR18D audits.

The helpers intentionally read only local JSONL/manifest artifacts from a
provided scope root. They do not import project runtime modules and never touch
RPC, gRPC, secrets, or network state.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from statistics import median
from typing import Any, Iterable


DEFAULT_SCOPE_ROOT = "reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1"
ARTIFACT_ROTATION_MANIFEST = "shadow_artifact_rotation_manifest_v2.jsonl"


def parser(description: str) -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=description)
    p.add_argument("--scope-root", default=DEFAULT_SCOPE_ROOT)
    p.add_argument("--pretty", action="store_true")
    return p


def emit(result: dict[str, Any], pretty: bool = False) -> None:
    print(json.dumps(result, indent=2 if pretty else None, sort_keys=True))


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def iter_jsonl(path: Path) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                yield None, True
                continue
            if isinstance(row, dict):
                yield row, False
            else:
                yield None, True


def read_jsonl(path: Path) -> tuple[list[dict[str, Any]], int]:
    rows: list[dict[str, Any]] = []
    malformed = 0
    for row, row_malformed in iter_jsonl(path) or ():
        if row_malformed or row is None:
            malformed += 1
            continue
        rows.append(row)
    return rows, malformed


def scope_path(scope_root: str | Path, artifact_name: str) -> Path:
    return Path(scope_root) / artifact_name


def artifact_jsonl_paths(scope_root: str | Path, artifact_name: str) -> list[Path]:
    """Return rotated JSONL parts followed by the active base artifact."""
    root = Path(scope_root)
    base = root / artifact_name
    if not artifact_name.endswith(".jsonl"):
        raise ValueError(f"artifact_name must end with .jsonl: {artifact_name}")

    stem = artifact_name[: -len(".jsonl")]
    parts = sorted(root.glob(f"{stem}.part-*.jsonl"))
    return [*parts, base]


def rotation_manifest_rows(
    scope_root: str | Path,
    artifact_name: str,
) -> tuple[list[dict[str, Any]], int]:
    artifact = artifact_name[: -len(".jsonl")] if artifact_name.endswith(".jsonl") else artifact_name
    rows, malformed = read_jsonl(scope_path(scope_root, ARTIFACT_ROTATION_MANIFEST))
    matched: list[dict[str, Any]] = []
    for row in rows:
        row_artifact = row.get("artifact")
        logical_path = row.get("logical_path")
        if row_artifact == artifact or (
            isinstance(logical_path, str) and Path(logical_path).name == artifact_name
        ):
            matched.append(row)
    return matched, malformed


def artifact_rotation_report(scope_root: str | Path, artifact_name: str) -> dict[str, Any]:
    paths = artifact_jsonl_paths(scope_root, artifact_name)
    manifest_rows, manifest_malformed = rotation_manifest_rows(scope_root, artifact_name)
    manifest_paths = []
    for row in manifest_rows:
        rotated_path = row.get("rotated_path")
        if isinstance(rotated_path, str):
            manifest_paths.append(str(Path(rotated_path)))
    discovered_parts = [str(path) for path in paths[:-1]]
    manifest_part_names = {Path(path).name for path in manifest_paths}
    discovered_part_names = {Path(path).name for path in discovered_parts}
    return {
        "artifact_name": artifact_name,
        "logical_stream_paths": [str(path) for path in paths],
        "rotated_part_count": len(paths) - 1,
        "base_path": str(paths[-1]),
        "manifest_path": str(scope_path(scope_root, ARTIFACT_ROTATION_MANIFEST)),
        "manifest_present": scope_path(scope_root, ARTIFACT_ROTATION_MANIFEST).exists(),
        "manifest_rows_for_artifact": len(manifest_rows),
        "manifest_malformed_rows": manifest_malformed,
        "manifest_missing_discovered_parts": sorted(
            discovered_part_names.difference(manifest_part_names)
        ),
        "manifest_extra_parts": sorted(
            manifest_part_names.difference(discovered_part_names)
        ),
        "read_depends_on_manifest": False,
    }


def iter_artifact_jsonl(
    scope_root: str | Path,
    artifact_name: str,
) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    for path in artifact_jsonl_paths(scope_root, artifact_name):
        for row, row_malformed in iter_jsonl(path) or ():
            yield row, row_malformed


def read_artifact_jsonl(scope_root: str | Path, artifact_name: str) -> tuple[list[dict[str, Any]], int]:
    rows: list[dict[str, Any]] = []
    malformed = 0
    for row, row_malformed in iter_artifact_jsonl(scope_root, artifact_name):
        if row_malformed or row is None:
            malformed += 1
            continue
        rows.append(row)
    return rows, malformed


def nested_record(row: dict[str, Any]) -> dict[str, Any]:
    payload = row.get("payload")
    if isinstance(payload, dict):
        record = payload.get("record")
        if isinstance(record, dict):
            return record
    return {}


def envelope(row: dict[str, Any]) -> dict[str, Any]:
    env = row.get("envelope")
    if isinstance(env, dict):
        return env
    rec_env = nested_record(row).get("envelope")
    if isinstance(rec_env, dict):
        return rec_env
    return {}


def canonical_payload_schema(row: dict[str, Any]) -> str:
    env = envelope(row)
    return str(
        env.get("schema")
        or row.get("canonical_payload_schema")
        or row.get("schema")
        or "UNKNOWN"
    )


def event_id(row: dict[str, Any]) -> str | None:
    env = envelope(row)
    value = env.get("event_id") or row.get("canonical_payload_event_id")
    return str(value) if value is not None else None


def position_id(row: dict[str, Any]) -> str | None:
    env = envelope(row)
    value = env.get("position_id") or row.get("position_id")
    return str(value) if value is not None else None


def is_smoke_marker(row: dict[str, Any]) -> bool:
    env = envelope(row)
    pos = str(env.get("position_id") or row.get("position_id") or "")
    candidate = str(env.get("candidate_id") or row.get("candidate_id") or "")
    return candidate == "VALIDATION_SMOKE_MARKER" or pos.startswith(
        "validation-smoke-marker:"
    )


def event_order_key(row: dict[str, Any]) -> dict[str, Any] | None:
    value = row.get("event_order_key")
    if isinstance(value, dict):
        return value
    rec_value = nested_record(row).get("event_order_key")
    if isinstance(rec_value, dict):
        return rec_value
    return None


def limitations(row: dict[str, Any]) -> list[str]:
    values: list[str] = []
    env_values = envelope(row).get("limitations")
    if isinstance(env_values, list):
        values.extend(str(v) for v in env_values)
    rec_values = nested_record(row).get("limitations")
    if isinstance(rec_values, list):
        values.extend(str(v) for v in rec_values)
    return values


def canonical_rows(scope_root: str | Path) -> tuple[list[dict[str, Any]], int]:
    return read_artifact_jsonl(scope_root, "shadow_position_event_v2.jsonl")


def replay_rows(scope_root: str | Path) -> tuple[list[dict[str, Any]], int]:
    return read_artifact_jsonl(scope_root, "shadow_replay_v2.jsonl")


def lifecycle_rows(scope_root: str | Path) -> tuple[list[dict[str, Any]], int]:
    return read_artifact_jsonl(scope_root, "shadow_lifecycle_v2.jsonl")


def density_rows(scope_root: str | Path) -> tuple[list[dict[str, Any]], int]:
    return read_artifact_jsonl(scope_root, "shadow_path_density_v2.jsonl")


def iter_canonical_rows(scope_root: str | Path) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    return iter_artifact_jsonl(scope_root, "shadow_position_event_v2.jsonl")


def iter_replay_rows(scope_root: str | Path) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    return iter_artifact_jsonl(scope_root, "shadow_replay_v2.jsonl")


def iter_lifecycle_rows(scope_root: str | Path) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    return iter_artifact_jsonl(scope_root, "shadow_lifecycle_v2.jsonl")


def iter_density_rows(scope_root: str | Path) -> Iterable[tuple[dict[str, Any] | None, bool]]:
    return iter_artifact_jsonl(scope_root, "shadow_path_density_v2.jsonl")


def filter_schema(rows: Iterable[dict[str, Any]], schema: str) -> list[dict[str, Any]]:
    return [row for row in rows if canonical_payload_schema(row) == schema]


def count_present(rows: Iterable[dict[str, Any]], field: str) -> int:
    count = 0
    for row in rows:
        rec = nested_record(row)
        if rec.get(field) is not None:
            count += 1
    return count


def quality(row: dict[str, Any]) -> str:
    return str(envelope(row).get("quality") or nested_record(row).get("quality") or "")


def measurement_grade(row: dict[str, Any]) -> str:
    return str(envelope(row).get("measurement_grade") or "")


def blocked_reasons(rows: Iterable[dict[str, Any]]) -> Counter[str]:
    reasons: Counter[str] = Counter()
    for row in rows:
        row_reasons: set[str] = set()
        rec = nested_record(row)
        recon = rec.get("reconstruction_status")
        if recon:
            row_reasons.add(str(recon))
        for value in limitations(row):
            if "MISSING" in value or "BLOCKED" in value or "NOT_EXECUTABLE" in value:
                row_reasons.add(value)
        reasons.update(row_reasons)
    return reasons


def distribution(values: Iterable[Any]) -> dict[str, Any]:
    numeric = [v for v in values if isinstance(v, (int, float))]
    if not numeric:
        return {"count": 0, "min": None, "median": None, "max": None}
    return {
        "count": len(numeric),
        "min": min(numeric),
        "median": median(numeric),
        "max": max(numeric),
    }


def rows_by_position(rows: Iterable[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        pos = position_id(row)
        if pos:
            out[pos].append(row)
    return out
