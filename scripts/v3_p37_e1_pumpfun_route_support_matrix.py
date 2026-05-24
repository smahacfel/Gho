#!/usr/bin/env python3
"""Build the P3.7-E1 Pump.fun executable route support matrix.

E1 is an offline route-support decision audit. It does not call RPC and it does
not run shadow simulation. The input is the route resolver / execution
feasibility evidence already emitted by the R17 shadow/probe artifacts.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1
DEFAULT_SHADOW_ROOT = Path(
    "/root/Gho-r17-clean/logs/shadow_run/"
    "shadow-burnin-v3-p37-r17-replay-ready-diagnostic"
)
DEFAULT_DECISION_ROOT = Path(
    "/root/Gho-r17-clean/logs/rollout/"
    "shadow-burnin-v3-p37-r17-replay-ready-diagnostic/decisions"
)
DEFAULT_MD_OUTPUT = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E1_PUMPFUN_EXECUTABLE_ROUTE_SUPPORT_MATRIX_20260524.md"
)
DEFAULT_JSON_OUTPUT = Path(
    "PLANS/AUDYT/RAPORT_P3_7_E1_PUMPFUN_EXECUTABLE_ROUTE_SUPPORT_MATRIX_20260524.json"
)

ARTIFACT_FILES = {
    "active_shadow_transport": "buys.jsonl",
    "active_shadow_entry": "shadow_entries.jsonl",
    "active_shadow_lifecycle": "shadow_lifecycle.jsonl",
    "probe_selection": "probe_selection.jsonl",
    "probe_skip": "probe_skips.jsonl",
    "probe_transport": "probe_transport.jsonl",
    "probe_entry": "probe_shadow_entries.jsonl",
    "probe_lifecycle": "probe_shadow_lifecycle.jsonl",
}
DECISION_FILES = {
    "seer_runtime_coverage": "seer_runtime_coverage_audit.jsonl",
}
EXECUTABLE_ROUTE_STATUSES = {"primary_route_ready", "fallback_route_ready"}
NON_EXECUTABLE_ROUTE_STATUSES = {"no_executable_route_account_set"}


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            raw = line.strip()
            if not raw:
                continue
            idx = 0
            while idx < len(raw):
                try:
                    obj, end = decoder.raw_decode(raw, idx)
                except json.JSONDecodeError:
                    break
                if isinstance(obj, dict):
                    yield obj
                idx = end
                while idx < len(raw) and raw[idx].isspace():
                    idx += 1


def row_string(row: dict[str, Any], field: str) -> str | None:
    value = row.get(field)
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    text = str(value).strip()
    return text or None


def row_bool(row: dict[str, Any], field: str) -> bool | None:
    value = row.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in {"true", "1", "yes"}:
            return True
        if lowered in {"false", "0", "no"}:
            return False
    return None


def row_string_list(row: dict[str, Any], field: str) -> list[str]:
    value = row.get(field)
    if isinstance(value, list):
        return [str(item) for item in value if str(item).strip()]
    if isinstance(value, str) and value.strip():
        return [value.strip()]
    return []


def parse_account_set_entry(value: str) -> dict[str, str]:
    parts = value.split(":")
    return {
        "role": parts[0] if len(parts) > 0 else "",
        "pubkey": parts[1] if len(parts) > 1 else "",
        "source": parts[2] if len(parts) > 2 else "",
    }


def route_state() -> dict[str, Any]:
    return {
        "evidence_row_ids": set(),
        "decision_ids": set(),
        "source_plane_counts": Counter(),
        "observation_kind_counts": Counter(),
        "route_resolution_status_counts": Counter(),
        "selected_route_kind_counts": Counter(),
        "program_id_counts": Counter(),
        "instruction_discriminator_counts": Counter(),
        "account_count_values": Counter(),
        "instruction_account_positions": set(),
        "message_account_indices": set(),
        "loaded_address_usage": Counter(),
        "account_index_to_role_map": defaultdict(Counter),
        "account_roles": Counter(),
        "account_sources": Counter(),
        "required_simulation_load_accounts": set(),
        "creatable_accounts": set(),
        "required_precheck_accounts": set(),
        "builder_support_signals": Counter(),
        "prepared_request_support_signals": Counter(),
        "rpc_load_ready_true": 0,
        "rpc_load_ready_false": 0,
        "readiness_reason_counts": Counter(),
        "shadow_simulation_support_counts": Counter(),
        "primary_failure_class_counts": Counter(),
        "missing_role_counts": Counter(),
        "missing_pubkey_counts": Counter(),
    }


def decision_id(row: dict[str, Any], fallback: str) -> str:
    for field in ("ab_record_id", "source_ab_record_id", "probe_id", "candidate_id"):
        value = row_string(row, field)
        if value:
            return value
    pool = row_string(row, "pool_id") or row_string(row, "pool_amm_id") or ""
    mint = row_string(row, "base_mint") or row_string(row, "mint_id") or row_string(row, "mint") or ""
    return f"{pool}:{mint}:{fallback}"


def add_common_route_fields(
    stats: dict[str, Any],
    route: str,
    row: dict[str, Any],
    artifact: str,
    line_no: int,
    observation_kind: str,
) -> None:
    stats["evidence_row_ids"].add(f"{artifact}:{line_no}:{observation_kind}:{route}")
    stats["decision_ids"].add(decision_id(row, f"{artifact}:{line_no}"))
    stats["source_plane_counts"][artifact] += 1
    stats["observation_kind_counts"][observation_kind] += 1

    if status := row_string(row, "route_resolution_status"):
        stats["route_resolution_status_counts"][status] += 1
    if selected := row_string(row, "selected_route_kind"):
        stats["selected_route_kind_counts"][selected] += 1
        if selected == route:
            stats["builder_support_signals"]["selected_route"] += 1
            stats["shadow_simulation_support_counts"]["selected_executable_route"] += 1
    if count := row_string(row, "prepared_request_account_set_count"):
        stats["account_count_values"][count] += 1
    if count := row_string(row, "simulation_account_set_count"):
        stats["account_count_values"][count] += 1


def add_readiness(
    stats: dict[str, Any],
    ready: bool | None,
    reason: str | None,
    support_signal: str,
) -> None:
    if ready is True:
        stats["rpc_load_ready_true"] += 1
        stats["builder_support_signals"][support_signal] += 1
        stats["shadow_simulation_support_counts"]["route_ready"] += 1
    elif ready is False:
        stats["rpc_load_ready_false"] += 1
        stats["shadow_simulation_support_counts"]["route_not_ready"] += 1
    if reason:
        stats["readiness_reason_counts"][reason] += 1
        stats["primary_failure_class_counts"][reason] += 1


def add_account_set_entries(
    stats: dict[str, Any],
    values: list[str],
    target: str,
) -> None:
    for value in values:
        entry = parse_account_set_entry(value)
        role = entry["role"]
        pubkey = entry["pubkey"]
        source = entry["source"]
        if role:
            if target == "simulation_load":
                stats["required_simulation_load_accounts"].add(f"{role}:{pubkey}:{source}")
            elif target == "creatable":
                stats["creatable_accounts"].add(f"{role}:{pubkey}:{source}")
            elif target == "precheck":
                stats["required_precheck_accounts"].add(f"{role}:{pubkey}:{source}")
            stats["account_roles"][role] += 1
        if source:
            stats["account_sources"][source] += 1


def add_manifest(stats: dict[str, Any], row: dict[str, Any]) -> None:
    manifest = row.get("simulation_account_manifest")
    if not isinstance(manifest, list):
        return
    stats["prepared_request_support_signals"]["simulation_account_manifest"] += 1
    stats["account_count_values"][str(len(manifest))] += 1
    for item in manifest:
        if not isinstance(item, dict):
            continue
        role = row_string(item, "role")
        source = row_string(item, "source")
        pubkey = row_string(item, "pubkey")
        account_index = row_string(item, "account_index")
        instruction_index = row_string(item, "instruction_index")
        if role:
            stats["account_roles"][role] += 1
            stats["required_simulation_load_accounts"].add(f"{role}:{pubkey or ''}:{source or ''}")
        if source:
            stats["account_sources"][source] += 1
        if account_index:
            stats["account_index_to_role_map"][account_index][f"{role or 'unknown'}:{source or 'unknown'}"] += 1
        if instruction_index:
            stats["instruction_account_positions"].add(str(item.get("account_index")))


def add_primary_observation(
    routes: dict[str, dict[str, Any]],
    row: dict[str, Any],
    artifact: str,
    line_no: int,
) -> None:
    route = row_string(row, "primary_route_kind")
    if not route:
        return
    stats = routes[route]
    add_common_route_fields(stats, route, row, artifact, line_no, "primary_route_candidate")
    stats["builder_support_signals"]["primary_route_candidate"] += 1
    add_readiness(
        stats,
        row_bool(row, "primary_route_ready"),
        row_string(row, "primary_route_not_ready_reason"),
        "primary_route_ready",
    )
    if row_bool(row, "primary_route_ready") is False:
        reason = row_string(row, "primary_route_not_ready_reason") or "primary_route_not_ready"
        stats["shadow_simulation_support_counts"]["primary_route_blocked"] += 1
        stats["primary_failure_class_counts"][reason] += 1
    add_manifest(stats, row)


def add_fallback_observation(
    routes: dict[str, dict[str, Any]],
    row: dict[str, Any],
    artifact: str,
    line_no: int,
) -> None:
    route = row_string(row, "fallback_route_kind")
    if not route:
        return
    stats = routes[route]
    add_common_route_fields(stats, route, row, artifact, line_no, "fallback_route_candidate")
    stats["builder_support_signals"]["fallback_route_candidate"] += 1
    if row_bool(row, "fallback_route_attempted") is True:
        stats["builder_support_signals"]["fallback_route_attempted"] += 1
    add_readiness(
        stats,
        row_bool(row, "fallback_route_ready"),
        row_string(row, "fallback_route_not_ready_reason"),
        "fallback_route_ready",
    )
    if failure := row_string(row, "fallback_failure_class"):
        stats["primary_failure_class_counts"][failure] += 1
    for role in row_string_list(row, "fallback_missing_roles"):
        stats["missing_role_counts"][role] += 1
    for pubkey in row_string_list(row, "fallback_missing_pubkeys"):
        stats["missing_pubkey_counts"][pubkey] += 1
    for source in row_string_list(row, "fallback_account_sources"):
        stats["account_sources"][source] += 1
    add_account_set_entries(stats, row_string_list(row, "fallback_simulation_load_account_set"), "simulation_load")
    add_account_set_entries(stats, row_string_list(row, "fallback_creatable_account_set"), "creatable")
    add_account_set_entries(stats, row_string_list(row, "fallback_required_precheck_account_set"), "precheck")
    if row_string_list(row, "fallback_simulation_load_account_set"):
        stats["prepared_request_support_signals"]["fallback_simulation_load_account_set"] += 1
    if row_string_list(row, "fallback_required_precheck_account_set"):
        stats["prepared_request_support_signals"]["fallback_required_precheck_account_set"] += 1


def add_observed_bcv2_observation(
    routes: dict[str, dict[str, Any]],
    row: dict[str, Any],
    artifact: str,
    line_no: int,
) -> None:
    route = row_string(row, "observed_bcv2_source_buy_variant")
    if not route:
        return
    stats = routes[route]
    add_common_route_fields(stats, route, row, artifact, line_no, "observed_tx_account_meta")
    stats["builder_support_signals"]["observed_tx_account_meta_identity"] += 1
    if program_id := row_string(row, "observed_bcv2_source_program_id"):
        stats["program_id_counts"][program_id] += 1
    if discriminator := row_string(row, "observed_bcv2_source_discriminator"):
        stats["instruction_discriminator_counts"][discriminator] += 1
    if pos := row_string(row, "observed_bcv2_instruction_account_position"):
        stats["instruction_account_positions"].add(pos)
    if idx := row_string(row, "observed_bcv2_message_account_index"):
        stats["message_account_indices"].add(idx)
        stats["account_index_to_role_map"][idx]["bonding_curve_v2:observed_tx_account_meta"] += 1
    if source := row_string(row, "observed_bcv2_loaded_address_source"):
        stats["loaded_address_usage"][source] += 1
    add_readiness(
        stats,
        row_bool(row, "bonding_curve_v2_rpc_load_ready"),
        row_string(row, "bonding_curve_v2_rpc_load_status")
        or row_string(row, "builder_required_curve_account_ready_reason"),
        "observed_bcv2_rpc_load_ready",
    )
    if provenance := row_string(row, "observed_bcv2_provenance_status"):
        stats["readiness_reason_counts"][f"observed_bcv2_provenance:{provenance}"] += 1


def collect_rows(shadow_root: Path, decision_root: Path | None = None) -> tuple[dict[str, list[dict[str, Any]]], dict[str, str]]:
    artifacts: dict[str, list[dict[str, Any]]] = {}
    paths: dict[str, str] = {}
    for artifact, filename in ARTIFACT_FILES.items():
        path = shadow_root / filename
        rows = list(iter_jsonl(path))
        artifacts[artifact] = rows
        paths[artifact] = str(path)
    if decision_root:
        for artifact, filename in DECISION_FILES.items():
            path = decision_root / filename
            rows = list(iter_jsonl(path))
            artifacts[artifact] = rows
            paths[artifact] = str(path)
    return artifacts, paths


def summarize_route(route: str, stats: dict[str, Any]) -> dict[str, Any]:
    evidence_rows = len(stats["evidence_row_ids"])
    observed_count = len(stats["decision_ids"])
    account_role_map = {
        str(index): dict(sorted(counter.items()))
        for index, counter in sorted(stats["account_index_to_role_map"].items(), key=lambda item: int(item[0]) if str(item[0]).isdigit() else 9999)
    }
    manifest_present = stats["prepared_request_support_signals"].get("simulation_account_manifest", 0) > 0
    fallback_account_set_present = stats["prepared_request_support_signals"].get("fallback_simulation_load_account_set", 0) > 0
    selected_or_ready = (
        stats["builder_support_signals"].get("selected_route", 0)
        + stats["builder_support_signals"].get("primary_route_ready", 0)
        + stats["builder_support_signals"].get("fallback_route_ready", 0)
    )
    builder_candidate = (
        stats["builder_support_signals"].get("primary_route_candidate", 0)
        + stats["builder_support_signals"].get("fallback_route_candidate", 0)
        + selected_or_ready
    )
    if builder_candidate:
        builder_support_status = "supported_by_current_builder"
    elif stats["builder_support_signals"].get("observed_tx_account_meta_identity", 0):
        builder_support_status = "observed_tx_identity_only"
    else:
        builder_support_status = "unknown"

    if manifest_present:
        prepared_request_support_status = "prepared_request_manifest_available"
    elif fallback_account_set_present:
        prepared_request_support_status = "fallback_account_set_available"
    elif builder_candidate:
        prepared_request_support_status = "candidate_without_account_manifest"
    else:
        prepared_request_support_status = "not_prepared"

    readiness_total = stats["rpc_load_ready_true"] + stats["rpc_load_ready_false"]
    readiness_rate = (
        round(stats["rpc_load_ready_true"] / readiness_total, 6)
        if readiness_total
        else None
    )
    if selected_or_ready:
        shadow_status = "executable"
    elif stats["route_resolution_status_counts"].get("no_executable_route_account_set", 0):
        shadow_status = "not_executable_no_executable_route_account_set"
    elif stats["rpc_load_ready_false"]:
        shadow_status = "not_executable_rpc_load_not_ready"
    elif stats["builder_support_signals"].get("observed_tx_account_meta_identity", 0):
        shadow_status = "observed_identity_no_executable_route"
    else:
        shadow_status = "unknown"

    primary_failure = "none"
    if stats["primary_failure_class_counts"]:
        primary_failure = stats["primary_failure_class_counts"].most_common(1)[0][0]

    return {
        "route_variant": route,
        "observed_count": observed_count,
        "evidence_row_count": evidence_rows,
        "source_plane_counts": dict(sorted(stats["source_plane_counts"].items())),
        "observation_kind_counts": dict(sorted(stats["observation_kind_counts"].items())),
        "program_id_counts": dict(sorted(stats["program_id_counts"].items())),
        "instruction_discriminator_counts": dict(sorted(stats["instruction_discriminator_counts"].items())),
        "account_count_values": dict(sorted(stats["account_count_values"].items())),
        "instruction_account_positions": sorted(stats["instruction_account_positions"], key=lambda item: int(item) if item.isdigit() else 9999),
        "message_account_indices": sorted(stats["message_account_indices"], key=lambda item: int(item) if item.isdigit() else 9999),
        "loaded_address_usage": dict(sorted(stats["loaded_address_usage"].items())),
        "account_index_to_role_map": account_role_map,
        "account_index_to_role_map_coverage": len(account_role_map),
        "account_role_counts": dict(sorted(stats["account_roles"].items())),
        "account_source_counts": dict(sorted(stats["account_sources"].items())),
        "required_simulation_load_accounts": sorted(stats["required_simulation_load_accounts"]),
        "creatable_accounts": sorted(stats["creatable_accounts"]),
        "required_precheck_accounts": sorted(stats["required_precheck_accounts"]),
        "builder_support_status": builder_support_status,
        "builder_support_signals": dict(sorted(stats["builder_support_signals"].items())),
        "prepared_request_support_status": prepared_request_support_status,
        "prepared_request_support_signals": dict(sorted(stats["prepared_request_support_signals"].items())),
        "rpc_load_ready_true": stats["rpc_load_ready_true"],
        "rpc_load_ready_false": stats["rpc_load_ready_false"],
        "rpc_load_readiness_rate": readiness_rate,
        "readiness_reason_counts": dict(sorted(stats["readiness_reason_counts"].items())),
        "shadow_simulation_support_status": shadow_status,
        "shadow_simulation_support_counts": dict(sorted(stats["shadow_simulation_support_counts"].items())),
        "primary_failure_class": primary_failure,
        "failure_class_counts": dict(sorted(stats["primary_failure_class_counts"].items())),
        "missing_role_counts": dict(sorted(stats["missing_role_counts"].items())),
        "missing_pubkey_counts": dict(sorted(stats["missing_pubkey_counts"].items())),
    }


def choose_recommendation(route_summaries: dict[str, dict[str, Any]]) -> dict[str, Any]:
    if not route_summaries:
        return {
            "final_decision": "BLOCK_E1_AUDIT_GAP",
            "recommended_next_path": "route_artifact_gap",
            "recommended_next_route_to_implement": "unknown",
            "recommendation_reason": "no route evidence found in configured artifacts",
        }

    executable = [
        route
        for route, summary in route_summaries.items()
        if summary["shadow_simulation_support_status"] == "executable"
    ]
    if executable:
        route = sorted(executable)[0]
        return {
            "final_decision": "GO_R18_EXECUTABLE_ROUTE_SCOPED_RUN",
            "recommended_next_path": "run_executable_route_scoped_diagnostic",
            "recommended_next_route_to_implement": route,
            "recommendation_reason": f"{route} already has executable route evidence",
        }

    legacy = route_summaries.get("legacy_buy")
    if legacy and (
        legacy["failure_class_counts"].get("fallback_missing_core_curve_account", 0) > 0
        or legacy["missing_role_counts"].get("bonding_curve", 0) > 0
        or legacy["failure_class_counts"].get("fallback_builder_account_source_unverified", 0) > 0
    ):
        return {
            "final_decision": "GO_E2_IMPLEMENT_TOP_ROUTE_SUPPORT",
            "recommended_next_path": "implement_legacy_buy_executable_account_set_materialization",
            "recommended_next_route_to_implement": "legacy_buy_executable_account_set_materialization",
            "recommendation_reason": (
                "legacy_buy is the attempted fallback and the only observed route-compatible "
                "tx-meta source, but current fallback evidence lacks a complete executable "
                "legacy account set: active rows miss the core bonding_curve source and probe "
                "rows still depend on the primary BCV2 route account set"
            ),
        }

    routed = route_summaries.get("routed_exact_sol_in")
    if routed and (
        routed["failure_class_counts"].get("bonding_curve_v2_observed_meta_missing_on_rpc", 0) > 0
        or routed["failure_class_counts"].get("bonding_curve_v2_identity_authoritative_but_not_load_ready", 0) > 0
    ):
        return {
            "final_decision": "BLOCK_POLICY_CALIBRATION_ROUTE_SUPPORT_REQUIRED",
            "recommended_next_path": "route_support_expansion_required",
            "recommended_next_route_to_implement": "no_implementable_route_found",
            "recommendation_reason": (
                "only routed_exact_sol_in evidence is present and its route-required "
                "bonding_curve_v2 is not RPC-load-ready"
            ),
        }

    return {
        "final_decision": "BLOCK_E1_AUDIT_GAP",
        "recommended_next_path": "route_failure_classification_gap",
        "recommended_next_route_to_implement": "unknown",
        "recommendation_reason": "route evidence exists but no executable or implementable route class was classified",
    }


def build_report(shadow_root: Path, decision_root: Path | None = None) -> dict[str, Any]:
    artifacts, paths = collect_rows(shadow_root, decision_root)
    routes: dict[str, dict[str, Any]] = defaultdict(route_state)
    artifact_rows: dict[str, int] = {}

    for artifact, rows in artifacts.items():
        artifact_rows[artifact] = len(rows)
        for line_no, row in enumerate(rows, start=1):
            add_primary_observation(routes, row, artifact, line_no)
            add_fallback_observation(routes, row, artifact, line_no)
            add_observed_bcv2_observation(routes, row, artifact, line_no)

    route_summaries = {
        route: summarize_route(route, stats)
        for route, stats in sorted(routes.items())
    }
    route_variant_counts = {
        route: summary["observed_count"]
        for route, summary in sorted(route_summaries.items())
    }
    builder_supported = {
        route: summary["observed_count"]
        for route, summary in route_summaries.items()
        if summary["builder_support_status"] == "supported_by_current_builder"
    }
    unsupported = {
        route: summary["observed_count"]
        for route, summary in route_summaries.items()
        if summary["shadow_simulation_support_status"] != "executable"
    }
    role_map_coverage = {
        route: summary["account_index_to_role_map_coverage"]
        for route, summary in route_summaries.items()
    }
    rpc_load_readiness = {
        route: {
            "ready_true": summary["rpc_load_ready_true"],
            "ready_false": summary["rpc_load_ready_false"],
            "rate": summary["rpc_load_readiness_rate"],
        }
        for route, summary in route_summaries.items()
    }
    simulation_support = {
        route: summary["shadow_simulation_support_status"]
        for route, summary in route_summaries.items()
    }
    route_classes_excluded_from_l2 = sorted(
        route
        for route, summary in route_summaries.items()
        if summary["shadow_simulation_support_status"] != "executable"
    )
    recommendation = choose_recommendation(route_summaries)

    summary = {
        "observed_route_variant_counts": route_variant_counts,
        "builder_supported_route_counts": dict(sorted(builder_supported.items())),
        "unsupported_route_counts": dict(sorted(unsupported.items())),
        "route_account_role_map_coverage": dict(sorted(role_map_coverage.items())),
        "rpc_load_readiness_by_route": dict(sorted(rpc_load_readiness.items())),
        "simulation_support_by_route": dict(sorted(simulation_support.items())),
        "recommended_next_route_to_implement": recommendation["recommended_next_route_to_implement"],
        "route_classes_excluded_from_l2": route_classes_excluded_from_l2,
        "route_variant_total": len(route_summaries),
        "executable_route_count": sum(
            1
            for summary in route_summaries.values()
            if summary["shadow_simulation_support_status"] == "executable"
        ),
    }
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "shadow_root": str(shadow_root),
        "decision_root": str(decision_root) if decision_root else None,
        "artifact_paths": paths,
        "artifact_rows": artifact_rows,
        "summary": summary,
        "routes": route_summaries,
        **recommendation,
    }


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def markdown_table(rows: list[list[Any]]) -> str:
    if not rows:
        return ""
    header = rows[0]
    separator = ["---"] * len(header)
    lines = [
        "| " + " | ".join(str(item) for item in header) + " |",
        "| " + " | ".join(separator) + " |",
    ]
    for row in rows[1:]:
        lines.append("| " + " | ".join(str(item) for item in row) + " |")
    return "\n".join(lines)


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    summary = report["summary"]
    routes = report["routes"]
    path.parent.mkdir(parents=True, exist_ok=True)

    route_rows = [[
        "route_variant",
        "observed_count",
        "builder_support",
        "prepared_request_support",
        "rpc_ready_true",
        "rpc_ready_false",
        "simulation_support",
        "primary_failure",
    ]]
    for route, item in sorted(routes.items()):
        route_rows.append([
            route,
            item["observed_count"],
            item["builder_support_status"],
            item["prepared_request_support_status"],
            item["rpc_load_ready_true"],
            item["rpc_load_ready_false"],
            item["shadow_simulation_support_status"],
            item["primary_failure_class"],
        ])

    role_rows = [[
        "route_variant",
        "role_map_coverage",
        "top_missing_roles",
        "top_account_sources",
    ]]
    for route, item in sorted(routes.items()):
        top_missing = ", ".join(
            f"{key}:{value}"
            for key, value in list(item["missing_role_counts"].items())[:6]
        ) or "-"
        top_sources = ", ".join(
            f"{key}:{value}"
            for key, value in list(item["account_source_counts"].items())[:8]
        ) or "-"
        role_rows.append([
            route,
            item["account_index_to_role_map_coverage"],
            top_missing,
            top_sources,
        ])

    content = f"""# P3.7-E1 Pump.fun Executable Route Support Matrix

Generated at: `{report['generated_at']}`

## Decision

- final_decision: `{report['final_decision']}`
- recommended_next_path: `{report['recommended_next_path']}`
- recommended_next_route_to_implement: `{summary['recommended_next_route_to_implement']}`
- recommendation_reason: `{report['recommendation_reason']}`

## Inputs

- shadow_root: `{report['shadow_root']}`
- decision_root: `{report['decision_root']}`
- runtime: not run; offline artifact audit only

Artifact rows:

```json
{json.dumps(report['artifact_rows'], ensure_ascii=False, indent=2, sort_keys=True)}
```

## Required E1 Counters

```json
{json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True)}
```

## Route Matrix

{markdown_table(route_rows)}

## Account Role / Source Coverage

{markdown_table(role_rows)}

## Route Details

```json
{json.dumps(routes, ensure_ascii=False, indent=2, sort_keys=True)}
```

## Interpretation

R17A closed the replay-readiness side. E1 shows whether the current route
builder/resolver has an executable Pump.fun buy route universe. Non-executable
route classes remain excluded from L2/lifecycle denominator until a route has a
complete simulation-load account set and successful shadow/probe entry evidence.
"""
    path.write_text(content, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shadow-root", type=Path, default=DEFAULT_SHADOW_ROOT)
    parser.add_argument("--decision-root", type=Path, default=DEFAULT_DECISION_ROOT)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_JSON_OUTPUT)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_MD_OUTPUT)
    parser.add_argument("--json", action="store_true", help="Print report JSON to stdout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_report(args.shadow_root, args.decision_root)
    write_json(args.output_json, report)
    write_markdown(args.output_md, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
