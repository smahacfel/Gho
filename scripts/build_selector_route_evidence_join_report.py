#!/usr/bin/env python3
"""Join r12 Program Stream route evidence with raw gRPC tx evidence.

This is an offline, shadow/simcov evidence-mode diagnostic.  It does not
change Gatekeeper, runtime route resolution, active execution, send path, or
shadow simulation behavior.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import audit_selector_buy_simulation_coverage as simcov
import selector_pipeline_common as common


ARTIFACT = "route_evidence_simcov_impact_v1"
JOINED_ARTIFACT = "joined_route_manifest_evidence_v1"
OUTLIER_ARTIFACT = "route_evidence_join_outliers_v1"
BLOCKER_TABLE_ARTIFACT = "route_evidence_buy_blocker_table_v1"
PROGRAM_STREAM_JOIN_KEY_AUDIT_ARTIFACT = "program_stream_join_key_audit_v1"
PUMPFUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
TAIL_CLASSES = {"tail_len_2", "tail_len_3", "tail_len_4", "tail_len_9"}
JOIN_TAXONOMY = (
    "tail_len_2",
    "tail_len_3",
    "tail_len_4",
    "tail_len_9",
    "tail_len_other",
    "raw_gRPC_no_match",
    "raw_gRPC_tail_mismatch",
    "raw_gRPC_account_order_mismatch",
    "named_account_role_conflict",
    "missing_signature",
    "missing_slot",
    "missing_tx_index",
    "missing_ix_index",
    "join_timeout",
    "raw_gRPC_missing_account_keys",
    "raw_gRPC_missing_compiled_instruction_account_indices",
    "resolver_validation_missing",
    "resolver_validation_failed",
    "route_kind_mismatch",
)
ROUTE_MATERIALIZATION_CLASSES = {
    "ROUTE_INCOMPLETE_TELEMETRY_ONLY",
    "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING",
    "ROUTE_INCOMPLETE_BCV2_MISSING",
    "ROUTE_INCOMPLETE_CREATOR_OR_ACCOUNT_ROLE",
}
STATE_READINESS_CLASSES = {"ROUTE_INCOMPLETE_STATE_NOT_READY"}
PROGRAM_STREAM_JOIN_KEY_GROUPS = {
    "signature_like": {
        "signature",
        "transaction_signature",
        "tx_signature",
        "txSignature",
        "transactionSignature",
    },
    "slot_like": {"slot", "block_slot", "blockSlot"},
    "tx_index_like": {"transaction_index", "transactionIndex", "tx_index", "txIndex"},
    "ix_index_like": {
        "instruction_index",
        "instructionIndex",
        "ix_index",
        "ixIndex",
        "outer_instruction_index",
        "outerInstructionIndex",
    },
    "generic_index_ambiguous": {"index"},
}


def read_jsonl(path: Path | None) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def iter_nested_key_hits(value: Any, wanted: set[str], path: str = "$") -> Iterable[tuple[str, Any]]:
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = f"{path}.{key}"
            if key in wanted and child not in (None, ""):
                yield child_path, child
            yield from iter_nested_key_hits(child, wanted, child_path)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from iter_nested_key_hits(child, wanted, f"{path}[{index}]")


def default_program_stream_raw_paths(root: Path, scope: str) -> list[Path]:
    base = root / "logs" / "nln_capture" / scope
    return [
        base / "nln_pumpfun_buy_raw_v1.jsonl",
        base / "nln_pumpfun_buy_exact_sol_in_raw_v1.jsonl",
    ]


def audit_program_stream_join_keys(root: Path, scope: str) -> dict[str, Any]:
    paths = default_program_stream_raw_paths(root, scope)
    group_counts: dict[str, int] = {group: 0 for group in PROGRAM_STREAM_JOIN_KEY_GROUPS}
    group_sample_paths: dict[str, list[dict[str, Any]]] = {
        group: [] for group in PROGRAM_STREAM_JOIN_KEY_GROUPS
    }
    rows_read = 0
    files_read: list[str] = []
    wanted = set().union(*PROGRAM_STREAM_JOIN_KEY_GROUPS.values())
    for path in paths:
        if not path.exists():
            continue
        files_read.append(str(path))
        for line_no, row in enumerate(read_jsonl(path), 1):
            rows_read += 1
            for key_path, value in iter_nested_key_hits(row, wanted):
                matched_groups = [
                    group
                    for group, names in PROGRAM_STREAM_JOIN_KEY_GROUPS.items()
                    if key_path.rsplit(".", 1)[-1] in names
                ]
                for group in matched_groups:
                    group_counts[group] += 1
                    if len(group_sample_paths[group]) < 10:
                        group_sample_paths[group].append(
                            {
                                "path": str(path),
                                "line": line_no,
                                "json_path": key_path,
                                "value": value,
                            }
                        )
    strong_absent = (
        group_counts["signature_like"] == 0
        and group_counts["slot_like"] == 0
        and group_counts["tx_index_like"] == 0
        and group_counts["ix_index_like"] == 0
    )
    return {
        "artifact": PROGRAM_STREAM_JOIN_KEY_AUDIT_ARTIFACT,
        "scope": scope,
        "files_read": files_read,
        "rows_read": rows_read,
        "field_group_counts": group_counts,
        "field_group_sample_paths": group_sample_paths,
        "generic_index_is_ambiguous_not_join_key": True,
        "PROGRAM_STREAM_JOIN_KEY_ABSENT_CONFIRMED": strong_absent,
    }


def value_from_path(row: dict[str, Any], *paths: tuple[str, ...]) -> Any:
    for path in paths:
        value: Any = row
        for key in path:
            if not isinstance(value, dict):
                value = None
                break
            value = value.get(key)
        if value not in (None, "", []):
            return value
    return None


def str_or_none(value: Any) -> str | None:
    return common.str_or_none(value)


def int_or_none(value: Any) -> int | None:
    return common.int_or_none(value)


def list_pubkeys(value: Any) -> list[str]:
    values: list[str] = []
    if not isinstance(value, list):
        return values
    for item in value:
        if isinstance(item, str) and item:
            values.append(item)
        elif isinstance(item, dict):
            pubkey = item.get("pubkey") or item.get("address") or item.get("key")
            if isinstance(pubkey, str) and pubkey:
                values.append(pubkey)
    return values


def named_account_map(row: dict[str, Any]) -> dict[str, str]:
    mapping: dict[str, str] = {}
    raw = row.get("named_accounts") or row.get("accounts")
    if isinstance(raw, list):
        for item in raw:
            if not isinstance(item, dict):
                continue
            role = item.get("role") or item.get("name") or item.get("account_role")
            pubkey = item.get("pubkey") or item.get("address") or item.get("key")
            if isinstance(role, str) and isinstance(pubkey, str) and role and pubkey:
                mapping[role] = pubkey
    elif isinstance(raw, dict):
        for role, value in raw.items():
            if isinstance(value, str) and value:
                mapping[str(role)] = value
            elif isinstance(value, dict):
                pubkey = value.get("pubkey") or value.get("address") or value.get("key")
                if isinstance(pubkey, str) and pubkey:
                    mapping[str(role)] = pubkey
    return mapping


def named_account_order(row: dict[str, Any]) -> list[tuple[str, str]]:
    ordered: list[tuple[str, str]] = []
    raw = row.get("named_accounts")
    if not isinstance(raw, list):
        mapping = named_account_map(row)
        return [(role, mapping[role]) for role in sorted(mapping)]
    for item in raw:
        if not isinstance(item, dict):
            continue
        role = item.get("role") or item.get("name") or item.get("account_role")
        pubkey = item.get("pubkey") or item.get("address") or item.get("key")
        if isinstance(role, str) and isinstance(pubkey, str) and role and pubkey:
            ordered.append((role, pubkey))
    return ordered


def route_kind_from_topic(topic: str | None) -> str | None:
    if topic == "solana.pump_fun.buy":
        return "legacy_buy"
    if topic == "solana.pump_fun.buy_exact_sol_in":
        return "routed_exact_sol_in"
    return None


def route_kind(row: dict[str, Any]) -> str | None:
    value = str_or_none(
        value_from_path(
            row,
            ("route_kind",),
            ("buy_variant",),
            ("selected_route_kind",),
            ("payload", "route_kind"),
            ("payload", "buy_variant"),
            ("kind", "payload", "route_kind"),
            ("kind", "payload", "buy_variant"),
        )
    )
    if value:
        return value
    return route_kind_from_topic(str_or_none(row.get("topic")))


def tx_index(row: dict[str, Any]) -> int | None:
    return int_or_none(
        value_from_path(
            row,
            ("tx_index",),
            ("transaction_index",),
            ("payload", "tx_index"),
            ("kind", "payload", "tx_index"),
            ("kind", "payload", "transaction_index"),
        )
    )


def ix_index(row: dict[str, Any]) -> int | None:
    return int_or_none(
        value_from_path(
            row,
            ("ix_index",),
            ("instruction_index",),
            ("outer_instruction_index",),
            ("payload", "ix_index"),
            ("payload", "instruction_index"),
            ("kind", "payload", "outer_instruction_index"),
            ("kind", "payload", "instruction_index"),
        )
    )


def signature(row: dict[str, Any]) -> str | None:
    return str_or_none(
        value_from_path(
            row,
            ("signature",),
            ("payload", "signature"),
            ("kind", "payload", "signature"),
        )
    )


def slot(row: dict[str, Any]) -> int | None:
    return int_or_none(
        value_from_path(
            row,
            ("slot",),
            ("event_slot",),
            ("partition",),
            ("payload", "slot"),
            ("payload", "event_slot"),
            ("kind", "payload", "slot"),
            ("kind", "payload", "event_slot"),
            ("envelope", "slot"),
        )
    )


def join_key(row: dict[str, Any]) -> tuple[str, int, int | None, int | None, str] | None:
    sig = signature(row)
    sl = slot(row)
    tx = tx_index(row)
    ix = ix_index(row)
    rk = route_kind(row)
    if not sig or sl is None or rk is None or (tx is None and ix is None):
        return None
    return sig, sl, tx, ix, rk


def missing_join_key_reasons(row: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    if not signature(row):
        reasons.append("missing_signature")
    if slot(row) is None:
        reasons.append("missing_slot")
    tx = tx_index(row)
    ix = ix_index(row)
    if tx is None and ix is None:
        reasons.append("missing_tx_index")
        reasons.append("missing_ix_index")
    if not route_kind(row):
        reasons.append("route_kind_mismatch")
    return reasons


def tail_class(count: int | None) -> str:
    if count == 2:
        return "tail_len_2"
    if count == 3:
        return "tail_len_3"
    if count == 4:
        return "tail_len_4"
    if count == 9:
        return "tail_len_9"
    return "tail_len_other"


def remaining_accounts(row: dict[str, Any]) -> list[str]:
    return list_pubkeys(row.get("remaining_accounts"))


def remaining_count(row: dict[str, Any]) -> int:
    explicit = int_or_none(row.get("remaining_accounts_count") or row.get("remaining_account_count"))
    if explicit is not None:
        return explicit
    return len(remaining_accounts(row))


def account_keys(row: dict[str, Any]) -> list[str]:
    return list_pubkeys(
        row.get("account_keys")
        or row.get("full_account_keys")
        or row.get("message_account_keys")
        or value_from_path(row, ("payload", "account_keys"), ("kind", "payload", "account_keys"))
    )


def compiled_indices(row: dict[str, Any]) -> list[int]:
    raw = (
        row.get("compiled_instruction_account_indices")
        or row.get("instruction_account_indices")
        or row.get("account_indices")
        or value_from_path(
            row,
            ("payload", "compiled_instruction_account_indices"),
            ("kind", "payload", "compiled_instruction_account_indices"),
        )
    )
    if not isinstance(raw, list):
        return []
    values: list[int] = []
    for item in raw:
        value = int_or_none(item)
        if value is not None:
            values.append(value)
    return values


def compiled_account_order(row: dict[str, Any]) -> list[str]:
    keys = account_keys(row)
    indices = compiled_indices(row)
    ordered: list[str] = []
    for index in indices:
        if 0 <= index < len(keys):
            ordered.append(keys[index])
    return ordered


def raw_remaining_accounts(row: dict[str, Any]) -> list[str]:
    return list_pubkeys(
        row.get("remaining_accounts")
        or row.get("raw_remaining_accounts")
        or value_from_path(row, ("payload", "remaining_accounts"), ("kind", "payload", "remaining_accounts"))
    )


def field_diff(
    field: str,
    program_value: Any,
    raw_value: Any,
    resolver_value: Any = None,
    *,
    reason: str,
) -> dict[str, Any]:
    return {
        "field": field,
        "program_stream_value": program_value,
        "raw_gRPC_value": raw_value,
        "resolver_value": resolver_value,
        "conflict_reason": reason,
    }


def expected_account_order(candidate: dict[str, Any]) -> list[str]:
    # Program id is not an instruction account meta.
    ordered = [pubkey for role, pubkey in named_account_order(candidate) if role != "program"]
    ordered.extend(remaining_accounts(candidate))
    return ordered


def account_order_matches(expected: list[str], observed: list[str]) -> bool:
    if not expected or not observed:
        return False
    cursor = 0
    for pubkey in observed:
        if cursor < len(expected) and pubkey == expected[cursor]:
            cursor += 1
    return cursor == len(expected)


def raw_records_from_event(row: dict[str, Any]) -> dict[str, Any] | None:
    kind_type = value_from_path(row, ("kind", "type"), ("type",), ("artifact",))
    if kind_type in {
        "raw_grpc_transaction_evidence_v1",
        "RawTransactionEvidence",
        "route_manifest_raw_grpc_transaction_evidence_v1",
        "raw_pumpfun_instruction_evidence_v1",
    }:
        return row
    if kind_type == "PoolTransaction":
        payload = value_from_path(row, ("kind", "payload"))
        if not isinstance(payload, dict):
            return None
        return {
            "source_artifact": "PoolTransaction",
            "signature": payload.get("signature"),
            "slot": payload.get("slot") or payload.get("event_slot"),
            "ix_index": payload.get("outer_instruction_index"),
            "tx_index": payload.get("tx_index"),
            "route_kind": payload.get("route_kind") or payload.get("buy_variant"),
            "pool_id": payload.get("pool_id") or payload.get("pool_amm_id"),
            "base_mint": payload.get("base_mint") or payload.get("mint_id"),
            "side": payload.get("side"),
            "is_buy": payload.get("is_buy"),
            "source": payload.get("source"),
        }
    return None


def iter_raw_evidence_paths(root: Path, scope: str, explicit_globs: list[str]) -> Iterable[Path]:
    if explicit_globs:
        for pattern in explicit_globs:
            yield from sorted(root.glob(pattern))
        return
    for path in sorted((root / "datasets" / "events" / scope).glob("*.jsonl")):
        yield path
    for path in sorted((root / "logs" / "rollout" / scope).glob("**/*raw*transaction*.jsonl")):
        yield path


def load_raw_index(root: Path, scope: str, explicit_globs: list[str]) -> tuple[dict[tuple[str, int, int | None, int | None, str], list[dict[str, Any]]], list[str]]:
    index: dict[tuple[str, int, int | None, int | None, str], list[dict[str, Any]]] = defaultdict(list)
    paths: list[str] = []
    for path in iter_raw_evidence_paths(root, scope, explicit_globs):
        if not path.exists():
            continue
        paths.append(str(path))
        for line_no, row in enumerate(read_jsonl(path), 1):
            raw = raw_records_from_event(row)
            if not raw:
                continue
            raw = dict(raw)
            raw["_source_path"] = str(path)
            raw["_source_line"] = line_no
            key = join_key(raw)
            if key:
                index[key].append(raw)
    return index, paths


def validate_join(candidate: dict[str, Any], raw_rows: list[dict[str, Any]]) -> dict[str, Any]:
    diffs: list[dict[str, Any]] = []
    taxonomy: list[str] = [tail_class(remaining_count(candidate))]
    if not raw_rows:
        taxonomy.append("raw_gRPC_no_match")
        return {
            "status": "pending_join",
            "raw_gRPC_match_status": "raw_gRPC_no_match",
            "taxonomy": taxonomy,
            "conflicts": diffs,
            "raw": None,
        }
    if len(raw_rows) > 1:
        diffs.append(field_diff("join_key", "single_candidate", len(raw_rows), reason="multiple_raw_matches"))
        return {
            "status": "conflicted",
            "raw_gRPC_match_status": "raw_gRPC_multiple_matches",
            "taxonomy": taxonomy + ["named_account_role_conflict"],
            "conflicts": diffs,
            "raw": raw_rows[0],
        }
    raw = raw_rows[0]
    raw_keys = account_keys(raw)
    raw_indices = compiled_indices(raw)
    raw_order = compiled_account_order(raw)
    raw_named = named_account_map(raw)
    cand_named = named_account_map(candidate)
    cand_tail = remaining_accounts(candidate)
    raw_tail = raw_remaining_accounts(raw)
    if not raw_keys:
        taxonomy.append("raw_gRPC_missing_account_keys")
    if not raw_indices:
        taxonomy.append("raw_gRPC_missing_compiled_instruction_account_indices")
    for role, cand_value in cand_named.items():
        if role == "program":
            continue
        raw_value = raw_named.get(role)
        if raw_value is not None and raw_value != cand_value:
            taxonomy.append("named_account_role_conflict")
            diffs.append(field_diff(role, cand_value, raw_value, reason="named_account_role_conflict"))
    if raw_tail and raw_tail != cand_tail:
        taxonomy.append("raw_gRPC_tail_mismatch")
        diffs.append(
            field_diff(
                "remaining_accounts",
                cand_tail,
                raw_tail,
                reason="raw_gRPC_tail_mismatch",
            )
        )
    expected_order = expected_account_order(candidate)
    if raw_order and not account_order_matches(expected_order, raw_order):
        taxonomy.append("raw_gRPC_account_order_mismatch")
        diffs.append(
            field_diff(
                "account_order",
                expected_order,
                raw_order,
                reason="raw_gRPC_account_order_mismatch",
            )
        )
    resolver_status = raw.get("resolver_validation_status") or raw.get("pda_role_validation_status")
    if resolver_status in (None, ""):
        taxonomy.append("resolver_validation_missing")
    elif str(resolver_status) != "PASS":
        taxonomy.append("resolver_validation_failed")
        diffs.append(
            field_diff(
                "resolver_validation_status",
                "PASS",
                resolver_status,
                reason="resolver_validation_failed",
            )
        )
    if diffs:
        return {
            "status": "conflicted",
            "raw_gRPC_match_status": "raw_gRPC_conflicted",
            "taxonomy": sorted(set(taxonomy)),
            "conflicts": diffs,
            "raw": raw,
        }
    missing_requirements = [
        item
        for item in taxonomy
        if item
        not in {
            "tail_len_2",
            "tail_len_3",
            "tail_len_4",
            "tail_len_9",
            "tail_len_other",
        }
    ]
    if remaining_count(candidate) != 2:
        return {
            "status": "incomplete",
            "raw_gRPC_match_status": "raw_gRPC_match_tail_not_legacy_len2",
            "taxonomy": sorted(set(taxonomy)),
            "conflicts": diffs,
            "raw": raw,
        }
    if missing_requirements:
        return {
            "status": "incomplete",
            "raw_gRPC_match_status": "raw_gRPC_match_incomplete",
            "taxonomy": sorted(set(taxonomy)),
            "conflicts": diffs,
            "raw": raw,
        }
    return {
        "status": "complete",
        "raw_gRPC_match_status": "raw_gRPC_match_complete",
        "taxonomy": sorted(set(taxonomy)),
        "conflicts": diffs,
        "raw": raw,
    }


def joined_row(candidate: dict[str, Any], raw_index: dict[tuple[str, int, int | None, int | None, str], list[dict[str, Any]]]) -> dict[str, Any]:
    reasons = missing_join_key_reasons(candidate)
    key = join_key(candidate)
    taxonomy = [tail_class(remaining_count(candidate))]
    if reasons:
        taxonomy.extend(reasons)
        status = "pending_join"
        raw_status = "missing_join_key"
        result = {"raw": None, "conflicts": [], "taxonomy": taxonomy}
    else:
        result = validate_join(candidate, raw_index.get(key, []))
        status = str(result["status"])
        raw_status = str(result["raw_gRPC_match_status"])
        taxonomy = list(result["taxonomy"])
    raw = result.get("raw")
    conflicts = result.get("conflicts") or []
    return {
        "artifact": "joined_route_manifest_evidence_v1",
        "schema_version": 1,
        "status": status,
        "manifest_status": status,
        "parse_status": candidate.get("parse_status"),
        "route_kind": route_kind(candidate),
        "topic": candidate.get("topic"),
        "signature": signature(candidate),
        "slot": slot(candidate),
        "tx_index": tx_index(candidate),
        "ix_index": ix_index(candidate),
        "account_manifest_hash": candidate.get("account_manifest_hash"),
        "instruction_evidence_hash": candidate.get("instruction_evidence_hash"),
        "remaining_accounts_count": remaining_count(candidate),
        "has_legacy_tail": candidate.get("has_legacy_tail") is True,
        "can_unlock_execution": False,
        "program_stream_can_unlock_execution": candidate.get("can_unlock_execution") is True,
        "raw_gRPC_match_status": raw_status,
        "raw_gRPC_source_path": raw.get("_source_path") if isinstance(raw, dict) else None,
        "raw_gRPC_source_line": raw.get("_source_line") if isinstance(raw, dict) else None,
        "taxonomy": sorted(set(taxonomy)),
        "conflicts": conflicts,
        "conflict_field": conflicts[0].get("field") if conflicts else None,
        "program_stream_value": conflicts[0].get("program_stream_value") if conflicts else None,
        "raw_gRPC_value": conflicts[0].get("raw_gRPC_value") if conflicts else None,
        "resolver_value": conflicts[0].get("resolver_value") if conflicts else None,
        "source": "nln_program_stream_joined_with_raw_grpc",
        "complete_can_unlock_execution": False,
    }


def default_candidate_path(root: Path, scope: str) -> Path:
    return root / "logs" / "nln_capture" / scope / "route_manifest_evidence_candidates_v1.jsonl"


def load_candidates(path: Path) -> list[dict[str, Any]]:
    return read_jsonl(path)


def build_joined_evidence(args: argparse.Namespace) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    root = args.root.resolve()
    candidates_path = args.candidates or default_candidate_path(root, args.scope)
    candidates = load_candidates(candidates_path)
    raw_index, raw_paths = load_raw_index(root, args.scope, args.raw_transaction_evidence_glob)
    joined = [joined_row(candidate, raw_index) for candidate in candidates]
    status_counts = Counter(str(row["status"]) for row in joined)
    taxonomy_counts: Counter[str] = Counter()
    tail_counts: Counter[str] = Counter()
    for row in joined:
        tail_counts[tail_class(int_or_none(row.get("remaining_accounts_count")))] += 1
        for item in row.get("taxonomy") or []:
            taxonomy_counts[str(item)] += 1
    can_unlock_true = sum(1 for row in joined if row.get("program_stream_can_unlock_execution") is True or row.get("can_unlock_execution") is True)
    summary = {
        "candidate_path": str(candidates_path),
        "raw_transaction_evidence_paths": raw_paths,
        "candidate_rows": len(candidates),
        "raw_join_key_rows": sum(len(rows) for rows in raw_index.values()),
        "status_counts": common.counter_dict(status_counts),
        "tail_taxonomy_counts": common.counter_dict(tail_counts),
        "taxonomy_counts": common.counter_dict(taxonomy_counts),
        "complete_rows": status_counts.get("complete", 0),
        "can_unlock_execution_true_rows": can_unlock_true,
    }
    return joined, summary


def exact_rate(numerator: int, denominator: int) -> dict[str, Any]:
    pct = (numerator / denominator * 100.0) if denominator else 0.0
    return {
        "numerator": numerator,
        "denominator": denominator,
        "rate": numerator / denominator if denominator else 0.0,
        "percent": pct,
        "display": f"{numerator} / {denominator} = {pct:.2f}%",
    }


def pct_from_rate(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        raw = float(value)
        return raw * 100.0 if raw <= 1.0 else raw
    return None


def scan_pooltransaction_raw_fields(root: Path, scope: str) -> dict[str, Any]:
    event_dir = root / "datasets" / "events" / scope
    rows_checked = 0
    pooltransaction_rows_checked = 0
    account_keys_present = 0
    compiled_indices_present = 0
    for path in sorted(event_dir.glob("*.jsonl"))[:20]:
        for row in read_jsonl(path):
            rows_checked += 1
            kind_type = value_from_path(row, ("kind", "type"), ("type",), ("event_type",))
            text_kind = str(kind_type or "")
            if text_kind != "PoolTransaction" and "PoolTransaction" not in json.dumps(row):
                if rows_checked >= 2_000:
                    break
                continue
            pooltransaction_rows_checked += 1
            payload = value_from_path(row, ("kind", "payload"), ("payload",)) or row
            if account_keys(payload):
                account_keys_present += 1
            if compiled_indices(payload):
                compiled_indices_present += 1
            if pooltransaction_rows_checked >= 250:
                break
        if rows_checked >= 2_000 or pooltransaction_rows_checked >= 250:
            break
    return {
        "rows_checked": rows_checked,
        "pooltransaction_rows_checked": pooltransaction_rows_checked,
        "pooltransaction_account_keys_rows": account_keys_present,
        "pooltransaction_compiled_instruction_indices_rows": compiled_indices_present,
        "pooltransaction_has_full_account_keys": account_keys_present > 0,
        "pooltransaction_has_compiled_instruction_indices": compiled_indices_present > 0,
    }


def raw_pumpfun_evidence_status(root: Path, scope: str) -> dict[str, Any]:
    paths = [
        root / "logs" / "nln_capture" / scope / "raw_pumpfun_instruction_evidence_v1.jsonl",
        root / "datasets" / "events" / scope / "raw_pumpfun_instruction_evidence_v1.jsonl",
    ]
    rows = 0
    account_key_rows = 0
    compiled_index_rows = 0
    existing: list[str] = []
    for path in paths:
        if not path.exists():
            continue
        existing.append(str(path))
        for row in read_jsonl(path):
            rows += 1
            if account_keys(row):
                account_key_rows += 1
            if compiled_indices(row):
                compiled_index_rows += 1
    return {
        "paths": existing,
        "rows": rows,
        "account_key_rows": account_key_rows,
        "compiled_instruction_indices_rows": compiled_index_rows,
        "raw_tx_payload_contains_account_keys": account_key_rows > 0,
        "parser_wrote_compiled_instruction_indices": compiled_index_rows > 0,
    }


def metrics_from_simcov_audit(path: Path) -> dict[str, Any] | None:
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    metrics = report.get("metrics")
    if not isinstance(metrics, dict):
        return None
    buy_rows = int_or_none(metrics.get("buy_rows")) or 0
    attempt_rate = pct_from_rate(metrics.get("simulation_attempt_coverage"))
    # shadow_dispatch_rows can include fail-closed dispatch records that never
    # reached simulateTransaction.  The simcov contract defines attempted rows
    # from simulation_attempt_coverage, not from dispatch rows.
    attempted = round((attempt_rate or 0.0) * buy_rows / 100.0)
    success = int_or_none(metrics.get("shadow_simulated_rows") or metrics.get("shadow_closed_rows"))
    if success is None:
        rate = pct_from_rate(metrics.get("simulation_success_coverage"))
        success = round((rate or 0.0) * buy_rows / 100.0)
    failed = int_or_none(metrics.get("simulation_failed_rows")) or 0
    not_exec = int_or_none(metrics.get("not_executable_route_rows")) or 0
    return {
        "scope": report.get("scope") or path.parent.name,
        "source_report": str(path),
        "definition_source": "buy_simulation_coverage_audit_v1",
        "buy_rows": buy_rows,
        "attempted_rows": attempted,
        "success_rows": success,
        "failed_rows": failed,
        "not_executable_rows": not_exec,
        "attempt_coverage": exact_rate(attempted, buy_rows),
        "success_coverage": exact_rate(success, buy_rows),
        "fail_reasons": report.get("fail_reasons") or [],
    }


def build_historical_coverage_comparison(
    root: Path,
    scope: str,
    baseline: dict[str, Any],
) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    for path in sorted((root / "reports" / "selector").glob("*/buy_simulation_coverage_audit_v1.json")):
        item = metrics_from_simcov_audit(path)
        if item:
            item["pooltransaction_raw_field_scan"] = scan_pooltransaction_raw_fields(root, item["scope"])
            item["raw_pumpfun_instruction_evidence_status"] = raw_pumpfun_evidence_status(root, item["scope"])
            rows.append(item)
    current = {
        "scope": scope,
        "source_report": str(root / "reports" / "selector" / scope / f"{ARTIFACT}.json"),
        "definition_source": "route_evidence_simcov_impact_v1.baseline",
        "buy_rows": baseline["buy_rows"],
        "attempted_rows": baseline["shadow_simulation_attempted_rows"],
        "success_rows": baseline["shadow_simulation_success_rows"],
        "failed_rows": baseline["shadow_simulation_failed_rows"],
        "not_executable_rows": baseline["not_executable_route_rows"],
        "attempt_coverage": baseline["simulation_attempt_coverage"],
        "success_coverage": baseline["simulation_success_coverage"],
        "fail_reasons": [],
        "pooltransaction_raw_field_scan": scan_pooltransaction_raw_fields(root, scope),
        "raw_pumpfun_instruction_evidence_status": raw_pumpfun_evidence_status(root, scope),
    }
    rows.append(current)
    near_93 = [
        row
        for row in rows
        if any(
            pct is not None and 92.5 <= pct <= 93.5
            for pct in (
                row["attempt_coverage"].get("percent"),
                row["success_coverage"].get("percent"),
            )
        )
    ]
    return {
        "artifact": "historical_coverage_comparison_v1",
        "scope": scope,
        "status": "LOCAL_COMPARISON_COMPLETE",
        "old_93_coverage_claim_status": (
            "CONFIRMED_IN_LOCAL_ARTIFACTS" if near_93 else "NOT_CONFIRMED_IN_LOCAL_ARTIFACTS"
        ),
        "old_93_matching_rows": near_93,
        "comparison_rows": rows,
        "interpretation": {
            "coverage_definitions_are_current_simcov_definitions": True,
            "attempt_coverage_is_attempted_rows_over_buy_rows": True,
            "success_coverage_is_success_rows_over_buy_rows": True,
            "raw_full_tx_manifest_absent_in_existing_r12": current[
                "raw_pumpfun_instruction_evidence_status"
            ]["rows"]
            == 0,
        },
    }


def build_buy_metrics(root: Path, scope: str, decision_plane: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    buy_paths = simcov.sorted_jsonl_paths(root, scope, "gatekeeper_v2_buys.jsonl")
    buy_rows = simcov.load_rows_for_plane(buy_paths, decision_plane)
    shadow_buys, _shadow_entries, shadow_lifecycle = simcov.load_shadow_rows(root, scope)
    dispatch_rows = simcov.shadow_dispatch_rows(shadow_buys, shadow_lifecycle)
    dispatch_index = simcov.index_shadow_dispatch(dispatch_rows)
    class_counts: Counter[str] = Counter()
    attempted = 0
    success = 0
    failed = 0
    not_executable = 0
    custom_2006 = 0
    custom_6002 = 0
    rows: list[dict[str, Any]] = []
    for buy in buy_rows:
        shadow = simcov.find_shadow_for_buy(dispatch_index, buy)
        row_success = simcov.simulation_success(buy, shadow)
        row_attempted = simcov.simulation_attempted(shadow)
        row_not_exec = simcov.is_not_executable(shadow)
        row_failed = simcov.is_failed(buy, shadow) and not row_success and not row_not_exec
        classification, _secondary = simcov.classify_failure(buy, shadow) if not row_success else ("SUCCESS", [])
        class_counts[classification] += 1
        text = simcov.text_blob(buy, shadow)
        if "Custom(2006)" in text:
            custom_2006 += 1
        if "Custom(6002)" in text:
            custom_6002 += 1
        attempted += int(row_attempted)
        success += int(row_success)
        failed += int(row_failed)
        not_executable += int(row_not_exec)
        rows.append(
            {
                "buy": buy,
                "shadow": shadow,
                "classification": classification,
                "simulation_attempted": row_attempted,
                "simulation_success": row_success,
                "simulation_failed": row_failed,
                "not_executable_route": row_not_exec,
                "buy_join_key": join_key(buy) or (join_key(shadow) if shadow else None),
            }
        )
    buy_count = len(buy_rows)
    route_materialization_error = sum(class_counts.get(klass, 0) for klass in ROUTE_MATERIALIZATION_CLASSES)
    state_readiness_error = sum(class_counts.get(klass, 0) for klass in STATE_READINESS_CLASSES)
    metrics = {
        "buy_rows": buy_count,
        "shadow_simulation_attempted_rows": attempted,
        "shadow_simulation_success_rows": success,
        "shadow_simulation_failed_rows": failed,
        "not_executable_route_rows": not_executable,
        "route_materialization_error_rows": route_materialization_error,
        "state_readiness_error_rows": state_readiness_error,
        "Custom(2006)": custom_2006,
        "Custom(6002)": custom_6002,
        "simulation_attempt_coverage": exact_rate(attempted, buy_count),
        "simulation_success_coverage": exact_rate(success, buy_count),
        "target_95_rows": math.ceil(buy_count * 0.95),
        "root_cause_counts": common.counter_dict(class_counts),
    }
    return metrics, rows


def build_evidence_enabled_projection(
    baseline: dict[str, Any],
    buy_rows: list[dict[str, Any]],
    joined: list[dict[str, Any]],
) -> tuple[dict[str, Any], dict[str, Any]]:
    complete_by_key: dict[tuple[str, int, int | None, int | None, str], dict[str, Any]] = {}
    for row in joined:
        if row.get("status") != "complete":
            continue
        key = join_key(row)
        if key:
            complete_by_key[key] = row
    additional_attempts = 0
    matched_not_executable = 0
    buy_join_key_missing = 0
    for row in buy_rows:
        key = row.get("buy_join_key")
        if key is None:
            buy_join_key_missing += 1
            continue
        if row["not_executable_route"] and key in complete_by_key:
            additional_attempts += 1
            matched_not_executable += 1
    projected = dict(baseline)
    projected_attempted = baseline["shadow_simulation_attempted_rows"] + additional_attempts
    projected_not_exec = max(0, baseline["not_executable_route_rows"] - matched_not_executable)
    projected["shadow_simulation_attempted_rows"] = projected_attempted
    projected["not_executable_route_rows"] = projected_not_exec
    projected["route_materialization_error_rows"] = max(
        0,
        baseline["route_materialization_error_rows"] - matched_not_executable,
    )
    projected["simulation_attempt_coverage"] = exact_rate(projected_attempted, baseline["buy_rows"])
    # Offline join cannot prove runtime simulation success.  Keep success rows
    # unchanged unless a real evidence-enabled shadow run writes success rows.
    projected["simulation_success_coverage"] = exact_rate(
        projected["shadow_simulation_success_rows"],
        baseline["buy_rows"],
    )
    projection_meta = {
        "mode": "offline_same_denominator_projection",
        "complete_joined_evidence_rows": len(complete_by_key),
        "not_executable_rows_matched_by_complete_evidence": matched_not_executable,
        "buy_join_key_missing_rows": buy_join_key_missing,
        "success_rows_not_projected_without_runtime_simulation": True,
        "active_execution_path_changed": False,
        "program_stream_candidates_can_unlock_execution": False,
    }
    return projected, projection_meta


def status_from_buy_row(row: dict[str, Any]) -> str:
    if row["simulation_success"]:
        return "simulation_success"
    if row["simulation_failed"]:
        return "simulation_failed"
    if row["not_executable_route"]:
        return "not_executable_route"
    if row["simulation_attempted"]:
        return "simulation_attempted_unresolved"
    return "not_attempted_unknown"


def first_nonempty_from_rows(*rows: dict[str, Any] | None, fields: str) -> Any:
    for row in rows:
        if not isinstance(row, dict):
            continue
        value = value_from_path(row, *(tuple(field.split(".")) for field in fields.split("|")))
        if value not in (None, "", []):
            return value
    return None


def concise_error_text(buy: dict[str, Any], shadow: dict[str, Any] | None) -> str | None:
    for field in (
        "simulation_error",
        "error_message",
        "simulation_error_account",
        "precheck_failure_reason",
        "execution_feasibility_reason",
        "route_resolution_status",
        "shadow_execution_outcome",
    ):
        value = first_nonempty_from_rows(buy, shadow, fields=field)
        if value not in (None, "", []):
            text = str(value)
            return text[:500]
    blob = simcov.text_blob(buy, shadow)
    return blob[:500] if blob else None


def exact_blocker_reason(
    row: dict[str, Any],
    joined_by_key: dict[tuple[str, int, int | None, int | None, str], dict[str, Any]],
) -> str:
    if row["simulation_success"]:
        return "simulation_success"
    if row["simulation_failed"]:
        return f"simulation_failed:{row['classification']}"
    if row["not_executable_route"]:
        key = row.get("buy_join_key")
        if key is None:
            return f"not_executable_route:{row['classification']}:buy_join_key_missing"
        joined = joined_by_key.get(key)
        if joined is None:
            return f"not_executable_route:{row['classification']}:no_complete_joined_route_evidence"
        if joined.get("status") != "complete":
            return f"not_executable_route:{row['classification']}:joined_evidence_{joined.get('status')}"
        return f"not_executable_route:{row['classification']}:complete_join_available_offline_only"
    if row["simulation_attempted"]:
        return "simulation_attempted_without_success_or_failure_terminal"
    return f"not_attempted_unknown:{row['classification']}"


def build_buy_blocker_table(
    buy_rows: list[dict[str, Any]],
    joined: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    joined_by_key = {
        key: row
        for row in joined
        if row.get("status") == "complete"
        for key in [join_key(row)]
        if key is not None
    }
    rows: list[dict[str, Any]] = []
    for row_id, row in enumerate(buy_rows, 1):
        buy = row["buy"]
        shadow = row.get("shadow")
        key = row.get("buy_join_key")
        joined_row_for_buy = joined_by_key.get(key) if key is not None else None
        classification = str(row["classification"])
        rows.append(
            {
                "row_id": row_id,
                "line": row_id,
                "mint": first_nonempty_from_rows(
                    buy,
                    shadow,
                    fields="base_mint|mint_id|mint|payload.base_mint|payload.mint_id",
                ),
                "pool_id": first_nonempty_from_rows(
                    buy,
                    shadow,
                    fields="pool_id|pool_amm_id|payload.pool_id|payload.pool_amm_id",
                ),
                "signature": signature(buy) or signature(shadow or {}),
                "slot": slot(buy) if slot(buy) is not None else slot(shadow or {}),
                "tx_index": tx_index(buy) if tx_index(buy) is not None else tx_index(shadow or {}),
                "ix_index": ix_index(buy) if ix_index(buy) is not None else ix_index(shadow or {}),
                "route_kind": route_kind(buy) or route_kind(shadow or {}),
                "baseline_status": status_from_buy_row(row),
                "baseline_error_class": classification,
                "not_executable_reason": first_nonempty_from_rows(
                    buy,
                    shadow,
                    fields=(
                        "precheck_failure_reason|execution_feasibility_reason|"
                        "route_resolution_status|selected_route_account_failure_reason"
                    ),
                ),
                "program_stream_candidate_status": (
                    "complete_join_available"
                    if joined_row_for_buy is not None
                    else ("buy_join_key_missing" if key is None else "no_complete_candidate_for_buy_key")
                ),
                "raw_tx_evidence_status": (
                    joined_row_for_buy.get("raw_gRPC_match_status")
                    if joined_row_for_buy is not None
                    else ("buy_join_key_missing" if key is None else "no_complete_raw_tx_evidence")
                ),
                "state_readiness_status": (
                    "state_readiness_error" if classification in STATE_READINESS_CLASSES else "not_state_readiness_error"
                ),
                "simulation_status": status_from_buy_row(row),
                "simulation_error": concise_error_text(buy, shadow),
                "exact_blocker_reason": exact_blocker_reason(row, joined_by_key),
            }
        )
    return rows


def delta_metrics(a: dict[str, Any], b: dict[str, Any]) -> dict[str, Any]:
    keys = (
        "shadow_simulation_attempted_rows",
        "shadow_simulation_success_rows",
        "shadow_simulation_failed_rows",
        "not_executable_route_rows",
        "route_materialization_error_rows",
        "state_readiness_error_rows",
        "Custom(2006)",
        "Custom(6002)",
    )
    delta = {f"{key}_delta": b.get(key, 0) - a.get(key, 0) for key in keys}
    root_a = a.get("root_cause_counts") or {}
    root_b = b.get("root_cause_counts") or {}
    for key in (
        "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING",
        "ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST",
        "ROUTE_CACHE_MISS_CONFLICT",
        "SIM_FAIL_CUSTOM_2006",
        "SIM_FAIL_CUSTOM_6002",
    ):
        delta[f"{key}_delta"] = root_b.get(key, 0) - root_a.get(key, 0)
    return delta


def outlier_rows(joined: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in joined:
        tail = tail_class(int_or_none(row.get("remaining_accounts_count")))
        include = (
            tail != "tail_len_2"
            or row.get("status") == "conflicted"
            or "raw_gRPC_tail_mismatch" in (row.get("taxonomy") or [])
            or "raw_gRPC_account_order_mismatch" in (row.get("taxonomy") or [])
            or "named_account_role_conflict" in (row.get("taxonomy") or [])
        )
        if not include:
            continue
        rows.append(
            {
                "route_kind": row.get("route_kind"),
                "topic": row.get("topic"),
                "signature": row.get("signature"),
                "slot": row.get("slot"),
                "tx_index": row.get("tx_index"),
                "ix_index": row.get("ix_index"),
                "account_manifest_hash": row.get("account_manifest_hash"),
                "instruction_evidence_hash": row.get("instruction_evidence_hash"),
                "remaining_accounts_count": row.get("remaining_accounts_count"),
                "tail_class": tail,
                "raw_gRPC_match_status": row.get("raw_gRPC_match_status"),
                "conflict_field": row.get("conflict_field"),
                "program_stream_value": row.get("program_stream_value"),
                "raw_gRPC_value": row.get("raw_gRPC_value"),
                "resolver_value": row.get("resolver_value"),
                "taxonomy": row.get("taxonomy"),
            }
        )
    return rows


def build_markdown(report: dict[str, Any]) -> str:
    a = report["baseline"]
    b = report["evidence_enabled"]
    d = report["delta"]
    lines = [
        "# R12 Route Evidence Join Impact",
        "",
        f"- scope: `{report['scope']}`",
        f"- status: `{report['status']}`",
        f"- decision_plane: `{report['decision_plane']}`",
        "- active execution changed: `false`",
        "- Program Stream candidates unlock execution: `false`",
        "",
        "## A Baseline",
        "",
        f"- buy_rows: `{a['buy_rows']}`",
        f"- shadow_simulation_attempted_rows: `{a['shadow_simulation_attempted_rows']}`",
        f"- shadow_simulation_success_rows: `{a['shadow_simulation_success_rows']}`",
        f"- shadow_simulation_failed_rows: `{a['shadow_simulation_failed_rows']}`",
        f"- not_executable_route_rows: `{a['not_executable_route_rows']}`",
        f"- route_materialization_error_rows: `{a['route_materialization_error_rows']}`",
        f"- state_readiness_error_rows: `{a['state_readiness_error_rows']}`",
        f"- Custom(2006): `{a['Custom(2006)']}`",
        f"- Custom(6002): `{a['Custom(6002)']}`",
        f"- attempt coverage: `{a['simulation_attempt_coverage']['display']}`",
        f"- success coverage: `{a['simulation_success_coverage']['display']}`",
        "",
        "## B Evidence-Enabled",
        "",
        f"- buy_rows: `{b['buy_rows']}`",
        f"- shadow_simulation_attempted_rows: `{b['shadow_simulation_attempted_rows']}`",
        f"- shadow_simulation_success_rows: `{b['shadow_simulation_success_rows']}`",
        f"- shadow_simulation_failed_rows: `{b['shadow_simulation_failed_rows']}`",
        f"- not_executable_route_rows: `{b['not_executable_route_rows']}`",
        f"- route_materialization_error_rows: `{b['route_materialization_error_rows']}`",
        f"- state_readiness_error_rows: `{b['state_readiness_error_rows']}`",
        f"- Custom(2006): `{b['Custom(2006)']}`",
        f"- Custom(6002): `{b['Custom(6002)']}`",
        f"- attempt coverage: `{b['simulation_attempt_coverage']['display']}`",
        f"- success coverage: `{b['simulation_success_coverage']['display']}`",
        "",
        "## Delta",
        "",
    ]
    for key, value in d.items():
        lines.append(f"- {key}: `{value}`")
    lines.extend(
        [
            "",
            "## Join Evidence",
            "",
            f"- candidate_rows: `{report['join_evidence']['candidate_rows']}`",
            f"- complete_rows: `{report['join_evidence']['complete_rows']}`",
            f"- can_unlock_execution_true_rows: `{report['join_evidence']['can_unlock_execution_true_rows']}`",
            f"- outlier_rows: `{report['outlier_rows']}`",
            f"- buy_blocker_rows: `{report['buy_blocker_rows']}`",
            f"- program_stream_join_key_absent_confirmed: `{report['program_stream_join_key_audit']['PROGRAM_STREAM_JOIN_KEY_ABSENT_CONFIRMED']}`",
            f"- program_stream_join_key_field_counts: `{report['program_stream_join_key_audit']['field_group_counts']}`",
            "",
            "## Historical Coverage Comparison",
            "",
            f"- old_93_coverage_claim_status: `{report['historical_coverage_comparison']['old_93_coverage_claim_status']}`",
            f"- comparison_rows: `{len(report['historical_coverage_comparison']['comparison_rows'])}`",
            f"- raw_full_tx_manifest_absent_in_existing_r12: `{report['historical_coverage_comparison']['interpretation']['raw_full_tx_manifest_absent_in_existing_r12']}`",
            "",
            "## Claim Boundaries",
            "",
            "- capture works claim: `not_evaluated_here`",
            "- route evidence can unlock execution: `false`",
            "- active execution path touched: `false`",
            "- success rows projected without runtime simulation: `false`",
        ]
    )
    if report.get("fail_reasons"):
        lines.extend(["", "## Fail Reasons", ""])
        for reason in report["fail_reasons"]:
            lines.append(f"- `{reason}`")
    return "\n".join(lines) + "\n"


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    output_dir = root / "reports" / "selector" / args.scope
    joined, join_summary = build_joined_evidence(args)
    program_stream_join_key_audit = audit_program_stream_join_keys(root, args.scope)
    baseline, buy_rows = build_buy_metrics(root, args.scope, args.decision_plane)
    evidence_enabled, projection_meta = build_evidence_enabled_projection(baseline, buy_rows, joined)
    delta = delta_metrics(baseline, evidence_enabled)
    outliers = outlier_rows(joined)
    blocker_table = build_buy_blocker_table(buy_rows, joined)
    historical_coverage = build_historical_coverage_comparison(root, args.scope, baseline)
    fail_reasons: list[str] = []
    if join_summary["can_unlock_execution_true_rows"]:
        fail_reasons.append("program_stream_candidate_can_unlock_execution_true")
    if baseline["buy_rows"] and evidence_enabled["shadow_simulation_attempted_rows"] < math.ceil(baseline["buy_rows"] * 0.95):
        fail_reasons.append("attempt_coverage_below_95pct")
    if baseline["buy_rows"] and evidence_enabled["shadow_simulation_success_rows"] < math.ceil(baseline["buy_rows"] * 0.95):
        fail_reasons.append("success_coverage_below_95pct")
    if (baseline["root_cause_counts"].get("UNKNOWN_UNCLASSIFIED") or 0) > 0:
        fail_reasons.append("UNKNOWN_UNCLASSIFIED_present")
    if join_summary["complete_rows"] == 0:
        fail_reasons.append("no_complete_joined_route_evidence")
    status = "PASS_DIAGNOSTIC" if not fail_reasons else "NO_GO_DIAGNOSTIC"
    outputs = {
        "json": str(output_dir / f"{ARTIFACT}.json"),
        "markdown": str(output_dir / "ROUTE_EVIDENCE_SIMCOV_IMPACT.md"),
        "joined": str(output_dir / f"{JOINED_ARTIFACT}.jsonl"),
        "outliers": str(output_dir / f"{OUTLIER_ARTIFACT}.jsonl"),
        "blocker_table": str(output_dir / f"{BLOCKER_TABLE_ARTIFACT}.jsonl"),
        "program_stream_join_key_audit": str(
            output_dir / f"{PROGRAM_STREAM_JOIN_KEY_AUDIT_ARTIFACT}.json"
        ),
        "historical_coverage_comparison": str(
            output_dir / "historical_coverage_comparison_v1.json"
        ),
    }
    report = {
        "artifact": ARTIFACT,
        "schema_version": 1,
        "scope": args.scope,
        "decision_plane": args.decision_plane,
        "status": status,
        "baseline": baseline,
        "evidence_enabled": evidence_enabled,
        "delta": delta,
        "join_evidence": join_summary,
        "program_stream_join_key_audit": program_stream_join_key_audit,
        "historical_coverage_comparison": historical_coverage,
        "projection_meta": projection_meta,
        "outlier_rows": len(outliers),
        "buy_blocker_rows": len(blocker_table),
        "fail_reasons": sorted(set(fail_reasons)),
        "claim_boundaries": {
            "offline_diagnostic_only": True,
            "active_execution_path_changed": False,
            "send_path_changed": False,
            "gatekeeper_changed": False,
            "program_stream_candidate_execution_unlock": False,
            "success_rows_projected_without_runtime_simulation": False,
        },
        "outputs": outputs,
    }
    common.write_jsonl(Path(outputs["joined"]), joined)
    common.write_jsonl(Path(outputs["outliers"]), outliers)
    common.write_jsonl(Path(outputs["blocker_table"]), blocker_table)
    common.write_json(Path(outputs["program_stream_join_key_audit"]), program_stream_join_key_audit)
    common.write_json(Path(outputs["historical_coverage_comparison"]), historical_coverage)
    common.write_json(Path(outputs["json"]), report)
    Path(outputs["markdown"]).parent.mkdir(parents=True, exist_ok=True)
    Path(outputs["markdown"]).write_text(build_markdown(report), encoding="utf-8")
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--decision-plane", default="legacy_live", choices=simcov.DECISION_PLANES)
    parser.add_argument("--candidates", type=Path)
    parser.add_argument(
        "--raw-transaction-evidence-glob",
        action="append",
        default=[],
        help="Root-relative glob for raw gRPC transaction evidence JSONL. May be repeated.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
