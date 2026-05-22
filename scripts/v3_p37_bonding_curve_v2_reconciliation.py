#!/usr/bin/env python3
"""Reconcile P3.7 bonding_curve_v2 readiness with route/account sources.

This report is intentionally offline-first. It compares the account identity
that active/probe execution tried to load as `bonding_curve_v2` with:

- active/probe failure artifacts,
- V3/MFS decision rows,
- DIAG_ACCOUNT_UPDATE_RELAY logs,
- optional current RPC getMultipleAccounts results.

Current RPC checks are explicitly labeled as current visibility, not
decision-time visibility.
"""

from __future__ import annotations

import argparse
import json
import base64
import re
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable
from urllib import request as urllib_request

from shadow_run_report import load_toml, resolve_runtime_path


SCHEMA_VERSION = 1
DECISION_FILE_NAMES = ("gatekeeper_v2_buys.jsonl", "gatekeeper_v2_decisions.jsonl")
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
DIAG_ACCOUNT_UPDATE_RE = re.compile(
    r"DIAG_ACCOUNT_UPDATE_RELAY\s+"
    r"(?:\S+\s+)*?base_mint=(?P<base_mint>\S+)\s+"
    r"(?:\S+\s+)*?bonding_curve=(?P<bonding_curve>\S+)\s+"
    r"(?:\S+\s+)*?slot=(?P<slot>\d+)"
)
EXECUTION_ACCOUNT_NOT_READY_RE = re.compile(
    r"^execution_account_not_ready:(?P<role>[^:]+):(?P<pubkey>[^:]+)$"
)


def strip_ansi(value: str) -> str:
    return ANSI_RE.sub("", value)


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line_number, line in enumerate(fh, 1):
            raw = line.strip()
            if not raw:
                continue
            try:
                row = json.loads(raw)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_number}: invalid JSONL: {exc}") from exc
            if isinstance(row, dict):
                yield row


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def parse_timestamp_ms(line: str) -> int | None:
    clean = strip_ansi(line).strip()
    if not clean:
        return None
    token = clean.split(maxsplit=1)[0]
    try:
        parsed = datetime.fromisoformat(token.replace("Z", "+00:00"))
    except ValueError:
        return None
    return int(parsed.astimezone(timezone.utc).timestamp() * 1000)


def parse_execution_account_not_ready(reason: str | None) -> tuple[str | None, str | None]:
    if not reason:
        return None, None
    match = EXECUTION_ACCOUNT_NOT_READY_RE.match(reason)
    if not match:
        return None, None
    return match.group("role"), match.group("pubkey")


def recursive_contains_key(value: Any, key: str) -> bool:
    if isinstance(value, dict):
        return key in value or any(recursive_contains_key(v, key) for v in value.values())
    if isinstance(value, list):
        return any(recursive_contains_key(v, key) for v in value)
    return False


def recursive_contains_value(value: Any, needle: str | None) -> bool:
    if not needle:
        return False
    if isinstance(value, str):
        return value == needle
    if isinstance(value, dict):
        return any(recursive_contains_value(v, needle) for v in value.values())
    if isinstance(value, list):
        return any(recursive_contains_value(v, needle) for v in value)
    return False


def recursive_string_values_for_keys(value: Any, keys: set[str]) -> list[str]:
    found: list[str] = []
    if isinstance(value, dict):
        for key, child in value.items():
            if key in keys and isinstance(child, str):
                found.append(child)
            found.extend(recursive_string_values_for_keys(child, keys))
    elif isinstance(value, list):
        for child in value:
            found.extend(recursive_string_values_for_keys(child, keys))
    return sorted(set(found))


def flatten_decision_logs(decision_root: Path) -> list[tuple[Path, int, dict[str, Any]]]:
    rows: list[tuple[Path, int, dict[str, Any]]] = []
    for name in DECISION_FILE_NAMES:
        for path in sorted(decision_root.rglob(name)):
            for index, row in enumerate(iter_jsonl(path)):
                rows.append((path, index, row))
    return rows


def build_decision_indexes(
    decisions: list[tuple[Path, int, dict[str, Any]]],
) -> tuple[dict[str, tuple[Path, int, dict[str, Any]]], dict[str, list[tuple[Path, int, dict[str, Any]]]]]:
    by_ab: dict[str, tuple[Path, int, dict[str, Any]]] = {}
    by_mint: dict[str, list[tuple[Path, int, dict[str, Any]]]] = {}
    for item in decisions:
        _, _, row = item
        ab_record_id = row.get("ab_record_id")
        if isinstance(ab_record_id, str) and ab_record_id not in by_ab:
            by_ab[ab_record_id] = item
        base_mint = row.get("base_mint") or row.get("mint_id")
        if isinstance(base_mint, str):
            by_mint.setdefault(base_mint, []).append(item)
    return by_ab, by_mint


def build_diag_index(log_paths: list[Path]) -> dict[str, Any]:
    by_pair: dict[tuple[str, str], list[dict[str, Any]]] = {}
    by_mint: dict[str, list[dict[str, Any]]] = {}
    by_curve: dict[str, list[dict[str, Any]]] = {}
    total = 0
    for path in log_paths:
        if not path.exists() or not path.is_file():
            continue
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for raw_line in fh:
                line = strip_ansi(raw_line)
                if "DIAG_ACCOUNT_UPDATE_RELAY" not in line:
                    continue
                match = DIAG_ACCOUNT_UPDATE_RE.search(line)
                if not match:
                    continue
                ts_ms = parse_timestamp_ms(raw_line)
                base_mint = match.group("base_mint")
                bonding_curve = match.group("bonding_curve")
                record = {
                    "ts_ms": ts_ms,
                    "base_mint": base_mint,
                    "bonding_curve": bonding_curve,
                    "slot": int(match.group("slot")),
                    "log_path": str(path),
                    "context": line.strip()[:420],
                }
                total += 1
                by_pair.setdefault((base_mint, bonding_curve), []).append(record)
                by_mint.setdefault(base_mint, []).append(record)
                by_curve.setdefault(bonding_curve, []).append(record)
    for records in list(by_pair.values()) + list(by_mint.values()) + list(by_curve.values()):
        records.sort(key=lambda row: (row.get("ts_ms") is None, row.get("ts_ms") or 0))
    return {
        "diag_account_update_total": total,
        "by_pair": by_pair,
        "by_mint": by_mint,
        "by_curve": by_curve,
    }


def scan_raw_occurrences(log_paths: list[Path], needle: str | None) -> dict[str, Any]:
    if not needle:
        return {"raw_log_occurrences": 0, "diag_raw_occurrences": 0, "first_raw_context": None}
    raw_count = 0
    diag_count = 0
    first_context = None
    for path in log_paths:
        if not path.exists() or not path.is_file():
            continue
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                if needle not in line:
                    continue
                raw_count += 1
                clean = strip_ansi(line).strip()
                if "DIAG_ACCOUNT_UPDATE_RELAY" in clean:
                    diag_count += 1
                if first_context is None:
                    first_context = f"{path}: {clean[:420]}"
    return {
        "raw_log_occurrences": raw_count,
        "diag_raw_occurrences": diag_count,
        "first_raw_context": first_context,
    }


def resolve_namespace(config: dict[str, Any], config_path: Path) -> str:
    probe = config.get("p37_shadow_probe") or {}
    namespace = probe.get("namespace") or probe.get("run_id")
    if isinstance(namespace, str) and namespace:
        return namespace
    output_path = ((config.get("trigger") or {}).get("shadow_run") or {}).get("output_path")
    if isinstance(output_path, str) and output_path:
        resolved = resolve_runtime_path(config_path, output_path)
        if resolved.parent.name:
            return resolved.parent.name
    raise SystemExit("Unable to resolve run namespace from config")


def resolve_paths(config_path: Path, config: dict[str, Any], namespace: str) -> dict[str, Path]:
    trigger_shadow = ((config.get("trigger") or {}).get("shadow_run") or {})
    execution_shadow = ((config.get("execution") or {}).get("shadow") or {})
    shadow_root = Path("logs/shadow_run") / namespace
    rollout_root = Path("logs/rollout") / namespace
    return {
        "shadow_root": shadow_root,
        "rollout_root": rollout_root,
        "decision_root": rollout_root / "decisions",
        "buys": resolve_runtime_path(config_path, trigger_shadow.get("output_path"))
        if isinstance(trigger_shadow.get("output_path"), str)
        else shadow_root / "buys.jsonl",
        "entries": resolve_runtime_path(config_path, execution_shadow.get("entry_log_path"))
        if isinstance(execution_shadow.get("entry_log_path"), str)
        else shadow_root / "shadow_entries.jsonl",
        "lifecycle": resolve_runtime_path(config_path, execution_shadow.get("lifecycle_log_path"))
        if isinstance(execution_shadow.get("lifecycle_log_path"), str)
        else shadow_root / "shadow_lifecycle.jsonl",
        "probe_selection": shadow_root / "probe_selection.jsonl",
        "probe_skips": shadow_root / "probe_skips.jsonl",
    }


def discover_log_paths(rollout_root: Path) -> list[Path]:
    if not rollout_root.exists():
        return []
    return sorted(
        path
        for path in rollout_root.rglob("*")
        if path.is_file() and path.suffix in {".log", ".jsonl"}
    )


def extract_bcv2_failure(row: dict[str, Any]) -> tuple[str | None, str | None]:
    role, pubkey = parse_execution_account_not_ready(row.get("precheck_failure_reason") or row.get("err"))
    if role == "bonding_curve_v2" and pubkey:
        return role, pubkey
    if row.get("simulation_error_account_role") == "bonding_curve_v2":
        pubkey = row.get("simulation_error_account_pubkey")
        if isinstance(pubkey, str) and pubkey:
            return "bonding_curve_v2", pubkey
    return None, None


def collect_cases(paths: dict[str, Path]) -> list[dict[str, Any]]:
    raw_cases: list[dict[str, Any]] = []
    artifact_specs = [
        ("active_shadow", "buys", paths["buys"]),
        ("active_shadow", "entry", paths["entries"]),
        ("active_shadow", "lifecycle", paths["lifecycle"]),
        ("probe", "probe_skip", paths["probe_skips"]),
    ]
    for plane, artifact_type, path in artifact_specs:
        for row_index, row in enumerate(iter_jsonl(path)):
            role, pubkey = extract_bcv2_failure(row)
            if role != "bonding_curve_v2" or not pubkey:
                continue
            raw_cases.append(
                {
                    "plane": plane,
                    "artifact_type": artifact_type,
                    "artifact_path": str(path),
                    "artifact_row_index": row_index,
                    "ab_record_id": row.get("ab_record_id") or row.get("source_ab_record_id"),
                    "pool_id": row.get("pool_id") or row.get("pool_amm_id"),
                    "base_mint": row.get("base_mint") or row.get("mint_id"),
                    "decision_ts_ms": row.get("decision_ts_ms"),
                    "probe_selected_ts_ms": row.get("probe_selected_ts_ms"),
                    "route_kind": row.get("route_kind"),
                    "buy_variant": row.get("buy_variant"),
                    "token_param_role": row.get("token_param_role"),
                    "builder_bonding_curve_v2_pubkey": pubkey,
                    "precheck_failure_reason": row.get("precheck_failure_reason") or row.get("err"),
                    "simulation_error_category": row.get("simulation_error_category"),
                    "simulation_error_account_source": row.get("simulation_error_account_source"),
                    "simulation_error_instruction_index": row.get("simulation_error_instruction_index"),
                    "simulation_error_account_index": row.get("simulation_error_account_index"),
                    "account_set_match": row.get("account_set_match"),
                    "prepared_request_account_set_hash": row.get("prepared_request_account_set_hash"),
                    "simulation_account_set_hash": row.get("simulation_account_set_hash"),
                    "raw_row": row,
                }
            )

    deduped: dict[tuple[str, str, str], dict[str, Any]] = {}
    for case in raw_cases:
        key = (
            str(case.get("plane")),
            str(case.get("ab_record_id")),
            str(case.get("builder_bonding_curve_v2_pubkey")),
        )
        existing = deduped.get(key)
        if existing is None:
            case["artifact_sources"] = [case["artifact_type"]]
            case["artifact_row_count"] = 1
            deduped[key] = case
        else:
            existing["artifact_sources"].append(case["artifact_type"])
            existing["artifact_row_count"] += 1
    return list(deduped.values())


def choose_decision_row(
    case: dict[str, Any],
    by_ab: dict[str, tuple[Path, int, dict[str, Any]]],
    by_mint: dict[str, list[tuple[Path, int, dict[str, Any]]]],
) -> tuple[Path | None, int | None, dict[str, Any] | None, str]:
    ab_record_id = case.get("ab_record_id")
    if isinstance(ab_record_id, str) and ab_record_id in by_ab:
        path, index, row = by_ab[ab_record_id]
        return path, index, row, "ab_record_id"
    base_mint = case.get("base_mint")
    if isinstance(base_mint, str):
        rows = by_mint.get(base_mint) or []
        if rows:
            path, index, row = rows[0]
            return path, index, row, "base_mint_first"
    return None, None, None, "missing"


def account_fields_from_decision(row: dict[str, Any] | None, pubkey: str | None) -> dict[str, Any]:
    if not row:
        return {
            "decision_row_present": False,
            "mfs_present": False,
            "mfs_contains_bonding_curve_v2_key": False,
            "mfs_contains_builder_bcv2_pubkey": False,
        }
    snapshot = row.get("v3_materialized_feature_snapshot") or {}
    account_features = snapshot.get("account_features") or {}
    mfs_bonding_curve_pubkeys = recursive_string_values_for_keys(
        snapshot,
        {
            "bonding_curve",
            "bonding_curve_pubkey",
            "bondingCurve",
            "bondingCurvePubkey",
        },
    )
    mfs_bonding_curve_v2_pubkeys = recursive_string_values_for_keys(
        snapshot,
        {
            "bonding_curve_v2",
            "bonding_curve_v2_pubkey",
            "bondingCurveV2",
            "bondingCurveV2Pubkey",
        },
    )
    return {
        "decision_row_present": True,
        "verdict_type": row.get("verdict_type"),
        "reason_code": row.get("reason_code"),
        "decision_plane": row.get("decision_plane"),
        "curve_data_known": row.get("curve_data_known"),
        "curve_finality": row.get("curve_finality"),
        "dev_pubkey": row.get("dev_pubkey"),
        "mfs_present": bool(snapshot),
        "mfs_contains_bonding_curve_v2_key": recursive_contains_key(snapshot, "bonding_curve_v2"),
        "mfs_contains_builder_bcv2_pubkey": recursive_contains_value(snapshot, pubkey),
        "mfs_bonding_curve_pubkeys": mfs_bonding_curve_pubkeys,
        "mfs_bonding_curve_v2_pubkeys": mfs_bonding_curve_v2_pubkeys,
        "mfs_contains_pool_id": recursive_contains_value(snapshot, row.get("pool_id")),
        "account_features_update_count": account_features.get("update_count"),
        "account_features_state_phase": account_features.get("state_phase"),
        "account_features_curve_finality": account_features.get("curve_finality"),
    }


def analyze_diag_for_case(
    case: dict[str, Any],
    diag_index: dict[str, Any],
    log_paths: list[Path],
) -> dict[str, Any]:
    base_mint = case.get("base_mint")
    pubkey = case.get("builder_bonding_curve_v2_pubkey")
    decision_ts_ms = case.get("decision_ts_ms")
    exact_records = []
    other_records = []
    if isinstance(base_mint, str) and isinstance(pubkey, str):
        exact_records = diag_index["by_pair"].get((base_mint, pubkey), [])
        all_for_mint = diag_index["by_mint"].get(base_mint, [])
        other_records = [row for row in all_for_mint if row.get("bonding_curve") != pubkey]
    elif isinstance(pubkey, str):
        exact_records = diag_index["by_curve"].get(pubkey, [])

    def first_ts(records: list[dict[str, Any]]) -> int | None:
        return next((row.get("ts_ms") for row in records if row.get("ts_ms") is not None), None)

    def seen_before(records: list[dict[str, Any]]) -> bool:
        if decision_ts_ms is None:
            return False
        return any(row.get("ts_ms") is not None and row["ts_ms"] <= decision_ts_ms for row in records)

    raw_pubkey = scan_raw_occurrences(log_paths, pubkey)
    other_curves = sorted({str(row.get("bonding_curve")) for row in other_records if row.get("bonding_curve")})
    return {
        "diag_seen_exact_pubkey": bool(exact_records),
        "diag_seen_exact_before_decision": seen_before(exact_records),
        "diag_exact_occurrences": len(exact_records),
        "diag_exact_first_ts_ms": first_ts(exact_records),
        "diag_exact_first_slot": exact_records[0].get("slot") if exact_records else None,
        "diag_seen_other_curve_pubkey_for_mint": bool(other_records),
        "diag_other_curve_occurrences_for_mint": len(other_records),
        "diag_other_curve_first_ts_ms": first_ts(other_records),
        "diag_other_curve_first_slot": other_records[0].get("slot") if other_records else None,
        "diag_other_curve_pubkeys_for_mint": other_curves[:8],
        "diag_other_curve_pubkey_count_for_mint": len(other_curves),
        "diag_other_curve_seen_before_decision": seen_before(other_records),
        "raw_builder_bcv2_log_occurrences": raw_pubkey["raw_log_occurrences"],
        "raw_builder_bcv2_diag_occurrences": raw_pubkey["diag_raw_occurrences"],
        "first_builder_bcv2_raw_context": raw_pubkey["first_raw_context"],
    }


def rpc_get_multiple_accounts(rpc_url: str, pubkeys: list[str]) -> dict[str, Any]:
    if not pubkeys:
        return {}
    request_id = 1
    results: dict[str, Any] = {}
    for start in range(0, len(pubkeys), 100):
        chunk = pubkeys[start : start + 100]
        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "getMultipleAccounts",
            "params": [
                chunk,
                {
                    "encoding": "base64",
                    "commitment": "processed",
                },
            ],
        }
        request_id += 1
        data = json.dumps(payload).encode("utf-8")
        req = urllib_request.Request(
            rpc_url,
            data=data,
            headers={
                "Content-Type": "application/json",
                "User-Agent": "curl/8.0 ghost-p37-bcv2-reconciliation",
            },
            method="POST",
        )
        with urllib_request.urlopen(req, timeout=20) as response:
            body = json.loads(response.read().decode("utf-8"))
        if "error" in body:
            raise RuntimeError(f"RPC getMultipleAccounts failed: {body['error']}")
        values = ((body.get("result") or {}).get("value") or [])
        for pubkey, account in zip(chunk, values):
            if account is None:
                results[pubkey] = {"rpc_current_status": "missing"}
            else:
                data_field = account.get("data")
                data_len = None
                if isinstance(data_field, list) and data_field:
                    try:
                        data_len = len(base64.b64decode(data_field[0], validate=False))
                    except Exception:
                        data_len = len(data_field[0])
                results[pubkey] = {
                    "rpc_current_status": "present",
                    "rpc_current_owner": account.get("owner"),
                    "rpc_current_lamports": account.get("lamports"),
                    "rpc_current_executable": account.get("executable"),
                    "rpc_current_rent_epoch": account.get("rentEpoch"),
                    "rpc_current_data_len": data_len,
                }
    return results


def classify_case(case: dict[str, Any], diag: dict[str, Any], mfs: dict[str, Any], rpc: dict[str, Any]) -> tuple[str, list[str]]:
    reasons: list[str] = []
    if not diag.get("diag_seen_exact_pubkey"):
        reasons.append("builder_bcv2_pubkey_not_seen_in_diag")
    if diag.get("diag_seen_other_curve_pubkey_for_mint"):
        reasons.append("diag_seen_other_curve_pubkey_for_same_mint")
    if not mfs.get("mfs_contains_bonding_curve_v2_key"):
        reasons.append("mfs_missing_bonding_curve_v2_field")
    if not mfs.get("mfs_contains_builder_bcv2_pubkey"):
        reasons.append("mfs_missing_builder_bcv2_pubkey")
    if rpc.get("rpc_current_status") == "missing":
        reasons.append("rpc_current_missing")
    elif rpc.get("rpc_current_status") == "present":
        reasons.append("rpc_current_present")

    if diag.get("diag_seen_exact_before_decision") and rpc.get("rpc_current_status") == "missing":
        return "diag_seen_but_rpc_missing", reasons
    if diag.get("diag_seen_exact_before_decision"):
        return "diag_seen_exact_pubkey", reasons
    if diag.get("diag_seen_other_curve_pubkey_for_mint"):
        return "builder_pubkey_not_seen_in_diag", reasons
    if mfs.get("mfs_present") and not mfs.get("mfs_contains_bonding_curve_v2_key"):
        return "mfs_missing_execution_account_identity", reasons
    return "unknown", reasons or ["no_reconciliation_evidence"]


def reconcile_cases(
    cases: list[dict[str, Any]],
    by_ab: dict[str, tuple[Path, int, dict[str, Any]]],
    by_mint: dict[str, list[tuple[Path, int, dict[str, Any]]]],
    diag_index: dict[str, Any],
    log_paths: list[Path],
    rpc_results: dict[str, Any],
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for case in cases:
        decision_path, decision_index, decision_row, decision_lookup_status = choose_decision_row(case, by_ab, by_mint)
        mfs = account_fields_from_decision(decision_row, case.get("builder_bonding_curve_v2_pubkey"))
        diag = analyze_diag_for_case(case, diag_index, log_paths)
        rpc = rpc_results.get(case.get("builder_bonding_curve_v2_pubkey"), {})
        classification, reasons = classify_case(case, diag, mfs, rpc)
        raw_row = case.pop("raw_row", {})
        out.append(
            {
                **case,
                "instruction_index": raw_row.get("simulation_error_instruction_index")
                or (
                    (raw_row.get("simulation_error_account_candidates_narrowed") or [{}])[0].get(
                        "instruction_index"
                    )
                    if isinstance(raw_row.get("simulation_error_account_candidates_narrowed"), list)
                    else None
                ),
                "account_index": raw_row.get("simulation_error_account_index")
                or (
                    (raw_row.get("simulation_error_account_candidates_narrowed") or [{}])[0].get(
                        "account_index"
                    )
                    if isinstance(raw_row.get("simulation_error_account_candidates_narrowed"), list)
                    else None
                ),
                "builder_source": raw_row.get("simulation_error_account_source")
                or (
                    (raw_row.get("simulation_error_account_candidates_narrowed") or [{}])[0].get("source")
                    if isinstance(raw_row.get("simulation_error_account_candidates_narrowed"), list)
                    else None
                ),
                "decision_log_path": str(decision_path) if decision_path else None,
                "decision_row_index": decision_index,
                "decision_lookup_status": decision_lookup_status,
                "mfs": mfs,
                "diag": diag,
                "rpc_current": rpc,
                "classification": classification,
                "classification_reasons": reasons,
            }
        )
    return out


def summarize(rows: list[dict[str, Any]], diag_index: dict[str, Any], rpc_checked: bool) -> dict[str, Any]:
    plane_counts = Counter(row["plane"] for row in rows)
    class_counts = Counter(row["classification"] for row in rows)
    reason_counts = Counter(reason for row in rows for reason in row.get("classification_reasons", []))
    active_rows = [row for row in rows if row["plane"] == "active_shadow"]
    probe_rows = [row for row in rows if row["plane"] == "probe"]
    return {
        "status": "not_ready_diagnosed" if rows else "no_bonding_curve_v2_not_ready_rows",
        "reconciled_rows": len(rows),
        "active_shadow_bcv2_not_ready_rows": len(active_rows),
        "probe_bcv2_not_ready_rows": len(probe_rows),
        "classifications": dict(sorted(class_counts.items())),
        "classification_reasons": dict(sorted(reason_counts.items())),
        "plane_counts": dict(sorted(plane_counts.items())),
        "diag_account_update_total": diag_index.get("diag_account_update_total", 0),
        "diag_seen_exact_pubkey_rows": sum(1 for row in rows if row["diag"]["diag_seen_exact_pubkey"]),
        "diag_seen_other_curve_pubkey_rows": sum(
            1 for row in rows if row["diag"]["diag_seen_other_curve_pubkey_for_mint"]
        ),
        "mfs_contains_bonding_curve_v2_key_rows": sum(
            1 for row in rows if row["mfs"].get("mfs_contains_bonding_curve_v2_key")
        ),
        "mfs_contains_builder_bcv2_pubkey_rows": sum(
            1 for row in rows if row["mfs"].get("mfs_contains_builder_bcv2_pubkey")
        ),
        "rpc_current_checked": rpc_checked,
        "rpc_current_status_counts": dict(
            sorted(Counter((row.get("rpc_current") or {}).get("rpc_current_status", "not_checked") for row in rows).items())
        ),
        "rpc_current_error_counts": dict(
            sorted(
                Counter(
                    (row.get("rpc_current") or {}).get("rpc_current_error")
                    for row in rows
                    if (row.get("rpc_current") or {}).get("rpc_current_error")
                ).items()
            )
        ),
        "recommended_next_stage": (
            "bonding_curve_v2_route_source_or_account_coverage_repair"
            if rows
            else "return_to_r16_policy_diagnostics"
        ),
    }


def render_markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    lines = [
        "# RAPORT P3.7-L1R10 BondingCurveV2 Readiness / Route Source Reconciliation",
        "",
        f"Date: {payload['date']}",
        f"Namespace: `{payload['namespace']}`",
        "",
        "## Status",
        "",
        "```text",
        f"P3.7-L1R10 status = {summary['status']}",
        f"active_shadow_bcv2_not_ready_rows = {summary['active_shadow_bcv2_not_ready_rows']}",
        f"probe_bcv2_not_ready_rows = {summary['probe_bcv2_not_ready_rows']}",
        f"classifications = {summary['classifications']}",
        f"recommended_next_stage = {summary['recommended_next_stage']}",
        "L2 ablation / collection / Phase B / P2 / live / tuning = HOLD / NO-GO",
        "```",
        "",
        "## Evidence Summary",
        "",
        "```text",
        f"diag_account_update_total = {summary['diag_account_update_total']}",
        f"diag_seen_exact_pubkey_rows = {summary['diag_seen_exact_pubkey_rows']}",
        f"diag_seen_other_curve_pubkey_rows = {summary['diag_seen_other_curve_pubkey_rows']}",
        f"mfs_contains_bonding_curve_v2_key_rows = {summary['mfs_contains_bonding_curve_v2_key_rows']}",
        f"mfs_contains_builder_bcv2_pubkey_rows = {summary['mfs_contains_builder_bcv2_pubkey_rows']}",
        f"rpc_current_checked = {summary['rpc_current_checked']}",
        f"rpc_current_status_counts = {summary['rpc_current_status_counts']}",
        f"rpc_current_error_counts = {summary['rpc_current_error_counts']}",
        f"classification_reasons = {summary['classification_reasons']}",
        "```",
        "",
        "## Interpretation",
        "",
    ]
    if summary["reconciled_rows"] == 0:
        lines.append("No `bonding_curve_v2` not-ready rows were found in the analyzed artifacts.")
    else:
        lines.extend(
            [
                "The builder-provided `bonding_curve_v2` identities are present in active/probe",
                "failure artifacts, but the analyzed DIAG relay did not observe the exact",
                "`bonding_curve_v2` pubkey for any reconciled row.",
                "",
                "When DIAG evidence exists for the same mint, it is for a different",
                "`bonding_curve` pubkey. That points at a route/account-source coverage issue:",
                "local curve updates prove the legacy bonding curve path, not the exact",
                "`bonding_curve_v2` account that the builder inserts into simulation metas.",
                "",
                "This is not a threshold or L2 policy problem. The next repair must decide",
                "whether `bonding_curve_v2` should be materialized/covered explicitly, whether",
                "the route builder is selecting the wrong source, or whether this route should",
                "be sampled only after `bonding_curve_v2` simulation-load readiness is proven.",
            ]
        )
    lines.extend(["", "## Sample Rows", ""])
    for row in payload["rows"][:10]:
        lines.extend(
            [
                "```text",
                f"plane = {row['plane']}",
                f"artifact_sources = {row['artifact_sources']}",
                f"ab_record_id = {row.get('ab_record_id')}",
                f"base_mint = {row.get('base_mint')}",
                f"pool_id = {row.get('pool_id')}",
                f"builder_bonding_curve_v2_pubkey = {row.get('builder_bonding_curve_v2_pubkey')}",
                f"classification = {row.get('classification')}",
                f"classification_reasons = {row.get('classification_reasons')}",
                f"diag_seen_exact_pubkey = {row['diag']['diag_seen_exact_pubkey']}",
                f"diag_seen_other_curve_pubkey_for_mint = {row['diag']['diag_seen_other_curve_pubkey_for_mint']}",
                f"diag_other_curve_pubkeys_for_mint = {row['diag']['diag_other_curve_pubkeys_for_mint']}",
                f"mfs_contains_bonding_curve_v2_key = {row['mfs'].get('mfs_contains_bonding_curve_v2_key')}",
                f"mfs_contains_builder_bcv2_pubkey = {row['mfs'].get('mfs_contains_builder_bcv2_pubkey')}",
                f"rpc_current_status = {(row.get('rpc_current') or {}).get('rpc_current_status', 'not_checked')}",
                "```",
                "",
            ]
        )
    return "\n".join(lines).rstrip() + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--output-md", type=Path)
    parser.add_argument("--json", action="store_true", help="Print JSON payload to stdout")
    parser.add_argument(
        "--rpc-check-current",
        action="store_true",
        help="Check current RPC visibility for builder bonding_curve_v2 pubkeys. Not decision-time evidence.",
    )
    parser.add_argument("--rpc-url", help="RPC endpoint for --rpc-check-current")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    config_path = args.config.resolve()
    config = load_toml(config_path)
    namespace = resolve_namespace(config, config_path)
    paths = resolve_paths(config_path, config, namespace)
    log_paths = discover_log_paths(paths["rollout_root"])
    decisions = flatten_decision_logs(paths["decision_root"])
    by_ab, by_mint = build_decision_indexes(decisions)
    diag_index = build_diag_index(log_paths)
    cases = collect_cases(paths)
    pubkeys = sorted({case["builder_bonding_curve_v2_pubkey"] for case in cases})
    rpc_results: dict[str, Any] = {}
    rpc_checked = False
    if args.rpc_check_current and pubkeys:
        rpc_url = args.rpc_url
        if not rpc_url:
            rpc_url = (((config.get("trigger") or {}).get("shadow_run") or {}).get("shadow_rpc_url"))
        if not rpc_url:
            rpc_url = ((config.get("seer") or {}).get("rpc_endpoint"))
        if not isinstance(rpc_url, str) or not rpc_url:
            raise SystemExit("--rpc-check-current requested but no RPC URL found")
        rpc_checked = True
        try:
            rpc_results = rpc_get_multiple_accounts(rpc_url, pubkeys)
        except Exception as exc:  # pragma: no cover - network/environment dependent
            rpc_results = {
                pubkey: {
                    "rpc_current_status": "error",
                    "rpc_current_error": str(exc),
                }
                for pubkey in pubkeys
            }
    rows = reconcile_cases(cases, by_ab, by_mint, diag_index, log_paths, rpc_results)
    payload = {
        "schema_version": SCHEMA_VERSION,
        "date": datetime.now(timezone.utc).isoformat(),
        "config_path": str(config_path),
        "namespace": namespace,
        "paths": {key: str(value) for key, value in paths.items()},
        "log_paths_scanned": [str(path) for path in log_paths],
        "decision_rows_loaded": len(decisions),
        "summary": summarize(rows, diag_index, rpc_checked),
        "rows": rows,
    }
    if args.output_json:
        write_json(args.output_json, payload)
    if args.output_md:
        args.output_md.parent.mkdir(parents=True, exist_ok=True)
        args.output_md.write_text(render_markdown(payload), encoding="utf-8")
    if args.json or not args.output_json:
        print(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
