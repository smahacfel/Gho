#!/usr/bin/env python3
"""Audit BUY simulation coverage for selector lifecycle-capable runs.

This is an offline diagnostic.  It does not change Gatekeeper, route
resolution, shadow execution, or runtime state.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


ARTIFACT = "buy_simulation_coverage_audit_v1"
DECISION_PLANES = ("legacy_live", "v25_shadow", "auto")
SUCCESS_OUTCOMES = {"shadow_simulated", "closed", "simulated", "simulation_completed"}
CRITICAL_MARKERS = (
    "AccountNotFound",
    "unsupported_legacy_buy_layout_requires_bcv2",
    "Custom(6062)",
    "custom program error: 0x17ae",
    "0x17ae",
    "ResourceExhausted",
    "relative URL without a base",
)
CLASS_ORDER = (
    "POSITION_LIMIT_REACHED",
    "SIM_FAIL_CUSTOM_2006",
    "SIM_FAIL_CUSTOM_6024",
    "SIM_FAIL_CUSTOM_6002",
    "ROUTE_INCOMPLETE_TELEMETRY_ONLY",
    "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING",
    "ROUTE_INCOMPLETE_BCV2_MISSING",
    "ROUTE_INCOMPLETE_STATE_NOT_READY",
    "ROUTE_INCOMPLETE_CREATOR_OR_ACCOUNT_ROLE",
    "SIM_FAIL_TIMEOUT",
    "SIM_FAIL_PROVIDER",
    "UNKNOWN_UNCLASSIFIED",
)
CACHE_CLASS_ORDER = (
    "ROUTE_CACHE_HIT_REUSED",
    "ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST",
    "ROUTE_CACHE_MISS_EXPIRED",
    "ROUTE_CACHE_MISS_CONFLICT",
    "ROUTE_CACHE_MISS_TELEMETRY_ONLY",
)
ROUTE_INCOMPLETE_CLASSES = {
    "ROUTE_INCOMPLETE_TELEMETRY_ONLY",
    "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING",
    "ROUTE_INCOMPLETE_BCV2_MISSING",
    "ROUTE_INCOMPLETE_STATE_NOT_READY",
    "ROUTE_INCOMPLETE_CREATOR_OR_ACCOUNT_ROLE",
}
SIM_FAIL_CLASSES = {
    "SIM_FAIL_CUSTOM_2006",
    "SIM_FAIL_CUSTOM_6024",
    "SIM_FAIL_CUSTOM_6002",
    "SIM_FAIL_TIMEOUT",
    "SIM_FAIL_PROVIDER",
    "UNKNOWN_UNCLASSIFIED",
}
KNOWN_SIM_DIAGNOSTIC_FIELDS = (
    "err",
    "logs_excerpt",
    "logs_digest",
    "units_consumed",
    "retry_count",
    "payer_provenance",
    "simulation_account_manifest",
    "selected_route_account_set_roles",
    "account_manifest_summary",
)


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def sorted_jsonl_paths(root: Path, scope: str, name: str) -> list[Path]:
    base = root / "logs" / "rollout" / scope / "decisions"
    if not base.exists():
        return []
    return sorted(base.rglob(name))


def row_plane(row: dict[str, Any]) -> str:
    return str(row.get("decision_plane") or "legacy_or_unknown")


def plane_match(row: dict[str, Any], decision_plane: str) -> bool:
    return decision_plane == "auto" or row_plane(row) == decision_plane


def load_rows_for_plane(paths: Iterable[Path], decision_plane: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in paths:
        for index, row in enumerate(read_jsonl(path), 1):
            if plane_match(row, decision_plane):
                row = dict(row)
                row["_source_path"] = str(path)
                row["_source_line"] = index
                rows.append(row)
    return rows


def is_buy_decision(row: dict[str, Any]) -> bool:
    return row.get("decision_verdict_buy") is True or str(row.get("verdict_type") or "") == "BUY"


def first_present(*values: Any) -> Any:
    for value in values:
        if value not in (None, "", []):
            return value
    return None


def row_key(row: dict[str, Any]) -> tuple[str | None, str | None, str | None]:
    return (
        common.str_or_none(row.get("pool_id") or row.get("pool_amm_id")),
        common.str_or_none(row.get("base_mint") or row.get("mint_id")),
        common.str_or_none(row.get("ab_record_id") or row.get("join_key")),
    )


def candidate_id_for(row: dict[str, Any], shadow: dict[str, Any] | None = None) -> str | None:
    if shadow:
        value = common.str_or_none(shadow.get("candidate_id"))
        if value:
            return value
    value = common.str_or_none(row.get("candidate_id") or row.get("execution_candidate_id"))
    if value:
        return value
    pool = common.str_or_none(row.get("pool_id") or row.get("pool_amm_id"))
    mint = common.str_or_none(row.get("base_mint") or row.get("mint_id"))
    ts = row.get("decision_ts_ms") or row.get("first_seen_ts_ms") or row.get("ab_t0_event_ts_ms")
    if pool and mint and ts not in (None, ""):
        return f"{mint}_{pool}_{ts}"
    return None


def text_blob(*rows: dict[str, Any] | None) -> str:
    return "\n".join(
        json.dumps(row, ensure_ascii=False, sort_keys=True)
        for row in rows
        if isinstance(row, dict)
    )


def extract_custom_code(text: str) -> str | None:
    match = re.search(r"Custom\((\d+)\)", text)
    return match.group(1) if match else None


def extract_legacy_remaining_count(text: str) -> int | None:
    match = re.search(r"legacy_buy_missing_buyback_remaining_accounts:count=(\d+)", text)
    return int(match.group(1)) if match else None


def extract_position_limit(text: str) -> tuple[int | None, int | None]:
    match = re.search(r"Max concurrent positions reached: active=(\d+), max=(\d+)", text)
    if not match:
        return None, None
    return int(match.group(1)), int(match.group(2))


def provider_failure_present(lower: str) -> bool:
    provider_markers = (
        "resourceexhausted",
        "http 429",
        "too many requests",
        "provider timeout",
        "rpc timeout",
        "transport error",
        "relative url",
        "connection refused",
        "connection reset",
        "service unavailable",
    )
    return any(marker in lower for marker in provider_markers)


def parse_log_field(line: str, key: str) -> str | None:
    match = re.search(rf"{re.escape(key)}=(\"[^\"]+\"|[^,\s]+)", line)
    if not match:
        return None
    return match.group(1).strip('"')


def parse_log_int_field(line: str, key: str) -> int | None:
    value = parse_log_field(line, key)
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def load_route_cache_lookup_index(root: Path, scope: str) -> dict[tuple[str | None, str | None], list[dict[str, Any]]]:
    index: dict[tuple[str | None, str | None], list[dict[str, Any]]] = defaultdict(list)
    for pattern in (
        root / "logs" / "rollout" / scope / "system.log*",
        root / "logs" / "rollout" / scope / "oracle.log*",
    ):
        for path in sorted(pattern.parent.glob(pattern.name)):
            with path.open(encoding="utf-8", errors="ignore") as handle:
                for line_no, line in enumerate(handle, 1):
                    if "ACTIVE_BUY_ROUTE_MANIFEST_CACHE_LOOKUP" not in line:
                        continue
                    row = {
                        "pool_id": parse_log_field(line, "pool"),
                        "base_mint": parse_log_field(line, "base_mint"),
                        "phase": parse_log_field(line, "phase"),
                        "manifest_cache_lookup_status": parse_log_field(
                            line, "manifest_cache_lookup_status"
                        ),
                        "manifest_cache_candidate_count": parse_log_int_field(
                            line, "manifest_cache_candidate_count"
                        ),
                        "prior_complete_legacy_manifest_age_ms": parse_log_int_field(
                            line, "prior_complete_legacy_manifest_age_ms"
                        ),
                        "has_prior_complete_legacy_manifest_in_session": (
                            parse_log_field(line, "has_prior_complete_legacy_manifest_in_session") == "true"
                        ),
                        "route_account_manifest_source": parse_log_field(
                            line, "route_account_manifest_source"
                        ),
                        "log_path": str(path),
                        "log_line": line_no,
                    }
                    index[(row["pool_id"], row["base_mint"])].append(row)
    return index


def route_cache_lookup_for(
    index: dict[tuple[str | None, str | None], list[dict[str, Any]]],
    pool: str | None,
    mint: str | None,
) -> dict[str, Any] | None:
    rows = index.get((pool, mint)) or []
    return rows[-1] if rows else None


def failure_relevant_text(buy: dict[str, Any], shadow: dict[str, Any] | None) -> str:
    fields = (
        "err",
        "error_class",
        "logs_excerpt",
        "program_logs_excerpt",
        "simulation_error_kind",
        "simulation_error_message",
        "precheck_failure_reason",
        "execution_feasibility_reason",
        "route_resolution_terminal_reason",
        "selected_route_reason",
        "legacy_buy_route_not_ready_reason",
        "legacy_buy_account_set_status",
        "route_resolution_status",
        "execution_feasibility_status",
        "dispatch_status",
        "simulation_outcome",
        "shadow_execution_outcome",
        "buy_variant",
        "source",
        "route_account_manifest_source",
        "selected_route_kind",
    )
    payload: dict[str, Any] = {}
    for prefix, row in (("buy", buy), ("shadow", shadow)):
        if not isinstance(row, dict):
            continue
        for field in fields:
            value = row.get(field)
            if value not in (None, "", []):
                payload[f"{prefix}.{field}"] = value
    return json.dumps(payload, ensure_ascii=False, sort_keys=True)


def role_strings(row: dict[str, Any] | None) -> list[str]:
    if not isinstance(row, dict):
        return []
    values: list[str] = []
    for field in (
        "selected_route_account_set_roles",
        "simulation_account_manifest",
        "legacy_buy_required_roles",
        "account_manifest",
        "simulation_account_set_roles",
    ):
        raw = row.get(field)
        if isinstance(raw, list):
            for item in raw:
                if isinstance(item, str):
                    values.append(item)
                elif isinstance(item, dict):
                    role = item.get("role") or item.get("account_role") or item.get("name")
                    pubkey = item.get("pubkey") or item.get("address")
                    values.append(":".join(str(x) for x in (role, pubkey) if x not in (None, "")))
    return values


def role_present(row: dict[str, Any] | None, role: str) -> bool:
    needle = role.lower()
    return any(needle in value.lower() for value in role_strings(row))


def complete_legacy_manifest(row: dict[str, Any]) -> bool:
    if str(row.get("selected_route_kind") or row.get("primary_route_kind") or "") != "legacy_buy":
        return False
    if row.get("execution_feasibility_status") == "not_executable_route":
        return False
    return role_present(row, "buyback_fee_recipient") and role_present(row, "buyback_quote_account")


def complete_manifest_index(shadow_rows: list[dict[str, Any]]) -> dict[tuple[str | None, str | None], list[dict[str, Any]]]:
    by_pool: dict[tuple[str | None, str | None], list[dict[str, Any]]] = defaultdict(list)
    for row in shadow_rows:
        if complete_legacy_manifest(row):
            key = (
                common.str_or_none(row.get("pool_id") or row.get("pool_amm_id")),
                common.str_or_none(row.get("base_mint") or row.get("mint_id")),
            )
            by_pool[key].append(row)
    for rows in by_pool.values():
        rows.sort(key=lambda row: common.int_or_none(row.get("decision_ts_ms")) or 0)
    return by_pool


def prior_complete_manifest(
    index: dict[tuple[str | None, str | None], list[dict[str, Any]]],
    shadow: dict[str, Any] | None,
    buy: dict[str, Any],
) -> tuple[bool, int | None]:
    pool = common.str_or_none(
        first_present(
            shadow.get("pool_id") if shadow else None,
            shadow.get("pool_amm_id") if shadow else None,
            buy.get("pool_id"),
            buy.get("pool_amm_id"),
        )
    )
    mint = common.str_or_none(
        first_present(
            shadow.get("base_mint") if shadow else None,
            shadow.get("mint_id") if shadow else None,
            buy.get("base_mint"),
            buy.get("mint_id"),
        )
    )
    decision_ts = common.int_or_none(
        first_present(
            shadow.get("decision_ts_ms") if shadow else None,
            buy.get("decision_ts_ms"),
            buy.get("first_seen_ts_ms"),
        )
    )
    candidates = index.get((pool, mint), [])
    prior = [
        row
        for row in candidates
        if (common.int_or_none(row.get("decision_ts_ms")) or 0) <= (decision_ts or 0)
        and row is not shadow
    ]
    if not prior:
        return False, None
    if decision_ts is None:
        return True, None
    latest_ts = common.int_or_none(prior[-1].get("decision_ts_ms"))
    return True, decision_ts - latest_ts if latest_ts is not None else None


def load_shadow_rows(root: Path, scope: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    shadow_buys = read_jsonl(root / "logs" / "shadow_run" / f"{scope}-buys.jsonl")
    shadow_entries = read_jsonl(root / "logs" / "shadow_run" / scope / "shadow_entries.jsonl")
    shadow_lifecycle = read_jsonl(root / "logs" / "shadow_run" / scope / "shadow_lifecycle.jsonl")
    return shadow_buys, shadow_entries, shadow_lifecycle


def shadow_dispatch_rows(shadow_buys: list[dict[str, Any]], shadow_lifecycle: list[dict[str, Any]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    # Merge lifecycle dispatch rows with the dedicated shadow BUY artifact when
    # both exist.  The lifecycle file carries dispatch/close status, while the
    # BUY artifact carries richer simulation diagnostics: err, logs_excerpt,
    # error_code/error_detail_class and account manifest fields.
    lifecycle_by_key: dict[str, dict[str, Any]] = {}
    lifecycle_rows = [row for row in shadow_lifecycle if row.get("record_type") == "shadow_dispatch"]
    for row in lifecycle_rows:
        key = common.str_or_none(row.get("ab_record_id")) or common.str_or_none(row.get("candidate_id"))
        if key:
            lifecycle_by_key[key] = row
    consumed_lifecycle_keys: set[str] = set()
    for row in shadow_buys:
        key = common.str_or_none(row.get("ab_record_id")) or common.str_or_none(row.get("candidate_id"))
        if key and key in lifecycle_by_key:
            merged = dict(lifecycle_by_key[key])
            merged.update(row)
            rows.append(merged)
            consumed_lifecycle_keys.add(key)
        else:
            rows.append(row)
    for row in lifecycle_rows:
        key = common.str_or_none(row.get("ab_record_id")) or common.str_or_none(row.get("candidate_id"))
        if key and key in consumed_lifecycle_keys:
            continue
        rows.append(row)
    return rows


def index_shadow_dispatch(rows: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    index: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        for key in (
            common.str_or_none(row.get("ab_record_id")),
            common.str_or_none(row.get("candidate_id")),
            common.str_or_none(row.get("join_key")),
        ):
            if key:
                index[key].append(row)
        pool = common.str_or_none(row.get("pool_id") or row.get("pool_amm_id"))
        mint = common.str_or_none(row.get("base_mint") or row.get("mint_id"))
        decision_ts = common.int_or_none(row.get("decision_ts_ms"))
        if pool and mint and decision_ts is not None:
            index[f"{pool}:{mint}:{decision_ts}"].append(row)
    return index


def find_shadow_for_buy(index: dict[str, list[dict[str, Any]]], buy: dict[str, Any]) -> dict[str, Any] | None:
    keys = [
        common.str_or_none(buy.get("ab_record_id")),
        common.str_or_none(buy.get("candidate_id")),
        common.str_or_none(buy.get("execution_candidate_id")),
        common.str_or_none(buy.get("join_key")),
    ]
    pool = common.str_or_none(buy.get("pool_id") or buy.get("pool_amm_id"))
    mint = common.str_or_none(buy.get("base_mint") or buy.get("mint_id"))
    decision_ts = common.int_or_none(buy.get("decision_ts_ms"))
    if pool and mint and decision_ts is not None:
        keys.append(f"{pool}:{mint}:{decision_ts}")
    for key in keys:
        if key and index.get(key):
            return index[key][0]
    return None


def classify_failure(buy: dict[str, Any], shadow: dict[str, Any] | None) -> tuple[str, list[str]]:
    text = failure_relevant_text(buy, shadow)
    lower = text.lower()
    matches: list[str] = []

    if "max concurrent positions reached" in lower or buy.get("shadow_execution_outcome") == "shadow_position_limit_reached":
        matches.append("POSITION_LIMIT_REACHED")
    code = extract_custom_code(text)
    if code == "2006":
        matches.append("SIM_FAIL_CUSTOM_2006")
    if code == "6024":
        matches.append("SIM_FAIL_CUSTOM_6024")
    if code == "6002":
        matches.append("SIM_FAIL_CUSTOM_6002")
    if "timeout" in lower or "timed out" in lower:
        matches.append("SIM_FAIL_TIMEOUT")
    if provider_failure_present(lower):
        matches.append("SIM_FAIL_PROVIDER")
    if "telemetry_only" in lower or "feature-only" in lower or "feature_only" in lower:
        matches.append("ROUTE_INCOMPLETE_TELEMETRY_ONLY")
    if "legacy_buy_missing_buyback_remaining_accounts" in lower:
        matches.append("ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING")
    if "primary_route_bcv2_missing" in lower or "missing_bonding_curve_v2" in lower:
        matches.append("ROUTE_INCOMPLETE_BCV2_MISSING")
    if "simulation_load_not_ready" in lower or "legacy_buy_simulation_load_not_ready" in lower:
        matches.append("ROUTE_INCOMPLETE_STATE_NOT_READY")
    if any(
        marker in lower
        for marker in (
            "route_account_manifest_incomplete",
            "missing_creator",
            "missing_global_config",
            "missing_fee_recipient",
            "missing_token_program",
            "missing_associated_bonding_curve",
            "missing_account_role",
        )
    ):
        matches.append("ROUTE_INCOMPLETE_CREATOR_OR_ACCOUNT_ROLE")

    for klass in CLASS_ORDER:
        if klass in matches:
            return klass, [item for item in matches if item != klass]
    return "UNKNOWN_UNCLASSIFIED", []


def simulation_success(buy: dict[str, Any], shadow: dict[str, Any] | None) -> bool:
    values = {
        str(buy.get("shadow_execution_outcome") or ""),
        str(shadow.get("dispatch_status") if shadow else ""),
        str(shadow.get("simulation_outcome") if shadow else ""),
        str(shadow.get("classification") if shadow else ""),
    }
    return bool(values & SUCCESS_OUTCOMES)


def simulation_attempted(shadow: dict[str, Any] | None) -> bool:
    if not shadow:
        return False
    if shadow.get("simulation_attempted") is True:
        return True
    return shadow.get("simulation_outcome") in {"closed", "failed"} or shadow.get("dispatch_status") in {"closed", "failed"}


def dispatch_attempted(shadow: dict[str, Any] | None) -> bool:
    if not shadow:
        return False
    if shadow.get("dispatch_attempted") is True:
        return True
    return shadow.get("dispatch_status") in {"closed", "failed"}


def is_not_executable(shadow: dict[str, Any] | None) -> bool:
    if not shadow:
        return False
    return (
        shadow.get("execution_feasibility_status") == "not_executable_route"
        or shadow.get("route_resolution_status") == "no_executable_route_account_set"
        or shadow.get("dispatch_status") == "not_dispatched"
    )


def is_failed(buy: dict[str, Any], shadow: dict[str, Any] | None) -> bool:
    if shadow and (shadow.get("simulation_outcome") == "failed" or shadow.get("dispatch_status") == "failed"):
        return True
    return str(buy.get("shadow_execution_outcome") or "") not in {"", "shadow_simulated"}


def error_text(buy: dict[str, Any], shadow: dict[str, Any] | None) -> str | None:
    value = first_present(
        shadow.get("err") if shadow else None,
        shadow.get("simulation_error_message") if shadow else None,
        shadow.get("precheck_failure_reason") if shadow else None,
        buy.get("shadow_execution_outcome"),
    )
    return str(value) if value not in (None, "") else None


def latch_eligibility_checked(row: dict[str, Any] | None) -> bool:
    if not isinstance(row, dict):
        return False
    return (
        row.get("state_latch_eligibility_checked") is True
        or row.get("state_latch_eligibility_marker") == "STATE_LATCH_ELIGIBILITY_CHECKED"
    )


def latch_attempted(row: dict[str, Any] | None) -> bool:
    return bool(isinstance(row, dict) and row.get("state_latch_attempted") is True)


def latch_skipped(row: dict[str, Any] | None) -> bool:
    if not isinstance(row, dict):
        return False
    skip_reason = common.str_or_none(row.get("state_latch_skip_reason"))
    outcome = common.str_or_none(row.get("state_latch_outcome"))
    return bool(skip_reason and skip_reason.startswith("STATE_LATCH_SKIPPED_")) or bool(
        outcome and outcome.startswith("STATE_LATCH_SKIPPED_")
    )


def sample_row(
    *,
    buy: dict[str, Any],
    shadow: dict[str, Any] | None,
    classification: str,
    secondary: list[str],
    manifest_index: dict[tuple[str | None, str | None], list[dict[str, Any]]],
    route_cache_lookup_index: dict[tuple[str | None, str | None], list[dict[str, Any]]],
) -> dict[str, Any]:
    text = text_blob(buy, shadow)
    active_positions, max_positions = extract_position_limit(text)
    has_prior_manifest, prior_age_ms = prior_complete_manifest(manifest_index, shadow, buy)
    route_manifest_source = first_present(
        shadow.get("selected_route_source") if shadow else None,
        shadow.get("legacy_buy_curve_source") if shadow else None,
        shadow.get("bonding_curve_v2_source") if shadow else None,
    )
    amount_lamports = common.int_or_none(shadow.get("amount_lamports")) if shadow else None
    retry_count = common.int_or_none(shadow.get("retry_count")) if shadow else None
    pool_id = first_present(
        shadow.get("pool_id") if shadow else None,
        shadow.get("pool_amm_id") if shadow else None,
        buy.get("pool_id"),
        buy.get("pool_amm_id"),
    )
    base_mint = first_present(
        shadow.get("base_mint") if shadow else None,
        shadow.get("mint_id") if shadow else None,
        buy.get("base_mint"),
        buy.get("mint_id"),
    )
    cache_lookup = route_cache_lookup_for(
        route_cache_lookup_index,
        common.str_or_none(pool_id),
        common.str_or_none(base_mint),
    )
    return {
        "classification": classification,
        "secondary_classifications": secondary,
        "candidate_id": candidate_id_for(buy, shadow),
        "pool_id": pool_id,
        "base_mint": base_mint,
        "ab_record_id": first_present(shadow.get("ab_record_id") if shadow else None, buy.get("ab_record_id")),
        "buy_variant": first_present(
            shadow.get("selected_route_kind") if shadow else None,
            shadow.get("primary_route_kind") if shadow else None,
            shadow.get("observed_bcv2_source_buy_variant") if shadow else None,
        ),
        "source": route_manifest_source,
        "selected_route_kind": shadow.get("selected_route_kind") if shadow else None,
        "route_resolution_status": shadow.get("route_resolution_status") if shadow else None,
        "execution_feasibility_status": shadow.get("execution_feasibility_status") if shadow else None,
        "execution_feasibility_reason": shadow.get("execution_feasibility_reason") if shadow else None,
        "dispatch_status": shadow.get("dispatch_status") if shadow else None,
        "simulation_outcome": shadow.get("simulation_outcome") if shadow else None,
        "shadow_execution_outcome": buy.get("shadow_execution_outcome"),
        "dispatch_attempted": dispatch_attempted(shadow),
        "simulation_attempted": simulation_attempted(shadow),
        "error_text": error_text(buy, shadow),
        "instruction_error": f"Custom({extract_custom_code(text)})" if extract_custom_code(text) else None,
        "program_logs_excerpt": shadow.get("logs_excerpt") if shadow else None,
        "amount_sol": amount_lamports / 1_000_000_000 if amount_lamports is not None else None,
        "slippage_bps_or_pct": first_present(
            shadow.get("slippage_bps") if shadow else None,
            shadow.get("slippage_pct") if shadow else None,
        ),
        "min_out_or_max_sol": first_present(
            shadow.get("min_out") if shadow else None,
            shadow.get("max_sol_cost") if shadow else None,
        ),
        "quote_age_ms": shadow.get("quote_age_ms") if shadow else None,
        "curve_snapshot_age_ms": shadow.get("curve_snapshot_age_ms") if shadow else None,
        "recent_blockhash_age_ms": shadow.get("recent_blockhash_age_ms") if shadow else None,
        "payer_strategy": first_present(shadow.get("payer_strategy") if shadow else None, shadow.get("payer_provenance") if shadow else None),
        "pre_balance_status": shadow.get("pre_balance_status") if shadow else None,
        "account_contract_status": shadow.get("execution_feasibility_status") if shadow else None,
        "route_account_manifest_source": route_manifest_source,
        "retry_attempted": bool(retry_count and retry_count > 0),
        "retry_result": shadow.get("retry_result") if shadow else None,
        "active_positions": active_positions,
        "max_concurrent_positions": max_positions,
        "entry_simulation_skipped_entirely": not simulation_attempted(shadow),
        "legacy_buy_account_set_status": shadow.get("legacy_buy_account_set_status") if shadow else None,
        "legacy_buy_remaining_account_count": extract_legacy_remaining_count(text),
        "associated_bonding_curve_present": role_present(shadow, "associated_bonding_curve")
        or bool(shadow and shadow.get("legacy_buy_associated_bonding_curve_pubkey")),
        "global_config_present": role_present(shadow, "global_config"),
        "fee_recipient_present": role_present(shadow, "fee_recipient"),
        "token_program_present": role_present(shadow, "token_program"),
        "creator_pubkey_authoritative": shadow.get("creator_pubkey_authoritative") if shadow else None,
        "bonding_curve_v2_present": bool(shadow and shadow.get("bonding_curve_v2_pubkey")),
        "primary_route_bcv2_missing": "primary_route_bcv2_missing" in text,
        "has_prior_complete_legacy_manifest_in_session": has_prior_manifest,
        "prior_complete_legacy_manifest_age_ms": prior_age_ms,
        "manifest_cache_lookup_status": cache_lookup.get("manifest_cache_lookup_status") if cache_lookup else None,
        "manifest_cache_candidate_count": cache_lookup.get("manifest_cache_candidate_count") if cache_lookup else None,
        "manifest_cache_prior_complete_legacy_manifest_age_ms": (
            cache_lookup.get("prior_complete_legacy_manifest_age_ms") if cache_lookup else None
        ),
        "manifest_cache_has_prior_complete_legacy_manifest_in_session": (
            cache_lookup.get("has_prior_complete_legacy_manifest_in_session") if cache_lookup else None
        ),
        "manifest_cache_route_account_manifest_source": (
            cache_lookup.get("route_account_manifest_source") if cache_lookup else None
        ),
        "manifest_cache_lookup_phase": cache_lookup.get("phase") if cache_lookup else None,
        "state_latch_eligibility_marker": shadow.get("state_latch_eligibility_marker") if shadow else None,
        "state_latch_eligibility_checked": shadow.get("state_latch_eligibility_checked") if shadow else None,
        "state_latch_enabled": shadow.get("state_latch_enabled") if shadow else None,
        "state_latch_normalized_error_class": shadow.get("state_latch_normalized_error_class") if shadow else None,
        "state_latch_eligible": shadow.get("state_latch_eligible") if shadow else None,
        "state_latch_skip_reason": shadow.get("state_latch_skip_reason") if shadow else None,
        "state_latch_attempted": shadow.get("state_latch_attempted") if shadow else None,
        "state_latch_outcome": shadow.get("state_latch_outcome") if shadow else None,
        "state_latch_wait_ms": shadow.get("state_latch_wait_ms") if shadow else None,
        "state_latch_state_before": shadow.get("state_latch_state_before") if shadow else None,
        "state_latch_state_after": shadow.get("state_latch_state_after") if shadow else None,
        "raw_shadow_source_line": shadow.get("_source_line") if shadow else None,
        "raw_buy_source_line": buy.get("_source_line"),
    }


def count_marker_occurrences(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for row in rows:
        text = text_blob(row)
        for marker in CRITICAL_MARKERS:
            counts[marker] += text.count(marker)
    for pattern in (
        root / "logs" / "rollout" / scope / "system.log*",
        root / "logs" / "rollout" / scope / "oracle.log*",
    ):
        for path in sorted(pattern.parent.glob(pattern.name)):
            with path.open(encoding="utf-8", errors="ignore") as handle:
                for line in handle:
                    for marker in CRITICAL_MARKERS:
                        counts[marker] += line.count(marker)
    return {marker: counts.get(marker, 0) for marker in CRITICAL_MARKERS}


def count_marker_rows(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for row in rows:
        text = text_blob(row)
        for marker in CRITICAL_MARKERS:
            if marker in text:
                counts[marker] += 1
    for pattern in (
        root / "logs" / "rollout" / scope / "system.log*",
        root / "logs" / "rollout" / scope / "oracle.log*",
    ):
        for path in sorted(pattern.parent.glob(pattern.name)):
            with path.open(encoding="utf-8", errors="ignore") as handle:
                for line in handle:
                    for marker in CRITICAL_MARKERS:
                        if marker in line:
                            counts[marker] += 1
    return {marker: counts.get(marker, 0) for marker in CRITICAL_MARKERS}


def build_markdown(report: dict[str, Any]) -> str:
    metrics = report["metrics"]
    lines = [
        "# BUY Simulation Coverage Audit",
        "",
        f"- scope: `{report['scope']}`",
        f"- decision_plane: `{report['decision_plane']}`",
        f"- status: `{report['status']}`",
        f"- buy_rows: `{metrics['buy_rows']}`",
        f"- simulation_success_coverage: `{metrics['simulation_success_coverage']:.6f}`",
        f"- simulation_attempt_coverage: `{metrics['simulation_attempt_coverage']:.6f}`",
        f"- not_executable_rate: `{metrics['not_executable_rate']:.6f}`",
        f"- simulation_failure_rate: `{metrics['simulation_failure_rate']:.6f}`",
        f"- position_limit_rate: `{metrics['position_limit_rate']:.6f}`",
        "",
        "## Failure Classes",
        "",
        "| class | count | rate |",
        "|---|---:|---:|",
    ]
    for klass, payload in report["failure_classes"].items():
        lines.append(f"| `{klass}` | {payload['count']} | {payload['rate']:.6f} |")
    cache = report.get("route_manifest_cache") or {}
    cache_classes = cache.get("classes") or {}
    lines.extend(
        [
            "",
            "## Route Manifest Cache",
            "",
            f"- lookup_rows: `{cache.get('lookup_rows', 0)}`",
            "",
            "| cache_status | count | rate |",
            "|---|---:|---:|",
        ]
    )
    if cache_classes:
        for klass, payload in cache_classes.items():
            lines.append(f"| `{klass}` | {payload['count']} | {payload['rate']:.6f} |")
    else:
        lines.append("| `none` | 0 | 0.000000 |")
    latch = report.get("state_latch_contract") or {}
    lines.extend(
        [
            "",
            "## State Latch Contract",
            "",
            f"- contract_status: `{latch.get('contract_status', 'UNKNOWN')}`",
            f"- state_not_ready_rows: `{latch.get('state_not_ready_rows', 0)}`",
            f"- state_latch_eligibility_checked_rows: `{latch.get('state_latch_eligibility_checked_rows', 0)}`",
            f"- state_latch_attempted_rows: `{latch.get('state_latch_attempted_rows', 0)}`",
            f"- state_latch_skipped_rows: `{latch.get('state_latch_skipped_rows', 0)}`",
            f"- state_not_ready_latch_marker_missing_rows: `{latch.get('state_not_ready_latch_marker_missing_rows', 0)}`",
            "",
            "| outcome | count |",
            "|---|---:|",
        ]
    )
    outcomes = latch.get("state_latch_outcomes") or {}
    if outcomes:
        for outcome, count in outcomes.items():
            lines.append(f"| `{outcome}` | {count} |")
    else:
        lines.append("| `none` | 0 |")
    lines.extend(
        [
            "",
            "## Critical Markers",
            "",
            "| marker | row_count | occurrence_count |",
            "|---|---:|---:|",
        ]
    )
    for marker, count in report["critical_regression_markers"].items():
        occurrences = report.get("critical_regression_marker_occurrences", {}).get(marker, 0)
        lines.append(f"| `{marker}` | {count} | {occurrences} |")
    lines.extend(
        [
            "",
            "## Claim Boundaries",
            "",
            "- offline audit only: `true`",
            "- runtime changed: `false`",
            "- r8 remains R2/GK-feature scope only: `true`",
            "- every-BUY lifecycle claim: `false`",
        ]
    )
    if report.get("fail_reasons"):
        lines.extend(["", "## Fail Reasons", ""])
        for reason in report["fail_reasons"]:
            lines.append(f"- `{reason}`")
    return "\n".join(lines) + "\n"


def build_audit(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    output_dir = root / "reports" / "selector" / args.scope
    decision_paths = sorted_jsonl_paths(root, args.scope, "gatekeeper_v2_decisions.jsonl")
    buy_paths = sorted_jsonl_paths(root, args.scope, "gatekeeper_v2_buys.jsonl")
    decisions = load_rows_for_plane(decision_paths, args.decision_plane)
    decision_buy_rows = [row for row in decisions if is_buy_decision(row)]
    buy_rows = load_rows_for_plane(buy_paths, args.decision_plane)
    if args.decision_plane != "legacy_live" and not buy_rows:
        # v25_shadow often has no BUY-only file.  Keep this explicit instead
        # of silently changing the denominator.
        buy_rows = []

    shadow_buys, shadow_entries, shadow_lifecycle = load_shadow_rows(root, args.scope)
    dispatch_rows = shadow_dispatch_rows(shadow_buys, shadow_lifecycle)
    for index, row in enumerate(dispatch_rows, 1):
        row["_source_line"] = index
    dispatch_index = index_shadow_dispatch(dispatch_rows)
    manifest_index = complete_manifest_index(dispatch_rows)
    route_cache_lookup_index = load_route_cache_lookup_index(root, args.scope)

    decision_buy_keys = Counter(row_key(row) for row in decision_buy_rows)
    buy_file_keys = Counter(row_key(row) for row in buy_rows)
    missing_from_buy_file = sum(max(0, count - buy_file_keys.get(key, 0)) for key, count in decision_buy_keys.items())
    buy_logging_coverage = (
        (len(decision_buy_rows) - missing_from_buy_file) / len(decision_buy_rows)
        if decision_buy_rows
        else 0.0
    )

    samples: list[dict[str, Any]] = []
    class_counts: Counter[str] = Counter()
    route_cache_class_counts: Counter[str] = Counter()
    shadow_dispatch_row_count = 0
    shadow_simulated_rows = 0
    shadow_closed_rows = 0
    simulation_attempted_rows = 0
    not_executable_rows = 0
    simulation_failed_rows = 0
    position_limit_rows = 0
    diagnostic_missing_fields: Counter[str] = Counter()
    joined_shadow_rows: list[dict[str, Any]] = []
    latch_eligibility_checked_rows = 0
    latch_attempted_rows = 0
    latch_skipped_rows = 0
    state_not_ready_rows = 0
    state_not_ready_latch_marker_missing_rows = 0
    state_latch_outcomes: Counter[str] = Counter()
    state_latch_skip_reasons: Counter[str] = Counter()

    for buy in buy_rows:
        shadow = find_shadow_for_buy(dispatch_index, buy)
        if shadow:
            joined_shadow_rows.append(shadow)
            shadow_dispatch_row_count += 1
            if latch_eligibility_checked(shadow):
                latch_eligibility_checked_rows += 1
            if latch_attempted(shadow):
                latch_attempted_rows += 1
            if latch_skipped(shadow):
                latch_skipped_rows += 1
            if shadow.get("state_latch_outcome") not in (None, ""):
                state_latch_outcomes[str(shadow.get("state_latch_outcome"))] += 1
            if shadow.get("state_latch_skip_reason") not in (None, ""):
                state_latch_skip_reasons[str(shadow.get("state_latch_skip_reason"))] += 1
        success = simulation_success(buy, shadow)
        if success:
            shadow_simulated_rows += 1
        if shadow and shadow.get("dispatch_status") == "closed":
            shadow_closed_rows += 1
        if simulation_attempted(shadow):
            simulation_attempted_rows += 1
        if is_not_executable(shadow):
            not_executable_rows += 1
        failed = is_failed(buy, shadow) and not success
        if failed and not is_not_executable(shadow):
            simulation_failed_rows += 1
        classification: str | None = None
        secondary: list[str] = []
        if not success:
            classification, secondary = classify_failure(buy, shadow)
            class_counts[classification] += 1
            state_not_ready = (
                classification == "ROUTE_INCOMPLETE_STATE_NOT_READY"
                or "ROUTE_INCOMPLETE_STATE_NOT_READY" in secondary
            )
            if state_not_ready:
                state_not_ready_rows += 1
                if not latch_eligibility_checked(shadow):
                    state_not_ready_latch_marker_missing_rows += 1
            if classification == "POSITION_LIMIT_REACHED":
                position_limit_rows += 1
            elif "max concurrent positions reached" in text_blob(buy, shadow).lower():
                position_limit_rows += 1
            samples.append(
                sample_row(
                    buy=buy,
                    shadow=shadow,
                    classification=classification,
                    secondary=secondary,
                    manifest_index=manifest_index,
                    route_cache_lookup_index=route_cache_lookup_index,
                )
            )
            cache_status = samples[-1].get("manifest_cache_lookup_status")
            if cache_status in CACHE_CLASS_ORDER:
                route_cache_class_counts[str(cache_status)] += 1
            if classification in SIM_FAIL_CLASSES:
                for field in KNOWN_SIM_DIAGNOSTIC_FIELDS:
                    if not shadow or shadow.get(field) in (None, "", []):
                        diagnostic_missing_fields[field] += 1

    buy_count = len(buy_rows)
    def rate(value: int) -> float:
        return value / buy_count if buy_count else 0.0

    failure_classes = {
        klass: {
            "count": class_counts.get(klass, 0),
            "rate": rate(class_counts.get(klass, 0)),
            "sample_candidate_ids": [
                sample.get("candidate_id")
                for sample in samples
                if sample.get("classification") == klass and sample.get("candidate_id")
            ][:10],
        }
        for klass in CLASS_ORDER
        if class_counts.get(klass, 0) > 0 or klass == "UNKNOWN_UNCLASSIFIED"
    }
    route_manifest_cache_classes = {
        klass: {
            "count": route_cache_class_counts.get(klass, 0),
            "rate": rate(route_cache_class_counts.get(klass, 0)),
            "sample_candidate_ids": [
                sample.get("candidate_id")
                for sample in samples
                if sample.get("manifest_cache_lookup_status") == klass and sample.get("candidate_id")
            ][:10],
        }
        for klass in CACHE_CLASS_ORDER
        if route_cache_class_counts.get(klass, 0) > 0
    }

    critical_rows = joined_shadow_rows + buy_rows
    marker_occurrence_counts = count_marker_occurrences(root, args.scope, critical_rows)
    marker_row_counts = count_marker_rows(root, args.scope, critical_rows)
    fail_reasons: list[str] = []
    if decision_buy_rows and missing_from_buy_file:
        fail_reasons.append("buy_decisions_missing_from_buy_file")
    unknown_rate = rate(class_counts.get("UNKNOWN_UNCLASSIFIED", 0))
    if unknown_rate > args.max_unknown_rate:
        fail_reasons.append("unknown_unclassified_rate_above_limit")
    if simulation_failed_rows and diagnostic_missing_fields:
        fail_reasons.append("INSUFFICIENT_SIMULATION_DIAGNOSTICS")
    if state_not_ready_latch_marker_missing_rows:
        fail_reasons.append("STATE_LATCH_MARKER_MISSING_FOR_STATE_NOT_READY")
    for marker, count in marker_row_counts.items():
        if marker in ("AccountNotFound", "unsupported_legacy_buy_layout_requires_bcv2", "Custom(6062)", "0x17ae", "ResourceExhausted", "relative URL without a base") and count > 0:
            fail_reasons.append(f"critical_marker_present:{marker}")

    status = "AUDIT_COMPLETE_WITH_FINDINGS" if fail_reasons or samples else "AUDIT_COMPLETE_PASS"
    outputs = {
        "json": str(output_dir / f"{ARTIFACT}.json"),
        "markdown": str(output_dir / "BUY_SIMULATION_COVERAGE_AUDIT.md"),
        "samples": str(output_dir / "buy_simulation_failure_samples_v1.jsonl"),
    }
    report = {
        "artifact": ARTIFACT,
        "schema_version": 1,
        "status": status,
        "scope": args.scope,
        "decision_plane": args.decision_plane,
        "decision_log_paths": [str(path) for path in decision_paths],
        "buy_log_paths": [str(path) for path in buy_paths],
        "shadow_artifacts": {
            "shadow_buys": str(root / "logs" / "shadow_run" / f"{args.scope}-buys.jsonl"),
            "shadow_entries": str(root / "logs" / "shadow_run" / args.scope / "shadow_entries.jsonl"),
            "shadow_lifecycle": str(root / "logs" / "shadow_run" / args.scope / "shadow_lifecycle.jsonl"),
        },
        "metrics": {
            "decision_buy_rows": len(decision_buy_rows),
            "buy_rows": buy_count,
            "buy_only_rows": buy_count,
            "buy_missing_from_buy_file": missing_from_buy_file,
            "shadow_dispatch_rows": shadow_dispatch_row_count,
            "shadow_simulated_rows": shadow_simulated_rows,
            "shadow_closed_rows": shadow_closed_rows,
            "not_executable_route_rows": not_executable_rows,
            "simulation_failed_rows": simulation_failed_rows,
            "position_limit_rows": position_limit_rows,
            "critical_regression_marker_rows": sum(1 for row in critical_rows if any(marker in text_blob(row) for marker in CRITICAL_MARKERS)),
            "buy_logging_coverage": buy_logging_coverage,
            "simulation_attempt_coverage": rate(simulation_attempted_rows),
            "simulation_success_coverage": rate(shadow_simulated_rows),
            "not_executable_rate": rate(not_executable_rows),
            "simulation_failure_rate": rate(simulation_failed_rows),
            "position_limit_rate": rate(position_limit_rows),
        },
        "failure_classes": failure_classes,
        "route_manifest_cache": {
            "lookup_rows": sum(len(rows) for rows in route_cache_lookup_index.values()),
            "classes": route_manifest_cache_classes,
        },
        "state_latch_contract": {
            "state_not_ready_rows": state_not_ready_rows,
            "state_latch_eligibility_checked_rows": latch_eligibility_checked_rows,
            "state_latch_attempted_rows": latch_attempted_rows,
            "state_latch_skipped_rows": latch_skipped_rows,
            "state_latch_attempted_plus_skipped_rows": latch_attempted_rows + latch_skipped_rows,
            "state_not_ready_latch_marker_missing_rows": state_not_ready_latch_marker_missing_rows,
            "state_latch_outcomes": common.counter_dict(state_latch_outcomes),
            "state_latch_skip_reasons": common.counter_dict(state_latch_skip_reasons),
            "contract_status": (
                "PASS"
                if state_not_ready_latch_marker_missing_rows == 0
                and latch_attempted_rows + latch_skipped_rows == latch_eligibility_checked_rows
                else "FAIL"
            ),
        },
        "critical_regression_markers": marker_row_counts,
        "critical_regression_marker_occurrences": marker_occurrence_counts,
        "simulation_diagnostics": {
            "status": "INSUFFICIENT_SIMULATION_DIAGNOSTICS" if diagnostic_missing_fields else "AVAILABLE",
            "missing_field_counts": common.counter_dict(diagnostic_missing_fields),
        },
        "fail_reasons": sorted(set(fail_reasons)),
        "claim_boundaries": {
            "offline_audit_only": True,
            "runtime_changed": False,
            "r8_r2_gk_feature_scope_valid": args.scope.endswith("r8-feature-rich-r2diag"),
            "every_buy_simulation_claim": False,
            "clean_full_lifecycle_dataset_claim": False,
            "production_readiness": False,
            "gatekeeper_tuning": False,
        },
        "outputs": outputs,
    }
    common.write_json(Path(outputs["json"]), report)
    common.write_jsonl(Path(outputs["samples"]), samples)
    Path(outputs["markdown"]).parent.mkdir(parents=True, exist_ok=True)
    Path(outputs["markdown"]).write_text(build_markdown(report), encoding="utf-8")
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--decision-plane", choices=DECISION_PLANES, default="legacy_live")
    parser.add_argument("--max-unknown-rate", type=float, default=0.10)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_audit(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
