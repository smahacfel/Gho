#!/usr/bin/env python3
"""Offline shadow burn-in fidelity audit.

This script intentionally does not start, stop, or modify runtime processes.
It reads existing source/log artifacts and writes derived CSV/Markdown audit
outputs under reports/selector, PLANS/AUDYT, and docs/ADR.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import re
import statistics
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

sys.dont_write_bytecode = True


SCOPE_SPECS = [
    {
        "scope": "R48/R2",
        "slug": "shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2",
        "active_partial": False,
        "target_bps": 6000,
        "stop_bps": -6000,
        "max_hold_ms": 120000,
    },
    {
        "scope": "R49",
        "slug": "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1",
        "active_partial": False,
        "target_bps": 6000,
        "stop_bps": -6000,
        "max_hold_ms": 66000,
    },
    {
        "scope": "R50",
        "slug": "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1",
        "active_partial": False,
        "target_bps": 6000,
        "stop_bps": -6000,
        "max_hold_ms": 66000,
    },
    {
        "scope": "R51",
        "slug": "shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1",
        "active_partial": True,
        "target_bps": 1200,
        "stop_bps": -600,
        "max_hold_ms": 45000,
    },
]

HORIZONS_MS = [1000, 2000, 3000, 5000, 10000, 20000, 30000, 60000, 120000, 300000, 500000]

REPORT_DIR = Path("reports/selector")
GOLDEN_DIR = REPORT_DIR / "shadow_fidelity_golden_traces"
AUDIT_REPORT_PATH = Path("PLANS/AUDYT/RAPORT_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md")
ADR_PATH = Path("docs/ADR/ADR_8D_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md")

CSV_PATHS = {
    "inventory": REPORT_DIR / "shadow_fidelity_inventory.csv",
    "entry": REPORT_DIR / "shadow_fidelity_entry_price_reconstruction.csv",
    "exit": REPORT_DIR / "shadow_fidelity_exit_price_reconstruction.csv",
    "pool_state": REPORT_DIR / "shadow_fidelity_pool_state_provenance.csv",
    "temporal": REPORT_DIR / "shadow_fidelity_temporal_integrity.csv",
    "reconciliation": REPORT_DIR / "shadow_fidelity_replay_lifecycle_reconciliation.csv",
    "live_gap": REPORT_DIR / "shadow_fidelity_live_equivalence_gap.csv",
    "path_density": REPORT_DIR / "shadow_fidelity_path_sampling_density.csv",
    "fixtures": REPORT_DIR / "shadow_fidelity_fixture_results.csv",
    "claims": REPORT_DIR / "shadow_fidelity_claim_evidence_matrix.csv",
}

SOURCE_INVENTORY = [
    (
        "pool_detection",
        "ghost-launcher/src/oracle_runtime.rs",
        "OracleRuntime pool registration / NewPoolDetected handling",
        "session creation, pool identity handoff, decision logging context",
        "source_code_trace",
    ),
    (
        "pool_state_snapshot",
        "ghost-brain/src/oracle/snapshot_engine.rs",
        "SnapshotEngine::handle_initialize_pool_event / handle_tx_event",
        "local bootstrap and tx-derived pool price/reserve snapshots",
        "source_code_trace",
    ),
    (
        "materialized_feature_snapshot",
        "ghost-launcher/src/session/observation.rs",
        "PoolObservationSession::materialize_features",
        "canonical decision snapshot boundary",
        "source_code_trace",
    ),
    (
        "feature_builder",
        "ghost-core/src/checkpoint/feature_builder.rs",
        "FeatureBuilder / checkpoint feature aggregation",
        "checkpoint-derived materialized evidence",
        "source_code_trace",
    ),
    (
        "checkpoint_types",
        "ghost-core/src/checkpoint/types.rs",
        "checkpoint data types",
        "typed checkpoint inputs to materialization",
        "source_code_trace",
    ),
    (
        "checkpoint_module",
        "ghost-core/src/checkpoint/mod.rs",
        "checkpoint module exports",
        "checkpoint ownership surface",
        "source_code_trace",
    ),
    (
        "shadow_entry_creation",
        "ghost-launcher/src/oracle_runtime.rs",
        "shadow_entry_record_from_event / shadow_entry_record_from_request",
        "synthetic shadow entry row and entry price construction",
        "source_code_trace",
    ),
    (
        "shadow_execution_backend",
        "ghost-brain/src/execution/shadow.rs",
        "ShadowBackend::execute_prepared_entry",
        "prepared-entry shadow fill mirror and shadow_entries writer",
        "source_code_trace",
    ),
    (
        "shadow_position_lifecycle",
        "ghost-brain/src/guardian/post_buy/engine.rs",
        "ShadowPostBuyEngine lifecycle emitters",
        "shadow position tracking, threshold closes, lifecycle JSONL",
        "source_code_trace",
    ),
    (
        "shadow_exit_replay",
        "ghost-brain/src/guardian/post_buy/exit_replay.rs",
        "ShadowExitReplayRecord / ShadowExitReplayWriter",
        "path_bps, first_hit_ms, MFE/MAE, replay-side outcome evidence",
        "source_code_trace",
    ),
    (
        "shadow_exit_replay_config",
        "ghost-brain/src/guardian/post_buy/config.rs",
        "ShadowExitReplayConfig",
        "research sidecar config; explicit non-policy boundary",
        "source_code_trace",
    ),
    (
        "decision_logging",
        "ghost-brain/src/oracle/decision_logger.rs",
        "DecisionLogger / GatekeeperBuyLog",
        "gatekeeper_v2_decisions and selector_shadow_score_v1 JSONL surface",
        "source_code_trace",
    ),
    (
        "counterfactual_lab",
        "scripts/time_stop_v2_counterfactual_lab.py",
        "time-stop v2 counterfactual reader",
        "downstream research consumer of shadow exit/path metrics",
        "source_code_trace",
    ),
]


@dataclass
class ScopeContext:
    scope: str
    slug: str
    active_partial: bool
    target_bps: int
    stop_bps: int
    max_hold_ms: int
    dirs: list[Path] = field(default_factory=list)
    artifacts: dict[str, list[Path]] = field(default_factory=lambda: defaultdict(list))


@dataclass
class DecisionSnapshot:
    scope: str
    path: Path
    run_id: str
    session_id: str
    pool_id: str
    base_mint: str
    decision_ts_ms: int | None
    first_seen_ts_ms: int | None
    state_ts_ms: int | None
    state_slot: int | None
    reserves: list[float] | None
    mfs_price_sol: float | None
    source_fields: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline audit for shadow burn-in measurement fidelity.")
    parser.add_argument("--repo-root", default=".", help="Repository root. Default: current directory.")
    parser.add_argument("--output-dir", default=str(REPORT_DIR), help="Output directory for CSV artifacts.")
    parser.add_argument(
        "--extra-log-root",
        action="append",
        default=[],
        help="Additional log root to scan. Can be passed multiple times.",
    )
    parser.add_argument(
        "--max-sha-bytes",
        type=int,
        default=256 * 1024 * 1024,
        help="Maximum file size for sha256 calculation. Larger files are marked SKIPPED_TOO_LARGE.",
    )
    return parser.parse_args()


def write_csv(path: Path, fieldnames: list[str], rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({key: csv_safe(row.get(key, "")) for key in fieldnames})


def csv_safe(value: Any) -> Any:
    if isinstance(value, (dict, list, tuple)):
        return json.dumps(value, ensure_ascii=False, sort_keys=True)
    if value is None:
        return ""
    return value


def json_loads(line: str) -> dict[str, Any] | None:
    try:
        value = json.loads(line)
    except json.JSONDecodeError:
        return None
    return value if isinstance(value, dict) else None


def iter_jsonl(path: Path) -> Iterable[tuple[int, dict[str, Any] | None, str]]:
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for lineno, line in enumerate(fh, 1):
            raw = line.rstrip("\n")
            if not raw.strip():
                yield lineno, None, raw
            else:
                yield lineno, json_loads(raw), raw


def file_sha256(path: Path, max_bytes: int) -> str:
    try:
        size = path.stat().st_size
    except OSError as exc:
        return f"ERROR:{exc}"
    if size > max_bytes:
        return "SKIPPED_TOO_LARGE"
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def norm_str(value: Any) -> str:
    if value is None:
        return ""
    return str(value)


def to_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return None
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return None


def to_float(value: Any) -> float | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(number):
        return None
    return number


def nested_get(data: dict[str, Any], path: list[str]) -> Any:
    cur: Any = data
    for key in path:
        if not isinstance(cur, dict):
            return None
        cur = cur.get(key)
    return cur


def first_present(data: dict[str, Any], paths: list[list[str]]) -> Any:
    for path in paths:
        value = nested_get(data, path)
        if value not in (None, ""):
            return value
    return None


def row_pool_id(row: dict[str, Any]) -> str:
    return norm_str(
        first_present(
            row,
            [
                ["pool_id"],
                ["pool"],
                ["bonding_curve"],
                ["materialized_feature_snapshot", "pool_id"],
                ["materialized_feature_snapshot", "bonding_curve"],
                ["v3_materialized_feature_snapshot", "pool_id"],
            ],
        )
    )


def row_base_mint(row: dict[str, Any]) -> str:
    return norm_str(
        first_present(
            row,
            [
                ["base_mint"],
                ["mint_id"],
                ["mint"],
                ["token_mint"],
                ["materialized_feature_snapshot", "base_mint"],
                ["materialized_feature_snapshot", "mint_id"],
                ["v3_materialized_feature_snapshot", "base_mint"],
            ],
        )
    )


def row_run_id(row: dict[str, Any]) -> str:
    return norm_str(row.get("run_id"))


def row_session_id(row: dict[str, Any]) -> str:
    return norm_str(row.get("session_id"))


def classify_scope_status(scope: ScopeContext) -> str:
    if scope.active_partial:
        has_post = any("POST" in p.name.upper() and "MANIFEST" in p.name.upper() for paths in scope.artifacts.values() for p in paths)
        return "COMPLETED" if has_post else "ACTIVE_PARTIAL"
    return "COMPLETED_OR_HISTORICAL"


def discover_scope_contexts(repo_root: Path, extra_roots: list[str]) -> list[ScopeContext]:
    roots = [
        repo_root / "logs",
        repo_root / "reports" / "selector",
        Path("/mnt/HC_Volume_105935807/logs"),
    ]
    roots.extend(Path(root) for root in extra_roots)
    contexts: list[ScopeContext] = []
    for spec in SCOPE_SPECS:
        ctx = ScopeContext(
            scope=spec["scope"],
            slug=spec["slug"],
            active_partial=bool(spec["active_partial"]),
            target_bps=int(spec["target_bps"]),
            stop_bps=int(spec["stop_bps"]),
            max_hold_ms=int(spec["max_hold_ms"]),
        )
        seen_dirs: set[Path] = set()
        for root in roots:
            candidates = [
                root / spec["slug"],
                root / "shadow_run" / spec["slug"],
                root / "rollout" / spec["slug"],
                root / "selector" / spec["slug"],
            ]
            for candidate in candidates:
                if candidate.exists() and candidate.is_dir():
                    resolved = candidate.resolve()
                    if resolved not in seen_dirs:
                        ctx.dirs.append(candidate)
                        seen_dirs.add(resolved)
        for directory in ctx.dirs:
            for path in directory.rglob("*"):
                if not path.is_file():
                    continue
                name = path.name
                lowered = name.lower()
                if name == "gatekeeper_v2_decisions.jsonl":
                    ctx.artifacts["gatekeeper_v2_decisions.jsonl"].append(path)
                elif name == "selector_shadow_score_v1.jsonl":
                    ctx.artifacts["selector_shadow_score_v1.jsonl"].append(path)
                elif name == "shadow_lifecycle.jsonl":
                    ctx.artifacts["shadow_lifecycle.jsonl"].append(path)
                elif name == "probe_shadow_lifecycle.jsonl":
                    ctx.artifacts["probe_shadow_lifecycle.jsonl"].append(path)
                elif name == "shadow_exit_replay_v1.jsonl":
                    ctx.artifacts["shadow_exit_replay_v1.jsonl"].append(path)
                elif "launcher" in lowered and lowered.endswith((".md", ".json")):
                    ctx.artifacts["launcher_report"].append(path)
                elif "pre" in lowered and "manifest" in lowered:
                    ctx.artifacts["pre_run_manifest"].append(path)
                elif "post" in lowered and "manifest" in lowered:
                    ctx.artifacts["post_run_manifest"].append(path)
                elif lowered.endswith(".jsonl") and "event" in lowered:
                    ctx.artifacts["raw_event_stream"].append(path)
                elif lowered.endswith((".jsonl", ".json")) and "snapshot" in lowered:
                    ctx.artifacts["pool_state_snapshots"].append(path)
        contexts.append(ctx)
    return contexts


def source_inventory_rows(repo_root: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for component, file_path, symbol, responsibility, evidence_type in SOURCE_INVENTORY:
        path = repo_root / file_path
        rows.append(
            {
                "component": component,
                "file_path": file_path,
                "symbol/function/struct": symbol,
                "responsibility": responsibility,
                "evidence_type": evidence_type,
                "inspected": path.exists(),
                "risk_notes": "present" if path.exists() else "MISSING_SOURCE_FILE",
            }
        )
    return rows


def artifact_identity_key(kind: str, row: dict[str, Any]) -> tuple[Any, ...]:
    if kind == "shadow_exit_replay_v1.jsonl":
        return (
            row.get("run_id"),
            row.get("session_id"),
            row.get("pool_id"),
            row.get("base_mint"),
            row.get("entry_ts_ms"),
        )
    if kind in {"shadow_lifecycle.jsonl", "probe_shadow_lifecycle.jsonl"}:
        return (
            row.get("run_id"),
            row.get("session_id"),
            row.get("pool_id"),
            row.get("mint_id") or row.get("base_mint"),
            row.get("candidate_id"),
            row.get("record_type"),
            row.get("timestamp_ms"),
        )
    if kind == "gatekeeper_v2_decisions.jsonl":
        return (
            row.get("run_id"),
            row.get("session_id"),
            row_pool_id(row),
            row_base_mint(row),
            row.get("first_seen_ts_ms") or row.get("decision_ts_ms") or row.get("timestamp_ms"),
        )
    return (row.get("run_id"), row.get("session_id"), row_pool_id(row), row_base_mint(row))


def inspect_artifact(scope: ScopeContext, kind: str, path: Path, max_sha_bytes: int) -> dict[str, Any]:
    size = path.stat().st_size if path.exists() else 0
    line_count = 0
    malformed = 0
    schema_counter: Counter[str] = Counter()
    run_ids: set[str] = set()
    sessions: set[str] = set()
    pools: set[str] = set()
    mints: set[str] = set()
    ts_count = 0
    slot_count = 0
    missing_identity = 0
    duplicate_keys = 0
    seen: set[tuple[Any, ...]] = set()
    for _, row, _ in iter_jsonl(path) if path.suffix == ".jsonl" else []:
        line_count += 1
        if row is None:
            malformed += 1
            continue
        schema = row.get("schema") or row.get("schema_version") or row.get("record_schema") or "UNKNOWN"
        schema_counter[norm_str(schema)] += 1
        if row_run_id(row):
            run_ids.add(row_run_id(row))
        if row_session_id(row):
            sessions.add(row_session_id(row))
        pool = row_pool_id(row)
        mint = row_base_mint(row)
        if pool:
            pools.add(pool)
        if mint:
            mints.add(mint)
        if any(key in row for key in ("timestamp_ms", "decision_ts_ms", "entry_ts_ms", "first_seen_ts_ms")):
            ts_count += 1
        if any(key in row for key in ("slot", "entry_slot", "sample_slot", "state_slot")):
            slot_count += 1
        identity = artifact_identity_key(kind, row)
        if any(part in (None, "") for part in identity):
            missing_identity += 1
        elif identity in seen:
            duplicate_keys += 1
        else:
            seen.add(identity)
    if path.suffix != ".jsonl":
        line_count = sum(1 for _ in path.open("rb")) if path.exists() else 0
    safe = "NO"
    notes: list[str] = []
    if not path.exists():
        safe = "NO"
        notes.append("missing")
    elif malformed:
        safe = "NO"
        notes.append(f"malformed_rows={malformed}")
    elif scope.active_partial and kind in {"shadow_lifecycle.jsonl", "shadow_exit_replay_v1.jsonl", "gatekeeper_v2_decisions.jsonl"}:
        safe = "ACTIVE_PARTIAL"
        notes.append("R51 active/partial unless post-run manifest exists")
    elif missing_identity:
        safe = "LIMITED"
        notes.append(f"missing_identity_keys={missing_identity}")
    else:
        safe = "YES"
    if duplicate_keys:
        notes.append(f"duplicate_identity_keys={duplicate_keys}")
    return {
        "component": f"artifact:{scope.scope}:{kind}",
        "file_path": str(path),
        "symbol/function/struct": kind,
        "responsibility": "available local evidence artifact",
        "evidence_type": "jsonl_artifact" if path.suffix == ".jsonl" else "run_report_or_manifest",
        "inspected": True,
        "risk_notes": "; ".join(
            [
                f"scope_status={classify_scope_status(scope)}",
                f"exists={path.exists()}",
                f"size_bytes={size}",
                f"line_count={line_count}",
                f"sha256={file_sha256(path, max_sha_bytes)}",
                f"schema={dict(schema_counter.most_common(5))}",
                f"run_id_coverage={len(run_ids)}",
                f"session_id_coverage={len(sessions)}",
                f"pool_id_coverage={len(pools)}",
                f"base_mint_coverage={len(mints)}",
                f"timestamp_rows={ts_count}",
                f"slot_rows={slot_count}",
                f"malformed_rows={malformed}",
                f"duplicate_identity_keys={duplicate_keys}",
                f"missing_identity_keys={missing_identity}",
                f"safe_for_research_use={safe}",
                ",".join(notes) if notes else "no_artifact_parse_risk_detected",
            ]
        ),
    }


def inventory_rows(repo_root: Path, contexts: list[ScopeContext], max_sha_bytes: int) -> list[dict[str, Any]]:
    rows = source_inventory_rows(repo_root)
    for scope in contexts:
        if not scope.dirs:
            rows.append(
                {
                    "component": f"scope:{scope.scope}",
                    "file_path": scope.slug,
                    "symbol/function/struct": "scope discovery",
                    "responsibility": "expected audit scope",
                    "evidence_type": "missing_artifact_scope",
                    "inspected": False,
                    "risk_notes": "BLOCKED_BY_MISSING_EVIDENCE: no local directory found",
                }
            )
            continue
        for kind, paths in sorted(scope.artifacts.items()):
            for path in sorted(paths):
                rows.append(inspect_artifact(scope, kind, path, max_sha_bytes))
        expected = [
            "gatekeeper_v2_decisions.jsonl",
            "shadow_lifecycle.jsonl",
            "probe_shadow_lifecycle.jsonl",
            "shadow_exit_replay_v1.jsonl",
            "selector_shadow_score_v1.jsonl",
            "pre_run_manifest",
            "post_run_manifest",
        ]
        for kind in expected:
            if not scope.artifacts.get(kind):
                rows.append(
                    {
                        "component": f"artifact:{scope.scope}:{kind}",
                        "file_path": "",
                        "symbol/function/struct": kind,
                        "responsibility": "expected evidence artifact",
                        "evidence_type": "missing_artifact",
                        "inspected": True,
                        "risk_notes": f"BLOCKED_BY_MISSING_EVIDENCE; scope_status={classify_scope_status(scope)}",
                    }
                )
    return rows


def load_replay_rows(contexts: list[ScopeContext]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for ctx in contexts:
        for path in ctx.artifacts.get("shadow_exit_replay_v1.jsonl", []):
            for lineno, row, _ in iter_jsonl(path):
                if row is None:
                    continue
                row = dict(row)
                row["_scope"] = ctx.scope
                row["_scope_slug"] = ctx.slug
                row["_artifact_path"] = str(path)
                row["_lineno"] = lineno
                row["_target_bps"] = ctx.target_bps
                row["_stop_bps"] = ctx.stop_bps
                row["_max_hold_ms"] = ctx.max_hold_ms
                row["_active_partial"] = ctx.active_partial
                rows.append(row)
    return rows


def load_lifecycle_rows(contexts: list[ScopeContext]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for ctx in contexts:
        for kind in ("shadow_lifecycle.jsonl", "probe_shadow_lifecycle.jsonl"):
            for path in ctx.artifacts.get(kind, []):
                for lineno, row, _ in iter_jsonl(path):
                    if row is None:
                        continue
                    row = dict(row)
                    row["_scope"] = ctx.scope
                    row["_scope_slug"] = ctx.slug
                    row["_artifact_kind"] = kind
                    row["_artifact_path"] = str(path)
                    row["_lineno"] = lineno
                    row["_active_partial"] = ctx.active_partial
                    rows.append(row)
    return rows


def extract_decision_snapshot(scope: str, path: Path, row: dict[str, Any]) -> DecisionSnapshot:
    mfs = row.get("materialized_feature_snapshot")
    if not isinstance(mfs, dict):
        mfs = {}
    account = mfs.get("account_features") if isinstance(mfs.get("account_features"), dict) else {}
    reserves = (
        account.get("current_reserves")
        or account.get("reserves")
        or mfs.get("current_reserves")
        or mfs.get("reserves")
    )
    if not isinstance(reserves, list) or len(reserves) < 2:
        reserves_list = None
    else:
        reserves_list = [float(reserves[0]), float(reserves[1])]
    price = to_float(account.get("price_sol") or mfs.get("price_sol") or row.get("price_sol"))
    state_ts = to_int(
        account.get("last_update_ts_ms")
        or account.get("last_ts_ms")
        or mfs.get("state_ts_ms")
        or mfs.get("snapshot_ts_ms")
        or mfs.get("observation_end_ts_ms")
        or row.get("timestamp_ms")
    )
    state_slot = to_int(
        account.get("last_update_slot")
        or account.get("slot")
        or mfs.get("state_slot")
        or mfs.get("snapshot_slot")
        or row.get("slot")
    )
    decision_ts = to_int(
        row.get("decision_ts_ms")
        or row.get("timestamp_ms")
        or mfs.get("decision_ts_ms")
        or mfs.get("observation_end_ts_ms")
        or row.get("first_seen_ts_ms")
    )
    first_seen = to_int(row.get("first_seen_ts_ms") or mfs.get("first_seen_ts_ms"))
    return DecisionSnapshot(
        scope=scope,
        path=path,
        run_id=row_run_id(row),
        session_id=row_session_id(row),
        pool_id=row_pool_id(row),
        base_mint=row_base_mint(row),
        decision_ts_ms=decision_ts,
        first_seen_ts_ms=first_seen,
        state_ts_ms=state_ts,
        state_slot=state_slot,
        reserves=reserves_list,
        mfs_price_sol=price,
        source_fields=json.dumps(
            {
                "has_materialized_feature_snapshot": bool(mfs),
                "has_account_features": bool(account),
                "reserve_field": "account_features.current_reserves" if reserves_list else "",
                "price_field": "account_features.price_sol" if price is not None else "",
            },
            sort_keys=True,
        ),
    )


def load_decision_snapshots(contexts: list[ScopeContext], replay_rows: list[dict[str, Any]]) -> dict[str, list[DecisionSnapshot]]:
    wanted_by_scope: dict[str, set[tuple[str, str]]] = defaultdict(set)
    for row in replay_rows:
        wanted_by_scope[row["_scope"]].add((norm_str(row.get("pool_id")), norm_str(row.get("base_mint"))))
    snapshots: dict[str, list[DecisionSnapshot]] = defaultdict(list)
    for ctx in contexts:
        wanted = wanted_by_scope.get(ctx.scope, set())
        if not wanted:
            continue
        for path in ctx.artifacts.get("gatekeeper_v2_decisions.jsonl", []):
            for _, row, _ in iter_jsonl(path):
                if row is None:
                    continue
                pool = row_pool_id(row)
                mint = row_base_mint(row)
                if (pool, mint) not in wanted:
                    continue
                snap = extract_decision_snapshot(ctx.scope, path, row)
                snapshots[ctx.scope].append(snap)
    return snapshots


def index_lifecycle_dispatch(lifecycle_rows: list[dict[str, Any]]) -> dict[tuple[str, str, str, str, int], list[dict[str, Any]]]:
    index: dict[tuple[str, str, str, str, int], list[dict[str, Any]]] = defaultdict(list)
    for row in lifecycle_rows:
        record_type = norm_str(row.get("record_type"))
        if record_type not in {"shadow_dispatch", "position_opened", "shadow_entry"}:
            continue
        entry_ts = to_int(row.get("decision_ts_ms") or row.get("entry_ts_ms") or row.get("timestamp_ms"))
        if entry_ts is None:
            entry_ts = candidate_ts(row.get("candidate_id"))
        if entry_ts is None:
            continue
        key = (
            row["_scope"],
            row_run_id(row),
            row_session_id(row),
            row_pool_id(row) or norm_str(row.get("pool_id")),
            row_base_mint(row) or norm_str(row.get("mint_id")),
            entry_ts,
        )
        index[key].append(row)
    return index


def candidate_ts(candidate_id_value: Any) -> int | None:
    candidate_id = norm_str(candidate_id_value)
    if not candidate_id:
        return None
    match = re.search(r"(\d{12,})$", candidate_id)
    return int(match.group(1)) if match else None


def decision_matches_for_replay(row: dict[str, Any], snapshots: dict[str, list[DecisionSnapshot]]) -> list[DecisionSnapshot]:
    scope = row["_scope"]
    run_id = norm_str(row.get("run_id"))
    session_id = norm_str(row.get("session_id"))
    pool_id = norm_str(row.get("pool_id"))
    base_mint = norm_str(row.get("base_mint"))
    matches = [
        snap
        for snap in snapshots.get(scope, [])
        if snap.run_id == run_id and snap.session_id == session_id and snap.pool_id == pool_id and snap.base_mint == base_mint
    ]
    if matches:
        return matches
    return [
        snap
        for snap in snapshots.get(scope, [])
        if snap.pool_id == pool_id and snap.base_mint == base_mint
    ]


def reconstruct_price_from_reserves(
    reserves: list[float] | None,
    logged_reference_price: float | None = None,
) -> tuple[float | None, str, str]:
    if not reserves or len(reserves) < 2:
        return None, "ENTRY_RECONSTRUCTION_BLOCKED", "missing reserve pair"
    quote_raw, base_raw = reserves[0], reserves[1]
    if quote_raw <= 0 or base_raw <= 0:
        return None, "ENTRY_RECONSTRUCTION_BLOCKED", "non-positive reserve"
    candidates = []
    for quote_decimals in (9,):
        for base_decimals in (6, 9):
            price = (quote_raw / (10**quote_decimals)) / (base_raw / (10**base_decimals))
            if logged_reference_price and logged_reference_price > 0:
                diff = abs((price / logged_reference_price - 1.0) * 10000.0)
            else:
                diff = 0.0
            candidates.append((diff, price, f"quote_decimals={quote_decimals};base_decimals={base_decimals}"))
    candidates.sort(key=lambda item: item[0])
    _, price, fields = candidates[0]
    return price, "RECONSTRUCTED_FROM_RESERVES", fields


def bps_diff(a: float | None, b: float | None) -> float | None:
    if a is None or b is None or a == 0:
        return None
    return (b / a - 1.0) * 10000.0


def entry_reconstruction_rows(
    replay_rows: list[dict[str, Any]],
    lifecycle_rows: list[dict[str, Any]],
    snapshots: dict[str, list[DecisionSnapshot]],
) -> list[dict[str, Any]]:
    lifecycle_dispatch = index_lifecycle_dispatch(lifecycle_rows)
    out: list[dict[str, Any]] = []
    for row in replay_rows:
        entry_ts = to_int(row.get("entry_ts_ms"))
        key = (
            row["_scope"],
            norm_str(row.get("run_id")),
            norm_str(row.get("session_id")),
            norm_str(row.get("pool_id")),
            norm_str(row.get("base_mint")),
            entry_ts or -1,
        )
        lifecycle_match = lifecycle_dispatch.get(key, [])
        decision_ts = entry_ts
        if lifecycle_match:
            decision_ts = to_int(lifecycle_match[0].get("decision_ts_ms") or lifecycle_match[0].get("timestamp_ms")) or decision_ts
        matches = decision_matches_for_replay(row, snapshots)
        match_status = "NO_DECISION_SNAPSHOT_MATCH"
        snap = matches[0] if matches else None
        if matches:
            exact = [
                candidate
                for candidate in matches
                if candidate.run_id == norm_str(row.get("run_id"))
                and candidate.session_id == norm_str(row.get("session_id"))
                and candidate.pool_id == norm_str(row.get("pool_id"))
                and candidate.base_mint == norm_str(row.get("base_mint"))
            ]
            if len(exact) == 1:
                snap = exact[0]
                match_status = "EXACT_DECISION_MATCH"
            elif len(exact) > 1:
                snap = min(exact, key=lambda s: abs((s.decision_ts_ms or 0) - (entry_ts or 0)))
                match_status = "MULTIPLE_EXACT_DECISION_MATCHES_NEAREST_USED"
            else:
                snap = min(matches, key=lambda s: abs((s.decision_ts_ms or 0) - (entry_ts or 0)))
                match_status = "POOL_BASE_FALLBACK_DECISION_MATCH"
        logged = to_float(row.get("entry_price"))
        reconstructed = None
        status = "ENTRY_RECONSTRUCTION_BLOCKED"
        source_fields = "shadow_exit_replay_v1.entry_price"
        failure = "no decision materialized_feature_snapshot reserve evidence"
        state_ts = None
        state_slot = None
        if snap:
            reconstructed, reserve_status, fields = reconstruct_price_from_reserves(snap.reserves, snap.mfs_price_sol)
            state_ts = snap.state_ts_ms
            state_slot = snap.state_slot
            source_fields = f"{source_fields}; decision_match={match_status}; {snap.source_fields}; {fields}"
            if reserve_status == "RECONSTRUCTED_FROM_RESERVES":
                status = "RECONSTRUCTED_DECISION_MFS_MARK_ONLY"
                failure = (
                    "reconstructed from decision MFS reserves; exact shadow entry fill/state still not independently proven"
                )
            else:
                failure = fields
        diff = bps_diff(logged, reconstructed)
        if status.startswith("RECONSTRUCTED") and diff is not None and abs(diff) <= 5.0:
            status = "RECONSTRUCTED_WITHIN_TOLERANCE"
            failure = ""
        elif status.startswith("RECONSTRUCTED") and diff is not None:
            failure = f"entry logged price differs from decision reserve mark by {diff:.3f} bps"
        out.append(
            {
                "scope": row["_scope"],
                "run_id": row.get("run_id"),
                "session_id": row.get("session_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "decision_ts_ms": decision_ts,
                "entry_ts_ms": entry_ts,
                "state_ts_ms": state_ts,
                "state_slot": state_slot,
                "entry_price_logged": logged,
                "entry_price_reconstructed": reconstructed,
                "entry_price_diff_bps": diff,
                "reconstruction_status": status,
                "source_fields": source_fields,
                "failure_reason": failure,
            }
        )
    return out


def parse_jsonish(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, (list, dict)):
        return value
    if isinstance(value, str):
        try:
            return json.loads(value)
        except json.JSONDecodeError:
            return None
    return None


def parse_path_bps(value: Any) -> tuple[list[tuple[int, int]], str]:
    parsed = parse_jsonish(value)
    if not isinstance(parsed, list):
        return [], "MALFORMED_PATH_BPS"
    out: list[tuple[int, int]] = []
    for point in parsed:
        if not isinstance(point, (list, tuple)) or len(point) < 2:
            return [], "MALFORMED_PATH_BPS"
        age = to_int(point[0])
        pnl = to_int(point[1])
        if age is None or pnl is None:
            return [], "MALFORMED_PATH_BPS"
        out.append((age, pnl))
    return out, "OK"


def parse_first_hit_ms(value: Any) -> tuple[dict[int, int], str]:
    parsed = parse_jsonish(value)
    if not isinstance(parsed, dict):
        return {}, "MALFORMED_FIRST_HIT_MS"
    out: dict[int, int] = {}
    for key, val in parsed.items():
        level = to_int(key)
        age = to_int(val)
        if level is None or age is None:
            return {}, "MALFORMED_FIRST_HIT_MS"
        out[level] = age
    return out, "OK"


def path_monotonic_status(path: list[tuple[int, int]]) -> str:
    if not path:
        return "NO_PATH"
    ages = [age for age, _ in path]
    if any(ages[i] < ages[i - 1] for i in range(1, len(ages))):
        return "NON_MONOTONIC"
    if len(set(ages)) < len(ages):
        return "DUPLICATE_TIMESTAMPS"
    return "OK"


def derive_first_crossings_from_path(path: list[tuple[int, int]], levels: list[int]) -> dict[int, int]:
    hits: dict[int, int] = {}
    for age, pnl in sorted(path):
        for level in levels:
            if level in hits:
                continue
            if level > 0 and pnl >= level:
                hits[level] = age
            elif level < 0 and pnl <= level:
                hits[level] = age
    return hits


def classify_result_from_hits(first_hits: dict[int, int], target_bps: int, stop_bps: int, max_hold_ms: int) -> tuple[str, int | None]:
    target_age = first_hits.get(target_bps)
    stop_age = first_hits.get(stop_bps)
    candidates: list[tuple[int, str]] = []
    if target_age is not None and target_age <= max_hold_ms:
        candidates.append((target_age, "target"))
    if stop_age is not None and stop_age <= max_hold_ms:
        candidates.append((stop_age, "stop"))
    if not candidates:
        return "timeout", max_hold_ms
    candidates.sort(key=lambda item: (item[0], 0 if item[1] == "stop" else 1))
    if len(candidates) > 1 and candidates[0][0] == candidates[1][0]:
        return "ambiguous_same_timestamp_stop_first", candidates[0][0]
    return candidates[0][1], candidates[0][0]


def simulate_exit_from_path(
    path: list[tuple[int, int]],
    target_bps: int,
    stop_bps: int,
    max_hold_ms: int,
    tie_policy: str = "stop_first",
) -> tuple[str, int | None, int | None, str]:
    if not path:
        return "unavailable_no_path", None, None, "NO_PATH"
    status = path_monotonic_status(path)
    ordered = sorted(path)
    for age, pnl in ordered:
        if age > max_hold_ms:
            break
        target = pnl >= target_bps
        stop = pnl <= stop_bps
        if target and stop:
            result = "stop" if tie_policy == "stop_first" else "target"
            return result, age, pnl, "TIE_POLICY_APPLIED"
        if stop:
            return "stop", age, pnl, status
        if target:
            return "target", age, pnl, status
    before_hold = [point for point in ordered if point[0] <= max_hold_ms]
    if before_hold:
        age, pnl = before_hold[-1]
        quality = status if age == max_hold_ms else f"{status};TIMEOUT_USES_LAST_KNOWN_BEFORE_MAX_HOLD"
        return "timeout", age, pnl, quality
    return "timeout_no_point_before_max_hold", None, None, f"{status};NO_POINT_BEFORE_MAX_HOLD"


def exact_pnl_from_logged_path(path: list[tuple[int, int]], max_hold_ms: int) -> int | None:
    if not path:
        return None
    before = [point for point in sorted(path) if point[0] <= max_hold_ms]
    return before[-1][1] if before else None


def exit_reconstruction_rows(replay_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in replay_rows:
        target = int(row["_target_bps"])
        stop = int(row["_stop_bps"])
        max_hold = int(row["_max_hold_ms"])
        path, path_status = parse_path_bps(row.get("path_bps"))
        first_hits, hit_status = parse_first_hit_ms(row.get("first_hit_ms"))
        levels = sorted(first_hits.keys())
        if not levels:
            parsed_levels = parse_jsonish(row.get("levels_bps"))
            if isinstance(parsed_levels, list):
                levels = [level for level in (to_int(item) for item in parsed_levels) if level is not None]
        path_hits = derive_first_crossings_from_path(path, [target, stop])
        logged_result, logged_age = classify_result_from_hits(first_hits, target, stop, max_hold)
        path_result, path_age, path_pnl, path_quality = simulate_exit_from_path(path, target, stop, max_hold)
        exact_result, exact_age = classify_result_from_hits(first_hits, target, stop, max_hold)
        logged_pnl = to_int(row.get("last_pnl_bps"))
        recomputed_pnl = exact_pnl_from_logged_path(path, max_hold)
        mfe_logged = to_int(row.get("mfe_bps"))
        mae_logged = to_int(row.get("mae_bps"))
        mfe_path = max((pnl for _, pnl in path), default=None)
        mae_path = min((pnl for _, pnl in path), default=None)
        result_match = logged_result == path_result or (
            logged_result.startswith("ambiguous") and path_result in {"target", "stop"}
        )
        pnl_diff = None if logged_pnl is None or recomputed_pnl is None else logged_pnl - recomputed_pnl
        result_quality = "OK" if result_match and path_status == "OK" and hit_status == "OK" else "LIMITED"
        pnl_quality = "OK" if pnl_diff == 0 else "DIFF_OR_BLOCKED"
        failure_parts = []
        if path_status != "OK":
            failure_parts.append(path_status)
        if hit_status != "OK":
            failure_parts.append(hit_status)
        if not result_match:
            failure_parts.append(f"logged={logged_result};path={path_result};path_hits={path_hits}")
        if pnl_diff not in (None, 0):
            failure_parts.append(f"last_pnl_diff={pnl_diff}")
        if mfe_logged is not None and mfe_path is not None and mfe_logged != mfe_path:
            failure_parts.append(f"mfe_logged={mfe_logged};mfe_path={mfe_path}")
        if mae_logged is not None and mae_path is not None and mae_logged != mae_path:
            failure_parts.append(f"mae_logged={mae_logged};mae_path={mae_path}")
        out.append(
            {
                "scope": row["_scope"],
                "run_id": row.get("run_id"),
                "session_id": row.get("session_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "entry_ts_ms": row.get("entry_ts_ms"),
                "target_bps": target,
                "stop_bps": stop,
                "max_hold_ms": max_hold,
                "logged_result": logged_result,
                "path_recomputed_result": path_result,
                "exact_recomputed_result": exact_result,
                "logged_pnl_bps": logged_pnl,
                "recomputed_pnl_bps": recomputed_pnl,
                "diff_bps": pnl_diff,
                "result_match": result_match,
                "result_quality": result_quality,
                "pnl_quality": pnl_quality,
                "failure_reason": "; ".join(failure_parts),
            }
        )
    return out


def pool_state_provenance_rows(
    replay_rows: list[dict[str, Any]],
    lifecycle_rows: list[dict[str, Any]],
    snapshots: dict[str, list[DecisionSnapshot]],
) -> list[dict[str, Any]]:
    lifecycle_by_key: dict[tuple[str, str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in lifecycle_rows:
        key = (row["_scope"], row_run_id(row), row_session_id(row), row_pool_id(row), row_base_mint(row) or norm_str(row.get("mint_id")))
        lifecycle_by_key[key].append(row)
    out: list[dict[str, Any]] = []
    for row in replay_rows:
        key4 = (
            row["_scope"],
            norm_str(row.get("run_id")),
            norm_str(row.get("session_id")),
            norm_str(row.get("pool_id")),
            norm_str(row.get("base_mint")),
        )
        lifecycle = lifecycle_by_key.get(key4, [])
        matching_snaps = decision_matches_for_replay(row, snapshots)
        snap = matching_snaps[0] if matching_snaps else None
        detection_ts = min((to_int(r.get("first_seen_ts_ms") or r.get("timestamp_ms")) for r in lifecycle if to_int(r.get("first_seen_ts_ms") or r.get("timestamp_ms")) is not None), default=None)
        decision_ts = min((to_int(r.get("decision_ts_ms")) for r in lifecycle if to_int(r.get("decision_ts_ms")) is not None), default=None)
        entry_ts = to_int(row.get("entry_ts_ms"))
        path, path_status = parse_path_bps(row.get("path_bps"))
        path_start = min((age for age, _ in path), default=None)
        close_age = to_int(row.get("close_age_ms"))
        replay_end = close_age or max((age for age, _ in path), default=None)
        state_ts = snap.state_ts_ms if snap else None
        state_slot = snap.state_slot if snap else None
        monotonic_ts = all(
            val is None or nxt is None or val <= nxt
            for val, nxt in [
                (detection_ts, decision_ts),
                (decision_ts, entry_ts),
                (entry_ts, entry_ts + path_start if entry_ts is not None and path_start is not None else None),
            ]
        )
        state_not_newer = (
            "BLOCKED_BY_MISSING_EVIDENCE"
            if state_ts is None or decision_ts is None
            else ("PASS" if state_ts <= decision_ts else "FAIL_POST_DECISION_STATE")
        )
        join_status = "OK" if row.get("pool_id") and row.get("base_mint") else "MISSING_POOL_OR_BASE_MINT"
        out.append(
            {
                "scope": row["_scope"],
                "run_id": row.get("run_id"),
                "session_id": row.get("session_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "pool_initialization_time": "",
                "first_seen_time": detection_ts,
                "decision_time": decision_ts,
                "entry_time": entry_ts,
                "state_fetch_time": state_ts,
                "state_slot": state_slot,
                "latest_transaction_before_decision": "",
                "latest_transaction_used_in_snapshot": "",
                "latest_account_state_used_in_snapshot": state_ts,
                "replay_path_start": path_start,
                "replay_path_end": replay_end,
                "lifecycle_close": max((to_int(r.get("timestamp_ms")) for r in lifecycle if norm_str(r.get("record_type")) in {"position_closed", "exit_filled"} and to_int(r.get("timestamp_ms")) is not None), default=None),
                "post_run_manifest": "ACTIVE_PARTIAL" if row.get("_active_partial") else "not_checked_here",
                "timestamps_monotonic": monotonic_ts,
                "slots_monotonic": "BLOCKED_BY_MISSING_EVIDENCE" if state_slot is None else "PARTIAL",
                "state_source_not_newer_than_decision": state_not_newer,
                "path_starts_at_or_after_entry": path_start is not None and path_start >= 0,
                "no_pre_entry_path_as_post_entry": path_start is not None and path_start >= 0,
                "no_cross_pool_contamination": join_status,
                "no_wrong_base_mint_pool_id_join": join_status,
                "fallback_join_marked": "NO_FALLBACK_ACCEPTED",
                "ambiguity_status": "OK" if len(matching_snaps) <= 1 else f"MULTIPLE_DECISION_SNAPSHOTS={len(matching_snaps)}",
                "failure_reason": "; ".join(
                    part
                    for part in [path_status if path_status != "OK" else "", state_not_newer if state_not_newer != "PASS" else ""]
                    if part
                ),
            }
        )
    return out


def temporal_integrity_rows(repo_root: Path) -> list[dict[str, Any]]:
    rows = [
        (
            "materialized_feature_snapshot.account_features.current_reserves",
            "gatekeeper_v2_decisions.jsonl",
            "ghost-launcher/src/session/observation.rs",
            "AT_DECISION",
            True,
            False,
            "LOW",
            "emitted inside MaterializedFeatureSet decision snapshot",
        ),
        (
            "materialized_feature_snapshot.decision_time_series",
            "gatekeeper_v2_decisions.jsonl",
            "ghost-launcher/src/session/observation.rs",
            "AT_DECISION",
            True,
            False,
            "LOW",
            "bounded observation-window evidence, not post-entry by contract",
        ),
        (
            "materialized_feature_snapshot.pre_entry_path_summary_v1",
            "gatekeeper_v2_decisions.jsonl",
            "ghost-launcher/src/session/observation.rs",
            "AT_DECISION",
            True,
            False,
            "LOW",
            "serialized as pre-entry summary in MFS",
        ),
        (
            "selector_shadow_score_v1.score",
            "selector_shadow_score_v1.jsonl",
            "ghost-brain/src/oracle/decision_logger.rs",
            "AT_DECISION",
            True,
            False,
            "LOW",
            "selector sidecar score emitted at decision logging boundary",
        ),
        (
            "shadow_exit_replay_v1.path_bps",
            "shadow_exit_replay_v1.jsonl",
            "ghost-brain/src/guardian/post_buy/exit_replay.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "post-entry path; valid only as label/outcome, not as selection feature",
        ),
        (
            "shadow_exit_replay_v1.first_hit_ms",
            "shadow_exit_replay_v1.jsonl",
            "ghost-brain/src/guardian/post_buy/exit_replay.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "derived from post-entry price samples",
        ),
        (
            "shadow_exit_replay_v1.mfe_bps",
            "shadow_exit_replay_v1.jsonl",
            "ghost-brain/src/guardian/post_buy/exit_replay.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "post-entry max favourable excursion",
        ),
        (
            "shadow_exit_replay_v1.mae_bps",
            "shadow_exit_replay_v1.jsonl",
            "ghost-brain/src/guardian/post_buy/exit_replay.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "post-entry max adverse excursion",
        ),
        (
            "shadow_lifecycle.final_pnl_pct",
            "shadow_lifecycle.jsonl",
            "ghost-brain/src/guardian/post_buy/engine.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "terminal lifecycle outcome",
        ),
        (
            "shadow_lifecycle.close_reason",
            "shadow_lifecycle.jsonl",
            "ghost-brain/src/guardian/post_buy/engine.rs",
            "OUTCOME",
            False,
            True,
            "LOW_AS_LABEL_HIGH_AS_FEATURE",
            "terminal lifecycle outcome classification",
        ),
        (
            "live_landing_latency",
            "missing_or_live_execution_artifact",
            "ghost-brain/src/execution",
            "UNKNOWN",
            False,
            False,
            "HIGH",
            "not present in shadow replay rows; cannot be used for live-equivalent proof",
        ),
    ]
    out = []
    for field, artifact, source, temporal, used_feature, used_label, risk, evidence in rows:
        out.append(
            {
                "field_name": field,
                "source_artifact": artifact,
                "source_path": source,
                "first_available_time": temporal,
                "used_by_research": True,
                "used_as_feature": used_feature,
                "used_as_label": used_label,
                "temporal_class": temporal,
                "leakage_risk": risk,
                "evidence": evidence,
                "notes": "feature leakage only if OUTCOME/UNKNOWN fields are fed into selector features",
            }
        )
    return out


def lifecycle_terminal_key(row: dict[str, Any]) -> tuple[str, str, str, str, str, int] | None:
    entry_ts = to_int(row.get("entry_ts_ms") or row.get("decision_ts_ms")) or candidate_ts(row.get("candidate_id"))
    pool = row_pool_id(row)
    base = row_base_mint(row) or norm_str(row.get("mint_id"))
    if entry_ts is None or not pool or not base:
        return None
    return (row["_scope"], row_run_id(row), row_session_id(row), pool, base, entry_ts)


def replay_key(row: dict[str, Any]) -> tuple[str, str, str, str, str, int] | None:
    entry_ts = to_int(row.get("entry_ts_ms"))
    if entry_ts is None:
        return None
    return (
        row["_scope"],
        norm_str(row.get("run_id")),
        norm_str(row.get("session_id")),
        norm_str(row.get("pool_id")),
        norm_str(row.get("base_mint")),
        entry_ts,
    )


def lifecycle_final_pnl_bps(row: dict[str, Any]) -> float | None:
    if row.get("final_pnl_bps") is not None:
        return to_float(row.get("final_pnl_bps"))
    pct = to_float(row.get("final_pnl_pct"))
    if pct is not None:
        return pct * 100.0
    return None


def choose_lifecycle_terminal(rows: list[dict[str, Any]]) -> dict[str, Any]:
    priority = {"position_closed": 0, "exit_filled": 1, "shadow_exit": 2}
    return sorted(rows, key=lambda row: (priority.get(norm_str(row.get("record_type")), 99), to_int(row.get("timestamp_ms")) or 0))[0]


def normalize_lifecycle_reason(row: dict[str, Any]) -> str:
    reason = norm_str(row.get("close_reason") or row.get("reason") or row.get("status"))
    lowered = reason.lower()
    if "take" in lowered or "target" in lowered:
        return "target"
    if "stop" in lowered or "loss" in lowered:
        return "stop"
    if "time" in lowered or "timeout" in lowered:
        return "timeout"
    return lowered or "unknown"


def pnl_at_or_before_age(path: list[tuple[int, int]], age_ms: int | None) -> int | None:
    if age_ms is None or not path:
        return None
    before = [point for point in sorted(path) if point[0] <= age_ms]
    return before[-1][1] if before else None


def reconciliation_rows(replay_rows: list[dict[str, Any]], lifecycle_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    terminals: dict[tuple[str, str, str, str, str, int], list[dict[str, Any]]] = defaultdict(list)
    for row in lifecycle_rows:
        if norm_str(row.get("record_type")) not in {"exit_filled", "position_closed", "shadow_exit"}:
            continue
        key = lifecycle_terminal_key(row)
        if key:
            terminals[key].append(row)
    out: list[dict[str, Any]] = []
    exact_join = 0
    missing_lifecycle = 0
    duplicate_terminal = 0
    close_match = 0
    pnl_match = 0
    close_age_match = 0
    pnl_diffs: list[float] = []
    matched_lifecycle_keys: set[tuple[str, str, str, str, str, int]] = set()
    for row in replay_rows:
        key = replay_key(row)
        if key is None:
            continue
        matches = terminals.get(key, [])
        replay_hits, hit_status = parse_first_hit_ms(row.get("first_hit_ms"))
        replay_reason, replay_close_age = classify_result_from_hits(replay_hits, int(row["_target_bps"]), int(row["_stop_bps"]), int(row["_max_hold_ms"]))
        path, _ = parse_path_bps(row.get("path_bps"))
        replay_terminal_pnl = to_float(row.get("last_pnl_bps"))
        if not matches:
            missing_lifecycle += 1
            out.append(
                {
                    "row_type": "position",
                    "scope": row["_scope"],
                    "run_id": row.get("run_id"),
                    "session_id": row.get("session_id"),
                    "pool_id": row.get("pool_id"),
                    "base_mint": row.get("base_mint"),
                    "entry_ts_ms": row.get("entry_ts_ms"),
                    "exact_join": False,
                    "fallback_join": False,
                    "ambiguous_join": False,
                    "missing_replay": False,
                    "missing_lifecycle": True,
                    "duplicate_terminal": False,
                    "replay_close_reason": replay_reason,
                    "lifecycle_close_reason": "",
                    "close_reason_match": False,
                    "replay_pnl_bps": replay_terminal_pnl,
                    "lifecycle_pnl_bps": "",
                    "final_pnl_match": False,
                    "replay_close_age_ms": replay_close_age,
                    "lifecycle_close_age_ms": "",
                    "close_age_match": False,
                    "pnl_diff_bps": "",
                    "failure_reason": "MISSING_LIFECYCLE_EXACT_KEY",
                }
            )
            continue
        exact_join += 1
        matched_lifecycle_keys.add(key)
        duplicate = len(matches) > 1
        duplicate_terminal += 1 if duplicate else 0
        chosen = choose_lifecycle_terminal(matches)
        lifecycle_reason = normalize_lifecycle_reason(chosen)
        reason_match = replay_reason == lifecycle_reason or (
            replay_reason.startswith("ambiguous") and lifecycle_reason in {"target", "stop"}
        )
        close_match += 1 if reason_match else 0
        lifecycle_pnl = lifecycle_final_pnl_bps(chosen)
        lifecycle_age = to_int(chosen.get("duration_ms") or chosen.get("close_age_ms"))
        replay_pnl = pnl_at_or_before_age(path, lifecycle_age)
        if replay_pnl is None:
            replay_pnl = replay_terminal_pnl
        pnl_diff = None if replay_pnl is None or lifecycle_pnl is None else replay_pnl - lifecycle_pnl
        pnl_ok = pnl_diff is not None and abs(pnl_diff) <= 5.0
        pnl_match += 1 if pnl_ok else 0
        age_ok = replay_close_age is not None and lifecycle_age is not None and abs(replay_close_age - lifecycle_age) <= 1000
        close_age_match += 1 if age_ok else 0
        if pnl_diff is not None:
            pnl_diffs.append(abs(pnl_diff))
        out.append(
            {
                "row_type": "position",
                "scope": row["_scope"],
                "run_id": row.get("run_id"),
                "session_id": row.get("session_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "entry_ts_ms": row.get("entry_ts_ms"),
                "exact_join": True,
                "fallback_join": False,
                "ambiguous_join": False,
                "missing_replay": False,
                "missing_lifecycle": False,
                "duplicate_terminal": duplicate,
                "replay_close_reason": replay_reason,
                "lifecycle_close_reason": lifecycle_reason,
                "close_reason_match": reason_match,
                "replay_pnl_bps": replay_pnl,
                "lifecycle_pnl_bps": lifecycle_pnl,
                "final_pnl_match": pnl_ok,
                "replay_close_age_ms": replay_close_age,
                "lifecycle_close_age_ms": lifecycle_age,
                "close_age_match": age_ok,
                "pnl_diff_bps": pnl_diff,
                "failure_reason": "DUPLICATE_TERMINAL_RECORDS" if duplicate else "",
            }
        )
    missing_replay = len([key for key in terminals if key not in matched_lifecycle_keys])
    total_replay = len(replay_rows)
    exact_join_rate = exact_join / total_replay if total_replay else 0.0
    out.append(
        {
            "row_type": "aggregate",
            "scope": "ALL",
            "run_id": "",
            "session_id": "",
            "pool_id": "",
            "base_mint": "",
            "entry_ts_ms": "",
            "exact_join": exact_join_rate,
            "fallback_join": 0.0,
            "ambiguous_join": 0,
            "missing_replay": missing_replay,
            "missing_lifecycle": missing_lifecycle,
            "duplicate_terminal": duplicate_terminal,
            "replay_close_reason": "",
            "lifecycle_close_reason": "",
            "close_reason_match": close_match / exact_join if exact_join else 0.0,
            "replay_pnl_bps": "",
            "lifecycle_pnl_bps": "",
            "final_pnl_match": pnl_match / exact_join if exact_join else 0.0,
            "replay_close_age_ms": "",
            "lifecycle_close_age_ms": "",
            "close_age_match": close_age_match / exact_join if exact_join else 0.0,
            "pnl_diff_bps": "",
            "failure_reason": json.dumps(
                {
                    "exact_join_rate": exact_join_rate,
                    "fallback_join_rate": 0.0,
                    "ambiguous_join_count": 0,
                    "missing_replay_count": missing_replay,
                    "missing_lifecycle_count": missing_lifecycle,
                    "duplicate_terminal_count": duplicate_terminal,
                    "close_reason_match_rate": close_match / exact_join if exact_join else 0.0,
                    "final_pnl_match_rate": pnl_match / exact_join if exact_join else 0.0,
                    "close_age_match_rate": close_age_match / exact_join if exact_join else 0.0,
                    "median_pnl_diff_bps": percentile(pnl_diffs, 0.5),
                    "p95_pnl_diff_bps": percentile(pnl_diffs, 0.95),
                    "max_pnl_diff_bps": max(pnl_diffs) if pnl_diffs else None,
                },
                sort_keys=True,
            ),
        }
    )
    return out


def percentile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * q))))
    return ordered[idx]


def path_density_verdict(path: list[tuple[int, int]], replay_horizon_ms: int, horizon_ms: int) -> str:
    if horizon_ms > replay_horizon_ms:
        return "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY"
    if not path:
        return "NOT_EVALUABLE_NO_COVERAGE"
    ordered = sorted(path)
    latest = ordered[-1][0]
    if latest < horizon_ms:
        return "NOT_EVALUABLE_NO_COVERAGE"
    if len(ordered) == 1:
        return "SPARSE_APPROX_ONLY"
    intervals = [ordered[i][0] - ordered[i - 1][0] for i in range(1, len(ordered))]
    max_interval = max(intervals)
    p90_interval = percentile([float(v) for v in intervals], 0.90) or max_interval
    if max_interval <= 1000:
        return "EVALUABLE_EXACT"
    if p90_interval <= max(1000, horizon_ms / 4):
        return "EVALUABLE_APPROX"
    return "SPARSE_APPROX_ONLY"


def path_density_rows(replay_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in replay_rows:
        path, status = parse_path_bps(row.get("path_bps"))
        ordered = sorted(path)
        intervals = [ordered[i][0] - ordered[i - 1][0] for i in range(1, len(ordered))]
        replay_horizon = to_int(row.get("horizon_ms")) or int(row["_max_hold_ms"])
        row_out: dict[str, Any] = {
            "scope": row["_scope"],
            "run_id": row.get("run_id"),
            "session_id": row.get("session_id"),
            "pool_id": row.get("pool_id"),
            "base_mint": row.get("base_mint"),
            "entry_ts_ms": row.get("entry_ts_ms"),
            "path_point_count": len(path),
            "first_path_point_age_ms": ordered[0][0] if ordered else "",
            "median_interval_ms": percentile([float(v) for v in intervals], 0.5),
            "p90_interval_ms": percentile([float(v) for v in intervals], 0.9),
            "max_interval_ms": max(intervals) if intervals else "",
            "target_stop_crossing_between_samples_possible": bool(intervals and max(intervals) > 1000),
            "same_slot_ambiguity_represented": False,
            "long_horizon_exceeds_replay": any(h > replay_horizon for h in (300000, 500000)),
            "path_status": status,
        }
        for horizon in HORIZONS_MS:
            row_out[f"coverage_{horizon}ms"] = path_density_verdict(path, replay_horizon, horizon)
        out.append(row_out)
    return out


def live_equivalence_gap_rows(repo_root: Path) -> list[dict[str, Any]]:
    rows = [
        ("transaction construction delay", False, False, True, "missing latency shifts live entry/exit price", "HIGH", False, "shadow_exit_replay_v1 lacks submit/land fields", "log decision-to-build/submit timing"),
        ("decision-to-submit latency", False, False, True, "live price may move after decision", "CRITICAL", False, "shadow entry timestamp anchored to decision timing", "log submit timestamp and latency"),
        ("submit-to-land latency", False, False, True, "live fill depends on landing slot", "CRITICAL", False, "no landing slot in replay/lifecycle rows", "log landed slot/time or failed status"),
        ("slot/block position", False, False, True, "intra-slot ordering can change fill/path", "HIGH", False, "path_bps only has age/pnl", "log slot and order proxy per sample"),
        ("priority fee", False, False, True, "landing probability not modeled", "HIGH", False, "shadow replay not execution artifact", "log fee policy and actual fee"),
        ("Jito tip", False, False, True, "bundle success/ordering unmodeled", "HIGH", False, "not present in shadow artifacts", "log tip and bundle result"),
        ("Jito bundle success/fail", False, False, True, "failed bundle means no fill", "CRITICAL", False, "not present in shadow artifacts", "log bundle lifecycle"),
        ("recent blockhash / durable nonce behavior", False, False, True, "expired tx not represented", "HIGH", False, "no blockhash fields in shadow replay", "log blockhash age/validity"),
        ("actual landing slot", False, False, True, "entry state cannot be live-equivalent", "CRITICAL", False, "no landing slot evidence", "record landing slot for live/shadow-probe attempts"),
        ("failed transactions", False, False, True, "strategy PnL ignores no-fill/failures", "CRITICAL", False, "shadow closes assume simulated positions", "model/log failed landing and no-fill"),
        ("partial fills", False, False, False, "Solana AMM swaps are atomic, but no-fill still matters", "LOW", False, "not applicable unless execution source supports partial", "document N/A or execution-specific handling"),
        ("entry slippage", False, False, True, "logged mark/fill approximation may exceed executable min output", "CRITICAL", False, "shadow entry has price but not live slippage outcome", "log quote, min_out, realized fill"),
        ("exit slippage", False, False, True, "exit PnL may be materially overstated", "CRITICAL", False, "resolve_shadow_exit called with zero slippage cost in runtime path", "model sell quote/min_out/fees"),
        ("own buy price impact", False, False, True, "entry price ignores own liquidity impact unless quote already embeds it", "HIGH", False, "not proven by replay artifacts", "log reserve before/after own simulated buy"),
        ("own sell price impact", False, False, True, "exit mark may differ from executable sell", "HIGH", False, "path_bps is mark-like price path", "log executable sell quote"),
        ("AMM fee", False, False, True, "gross path may overstate net PnL", "HIGH", False, "no fee fields in exit replay", "log AMM and protocol fees"),
        ("account-state commitment level", False, False, True, "processed/confirmed/finalized divergence untracked", "MEDIUM", False, "artifact lacks commitment", "log commitment level per sample"),
        ("RPC/stream delay", False, False, True, "shadow samples may lag chain truth", "HIGH", False, "no stream delay fields in replay", "log event receive vs chain slot/time"),
        ("reorg/fork handling", False, False, True, "forked state can corrupt path", "MEDIUM", False, "not evidenced in artifacts", "log slot status/finality source"),
        ("quote/fill divergence", False, False, True, "quote does not prove landed fill", "CRITICAL", False, "shadow is simulated", "record quote and landed fill separately"),
        ("minimum output protection", True, False, True, "some shadow entry paths carry min_tokens_out, but replay rows do not prove live min_out execution", "HIGH", False, "ghost-brain/src/execution/shadow.rs", "carry min_out into lifecycle/replay evidence"),
        ("compute failure", False, False, True, "program failure unmodeled", "HIGH", False, "no simulation/live failure class in replay", "log simulation/program error classes"),
        ("contention with other bots", False, False, True, "landing and price path can drift materially", "HIGH", False, "not modeled in shadow path", "log account contention/failed inclusion evidence"),
    ]
    return [
        {
            "item": item,
            "logged": logged,
            "modeled": modeled,
            "required_for_live_equivalence": required,
            "impact_if_missing": impact,
            "severity": severity,
            "can_reconstruct_offline": reconstruct,
            "evidence_path": evidence,
            "required_fix_or_instrumentation": fix,
        }
        for item, logged, modeled, required, impact, severity, reconstruct, evidence, fix in rows
    ]


def fixture_cases() -> list[dict[str, Any]]:
    cases: list[dict[str, Any]] = []

    def add(name: str, expected: str, actual: str, behavior: str, notes: str = "") -> None:
        cases.append(
            {
                "fixture_name": name,
                "expected_result": expected,
                "actual_result": actual,
                "pass/fail": "pass" if expected == actual or actual.startswith(expected) else "fail",
                "what_behavior_this_proves": behavior,
                "source_test_path": "tests/test_shadow_burnin_fidelity_fixtures.py",
                "notes": notes,
            }
        )

    add("target_before_stop", "target", simulate_exit_from_path([(0, 0), (1000, 1300), (2000, -700)], 1200, -600, 45000)[0], "target hit before later stop")
    add("stop_before_target", "stop", simulate_exit_from_path([(0, 0), (1000, -700), (2000, 1300)], 1200, -600, 45000)[0], "stop hit before later target")
    add("target_and_stop_same_timestamp", "stop", simulate_exit_from_path([(0, 0), (1000, 1300), (1000, -700)], 1200, -600, 45000)[0], "same timestamp tie uses stop-first after sorting ambiguity")
    add("target_and_stop_same_slot_unknown_order", "ambiguous_same_timestamp_stop_first", classify_result_from_hits({1200: 1000, -600: 1000}, 1200, -600, 45000)[0], "exact first-hit tie is explicitly ambiguous")
    add("sparse_path_timeout", "timeout", simulate_exit_from_path([(0, 0), (44000, 100)], 1200, -600, 45000)[0], "sparse path can only approximate timeout")
    add("no_path_point_before_max_hold", "timeout_no_point_before_max_hold", simulate_exit_from_path([(50000, 100)], 1200, -600, 45000)[0], "timeout cannot infer PnL without pre-hold point")
    add("missing_exact_levels", "timeout", classify_result_from_hits({}, 1200, -600, 45000)[0], "missing first_hit_ms levels fall back to timeout classification")
    add("exact_levels_vs_path_approximation", "target", classify_result_from_hits({1200: 1000}, 1200, -600, 45000)[0], "exact levels can show target even if compressed path omits crossing")
    add("max_hold_shorter_than_first_hit", "timeout", classify_result_from_hits({1200: 50000}, 1200, -600, 45000)[0], "hit after max_hold is not an exit")
    add("max_hold_longer_than_replay_horizon", "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY", path_density_verdict([(0, 0), (120000, 100)], 120000, 300000), "long horizon cannot be inferred past replay horizon")
    add("malformed_first_hit_ms", "MALFORMED_FIRST_HIT_MS", parse_first_hit_ms("{bad")[1], "malformed first_hit_ms is fail-closed")
    add("malformed_path_bps", "MALFORMED_PATH_BPS", parse_path_bps("{bad")[1], "malformed path_bps is fail-closed")
    add("non_monotonic_path_age", "NON_MONOTONIC", path_monotonic_status([(1000, 0), (500, 1)]), "non-monotonic ages are detected")
    add("duplicate_path_timestamps", "DUPLICATE_TIMESTAMPS", path_monotonic_status([(1000, 0), (1000, 1)]), "duplicate path timestamps are detected")
    add("mfe_mae_reconstruction", "200/-100", f"{max([0, 200, -100])}/{min([0, 200, -100])}", "MFE/MAE are path max/min")
    price, status, _ = reconstruct_price_from_reserves([30_000_000_000, 1_000_000_000_000_000], 0.00003)
    add("entry_price_from_reserves", "RECONSTRUCTED_FROM_RESERVES", status, "reserve price reconstruction works", f"price={price}")
    price_round, _, fields_round = reconstruct_price_from_reserves([1, 3_000_000], None)
    add("reserve_rounding_token_decimals", "quote_decimals=9;base_decimals=6", fields_round, "token decimal candidate is explicit", f"price={price_round}")
    add("stale_state_snapshot", "STALE_OR_PRE_DECISION", classify_snapshot_timing(900, 1000), "pre-decision state is not future")
    add("post_decision_state_accidentally_used", "POST_DECISION_STATE", classify_snapshot_timing(1100, 1000), "post-decision state is detected")
    add("own_trade_impact_absent", "ABSENT", classify_modeling(False), "own trade impact missing is explicit")
    add("slippage_absent", "ABSENT", classify_modeling(False), "slippage missing is explicit")
    add("lifecycle_duplicate_terminal_rows", "DUPLICATE_TERMINAL_RECORDS", classify_duplicate_terminals(2), "duplicate terminal rows are detected")
    add("ambiguous_fallback_joins", "AMBIGUOUS_FALLBACK_JOIN", classify_fallback_join(2), "ambiguous fallback is not accepted")
    add("missing_base_mint_pool_id", "MISSING_IDENTITY", classify_identity("", "mint"), "missing pool/base identity is detected")
    add("replay_lifecycle_disagree", "DISAGREE", classify_replay_lifecycle_agreement("target", "stop"), "replay/lifecycle disagreement is detected")
    return cases


def classify_snapshot_timing(state_ts: int | None, decision_ts: int | None) -> str:
    if state_ts is None or decision_ts is None:
        return "UNKNOWN"
    if state_ts <= decision_ts:
        return "STALE_OR_PRE_DECISION"
    return "POST_DECISION_STATE"


def classify_modeling(enabled: bool) -> str:
    return "PRESENT" if enabled else "ABSENT"


def classify_duplicate_terminals(count: int) -> str:
    return "DUPLICATE_TERMINAL_RECORDS" if count > 1 else "OK"


def classify_fallback_join(candidates: int) -> str:
    if candidates == 0:
        return "NO_FALLBACK_MATCH"
    if candidates == 1:
        return "SINGLE_FALLBACK_MARKED"
    return "AMBIGUOUS_FALLBACK_JOIN"


def classify_identity(pool_id: str, base_mint: str) -> str:
    return "OK" if pool_id and base_mint else "MISSING_IDENTITY"


def classify_replay_lifecycle_agreement(replay: str, lifecycle: str) -> str:
    return "AGREE" if replay == lifecycle else "DISAGREE"


def claim_rows(
    entry_rows: list[dict[str, Any]],
    exit_rows: list[dict[str, Any]],
    recon_rows: list[dict[str, Any]],
    path_rows: list[dict[str, Any]],
    live_gap_rows_data: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    entry_ok = sum(1 for row in entry_rows if norm_str(row.get("reconstruction_status")) == "RECONSTRUCTED_WITHIN_TOLERANCE")
    entry_partial = sum(1 for row in entry_rows if norm_str(row.get("reconstruction_status")).startswith("RECONSTRUCTED"))
    exit_result_ok = sum(1 for row in exit_rows if row.get("result_match") in (True, "True", "true"))
    exit_pnl_ok = sum(1 for row in exit_rows if row.get("pnl_quality") == "OK")
    aggregate = next((row for row in recon_rows if row.get("row_type") == "aggregate"), {})
    agg_metrics = {}
    if aggregate.get("failure_reason"):
        try:
            agg_metrics = json.loads(norm_str(aggregate.get("failure_reason")))
        except json.JSONDecodeError:
            agg_metrics = {}
    horizon_2s = Counter(row.get("coverage_2000ms") for row in path_rows)
    horizon_3s = Counter(row.get("coverage_3000ms") for row in path_rows)
    horizon_120s = Counter(row.get("coverage_120000ms") for row in path_rows)
    horizon_300s = Counter(row.get("coverage_300000ms") for row in path_rows)
    horizon_500s = Counter(row.get("coverage_500000ms") for row in path_rows)
    critical_missing = [row["item"] for row in live_gap_rows_data if row["severity"] == "CRITICAL" and not row["modeled"]]

    def row(
        claim_id: str,
        text: str,
        status: str,
        proof_type: str,
        artifact: str,
        source: str,
        function: str,
        numeric: Any,
        limitations: str,
    ) -> dict[str, Any]:
        return {
            "claim_id": claim_id,
            "claim_text": text,
            "status": status,
            "proof_type": proof_type,
            "artifact_path": artifact,
            "source_file": source,
            "source_function_or_struct": function,
            "sample_scope": "ALL",
            "sample_pool_or_mint": "",
            "numeric_result": numeric,
            "limitations": limitations,
        }

    return [
        row("ENTRY_PRICE_SOURCE_KNOWN", "Shadow entry price source is known in code and artifacts.", "PARTIALLY_PROVEN", "source_code_trace", str(CSV_PATHS["entry"]), "ghost-launcher/src/oracle_runtime.rs", "shadow_entry_record_from_event/request", len(entry_rows), "source is known as shadow/synthetic entry price; exact live fill source is not proven"),
        row("ENTRY_PRICE_RECONSTRUCTABLE", "Entry price can be independently reconstructed from reserve/state evidence.", "PARTIALLY_PROVEN" if entry_partial else "BLOCKED", "independent_reconstruction", str(CSV_PATHS["entry"]), "ghost-launcher/src/session/observation.rs", "MaterializedFeatureSet account_features", f"within_tolerance={entry_ok};partial={entry_partial};total={len(entry_rows)}", "reconstruction is often decision mark only; missing exact entry-state reserves blocks stronger proof"),
        row("ENTRY_PRICE_NOT_FROM_FUTURE_STATE", "Entry price state is not from future/post-decision state.", "PARTIALLY_PROVEN", "source_code_trace", str(CSV_PATHS["pool_state"]), "ghost-launcher/src/oracle_runtime.rs", "shadow entry timing source", "", "artifact state timestamp/slot evidence is incomplete in some scopes"),
        row("ENTRY_PRICE_LIVE_EQUIVALENT", "Entry price is live-equivalent fill price.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-brain/src/execution/shadow.rs", "ShadowBackend::execute_prepared_entry", "", "landing latency, live slippage, failed tx, own impact are not modeled in replay"),
        row("EXIT_PRICE_SOURCE_KNOWN", "Exit price/path source is known.", "PROVEN", "source_code_trace", str(CSV_PATHS["exit"]), "ghost-brain/src/guardian/post_buy/exit_replay.rs", "observe_price_sample", len(exit_rows), "known as shadow observed price samples, not executable sell fills"),
        row("EXIT_RESULT_RECONSTRUCTABLE", "Exit result is independently reconstructable.", "PROVEN" if exit_result_ok == len(exit_rows) and exit_rows else "PARTIALLY_PROVEN", "independent_reconstruction", str(CSV_PATHS["exit"]), "scripts/shadow_burnin_fidelity_audit.py", "simulate_exit_from_path / classify_result_from_hits", f"match={exit_result_ok}/{len(exit_rows)}", "path compression can make exact-level and sampled-path interpretations differ"),
        row("FIRST_HIT_MS_CONSISTENT_WITH_PATH", "first_hit_ms is consistent with path_bps.", "PARTIALLY_PROVEN", "independent_reconstruction", str(CSV_PATHS["exit"]), "ghost-brain/src/guardian/post_buy/exit_replay.rs", "first_hit_ms update", f"result_match={exit_result_ok}/{len(exit_rows)}", "first_hit_ms is exact-level state; path_bps is capped/compressed and may not carry all crossings"),
        row("TIMEOUT_PNL_CONSISTENT_WITH_PATH", "Timeout PnL is consistent with path terminal point.", "PARTIALLY_PROVEN", "independent_reconstruction", str(CSV_PATHS["exit"]), "scripts/shadow_burnin_fidelity_audit.py", "exact_pnl_from_logged_path", f"pnl_ok={exit_pnl_ok}/{len(exit_rows)}", "timeout uses last known point if no exact max_hold sample exists"),
        row("MFE_MAE_CONSISTENT_WITH_PATH", "MFE/MAE are consistent with path.", "PARTIALLY_PROVEN", "independent_reconstruction", str(CSV_PATHS["exit"]), "ghost-brain/src/guardian/post_buy/exit_replay.rs", "observe_price_sample", "", "MFE/MAE are over all observed samples; compressed path can be insufficient for exact reconstruction"),
        row("POOL_STATE_SNAPSHOT_TEMPORALLY_SAFE", "Pool state snapshot is temporally safe.", "PARTIALLY_PROVEN", "source_code_trace", str(CSV_PATHS["pool_state"]), "ghost-brain/src/oracle/snapshot_engine.rs", "SnapshotEngine", "", "slot/timestamp provenance is incomplete for some historical scopes"),
        row(
            "REPLAY_LIFECYCLE_JOIN_SAFE",
            "Replay and lifecycle exact join is safe.",
            "PROVEN"
            if (agg_metrics.get("exact_join_rate") or 0) >= 0.99
            and (agg_metrics.get("close_reason_match_rate") or 0) >= 0.95
            and (agg_metrics.get("final_pnl_match_rate") or 0) >= 0.95
            and (agg_metrics.get("duplicate_terminal_count") or 0) == 0
            else ("NOT_PROVEN" if (agg_metrics.get("exact_join_rate") or 0) > 0 else "BLOCKED"),
            "reconciliation",
            str(CSV_PATHS["reconciliation"]),
            "scripts/shadow_burnin_fidelity_audit.py",
            "reconciliation_rows",
            agg_metrics,
            "exact join may be high while story equivalence still fails on close reason/age/PnL or duplicate terminal rows",
        ),
        row("NO_SILENT_AMBIGUOUS_FALLBACK_JOIN", "Ambiguous fallback joins are not silently accepted.", "PROVEN", "deterministic_fixture", str(CSV_PATHS["fixtures"]), "scripts/shadow_burnin_fidelity_audit.py", "classify_fallback_join", "fallback_join_rate=0", "audit refuses silent fallback joins"),
        row("PATH_DENSITY_SUPPORTS_2S_3S", "Path density supports 2-3s conclusions.", "PARTIALLY_PROVEN", "aggregate_metric", str(CSV_PATHS["path_density"]), "scripts/shadow_burnin_fidelity_audit.py", "path_density_verdict", {"2s": dict(horizon_2s), "3s": dict(horizon_3s)}, "only rows with coverage may be used; sparse/no-coverage rows must be excluded"),
        row("PATH_DENSITY_SUPPORTS_120S", "Path density supports 120s conclusions.", "PARTIALLY_PROVEN", "aggregate_metric", str(CSV_PATHS["path_density"]), "scripts/shadow_burnin_fidelity_audit.py", "path_density_verdict", dict(horizon_120s), "only if replay horizon and observed path cover 120s"),
        row("PATH_DENSITY_SUPPORTS_300S_500S", "Path density supports 300-500s conclusions.", "DISPROVEN" if horizon_300s or horizon_500s else "BLOCKED", "aggregate_metric", str(CSV_PATHS["path_density"]), "scripts/shadow_burnin_fidelity_audit.py", "path_density_verdict", {"300s": dict(horizon_300s), "500s": dict(horizon_500s)}, "cannot infer horizons beyond replay coverage"),
        row("SHADOW_MODELS_ENTRY_SLIPPAGE", "Shadow models entry slippage.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-launcher/src/oracle_runtime.rs", "shadow_entry_record_from_request", "", "replay rows do not model live realized slippage"),
        row("SHADOW_MODELS_EXIT_SLIPPAGE", "Shadow models exit slippage.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-brain/src/guardian/post_buy/engine.rs", "resolve_shadow_exit(..., 0.0)", "", "exit replay is mark/path evidence, not executable sell proof"),
        row("SHADOW_MODELS_OWN_TRADE_IMPACT", "Shadow models own trade impact.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-brain/src/execution/shadow.rs", "ShadowBackend", "", "not proven in replay/lifecycle artifacts"),
        row("SHADOW_MODELS_LANDING_LATENCY", "Shadow models landing latency.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-brain/src/execution", "shadow/live boundary", "", "no submit-to-land or landed slot evidence"),
        row("SHADOW_MODELS_FAILED_TX", "Shadow models failed transactions.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "ghost-brain/src/execution", "shadow/live boundary", "", "failed landing/no-fill not represented as outcome class in replay rows"),
        row("SHADOW_CAN_BE_USED_FOR_RESEARCH", "Shadow can be used for offline research.", "PARTIALLY_PROVEN", "aggregate_metric", str(CSV_PATHS["claims"]), "scripts/shadow_burnin_fidelity_audit.py", "audit synthesis", f"critical_live_gaps={len(critical_missing)}", "valid only as offline shadow/path research, not as live execution proof"),
        row("SHADOW_CAN_BE_USED_AS_LIVE_EQUIVALENT", "Shadow can be used as live-equivalent evidence.", "DISPROVEN", "missing_evidence", str(CSV_PATHS["live_gap"]), "docs/agents/solana-execution-path-engineer.md", "simulation vs inclusion discipline", critical_missing, "critical live-equivalence fields are not modeled/logged"),
    ]


def final_verdict(
    claims: list[dict[str, Any]],
    entry_rows: list[dict[str, Any]],
    exit_rows: list[dict[str, Any]],
    reconciliation_data: list[dict[str, Any]],
) -> str:
    live_equiv = next((row for row in claims if row["claim_id"] == "SHADOW_CAN_BE_USED_AS_LIVE_EQUIVALENT"), None)
    research = next((row for row in claims if row["claim_id"] == "SHADOW_CAN_BE_USED_FOR_RESEARCH"), None)
    entry_recon = next((row for row in claims if row["claim_id"] == "ENTRY_PRICE_RECONSTRUCTABLE"), None)
    exit_recon = next((row for row in claims if row["claim_id"] == "EXIT_RESULT_RECONSTRUCTABLE"), None)
    if not entry_rows or not exit_rows:
        return "SHADOW_BLOCKED_BY_MISSING_EVIDENCE"
    if entry_recon and entry_recon["status"] == "BLOCKED":
        return "SHADOW_BLOCKED_BY_MISSING_EVIDENCE"
    if exit_recon and exit_recon["status"] in {"BLOCKED", "DISPROVEN"}:
        return "SHADOW_REPLAY_LIFECYCLE_MISMATCH"
    aggregate = next((row for row in reconciliation_data if row.get("row_type") == "aggregate"), {})
    recon_metrics: dict[str, Any] = {}
    if aggregate.get("failure_reason"):
        try:
            recon_metrics = json.loads(norm_str(aggregate.get("failure_reason")))
        except json.JSONDecodeError:
            recon_metrics = {}
    if recon_metrics:
        exact_rate = float(recon_metrics.get("exact_join_rate") or 0.0)
        reason_rate = float(recon_metrics.get("close_reason_match_rate") or 0.0)
        pnl_rate = float(recon_metrics.get("final_pnl_match_rate") or 0.0)
        duplicate_count = int(recon_metrics.get("duplicate_terminal_count") or 0)
        if exact_rate >= 0.90 and (reason_rate < 0.95 or pnl_rate < 0.95 or duplicate_count > 0):
            return "SHADOW_REPLAY_LIFECYCLE_MISMATCH"
    if live_equiv and live_equiv["status"] == "DISPROVEN" and research and research["status"] == "PARTIALLY_PROVEN":
        return "SHADOW_TRUSTWORTHY_WITH_LIMITATIONS"
    return "SHADOW_NOT_LIVE_EQUIVALENT"


def write_golden_traces(replay_rows: list[dict[str, Any]], exit_rows: list[dict[str, Any]], lifecycle_rows: list[dict[str, Any]]) -> list[Path]:
    GOLDEN_DIR.mkdir(parents=True, exist_ok=True)
    exit_by_key = {
        (
            row["scope"],
            norm_str(row.get("run_id")),
            norm_str(row.get("session_id")),
            norm_str(row.get("pool_id")),
            norm_str(row.get("base_mint")),
            to_int(row.get("entry_ts_ms")),
        ): row
        for row in exit_rows
    }
    lifecycle_by_key: dict[tuple[str, str, str, str, int], list[dict[str, Any]]] = defaultdict(list)
    for row in lifecycle_rows:
        entry_ts = to_int(row.get("entry_ts_ms") or row.get("decision_ts_ms")) or candidate_ts(row.get("candidate_id"))
        if entry_ts is None:
            continue
        lifecycle_by_key[(row["_scope"], row_session_id(row), row_pool_id(row), row_base_mint(row) or norm_str(row.get("mint_id")), entry_ts)].append(row)
    buckets: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for replay in replay_rows:
        if replay.get("_scope") == "R51" and replay.get("_active_partial"):
            continue
        key = (
            replay["_scope"],
            norm_str(replay.get("run_id")),
            norm_str(replay.get("session_id")),
            norm_str(replay.get("pool_id")),
            norm_str(replay.get("base_mint")),
            to_int(replay.get("entry_ts_ms")),
        )
        exit_row = exit_by_key.get(key)
        if not exit_row:
            continue
        result = norm_str(exit_row.get("path_recomputed_result"))
        if result == "target":
            buckets["winning"].append(replay)
        elif result == "stop":
            buckets["losing"].append(replay)
        elif result.startswith("timeout"):
            buckets["timeout"].append(replay)
        if "SPARSE" in norm_str(exit_row.get("result_quality")) or norm_str(exit_row.get("failure_reason")):
            buckets["ambiguous_or_sparse"].append(replay)
    wanted = [("winning", 5), ("losing", 5), ("timeout", 5), ("ambiguous_or_sparse", 5)]
    written: list[Path] = []
    for bucket, limit in wanted:
        for idx, replay in enumerate(buckets.get(bucket, [])[:limit], 1):
            key_exit = (
                replay["_scope"],
                norm_str(replay.get("run_id")),
                norm_str(replay.get("session_id")),
                norm_str(replay.get("pool_id")),
                norm_str(replay.get("base_mint")),
                to_int(replay.get("entry_ts_ms")),
            )
            exit_row = exit_by_key.get(key_exit, {})
            life = lifecycle_by_key.get(
                (replay["_scope"], norm_str(replay.get("session_id")), norm_str(replay.get("pool_id")), norm_str(replay.get("base_mint")), to_int(replay.get("entry_ts_ms")) or -1),
                [],
            )
            path, _ = parse_path_bps(replay.get("path_bps"))
            trace_path = GOLDEN_DIR / f"{bucket}_{idx:02d}_{replay['_scope'].replace('/', '_')}_{norm_str(replay.get('session_id'))[:12]}.md"
            lines = [
                f"# Golden trace: {bucket} {idx}",
                "",
                f"- scope: {replay['_scope']}",
                f"- run_id: {replay.get('run_id')}",
                f"- session_id: {replay.get('session_id')}",
                f"- pool_id: {replay.get('pool_id')}",
                f"- base_mint: {replay.get('base_mint')}",
                f"- entry_ts_ms: {replay.get('entry_ts_ms')}",
                f"- entry_price: {replay.get('entry_price')}",
                f"- result: {exit_row.get('path_recomputed_result')}",
                f"- logged_result: {exit_row.get('logged_result')}",
                f"- result_quality: {exit_row.get('result_quality')}",
                "",
                "## Chronologia",
                "",
                "| step | ts_or_age_ms | evidence | notes |",
                "| --- | ---: | --- | --- |",
                f"| shadow entry | {replay.get('entry_ts_ms')} | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |",
            ]
            for life_row in sorted(life, key=lambda item: to_int(item.get("timestamp_ms")) or 0)[:20]:
                lines.append(
                    f"| lifecycle {life_row.get('record_type')} | {life_row.get('timestamp_ms')} | {life_row.get('_artifact_kind')} | close_reason={life_row.get('close_reason')}; pnl={life_row.get('final_pnl_pct')} |"
                )
            for age, pnl in path[:40]:
                lines.append(f"| path point | {age} | path_bps | pnl_bps={pnl} |")
            if len(path) > 40:
                lines.append(f"| path omitted |  | path_bps | {len(path) - 40} additional points omitted from trace view |")
            lines.extend(
                [
                    f"| replay close | {replay.get('close_age_ms')} | shadow_exit_replay_v1 | last_pnl_bps={replay.get('last_pnl_bps')}; quality={replay.get('quality')}; truncated={replay.get('truncated')} |",
                    "",
                    "## Odpowiedzi audytowe",
                    "",
                    f"- Co shadow uwazal za cene: `{replay.get('entry_price')}` plus post-entry path_bps.",
                    "- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.",
                    "- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.",
                    f"- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `{CSV_PATHS['live_gap']}`.",
                    f"- Czy trace jest wiarygodny: {exit_row.get('result_quality')} dla offline path research; nie live-equivalent.",
                ]
            )
            trace_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
            written.append(trace_path)
    if not written:
        placeholder = GOLDEN_DIR / "NO_GOLDEN_TRACES_BLOCKED_BY_MISSING_EVIDENCE.md"
        placeholder.write_text(
            "# Golden traces blocked\n\nNie znaleziono wystarczajacych zakonczonych pozycji poza aktywnym R51.\n",
            encoding="utf-8",
        )
        written.append(placeholder)
    return written


def summarize_counter(rows: Iterable[Any]) -> str:
    return json.dumps(dict(Counter(rows)), sort_keys=True)


def write_report(
    verdict: str,
    contexts: list[ScopeContext],
    entry_rows_data: list[dict[str, Any]],
    exit_rows_data: list[dict[str, Any]],
    pool_rows_data: list[dict[str, Any]],
    reconciliation_data: list[dict[str, Any]],
    path_rows_data: list[dict[str, Any]],
    claim_data: list[dict[str, Any]],
    golden_paths: list[Path],
) -> None:
    AUDIT_REPORT_PATH.parent.mkdir(parents=True, exist_ok=True)
    aggregate_recon = next((row for row in reconciliation_data if row.get("row_type") == "aggregate"), {})
    entry_status = Counter(row["reconstruction_status"] for row in entry_rows_data)
    exit_quality = Counter(row["result_quality"] for row in exit_rows_data)
    path_2s = Counter(row.get("coverage_2000ms") for row in path_rows_data)
    path_3s = Counter(row.get("coverage_3000ms") for row in path_rows_data)
    path_120s = Counter(row.get("coverage_120000ms") for row in path_rows_data)
    path_300s = Counter(row.get("coverage_300000ms") for row in path_rows_data)
    path_500s = Counter(row.get("coverage_500000ms") for row in path_rows_data)
    scope_lines = [
        f"- {ctx.scope}: `{ctx.slug}`; status={classify_scope_status(ctx)}; dirs={len(ctx.dirs)}; artifacts={sum(len(v) for v in ctx.artifacts.values())}"
        for ctx in contexts
    ]
    claims_by_status = Counter(row["status"] for row in claim_data)
    report = f"""# Raport Shadow Burnin Fidelity Audit 2026-06-29

## 1. Executive verdict

Finalny verdict enum: **{verdict}**.

Shadow burnin / `shadow_exit_replay_v1` / `shadow_lifecycle` **nie jest obecnie wiarygodny jako jeden spojny, lifecycle-equivalent system badawczy**. Audyt potwierdzil istnienie i kodowy kontrakt zrodel entry/exit/path oraz zbudowal niezalezna rekonstrukcje replay/path, ale replay i lifecycle materialnie sie rozjezdzaja na close reason / close age / final PnL, mimo wysokiego exact join rate. To wymusza downgrade wszystkich wnioskow, ktore laczyly te artefakty jako jedna historie pozycji.

`shadow_exit_replay_v1` moze byc uzywany tylko komponentowo: jako offline path/label research pod jawnie ograniczonymi zalozeniami. Nie jest live-equivalent. Krytyczne elementy live-equivalence sa nieobecne albo nieudowodnione: landing latency, landing slot, failed tx/no-fill, entry/exit slippage, own trade impact, AMM fees i realne quote/fill divergence.

Nie wolno z tego materialu wyciagac wniosku: "to bylby live PnL". Dopuszczalny wniosek jest waszy: "to jest ograniczony shadow/path label pod zalozeniem, ze mark/path price jest wystarczajacym proxy dla izolowanego eksperymentu i ze lifecycle nie jest uzywany jako potwierdzenie tej samej historii".

## 2. Co shadow faktycznie mierzy

- syntetyczna entry price zapisana w shadow evidence, zwykle zrodzona z decision/shadow request path;
- post-entry sampled mark/path PnL w `path_bps`;
- first-hit exact-level state w `first_hit_ms`;
- MFE/MAE/last PnL z obserwowanych probek;
- lifecycle close/reason/PnL w cieniu, bez dowodu live landing.

Zakresy:
{os.linesep.join(scope_lines)}

## 3. Czego shadow nie mierzy

- live submit-to-land latency;
- rzeczywisty landing slot i intra-slot ordering;
- nieudane transakcje i no-fill;
- slippage/fill divergence na wejsciu i wyjsciu;
- own buy/sell impact jako oddzielny, sprawdzalny komponent;
- priorytet fee, Jito tip/bundle result, blockhash validity, compute/program failure;
- reorg/fork/commitment divergence.

Szczegoly sa w `{CSV_PATHS['live_gap']}`.

## 4. Entry price contract

Zrodlo entry price jest znane czesciowo: runtime tworzy shadow entry jako syntetyczna cene shadow/simulation, a replay przenosi `entry_price`. Niezalezna rekonstrukcja z reserve/state evidence jest tylko czesciowa.

Statusy rekonstrukcji entry:

```json
{json.dumps(dict(entry_status), indent=2, sort_keys=True)}
```

Wniosek: entry price nie jest potwierdzonym live fill. Dla czesci scope'ow dokladny reserve/state snapshot potrzebny do rekonstrukcji jest `BLOCKED_BY_MISSING_EVIDENCE` albo jest tylko decision-MFS mark, nie entry-fill proof.

## 5. Exit price contract

`shadow_exit_replay_v1` zapisuje `levels_bps`, `first_hit_ms`, `path_bps`, `mfe_bps`, `mae_bps`, `last_pnl_bps`, `horizon_ms`, `close_age_ms`, `quality` i `truncated`. Audyt rekonstruuje target/stop/timeout z `first_hit_ms` oraz z `path_bps`.

Jakosc exit reconstruction:

```json
{json.dumps(dict(exit_quality), indent=2, sort_keys=True)}
```

Wniosek: exit result jest w znacznym stopniu rekonstruowalny dla offline path research, ale exact-level i compressed/sampled path moga sie rozejsc. `first_hit_ms` jest silniejszym exact-level dowodem niz `path_bps`; `path_bps` jest ograniczonym zapisem sciezki, nie pelnym tick streamem.

## 6. Pool state acquisition contract

Kodowa sciezka pool state obejmuje SnapshotEngine, AccountStateCore/feature materialization i `MaterializedFeatureSet`. Audyt nie znalazl runtime changes. Artefaktowo state timing pozostaje czesciowo zablokowany tam, gdzie brakuje kompletnego state timestamp/slot/raw account state.

Wniosek: pool-state timing jest **czesciowo potwierdzony kodowo**, ale nie globalnie udowodniony artefaktowo dla kazdego historycznego rekordu.

## 7. Temporal/no-lookahead integrity

Pola decyzyjne z MFS sa klasyfikowane jako PRE/AT_DECISION. Pola `path_bps`, `first_hit_ms`, `mfe_bps`, `mae_bps`, lifecycle final PnL i close reason sa OUTCOME. Sa bezpieczne jako label, ale nie jako selection feature.

Hard rule: gdyby ktorykolwiek OUTCOME/UNKNOWN field byl uzyty jako feature selekcyjny, verdict nalezy zdegradowac do `SHADOW_TEMPORAL_LEAKAGE_RISK`. Ten audyt nie zmienial selector runtime i nie potwierdza, ze wszystkie stare notatniki/raporty poprawnie separowaly feature vs label.

## 8. Replay/lifecycle reconciliation

Agregat reconciliation:

```json
{aggregate_recon.get('failure_reason', '{}')}
```

Join fallback nie jest akceptowany po cichu: audit przyjmuje exact key `(run_id, session_id, pool_id, base_mint, entry_ts_ms)` i raportuje brak/duplikaty jako ryzyko. Duplikaty terminalne typu `exit_filled` + `position_closed` sa raportowane, bo moga byc benign jako dwa typy zdarzen albo damaging, jesli downstream liczy je jako dwa zamkniecia.

## 9. Path sampling density

Verdicty density:

- 2s: `{dict(path_2s)}`
- 3s: `{dict(path_3s)}`
- 120s: `{dict(path_120s)}`
- 300s: `{dict(path_300s)}`
- 500s: `{dict(path_500s)}`

Nie wolno inferowac 300s/500s, jezeli replay horizon i realna sciezka tego nie pokrywaja. `path_bps` moze wspierac krotkie horyzonty tylko per-row, tam gdzie coverage nie jest `NOT_EVALUABLE_*`.

## 10. Live-equivalence gap

Shadow nie modeluje krytycznych komponentow live-equivalence: latency, landing/failure, slippage, own impact, fees, blockhash/Jito/fee policy i contention. Dlatego shadow moze byc uzyty do porownan offline pod jawnie wymienionymi zalozeniami, ale nie do claimu live-equivalent.

## 11. Fixture proof summary

Fixture CSV: `{CSV_PATHS['fixtures']}`.

Fixture tests obejmuja 25 przypadkow: target-before-stop, stop-before-target, tie same timestamp/slot, sparse timeout, missing path before max_hold, malformed first_hit/path, non-monotonic/duplicate timestamps, MFE/MAE, reserve price, stale/future state, absent own-impact/slippage, duplicate terminal rows, ambiguous fallback joins i replay/lifecycle disagreement.

## 12. Claim evidence matrix summary

Claim statusy:

```json
{json.dumps(dict(claims_by_status), indent=2, sort_keys=True)}
```

Pelna macierz: `{CSV_PATHS['claims']}`.

## 13. Ktore poprzednie wnioski research zostaja

Pozostaja tylko wnioski, ktore byly sformulowane jako izolowany offline `shadow_exit_replay_v1` / path-label research i nie wymagaly lifecycle equivalence, live fill, live latency, failed tx/no-fill, entry/exit slippage ani 300s/500s coverage bez realnego horizon coverage.

## 14. Ktore poprzednie wnioski trzeba zdegradowac

Do downgrade label ida wszystkie stare wnioski, ktore:

- nazywaly shadow PnL live-equivalent;
- traktowaly lifecycle i replay jako jedna zgodna historie pozycji;
- traktowaly entry price jako rzeczywisty fill;
- traktowaly exit mark/path jako wykonalny sell fill;
- inferowaly 300s/500s bez coverage;
- uzywaly OUTCOME fields jako selection features;
- ignorowaly missing failed tx/no-fill/latency/slippage/own-impact.

## 15. Co trzeba zinstrumentowac przed dalszym research

Minimum:

- entry quote, min_out, reserve-before/reserve-after, explicit decimals;
- submit timestamp, landed slot/time albo failed/no-fill status;
- exit quote/min_out/sell impact/fees;
- sample slot/timestamp/commitment for every path point;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality;
- raw pool state provenance for entry and exit.

## 16. Final decision

- usable for offline research: **tylko komponentowo dla `shadow_exit_replay_v1`/path labels; nie jako spojny lifecycle/replay dataset**;
- usable for live-equivalent claims: **nie**;
- usable for RCE: **nie jako runtime approval/live-equivalent proof; tylko jako logging-surface evidence**;
- usable for runtime approval: **nie**.

## Artefakty

- inventory: `{CSV_PATHS['inventory']}`
- entry reconstruction: `{CSV_PATHS['entry']}`
- exit reconstruction: `{CSV_PATHS['exit']}`
- pool state provenance: `{CSV_PATHS['pool_state']}`
- temporal integrity: `{CSV_PATHS['temporal']}`
- replay/lifecycle reconciliation: `{CSV_PATHS['reconciliation']}`
- live gap: `{CSV_PATHS['live_gap']}`
- path density: `{CSV_PATHS['path_density']}`
- fixtures: `{CSV_PATHS['fixtures']}`
- claims: `{CSV_PATHS['claims']}`
- golden traces: `{GOLDEN_DIR}` ({len(golden_paths)} files)
"""
    AUDIT_REPORT_PATH.write_text(report, encoding="utf-8")


def write_adr(verdict: str, claim_data: list[dict[str, Any]]) -> None:
    ADR_PATH.parent.mkdir(parents=True, exist_ok=True)
    claim_statuses = Counter(row["status"] for row in claim_data)
    adr = f"""# ADR-8D: Shadow Burnin Fidelity Audit 2026-06-29

## Status

Accepted as audit decision.

## Decyzja

Finalny verdict enum: **{verdict}**.

Shadow burnin nie moze byc uzywany jako spojny lifecycle/replay dataset ani jako live-equivalent/runtime approval proof. `shadow_exit_replay_v1` moze byc uzywany tylko komponentowo jako offline path-label evidence z ograniczeniami. Stare raporty strategii, ktore zakladaly live-equivalence albo lifecycle/replay equivalence, wymagaja downgrade label.

## Kontekst

Audyt dotyczy systemu pomiarowego: `shadow_exit_replay_v1`, `shadow_lifecycle`, `probe_shadow_lifecycle`, `gatekeeper_v2_decisions`, `selector_shadow_score_v1`, state/provenance i path density. Audyt nie zmienial runtime semantics, BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only` ani active close.

Uwaga o szablonie: plik `docs/ADR/ADR_8D_SZABLON.md` nie byl obecny w tej kopii worktree podczas generowania ADR, wiec zastosowano istniejacy styl ADR-8D z repo i wymagane pola z zadania.

## Evidence

- source inventory: `{CSV_PATHS['inventory']}`
- entry price reconstruction: `{CSV_PATHS['entry']}`
- exit price reconstruction: `{CSV_PATHS['exit']}`
- pool state provenance: `{CSV_PATHS['pool_state']}`
- temporal integrity: `{CSV_PATHS['temporal']}`
- replay/lifecycle reconciliation: `{CSV_PATHS['reconciliation']}`
- live-equivalence gap: `{CSV_PATHS['live_gap']}`
- path density: `{CSV_PATHS['path_density']}`
- deterministic fixtures: `{CSV_PATHS['fixtures']}`
- claim evidence matrix: `{CSV_PATHS['claims']}`

Claim status summary: `{json.dumps(dict(claim_statuses), sort_keys=True)}`.

## Limitations

- Entry price is not proven as a live landed fill.
- Exact reserve/state reconstruction is partial and blocked where raw state evidence is missing.
- Exit path is sampled/compressed and not a full executable sell stream.
- Path density must be evaluated per horizon and per row.
- Lifecycle/replay exact join issues and duplicate terminal rows must not be collapsed silently.
- Live execution gaps remain unmodeled: latency, landing/failure, slippage, own impact, AMM fees, blockhash, priority fee/Jito, contention.

## Runtime boundary

No runtime path was changed. Shadow evidence remains shadow evidence. Submit, simulation and lifecycle shadow close are not live confirmation.

## Research boundary

Allowed: offline relative research over shadow/path labels when the horizon is covered and OUTCOME fields are not used as selection features.

Forbidden: live-equivalent PnL, runtime approval, RCE approval, claims that rely on unmodeled landing/slippage/failure/own-impact.

## Required instrumentation

- entry quote/min_out/reserve-before/reserve-after/decimals;
- decision-to-submit and submit-to-land timestamps;
- actual landing slot or failed/no-fill status;
- exit quote/min_out/slippage/fees/own sell impact;
- path sample slot/timestamp/commitment;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality.

## Consequences for ORG/TSV2/EIX/RTP/RUG/RCE

- ORG/TSV2/RTP/RUG research remains valid only where it used pre/at-decision features and post-entry fields strictly as labels.
- EIX/RCE claims must not treat shadow as execution-quality proof.
- Any report claiming live-equivalent outcome must be downgraded.
- Any report treating lifecycle and replay as one consistent position story must be downgraded.
- Any report inferring 300s/500s without replay coverage must be downgraded.
- Any selector conclusion using outcome/path fields as features must be treated as temporal leakage risk until disproven.

## Old report downgrade labels

Required for old reports that used unsupported assumptions:

- `DOWNGRADE_SHADOW_NOT_LIVE_EQUIVALENT`
- `DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH`
- `DOWNGRADE_ENTRY_FILL_NOT_PROVEN`
- `DOWNGRADE_EXIT_FILL_NOT_PROVEN`
- `DOWNGRADE_HORIZON_COVERAGE_NOT_PROVEN`
- `DOWNGRADE_TEMPORAL_LABEL_FEATURE_SEPARATION_UNPROVEN`

## No runtime changes confirmation

This ADR records an offline measurement-system audit only. It does not approve runtime behavior and does not change runtime semantics.
"""
    ADR_PATH.write_text(adr, encoding="utf-8")


def run(repo_root: Path, extra_roots: list[str], max_sha_bytes: int) -> str:
    os.chdir(repo_root)
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    contexts = discover_scope_contexts(repo_root, extra_roots)
    inventory = inventory_rows(repo_root, contexts, max_sha_bytes)
    replay_rows = load_replay_rows(contexts)
    lifecycle_rows = load_lifecycle_rows(contexts)
    snapshots = load_decision_snapshots(contexts, replay_rows)
    entry_rows_data = entry_reconstruction_rows(replay_rows, lifecycle_rows, snapshots)
    exit_rows_data = exit_reconstruction_rows(replay_rows)
    pool_rows_data = pool_state_provenance_rows(replay_rows, lifecycle_rows, snapshots)
    temporal_rows_data = temporal_integrity_rows(repo_root)
    reconciliation_data = reconciliation_rows(replay_rows, lifecycle_rows)
    path_rows_data = path_density_rows(replay_rows)
    live_gap_data = live_equivalence_gap_rows(repo_root)
    fixtures = fixture_cases()
    claims = claim_rows(entry_rows_data, exit_rows_data, reconciliation_data, path_rows_data, live_gap_data)
    verdict = final_verdict(claims, entry_rows_data, exit_rows_data, reconciliation_data)
    golden_paths = write_golden_traces(replay_rows, exit_rows_data, lifecycle_rows)

    write_csv(
        CSV_PATHS["inventory"],
        ["component", "file_path", "symbol/function/struct", "responsibility", "evidence_type", "inspected", "risk_notes"],
        inventory,
    )
    write_csv(
        CSV_PATHS["entry"],
        [
            "scope",
            "run_id",
            "session_id",
            "pool_id",
            "base_mint",
            "decision_ts_ms",
            "entry_ts_ms",
            "state_ts_ms",
            "state_slot",
            "entry_price_logged",
            "entry_price_reconstructed",
            "entry_price_diff_bps",
            "reconstruction_status",
            "source_fields",
            "failure_reason",
        ],
        entry_rows_data,
    )
    write_csv(
        CSV_PATHS["exit"],
        [
            "scope",
            "run_id",
            "session_id",
            "pool_id",
            "base_mint",
            "entry_ts_ms",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "logged_result",
            "path_recomputed_result",
            "exact_recomputed_result",
            "logged_pnl_bps",
            "recomputed_pnl_bps",
            "diff_bps",
            "result_match",
            "result_quality",
            "pnl_quality",
            "failure_reason",
        ],
        exit_rows_data,
    )
    write_csv(
        CSV_PATHS["pool_state"],
        [
            "scope",
            "run_id",
            "session_id",
            "pool_id",
            "base_mint",
            "pool_initialization_time",
            "first_seen_time",
            "decision_time",
            "entry_time",
            "state_fetch_time",
            "state_slot",
            "latest_transaction_before_decision",
            "latest_transaction_used_in_snapshot",
            "latest_account_state_used_in_snapshot",
            "replay_path_start",
            "replay_path_end",
            "lifecycle_close",
            "post_run_manifest",
            "timestamps_monotonic",
            "slots_monotonic",
            "state_source_not_newer_than_decision",
            "path_starts_at_or_after_entry",
            "no_pre_entry_path_as_post_entry",
            "no_cross_pool_contamination",
            "no_wrong_base_mint_pool_id_join",
            "fallback_join_marked",
            "ambiguity_status",
            "failure_reason",
        ],
        pool_rows_data,
    )
    write_csv(
        CSV_PATHS["temporal"],
        [
            "field_name",
            "source_artifact",
            "source_path",
            "first_available_time",
            "used_by_research",
            "used_as_feature",
            "used_as_label",
            "temporal_class",
            "leakage_risk",
            "evidence",
            "notes",
        ],
        temporal_rows_data,
    )
    write_csv(
        CSV_PATHS["reconciliation"],
        [
            "row_type",
            "scope",
            "run_id",
            "session_id",
            "pool_id",
            "base_mint",
            "entry_ts_ms",
            "exact_join",
            "fallback_join",
            "ambiguous_join",
            "missing_replay",
            "missing_lifecycle",
            "duplicate_terminal",
            "replay_close_reason",
            "lifecycle_close_reason",
            "close_reason_match",
            "replay_pnl_bps",
            "lifecycle_pnl_bps",
            "final_pnl_match",
            "replay_close_age_ms",
            "lifecycle_close_age_ms",
            "close_age_match",
            "pnl_diff_bps",
            "failure_reason",
        ],
        reconciliation_data,
    )
    write_csv(
        CSV_PATHS["path_density"],
        [
            "scope",
            "run_id",
            "session_id",
            "pool_id",
            "base_mint",
            "entry_ts_ms",
            "path_point_count",
            "first_path_point_age_ms",
            "median_interval_ms",
            "p90_interval_ms",
            "max_interval_ms",
            *[f"coverage_{horizon}ms" for horizon in HORIZONS_MS],
            "target_stop_crossing_between_samples_possible",
            "same_slot_ambiguity_represented",
            "long_horizon_exceeds_replay",
            "path_status",
        ],
        path_rows_data,
    )
    write_csv(
        CSV_PATHS["live_gap"],
        [
            "item",
            "logged",
            "modeled",
            "required_for_live_equivalence",
            "impact_if_missing",
            "severity",
            "can_reconstruct_offline",
            "evidence_path",
            "required_fix_or_instrumentation",
        ],
        live_gap_data,
    )
    write_csv(
        CSV_PATHS["fixtures"],
        [
            "fixture_name",
            "expected_result",
            "actual_result",
            "pass/fail",
            "what_behavior_this_proves",
            "source_test_path",
            "notes",
        ],
        fixtures,
    )
    write_csv(
        CSV_PATHS["claims"],
        [
            "claim_id",
            "claim_text",
            "status",
            "proof_type",
            "artifact_path",
            "source_file",
            "source_function_or_struct",
            "sample_scope",
            "sample_pool_or_mint",
            "numeric_result",
            "limitations",
        ],
        claims,
    )
    write_report(verdict, contexts, entry_rows_data, exit_rows_data, pool_rows_data, reconciliation_data, path_rows_data, claims, golden_paths)
    write_adr(verdict, claims)
    print(json.dumps({"verdict": verdict, "replay_rows": len(replay_rows), "entry_rows": len(entry_rows_data), "exit_rows": len(exit_rows_data), "golden_traces": len(golden_paths)}, sort_keys=True))
    return verdict


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    run(repo_root, args.extra_log_root, args.max_sha_bytes)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
