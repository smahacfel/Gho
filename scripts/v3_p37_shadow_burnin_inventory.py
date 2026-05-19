#!/usr/bin/env python3
"""Inventory P3.7 shadow-burnin artifacts.

The scanner is intentionally read-only except for the requested report outputs.
It separates repository code availability from current VPS artifacts and
optional externally restored artifact roots.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback path.
    tomllib = None  # type: ignore[assignment]


ROOT_CONFIG_NAMESPACE = "root_config"
SKIP_DIR_NAMES = {
    ".git",
    ".mypy_cache",
    ".pytest_cache",
    ".venv",
    "node_modules",
    "target",
    "venv",
}
SHADOW_ONCHAIN_REPORT_GLOB = "shadow_onchain_lifecycle_report*.jsonl"
P37_LIFECYCLE_GLOB = "p3_7_*shadow*lifecycle*.jsonl"
COUNT_LIMIT_NOTE_THRESHOLD = 20


@dataclass
class ConfigInfo:
    path: str
    entry_mode: str | None = None
    execution_mode: str | None = None
    shadow_run_enabled: bool | None = None
    emit_event_bus: bool | None = None
    funding_lane_mode: str | None = None


@dataclass
class RunFiles:
    config_paths: set[Path] = field(default_factory=set)
    entry_logs: set[Path] = field(default_factory=set)
    lifecycle_logs: set[Path] = field(default_factory=set)
    transport_logs: set[Path] = field(default_factory=set)
    decision_logs: set[Path] = field(default_factory=set)
    buy_logs: set[Path] = field(default_factory=set)
    system_logs: set[Path] = field(default_factory=set)
    oracle_logs: set[Path] = field(default_factory=set)
    event_dirs: set[Path] = field(default_factory=set)
    truth_reports: set[Path] = field(default_factory=set)
    p37_lifecycle_files: set[Path] = field(default_factory=set)
    notes: list[str] = field(default_factory=list)
    config_infos: list[ConfigInfo] = field(default_factory=list)


@dataclass(frozen=True)
class ScanRoot:
    path: Path
    root_kind: str


def utc_now_iso() -> str:
    return dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def rel_display(path: Path, base: Path | None = None) -> str:
    try:
        if base is not None:
            return str(path.resolve().relative_to(base.resolve()))
    except ValueError:
        pass
    return str(path)


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


def count_jsonl_rows(path: Path) -> int:
    rows = 0
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            if line.strip():
                rows += 1
    return rows


def count_rows(paths: Iterable[Path]) -> int:
    return sum(count_jsonl_rows(path) for path in sorted(paths))


def lifecycle_counts(paths: Iterable[Path]) -> tuple[int, int, int]:
    rows = 0
    position_closed = 0
    exit_filled = 0
    for path in sorted(paths):
        for row in iter_jsonl(path):
            rows += 1
            record_type = str(row.get("record_type") or row.get("type") or row.get("event_type") or "")
            if record_type == "position_closed":
                position_closed += 1
            elif record_type == "exit_filled":
                exit_filled += 1
    return rows, position_closed, exit_filled


def count_literal_in_files(paths: Iterable[Path], needle: str) -> int:
    files = [path for path in sorted(paths) if path.exists()]
    if not files:
        return 0
    rg = shutil.which("rg")
    if rg is not None:
        cmd = [rg, "--text", "--fixed-strings", "--count-matches", needle]
        cmd.extend(str(path) for path in files)
        proc = subprocess.run(cmd, text=True, capture_output=True, check=False)
        if proc.returncode in {0, 1}:
            total = 0
            for line in proc.stdout.splitlines():
                raw = line.strip()
                if not raw:
                    continue
                try:
                    total += int(raw.rsplit(":", 1)[-1])
                except ValueError:
                    continue
            return total
    total = 0
    for path in files:
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                if needle in line:
                    total += line.count(needle)
    return total


def count_literal_with_limit(
    paths: Iterable[Path],
    needle: str,
    *,
    max_scan_bytes: int,
) -> tuple[int | None, str | None]:
    files = [path for path in sorted(paths) if path.exists()]
    total_bytes = sum(path.stat().st_size for path in files)
    if max_scan_bytes >= 0 and total_bytes > max_scan_bytes:
        return (
            None,
            f"diag_account_update_relay_count_skipped_system_log_bytes={total_bytes}_limit={max_scan_bytes}",
        )
    return count_literal_in_files(files, needle), None


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is not None:
        with path.open("rb") as fh:
            data = tomllib.load(fh)
        return data if isinstance(data, dict) else {}
    return load_toml_minimal(path)


def load_toml_minimal(path: Path) -> dict[str, Any]:
    data: dict[str, Any] = {}
    current: list[str] = []
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.split("#", 1)[0].strip()
            if not raw:
                continue
            if raw.startswith("[") and raw.endswith("]"):
                current = [part.strip() for part in raw.strip("[]").split(".") if part.strip()]
                node = data
                for part in current:
                    node = node.setdefault(part, {})
                continue
            if "=" not in raw:
                continue
            key, value = (part.strip() for part in raw.split("=", 1))
            parsed: Any = value.strip().strip('"')
            if value.lower() == "true":
                parsed = True
            elif value.lower() == "false":
                parsed = False
            node = data
            for part in current:
                node = node.setdefault(part, {})
            node[key] = parsed
    return data


def nested(data: dict[str, Any], *keys: str) -> Any:
    node: Any = data
    for key in keys:
        if not isinstance(node, dict):
            return None
        node = node.get(key)
    return node


def normalize_config_path(config_path: Path, value: Any) -> Path | None:
    if not isinstance(value, str) or not value:
        return None
    path = Path(value)
    if not path.is_absolute():
        path = config_path.parent / path
    return path.resolve()


def path_parts_after(path: Path, root: Path, marker: tuple[str, ...]) -> list[str] | None:
    try:
        parts = path.resolve().relative_to(root.resolve()).parts
    except ValueError:
        parts = path.resolve().parts
    marker_len = len(marker)
    for idx in range(0, len(parts) - marker_len + 1):
        if tuple(parts[idx : idx + marker_len]) == marker:
            return list(parts[idx + marker_len :])
    return None


def namespace_from_artifact_path(path: Path, root: Path) -> str | None:
    after = path_parts_after(path, root, ("logs", "shadow_run"))
    if after:
        first = after[0]
        if len(after) == 1:
            if first in {"buys.jsonl", "shadow_entries.jsonl", "shadow_lifecycle.jsonl"}:
                return ROOT_CONFIG_NAMESPACE
            if first.endswith("-buys.jsonl"):
                return first[: -len("-buys.jsonl")]
            return Path(first).stem
        return first

    for marker in (("logs", "rollout"), ("datasets", "events"), ("data", "rollout")):
        after = path_parts_after(path, root, marker)
        if after:
            return after[0] if after[0] else ROOT_CONFIG_NAMESPACE
    return None


def namespace_from_config(config_path: Path, config: dict[str, Any], root: Path) -> str:
    candidate_values = (
        nested(config, "execution", "shadow", "entry_log_path"),
        nested(config, "execution", "shadow", "lifecycle_log_path"),
        nested(config, "trigger", "shadow_run", "output_path"),
        nested(config, "execution", "events", "output_dir"),
        nested(config, "oracle", "decision_log_path"),
    )
    for value in candidate_values:
        normalized = normalize_config_path(config_path, value)
        if normalized is None:
            continue
        namespace = namespace_from_artifact_path(normalized, root)
        if namespace is not None:
            return namespace
    return ROOT_CONFIG_NAMESPACE if config_path.name == "config.toml" else config_path.stem


def config_info(config_path: Path, config: dict[str, Any], repo_root: Path) -> ConfigInfo:
    return ConfigInfo(
        path=rel_display(config_path, repo_root),
        entry_mode=string_or_none(nested(config, "trigger", "entry_mode")),
        execution_mode=string_or_none(nested(config, "execution", "execution_mode")),
        shadow_run_enabled=bool_or_none(nested(config, "trigger", "shadow_run", "enabled")),
        emit_event_bus=bool_or_none(nested(config, "trigger", "shadow_run", "emit_event_bus")),
        funding_lane_mode=string_or_none(
            nested(config, "seer", "funding_lane_mode")
            or nested(config, "funding", "funding_lane_mode")
            or config.get("funding_lane_mode")
        ),
    )


def string_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def iter_known_config_paths(root: Path) -> Iterable[Path]:
    config = root / "config.toml"
    if config.exists():
        yield config
    rollout_dir = root / "configs" / "rollout"
    if rollout_dir.exists():
        yield from sorted(rollout_dir.glob("shadow-burnin*.toml"))


def safe_rglob(root: Path, pattern: str) -> Iterable[Path]:
    if not root.exists():
        return
    for path in root.rglob(pattern):
        if any(part in SKIP_DIR_NAMES for part in path.parts):
            continue
        yield path


def record_for(records: dict[tuple[str, str, str], RunFiles], scan_root: ScanRoot, namespace: str) -> RunFiles:
    key = (scan_root.root_kind, str(scan_root.path.resolve()), namespace)
    return records.setdefault(key, RunFiles())


def add_path(
    records: dict[tuple[str, str, str], RunFiles],
    scan_root: ScanRoot,
    path: Path,
    attr: str,
    *,
    namespace: str | None = None,
) -> None:
    detected = namespace or namespace_from_artifact_path(path, scan_root.path)
    if detected is None:
        return
    bucket = getattr(record_for(records, scan_root, detected), attr)
    bucket.add(path.resolve())


def discover_configs(records: dict[tuple[str, str, str], RunFiles], scan_root: ScanRoot, repo_root: Path) -> None:
    for path in iter_known_config_paths(scan_root.path):
        try:
            config = load_toml(path)
        except Exception as exc:  # noqa: BLE001 - inventory should report and continue.
            namespace = ROOT_CONFIG_NAMESPACE if path.name == "config.toml" else path.stem
            run = record_for(records, scan_root, namespace)
            run.config_paths.add(path.resolve())
            run.notes.append(f"config_parse_error:{rel_display(path, repo_root)}:{type(exc).__name__}")
            continue
        namespace = namespace_from_config(path, config, scan_root.path)
        run = record_for(records, scan_root, namespace)
        run.config_paths.add(path.resolve())
        run.config_infos.append(config_info(path, config, repo_root))


def discover_layout_artifacts(records: dict[tuple[str, str, str], RunFiles], scan_root: ScanRoot) -> None:
    shadow_root = scan_root.path / "logs" / "shadow_run"
    if shadow_root.exists():
        for path in safe_rglob(shadow_root, "shadow_entries.jsonl"):
            add_path(records, scan_root, path, "entry_logs")
        for path in safe_rglob(shadow_root, "shadow_lifecycle.jsonl"):
            add_path(records, scan_root, path, "lifecycle_logs")
        for path in safe_rglob(shadow_root, "buys.jsonl"):
            add_path(records, scan_root, path, "transport_logs")
        for path in safe_rglob(shadow_root, "shadow-burnin-*-buys.jsonl"):
            add_path(records, scan_root, path, "transport_logs")
        for path in safe_rglob(shadow_root, SHADOW_ONCHAIN_REPORT_GLOB):
            add_path(records, scan_root, path, "truth_reports")
        for path in safe_rglob(shadow_root, P37_LIFECYCLE_GLOB):
            add_path(records, scan_root, path, "p37_lifecycle_files")

    rollout_root = scan_root.path / "logs" / "rollout"
    if rollout_root.exists():
        for namespace_dir in sorted(path for path in rollout_root.iterdir() if path.is_dir()):
            namespace = namespace_dir.name
            for path in safe_rglob(namespace_dir, "gatekeeper_v2_decisions.jsonl"):
                add_path(records, scan_root, path, "decision_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, "gatekeeper_v2_buys.jsonl"):
                add_path(records, scan_root, path, "buy_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, "system.log*"):
                add_path(records, scan_root, path, "system_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, "oracle.log*"):
                add_path(records, scan_root, path, "oracle_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, P37_LIFECYCLE_GLOB):
                add_path(records, scan_root, path, "p37_lifecycle_files", namespace=namespace)

    events_root = scan_root.path / "datasets" / "events"
    if events_root.exists():
        namespace_dirs = [path for path in sorted(events_root.iterdir()) if path.is_dir()]
        if not namespace_dirs and any(events_root.glob("*.jsonl")):
            record_for(records, scan_root, ROOT_CONFIG_NAMESPACE).event_dirs.add(events_root.resolve())
        for namespace_dir in namespace_dirs:
            record_for(records, scan_root, namespace_dir.name).event_dirs.add(namespace_dir.resolve())

    data_rollout = scan_root.path / "data" / "rollout"
    if data_rollout.exists():
        for namespace_dir in sorted(path for path in data_rollout.iterdir() if path.is_dir()):
            namespace = namespace_dir.name
            for path in safe_rglob(namespace_dir, "system.log*"):
                add_path(records, scan_root, path, "system_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, "oracle.log*"):
                add_path(records, scan_root, path, "oracle_logs", namespace=namespace)
            for path in safe_rglob(namespace_dir, SHADOW_ONCHAIN_REPORT_GLOB):
                add_path(records, scan_root, path, "truth_reports", namespace=namespace)

    plans_audyt = scan_root.path / "PLANS" / "AUDYT"
    if plans_audyt.exists():
        for path in safe_rglob(plans_audyt, P37_LIFECYCLE_GLOB):
            namespace = namespace_from_artifact_path(path, scan_root.path) or "plans_audyt"
            add_path(records, scan_root, path, "p37_lifecycle_files", namespace=namespace)


def discover_generic_extra_artifacts(records: dict[tuple[str, str, str], RunFiles], scan_root: ScanRoot) -> None:
    if scan_root.root_kind != "external_restored":
        return
    patterns = {
        "shadow_entries.jsonl": "entry_logs",
        "shadow_lifecycle.jsonl": "lifecycle_logs",
        "buys.jsonl": "transport_logs",
        "shadow-burnin-*-buys.jsonl": "transport_logs",
        SHADOW_ONCHAIN_REPORT_GLOB: "truth_reports",
        "gatekeeper_v2_decisions.jsonl": "decision_logs",
        "gatekeeper_v2_buys.jsonl": "buy_logs",
        "system.log*": "system_logs",
        "oracle.log*": "oracle_logs",
        P37_LIFECYCLE_GLOB: "p37_lifecycle_files",
    }
    for pattern, attr in patterns.items():
        for path in safe_rglob(scan_root.path, pattern):
            namespace = namespace_from_artifact_path(path, scan_root.path)
            if namespace is None:
                namespace = namespace_from_loose_artifact(path, scan_root.path)
            add_path(records, scan_root, path, attr, namespace=namespace)


def namespace_from_loose_artifact(path: Path, root: Path) -> str:
    try:
        rel = path.resolve().relative_to(root.resolve())
    except ValueError:
        rel = path
    if len(rel.parts) >= 2:
        parent = rel.parts[-2]
        if parent not in {".", ""}:
            return parent
    if path.name.endswith("-buys.jsonl"):
        return path.name[: -len("-buys.jsonl")]
    return ROOT_CONFIG_NAMESPACE


def choose_config_info(run: RunFiles) -> ConfigInfo | None:
    if not run.config_infos:
        return None
    return sorted(run.config_infos, key=lambda info: info.path)[0]


def event_file_count(event_dirs: Iterable[Path]) -> int:
    total = 0
    for path in sorted(event_dirs):
        if path.exists():
            total += sum(1 for _ in safe_rglob(path, "*.jsonl"))
    return total


def detect_session_scope(event_dirs: Iterable[Path]) -> str:
    for path in sorted(event_dirs):
        jsonl_files = sorted(path.glob("*.jsonl"))
        if jsonl_files:
            return f"events:{path.name}:{jsonl_files[0].stem}"
        subdirs = sorted(child.name for child in path.iterdir() if child.is_dir()) if path.exists() else []
        if subdirs:
            return f"events:{path.name}:subdirs={len(subdirs)}"
    return "none"


def classify_artifacts(row: dict[str, Any]) -> str:
    has_transport = bool(row["transport_log_exists"])
    has_entry = bool(row["entry_log_exists"])
    has_lifecycle = bool(row["lifecycle_log_exists"])
    has_decision = bool(row["decision_log_exists"] or row["buy_log_exists"])
    has_logs = bool(row["system_log_exists"] or row["oracle_log_exists"])
    has_events = bool(row["events_dir_exists"])
    has_truth = bool(row["truth_report_exists"])

    if has_truth and has_transport and has_entry and has_lifecycle and has_decision and has_logs and has_events:
        return "artifact_complete_for_shadow_onchain_labeling"
    if has_transport and has_entry and has_lifecycle and has_decision and has_logs and has_events:
        return "artifact_complete_for_shadow_runtime_only"
    if has_transport and has_entry and has_lifecycle:
        return "artifact_partial_transport_entry_lifecycle"
    if has_transport and has_entry:
        return "artifact_partial_transport_entry_only"
    if has_transport:
        return "artifact_partial_transport_only"
    if has_decision or has_events or has_logs:
        return "artifact_primary_market_path_only"
    if row.get("config_path"):
        return "artifact_missing"
    return "artifact_unknown"


def code_availability(repo_root: Path) -> dict[str, Any]:
    required = {
        "root_config": repo_root / "config.toml",
        "rollout_shadow_burnin_config": repo_root / "configs" / "rollout" / "shadow-burnin.toml",
        "shadow_onchain_lifecycle_report": repo_root / "scripts" / "shadow_onchain_lifecycle_report.py",
        "shadow_run_report": repo_root / "scripts" / "shadow_run_report.py",
        "trigger_shadow_run_module": repo_root / "ghost-launcher" / "src" / "components" / "trigger" / "shadow_run.rs",
        "trigger_component": repo_root / "ghost-launcher" / "src" / "components" / "trigger" / "component.rs",
        "oracle_runtime": repo_root / "ghost-launcher" / "src" / "oracle_runtime.rs",
        "post_buy_runtime": repo_root / "ghost-launcher" / "src" / "components" / "post_buy_runtime.rs",
    }
    present = {name: path.exists() for name, path in required.items()}
    missing = [name for name, exists in present.items() if not exists]
    if not missing:
        klass = "shadow_burnin_code_present"
    elif present["root_config"] and present["shadow_onchain_lifecycle_report"]:
        klass = "shadow_burnin_code_partial"
    else:
        klass = "shadow_burnin_code_missing"
    return {
        "class": klass,
        "required_files_present": present,
        "missing_required_files": missing,
    }


def git_head(repo_root: Path) -> str | None:
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        return None
    return proc.stdout.strip() or None


def build_run_row(
    key: tuple[str, str, str],
    run: RunFiles,
    repo_root: Path,
    *,
    max_system_log_scan_bytes: int,
) -> dict[str, Any]:
    root_kind, root_path, namespace = key
    info = choose_config_info(run)
    lifecycle_rows, position_closed, exit_filled = lifecycle_counts(run.lifecycle_logs)
    diag_count, diag_note = count_literal_with_limit(
        run.system_logs,
        "DIAG_ACCOUNT_UPDATE_RELAY",
        max_scan_bytes=max_system_log_scan_bytes,
    )
    row: dict[str, Any] = {
        "namespace": namespace,
        "artifact_root": root_path,
        "artifact_root_kind": root_kind,
        "config_path": info.path if info is not None else None,
        "entry_mode": info.entry_mode if info is not None else None,
        "execution_mode": info.execution_mode if info is not None else None,
        "shadow_run_enabled": info.shadow_run_enabled if info is not None else None,
        "emit_event_bus": info.emit_event_bus if info is not None else None,
        "funding_lane_mode": info.funding_lane_mode if info is not None else None,
        "entry_log_exists": bool(run.entry_logs),
        "entry_rows": count_rows(run.entry_logs),
        "lifecycle_log_exists": bool(run.lifecycle_logs),
        "lifecycle_rows": lifecycle_rows,
        "position_closed_count": position_closed,
        "exit_filled_count": exit_filled,
        "transport_log_exists": bool(run.transport_logs),
        "transport_rows": count_rows(run.transport_logs),
        "decision_log_exists": bool(run.decision_logs),
        "decision_rows": count_rows(run.decision_logs),
        "buy_log_exists": bool(run.buy_logs),
        "buy_rows": count_rows(run.buy_logs),
        "system_log_exists": bool(run.system_logs),
        "oracle_log_exists": bool(run.oracle_logs),
        "diag_account_update_relay_count": diag_count,
        "events_dir_exists": bool(run.event_dirs),
        "event_file_count": event_file_count(run.event_dirs),
        "session_scope_detected": detect_session_scope(run.event_dirs),
        "truth_report_exists": bool(run.truth_reports),
        "truth_report_rows": count_rows(run.truth_reports),
    }
    notes = list(run.notes)
    if diag_note is not None:
        notes.append(diag_note)
    if len(run.config_paths) > 1:
        notes.append(f"config_path_count={len(run.config_paths)}")
    if len(run.decision_logs) > 1:
        notes.append(f"decision_log_files={len(run.decision_logs)}")
    if len(run.buy_logs) > 1:
        notes.append(f"buy_log_files={len(run.buy_logs)}")
    if len(run.transport_logs) > 1:
        notes.append(f"transport_log_files={len(run.transport_logs)}")
    if len(run.truth_reports) == 0:
        notes.append("no_shadow_onchain_lifecycle_report_found")
    if run.p37_lifecycle_files:
        notes.append(f"p37_lifecycle_files={len(run.p37_lifecycle_files)}")
    row["artifact_availability_class"] = classify_artifacts(row)
    row["notes"] = sorted(set(notes))
    row["artifact_files"] = artifact_file_summary(run, repo_root)
    return row


def artifact_file_summary(run: RunFiles, repo_root: Path) -> dict[str, list[str]]:
    def summarize(paths: Iterable[Path]) -> list[str]:
        values = [rel_display(path, repo_root) for path in sorted(paths)]
        if len(values) <= COUNT_LIMIT_NOTE_THRESHOLD:
            return values
        head = values[:COUNT_LIMIT_NOTE_THRESHOLD]
        head.append(f"... {len(values) - COUNT_LIMIT_NOTE_THRESHOLD} more")
        return head

    return {
        "config_paths": summarize(run.config_paths),
        "entry_logs": summarize(run.entry_logs),
        "lifecycle_logs": summarize(run.lifecycle_logs),
        "transport_logs": summarize(run.transport_logs),
        "decision_logs": summarize(run.decision_logs),
        "buy_logs": summarize(run.buy_logs),
        "system_logs": summarize(run.system_logs),
        "oracle_logs": summarize(run.oracle_logs),
        "event_dirs": summarize(run.event_dirs),
        "truth_reports": summarize(run.truth_reports),
        "p37_lifecycle_files": summarize(run.p37_lifecycle_files),
    }


def summary_for(rows: list[dict[str, Any]]) -> dict[str, Any]:
    class_counts = Counter(row["artifact_availability_class"] for row in rows)
    root_kind_counts = Counter(row["artifact_root_kind"] for row in rows)
    current_vps_rows = [row for row in rows if row["artifact_root_kind"] == "current_vps_repo"]
    external_rows = [row for row in rows if row["artifact_root_kind"] == "external_restored"]
    return {
        "run_count": len(rows),
        "artifact_availability_class_counts": dict(sorted(class_counts.items())),
        "artifact_root_kind_counts": dict(sorted(root_kind_counts.items())),
        "current_vps": {
            "run_count": len(current_vps_rows),
            "truth_report_run_count": sum(1 for row in current_vps_rows if row["truth_report_exists"]),
            "entry_log_run_count": sum(1 for row in current_vps_rows if row["entry_log_exists"]),
            "lifecycle_log_run_count": sum(1 for row in current_vps_rows if row["lifecycle_log_exists"]),
            "transport_log_run_count": sum(1 for row in current_vps_rows if row["transport_log_exists"]),
        },
        "external_restored": {
            "run_count": len(external_rows),
            "truth_report_run_count": sum(1 for row in external_rows if row["truth_report_exists"]),
        },
    }


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def format_bool(value: Any) -> str:
    if value is True:
        return "true"
    if value is False:
        return "false"
    if value is None:
        return "unknown"
    return str(value)


def write_markdown(path: Path, payload: dict[str, Any], repo_root: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    rows = payload["runs"]
    summary = payload["summary"]
    lines: list[str] = []
    lines.append("# P3.7 Shadow-Burnin Inventory")
    lines.append("")
    lines.append(f"Generated: `{payload['generated_at']}`")
    lines.append(f"Repo root: `{payload['repo_root']}`")
    lines.append(f"Git HEAD: `{payload.get('repo_head') or 'unknown'}`")
    lines.append(f"Max system log scan bytes: `{payload['limits']['max_system_log_scan_bytes']}`")
    lines.append("")
    lines.append("## Scope")
    lines.append("")
    lines.append(
        "This inventory is read-only with respect to Ghost runtime artifacts. "
        "It writes only the requested JSON and Markdown reports."
    )
    lines.append("")
    lines.append("It separates repo code availability, current VPS artifact availability, and external/restored artifact roots.")
    lines.append("")
    lines.append("## Code Availability")
    lines.append("")
    code = payload["code_availability"]
    lines.append(f"- class: `{code['class']}`")
    lines.append(f"- missing_required_files: `{', '.join(code['missing_required_files']) if code['missing_required_files'] else 'none'}`")
    lines.append("")
    lines.append("## Scanned Roots")
    lines.append("")
    for root in payload["scanned_roots"]:
        lines.append(f"- `{root['path']}` ({root['root_kind']})")
    lines.append("")
    lines.append("## Summary")
    lines.append("")
    lines.append(f"- detected_runs: `{summary['run_count']}`")
    lines.append(f"- artifact_availability_class_counts: `{json.dumps(summary['artifact_availability_class_counts'], sort_keys=True)}`")
    lines.append(f"- artifact_root_kind_counts: `{json.dumps(summary['artifact_root_kind_counts'], sort_keys=True)}`")
    lines.append(f"- current_vps_truth_report_run_count: `{summary['current_vps']['truth_report_run_count']}`")
    if summary["current_vps"]["truth_report_run_count"] == 0:
        lines.append("- current_vps_shadow_onchain_lifecycle_reports: `none_found`")
    if summary["external_restored"]["run_count"] == 0:
        lines.append("- external_restored_roots: `none_provided`")
    lines.append("")
    lines.append("## Run Inventory")
    lines.append("")
    for row in rows:
        lines.append(f"### {row['namespace']}")
        lines.append("")
        field_names = [
            "artifact_root",
            "artifact_root_kind",
            "config_path",
            "entry_mode",
            "execution_mode",
            "shadow_run_enabled",
            "emit_event_bus",
            "funding_lane_mode",
            "entry_log_exists",
            "entry_rows",
            "lifecycle_log_exists",
            "lifecycle_rows",
            "position_closed_count",
            "exit_filled_count",
            "transport_log_exists",
            "transport_rows",
            "decision_log_exists",
            "decision_rows",
            "buy_log_exists",
            "buy_rows",
            "system_log_exists",
            "oracle_log_exists",
            "diag_account_update_relay_count",
            "events_dir_exists",
            "event_file_count",
            "session_scope_detected",
            "truth_report_exists",
            "truth_report_rows",
            "artifact_availability_class",
        ]
        for name in field_names:
            value = row.get(name)
            if isinstance(value, bool) or value is None:
                rendered = format_bool(value)
            else:
                rendered = str(value)
            lines.append(f"- `{name}`: `{rendered}`")
        notes = row.get("notes") or []
        lines.append(f"- `notes`: `{'; '.join(notes) if notes else 'none'}`")
        lines.append("")
    lines.append("## Acceptance Notes")
    lines.append("")
    lines.append("- Current VPS artifacts are reported separately from external/restored roots.")
    lines.append("- Missing historical artifacts remain explicit as missing or partial artifact classes.")
    lines.append("- Shadow simulation, shadow-onchain report availability, and live inclusion are not conflated.")
    lines.append("- No active policy, live sender, IWIM, or FSC behavior is changed by this inventory.")
    lines.append("")
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def build_payload(repo_root: Path, extra_roots: list[Path], *, max_system_log_scan_bytes: int) -> dict[str, Any]:
    scan_roots = [ScanRoot(repo_root.resolve(), "current_vps_repo")]
    scan_roots.extend(ScanRoot(path.resolve(), "external_restored") for path in extra_roots)
    records: dict[tuple[str, str, str], RunFiles] = {}
    for scan_root in scan_roots:
        if not scan_root.path.exists():
            key = (scan_root.root_kind, str(scan_root.path.resolve()), "missing_artifact_root")
            run = records.setdefault(key, RunFiles())
            run.notes.append("artifact_root_does_not_exist")
            continue
        discover_configs(records, scan_root, repo_root)
        discover_layout_artifacts(records, scan_root)
        discover_generic_extra_artifacts(records, scan_root)
    rows = [
        build_run_row(key, run, repo_root, max_system_log_scan_bytes=max_system_log_scan_bytes)
        for key, run in sorted(records.items(), key=lambda item: item[0])
    ]
    payload = {
        "schema_version": 1,
        "generated_at": utc_now_iso(),
        "repo_root": str(repo_root.resolve()),
        "repo_head": git_head(repo_root),
        "code_availability": code_availability(repo_root),
        "scanned_roots": [{"path": str(root.path), "root_kind": root.root_kind} for root in scan_roots],
        "limits": {"max_system_log_scan_bytes": max_system_log_scan_bytes},
        "summary": summary_for(rows),
        "runs": rows,
    }
    return payload


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", default=".", help="Ghost repository root. Defaults to current directory.")
    parser.add_argument(
        "--extra-artifact-root",
        action="append",
        default=[],
        help="Optional restored artifact root to include without copying into the repo. Repeatable.",
    )
    parser.add_argument("--output-json", required=True, help="JSON inventory output path.")
    parser.add_argument("--output-md", required=True, help="Markdown inventory output path.")
    parser.add_argument(
        "--max-system-log-scan-bytes",
        type=int,
        default=536_870_912,
        help=(
            "Maximum system.log* bytes scanned per namespace for DIAG_ACCOUNT_UPDATE_RELAY. "
            "Use -1 for an exact unbounded scan."
        ),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    extra_roots = [Path(path).resolve() for path in args.extra_artifact_root]
    payload = build_payload(
        repo_root,
        extra_roots,
        max_system_log_scan_bytes=args.max_system_log_scan_bytes,
    )
    write_json(Path(args.output_json), payload)
    write_markdown(Path(args.output_md), payload, repo_root)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
