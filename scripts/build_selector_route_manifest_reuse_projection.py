#!/usr/bin/env python3
"""Project safe observed route-manifest reuse from raw tx evidence.

This is an offline diagnostic.  It does not change Gatekeeper, route
resolution, shadow execution, active execution, or send path.  A projected
recoverable row only means that raw observed evidence appears sufficient to
materialize a route manifest for the same pool/route under the constraints
reported here; it never proves simulation success and never unlocks execution.
"""

from __future__ import annotations

import argparse
import json
import math
import re
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import build_selector_route_evidence_join_report as exact_join
import selector_pipeline_common as common


ARTIFACT = "legacy_tail_manifest_resolution_projection_v1"
STORE_ARTIFACT = "observed_route_manifest_store_v1"
ROWS_ARTIFACT = "route_manifest_reuse_projection_rows_v1"
TAIL_ROWS_ARTIFACT = "legacy_tail_manifest_resolution_rows_v1"
STATE_AUDIT_ARTIFACT = "route_manifest_state_readiness_audit_v1"
PUMPFUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
STATE_READINESS_CLASS = "ROUTE_INCOMPLETE_STATE_NOT_READY"
LEGACY_TAIL_CLASS = "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING"
BCV2_CLASS = "ROUTE_INCOMPLETE_BCV2_MISSING"
ROUTE_RECOVERABLE_CLASSES = {
    LEGACY_TAIL_CLASS,
    BCV2_CLASS,
}
STABLE_CONFLICT_ROLES = {
    "global",
    "fee_recipient",
    "mint",
    "bonding_curve",
    "associated_bonding_curve",
    "system_program",
    "token_program",
    "creator_vault",
    "event_authority",
    "global_volume_accumulator",
    "user_volume_accumulator",
    "fee_config",
    "fee_program",
    "program",
}


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def exact_rate(numerator: int, denominator: int) -> dict[str, Any]:
    pct = (numerator / denominator * 100.0) if denominator else 0.0
    return {
        "numerator": numerator,
        "denominator": denominator,
        "rate": numerator / denominator if denominator else 0.0,
        "percent": pct,
        "display": f"{numerator} / {denominator} = {pct:.2f}%",
    }


def raw_evidence_paths(root: Path, scope: str, explicit_globs: list[str]) -> list[Path]:
    if explicit_globs:
        paths: list[Path] = []
        for pattern in explicit_globs:
            paths.extend(sorted(root.glob(pattern)))
        return paths
    return [
        root
        / "logs"
        / "nln_capture"
        / scope
        / "raw_pumpfun_instruction_evidence_v1.jsonl"
    ]


def program_candidate_paths(root: Path, scope: str) -> list[Path]:
    path = root / "logs" / "nln_capture" / scope / "route_manifest_evidence_candidates_v1.jsonl"
    return [path] if path.exists() else []


def field(row: dict[str, Any], *names: str) -> Any:
    for name in names:
        value = exact_join.value_from_path(row, *(tuple(part.split(".")) for part in name.split("|")))
        if value not in (None, "", []):
            return value
    return None


def named_accounts(row: dict[str, Any]) -> dict[str, str]:
    return exact_join.named_account_map(row)


def remaining_pubkeys(row: dict[str, Any] | None) -> list[str]:
    if not isinstance(row, dict):
        return []
    output: list[str] = []
    for item in row.get("remaining_accounts") or []:
        if isinstance(item, dict):
            value = exact_join.str_or_none(item.get("pubkey"))
            if value:
                output.append(value)
        elif item not in (None, ""):
            output.append(str(item))
    return output


def account_order(row: dict[str, Any]) -> list[str]:
    order = exact_join.compiled_account_order(row)
    if order:
        return order
    return [pubkey for _role, pubkey in exact_join.named_account_order(row)] + exact_join.remaining_accounts(row)


def stable_role_map(row: dict[str, Any]) -> dict[str, str]:
    return {
        role: value
        for role, value in named_accounts(row).items()
        if role in STABLE_CONFLICT_ROLES
    }


def manifest_identity(row: dict[str, Any]) -> tuple[str | None, str | None, str | None, str | None, str | None, str | None]:
    accounts = named_accounts(row)
    return (
        exact_join.str_or_none(row.get("mint")),
        exact_join.route_kind(row),
        accounts.get("bonding_curve"),
        accounts.get("associated_bonding_curve"),
        accounts.get("token_program"),
        exact_join.str_or_none(row.get("program_id")),
    )


def candidate_identity(row: dict[str, Any]) -> tuple[str | None, str | None, str | None, str | None, str | None]:
    return (
        exact_join.str_or_none(row.get("mint")),
        exact_join.str_or_none(row.get("route_kind")),
        exact_join.str_or_none(row.get("bonding_curve")),
        exact_join.str_or_none(row.get("associated_bonding_curve")),
        exact_join.str_or_none(row.get("token_program")),
    )


def manifest_id_for(row: dict[str, Any]) -> str:
    sig = exact_join.signature(row) or "nosig"
    slot = exact_join.slot(row)
    ix = exact_join.ix_index(row)
    h = row.get("account_manifest_hash") or "nohash"
    return f"manifest:{h}:{sig}:{slot}:{ix}"


def is_clean_raw_manifest(row: dict[str, Any]) -> bool:
    if row.get("can_unlock_execution") is True:
        return False
    if row.get("parser_status") not in (None, "", "OK"):
        return False
    if row.get("resolver_validation_status") not in (None, "", "PASS"):
        return False
    if exact_join.signature(row) is None or exact_join.slot(row) is None or exact_join.ix_index(row) is None:
        return False
    if not exact_join.account_keys(row) or not exact_join.compiled_indices(row):
        return False
    return True


def is_program_tail_candidate(row: dict[str, Any]) -> bool:
    if row.get("can_unlock_execution") is True:
        return False
    if row.get("parse_status") not in (None, "", "OK"):
        return False
    if row.get("source") != "nln_program_stream":
        return False
    if row.get("route_kind") not in ("legacy_buy", "routed_exact_sol_in"):
        return False
    if exact_join.int_or_none(row.get("remaining_accounts_count") or row.get("remaining_account_count")) is None:
        return False
    return True


def build_program_tail_index(rows: Iterable[dict[str, Any]]) -> dict[tuple[str, str], list[dict[str, Any]]]:
    index: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        if not is_program_tail_candidate(row):
            continue
        mint, route_kind, _bc, _abc, _token_program = candidate_identity(row)
        if not mint or not route_kind:
            continue
        index[(mint, route_kind)].append(row)
    return index


def build_manifest_store(raw_rows: Iterable[dict[str, Any]]) -> tuple[list[dict[str, Any]], dict[tuple[str, str], list[dict[str, Any]]]]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = defaultdict(list)
    for row in raw_rows:
        if not is_clean_raw_manifest(row):
            continue
        identity = manifest_identity(row)
        if any(part in (None, "") for part in identity):
            continue
        grouped[identity].append(row)

    conflict_identities: set[tuple[Any, ...]] = set()
    for identity, rows in grouped.items():
        reference: dict[str, str] | None = None
        reference_tail: list[str] | None = None
        for row in rows:
            stable = stable_role_map(row)
            tail = exact_join.remaining_accounts(row)
            if reference is None:
                reference = stable
                reference_tail = tail
                continue
            if stable != reference or tail != reference_tail:
                conflict_identities.add(identity)
                break

    store: list[dict[str, Any]] = []
    index: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for identity, rows in grouped.items():
        mint, route_kind, bonding_curve, associated_bonding_curve, token_program, program_id = identity
        first_slot = min(exact_join.slot(row) or 0 for row in rows)
        last_slot = max(exact_join.slot(row) or 0 for row in rows)
        for row in rows:
            accounts = named_accounts(row)
            manifest = {
                "artifact": STORE_ARTIFACT,
                "schema_version": 1,
                "manifest_id": manifest_id_for(row),
                "source_signature": exact_join.signature(row),
                "source_slot": exact_join.slot(row),
                "source_ix_index": exact_join.ix_index(row),
                "route_kind": route_kind,
                "mint": mint,
                "bonding_curve": bonding_curve,
                "associated_bonding_curve": associated_bonding_curve,
                "associated_user_role_present": "associated_user" in accounts,
                "token_program": token_program,
                "program_id": program_id,
                "named_accounts": exact_join.named_account_order(row),
                "remaining_accounts": exact_join.remaining_accounts(row),
                "remaining_accounts_count": exact_join.remaining_count(row),
                "has_legacy_tail": exact_join.remaining_count(row) == 2,
                "account_order": account_order(row),
                "account_manifest_hash": row.get("account_manifest_hash"),
                "first_seen_slot": first_slot,
                "last_seen_slot": last_slot,
                "observations_count": len(rows),
                "conflict_status": "conflicted" if identity in conflict_identities else "clean",
                "can_unlock_execution": False,
                "resolver_validation_status": row.get("resolver_validation_status") or "PASS",
            }
            store.append(manifest)
            index[(str(mint), str(route_kind))].append(manifest)
    return store, index


def role_from_account_set(rows: list[Any], role: str) -> str | None:
    for item in rows:
        if not isinstance(item, str):
            continue
        parts = item.split(":")
        if len(parts) >= 2 and parts[0] == role and parts[1]:
            return parts[1]
    return None


def first_row_value(*rows: dict[str, Any] | None, names: str) -> Any:
    for row in rows:
        if not isinstance(row, dict):
            continue
        value = field(row, names)
        if value not in (None, "", []):
            return value
    return None


def desired_route_kind(classification: str, buy: dict[str, Any], shadow: dict[str, Any] | None) -> str | None:
    if classification == LEGACY_TAIL_CLASS:
        fallback = first_row_value(buy, shadow, names="fallback_route_kind")
        if fallback:
            return str(fallback)
        return "legacy_buy"
    if classification == BCV2_CLASS:
        value = first_row_value(
            buy,
            shadow,
            names="primary_route_kind|observed_bcv2_source_buy_variant|buy_variant|route_kind",
        )
        return str(value) if value else "routed_exact_sol_in"
    value = first_row_value(
        buy,
        shadow,
        names="buy_variant|selected_route_kind|primary_route_kind|fallback_route_kind|route_kind",
    )
    return str(value) if value else None


def buy_identity(row: dict[str, Any]) -> dict[str, Any]:
    buy = row["buy"]
    shadow = row.get("shadow")
    account_lists: list[Any] = []
    for source in (buy, shadow):
        if isinstance(source, dict):
            for key in (
                "selected_route_account_set_roles",
                "fallback_simulation_load_account_set",
                "simulation_account_manifest",
            ):
                value = source.get(key)
                if isinstance(value, list):
                    account_lists.extend(value)
    classification = str(row["classification"])
    mint = first_row_value(buy, shadow, names="base_mint|mint_id|mint|payload.base_mint|payload.mint_id")
    pool_id = first_row_value(buy, shadow, names="pool_id|pool_amm_id|payload.pool_id|payload.pool_amm_id")
    route_kind = desired_route_kind(classification, buy, shadow)
    bonding_curve = first_row_value(
        buy,
        shadow,
        names="legacy_buy_curve_pubkey|bonding_curve_pubkey|payload.bonding_curve",
    ) or role_from_account_set(account_lists, "bonding_curve") or pool_id
    associated_bonding_curve = first_row_value(
        buy,
        shadow,
        names="legacy_buy_associated_bonding_curve_pubkey|associated_bonding_curve_pubkey|payload.associated_bonding_curve",
    ) or role_from_account_set(account_lists, "associated_bonding_curve")
    token_program = first_row_value(
        buy,
        shadow,
        names="token_program|payload.token_program",
    ) or role_from_account_set(account_lists, "token_program")
    raw_decision_slot = exact_join.int_or_none(
        first_row_value(
            buy,
            shadow,
            names="decision_slot|slot|observed_bcv2_source_slot|entry_slot|sample_slot|rpc_slot",
        )
    )
    decision_slot = raw_decision_slot if raw_decision_slot and raw_decision_slot > 0 else None
    decision_ts_ms = exact_join.int_or_none(
        first_row_value(buy, shadow, names="decision_ts_ms|timestamp_ms|first_seen_ts_ms")
    )
    enough_identity = all([mint, route_kind, bonding_curve, associated_bonding_curve])
    return {
        "buy_row_id": None,
        "mint": exact_join.str_or_none(mint),
        "pool_id": exact_join.str_or_none(pool_id),
        "route_kind": exact_join.str_or_none(route_kind),
        "decision_slot": decision_slot,
        "decision_ts_ms": decision_ts_ms,
        "bonding_curve": exact_join.str_or_none(bonding_curve),
        "associated_bonding_curve": exact_join.str_or_none(associated_bonding_curve),
        "token_program": exact_join.str_or_none(token_program),
        "current_error_class": classification,
        "has_enough_identity_for_manifest_lookup": enough_identity,
    }


def original_error_text(row: dict[str, Any]) -> str | None:
    buy = row.get("buy") if isinstance(row.get("buy"), dict) else {}
    shadow = row.get("shadow") if isinstance(row.get("shadow"), dict) else {}
    for source in (shadow, buy):
        for name in (
            "state_latch_original_error",
            "precheck_failure_reason",
            "simulation_error_message",
            "route_resolution_terminal_reason",
            "execution_feasibility_reason",
            "err",
            "error",
        ):
            value = source.get(name)
            if value not in (None, ""):
                return str(value)
    return None


def route_cache_status(row: dict[str, Any]) -> str | None:
    buy = row.get("buy") if isinstance(row.get("buy"), dict) else {}
    shadow = row.get("shadow") if isinstance(row.get("shadow"), dict) else {}
    for source in (shadow, buy):
        for name in (
            "route_account_manifest_source",
            "manifest_cache_lookup_status",
            "manifest_cache_lookup_phase",
            "route_resolution_status",
            "route_resolution_terminal_reason",
        ):
            value = source.get(name)
            if value not in (None, ""):
                return str(value)
    return None


def temporal_match(manifest: dict[str, Any], identity: dict[str, Any]) -> tuple[bool, str]:
    source_slot = exact_join.int_or_none(manifest.get("source_slot"))
    decision_slot = exact_join.int_or_none(identity.get("decision_slot"))
    if source_slot is not None and decision_slot is not None:
        return source_slot <= decision_slot, "slot_lte_decision_slot" if source_slot <= decision_slot else "source_slot_after_decision_slot"
    return True, "temporal_unavailable_no_slot_block_not_applied"


def manifest_matches_identity(manifest: dict[str, Any], identity: dict[str, Any]) -> bool:
    if manifest.get("mint") != identity.get("mint"):
        return False
    if manifest.get("route_kind") != identity.get("route_kind"):
        return False
    for field_name in ("bonding_curve", "associated_bonding_curve", "token_program"):
        expected = identity.get(field_name)
        if expected and manifest.get(field_name) != expected:
            return False
    return True


def lookup_manifest(identity: dict[str, Any], manifest_index: dict[tuple[str, str], list[dict[str, Any]]]) -> tuple[str, dict[str, Any] | None, str]:
    if not identity["mint"] or not identity["route_kind"]:
        return "state_identity_missing", None, "identity_missing_mint_or_route"
    candidates = manifest_index.get((identity["mint"], identity["route_kind"]), [])
    if not candidates:
        return "no_manifest_for_mint_route", None, "no_manifest_for_mint_route"
    exact_candidates = [m for m in candidates if manifest_matches_identity(m, identity)]
    if not identity["has_enough_identity_for_manifest_lookup"]:
        return "mint_route_manifest_found_pool_identity_missing", None, "missing_pool_route_identity"
    if not exact_candidates:
        return "state_identity_missing", None, "no_manifest_matching_pool_identity"
    clean: list[dict[str, Any]] = []
    for manifest in exact_candidates:
        if manifest.get("conflict_status") != "clean":
            continue
        temporal_ok, temporal_quality = temporal_match(manifest, identity)
        if not temporal_ok:
            return "temporal_violation", manifest, temporal_quality
        if manifest.get("resolver_validation_status") not in (None, "", "PASS"):
            return "resolver_validation_failed", manifest, temporal_quality
        clean.append(manifest)
    if not clean:
        return "manifest_conflict", exact_candidates[0], "conflict_or_validation_failed"
    # Prefer the most observed and latest compatible manifest for projection.
    clean.sort(key=lambda m: (int(m.get("observations_count") or 0), int(m.get("last_seen_slot") or 0)), reverse=True)
    return "exact_pool_route_manifest_found", clean[0], temporal_match(clean[0], identity)[1]


def candidate_matches_identity(candidate: dict[str, Any], identity: dict[str, Any]) -> bool:
    mint, route_kind, bonding_curve, associated_bonding_curve, token_program = candidate_identity(candidate)
    if mint != identity.get("mint") or route_kind != identity.get("route_kind"):
        return False
    for expected, candidate_value in (
        (identity.get("bonding_curve"), bonding_curve),
        (identity.get("associated_bonding_curve"), associated_bonding_curve),
        (identity.get("token_program"), token_program),
    ):
        if expected and candidate_value != expected:
            return False
    return True


def lookup_program_tail(
    identity: dict[str, Any],
    program_index: dict[tuple[str, str], list[dict[str, Any]]],
) -> tuple[bool, dict[str, Any] | None]:
    if not identity.get("mint") or not identity.get("route_kind"):
        return False, None
    candidates = program_index.get((identity["mint"], identity["route_kind"]), [])
    for candidate in candidates:
        if not candidate_matches_identity(candidate, identity):
            continue
        count = exact_join.int_or_none(candidate.get("remaining_accounts_count") or candidate.get("remaining_account_count"))
        if count and count > 0:
            return True, candidate
    return False, None


def route_tail_requirement_ok(identity: dict[str, Any], manifest: dict[str, Any]) -> bool:
    if identity.get("route_kind") == "legacy_buy":
        return int(manifest.get("remaining_accounts_count") or 0) == 2
    return int(manifest.get("remaining_accounts_count") or 0) > 0


def recoverability_for(
    *,
    classification: str,
    lookup_status: str,
    projected_status: str,
    manifest: dict[str, Any] | None,
    program_tail_available: bool,
) -> tuple[str, str]:
    if classification == BCV2_CLASS:
        return "BLOCKED_BY_BCV2_SCHEMA", "bcv2 route schema requires BCV2 readiness, not legacy tail reuse"
    if projected_status == "would_be_route_materializable_offline":
        if program_tail_available and manifest is not None:
            return (
                "TAIL_RECOVERABLE_BY_EXACT_POOL_ROUTE_MANIFEST",
                "clean raw transaction manifest matches pool identity and restore tail contract",
            )
        return (
            "TAIL_RECOVERABLE_BY_RAW_TX_MANIFEST",
            "clean raw transaction manifest matches pool identity and restore tail contract",
        )
    if lookup_status == "no_manifest_for_mint_route":
        return "BLOCKED_BY_NO_PRIOR_MANIFEST", "no raw manifest for mint+route"
    if lookup_status == "manifest_conflict":
        return "BLOCKED_BY_ROUTE_CACHE_CONFLICT", "raw manifest candidates conflict on stable account roles or tail"
    if lookup_status == "temporal_violation":
        return "BLOCKED_BY_TEMPORAL_VIOLATION", "raw manifest source slot is after decision slot"
    if lookup_status in ("state_identity_missing", "mint_route_manifest_found_pool_identity_missing"):
        return "BLOCKED_BY_POOL_IDENTITY_MISSING", "BUY row lacks enough pool identity for safe reuse"
    if lookup_status == "route_tail_requirement_mismatch":
        return "BLOCKED_BY_TAIL_LEN_MISMATCH", "candidate manifest tail length does not match restore contract"
    if lookup_status == "resolver_validation_failed":
        return "BLOCKED_BY_RESOLVER_VALIDATION_FAILED", "raw manifest resolver validation failed"
    if lookup_status == "route_kind_mismatch":
        return "BLOCKED_BY_ROUTE_KIND_MISMATCH", "raw manifest route kind does not match BUY route"
    if program_tail_available and manifest is not None:
        return (
            "TAIL_RECOVERABLE_BY_PROGRAM_STREAM_TAIL_PLUS_RAW_IDENTITY",
            "program stream tail exists and raw identity is available, but complete unlock remains projection-only",
        )
    return "BLOCKED_BY_NO_PRIOR_MANIFEST", "no complete safe tail manifest available"


def projection_for_row(
    row_id: int,
    row: dict[str, Any],
    manifest_index: dict[tuple[str, str], list[dict[str, Any]]],
    program_index: dict[tuple[str, str], list[dict[str, Any]]],
) -> dict[str, Any]:
    identity = buy_identity(row)
    identity["buy_row_id"] = row_id
    classification = str(row["classification"])
    mint_route_candidates = manifest_index.get((identity.get("mint") or "", identity.get("route_kind") or ""), [])
    lookup_status, manifest, temporal_quality = lookup_manifest(identity, manifest_index)
    program_tail_available, program_tail_candidate = lookup_program_tail(identity, program_index)
    projected_status = "would_remain_not_executable"
    recoverable = False
    if row["simulation_success"]:
        projected_status = "already_simulation_success"
    elif row["simulation_failed"]:
        projected_status = "already_simulation_failed"
    elif not row["not_executable_route"]:
        projected_status = "not_not_executable"
    elif classification == STATE_READINESS_CLASS:
        projected_status = "blocked_by_state_readiness"
    elif not identity["has_enough_identity_for_manifest_lookup"]:
        projected_status = "blocked_by_identity_gap"
    elif lookup_status == "exact_pool_route_manifest_found" and manifest is not None:
        if route_tail_requirement_ok(identity, manifest):
            projected_status = "would_be_route_materializable_offline"
            recoverable = classification in ROUTE_RECOVERABLE_CLASSES
        else:
            projected_status = "blocked_by_bcv2_schema"
            lookup_status = "route_tail_requirement_mismatch"
    elif lookup_status == "no_manifest_for_mint_route":
        projected_status = "blocked_by_no_manifest"
    elif lookup_status == "manifest_conflict":
        projected_status = "blocked_by_conflict"
    elif lookup_status == "temporal_violation":
        projected_status = "would_remain_not_executable"
    else:
        projected_status = "blocked_by_identity_gap"
    projected_recoverability, blocking_reason = recoverability_for(
        classification=classification,
        lookup_status=lookup_status,
        projected_status=projected_status,
        manifest=manifest,
        program_tail_available=program_tail_available,
    )
    raw_tail_accounts = remaining_pubkeys(manifest)
    program_tail_accounts = remaining_pubkeys(program_tail_candidate)
    tail_accounts = raw_tail_accounts or program_tail_accounts
    remaining_accounts_count = (
        manifest.get("remaining_accounts_count")
        if manifest
        else exact_join.int_or_none(
            (program_tail_candidate or {}).get("remaining_accounts_count")
            or (program_tail_candidate or {}).get("remaining_account_count")
        )
    )
    return {
        "artifact": ROWS_ARTIFACT,
        "schema_version": 1,
        "row_id": row_id,
        "buy_row_id": row_id,
        "buy_line": row.get("buy", {}).get("_source_line") if isinstance(row.get("buy"), dict) else None,
        "shadow_line": row.get("shadow", {}).get("_source_line") if isinstance(row.get("shadow"), dict) else None,
        "mint": identity.get("mint"),
        "pool_id": identity.get("pool_id"),
        "route_kind": identity.get("route_kind"),
        "bonding_curve": identity.get("bonding_curve"),
        "associated_bonding_curve": identity.get("associated_bonding_curve"),
        "token_program": identity.get("token_program"),
        "decision_slot": identity.get("decision_slot"),
        "decision_ts_ms": identity.get("decision_ts_ms"),
        "original_error": original_error_text(row),
        "route_cache_status": route_cache_status(row),
        "raw_manifest_available": bool(mint_route_candidates),
        "program_stream_tail_available": program_tail_available,
        "remaining_accounts_count": remaining_accounts_count,
        "manifest_source_signature": manifest.get("source_signature") if manifest else None,
        "manifest_source_slot": manifest.get("source_slot") if manifest else None,
        "manifest_source_ix_index": manifest.get("source_ix_index") if manifest else None,
        "manifest_id": manifest.get("manifest_id") if manifest else None,
        "account_manifest_hash": manifest.get("account_manifest_hash") if manifest else (
            program_tail_candidate.get("account_manifest_hash") if program_tail_candidate else None
        ),
        "tail_accounts": tail_accounts,
        "temporal_match_status": temporal_quality,
        "conflict_status": manifest.get("conflict_status") if manifest else (
            "candidate_conflict_or_missing" if lookup_status == "manifest_conflict" else "none"
        ),
        "resolver_validation_status": (
            manifest.get("resolver_validation_status") if manifest else "NOT_RUN_BUT_REQUIRED_FOR_EXECUTION"
        ),
        "projected_recoverability": projected_recoverability,
        "blocking_reason": blocking_reason,
        "pool_identity": {
            "bonding_curve": identity.get("bonding_curve"),
            "associated_bonding_curve": identity.get("associated_bonding_curve"),
            "token_program": identity.get("token_program"),
        },
        "current_error_class": classification,
        "baseline_status": exact_join.status_from_buy_row(row),
        "has_enough_identity_for_manifest_lookup": identity["has_enough_identity_for_manifest_lookup"],
        "manifest_lookup_status": lookup_status,
        "temporal_match_quality": temporal_quality,
        "manifest_remaining_accounts_count": manifest.get("remaining_accounts_count") if manifest else None,
        "manifest_conflict_status": manifest.get("conflict_status") if manifest else None,
        "local_resolver_validation": (
            manifest.get("resolver_validation_status") if manifest else "NOT_RUN_BUT_REQUIRED_FOR_EXECUTION"
        ),
        "projected_attempt_status": projected_status,
        "recoverable_by_manifest": recoverable,
        "can_unlock_execution": False,
        "row_level_reason": (
            "recoverable_by_manifest"
            if recoverable
            else projected_status
        ),
    }


def extract_state_curve(row: dict[str, Any]) -> str | None:
    for candidate in (
        row.get("pool_identity", {}).get("bonding_curve") if isinstance(row.get("pool_identity"), dict) else None,
        row.get("pool_id"),
    ):
        if candidate:
            return str(candidate)
    return None


def scan_diag_lines(root: Path, scope: str, curves: set[str]) -> dict[str, dict[str, Any]]:
    result = {
        curve: {
            "diag_account_update_relay_count": 0,
            "diag_account_update_runtime_ingress_count": 0,
            "diag_account_update_applied_count": 0,
            "first_diag_sample": None,
        }
        for curve in curves
    }
    if not curves:
        return result
    for base in (root / "logs" / "rollout" / scope).glob("**/*.log*"):
        if not base.is_file():
            continue
        with base.open(encoding="utf-8", errors="ignore") as handle:
            for line_no, line in enumerate(handle, 1):
                if "DIAG_ACCOUNT_UPDATE" not in line:
                    continue
                for curve in curves:
                    if curve not in line:
                        continue
                    item = result[curve]
                    if "DIAG_ACCOUNT_UPDATE_RELAY" in line:
                        item["diag_account_update_relay_count"] += 1
                    if "DIAG_ACCOUNT_UPDATE_RUNTIME_INGRESS" in line:
                        item["diag_account_update_runtime_ingress_count"] += 1
                    if "DIAG_ACCOUNT_UPDATE_APPLIED" in line:
                        item["diag_account_update_applied_count"] += 1
                    if item["first_diag_sample"] is None:
                        item["first_diag_sample"] = {
                            "path": str(base),
                            "line": line_no,
                            "text": line.strip()[:500],
                        }
    return result


def build_state_readiness_audit(root: Path, scope: str, projection_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    state_rows = [row for row in projection_rows if row["current_error_class"] == STATE_READINESS_CLASS]
    curves = {curve for row in state_rows for curve in [extract_state_curve(row)] if curve}
    diag = scan_diag_lines(root, scope, curves)
    audit_rows: list[dict[str, Any]] = []
    for row in state_rows:
        curve = extract_state_curve(row)
        diag_item = diag.get(curve or "", {})
        relay_count = int(diag_item.get("diag_account_update_relay_count") or 0)
        applied_count = int(diag_item.get("diag_account_update_applied_count") or 0)
        audit_rows.append(
            {
                "artifact": STATE_AUDIT_ARTIFACT,
                "schema_version": 1,
                "row_id": row["row_id"],
                "mint": row.get("mint"),
                "bonding_curve": curve,
                "decision_slot": row.get("decision_slot"),
                "decision_ts_ms": row.get("decision_ts_ms"),
                "diag_account_update_relay_count": relay_count,
                "diag_account_update_runtime_ingress_count": int(
                    diag_item.get("diag_account_update_runtime_ingress_count") or 0
                ),
                "diag_account_update_applied_count": applied_count,
                "first_diag_sample": diag_item.get("first_diag_sample"),
                "state_readiness_diagnosis": (
                    "diag_update_seen_but_timing_unverified"
                    if relay_count or applied_count
                    else "no_diag_update_found_for_curve_in_rollout_logs"
                ),
                "can_unlock_execution": False,
            }
        )
    return audit_rows


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def build_markdown(report: dict[str, Any]) -> str:
    b = report["baseline"]
    p = report["projection"]
    r17 = report["r17_tail_resolution"]
    lines = [
        "# PR-R17 Legacy Tail Manifest Resolution Projection",
        "",
        f"- scope: `{report['scope']}`",
        f"- status: `{report['status']}`",
        "- mode: `offline_projection_only`",
        "- active execution unlock: `false`",
        "",
        "## Baseline",
        "",
        f"- buy_rows: `{b['buy_rows']}`",
        f"- attempted: `{b['simulation_attempt_coverage']['display']}`",
        f"- success: `{b['simulation_success_coverage']['display']}`",
        f"- failed: `{b['shadow_simulation_failed_rows']} / {b['buy_rows']}`",
        f"- not_executable: `{b['not_executable_route_rows']} / {b['buy_rows']}`",
        f"- target_90_rows: `{b['target_90_rows']}`",
        f"- target_95_rows: `{b['target_95_rows']}`",
        "",
        "## R17 Tail Resolution",
        "",
        f"- LEGACY_TAIL_MISSING rows: `{r17['LEGACY_TAIL_MISSING_rows']}`",
        f"- tail_recoverable_rows: `{r17['tail_recoverable_rows']}`",
        f"- tail_blocked_by_conflict: `{r17['tail_blocked_by_conflict']}`",
        f"- tail_blocked_by_no_manifest: `{r17['tail_blocked_by_no_manifest']}`",
        f"- tail_blocked_by_temporal_violation: `{r17['tail_blocked_by_temporal_violation']}`",
        f"- tail_blocked_by_identity_gap: `{r17['tail_blocked_by_identity_gap']}`",
        f"- projected_attempted: `{r17['projected_attempt_coverage']['display']}`",
        f"- gap_to_90: `{r17['gap_to_90']}`",
        f"- gap_to_95: `{r17['gap_to_95']}`",
        f"- path_to_90: `{r17['path_to_90']}`",
        f"- path_to_95: `{r17['path_to_95']}`",
        "",
        "## General Manifest Projection",
        "",
        f"- rows_with_raw_manifest_available: `{p['rows_with_raw_manifest_available']}`",
        f"- not_executable_rows_matched_by_manifest: `{p['not_executable_rows_matched_by_manifest']}`",
        f"- LEGACY_TAIL_MISSING rows recoverable: `{p['LEGACY_TAIL_MISSING_rows_recoverable']}`",
        f"- BCV2_MISSING rows recoverable: `{p['BCV2_MISSING_rows_recoverable']}`",
        f"- STATE_NOT_READY rows recoverable: `{p['STATE_NOT_READY_rows_recoverable']}`",
        f"- rows_blocked_by_identity_gap: `{p['rows_blocked_by_identity_gap']}`",
        f"- rows_blocked_by_no_manifest: `{p['rows_blocked_by_no_manifest']}`",
        f"- rows_blocked_by_conflict: `{p['rows_blocked_by_conflict']}`",
        f"- projected_attempted: `{p['projected_attempt_coverage']['display']}`",
        f"- projected_gap_to_90: `{p['projected_gap_to_90']}`",
        f"- projected_gap_to_95: `{p['projected_gap_to_95']}`",
        f"- manifest_reuse_alone_can_reach_95: `{p['manifest_reuse_alone_can_reach_95']}`",
        f"- minimal_additional_state_lift_needed_for_95: `{p['minimal_additional_state_lift_needed_for_95']}`",
        f"- theoretical tail-only attempted: `{p['theoretical_tail_only_attempt_coverage']['display']}`",
        f"- theoretical gap after all legacy tail missing fixed: `{p['theoretical_gap_after_all_legacy_tail_missing_fixed']}`",
        "",
        "## Manifest Store",
        "",
        f"- raw_evidence_rows: `{report['manifest_store']['raw_evidence_rows']}`",
        f"- manifest_rows: `{report['manifest_store']['manifest_rows']}`",
        f"- clean_manifest_rows: `{report['manifest_store']['clean_manifest_rows']}`",
        f"- unique_manifest_id_rows: `{report['manifest_store']['unique_manifest_id_rows']}`",
        f"- conflict_manifest_rows: `{report['manifest_store']['conflict_manifest_rows']}`",
        "",
        "## Claim Boundaries",
        "",
        "- observed manifest store can unlock execution: `false`",
        "- active execution path changed: `false`",
        "- success rows projected without runtime simulation: `false`",
    ]
    if report.get("fail_reasons"):
        lines.extend(["", "## Fail Reasons", ""])
        lines.extend(f"- `{reason}`" for reason in report["fail_reasons"])
    return "\n".join(lines) + "\n"


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    output_dir = root / "reports" / "selector" / args.scope
    paths = raw_evidence_paths(root, args.scope, args.raw_transaction_evidence_glob)
    raw_rows: list[dict[str, Any]] = []
    for path in paths:
        if path.exists():
            raw_rows.extend(read_jsonl(path))
    program_paths = program_candidate_paths(root, args.scope)
    program_rows: list[dict[str, Any]] = []
    for path in program_paths:
        if path.exists():
            program_rows.extend(read_jsonl(path))
    store, manifest_index = build_manifest_store(raw_rows)
    program_index = build_program_tail_index(program_rows)
    baseline, buy_rows = exact_join.build_buy_metrics(root, args.scope, args.decision_plane)
    baseline["target_90_rows"] = math.ceil(baseline["buy_rows"] * 0.90)
    projection_rows = [
        projection_for_row(row_id, row, manifest_index, program_index)
        for row_id, row in enumerate(buy_rows, 1)
    ]
    not_exec_rows = [row for row in projection_rows if row["baseline_status"] == "not_executable_route"]
    tail_rows = [row for row in not_exec_rows if row["current_error_class"] == LEGACY_TAIL_CLASS]
    tail_recoverable_rows = [
        row
        for row in tail_rows
        if str(row["projected_recoverability"]).startswith("TAIL_RECOVERABLE_")
    ]
    recoverable_rows = [row for row in not_exec_rows if row["recoverable_by_manifest"]]
    projected_attempted = baseline["shadow_simulation_attempted_rows"] + len(tail_recoverable_rows)
    general_projected_attempted = baseline["shadow_simulation_attempted_rows"] + len(recoverable_rows)
    target_90 = baseline["target_90_rows"]
    target_95 = baseline["target_95_rows"]
    lookup_counts = Counter(row["manifest_lookup_status"] for row in projection_rows)
    projected_counts = Counter(row["projected_attempt_status"] for row in projection_rows)
    tail_recoverability_counts = Counter(row["projected_recoverability"] for row in tail_rows)
    class_recoverable = Counter(
        row["current_error_class"] for row in recoverable_rows
    )
    state_audit_rows = build_state_readiness_audit(root, args.scope, projection_rows)
    state_rows = [row for row in projection_rows if row["current_error_class"] == STATE_READINESS_CLASS]
    state_needed = max(0, target_95 - general_projected_attempted)
    theoretical_tail_lift = baseline["root_cause_counts"].get("ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING", 0)
    theoretical_tail_attempted = baseline["shadow_simulation_attempted_rows"] + theoretical_tail_lift
    can_unlock_execution_true_rows = sum(
        1
        for row in store + projection_rows + state_audit_rows + program_rows
        if row.get("can_unlock_execution") is True
    )
    fail_reasons: list[str] = []
    if can_unlock_execution_true_rows:
        fail_reasons.append("can_unlock_execution_true")
    if len(not_exec_rows) != baseline["not_executable_route_rows"]:
        fail_reasons.append("not_executable_row_count_mismatch")
    if baseline["root_cause_counts"].get("UNKNOWN_UNCLASSIFIED", 0):
        fail_reasons.append("unknown_unclassified_present")
    if len(tail_rows) != baseline["root_cause_counts"].get(LEGACY_TAIL_CLASS, 0):
        fail_reasons.append("legacy_tail_row_count_mismatch")
    status = (
        "PASS_PATH_TO_95_DIAGNOSTIC"
        if len(tail_recoverable_rows) >= 15 and not fail_reasons
        else "PASS_PATH_TO_90_DIAGNOSTIC"
        if len(tail_recoverable_rows) >= 9 and not fail_reasons
        else "NO_GO_DIAGNOSTIC"
    )
    outputs = {
        "json": str(output_dir / f"{ARTIFACT}.json"),
        "markdown": str(output_dir / "LEGACY_TAIL_MANIFEST_RESOLUTION_PROJECTION.md"),
        "manifest_store": str(output_dir / f"{STORE_ARTIFACT}.jsonl"),
        "projection_rows": str(output_dir / f"{ROWS_ARTIFACT}.jsonl"),
        "legacy_tail_rows": str(output_dir / f"{TAIL_ROWS_ARTIFACT}.jsonl"),
        "state_readiness_audit": str(output_dir / f"{STATE_AUDIT_ARTIFACT}.jsonl"),
    }
    report = {
        "artifact": ARTIFACT,
        "schema_version": 1,
        "scope": args.scope,
        "decision_plane": args.decision_plane,
        "status": status,
        "baseline": baseline,
        "manifest_store": {
            "raw_evidence_paths": [str(path) for path in paths if path.exists()],
            "raw_evidence_rows": len(raw_rows),
            "manifest_rows": len(store),
            "clean_manifest_rows": sum(1 for row in store if row["conflict_status"] == "clean"),
            "unique_manifest_id_rows": len({row["manifest_id"] for row in store}),
            "conflict_manifest_rows": sum(1 for row in store if row["conflict_status"] != "clean"),
            "route_kind_counts": common.counter_dict(Counter(row["route_kind"] for row in store)),
            "tail_count_distribution": common.counter_dict(Counter(str(row["remaining_accounts_count"]) for row in store)),
        },
        "program_stream_tail_evidence": {
            "program_candidate_paths": [str(path) for path in program_paths if path.exists()],
            "program_candidate_rows": len(program_rows),
            "program_tail_candidate_rows": sum(1 for row in program_rows if is_program_tail_candidate(row)),
            "route_kind_counts": common.counter_dict(Counter(str(row.get("route_kind")) for row in program_rows)),
            "tail_count_distribution": common.counter_dict(
                Counter(
                    str(row.get("remaining_accounts_count") or row.get("remaining_account_count"))
                    for row in program_rows
                )
            ),
        },
        "r17_tail_resolution": {
            "buy_rows": baseline["buy_rows"],
            "attempted_rows_baseline": baseline["shadow_simulation_attempted_rows"],
            "attempted_baseline_coverage": baseline["simulation_attempt_coverage"],
            "LEGACY_TAIL_MISSING_rows": len(tail_rows),
            "tail_recoverable_rows": len(tail_recoverable_rows),
            "tail_blocked_by_conflict": tail_recoverability_counts.get("BLOCKED_BY_ROUTE_CACHE_CONFLICT", 0),
            "tail_blocked_by_no_manifest": tail_recoverability_counts.get("BLOCKED_BY_NO_PRIOR_MANIFEST", 0),
            "tail_blocked_by_temporal_violation": tail_recoverability_counts.get("BLOCKED_BY_TEMPORAL_VIOLATION", 0),
            "tail_blocked_by_identity_gap": tail_recoverability_counts.get("BLOCKED_BY_POOL_IDENTITY_MISSING", 0),
            "tail_blocked_by_route_kind_mismatch": tail_recoverability_counts.get("BLOCKED_BY_ROUTE_KIND_MISMATCH", 0),
            "tail_blocked_by_tail_len_mismatch": tail_recoverability_counts.get("BLOCKED_BY_TAIL_LEN_MISMATCH", 0),
            "tail_blocked_by_resolver_validation_failed": tail_recoverability_counts.get(
                "BLOCKED_BY_RESOLVER_VALIDATION_FAILED", 0
            ),
            "tail_projected_recoverability_counts": common.counter_dict(tail_recoverability_counts),
            "projected_attempted_rows": projected_attempted,
            "projected_attempt_coverage": exact_rate(projected_attempted, baseline["buy_rows"]),
            "target_90_rows": target_90,
            "target_95_rows": target_95,
            "gap_to_90": max(0, target_90 - projected_attempted),
            "gap_to_95": max(0, target_95 - projected_attempted),
            "path_to_90": len(tail_recoverable_rows) >= 9,
            "path_to_95": len(tail_recoverable_rows) >= 15,
        },
        "projection": {
            "rows_with_raw_manifest_available": sum(
                1 for row in projection_rows if row["manifest_lookup_status"] == "exact_pool_route_manifest_found"
            ),
            "not_executable_rows_matched_by_manifest": len(recoverable_rows),
            "LEGACY_TAIL_MISSING_rows_recoverable": class_recoverable.get(
                "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING", 0
            ),
            "BCV2_MISSING_rows_recoverable": class_recoverable.get("ROUTE_INCOMPLETE_BCV2_MISSING", 0),
            "STATE_NOT_READY_rows_recoverable": class_recoverable.get(STATE_READINESS_CLASS, 0),
            "rows_blocked_by_identity_gap": sum(
                1 for row in not_exec_rows if row["projected_attempt_status"] == "blocked_by_identity_gap"
            ),
            "rows_blocked_by_no_manifest": sum(
                1 for row in not_exec_rows if row["projected_attempt_status"] == "blocked_by_no_manifest"
            ),
            "rows_blocked_by_conflict": sum(
                1 for row in not_exec_rows if row["projected_attempt_status"] == "blocked_by_conflict"
            ),
            "rows_blocked_by_state_readiness": sum(
                1 for row in not_exec_rows if row["projected_attempt_status"] == "blocked_by_state_readiness"
            ),
            "projected_attempted_rows": general_projected_attempted,
            "projected_attempt_coverage": exact_rate(general_projected_attempted, baseline["buy_rows"]),
            "projected_success_rows": baseline["shadow_simulation_success_rows"],
            "projected_success_coverage": baseline["simulation_success_coverage"],
            "projected_gap_to_90": max(0, target_90 - general_projected_attempted),
            "projected_gap_to_95": max(0, target_95 - general_projected_attempted),
            "manifest_reuse_alone_can_reach_95": general_projected_attempted >= target_95,
            "minimal_additional_state_lift_needed_for_95": min(state_needed, len(state_rows)),
            "theoretical_tail_only_attempted_rows": theoretical_tail_attempted,
            "theoretical_tail_only_attempt_coverage": exact_rate(
                theoretical_tail_attempted,
                baseline["buy_rows"],
            ),
            "theoretical_gap_after_all_legacy_tail_missing_fixed": max(
                0,
                target_95 - theoretical_tail_attempted,
            ),
            "lookup_status_counts": common.counter_dict(lookup_counts),
            "projected_attempt_status_counts": common.counter_dict(projected_counts),
        },
        "state_readiness": {
            "state_not_ready_rows": len(state_rows),
            "state_rows_with_diag_update": sum(
                1
                for row in state_audit_rows
                if row["diag_account_update_relay_count"] or row["diag_account_update_applied_count"]
            ),
            "state_rows_without_diag_update": sum(
                1
                for row in state_audit_rows
                if not row["diag_account_update_relay_count"] and not row["diag_account_update_applied_count"]
            ),
            "state_readiness_path_is_separate_from_tail_manifest": True,
        },
        "row_level_explanation_rows": len(projection_rows),
        "not_executable_row_level_explanation_rows": len(not_exec_rows),
        "fail_reasons": sorted(set(fail_reasons)),
        "claim_boundaries": {
            "offline_projection_only": True,
            "active_execution_path_changed": False,
            "send_path_changed": False,
            "gatekeeper_changed": False,
            "observed_manifest_store_can_unlock_execution": False,
            "program_stream_or_raw_manifest_can_unlock_execution": False,
            "success_rows_projected_without_runtime_simulation": False,
            "mint_only_complete_claim": False,
            "can_unlock_execution_true_rows": can_unlock_execution_true_rows,
            "unknown_unclassified_rows": baseline["root_cause_counts"].get("UNKNOWN_UNCLASSIFIED", 0),
        },
        "outputs": outputs,
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    common.write_json(Path(outputs["json"]), report)
    Path(outputs["markdown"]).write_text(build_markdown(report), encoding="utf-8")
    write_jsonl(Path(outputs["manifest_store"]), store)
    write_jsonl(Path(outputs["projection_rows"]), projection_rows)
    write_jsonl(Path(outputs["legacy_tail_rows"]), tail_rows)
    write_jsonl(Path(outputs["state_readiness_audit"]), state_audit_rows)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--decision-plane", default="legacy_live")
    parser.add_argument(
        "--raw-transaction-evidence-glob",
        action="append",
        default=[],
        help="Glob relative to --root for raw_pumpfun_instruction_evidence_v1.jsonl.",
    )
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(f"{ARTIFACT}: {report['status']}")
        print(report["outputs"]["json"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
