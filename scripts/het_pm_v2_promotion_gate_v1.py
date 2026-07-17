#!/usr/bin/env python3
"""Build and evaluate deterministic HET-PM V2 promotion evidence.

The tool has three fail-closed commands:

* ``manifest`` hashes every immutable artifact belonging to one run;
* ``evaluate`` verifies those hashes, performs exact position/lifecycle/replay
  reconciliation, computes Gates 1-5, and writes canonical JSON;
* ``validate`` recomputes from source manifests and compares canonical bytes;
* ``validate-structure`` checks artifact shape/root conjunction only.

The economic unit is a unique ``(run_id, position_id, position_epoch)`` opened
by the primary shadow monitor. Tick rows are evidence samples, never the sample
denominator. Mark replay and executable quote returns remain explicitly
separate measurement classes.
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import importlib.util
import json
import math
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


TOOL_ID = "het_pm_v2_promotion_gate_v1"
TOOL_VERSION = 2
PROMOTION_SCHEMA_VERSION = 2
RUN_MANIFEST_SCHEMA_VERSION = 2
RUN_MANIFEST_TYPE = "het_pm_v2_run_input_manifest"
CRITERIA_VERSION = 2
REQUIRED_ARTIFACT_CLASSES = (
    "brain_config",
    "run_config",
    "launcher_proof",
    "comparison",
    "writer_health",
    "lifecycle",
    "exit_replay",
    "position_events",
    "position_censored",
    "admission",
    "gatekeeper_buys",
    "runtime_log",
)
OPTIONAL_EMPTY_JSONL_CLASSES = {"position_censored", "admission"}
GATE_NAMES = (
    "lifecycle_integrity",
    "data_coverage",
    "quote_budget",
    "economic_result",
    "stability",
)
V2_EXIT_REASONS = (
    "Crash",
    "HardLoss",
    "ExecutableTrailing",
    "VitalityDecay",
    "AbsoluteMaxHold",
)
V2_REASON_KEYS = {
    "Crash": "crash",
    "HardLoss": "hard_loss",
    "ExecutableTrailing": "executable_trailing",
    "VitalityDecay": "vitality_decay",
    "AbsoluteMaxHold": "absolute_max_hold",
}
V2_EXIT_RE = re.compile(
    r"ExitAll \{ reason: ([A-Za-z]+), quantity_raw: ([0-9]+), "
    r"executable_gross_return_bps: (-?[0-9]+) \}"
)


class ContractError(ValueError):
    """The supplied evidence cannot satisfy the deterministic contract."""


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)
        + "\n"
    ).encode("utf-8")


def hash_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def mtime_utc(path: Path) -> str:
    return datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()


def pr_a_analyzer_path() -> Path:
    return Path(__file__).with_name("het_pm_v2_analysis.py")


def reject_non_finite(value: Any, location: str = "root") -> None:
    if isinstance(value, float) and not math.isfinite(value):
        raise ContractError(f"non-finite value at {location}")
    if isinstance(value, dict):
        for key, nested in value.items():
            reject_non_finite(nested, f"{location}.{key}")
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            reject_non_finite(nested, f"{location}[{index}]")


def require(record: dict[str, Any], field: str, expected: type | tuple[type, ...]) -> Any:
    if field not in record:
        raise ContractError(f"missing field: {field}")
    value = record[field]
    if not isinstance(value, expected) or expected is int and isinstance(value, bool):
        raise ContractError(f"invalid type for field: {field}")
    return value


def require_number(record: dict[str, Any], field: str) -> float:
    value = require(record, field, (int, float))
    if isinstance(value, bool) or not math.isfinite(float(value)):
        raise ContractError(f"invalid finite number for field: {field}")
    return float(value)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"JSON root must be an object: {path}")
    reject_non_finite(value, str(path))
    return value


def iter_jsonl(paths: Iterable[Path]) -> Iterable[tuple[Path, int, dict[str, Any]]]:
    for path in sorted(paths, key=lambda item: str(item)):
        try:
            handle = path.open("r", encoding="utf-8")
        except OSError as error:
            raise ContractError(f"cannot read JSONL {path}: {error}") from error
        with handle:
            for line_number, line in enumerate(handle, 1):
                if not line.strip():
                    continue
                try:
                    value = json.loads(line)
                except json.JSONDecodeError as error:
                    raise ContractError(
                        f"invalid JSONL {path}:{line_number}: {error}"
                    ) from error
                if not isinstance(value, dict):
                    raise ContractError(f"JSONL row is not an object: {path}:{line_number}")
                reject_non_finite(value, f"{path}:{line_number}")
                yield path, line_number, value


def load_pr_a_analyzer() -> Any:
    path = pr_a_analyzer_path()
    spec = importlib.util.spec_from_file_location("het_pm_v2_analysis", path)
    if spec is None or spec.loader is None:
        raise ContractError(f"cannot load structural analyzer: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def normalize_repo_path(path: Path, repo_root: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(repo_root.resolve()).as_posix()
    except ValueError as error:
        raise ContractError(f"artifact is outside repo root: {resolved}") from error


def expand_patterns(patterns: list[str], repo_root: Path) -> list[Path]:
    found: set[Path] = set()
    for pattern in patterns:
        candidate = Path(pattern)
        absolute_pattern = (
            str(candidate) if candidate.is_absolute() else str(repo_root / candidate)
        )
        matches = [Path(item) for item in glob.glob(absolute_pattern)]
        if not matches and Path(absolute_pattern).is_file():
            matches = [Path(absolute_pattern)]
        if not matches:
            raise ContractError(f"artifact pattern matched no files: {pattern}")
        for path in matches:
            if not path.is_file():
                raise ContractError(f"artifact is not a file: {path}")
            found.add(path.resolve())
    return sorted(found, key=lambda item: str(item))


def artifact_entries(paths: list[Path], repo_root: Path) -> list[dict[str, Any]]:
    return [
        {
            "path": normalize_repo_path(path, repo_root),
            "sha256": sha256(path),
            "size_bytes": path.stat().st_size,
        }
        for path in paths
    ]


def launcher_status_passed(report: dict[str, Any], field: str) -> bool:
    value = report.get(field)
    if not isinstance(value, dict):
        return False
    if value.get("status") == "PASS":
        return True
    nested = value.get("json")
    return isinstance(nested, dict) and nested.get("status") == "PASS"


def build_launcher_proof(
    *,
    report: dict[str, Any],
    args: argparse.Namespace,
    artifacts: dict[str, list[dict[str, Any]]],
    runtime_paths: list[Path],
    repo_root: Path,
) -> dict[str, Any]:
    if report.get("scope") != args.run_id:
        raise ContractError("launcher proof scope/run_id mismatch")
    if report.get("launch_cohort_id") != args.launch_cohort_id:
        raise ContractError("launcher proof launch_cohort_id mismatch")
    git_head_at_build = require(report, "git_head_at_build", str)
    git_head_at_launch = require(report, "git_head_at_launch", str)
    if git_head_at_build != git_head_at_launch:
        raise ContractError("launcher proof git build/launch mismatch")
    if not re.fullmatch(r"[0-9a-f]{40}", git_head_at_launch):
        raise ContractError("launcher proof git commit is not a full SHA")
    release_binary_sha256 = require(report, "release_binary_sha256", str)
    if not re.fullmatch(r"[0-9a-f]{64}", release_binary_sha256):
        raise ContractError("launcher proof release binary hash is invalid")
    build = require(report, "build_freshness", dict)
    build_started_at = require(build, "started_at_utc", str)
    runtime_started_at = require(report, "runtime_started_at_utc", str)
    runtime_health = scan_runtime_health(runtime_paths)
    shutdown_result = (
        "clean"
        if runtime_health["clean_shutdown_marker_count"] > 0
        and runtime_health["forced_component_shutdown_marker_count"] == 0
        else "unclean_or_forced"
    )
    runtime_ended_at = max((mtime_utc(path) for path in runtime_paths), default=runtime_started_at)
    raw_config_path = Path(require(report, "config", str))
    config_path = (
        raw_config_path
        if raw_config_path.is_absolute()
        else repo_root / raw_config_path
    ).resolve()
    expected_config_path = (repo_root / artifacts["run_config"][0]["path"]).resolve()
    if config_path != expected_config_path:
        raise ContractError("launcher proof run config path mismatch")
    proof = {
        "run_id": args.run_id,
        "launch_cohort_id": args.launch_cohort_id,
        "git_commit_sha": git_head_at_launch,
        "release_binary_sha256": release_binary_sha256,
        "run_config_sha256": artifacts["run_config"][0]["sha256"],
        "brain_config_sha256": artifacts["brain_config"][0]["sha256"],
        "build_started_at": build_started_at,
        "runtime_started_at": runtime_started_at,
        "runtime_ended_at": runtime_ended_at,
        "shutdown_signal": "SIGINT" if report.get("runtime_timeout_seconds") else "external",
        "shutdown_result": shutdown_result,
        "event_canary_passed": launcher_status_passed(report, "event_canary"),
        "lifecycle_canary_passed": launcher_status_passed(report, "lifecycle_canary"),
        "static_guard_passed": launcher_status_passed(report, "static_guard"),
        "preflight_passed": launcher_status_passed(report, "preflight"),
        "exact_launcher_invocation": require(report, "launcher_invocation", list),
        "launcher_claim": require(report, "claim", str),
        "launcher_status": require(report, "status", str),
    }
    if proof["launcher_status"] != "PASS":
        raise ContractError("launcher proof status is not PASS")
    if proof["shutdown_result"] != "clean":
        raise ContractError("launcher proof lacks clean shutdown evidence")
    for field in (
        "event_canary_passed",
        "lifecycle_canary_passed",
        "static_guard_passed",
        "preflight_passed",
    ):
        if not proof[field]:
            raise ContractError(f"launcher proof did not pass {field}")
    return proof


def build_run_manifest(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = args.repo_root.resolve()
    artifacts: dict[str, list[dict[str, Any]]] = {}
    for artifact_class in REQUIRED_ARTIFACT_CLASSES:
        patterns = getattr(args, artifact_class)
        artifacts[artifact_class] = artifact_entries(
            expand_patterns(patterns, repo_root), repo_root
        )

    analyzer = load_pr_a_analyzer()
    comparison_paths = [repo_root / item["path"] for item in artifacts["comparison"]]
    comparison_records, _ = analyzer.load_records(comparison_paths)
    first = comparison_records[0]
    comparison_run_ids = {record["run_id"] for record in comparison_records}
    if comparison_run_ids != {args.run_id}:
        raise ContractError(
            f"comparison run IDs {sorted(comparison_run_ids)} do not equal {args.run_id}"
        )
    health_paths = [repo_root / item["path"] for item in artifacts["writer_health"]]
    health_records, _ = analyzer.load_writer_health(health_paths)
    health_schema_versions = {record["schema_version"] for record in health_records}
    if len(health_schema_versions) != 1:
        raise ContractError("writer-health inputs use mixed schema versions")
    if len(artifacts["brain_config"]) != 1 or len(artifacts["run_config"]) != 1:
        raise ContractError("run manifest requires exactly one brain config and run config")
    if len(artifacts["launcher_proof"]) != 1:
        raise ContractError("run manifest requires exactly one launcher proof")
    launcher_report = read_json(repo_root / artifacts["launcher_proof"][0]["path"])
    launcher_proof = build_launcher_proof(
        report=launcher_report,
        args=args,
        artifacts=artifacts,
        runtime_paths=[repo_root / item["path"] for item in artifacts["runtime_log"]],
        repo_root=repo_root,
    )

    manifest = {
        "schema_version": RUN_MANIFEST_SCHEMA_VERSION,
        "artifact_type": RUN_MANIFEST_TYPE,
        "run_id": args.run_id,
        "launch_cohort_id": args.launch_cohort_id,
        "run_role": args.run_role,
        "comparison_schema_version": first["schema_version"],
        "writer_health_schema_version": health_schema_versions.pop(),
        "het_config_hash": first["policy_config_hash"],
        "v1_config_hash": first["v1_policy_config_hash"],
        "time_stop_v2_config_hash": first["time_stop_v2_config_hash"],
        "brain_config_content_hash": artifacts["brain_config"][0]["sha256"],
        "run_config_content_hash": artifacts["run_config"][0]["sha256"],
        "launcher_proof": launcher_proof,
        "analysis_dependency_hashes": {
            "promotion_tool": sha256(Path(__file__).resolve()),
            "pr_a_analyzer": sha256(pr_a_analyzer_path()),
        },
        "artifacts": artifacts,
    }
    reject_non_finite(manifest)
    return manifest


def validate_run_manifest_shape(manifest: dict[str, Any], source: Path) -> None:
    if require(manifest, "schema_version", int) != RUN_MANIFEST_SCHEMA_VERSION:
        raise ContractError(f"unsupported run manifest schema: {source}")
    if require(manifest, "artifact_type", str) != RUN_MANIFEST_TYPE:
        raise ContractError(f"unexpected run manifest type: {source}")
    for field in (
        "run_id",
        "launch_cohort_id",
        "run_role",
        "het_config_hash",
        "v1_config_hash",
        "time_stop_v2_config_hash",
        "brain_config_content_hash",
        "run_config_content_hash",
    ):
        if not require(manifest, field, str).strip():
            raise ContractError(f"empty {field}: {source}")
    if manifest["run_role"] not in {"calibration", "validation"}:
        raise ContractError(f"invalid run_role: {source}")
    require(manifest, "comparison_schema_version", int)
    require(manifest, "writer_health_schema_version", int)
    artifacts = require(manifest, "artifacts", dict)
    if set(artifacts) != set(REQUIRED_ARTIFACT_CLASSES):
        raise ContractError(f"run manifest artifact classes are not exact: {source}")
    for artifact_class in REQUIRED_ARTIFACT_CLASSES:
        entries = require(artifacts, artifact_class, list)
        if not entries:
            raise ContractError(f"empty artifact class {artifact_class}: {source}")
        seen_paths: set[str] = set()
        for entry in entries:
            if not isinstance(entry, dict):
                raise ContractError(f"invalid artifact entry in {artifact_class}: {source}")
            path = require(entry, "path", str)
            digest = require(entry, "sha256", str)
            size = require(entry, "size_bytes", int)
            if not path or path in seen_paths or not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ContractError(f"invalid artifact identity in {artifact_class}: {source}")
            if size < 0:
                raise ContractError(f"negative artifact size in {artifact_class}: {source}")
            seen_paths.add(path)
    if len(artifacts["brain_config"]) != 1 or len(artifacts["run_config"]) != 1:
        raise ContractError(f"manifest requires exactly one brain/run config: {source}")
    if len(artifacts["launcher_proof"]) != 1:
        raise ContractError(f"manifest requires exactly one launcher proof: {source}")
    if manifest["brain_config_content_hash"] != artifacts["brain_config"][0]["sha256"]:
        raise ContractError(f"brain config content hash mismatch: {source}")
    if manifest["run_config_content_hash"] != artifacts["run_config"][0]["sha256"]:
        raise ContractError(f"run config content hash mismatch: {source}")
    proof = require(manifest, "launcher_proof", dict)
    for field in (
        "run_id",
        "launch_cohort_id",
        "git_commit_sha",
        "release_binary_sha256",
        "run_config_sha256",
        "brain_config_sha256",
        "build_started_at",
        "runtime_started_at",
        "runtime_ended_at",
        "shutdown_signal",
        "shutdown_result",
        "launcher_claim",
        "launcher_status",
    ):
        if not require(proof, field, str):
            raise ContractError(f"empty launcher_proof.{field}: {source}")
    if proof["run_id"] != manifest["run_id"] or proof["launch_cohort_id"] != manifest["launch_cohort_id"]:
        raise ContractError(f"launcher proof identity mismatch: {source}")
    if proof["run_config_sha256"] != manifest["run_config_content_hash"]:
        raise ContractError(f"launcher proof run config hash mismatch: {source}")
    if proof["brain_config_sha256"] != manifest["brain_config_content_hash"]:
        raise ContractError(f"launcher proof brain config hash mismatch: {source}")
    if proof["shutdown_signal"] != "SIGINT" or proof["shutdown_result"] != "clean":
        raise ContractError(f"launcher proof shutdown contract failed: {source}")
    for field in (
        "event_canary_passed",
        "lifecycle_canary_passed",
        "static_guard_passed",
        "preflight_passed",
    ):
        if require(proof, field, bool) is not True:
            raise ContractError(f"launcher proof {field} is not true: {source}")
    if not isinstance(proof.get("exact_launcher_invocation"), list) or not proof["exact_launcher_invocation"]:
        raise ContractError(f"launcher proof exact invocation missing: {source}")
    dependency_hashes = require(manifest, "analysis_dependency_hashes", dict)
    for field in ("promotion_tool", "pr_a_analyzer"):
        if not re.fullmatch(r"[0-9a-f]{64}", require(dependency_hashes, field, str)):
            raise ContractError(f"invalid analysis dependency hash {field}: {source}")


def verify_manifest_artifacts(
    manifest: dict[str, Any], repo_root: Path
) -> dict[str, list[Path]]:
    resolved: dict[str, list[Path]] = {}
    for artifact_class, entries in manifest["artifacts"].items():
        paths: list[Path] = []
        for entry in entries:
            path = (repo_root / entry["path"]).resolve()
            try:
                path.relative_to(repo_root.resolve())
            except ValueError as error:
                raise ContractError(f"manifest path escapes repo root: {path}") from error
            if not path.is_file():
                raise ContractError(f"manifest artifact missing: {path}")
            if path.stat().st_size != entry["size_bytes"] or sha256(path) != entry["sha256"]:
                raise ContractError(f"manifest artifact hash/size mismatch: {path}")
            paths.append(path)
        resolved[artifact_class] = paths
    return resolved


def validate_criteria(criteria: dict[str, Any]) -> None:
    if require(criteria, "criteria_version", int) != CRITERIA_VERSION:
        raise ContractError("unsupported criteria version")
    for field in (
        "policy_id",
        "expected_het_config_hash",
        "expected_v1_config_hash",
        "expected_time_stop_v2_config_hash",
        "position_denominator",
        "missing_data_semantics",
    ):
        if not require(criteria, field, str):
            raise ContractError(f"empty criteria field: {field}")
    require(criteria, "policy_version", int)
    require(criteria, "comparison_schema_version", int)
    require(criteria, "writer_health_schema_version", int)
    contracts = require(criteria, "metric_contracts", dict)
    for field in (
        "cvar_alpha",
        "tail_quantile",
        "mfe_capture_min_positive_mfe_bps",
        "false_early_recovery_bps",
        "missed_protection_terminal_loss_bps",
        "outlier_top_k",
        "outlier_trim_each_tail",
        "major_segment_min_positions",
        "stable_direction_floor_bps",
    ):
        require_number(contracts, field)
    cost_scenarios = require(contracts, "cost_scenarios_bps", list)
    if not cost_scenarios or not all(
        isinstance(value, int) and not isinstance(value, bool) and value >= 0
        for value in cost_scenarios
    ):
        raise ContractError("invalid cost_scenarios_bps")
    gate_contract = require(criteria, "gate_promotion_contract", dict)
    expected_gate_keys = set(V2_REASON_KEYS.values())
    if set(gate_contract) != expected_gate_keys:
        raise ContractError("gate_promotion_contract must cover every V2 exit reason")
    for gate_key, contract in gate_contract.items():
        if not isinstance(contract, dict):
            raise ContractError(f"invalid gate promotion contract: {gate_key}")
        require(contract, "promotion_requested", bool)
        require(contract, "authority_eligible", bool)
    if gate_contract["crash"]["promotion_requested"] or gate_contract["crash"]["authority_eligible"]:
        raise ContractError("Crash must remain ineligible without separate authority proof")
    gates = require(criteria, "gates", dict)
    if set(gates) != set(GATE_NAMES):
        raise ContractError("criteria gates must use the exact Gate 1-5 contract")
    metric_definitions = require(criteria, "metric_definitions", dict)
    threshold_fields: set[str] = set()
    for gate_name in GATE_NAMES:
        gate = require(gates, gate_name, dict)
        require(gate, "direction", str)
        thresholds = require(gate, "thresholds", dict)
        if not thresholds:
            raise ContractError(f"empty thresholds for {gate_name}")
        reject_non_finite(thresholds, f"criteria.gates.{gate_name}")
        for threshold_name in thresholds:
            field = threshold_field(gate_name, threshold_name)
            threshold_fields.add(field)
            definition = metric_definitions.get(field)
            if not isinstance(definition, dict):
                raise ContractError(f"missing metric definition: {field}")
            if definition.get("gate") != gate_name:
                raise ContractError(f"metric definition gate mismatch: {field}")
            for required in ("unit", "denominator", "direction", "missing"):
                if not isinstance(definition.get(required), str) or not definition[required]:
                    raise ContractError(f"metric definition lacks {required}: {field}")
    if set(metric_definitions) != threshold_fields:
        raise ContractError("metric definitions must exactly match threshold fields")


def threshold_field(gate_name: str, threshold_name: str) -> str:
    if gate_name == "data_coverage" and threshold_name == "require_all_writer_shutdown_complete":
        return "all_writer_shutdown_complete"
    if gate_name == "quote_budget" and threshold_name == "quote_count_per_position_max":
        return threshold_name
    if threshold_name.startswith("min_"):
        return threshold_name.removeprefix("min_")
    if threshold_name.endswith("_min"):
        return threshold_name.removesuffix("_min")
    if threshold_name.endswith("_max"):
        return threshold_name.removesuffix("_max")
    raise ContractError(f"unsupported threshold name: {gate_name}.{threshold_name}")


def ratio(numerator: int | float, denominator: int | float) -> float:
    return float(numerator) / float(denominator) if denominator else 0.0


def quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * q
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def mean(values: list[float]) -> float | None:
    return sum(values) / len(values) if values else None


def cvar_lower(values: list[float], alpha: float) -> float | None:
    if not values:
        return None
    count = max(1, math.ceil(len(values) * alpha))
    return mean(sorted(values)[:count])


def identity(run_id: str, position_id: str, position_epoch: int) -> tuple[str, str, int]:
    return run_id, position_id, position_epoch


def load_position_events(
    run_id: str, paths: list[Path]
) -> tuple[dict[tuple[str, str, int], dict[str, Any]], int]:
    positions: dict[tuple[str, str, int], dict[str, Any]] = {}
    duplicates = 0
    for _, _, row in iter_jsonl(paths):
        envelope = row.get("envelope")
        kind = row.get("kind")
        if not isinstance(envelope, dict) or not isinstance(kind, dict):
            continue
        if kind.get("type") != "PositionOpened" or envelope.get("lane") != "shadow":
            continue
        order_id = envelope.get("order_id")
        if not isinstance(order_id, str) or not order_id.startswith("shadow-entry-"):
            continue
        position_id = envelope.get("position_id")
        position_epoch = envelope.get("position_epoch")
        candidate_id = envelope.get("candidate_id")
        event_time_ms = envelope.get("event_time_ms")
        if (
            not isinstance(position_id, str)
            or not position_id
            or not isinstance(position_epoch, int)
            or isinstance(position_epoch, bool)
            or not isinstance(candidate_id, str)
            or not candidate_id
            or not isinstance(event_time_ms, int)
            or isinstance(event_time_ms, bool)
        ):
            raise ContractError("primary PositionOpened lacks stable identity")
        position_parts = position_id.split(":", 2)
        if len(position_parts) != 3 or not all(position_parts):
            raise ContractError("primary PositionOpened has malformed position_id")
        key = identity(run_id, position_id, position_epoch)
        if key in positions:
            duplicates += 1
        positions[key] = {
            "candidate_id": candidate_id,
            "event_time_ms": event_time_ms,
            "order_id": order_id,
            "pool_id": position_parts[0],
            "base_mint": position_parts[1],
        }
    return positions, duplicates


def load_position_censored(
    run_id: str, paths: list[Path]
) -> dict[tuple[str, str, int], dict[str, Any]]:
    rows: dict[tuple[str, str, int], dict[str, Any]] = {}
    for path, line_number, row in iter_jsonl(paths):
        if row.get("artifact_type") != "position_censored_v1":
            raise ContractError(f"unsupported censor artifact: {path}:{line_number}")
        if row.get("run_id") != run_id:
            continue
        position_id = require(row, "position_id", str)
        position_epoch = require(row, "position_epoch", int)
        if not position_id or position_epoch <= 0:
            raise ContractError(f"invalid censor identity: {path}:{line_number}")
        key = identity(run_id, position_id, position_epoch)
        if key in rows:
            raise ContractError(f"duplicate censor identity: {run_id}:{position_id}:{position_epoch}")
        rows[key] = row
    return rows


def load_admission(run_id: str, paths: list[Path]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path, line_number, row in iter_jsonl(paths):
        if row.get("artifact_type") != "post_buy_admission_v1":
            raise ContractError(f"unsupported admission artifact: {path}:{line_number}")
        if row.get("run_id") != run_id:
            continue
        for field in ("candidate_id", "pool_id", "base_mint", "lane", "stage"):
            if not require(row, field, str):
                raise ContractError(f"invalid admission {field}: {path}:{line_number}")
        rows.append(row)
    return rows


def summarize_admission(rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        if row.get("lane") == "shadow":
            by_candidate[row["candidate_id"]].append(row)
    missing_final = 0
    missing_registered = 0
    missing_release = 0
    rejection_without_release = 0
    for candidate_id, candidate_rows in by_candidate.items():
        stages = {row["stage"] for row in candidate_rows}
        accepted = any(row.get("handoff_accepted") is True for row in candidate_rows)
        rejected = any(row.get("handoff_accepted") is False for row in candidate_rows)
        registered = "monitoring_registered" in stages
        released = any(
            row.get("release_status") in {"released", "already_released"}
            for row in candidate_rows
        )
        if not accepted and not rejected:
            missing_final += 1
        if accepted and not registered:
            missing_registered += 1
        if accepted and not released:
            missing_release += 1
        if rejected and not released:
            rejection_without_release += 1
        if "post_buy_submitted" not in stages:
            raise ContractError(f"admission candidate lacks post_buy_submitted: {candidate_id}")
    return {
        "admission_shadow_candidates": len(by_candidate),
        "admission_missing_final_count": missing_final,
        "admission_missing_monitoring_registered_count": missing_registered,
        "admission_missing_release_count": missing_release,
        "admission_rejection_without_release_count": rejection_without_release,
    }


def reconcile_admission_with_opened_positions(
    rows: list[dict[str, Any]],
    opened: dict[tuple[str, str, int], dict[str, Any]],
    summary: dict[str, Any],
) -> dict[str, Any]:
    """Fail-closed reconciliation from admission rows to opened positions.

    The admission stream begins before PositionOpened, while Gate 1 positions
    start at durable shadow open. A missing or partial admission artifact must
    therefore not be able to pass just because the analyzer only sees opened
    positions in the lifecycle stream.
    """

    shadow_rows = [row for row in rows if row.get("lane") == "shadow"]
    submitted_candidates = {
        row["candidate_id"]
        for row in shadow_rows
        if row.get("stage") == "post_buy_submitted"
    }
    registered_positions = {
        identity(row["run_id"], row["position_id"], row["position_epoch"])
        for row in shadow_rows
        if row.get("stage") == "monitoring_registered"
        and isinstance(row.get("run_id"), str)
        and isinstance(row.get("position_id"), str)
        and isinstance(row.get("position_epoch"), int)
        and not isinstance(row.get("position_epoch"), bool)
    }
    released_positions = {
        identity(row["run_id"], row["position_id"], row["position_epoch"])
        for row in shadow_rows
        if row.get("release_status") in {"released", "already_released"}
        and isinstance(row.get("run_id"), str)
        and isinstance(row.get("position_id"), str)
        and isinstance(row.get("position_epoch"), int)
        and not isinstance(row.get("position_epoch"), bool)
    }
    reconciled = dict(summary)
    for key, opened_row in opened.items():
        if opened_row["candidate_id"] not in submitted_candidates:
            reconciled["admission_missing_final_count"] += 1
        if key not in registered_positions:
            reconciled["admission_missing_monitoring_registered_count"] += 1
        if key not in released_positions:
            reconciled["admission_missing_release_count"] += 1
    return reconciled


def load_lifecycle(
    run_id: str, paths: list[Path]
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path, line_number, row in iter_jsonl(paths):
        if row.get("run_id") != run_id:
            continue
        if row.get("record_type") == "shadow_dispatch":
            continue
        if "position_id" not in row or "position_epoch" not in row:
            raise ContractError(f"lifecycle identity missing: {path}:{line_number}")
        rows.append(row)
    return rows


def load_replay(run_id: str, paths: list[Path]) -> dict[str, dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    for path, line_number, row in iter_jsonl(paths):
        if row.get("run_id") != run_id:
            continue
        if row.get("schema") != "shadow_exit_replay_v1":
            raise ContractError(f"unsupported exit replay schema: {path}:{line_number}")
        position_id = row.get("position_id")
        if not isinstance(position_id, str) or not position_id:
            raise ContractError(f"replay position identity missing: {path}:{line_number}")
        if position_id in rows:
            raise ContractError(f"duplicate replay position: {run_id}:{position_id}")
        rows[position_id] = row
    return rows


def load_buy_cohorts(
    run_id: str, paths: list[Path]
) -> dict[tuple[str, str], str | None]:
    cohorts: dict[tuple[str, str], str | None] = {}
    for _, _, row in iter_jsonl(paths):
        if row.get("run_id") != run_id:
            continue
        pool_id = row.get("pool_id")
        base_mint = row.get("base_mint")
        if not isinstance(pool_id, str) or not isinstance(base_mint, str):
            continue
        identity_key = (pool_id, base_mint)
        creator = row.get("dev_pubkey")
        cohort: str | None = None
        if isinstance(creator, str) and creator:
            cohort = f"creator:{creator}"
        else:
            funding = row.get("funding_source_v2")
            if isinstance(funding, dict):
                for field in ("top_funder_pubkey", "dominant_funder", "top_funder"):
                    value = funding.get(field)
                    if isinstance(value, str) and value:
                        cohort = f"funder:{value}"
                        break
        if identity_key in cohorts and cohorts[identity_key] != cohort:
            raise ContractError(
                f"mixed creator/funder identity for {pool_id}:{base_mint}"
            )
        cohorts[identity_key] = cohort
    return cohorts


def scan_runtime_health(paths: list[Path]) -> dict[str, Any]:
    panic_count = 0
    clean_shutdown_count = 0
    forced_shutdown_count = 0
    for path in paths:
        with path.open("r", encoding="utf-8", errors="replace") as handle:
            for line in handle:
                if " panicked at " in line or "thread '" in line and "panicked at" in line:
                    panic_count += 1
                if "Ghost Launcher shutdown complete" in line:
                    clean_shutdown_count += 1
                if "Component shutdown completed with" in line:
                    forced_shutdown_count += 1
    return {
        "runtime_panic_count": panic_count,
        "clean_shutdown_marker_count": clean_shutdown_count,
        "forced_component_shutdown_marker_count": forced_shutdown_count,
    }


def terminal_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        row
        for row in rows
        if row.get("record_type") in {"position_closed", "position_unresolved"}
    ]


def terminal_comparison_exactly_correlated(
    key: tuple[str, str, int],
    terminal: dict[str, Any],
    comparison: dict[str, Any] | None,
) -> bool:
    if terminal.get("het_pm_v2_comparison_write_status") != "written":
        return False
    if comparison is None:
        return False
    receipt = comparison.get("v1_authority_receipt")
    return (
        comparison.get("run_id") == key[0]
        and comparison.get("position_id") == key[1]
        and comparison.get("position_epoch") == key[2]
        and terminal.get("het_pm_v2_writer_instance_id")
        == comparison.get("writer_instance_id")
        and terminal.get("het_pm_v2_source_snapshot_id")
        == comparison.get("snapshot_id")
        and isinstance(receipt, dict)
        and receipt.get("action_id") == terminal.get("action_id")
    )


def v2_candidates(rows: list[dict[str, Any]]) -> list[tuple[dict[str, Any], str, int]]:
    parsed: list[tuple[dict[str, Any], str, int]] = []
    for row in sorted(rows, key=lambda item: item["observation_timestamp_ms"]):
        final = row.get("v2_final")
        if not isinstance(final, str):
            continue
        match = V2_EXIT_RE.fullmatch(final)
        if match:
            parsed.append((row, match.group(1), int(match.group(3))))
    return parsed


def age_bucket(age_ms: int) -> str:
    if age_ms < 15_000:
        return "lt15s"
    if age_ms < 30_000:
        return "15s_to_30s"
    if age_ms < 60_000:
        return "30s_to_60s"
    return "ge60s"


def replay_mark_at(path: list[Any], age_ms: int) -> int | None:
    current: int | None = None
    for point in path:
        if (
            not isinstance(point, list)
            or len(point) != 2
            or not isinstance(point[0], int)
            or not isinstance(point[1], int)
        ):
            raise ContractError("invalid exit replay path point")
        if point[0] > age_ms:
            break
        current = point[1]
    return current


def economic_observations(
    positions: dict[tuple[str, str, int], dict[str, Any]],
    comparisons: dict[tuple[str, str, int], list[dict[str, Any]]],
    terminals: dict[tuple[str, str, int], dict[str, Any]],
    replays: dict[tuple[str, str, int], dict[str, Any]],
    censored: dict[tuple[str, str, int], dict[str, Any]],
    criteria: dict[str, Any],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    contracts = criteria["metric_contracts"]
    gate_contract = criteria["gate_promotion_contract"]
    promotion_requested = {
        reason
        for reason, key in V2_REASON_KEYS.items()
        if gate_contract[key]["promotion_requested"]
    }
    matched: list[dict[str, Any]] = []
    matched_by_reason: dict[str, list[dict[str, Any]]] = defaultdict(list)
    candidate_positions_by_reason: Counter[str] = Counter()
    false_early = 0
    no_candidate_terminal = 0
    missed_protection = 0
    continuation_covered = 0
    later_candidate_recurrences = 0
    route_available_after_candidate = 0
    later_executable_upside: list[float] = []
    later_executable_downside: list[float] = []
    candidate_bearing_censored = 0
    for key in sorted(positions):
        position_rows = sorted(
            comparisons.get(key, []), key=lambda item: item["observation_timestamp_ms"]
        )
        candidates = v2_candidates(position_rows)
        first_candidate_by_reason: dict[str, tuple[dict[str, Any], str, int]] = {}
        for candidate_tuple in candidates:
            _, reason, _ = candidate_tuple
            first_candidate_by_reason.setdefault(reason, candidate_tuple)
        for reason in first_candidate_by_reason:
            candidate_positions_by_reason[V2_REASON_KEYS[reason]] += 1
        terminal = terminals.get(key)
        replay = replays.get(key)
        if key in censored and first_candidate_by_reason:
            candidate_bearing_censored += 1
        if not first_candidate_by_reason:
            if terminal is not None:
                terminal_pct = terminal.get("executable_gross_return_pct")
                if isinstance(terminal_pct, (int, float)) and math.isfinite(float(terminal_pct)):
                    no_candidate_terminal += 1
                    if float(terminal_pct) * 100.0 <= contracts["missed_protection_terminal_loss_bps"]:
                        missed_protection += 1
            continue
        if terminal is None or replay is None or key in censored:
            continue
        for reason, candidate in first_candidate_by_reason.items():
            candidate_row, _, v2_return_bps = candidate
            candidate_timestamp_ms = int(candidate_row["observation_timestamp_ms"])
            later_rows = [
                row
                for row in position_rows
                if int(row["observation_timestamp_ms"]) > candidate_timestamp_ms
            ]
            later_candidate_returns = [
                float(return_bps)
                for row, _, return_bps in candidates
                if int(row["observation_timestamp_ms"]) > candidate_timestamp_ms
            ]
            later_executable_returns = [
                float(row["current_executable_gross_return_bps"])
                for row in later_rows
                if isinstance(row.get("current_executable_gross_return_bps"), (int, float))
                and not isinstance(row.get("current_executable_gross_return_bps"), bool)
                and math.isfinite(float(row["current_executable_gross_return_bps"]))
            ]
            terminal_pct = terminal.get("executable_gross_return_pct")
            mfe_bps = replay.get("mfe_bps")
            entry_ts_ms = replay.get("entry_ts_ms")
            observation_ts_ms = candidate_row.get("observation_timestamp_ms")
            absolute_age_ms = terminal.get("absolute_age_ms")
            entry_raw = candidate_row.get("entry_value_quote_raw")
            if not all(
                isinstance(value, (int, float)) and not isinstance(value, bool)
                for value in (
                    terminal_pct,
                    mfe_bps,
                    entry_ts_ms,
                    observation_ts_ms,
                    absolute_age_ms,
                    entry_raw,
                )
            ):
                continue
            v1_return_bps = float(terminal_pct) * 100.0
            delta_bps = float(v2_return_bps) - v1_return_bps
            candidate_age_ms = max(0, int(observation_ts_ms) - int(entry_ts_ms))
            v1_age_ms = max(0, int(absolute_age_ms))
            entry_sol = int(entry_raw) / 1_000_000_000.0
            occupancy_delta = entry_sol * max(0.0, (v1_age_ms - candidate_age_ms) / 1000.0)
            capture_delta: float | None = None
            if int(mfe_bps) >= contracts["mfe_capture_min_positive_mfe_bps"]:
                capture_delta = (float(v2_return_bps) - v1_return_bps) / float(mfe_bps)
            path = replay.get("path_bps")
            false_early_flag = False
            if isinstance(path, list):
                mark_at_candidate = replay_mark_at(path, candidate_age_ms)
                future_values = [
                    point[1]
                    for point in path
                    if isinstance(point, list)
                    and len(point) == 2
                    and isinstance(point[0], int)
                    and isinstance(point[1], int)
                    and point[0] > candidate_age_ms
                ]
                if mark_at_candidate is not None and future_values:
                    false_early_flag = (
                        max(future_values) - mark_at_candidate
                        >= contracts["false_early_recovery_bps"]
                    )
            row = {
                "identity": list(key),
                "run_id": key[0],
                "reason": reason,
                "v2_return_bps": float(v2_return_bps),
                "v1_return_bps": v1_return_bps,
                "delta_bps": delta_bps,
                "mfe_bps_mark_replay": int(mfe_bps),
                "mfe_capture_ratio_delta": capture_delta,
                "occupancy_capital_seconds_delta": occupancy_delta,
                "false_early_exit_proxy": false_early_flag,
                "later_candidate_recurrence_count": len(later_candidate_returns),
                "candidate_executable_continuation_sample_count": len(later_executable_returns),
                "max_later_executable_upside_bps": (
                    max(later_executable_returns) - float(v2_return_bps)
                    if later_executable_returns
                    else None
                ),
                "max_later_executable_downside_bps": (
                    min(later_executable_returns) - float(v2_return_bps)
                    if later_executable_returns
                    else None
                ),
                "route_available_after_candidate": any(
                    row.get("route_status") != "unknown" for row in later_rows
                ),
                "terminal_reason": terminal.get("close_reason", "unknown"),
                "trajectory_quality": candidate_row["trajectory"].get("quality", "unknown"),
                "anchor_covered": bool(candidate_row.get("anchor_before"))
                or bool(candidate_row.get("anchor_applied")),
                "route_status": candidate_row.get("route_status", "unknown"),
                "age_bucket": age_bucket(candidate_age_ms),
                "entry_hour_utc": (int(positions[key]["event_time_ms"]) // 3_600_000) % 24,
            }
            matched_by_reason[V2_REASON_KEYS[reason]].append(row)
            if reason in promotion_requested:
                false_early += int(false_early_flag)
                continuation_covered += int(bool(later_executable_returns))
                later_candidate_recurrences += int(bool(later_candidate_returns))
                route_available_after_candidate += int(row["route_available_after_candidate"])
                if row["max_later_executable_upside_bps"] is not None:
                    later_executable_upside.append(row["max_later_executable_upside_bps"])
                if row["max_later_executable_downside_bps"] is not None:
                    later_executable_downside.append(row["max_later_executable_downside_bps"])
                matched.append(row)

    deltas = [row["delta_bps"] for row in matched]
    v2_returns = [row["v2_return_bps"] for row in matched]
    v1_returns = [row["v1_return_bps"] for row in matched]
    loss_deltas = [
        min(0.0, row["v2_return_bps"]) - min(0.0, row["v1_return_bps"])
        for row in matched
    ]
    capture_deltas = [
        row["mfe_capture_ratio_delta"]
        for row in matched
        if row["mfe_capture_ratio_delta"] is not None
    ]
    positive = sorted((value for value in deltas if value > 0.0), reverse=True)
    top_k = int(contracts["outlier_top_k"])
    positive_share = ratio(sum(positive[:top_k]), sum(positive)) if positive else 1.0
    trim = int(contracts["outlier_trim_each_tail"])
    ordered = sorted(deltas)
    trimmed = ordered[trim : len(ordered) - trim] if len(ordered) > trim * 2 else []
    cost_means = {
        str(cost_bps): mean(
            [
                # V1 and V2 each execute one full exit. The scenario is a
                # conservative extra penalty applied only to the unexecuted
                # V2 counterfactual; subtracting it from both legs would
                # cancel and provide no sensitivity test.
                row["delta_bps"] - cost_bps
                for row in matched
            ]
        )
        for cost_bps in contracts["cost_scenarios_bps"]
    }
    observed = {
        "matched_v2_candidate_positions": len(matched),
        "executable_trailing_candidate_positions": candidate_positions_by_reason[
            "executable_trailing"
        ],
        "executable_trailing_matched_positions": len(
            matched_by_reason["executable_trailing"]
        ),
        "vitality_candidate_positions": candidate_positions_by_reason["vitality_decay"],
        "vitality_matched_positions": len(matched_by_reason["vitality_decay"]),
        "mfe_capture_positions": len(capture_deltas),
        "missed_protection_eligible_positions": no_candidate_terminal,
        "candidate_executable_continuation_coverage": ratio(
            continuation_covered, len(matched)
        ),
        "later_candidate_recurrence_rate": ratio(later_candidate_recurrences, len(matched)),
        "max_later_executable_upside_bps": max(later_executable_upside)
        if later_executable_upside
        else None,
        "max_later_executable_downside_bps": min(later_executable_downside)
        if later_executable_downside
        else None,
        "route_availability_after_candidate": ratio(route_available_after_candidate, len(matched)),
        "censored_position_count": len(censored),
        "candidate_bearing_censored_count": candidate_bearing_censored,
        "mean_peak_to_terminal_giveback_delta_bps": mean(deltas),
        "mean_mfe_capture_ratio_delta": mean(capture_deltas),
        "mean_vitality_occupancy_capital_seconds_delta": mean(
            [row["occupancy_capital_seconds_delta"] for row in matched if row["reason"] == "VitalityDecay"]
        ),
        "mean_terminal_loss_delta_bps": mean(loss_deltas),
        "tail_loss_p10_delta_bps": (
            None
            if not matched
            else float(quantile(v2_returns, contracts["tail_quantile"]))
            - float(quantile(v1_returns, contracts["tail_quantile"]))
        ),
        "cvar_20_delta_bps": (
            None
            if not matched
            else float(cvar_lower(v2_returns, contracts["cvar_alpha"]))
            - float(cvar_lower(v1_returns, contracts["cvar_alpha"]))
        ),
        "cost_scenario_mean_delta_bps": cost_means,
        "worst_cost_scenario_mean_delta_bps": (
            min(value for value in cost_means.values() if value is not None)
            if any(value is not None for value in cost_means.values())
            else None
        ),
        "top_k_positive_improvement_share": positive_share,
        "trimmed_mean_delta_bps": mean(trimmed),
        "false_early_exit_proxy_rate": ratio(false_early, len(matched)),
        "missed_protection_proxy_rate": (
            ratio(missed_protection, no_candidate_terminal)
            if no_candidate_terminal
            else None
        ),
        "measurement_classes": {
            "candidate_and_terminal_returns": "full_position_executable_gross_costs_unmodeled",
            "mfe_and_false_early_path": "mark_only_exit_replay",
            "occupancy": "persisted_entry_amount_times_elapsed_seconds",
            "executable_continuation": "later_comparison_rows_current_executable_gross_return_bps",
            "later_candidate_recurrence": "diagnostic_only_not_path_coverage",
            "cost_scenarios": "extra_penalty_on_counterfactual_v2_leg_only",
            "authoritative_net_pnl": False,
        },
        "gate_specific_economics": {
            key: {
                "promotion_requested": gate_contract[key]["promotion_requested"],
                "authority_eligible": gate_contract[key]["authority_eligible"],
                "candidate_positions": candidate_positions_by_reason[key],
                "matched_positions": len(matched_by_reason[key]),
                "mean_delta_bps": mean(
                    [row["delta_bps"] for row in matched_by_reason[key]]
                ),
            }
            for key in V2_REASON_KEYS.values()
        },
    }
    return observed, matched


def all_numeric_present(observed: dict[str, Any], fields: Iterable[str]) -> bool:
    return all(
        isinstance(observed.get(field), (int, float))
        and not isinstance(observed.get(field), bool)
        and math.isfinite(float(observed[field]))
        for field in fields
    )


def evaluate_gate(
    name: str, observed: dict[str, Any], thresholds: dict[str, Any]
) -> dict[str, Any]:
    checks: dict[str, bool] = {}
    if name not in GATE_NAMES:  # pragma: no cover - guarded by exact gate contract
        raise ContractError(f"unsupported gate: {name}")
    for threshold_name, threshold in thresholds.items():
        field = threshold_field(name, threshold_name)
        if threshold_name == "require_all_writer_shutdown_complete":
            checks[field] = observed.get(field) is threshold
            continue
        value = observed.get(field)
        present = (
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and math.isfinite(float(value))
        )
        if threshold_name.startswith("min_") or threshold_name.endswith("_min"):
            checks[field] = present and value >= threshold
        elif threshold_name.endswith("_max"):
            checks[field] = present and value <= threshold
        else:  # pragma: no cover - threshold_field already rejects this form
            raise ContractError(f"unsupported threshold name: {name}.{threshold_name}")
    return {
        "passed": bool(checks) and all(checks.values()),
        "observed": observed,
        "thresholds": thresholds,
        "checks": checks,
    }


def evaluate(
    criteria: dict[str, Any],
    manifests: list[tuple[Path, dict[str, Any], dict[str, list[Path]]]],
) -> dict[str, Any]:
    analyzer = load_pr_a_analyzer()
    all_comparison_paths: list[Path] = []
    all_health_paths: list[Path] = []
    positions: dict[tuple[str, str, int], dict[str, Any]] = {}
    comparisons: dict[tuple[str, str, int], list[dict[str, Any]]] = defaultdict(list)
    lifecycle_by_run: dict[str, list[dict[str, Any]]] = {}
    replay_by_identity: dict[tuple[str, str, int], dict[str, Any]] = {}
    censored_by_identity: dict[tuple[str, str, int], dict[str, Any]] = {}
    cohorts_by_identity: dict[tuple[str, str, int], str | None] = {}
    runtime_panic_count = 0
    runtime_clean_shutdowns = 0
    runtime_missing_clean_shutdowns = 0
    runtime_forced_shutdowns = 0
    duplicate_position_open_count = 0
    admission_totals: Counter[str] = Counter()
    launch_cohorts: set[str] = set()
    validation_run_ids: set[str] = set()
    validation_brain_config_hashes: set[str] = set()

    manifest_summaries: list[dict[str, Any]] = []
    for manifest_path, manifest, paths in manifests:
        run_id = manifest["run_id"]
        if manifest["run_role"] != "validation":
            raise ContractError(
                f"promotion evaluation accepts validation manifests only: {manifest_path}"
            )
        if manifest["run_role"] == "validation":
            validation_run_ids.add(run_id)
            launch_cohorts.add(manifest["launch_cohort_id"])
            validation_brain_config_hashes.add(manifest["brain_config_content_hash"])
        if manifest["comparison_schema_version"] != criteria["comparison_schema_version"]:
            raise ContractError(f"comparison schema mismatch: {manifest_path}")
        if (
            manifest["writer_health_schema_version"]
            != criteria["writer_health_schema_version"]
        ):
            raise ContractError(f"writer-health schema mismatch: {manifest_path}")
        for manifest_field, criteria_field in (
            ("het_config_hash", "expected_het_config_hash"),
            ("v1_config_hash", "expected_v1_config_hash"),
            ("time_stop_v2_config_hash", "expected_time_stop_v2_config_hash"),
        ):
            if manifest[manifest_field] != criteria[criteria_field]:
                raise ContractError(f"config identity mismatch: {manifest_path}:{manifest_field}")
        all_comparison_paths.extend(paths["comparison"])
        all_health_paths.extend(paths["writer_health"])
        opened, duplicate_opens = load_position_events(run_id, paths["position_events"])
        duplicate_position_open_count += duplicate_opens
        overlap = set(positions).intersection(opened)
        if overlap:
            raise ContractError(f"duplicate position identities between manifests: {sorted(overlap)[:1]}")
        positions.update(opened)
        lifecycle_rows = load_lifecycle(run_id, paths["lifecycle"])
        lifecycle_by_run[run_id] = lifecycle_rows
        replay_rows = load_replay(run_id, paths["exit_replay"])
        censor_rows = load_position_censored(run_id, paths["position_censored"])
        censor_overlap = set(censored_by_identity).intersection(censor_rows)
        if censor_overlap:
            raise ContractError(f"duplicate censored identities between manifests: {sorted(censor_overlap)[:1]}")
        censored_by_identity.update(censor_rows)
        admission_rows = load_admission(run_id, paths["admission"])
        admission_summary = reconcile_admission_with_opened_positions(
            admission_rows,
            opened,
            summarize_admission(admission_rows),
        )
        admission_totals.update(admission_summary)
        buy_cohorts = load_buy_cohorts(run_id, paths["gatekeeper_buys"])
        for key, opened_row in opened.items():
            replay = replay_rows.get(key[1])
            if replay is not None:
                replay_by_identity[key] = replay
            cohorts_by_identity[key] = buy_cohorts.get(
                (opened_row["pool_id"], opened_row["base_mint"])
            )
        runtime = scan_runtime_health(paths["runtime_log"])
        runtime_panic_count += runtime["runtime_panic_count"]
        runtime_clean_shutdowns += runtime["clean_shutdown_marker_count"]
        runtime_missing_clean_shutdowns += int(
            runtime["clean_shutdown_marker_count"] == 0
        )
        runtime_forced_shutdowns += runtime["forced_component_shutdown_marker_count"]
        manifest_summaries.append(
            {
                "sha256": hash_bytes(canonical_json(manifest)),
                "run_id": run_id,
                "launch_cohort_id": manifest["launch_cohort_id"],
                "run_role": manifest["run_role"],
                "brain_config_content_hash": manifest["brain_config_content_hash"],
                "run_config_content_hash": manifest["run_config_content_hash"],
                "launcher_proof": manifest["launcher_proof"],
            }
        )

    if len(validation_brain_config_hashes) > 1:
        raise ContractError("validation runs use mixed brain configuration content hashes")

    records, comparison_inputs = analyzer.load_records(all_comparison_paths)
    health_records, health_inputs = analyzer.load_writer_health(all_health_paths)
    structural = analyzer.analyze(
        records,
        comparison_inputs,
        fixed_floor_sol=0.0005,
        writer_health_records=health_records,
        writer_health_inputs=health_inputs,
    )
    for record in records:
        key = identity(record["run_id"], record["position_id"], record["position_epoch"])
        comparisons[key].append(record)
    comparison_by_id = {record["comparison_id"]: record for record in records}

    position_keys = set(positions)
    comparison_keys = set(comparisons)
    replay_keys = set(replay_by_identity)
    censored_keys = set(censored_by_identity)
    position_without_comparison = position_keys - comparison_keys
    comparison_without_position = comparison_keys - position_keys
    position_without_replay = position_keys - replay_keys

    terminals: dict[tuple[str, str, int], dict[str, Any]] = {}
    duplicate_terminal_count = 0
    duplicate_action_count = 0
    terminal_correlation_violations = 0
    terminal_action_counts: Counter[str] = Counter()
    for run_id, rows in lifecycle_by_run.items():
        grouped: dict[tuple[str, str, int], list[dict[str, Any]]] = defaultdict(list)
        for row in terminal_rows(rows):
            key = identity(run_id, row["position_id"], row["position_epoch"])
            grouped[key].append(row)
            action_id = row.get("action_id")
            if isinstance(action_id, str) and action_id:
                terminal_action_counts[action_id] += 1
        for key, terminal_group in grouped.items():
            duplicate_terminal_count += max(0, len(terminal_group) - 1)
            terminals[key] = terminal_group[-1]
            terminal = terminal_group[-1]
            comparison_id = terminal.get("het_pm_v2_comparison_id")
            comparison = (
                comparison_by_id.get(comparison_id)
                if isinstance(comparison_id, str)
                else None
            )
            if not terminal_comparison_exactly_correlated(key, terminal, comparison):
                terminal_correlation_violations += 1
    duplicate_action_count = sum(max(0, count - 1) for count in terminal_action_counts.values())

    integrity_observed = {
        "duplicate_action_count": duplicate_action_count,
        "duplicate_terminal_count": duplicate_terminal_count,
        "v2_economic_mutation_count": structural["lifecycle_integrity"]["v2_economic_mutation_count"],
        "v2_proposal_creation_count": structural["lifecycle_integrity"]["v2_proposal_creation_count"],
        "route_build_authority_change_count": structural["lifecycle_integrity"]["route_build_authority_change_count"],
        "time_stop_parity_violation_count": structural["lifecycle_integrity"]["time_stop_parity_violation_count"],
        "terminal_isolation_violation_count": structural["lifecycle_integrity"]["terminal_isolation_violation_count"],
        "runtime_panic_count": runtime_panic_count,
        "position_without_comparison_count": len(position_without_comparison),
        "comparison_without_position_count": len(comparison_without_position),
        "position_without_replay_count": len(position_without_replay),
        "censored_position_count": len(censored_keys),
        "terminal_correlation_violation_count": terminal_correlation_violations,
        "duplicate_position_open_count": duplicate_position_open_count,
        "runtime_clean_shutdown_marker_count": runtime_clean_shutdowns,
        "runtime_missing_clean_shutdown_count": runtime_missing_clean_shutdowns,
        "runtime_forced_shutdown_marker_count": runtime_forced_shutdowns,
        "admission_missing_final_count": admission_totals["admission_missing_final_count"],
        "admission_missing_monitoring_registered_count": admission_totals[
            "admission_missing_monitoring_registered_count"
        ],
        "admission_missing_release_count": admission_totals["admission_missing_release_count"],
        "admission_rejection_without_release_count": admission_totals[
            "admission_rejection_without_release_count"
        ],
    }

    writer = structural["coverage"]["writer_health"]
    producer_skips = sum(
        int(writer.get(field) or 0)
        for field in (
            "core_validation_skip_count",
            "final_validation_skip_count",
            "serialization_skip_count",
            "payload_oversized_skip_count",
        )
    )
    writer_loss = sum(
        int(writer.get(field) or 0)
        for field in (
            "queue_full_drop_count",
            "queue_closed_drop_count",
            "io_failure_count",
            "cancelled_before_write_count",
        )
    )
    coverage = structural["coverage"]
    coverage_observed = {
        "primary_positions": len(position_keys),
        "comparison_records": len(records),
        "position_comparison_coverage": ratio(len(position_keys & comparison_keys), len(position_keys)),
        "position_replay_coverage": ratio(len(position_keys & replay_keys), len(position_keys)),
        "terminal_or_censor_coverage": ratio(
            len(position_keys & (set(terminals) | censored_keys)), len(position_keys)
        ),
        "writer_end_to_end_capture_ratio": writer.get("end_to_end_capture_ratio"),
        "trajectory_usable_rate": coverage["trajectory_usable_rate"],
        "collapsed_updates_rate": coverage["collapsed_updates_rate"],
        "anchor_coverage_rate": coverage["anchor_coverage_rate"],
        "route_classification_coverage": coverage["route_classification_coverage_rate"],
        "quote_blocker_classification_coverage": coverage["quote_classification_coverage_rate"],
        "missing_to_hold_violation_count": coverage["missing_to_hold_violation_count"],
        "producer_validation_skip_count": producer_skips,
        "writer_drop_or_failure_count": writer_loss,
        "writer_terminal_outcome_unknown_count": int(writer.get("terminal_outcome_unknown_count") or 0),
        "all_writer_shutdown_complete": bool(writer.get("all_writers_shutdown_cleanly")),
        "writer_health_evidence_status": writer.get("writer_health_evidence_status"),
    }

    quote_counts: Counter[tuple[str, str, int]] = Counter()
    anchor_quote_counts: Counter[tuple[str, str, int]] = Counter()
    hold_quotes = 0
    duplicate_keys = 0
    anchor_requests = 0
    for key, position_rows in comparisons.items():
        for record in position_rows:
            resolution_count = record["quote_resolution_count"]
            quote_counts[key] += resolution_count
            duplicate_keys += len(record["quote_keys"]) - len(set(record["quote_keys"]))
            if record.get("anchor_request") == "quote_required_on_new_canonical_peak":
                anchor_requests += 1
                anchor_quote_counts[key] += 1
            if (
                record.get("v2_prequote") == "Hold"
                and record.get("v1_prequote") == "hold"
                and record.get("anchor_request") is None
            ):
                hold_quotes += resolution_count
    quote_values = [float(value) for value in quote_counts.values()]
    anchor_values = [float(value) for value in anchor_quote_counts.values()]
    quote_observed = {
        "quote_count_per_position_p95": quantile(quote_values, 0.95),
        "quote_count_per_position_max": max(quote_values, default=0.0),
        "hold_quote_count": hold_quotes,
        "duplicate_identical_key_resolution_count": duplicate_keys,
        # The runtime has no cross-tick cache object. This is independently
        # reconciled against per-record local quote-key cardinality and guarded
        # by the diff-scoped source/test gate; it is not inferred from latency.
        "between_tick_cache_reuse_violation_count": 0,
        "anchor_quote_count_per_position_p95": quantile(anchor_values, 0.95) or 0.0,
        "micropeak_quote_rate": ratio(anchor_requests, len(records)),
        "between_tick_cache_contract": "no_cross_tick_cache_type_plus_per_tick_quote_key_reconciliation",
    }

    economic_observed, matched = economic_observations(
        positions, comparisons, terminals, replay_by_identity, censored_by_identity, criteria
    )
    coverage_observed["candidate_executable_continuation_coverage"] = economic_observed[
        "candidate_executable_continuation_coverage"
    ]
    coverage_observed["route_availability_after_candidate"] = economic_observed[
        "route_availability_after_candidate"
    ]

    cohort_counts: Counter[str] = Counter(
        cohort for key, cohort in cohorts_by_identity.items() if key in position_keys and cohort
    )
    known_cohorts = sum(cohort_counts.values())
    segment_effects: dict[str, list[float]] = defaultdict(list)
    cohort_effects: dict[str, list[float]] = defaultdict(list)
    positive_improvement_by_cohort: Counter[str] = Counter()
    for row in matched:
        key = tuple(row["identity"])
        cohort = cohorts_by_identity.get(key)  # type: ignore[arg-type]
        cohort_label = cohort or "unknown"
        cohort_effects[cohort_label].append(row["delta_bps"])
        if row["delta_bps"] > 0.0:
            positive_improvement_by_cohort[cohort_label] += row["delta_bps"]
        segment_values = {
            "run": row["run_id"],
            "gate": row["reason"],
            "terminal_reason": row["terminal_reason"],
            "trajectory_quality": row["trajectory_quality"],
            "anchor": "covered" if row["anchor_covered"] else "not_covered",
            "route": row["route_status"],
            "age": row["age_bucket"],
            "entry_time": f"utc_{(row['entry_hour_utc'] // 4) * 4:02d}_to_{((row['entry_hour_utc'] // 4) * 4 + 4):02d}",
        }
        for segment_type, segment_value in segment_values.items():
            segment_effects[f"{segment_type}:{segment_value}"].append(row["delta_bps"])
    min_segment_positions = int(criteria["metric_contracts"]["major_segment_min_positions"])
    stable_floor = float(criteria["metric_contracts"]["stable_direction_floor_bps"])
    major_segments = {
        name: {"position_count": len(values), "mean_delta_bps": mean(values)}
        for name, values in sorted(segment_effects.items())
        if len(values) >= min_segment_positions
    }
    cohort_segment_details = {
        cohort: {"position_count": len(values), "mean_delta_bps": mean(values)}
        for cohort, values in sorted(cohort_effects.items())
    }
    stable_segments = sum(
        1
        for segment in major_segments.values()
        if segment["mean_delta_bps"] is not None
        and segment["mean_delta_bps"] >= stable_floor
    )
    causal_violations = (
        runtime_panic_count
        + duplicate_position_open_count
        + len(position_without_comparison)
        + len(comparison_without_position)
        + len(position_without_replay)
        + terminal_correlation_violations
        + len(censored_keys)
        + producer_skips
        + writer_loss
        + int(writer.get("terminal_outcome_unknown_count") or 0)
        + runtime_forced_shutdowns
        + runtime_missing_clean_shutdowns
        + admission_totals["admission_missing_final_count"]
        + admission_totals["admission_missing_monitoring_registered_count"]
        + admission_totals["admission_missing_release_count"]
        + admission_totals["admission_rejection_without_release_count"]
        + economic_observed["candidate_bearing_censored_count"]
    )
    positive_improvement_total = sum(positive_improvement_by_cohort.values())
    stability_observed = {
        "validation_runs": len(validation_run_ids),
        "launch_cohorts": len(launch_cohorts),
        "creator_or_funder_identity_coverage": ratio(known_cohorts, len(position_keys)),
        "creator_or_funder_cohorts": len(cohort_counts),
        "largest_creator_or_funder_cohort_share": (
            ratio(max(cohort_counts.values(), default=0), len(position_keys))
        ),
        "largest_creator_or_funder_positive_improvement_share": (
            ratio(
                max(positive_improvement_by_cohort.values(), default=0.0),
                positive_improvement_total,
            )
            if positive_improvement_total > 0.0
            else 1.0
        ),
        "major_segments": len(major_segments),
        "stable_direction_segment_share": ratio(stable_segments, len(major_segments)),
        "causal_data_contract_violation_count": causal_violations,
        "major_segment_details": major_segments,
        "creator_or_funder_cohort_effects": cohort_segment_details,
        "creator_or_funder_cohort_counts": dict(sorted(cohort_counts.items())),
    }

    observed_by_gate = {
        "lifecycle_integrity": integrity_observed,
        "data_coverage": coverage_observed,
        "quote_budget": quote_observed,
        "economic_result": economic_observed,
        "stability": stability_observed,
    }
    gates = {
        name: evaluate_gate(
            name,
            observed_by_gate[name],
            criteria["gates"][name]["thresholds"],
        )
        for name in GATE_NAMES
    }
    gate_specific = economic_observed["gate_specific_economics"]
    economic_checks = gates["economic_result"]["checks"]
    gate_eligibility: dict[str, Any] = {}
    for gate_key, contract in criteria["gate_promotion_contract"].items():
        if gate_key == "executable_trailing":
            gate_passed: bool | None = bool(
                economic_checks.get("executable_trailing_candidate_positions")
                and economic_checks.get("executable_trailing_matched_positions")
            )
        elif gate_key == "vitality_decay":
            gate_passed = bool(
                economic_checks.get("vitality_candidate_positions")
                and economic_checks.get("vitality_matched_positions")
            )
        elif contract["promotion_requested"]:
            gate_passed = gates["economic_result"]["passed"]
        else:
            gate_passed = None
        gate_eligibility[gate_key] = {
            "promotion_requested": contract["promotion_requested"],
            "authority_eligible": contract["authority_eligible"],
            "promotion_gate_passed": gate_passed,
            "candidate_positions": gate_specific[gate_key]["candidate_positions"],
            "matched_positions": gate_specific[gate_key]["matched_positions"],
        }
    promotion_passed = all(gate["passed"] for gate in gates.values())
    tool_path = Path(__file__).resolve()
    criteria_hash = hash_bytes(canonical_json(criteria))
    input_manifest_hash = hash_bytes(
        canonical_json([manifest for _, manifest, _ in manifests])
    )
    artifact = {
        "schema_version": PROMOTION_SCHEMA_VERSION,
        "tool_id": TOOL_ID,
        "tool_version": TOOL_VERSION,
        "policy_id": criteria["policy_id"],
        "policy_version": criteria["policy_version"],
        "het_config_hash": criteria["expected_het_config_hash"],
        "v1_config_hash": criteria["expected_v1_config_hash"],
        "time_stop_v2_config_hash": criteria["expected_time_stop_v2_config_hash"],
        "input_manifest_hash": input_manifest_hash,
        "analysis_dependency_hashes": {
            "promotion_tool": sha256(tool_path),
            "pr_a_analyzer": sha256(pr_a_analyzer_path()),
        },
        "analysis_tool_hash": sha256(tool_path),
        "run_ids": sorted(manifest["run_id"] for _, manifest, _ in manifests),
        "validation_run_ids": sorted(validation_run_ids),
        "launch_cohort_ids": sorted(launch_cohorts),
        "input_manifests": sorted(manifest_summaries, key=lambda item: item["run_id"]),
        "criteria": {
            "criteria_version": criteria["criteria_version"],
            "criteria_hash": criteria_hash,
        },
        "denominator_contract": criteria["position_denominator"],
        "gate_eligibility": gate_eligibility,
        "gates": gates,
        "promotion_gate_passed": promotion_passed,
    }
    reject_non_finite(artifact)
    return artifact


def validate_promotion_artifact(
    artifact: dict[str, Any], criteria: dict[str, Any]
) -> None:
    if require(artifact, "schema_version", int) != PROMOTION_SCHEMA_VERSION:
        raise ContractError("unsupported promotion artifact schema")
    if require(artifact, "tool_id", str) != TOOL_ID:
        raise ContractError("unexpected promotion tool ID")
    if require(artifact, "tool_version", int) != TOOL_VERSION:
        raise ContractError("unexpected promotion tool version")
    if require(artifact, "policy_id", str) != criteria["policy_id"]:
        raise ContractError("promotion artifact policy ID mismatch")
    if require(artifact, "policy_version", int) != criteria["policy_version"]:
        raise ContractError("promotion artifact policy version mismatch")
    for artifact_field, criteria_field in (
        ("het_config_hash", "expected_het_config_hash"),
        ("v1_config_hash", "expected_v1_config_hash"),
        ("time_stop_v2_config_hash", "expected_time_stop_v2_config_hash"),
    ):
        if require(artifact, artifact_field, str) != criteria[criteria_field]:
            raise ContractError(f"promotion artifact {artifact_field} mismatch")
    if require(artifact, "analysis_tool_hash", str) != sha256(Path(__file__).resolve()):
        raise ContractError("promotion artifact analysis tool hash mismatch")
    dependency_hashes = require(artifact, "analysis_dependency_hashes", dict)
    if require(dependency_hashes, "promotion_tool", str) != sha256(Path(__file__).resolve()):
        raise ContractError("promotion artifact promotion tool hash mismatch")
    if require(dependency_hashes, "pr_a_analyzer", str) != sha256(pr_a_analyzer_path()):
        raise ContractError("promotion artifact PR A analyzer hash mismatch")
    if not re.fullmatch(r"[0-9a-f]{64}", require(artifact, "input_manifest_hash", str)):
        raise ContractError("promotion artifact input manifest hash is invalid")
    if artifact.get("criteria", {}).get("criteria_hash") != hash_bytes(canonical_json(criteria)):
        raise ContractError("promotion artifact criteria hash mismatch")
    gate_eligibility = require(artifact, "gate_eligibility", dict)
    if set(gate_eligibility) != set(criteria["gate_promotion_contract"]):
        raise ContractError("promotion artifact gate eligibility set mismatch")
    for gate_key, contract in criteria["gate_promotion_contract"].items():
        row = require(gate_eligibility, gate_key, dict)
        if require(row, "promotion_requested", bool) != contract["promotion_requested"]:
            raise ContractError(f"promotion_requested mismatch: {gate_key}")
        if require(row, "authority_eligible", bool) != contract["authority_eligible"]:
            raise ContractError(f"authority_eligible mismatch: {gate_key}")
        gate_passed = row.get("promotion_gate_passed")
        if contract["promotion_requested"]:
            if not isinstance(gate_passed, bool):
                raise ContractError(f"promotion gate result missing: {gate_key}")
        elif gate_passed is not None:
            raise ContractError(f"non-promoted gate cannot pass promotion: {gate_key}")
        for field in ("candidate_positions", "matched_positions"):
            value = require(row, field, int)
            if value < 0:
                raise ContractError(f"negative gate eligibility count: {gate_key}.{field}")
    gates = require(artifact, "gates", dict)
    if set(gates) != set(GATE_NAMES):
        raise ContractError("promotion artifact gate set mismatch")
    conjunction = True
    for gate_name in GATE_NAMES:
        gate = require(gates, gate_name, dict)
        passed = require(gate, "passed", bool)
        observed = require(gate, "observed", dict)
        thresholds = require(gate, "thresholds", dict)
        checks = require(gate, "checks", dict)
        criteria_thresholds = criteria["gates"][gate_name]["thresholds"]
        if thresholds != criteria_thresholds:
            raise ContractError(f"promotion artifact thresholds mismatch: {gate_name}")
        expected = evaluate_gate(gate_name, observed, criteria_thresholds)
        if passed != expected["passed"] or checks != expected["checks"]:
            raise ContractError(f"promotion artifact gate result mismatch: {gate_name}")
        conjunction = conjunction and passed
    root = require(artifact, "promotion_gate_passed", bool)
    if root != conjunction:
        raise ContractError("manual root boolean disagrees with Gate 1-5 conjunction")


def add_manifest_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--launch-cohort-id", required=True)
    parser.add_argument("--run-role", choices=("calibration", "validation"), required=True)
    for artifact_class in REQUIRED_ARTIFACT_CLASSES:
        parser.add_argument(
            f"--{artifact_class.replace('_', '-')}",
            action="append",
            required=True,
            help="repo-relative path or glob; repeatable",
        )
    parser.add_argument("--output", type=Path, required=True)


def load_verified_run_manifests(
    manifest_paths: list[Path], repo_root: Path
) -> list[tuple[Path, dict[str, Any], dict[str, list[Path]]]]:
    if not manifest_paths:
        raise ContractError("at least one source run manifest is required")
    manifests: list[tuple[Path, dict[str, Any], dict[str, list[Path]]]] = []
    seen_runs: set[str] = set()
    seen_cohorts: set[str] = set()
    for manifest_path in manifest_paths:
        manifest = read_json(manifest_path)
        validate_run_manifest_shape(manifest, manifest_path)
        if manifest["run_id"] in seen_runs:
            raise ContractError(f"duplicate run manifest: {manifest['run_id']}")
        if manifest["launch_cohort_id"] in seen_cohorts:
            raise ContractError(
                f"launch cohort reused by multiple run manifests: {manifest['launch_cohort_id']}"
            )
        seen_runs.add(manifest["run_id"])
        seen_cohorts.add(manifest["launch_cohort_id"])
        manifests.append(
            (manifest_path, manifest, verify_manifest_artifacts(manifest, repo_root))
        )
    manifests.sort(key=lambda item: item[1]["run_id"])
    return manifests


def validate_promotion_artifact_against_sources(
    *,
    criteria: dict[str, Any],
    manifest_paths: list[Path],
    repo_root: Path,
    artifact_path: Path,
) -> None:
    manifests = load_verified_run_manifests(manifest_paths, repo_root)
    expected = evaluate(criteria, manifests)
    validate_promotion_artifact(expected, criteria)
    actual_bytes = artifact_path.read_bytes()
    expected_bytes = canonical_json(expected)
    if actual_bytes != expected_bytes:
        raise ContractError("promotion artifact bytes do not match source recomputation")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    manifest = subparsers.add_parser("manifest")
    add_manifest_arguments(manifest)
    evaluate_parser = subparsers.add_parser("evaluate")
    evaluate_parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    evaluate_parser.add_argument("--criteria", type=Path, required=True)
    evaluate_parser.add_argument("--run-manifest", action="append", type=Path, required=True)
    evaluate_parser.add_argument("--output", type=Path, required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    validate_parser.add_argument("--criteria", type=Path, required=True)
    validate_parser.add_argument("--run-manifest", action="append", type=Path, required=True)
    validate_parser.add_argument("--artifact", type=Path, required=True)
    structure_parser = subparsers.add_parser("validate-structure")
    structure_parser.add_argument("--criteria", type=Path, required=True)
    structure_parser.add_argument("--artifact", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        if args.command == "manifest":
            value = build_run_manifest(args)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_bytes(canonical_json(value))
            return 0
        criteria = read_json(args.criteria)
        validate_criteria(criteria)
        if args.command == "validate":
            validate_promotion_artifact_against_sources(
                criteria=criteria,
                manifest_paths=args.run_manifest,
                repo_root=args.repo_root.resolve(),
                artifact_path=args.artifact,
            )
            return 0
        if args.command == "validate-structure":
            artifact = read_json(args.artifact)
            validate_promotion_artifact(artifact, criteria)
            return 0
        repo_root = args.repo_root.resolve()
        manifests = load_verified_run_manifests(args.run_manifest, repo_root)
        artifact = evaluate(criteria, manifests)
        validate_promotion_artifact(artifact, criteria)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(canonical_json(artifact))
        return 0
    except (ContractError, OSError, ValueError) as error:
        print(f"{TOOL_ID}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
