#!/usr/bin/env python3
"""Audit join-key coverage for a P3.7 V3/MFS + lifecycle collection run."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_config_path, resolve_runtime_path


SCHEMA_VERSION = 5
DECISION_FILE_NAMES = ("gatekeeper_v2_decisions.jsonl", "gatekeeper_v2_buys.jsonl")
FIELD_GROUPS = {
    "ab_record_id": ("ab_record_id",),
    "source_ab_record_id": ("source_ab_record_id",),
    "probe_id": ("probe_id",),
    "dispatch_source": ("dispatch_source",),
    "collection_plane": ("collection_plane",),
    "probe_plane": ("probe_plane",),
    "candidate_id": ("candidate_id", "execution_candidate_id"),
    "position_id": ("position_id",),
    "pool_id": ("pool_id",),
    "mint": ("base_mint", "mint_id", "mint"),
    "decision_ts_ms": ("decision_ts_ms", "ab_t_end_event_ts_ms", "timestamp_ms", "timestamp"),
    "observation_start_ts_ms": ("observation_start_ts_ms", "ab_t0_event_ts_ms", "first_seen_ts_ms"),
    "observation_end_ts_ms": ("observation_end_ts_ms", "ab_t_end_event_ts_ms"),
    "feature_snapshot_hash": ("v3_feature_snapshot_hash", "feature_snapshot_hash"),
    "v3_policy_config_hash": ("v3_policy_config_hash", "config_hash"),
    "source_v3_feature_snapshot_hash": ("source_v3_feature_snapshot_hash",),
    "source_v3_policy_config_hash": ("source_v3_policy_config_hash",),
    "transport_v3_feature_snapshot_hash": ("transport_v3_feature_snapshot_hash",),
    "transport_v3_policy_config_hash": ("transport_v3_policy_config_hash",),
    "source_decision_log_path": ("source_decision_log_path",),
    "source_decision_row_offset": ("source_decision_row_offset",),
    "source_decision_row_sha256": ("source_decision_row_sha256",),
    "decision_plane": ("decision_plane", "source_decision_plane"),
    "rollout_namespace": ("rollout_namespace", "rollout_profile"),
    "v3_replay_payload": ("v3_replay_payload_schema_version", "v3_replay_payload"),
    "probe_bucket": ("probe_bucket",),
    "probe_skip_reason": ("probe_skip_reason", "skip_reason"),
    "probe_amount_source": ("probe_amount_source",),
    "buy_variant": ("buy_variant",),
    "token_param_role": ("token_param_role",),
    "entry_token_amount_raw": ("entry_token_amount_raw",),
    "min_tokens_out": ("min_tokens_out",),
    "execution_outcome": ("execution_outcome",),
    "error_class": ("error_class",),
    "simulation_error_category": ("simulation_error_category",),
    "simulation_error_kind": ("simulation_error_kind",),
    "simulation_error_custom_code": ("simulation_error_custom_code",),
    "simulation_error_account_pubkey": ("simulation_error_account_pubkey",),
    "simulation_error_account_role": ("simulation_error_account_role",),
    "simulation_error_account_source": ("simulation_error_account_source",),
    "simulation_error_account_candidates": ("simulation_error_account_candidates",),
    "simulation_error_account_candidates_raw": ("simulation_error_account_candidates_raw",),
    "simulation_error_account_candidates_narrowed": ("simulation_error_account_candidates_narrowed",),
    "simulation_error_account_candidates_excluded": ("simulation_error_account_candidates_excluded",),
    "simulation_error_account_narrowing_status": ("simulation_error_account_narrowing_status",),
    "simulation_error_account_narrowing_reason": ("simulation_error_account_narrowing_reason",),
    "precheck_account_set_hash": ("precheck_account_set_hash",),
    "prepared_request_account_set_hash": ("prepared_request_account_set_hash",),
    "simulation_account_set_hash": ("simulation_account_set_hash",),
    "account_set_match": ("account_set_match",),
    "account_set_mismatch_reason": ("account_set_mismatch_reason",),
    "probe_entry_materialization_status": ("probe_entry_materialization_status",),
    "probe_lifecycle_eligibility_status": ("probe_lifecycle_eligibility_status",),
    "active_shadow_precheck_status": ("active_shadow_precheck_status",),
    "active_shadow_lifecycle_eligibility_status": ("active_shadow_lifecycle_eligibility_status",),
    "execution_account_readiness_status": ("execution_account_readiness_status",),
    "execution_account_readiness_role": ("execution_account_readiness_role",),
    "execution_account_readiness_reason": ("execution_account_readiness_reason",),
    "fallback_failure_class": ("fallback_failure_class",),
    "fallback_missing_roles": ("fallback_missing_roles",),
    "fallback_account_sources": ("fallback_account_sources",),
    "run_id": ("run_id",),
    "session_id": ("session_id",),
}
COUNTER_FIELD_GROUPS = (
    "probe_bucket",
    "probe_skip_reason",
    "probe_amount_source",
    "dispatch_source",
    "buy_variant",
    "token_param_role",
    "execution_outcome",
    "error_class",
    "simulation_error_category",
    "simulation_error_kind",
    "simulation_error_account_role",
    "simulation_error_account_source",
    "simulation_error_account_narrowing_status",
    "account_set_match",
    "probe_entry_materialization_status",
    "probe_lifecycle_eligibility_status",
    "active_shadow_precheck_status",
    "active_shadow_lifecycle_eligibility_status",
    "execution_account_readiness_status",
    "execution_account_readiness_role",
    "fallback_failure_class",
)
CANONICAL_JOIN_ARTIFACTS = (
    "decision",
    "shadow_transport",
    "shadow_entry",
    "shadow_lifecycle",
    "shadow_onchain_lifecycle",
)
PROBE_JOIN_ARTIFACTS = (
    "probe_selection",
    "probe_transport",
    "probe_entry",
    "probe_lifecycle",
)


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
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
    value_counts: dict[str, Counter[str]] = {field: Counter() for field in COUNTER_FIELD_GROUPS}
    for group, fields in FIELD_GROUPS.items():
        count = 0
        for row in rows:
            values = value_set(row, fields)
            if values:
                count += 1
                identifiers[group].update(values)
            if group in value_counts:
                value = first_present(row, fields)
                if value is not None:
                    value_counts[group][str(value)] += 1
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
        "value_counts": {
            field: dict(sorted(counter.items()))
            for field, counter in value_counts.items()
            if counter
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
        resolved_lifecycle = Path(resolve_runtime_path(resolved, lifecycle_path))
        paths["shadow_lifecycle"].append(resolved_lifecycle)
        onchain_report = resolved_lifecycle.parent / "shadow_onchain_lifecycle_report.jsonl"
        if onchain_report.exists():
            paths["shadow_onchain_lifecycle"].append(onchain_report)

    probe = config.get("p37_shadow_probe", {})
    probe_path_fields = {
        "probe_selection": "selection_log_path",
        "probe_skip": "skip_log_path",
        "probe_transport": "transport_log_path",
        "probe_entry": "entry_log_path",
        "probe_lifecycle": "lifecycle_log_path",
    }
    for artifact_type, field in probe_path_fields.items():
        raw_path = probe.get(field)
        if raw_path:
            paths[artifact_type].append(Path(resolve_runtime_path(resolved, raw_path)))
    return {key: sorted(set(value)) for key, value in paths.items()}


def intersection_counts(summaries: list[dict[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for group in ("ab_record_id", "probe_id", "candidate_id", "pool_id", "mint"):
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


def artifact_totals(report: dict[str, Any], artifact_type: str, field_group: str | None = None) -> int:
    if field_group is None:
        return sum(item["rows"] for item in report["artifacts"].get(artifact_type, []))
    return sum(
        item["field_counts"].get(field_group, 0)
        for item in report["artifacts"].get(artifact_type, [])
    )


def join_quality(report: dict[str, Any]) -> str:
    intersections = report["cross_artifact_intersections"]
    ab = intersections.get("ab_record_id", {})
    candidate = intersections.get("candidate_id", {})
    pool = intersections.get("pool_id", {})
    mint = intersections.get("mint", {})
    nonempty_artifact_count = sum(
        1 for items in report["artifacts"].values() for item in items if item.get("rows", 0) > 0
    )
    if (
        nonempty_artifact_count > 1
        and ab.get("artifacts_with_rows", 0) == nonempty_artifact_count
        and ab.get("common_values", 0) > 0
    ):
        return "exact_ab_record_id"
    if candidate.get("common_values", 0) > 0:
        return "exact_candidate_id"
    if pool.get("common_values", 0) > 0 and mint.get("common_values", 0) > 0:
        return "pool_mint_time_window"
    if mint.get("common_values", 0) > 0:
        return "mint_only"
    return "unmatched"


def join_key_coverage(report: dict[str, Any]) -> dict[str, Any]:
    decision_rows = artifact_totals(report, "decision")
    transport_rows = artifact_totals(report, "shadow_transport")
    entry_rows = artifact_totals(report, "shadow_entry")
    lifecycle_rows = artifact_totals(report, "shadow_lifecycle")
    onchain_rows = artifact_totals(report, "shadow_onchain_lifecycle")
    artifact_rows = {
        "decision": decision_rows,
        "shadow_transport": transport_rows,
        "shadow_entry": entry_rows,
        "shadow_lifecycle": lifecycle_rows,
        "shadow_onchain_lifecycle": onchain_rows,
    }
    artifact_ab_rows = {
        key: artifact_totals(report, key, "ab_record_id") for key in artifact_rows
    }
    nonempty_ab_coverages = [
        artifact_ab_rows[key] / rows for key, rows in artifact_rows.items() if rows > 0
    ]
    return {
        "decision_rows_with_ab_record_id": artifact_ab_rows["decision"],
        "shadow_transport_rows_with_ab_record_id": artifact_ab_rows["shadow_transport"],
        "shadow_entry_rows_with_ab_record_id": artifact_ab_rows["shadow_entry"],
        "shadow_lifecycle_rows_with_ab_record_id": artifact_ab_rows["shadow_lifecycle"],
        "onchain_lifecycle_rows_with_ab_record_id": artifact_ab_rows["shadow_onchain_lifecycle"],
        "full_chain_ab_record_id_coverage": round(min(nonempty_ab_coverages), 6)
        if nonempty_ab_coverages
        else 0.0,
        "join_quality": join_quality(report),
    }


def probe_join_quality(report: dict[str, Any]) -> str:
    intersections = report.get("probe_artifact_intersections", {})
    probe = intersections.get("probe_id", {})
    ab = intersections.get("ab_record_id", {})
    candidate = intersections.get("candidate_id", {})
    pool = intersections.get("pool_id", {})
    mint = intersections.get("mint", {})
    nonempty_artifact_count = sum(
        1
        for artifact_type in PROBE_JOIN_ARTIFACTS
        for item in report["artifacts"].get(artifact_type, [])
        if item.get("rows", 0) > 0
    )
    if (
        nonempty_artifact_count > 1
        and probe.get("artifacts_with_rows", 0) == nonempty_artifact_count
        and ab.get("artifacts_with_rows", 0) == nonempty_artifact_count
        and probe.get("common_values", 0) > 0
        and ab.get("common_values", 0) > 0
    ):
        return "exact_probe_id_and_ab_record_id"
    if (
        nonempty_artifact_count > 1
        and ab.get("artifacts_with_rows", 0) == nonempty_artifact_count
        and ab.get("common_values", 0) > 0
    ):
        return "exact_ab_record_id"
    if probe.get("common_values", 0) > 0:
        return "exact_probe_id"
    if candidate.get("common_values", 0) > 0:
        return "exact_candidate_id"
    if pool.get("common_values", 0) > 0 and mint.get("common_values", 0) > 0:
        return "pool_mint_time_window"
    if mint.get("common_values", 0) > 0:
        return "mint_only"
    return "unmatched"


def probe_join_key_coverage(report: dict[str, Any]) -> dict[str, Any]:
    artifact_rows = {
        "probe_selection": artifact_totals(report, "probe_selection"),
        "probe_skip": artifact_totals(report, "probe_skip"),
        "probe_transport": artifact_totals(report, "probe_transport"),
        "probe_entry": artifact_totals(report, "probe_entry"),
        "probe_lifecycle": artifact_totals(report, "probe_lifecycle"),
    }
    field_rows = {
        key: {
            "ab_record_id": artifact_totals(report, key, "ab_record_id"),
            "source_ab_record_id": artifact_totals(report, key, "source_ab_record_id"),
            "probe_id": artifact_totals(report, key, "probe_id"),
            "feature_snapshot_hash": artifact_totals(report, key, "feature_snapshot_hash"),
            "v3_policy_config_hash": artifact_totals(report, key, "v3_policy_config_hash"),
            "dispatch_source": artifact_totals(report, key, "dispatch_source"),
        }
        for key in artifact_rows
    }
    join_artifact_rows = {
        key: rows for key, rows in artifact_rows.items() if key != "probe_skip"
    }
    nonempty_ab_coverages = [
        field_rows[key]["ab_record_id"] / rows
        for key, rows in join_artifact_rows.items()
        if rows > 0
    ]
    nonempty_probe_coverages = [
        field_rows[key]["probe_id"] / rows
        for key, rows in join_artifact_rows.items()
        if rows > 0
    ]
    return {
        "probe_selection_rows": artifact_rows["probe_selection"],
        "probe_skipped_rows": artifact_rows["probe_skip"],
        "probe_transport_rows": artifact_rows["probe_transport"],
        "probe_entry_rows": artifact_rows["probe_entry"],
        "probe_lifecycle_rows": artifact_rows["probe_lifecycle"],
        "probe_selection_rows_with_ab_record_id": field_rows["probe_selection"]["ab_record_id"],
        "probe_transport_rows_with_ab_record_id": field_rows["probe_transport"]["ab_record_id"],
        "probe_entry_rows_with_ab_record_id": field_rows["probe_entry"]["ab_record_id"],
        "probe_lifecycle_rows_with_ab_record_id": field_rows["probe_lifecycle"]["ab_record_id"],
        "probe_selection_rows_with_probe_id": field_rows["probe_selection"]["probe_id"],
        "probe_transport_rows_with_probe_id": field_rows["probe_transport"]["probe_id"],
        "probe_entry_rows_with_probe_id": field_rows["probe_entry"]["probe_id"],
        "probe_lifecycle_rows_with_probe_id": field_rows["probe_lifecycle"]["probe_id"],
        "probe_transport_rows_with_dispatch_source": field_rows["probe_transport"]["dispatch_source"],
        "probe_entry_rows_with_dispatch_source": field_rows["probe_entry"]["dispatch_source"],
        "probe_transport_rows_with_feature_hash": field_rows["probe_transport"]["feature_snapshot_hash"],
        "probe_entry_rows_with_feature_hash": field_rows["probe_entry"]["feature_snapshot_hash"],
        "probe_transport_rows_with_policy_hash": field_rows["probe_transport"]["v3_policy_config_hash"],
        "probe_entry_rows_with_policy_hash": field_rows["probe_entry"]["v3_policy_config_hash"],
        "probe_chain_ab_record_id_coverage": round(min(nonempty_ab_coverages), 6)
        if nonempty_ab_coverages
        else 0.0,
        "probe_chain_probe_id_coverage": round(min(nonempty_probe_coverages), 6)
        if nonempty_probe_coverages
        else 0.0,
        "probe_join_quality": probe_join_quality(report),
    }


def artifact_rows(paths: dict[str, list[Path]], artifact_type: str) -> list[dict[str, Any]]:
    return [row for path in paths.get(artifact_type, []) for row in iter_jsonl(path)]


def row_ab_record_id(row: dict[str, Any]) -> str | None:
    value = first_present(row, FIELD_GROUPS["ab_record_id"])
    if value is None:
        value = first_present(row, FIELD_GROUPS["source_ab_record_id"])
    return str(value) if value is not None else None


def row_feature_hash(row: dict[str, Any]) -> str | None:
    value = first_present(row, FIELD_GROUPS["feature_snapshot_hash"])
    return str(value) if value is not None else None


def row_policy_hash(row: dict[str, Any]) -> str | None:
    value = first_present(row, FIELD_GROUPS["v3_policy_config_hash"])
    return str(value) if value is not None else None


def row_decision_plane(row: dict[str, Any]) -> str | None:
    value = first_present(row, FIELD_GROUPS["decision_plane"])
    return str(value) if value is not None else None


def row_has_v3_payload(row: dict[str, Any]) -> bool:
    return first_present(row, FIELD_GROUPS["v3_replay_payload"]) is not None


def row_probe_id(row: dict[str, Any]) -> str | None:
    value = first_present(row, FIELD_GROUPS["probe_id"])
    return str(value) if value is not None else None


def row_string(row: dict[str, Any], field: str) -> str | None:
    value = row.get(field)
    if value is None:
        return None
    if isinstance(value, str) and value == "":
        return None
    return str(value)


def row_bool_string(row: dict[str, Any], field: str) -> str | None:
    value = row.get(field)
    if value is None:
        return None
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str) and value == "":
        return None
    return str(value).lower()


def row_int(row: dict[str, Any], field: str) -> int | None:
    value = row.get(field)
    if value is None or isinstance(value, bool):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def is_account_not_found_row(row: dict[str, Any]) -> bool:
    kind = row_string(row, "simulation_error_kind")
    category = row_string(row, "simulation_error_category")
    message = row_string(row, "simulation_error_message") or row_string(row, "err")
    return (
        kind == "AccountNotFound"
        or (category is not None and "account_not_found" in category)
        or (message is not None and "AccountNotFound" in message)
    )


def iter_account_candidates(row: dict[str, Any], field: str) -> Iterable[dict[str, Any]]:
    value = row.get(field)
    if isinstance(value, list):
        for item in value:
            if isinstance(item, dict):
                yield item


def row_string_list(row: dict[str, Any], field: str) -> list[str]:
    value = row.get(field)
    if isinstance(value, list):
        return [str(item) for item in value if str(item).strip()]
    if isinstance(value, str) and value.strip():
        return [value]
    return []


LEGACY_ROUTE_VALUES = {"legacy_buy", "LegacyBuy"}


def row_has_legacy_route_diagnostics(row: dict[str, Any]) -> bool:
    scalar_fields = [
        "legacy_buy_account_set_status",
        "legacy_buy_curve_pubkey",
        "legacy_buy_curve_source",
        "legacy_buy_curve_authority_status",
        "legacy_buy_curve_rpc_load_status",
        "legacy_buy_curve_rpc_load_ready",
        "legacy_buy_curve_authority_readiness_status",
        "legacy_buy_associated_bonding_curve_pubkey",
        "legacy_buy_associated_bonding_curve_source",
        "legacy_buy_associated_bonding_curve_rpc_load_ready",
        "legacy_buy_route_ready",
        "legacy_buy_route_not_ready_reason",
    ]
    vector_fields = [
        "legacy_buy_required_roles",
        "legacy_buy_missing_roles",
        "legacy_buy_missing_pubkeys",
    ]
    return any(row_string(row, field) is not None for field in scalar_fields) or any(
        row_string_list(row, field) for field in vector_fields
    )


def slot_age_bucket(value: int | None) -> str:
    if value is None:
        return "missing"
    if value < 0:
        return "negative"
    if value == 0:
        return "0"
    if value <= 2:
        return "1_2"
    if value <= 8:
        return "3_8"
    if value <= 32:
        return "9_32"
    return "gt_32"


def fallback_failure_class_for_row(row: dict[str, Any]) -> str:
    explicit = row_string(row, "fallback_failure_class")
    if explicit:
        return explicit
    reason = row_string(row, "fallback_route_not_ready_reason")
    if reason in {
        "unsupported_builder_layout_requires_bcv2",
        "legacy_buy_unsupported_builder_layout_requires_bcv2",
    }:
        return "fallback_unsupported_builder_layout"
    if reason == "fallback_route_missing_legacy_buy_curve":
        return "fallback_missing_core_curve_account"
    if reason in {
        "fallback_route_requires_same_bcv2_simulation_load_account",
        "fallback_route_requires_authoritative_primary_route_accounts",
    }:
        return "fallback_builder_account_source_unverified"
    if reason == "fallback_route_not_available_for_primary":
        return "fallback_no_prepared_route"
    missing_roles = set(row_string_list(row, "fallback_missing_roles"))
    if "bonding_curve" in missing_roles:
        return "fallback_missing_core_curve_account"
    if "associated_bonding_curve" in missing_roles:
        return "fallback_missing_associated_bonding_curve"
    if "creator_vault" in missing_roles:
        return "fallback_missing_creator_vault"
    if "user_ata" in missing_roles:
        return "fallback_missing_user_ata_but_creatable"
    if "payer_pubkey" in missing_roles:
        return "fallback_missing_payer_but_ephemeral"
    if "bonding_curve_v2" in missing_roles:
        return "fallback_missing_route_identity"
    if row_string(row, "fallback_route_kind") is None:
        return "fallback_no_prepared_route"
    return "fallback_unknown"


def fallback_missing_roles_for_row(row: dict[str, Any]) -> list[str]:
    roles = row_string_list(row, "fallback_missing_roles")
    if roles:
        return roles
    failure_class = fallback_failure_class_for_row(row)
    if failure_class == "fallback_missing_core_curve_account":
        return ["bonding_curve"]
    if failure_class == "fallback_missing_associated_bonding_curve":
        return ["associated_bonding_curve"]
    if failure_class == "fallback_missing_creator_vault":
        return ["creator_vault"]
    if failure_class == "fallback_missing_user_ata_but_creatable":
        return ["user_ata"]
    if failure_class == "fallback_missing_payer_but_ephemeral":
        return ["payer_pubkey"]
    if failure_class in {
        "fallback_missing_route_identity",
        "fallback_builder_account_source_unverified",
    }:
        return ["bonding_curve_v2"]
    return []


def fallback_account_sources_for_row(row: dict[str, Any]) -> list[str]:
    sources = row_string_list(row, "fallback_account_sources")
    if sources:
        return sources
    reason = row_string(row, "fallback_route_not_ready_reason")
    if reason == "fallback_route_missing_legacy_buy_curve":
        return ["legacy_buy_curve"]
    if reason in {
        "fallback_route_requires_same_bcv2_simulation_load_account",
        "fallback_route_requires_authoritative_primary_route_accounts",
    }:
        return ["primary_route_account_set"]
    if reason == "fallback_route_not_available_for_primary":
        return ["route_builder"]
    if reason in {
        "unsupported_builder_layout_requires_bcv2",
        "legacy_buy_unsupported_builder_layout_requires_bcv2",
    }:
        return ["direct_buy_builder"]
    return []


def fallback_decision_payload(failed_rows: list[dict[str, Any]]) -> dict[str, Any]:
    class_counts: Counter[str] = Counter()
    missing_role_counts: Counter[str] = Counter()
    missing_pubkey_counts: Counter[str] = Counter()
    account_source_counts: Counter[str] = Counter()
    simulation_load_account_set_rows = 0
    creatable_account_set_rows = 0
    required_precheck_account_set_rows = 0

    for row in failed_rows:
        failure_class = fallback_failure_class_for_row(row)
        class_counts[failure_class] += 1
        for role in fallback_missing_roles_for_row(row):
            missing_role_counts[role] += 1
        for pubkey in row_string_list(row, "fallback_missing_pubkeys"):
            missing_pubkey_counts[pubkey] += 1
        for source in fallback_account_sources_for_row(row):
            account_source_counts[source] += 1
        if row_string_list(row, "fallback_simulation_load_account_set"):
            simulation_load_account_set_rows += 1
        if row_string_list(row, "fallback_creatable_account_set"):
            creatable_account_set_rows += 1
        if row_string_list(row, "fallback_required_precheck_account_set"):
            required_precheck_account_set_rows += 1

    repairable_classes = {
        "fallback_missing_user_ata_but_creatable",
        "fallback_missing_payer_but_ephemeral",
    }
    exclusion_classes = {
        "fallback_missing_core_curve_account",
        "fallback_missing_associated_bonding_curve",
        "fallback_missing_creator_vault",
        "fallback_missing_route_identity",
        "fallback_builder_account_source_unverified",
        "fallback_no_prepared_route",
        "fallback_unsupported_builder_layout",
    }
    observed_classes = set(class_counts)
    if not failed_rows or "fallback_unknown" in observed_classes:
        fallback_repairable: bool | None = None
        recommended_next_path = "audit_gap"
    elif observed_classes and observed_classes.issubset(repairable_classes):
        fallback_repairable = True
        recommended_next_path = "fallback_route_account_source_repair"
    elif observed_classes & exclusion_classes:
        fallback_repairable = False
        recommended_next_path = "route_class_exclusion_from_execution_label_universe"
    else:
        fallback_repairable = None
        recommended_next_path = "audit_gap"

    return {
        "fallback_failure_class_counts": dict(sorted(class_counts.items())),
        "fallback_missing_role_counts": dict(sorted(missing_role_counts.items())),
        "fallback_missing_pubkey_counts": dict(sorted(missing_pubkey_counts.items())),
        "fallback_account_source_counts": dict(sorted(account_source_counts.items())),
        "fallback_simulation_load_account_set_rows": simulation_load_account_set_rows,
        "fallback_creatable_account_set_rows": creatable_account_set_rows,
        "fallback_required_precheck_account_set_rows": required_precheck_account_set_rows,
        "fallback_repairable": fallback_repairable,
        "recommended_next_path": recommended_next_path,
    }


def working_builder_parity_payload(
    rows: list[dict[str, Any]],
    prefix: str = "",
) -> dict[str, Any]:
    ready_rpc_load_statuses = {
        "rpc_load_ready",
        "local_diag_ready",
        "mfs_materialized_ready",
        "account_state_ready",
    }

    def source_authority_ready(row: dict[str, Any], field: str) -> bool:
        status = row_string(row, field)
        return status is None or status.startswith("authoritative_")

    def source_authority_authoritative(row: dict[str, Any], field: str) -> bool:
        status = row_string(row, field)
        return status is not None and status.startswith("authoritative_")

    def rpc_load_ready(row: dict[str, Any], field: str) -> bool:
        status = row_string(row, field)
        return status is None or status in ready_rpc_load_statuses

    def role_rpc_load_ready(
        row: dict[str, Any],
        ready_field: str,
        status_field: str,
    ) -> bool:
        ready = row_bool_string(row, ready_field)
        if ready is not None:
            return ready == "true"
        return rpc_load_ready(row, status_field)

    def account_source_ready_after_repair(
        row: dict[str, Any],
        source_field: str,
        ready_field: str,
        status_field: str,
    ) -> bool:
        return source_authority_authoritative(row, source_field) and role_rpc_load_ready(
            row, ready_field, status_field
        )

    def bcv2_materialization_evidence_ready(row: dict[str, Any]) -> bool:
        if row_bool_string(row, "working_builder_bcv2_account_state_materialized") == "true":
            return True
        if row_bool_string(row, "working_builder_bcv2_mfs_materialized") == "true":
            return True
        if row_bool_string(row, "working_builder_bcv2_diag_materialized") == "true":
            return True
        if row_bool_string(row, "working_builder_bcv2_rpc_fetch_ready") == "true":
            return (
                row_string(row, "working_builder_bcv2_rpc_fetch_owner") is not None
                and row_int(row, "working_builder_bcv2_rpc_fetch_data_len") is not None
                and row_int(row, "working_builder_bcv2_precheck_context_slot") is not None
            )
        return False

    def bcv2_ready_with_materialization_evidence(row: dict[str, Any]) -> bool:
        return account_source_ready_after_repair(
            row,
            "working_builder_bcv2_source_authority",
            "working_builder_bcv2_rpc_load_ready",
            "working_builder_bcv2_rpc_load_status",
        ) and bcv2_materialization_evidence_ready(row)

    parity_rows = [
        row
        for row in rows
        if row_string(row, "working_builder_parity_mode") == "working_builder_parity"
    ]
    request_built_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_request_built") == "true"
    ]
    working_builder_buy_variant_counts = Counter(
        row_string(row, "working_builder_buy_variant") or "missing"
        for row in parity_rows
    )
    variant_drift_rows = [
        row
        for row in parity_rows
        if row_string(row, "buy_variant") is not None
        and row_string(row, "working_builder_buy_variant") is not None
        and row_string(row, "buy_variant") != row_string(row, "working_builder_buy_variant")
    ]
    legacy_variant_rows = [
        row
        for row in parity_rows
        if row_string(row, "buy_variant") in LEGACY_ROUTE_VALUES
        or row_string(row, "working_builder_buy_variant") in LEGACY_ROUTE_VALUES
    ]
    selected_legacy_handoff_rows = [
        row
        for row in parity_rows
        if row_string(row, "selected_route_kind") in LEGACY_ROUTE_VALUES
        or row_string(row, "selected_route_source")
        == "selected_fallback_route_execution_handoff"
        or row_string(row, "selected_route_handoff_status")
        in {
            "selected_route_handoff_applied",
            "selected_route_handoff_mismatch",
            "selected_route_handoff_hash_mismatch",
        }
    ]
    stale_route_diagnostics_rows = [
        row
        for row in parity_rows
        if row_string(row, "fallback_route_kind") in LEGACY_ROUTE_VALUES
        or row_string(row, "selected_route_kind") in LEGACY_ROUTE_VALUES
        or row_string(row, "selected_route_source")
        == "selected_fallback_route_execution_handoff"
        or row_string(row, "route_resolution_status") == "fallback_route_ready"
        or row_has_legacy_route_diagnostics(row)
    ]
    legacy_fallback_attempted_rows = [
        row
        for row in rows
        if row_string(row, "fallback_route_kind") in LEGACY_ROUTE_VALUES
        and row_bool_string(row, "fallback_route_attempted") == "true"
    ]
    selected_route_handoff_mismatch_rows = [
        row
        for row in rows
        if row_string(row, "selected_route_handoff_status")
        in {
            "selected_route_handoff_mismatch",
            "selected_route_handoff_hash_mismatch",
        }
        or "selected_route_handoff_mismatch"
        in (row_string(row, "selected_route_handoff_reason") or "")
        or "selected_route_handoff_mismatch"
        in (row_string(row, "precheck_failure_reason") or "")
    ]
    manifest_missing_required_rows = [
        row
        for row in parity_rows
        if row_string_list(row, "working_builder_missing_required_accounts")
    ]
    manifest_ready_rows = [
        row
        for row in request_built_rows
        if not row_string_list(row, "working_builder_missing_required_accounts")
        and (
            row_string(row, "working_builder_bcv2_source_authority") is None
            or bcv2_ready_with_materialization_evidence(row)
        )
        and source_authority_ready(row, "working_builder_creator_vault_source_authority")
        and rpc_load_ready(row, "working_builder_creator_vault_rpc_load_status")
        and (
            row_string(row, "working_builder_rpc_manifest_hash")
            or row_string(row, "working_builder_sender_manifest_hash")
        )
    ]
    contains_bcv2_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_manifest_contains_bcv2") == "true"
        or any(
            "bonding_curve_v2" in item
            for item in row_string_list(row, "working_builder_rpc_manifest_account_roles")
            + row_string_list(row, "working_builder_sender_manifest_account_roles")
        )
    ]
    bcv2_source_authority_counts = Counter(
        row_string(row, "working_builder_bcv2_source_authority") or "missing"
        for row in parity_rows
    )
    bcv2_rpc_load_status_counts = Counter(
        row_string(row, "working_builder_bcv2_rpc_load_status") or "missing"
        for row in parity_rows
    )
    bcv2_reconciliation_class_counts = Counter(
        row_string(row, "working_builder_bcv2_reconciliation_class") or "missing"
        for row in parity_rows
    )
    bcv2_pubkey_consistency_status_counts = Counter(
        row_string(row, "working_builder_bcv2_pubkey_consistency_status") or "missing"
        for row in parity_rows
    )
    bcv2_precheck_commitment_counts = Counter(
        row_string(row, "working_builder_bcv2_precheck_commitment") or "missing"
        for row in parity_rows
    )
    bcv2_rpc_error_class_counts = Counter(
        row_string(row, "working_builder_bcv2_rpc_error_class") or "missing"
        for row in parity_rows
    )
    bcv2_loaded_address_source_counts = Counter(
        row_string(row, "observed_bcv2_loaded_address_source") or "missing"
        for row in parity_rows
    )
    bcv2_precheck_age_bucket_counts = Counter(
        slot_age_bucket(
            row_int(row, "working_builder_bcv2_precheck_age_from_observed_slot")
        )
        for row in parity_rows
    )
    bcv2_local_coverage_class_counts = Counter(
        row_string(row, "working_builder_bcv2_local_coverage_class") or "missing"
        for row in parity_rows
    )
    bcv2_materialization_class_counts = Counter(
        row_string(row, "working_builder_bcv2_materialization_class") or "missing"
        for row in parity_rows
    )
    bcv2_subscription_requested_counts = Counter(
        row_bool_string(row, "working_builder_bcv2_subscription_requested")
        or "missing"
        for row in parity_rows
    )
    bcv2_account_update_received_counts = Counter(
        row_bool_string(row, "working_builder_bcv2_account_update_received")
        or "missing"
        for row in parity_rows
    )
    bcv2_account_update_mapped_counts = Counter(
        row_bool_string(row, "working_builder_bcv2_account_update_mapped")
        or "missing"
        for row in parity_rows
    )
    bcv2_account_state_lookup_performed_counts = Counter(
        row_bool_string(row, "working_builder_bcv2_account_state_lookup_performed")
        or "missing"
        for row in parity_rows
    )
    bcv2_account_state_age_bucket_counts = Counter(
        slot_age_bucket(row_int(row, "working_builder_bcv2_account_state_age_slots"))
        for row in parity_rows
    )
    bcv2_mfs_seen_reason_counts = Counter(
        row_string(row, "working_builder_bcv2_mfs_seen_reason") or "missing"
        for row in parity_rows
    )
    bcv2_diag_seen_reason_counts = Counter(
        row_string(row, "working_builder_bcv2_diag_seen_reason") or "missing"
        for row in parity_rows
    )
    bcv2_precheck_pubkey_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_precheck_pubkey") is not None
    ]
    bcv2_builder_pubkey_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_builder_pubkey") is not None
    ]
    bcv2_observed_pubkey_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_observed_pubkey") is not None
    ]
    bcv2_observed_slot_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_observed_slot") is not None
    ]
    bcv2_observed_tx_signature_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_observed_tx_signature") is not None
    ]
    bcv2_precheck_context_slot_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_precheck_context_slot") is not None
    ]
    bcv2_precheck_attempt_count_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_precheck_attempt_count") is not None
    ]
    bcv2_precheck_latency_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_precheck_latency_ms") is not None
    ]
    bcv2_precheck_age_from_observed_slot_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_precheck_age_from_observed_slot")
        is not None
    ]
    bcv2_loaded_address_source_missing_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_observed_pubkey") is not None
        and row_string(row, "observed_bcv2_loaded_address_source") is None
    ]
    bcv2_account_state_lookup_performed_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_account_state_lookup_performed")
        is not None
    ]
    bcv2_account_state_seen_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_account_state_seen") == "true"
    ]
    bcv2_account_state_seen_slot_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_account_state_seen_slot") is not None
    ]
    bcv2_account_state_age_slots_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_account_state_age_slots") is not None
    ]
    bcv2_account_state_owner_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_account_state_owner") is not None
    ]
    bcv2_account_state_data_len_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_account_state_data_len") is not None
    ]
    bcv2_subscription_requested_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_subscription_requested") == "true"
    ]
    bcv2_account_update_received_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_account_update_received") == "true"
    ]
    bcv2_account_update_mapped_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_account_update_mapped") == "true"
    ]
    bcv2_rpc_fetch_ready_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_rpc_fetch_ready") == "true"
    ]
    bcv2_rpc_fetch_missing_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_rpc_fetch_missing") == "true"
    ]
    bcv2_rpc_fetch_owner_rows = [
        row
        for row in parity_rows
        if row_string(row, "working_builder_bcv2_rpc_fetch_owner") is not None
    ]
    bcv2_rpc_fetch_data_len_rows = [
        row
        for row in parity_rows
        if row_int(row, "working_builder_bcv2_rpc_fetch_data_len") is not None
    ]
    bcv2_account_state_materialized_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_account_state_materialized")
        == "true"
    ]
    bcv2_mfs_materialized_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_mfs_materialized") == "true"
    ]
    bcv2_diag_materialized_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_diag_materialized") == "true"
    ]
    creator_vault_source_authority_counts = Counter(
        row_string(row, "working_builder_creator_vault_source_authority") or "missing"
        for row in parity_rows
    )
    creator_vault_rpc_load_status_counts = Counter(
        row_string(row, "working_builder_creator_vault_rpc_load_status") or "missing"
        for row in parity_rows
    )
    bcv2_authoritative_and_load_ready_rows = [
        row
        for row in parity_rows
        if bcv2_ready_with_materialization_evidence(row)
    ]
    bcv2_authoritative_but_missing_on_rpc_rows = [
        row
        for row in parity_rows
        if source_authority_authoritative(row, "working_builder_bcv2_source_authority")
        and row_string(row, "working_builder_bcv2_rpc_load_status")
        == "missing_on_rpc_precheck"
    ]
    bcv2_pubkey_mismatch_rows = [
        row
        for row in parity_rows
        if (
            row_string(row, "working_builder_bcv2_pubkey")
            and row_string(row, "bonding_curve_v2_pubkey")
            and row_string(row, "working_builder_bcv2_pubkey")
            != row_string(row, "bonding_curve_v2_pubkey")
        )
        or (
            row_string(row, "working_builder_bcv2_pubkey")
            and row_string(row, "observed_bcv2_resolved_pubkey")
            and row_string(row, "working_builder_bcv2_pubkey")
            != row_string(row, "observed_bcv2_resolved_pubkey")
        )
    ]
    bcv2_observed_tx_missing_on_rpc_rows = [
        row
        for row in parity_rows
        if (
            row_string(row, "working_builder_bcv2_source_authority")
            == "authoritative_observed_tx"
            or row_bool_string(row, "working_builder_bcv2_seen_in_observed_tx")
            == "true"
        )
        and row_string(row, "working_builder_bcv2_rpc_load_status")
        == "missing_on_rpc_precheck"
    ]
    bcv2_account_state_missing_rows = [
        row
        for row in parity_rows
        if row_bool_string(row, "working_builder_bcv2_seen_in_account_state") == "false"
    ]
    creator_vault_authoritative_and_load_ready_rows = [
        row
        for row in parity_rows
        if account_source_ready_after_repair(
            row,
            "working_builder_creator_vault_source_authority",
            "working_builder_creator_vault_rpc_load_ready",
            "working_builder_creator_vault_rpc_load_status",
        )
    ]
    creator_vault_authoritative_but_missing_on_rpc_rows = [
        row
        for row in parity_rows
        if source_authority_authoritative(
            row, "working_builder_creator_vault_source_authority"
        )
        and row_string(row, "working_builder_creator_vault_rpc_load_status")
        == "missing_on_rpc_precheck"
    ]
    creator_vault_source_mismatch_rows = [
        row
        for row in parity_rows
        if (
            row_string(row, "working_builder_creator_vault_source_authority")
            is not None
            and not source_authority_authoritative(
                row, "working_builder_creator_vault_source_authority"
            )
        )
        or row_string(row, "working_builder_creator_vault_readiness_reason")
        == "creator_vault_source_not_authoritative"
    ]
    manifest_ready_after_account_source_repair_rows = [
        row
        for row in request_built_rows
        if not row_string_list(row, "working_builder_missing_required_accounts")
        and bcv2_ready_with_materialization_evidence(row)
        and account_source_ready_after_repair(
            row,
            "working_builder_creator_vault_source_authority",
            "working_builder_creator_vault_rpc_load_ready",
            "working_builder_creator_vault_rpc_load_status",
        )
        and (
            row_string(row, "working_builder_rpc_manifest_hash")
            or row_string(row, "working_builder_sender_manifest_hash")
        )
    ]
    manifest_still_not_ready_after_account_source_repair_rows = [
        row
        for row in request_built_rows
        if row not in manifest_ready_after_account_source_repair_rows
    ]
    return {
        f"{prefix}working_builder_parity_rows": len(parity_rows),
        f"{prefix}working_builder_request_built_rows": len(request_built_rows),
        f"{prefix}working_builder_buy_variant_counts": dict(
            sorted(working_builder_buy_variant_counts.items())
        ),
        f"{prefix}probe_working_builder_variant_drift_rows": len(variant_drift_rows),
        f"{prefix}probe_working_builder_legacy_variant_rows": len(legacy_variant_rows),
        f"{prefix}probe_working_builder_selected_legacy_handoff_rows": len(
            selected_legacy_handoff_rows
        ),
        f"{prefix}probe_working_builder_stale_route_diagnostics_rows": len(
            stale_route_diagnostics_rows
        ),
        f"{prefix}legacy_fallback_attempted_rows": len(legacy_fallback_attempted_rows),
        f"{prefix}selected_route_handoff_mismatch_rows": len(
            selected_route_handoff_mismatch_rows
        ),
        f"{prefix}working_builder_manifest_missing_required_rows": len(
            manifest_missing_required_rows
        ),
        f"{prefix}working_builder_manifest_ready_rows": len(manifest_ready_rows),
        f"{prefix}working_builder_manifest_contains_bcv2_rows": len(contains_bcv2_rows),
        f"{prefix}working_builder_bcv2_source_authority_counts": dict(
            sorted(bcv2_source_authority_counts.items())
        ),
        f"{prefix}working_builder_bcv2_rpc_load_status_counts": dict(
            sorted(bcv2_rpc_load_status_counts.items())
        ),
        f"{prefix}working_builder_bcv2_reconciliation_class_counts": dict(
            sorted(bcv2_reconciliation_class_counts.items())
        ),
        f"{prefix}working_builder_bcv2_pubkey_consistency_status_counts": dict(
            sorted(bcv2_pubkey_consistency_status_counts.items())
        ),
        f"{prefix}working_builder_bcv2_precheck_commitment_counts": dict(
            sorted(bcv2_precheck_commitment_counts.items())
        ),
        f"{prefix}working_builder_bcv2_rpc_error_class_counts": dict(
            sorted(bcv2_rpc_error_class_counts.items())
        ),
        f"{prefix}working_builder_bcv2_loaded_address_source_counts": dict(
            sorted(bcv2_loaded_address_source_counts.items())
        ),
        f"{prefix}working_builder_bcv2_precheck_age_bucket_counts": dict(
            sorted(bcv2_precheck_age_bucket_counts.items())
        ),
        f"{prefix}working_builder_bcv2_local_coverage_class_counts": dict(
            sorted(bcv2_local_coverage_class_counts.items())
        ),
        f"{prefix}working_builder_bcv2_materialization_class_counts": dict(
            sorted(bcv2_materialization_class_counts.items())
        ),
        f"{prefix}working_builder_bcv2_subscription_requested_counts": dict(
            sorted(bcv2_subscription_requested_counts.items())
        ),
        f"{prefix}working_builder_bcv2_account_update_received_counts": dict(
            sorted(bcv2_account_update_received_counts.items())
        ),
        f"{prefix}working_builder_bcv2_account_update_mapped_counts": dict(
            sorted(bcv2_account_update_mapped_counts.items())
        ),
        f"{prefix}working_builder_bcv2_account_state_lookup_performed_counts": dict(
            sorted(bcv2_account_state_lookup_performed_counts.items())
        ),
        f"{prefix}working_builder_bcv2_account_state_age_bucket_counts": dict(
            sorted(bcv2_account_state_age_bucket_counts.items())
        ),
        f"{prefix}working_builder_bcv2_mfs_seen_reason_counts": dict(
            sorted(bcv2_mfs_seen_reason_counts.items())
        ),
        f"{prefix}working_builder_bcv2_diag_seen_reason_counts": dict(
            sorted(bcv2_diag_seen_reason_counts.items())
        ),
        f"{prefix}working_builder_bcv2_precheck_pubkey_rows": len(
            bcv2_precheck_pubkey_rows
        ),
        f"{prefix}working_builder_bcv2_builder_pubkey_rows": len(
            bcv2_builder_pubkey_rows
        ),
        f"{prefix}working_builder_bcv2_observed_pubkey_rows": len(
            bcv2_observed_pubkey_rows
        ),
        f"{prefix}working_builder_bcv2_observed_slot_rows": len(
            bcv2_observed_slot_rows
        ),
        f"{prefix}working_builder_bcv2_observed_tx_signature_rows": len(
            bcv2_observed_tx_signature_rows
        ),
        f"{prefix}working_builder_bcv2_precheck_context_slot_rows": len(
            bcv2_precheck_context_slot_rows
        ),
        f"{prefix}working_builder_bcv2_precheck_attempt_count_rows": len(
            bcv2_precheck_attempt_count_rows
        ),
        f"{prefix}working_builder_bcv2_precheck_latency_rows": len(
            bcv2_precheck_latency_rows
        ),
        f"{prefix}working_builder_bcv2_precheck_age_from_observed_slot_rows": len(
            bcv2_precheck_age_from_observed_slot_rows
        ),
        f"{prefix}working_builder_bcv2_loaded_address_source_missing_rows": len(
            bcv2_loaded_address_source_missing_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_lookup_performed_rows": len(
            bcv2_account_state_lookup_performed_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_seen_rows": len(
            bcv2_account_state_seen_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_seen_slot_rows": len(
            bcv2_account_state_seen_slot_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_age_slots_rows": len(
            bcv2_account_state_age_slots_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_owner_rows": len(
            bcv2_account_state_owner_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_data_len_rows": len(
            bcv2_account_state_data_len_rows
        ),
        f"{prefix}working_builder_bcv2_subscription_requested_rows": len(
            bcv2_subscription_requested_rows
        ),
        f"{prefix}working_builder_bcv2_account_update_received_rows": len(
            bcv2_account_update_received_rows
        ),
        f"{prefix}working_builder_bcv2_account_update_mapped_rows": len(
            bcv2_account_update_mapped_rows
        ),
        f"{prefix}working_builder_bcv2_rpc_fetch_ready_rows": len(
            bcv2_rpc_fetch_ready_rows
        ),
        f"{prefix}working_builder_bcv2_rpc_fetch_missing_rows": len(
            bcv2_rpc_fetch_missing_rows
        ),
        f"{prefix}working_builder_bcv2_rpc_fetch_owner_rows": len(
            bcv2_rpc_fetch_owner_rows
        ),
        f"{prefix}working_builder_bcv2_rpc_fetch_data_len_rows": len(
            bcv2_rpc_fetch_data_len_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_materialized_rows": len(
            bcv2_account_state_materialized_rows
        ),
        f"{prefix}working_builder_bcv2_mfs_materialized_rows": len(
            bcv2_mfs_materialized_rows
        ),
        f"{prefix}working_builder_bcv2_diag_materialized_rows": len(
            bcv2_diag_materialized_rows
        ),
        f"{prefix}working_builder_creator_vault_source_authority_counts": dict(
            sorted(creator_vault_source_authority_counts.items())
        ),
        f"{prefix}working_builder_creator_vault_rpc_load_status_counts": dict(
            sorted(creator_vault_rpc_load_status_counts.items())
        ),
        f"{prefix}working_builder_bcv2_authoritative_and_load_ready_rows": len(
            bcv2_authoritative_and_load_ready_rows
        ),
        f"{prefix}working_builder_bcv2_authoritative_but_missing_on_rpc_rows": len(
            bcv2_authoritative_but_missing_on_rpc_rows
        ),
        f"{prefix}working_builder_bcv2_pubkey_mismatch_rows": len(
            bcv2_pubkey_mismatch_rows
        ),
        f"{prefix}working_builder_bcv2_observed_tx_missing_on_rpc_rows": len(
            bcv2_observed_tx_missing_on_rpc_rows
        ),
        f"{prefix}working_builder_bcv2_account_state_missing_rows": len(
            bcv2_account_state_missing_rows
        ),
        f"{prefix}working_builder_creator_vault_authoritative_and_load_ready_rows": len(
            creator_vault_authoritative_and_load_ready_rows
        ),
        f"{prefix}working_builder_creator_vault_authoritative_but_missing_on_rpc_rows": len(
            creator_vault_authoritative_but_missing_on_rpc_rows
        ),
        f"{prefix}working_builder_creator_vault_source_mismatch_rows": len(
            creator_vault_source_mismatch_rows
        ),
        f"{prefix}working_builder_manifest_ready_after_account_source_repair_rows": len(
            manifest_ready_after_account_source_repair_rows
        ),
        f"{prefix}working_builder_manifest_still_not_ready_after_account_source_repair_rows": len(
            manifest_still_not_ready_after_account_source_repair_rows
        ),
    }


def is_legacy_buy_route_row(row: dict[str, Any]) -> bool:
    return (
        row_string(row, "fallback_route_kind") in LEGACY_ROUTE_VALUES
        or row_string(row, "selected_route_kind") in LEGACY_ROUTE_VALUES
        or row_string(row, "buy_variant") in LEGACY_ROUTE_VALUES
    )


def legacy_buy_curve_is_authoritative(row: dict[str, Any]) -> bool:
    authority = row_string(row, "legacy_buy_curve_authority_status")
    if authority and authority.startswith("authoritative_"):
        return True
    source = row_string(row, "legacy_buy_curve_source")
    return source in {
        "observed_tx_account_meta",
        "materialized_feature_set",
        "mfs",
        "account_state_core",
        "diag_account_update",
        "diag",
    }


def legacy_buy_route_ready(row: dict[str, Any]) -> bool:
    if row_bool_string(row, "legacy_buy_route_ready") == "true":
        return True
    return (
        row_string(row, "route_resolution_status") == "fallback_route_ready"
        and row_string(row, "selected_route_kind") in {"legacy_buy", "LegacyBuy"}
    )


def selected_legacy_fallback_route_ready(row: dict[str, Any]) -> bool:
    selected_legacy = row_string(row, "selected_route_kind") in {"legacy_buy", "LegacyBuy"}
    if not selected_legacy:
        return False
    if (
        row_string(row, "route_resolution_status") == "fallback_route_ready"
        and row_bool_string(row, "fallback_route_ready") == "true"
    ):
        return True
    return (
        row_string(row, "selected_route_source")
        == "selected_fallback_route_execution_handoff"
        and row_string(row, "selected_route_handoff_status")
        == "selected_route_handoff_applied"
    )


def row_primary_bcv2_terminal_reason(row: dict[str, Any]) -> bool:
    joined = ":".join(
        filter(
            None,
            [
                row_string(row, "no_executable_route_account_set_reason"),
                row_string(row, "precheck_failure_reason"),
                row_string(row, "execution_account_readiness_reason"),
                row_string(row, "primary_route_not_ready_reason"),
            ],
        )
    )
    return "primary_route_bcv2_missing" in joined or (
        "bonding_curve_v2_observed_meta_missing_on_rpc" in joined
        and "no_executable_route_account_set" in joined
    )


def selected_route_roles_contain_primary_bcv2(row: dict[str, Any]) -> bool:
    return any(
        role == "bonding_curve_v2" or role.startswith("bonding_curve_v2:")
        for role in row_string_list(row, "selected_route_account_set_roles")
    )


def selected_route_final_manifest_contains_role(row: dict[str, Any], role: str) -> bool:
    if any(
        value == role or value.startswith(f"{role}:")
        for value in row_string_list(row, "selected_route_account_set_roles")
    ):
        return True
    manifest = row.get("simulation_account_manifest")
    if isinstance(manifest, list):
        for entry in manifest:
            if isinstance(entry, dict) and row_string(entry, "role") == role:
                return True
    return False


def selected_route_final_manifest_contains_primary_route_builder_bcv2(
    row: dict[str, Any],
) -> bool:
    manifest = row.get("simulation_account_manifest")
    if isinstance(manifest, list):
        for entry in manifest:
            if (
                isinstance(entry, dict)
                and row_string(entry, "role") == "bonding_curve_v2"
                and row_string(entry, "source") == "route_builder"
            ):
                return True
    return (
        row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
        and row_string(row, "simulation_error_account_source") == "route_builder"
    )


def selected_route_hash_mismatch(row: dict[str, Any], selected_field: str, actual_field: str) -> bool:
    selected_hash = row_string(row, selected_field)
    actual_hash = row_string(row, actual_field)
    if not selected_hash or not actual_hash:
        return True
    return selected_hash != actual_hash


def legacy_buy_missing_core_curve(row: dict[str, Any]) -> bool:
    return (
        row_string(row, "legacy_buy_route_not_ready_reason")
        == "legacy_buy_missing_core_curve_account"
        or row_string(row, "fallback_route_not_ready_reason")
        == "fallback_route_missing_legacy_buy_curve"
        or row_string(row, "fallback_failure_class") == "fallback_missing_core_curve_account"
        or "bonding_curve" in set(row_string_list(row, "legacy_buy_missing_roles"))
        or "bonding_curve" in set(row_string_list(row, "fallback_missing_roles"))
    )


def legacy_buy_missing_associated_curve(row: dict[str, Any]) -> bool:
    return (
        row_string(row, "legacy_buy_route_not_ready_reason")
        == "legacy_buy_missing_associated_bonding_curve"
        or row_string(row, "fallback_failure_class")
        == "fallback_missing_associated_bonding_curve"
        or "associated_bonding_curve" in set(row_string_list(row, "legacy_buy_missing_roles"))
        or "associated_bonding_curve" in set(row_string_list(row, "fallback_missing_roles"))
    )


def legacy_buy_unsupported_builder_layout(row: dict[str, Any]) -> bool:
    joined = ":".join(
        filter(
            None,
            [
                row_string(row, "legacy_buy_route_not_ready_reason"),
                row_string(row, "fallback_route_not_ready_reason"),
                row_string(row, "fallback_failure_class"),
                row_string(row, "no_executable_route_account_set_reason"),
                row_string(row, "precheck_failure_reason"),
                row_string(row, "execution_account_readiness_reason"),
            ],
        )
    )
    return (
        "legacy_buy_unsupported_builder_layout_requires_bcv2" in joined
        or "unsupported_builder_layout_requires_bcv2" in joined
        or "fallback_unsupported_builder_layout" in joined
    )


def legacy_buy_route_payload(
    route_rows: list[dict[str, Any]],
    successful_entry_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    attempted_rows = [row for row in route_rows if is_legacy_buy_route_row(row)]
    ready_rows = [row for row in attempted_rows if legacy_buy_route_ready(row)]
    not_ready_rows = [row for row in attempted_rows if row not in ready_rows]
    missing_core_rows = [row for row in attempted_rows if legacy_buy_missing_core_curve(row)]
    missing_associated_rows = [
        row for row in attempted_rows if legacy_buy_missing_associated_curve(row)
    ]
    authoritative_curve_rows = [
        row for row in attempted_rows if legacy_buy_curve_is_authoritative(row)
    ]
    rpc_load_ready_rows = [
        row
        for row in attempted_rows
        if row_bool_string(row, "legacy_buy_curve_rpc_load_ready") == "true"
    ]
    success_rows = [row for row in successful_entry_rows if is_legacy_buy_route_row(row)]
    status_counts = Counter(
        row_string(row, "legacy_buy_account_set_status") or "unknown"
        for row in attempted_rows
    )
    source_counts = Counter(
        row_string(row, "legacy_buy_curve_source")
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_source")
    )
    authority_counts = Counter(
        row_string(row, "legacy_buy_curve_authority_status")
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_status")
    )
    rpc_status_counts = Counter(
        row_string(row, "legacy_buy_curve_rpc_load_status")
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_rpc_load_status")
    )
    authority_readiness_counts = Counter(
        row_string(row, "legacy_buy_curve_authority_readiness_status")
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_readiness_status")
    )
    authoritative_and_load_ready_rows = [
        row
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "authoritative_and_load_ready"
    ]
    load_ready_but_authority_unverified_rows = [
        row
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "load_ready_but_authority_unverified"
    ]
    authoritative_but_not_checked_rows = [
        row
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "authoritative_but_not_load_checked"
    ]
    derived_matches_account_state_rows = [
        row
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_status")
        == "authoritative_cross_checked"
        or row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "derived_matches_authoritative_source"
    ]
    derived_mismatch_account_state_rows = [
        row
        for row in attempted_rows
        if row_string(row, "legacy_buy_curve_authority_status")
        == "derived_mismatch_authoritative_source"
        or row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "derived_mismatch_authoritative_source"
    ]
    route_ready_after_reconciliation_rows = [
        row
        for row in ready_rows
        if row_string(row, "legacy_buy_curve_authority_readiness_status")
        == "authoritative_and_load_ready"
    ]
    route_still_not_ready_after_reconciliation_rows = [
        row
        for row in attempted_rows
        if row not in route_ready_after_reconciliation_rows
    ]
    not_ready_reason_counts = Counter(
        row_string(row, "legacy_buy_route_not_ready_reason")
        or row_string(row, "fallback_route_not_ready_reason")
        or "unknown"
        for row in not_ready_rows
    )
    primary_bcv2_leak_rows = [
        row
        for row in attempted_rows
        if (
            "primary_route_account_set"
            in set(row_string_list(row, "fallback_account_sources"))
            and (
                "bonding_curve_v2"
                in set(row_string_list(row, "fallback_missing_roles"))
                or any(
                    value.startswith("bonding_curve_v2:")
                    for value in row_string_list(
                        row, "fallback_required_precheck_account_set"
                    )
                    + row_string_list(row, "fallback_simulation_load_account_set")
                )
            )
        )
    ]
    missing_creatable_user_ata_rows = [
        row
        for row in attempted_rows
        if "user_ata" in set(row_string_list(row, "fallback_missing_roles"))
    ]
    missing_creatable_uva_rows = [
        row
        for row in attempted_rows
        if "user_volume_accumulator"
        in set(row_string_list(row, "fallback_missing_roles"))
    ]
    missing_ephemeral_payer_rows = [
        row
        for row in attempted_rows
        if "payer_pubkey" in set(row_string_list(row, "fallback_missing_roles"))
        and row_string(row, "payer_provenance") == "ephemeral"
    ]
    non_blocking_creatable_rows = [
        row
        for row in attempted_rows
        if (
            any(
                value.startswith("user_ata:")
                or value.startswith("user_volume_accumulator:")
                for value in row_string_list(row, "fallback_creatable_account_set")
            )
            and "user_ata" not in set(row_string_list(row, "fallback_missing_roles"))
            and "user_volume_accumulator"
            not in set(row_string_list(row, "fallback_missing_roles"))
        )
    ]
    non_blocking_ephemeral_payer_rows = [
        row
        for row in attempted_rows
        if row_string(row, "payer_provenance") == "ephemeral"
        and "payer_pubkey" not in set(row_string_list(row, "fallback_missing_roles"))
    ]
    selected_fallback_ready_rows = [
        row for row in attempted_rows if selected_legacy_fallback_route_ready(row)
    ]
    selected_legacy_handoff_claimed_rows = [
        row
        for row in attempted_rows
        if row_string(row, "selected_route_kind") in {"legacy_buy", "LegacyBuy"}
        or row_string(row, "selected_route_source")
        == "selected_fallback_route_execution_handoff"
        or row_string(row, "selected_route_handoff_status") in {
            "selected_route_handoff_applied",
            "selected_route_handoff_mismatch",
        }
    ]
    selected_legacy_handoff_mismatch_rows = [
        row
        for row in selected_legacy_handoff_claimed_rows
        if row_string(row, "selected_route_handoff_status")
        == "selected_route_handoff_mismatch"
    ]
    selected_fallback_handoff_applied_rows = [
        row
        for row in selected_fallback_ready_rows
        if row_string(row, "selected_route_handoff_status")
        == "selected_route_handoff_applied"
    ]
    selected_fallback_handoff_mismatch_rows = [
        row
        for row in selected_fallback_ready_rows
        if row_string(row, "selected_route_handoff_status")
        == "selected_route_handoff_mismatch"
    ]
    selected_fallback_handoff_not_applied_rows = [
        row
        for row in selected_fallback_ready_rows
        if row_string(row, "selected_route_handoff_status")
        != "selected_route_handoff_applied"
    ]
    selected_fallback_blocked_by_primary_reason_rows = [
        row
        for row in selected_fallback_ready_rows
        if row_primary_bcv2_terminal_reason(row)
    ]
    selected_but_request_variant_not_legacy_rows = [
        row
        for row in selected_fallback_ready_rows
        if row_string(row, "buy_variant")
        and row_string(row, "buy_variant") not in {"legacy_buy", "LegacyBuy"}
    ]
    selected_but_primary_bcv2_in_manifest_rows = [
        row
        for row in selected_legacy_handoff_claimed_rows
        if selected_route_final_manifest_contains_role(row, "bonding_curve_v2")
        or selected_route_roles_contain_primary_bcv2(row)
    ]
    selected_final_manifest_contains_primary_route_builder_rows = [
        row
        for row in selected_legacy_handoff_claimed_rows
        if selected_route_final_manifest_contains_primary_route_builder_bcv2(row)
    ]
    selected_but_precheck_hash_mismatch_rows = [
        row
        for row in selected_fallback_handoff_applied_rows
        if selected_route_hash_mismatch(
            row,
            "selected_route_precheck_hash",
            "precheck_account_set_hash",
        )
    ]
    selected_but_simulation_hash_mismatch_rows = [
        row
        for row in selected_fallback_handoff_applied_rows
        if selected_route_hash_mismatch(
            row,
            "selected_route_simulation_hash",
            "simulation_account_set_hash",
        )
    ]
    selected_precheck_uses_legacy_rows = [
        row
        for row in selected_fallback_handoff_applied_rows
        if row_string(row, "selected_route_precheck_hash")
        and not row_primary_bcv2_terminal_reason(row)
    ]
    selected_simulation_uses_legacy_rows = [
        row
        for row in selected_fallback_handoff_applied_rows
        if row_string(row, "selected_route_simulation_hash")
        and not row_primary_bcv2_terminal_reason(row)
    ]
    no_executable_route_but_simulated_rows = [
        row
        for row in attempted_rows
        if row_string(row, "route_resolution_status") == "no_executable_route_account_set"
        and (
            row_string(row, "simulation_error_kind")
            or row_string(row, "simulation_error_category")
            or row_string(row, "execution_outcome")
            in {
                "counterfactual_shadow_probe_simulation_error",
                "counterfactual_shadow_probe_simulation_failed",
                "shadow_simulation_error",
            }
        )
        and not (
            row_string(row, "precheck_failure_reason")
            or row_string(row, "execution_account_readiness_reason")
            or ""
        ).startswith("selected_route_handoff_mismatch:")
    ]
    unsupported_builder_layout_rows = [
        row for row in attempted_rows if legacy_buy_unsupported_builder_layout(row)
    ]
    removed_from_fallback_candidates_rows = [
        row
        for row in unsupported_builder_layout_rows
        if row_string(row, "fallback_route_kind") in {"legacy_buy", "LegacyBuy"}
        and row_bool_string(row, "fallback_route_attempted") != "true"
    ]
    return {
        "legacy_buy_route_attempted_rows": len(attempted_rows),
        "legacy_buy_route_ready_rows": len(ready_rows),
        "legacy_buy_route_not_ready_rows": len(not_ready_rows),
        "legacy_buy_missing_core_curve_account_rows": len(missing_core_rows),
        "legacy_buy_missing_associated_bonding_curve_rows": len(missing_associated_rows),
        "legacy_buy_authoritative_curve_rows": len(authoritative_curve_rows),
        "legacy_buy_rpc_load_ready_rows": len(rpc_load_ready_rows),
        "legacy_buy_successful_entry_rows": len(success_rows),
        "legacy_buy_account_set_status_counts": dict(sorted(status_counts.items())),
        "legacy_buy_curve_source_counts": dict(sorted(source_counts.items())),
        "legacy_buy_curve_authority_status_counts": dict(sorted(authority_counts.items())),
        "legacy_buy_curve_rpc_load_status_counts": dict(sorted(rpc_status_counts.items())),
        "legacy_buy_curve_authority_readiness_status_counts": dict(
            sorted(authority_readiness_counts.items())
        ),
        "legacy_buy_curve_authoritative_and_load_ready_rows": len(
            authoritative_and_load_ready_rows
        ),
        "legacy_buy_curve_load_ready_but_authority_unverified_rows": len(
            load_ready_but_authority_unverified_rows
        ),
        "legacy_buy_curve_authoritative_but_not_checked_rows": len(
            authoritative_but_not_checked_rows
        ),
        "legacy_buy_curve_derived_matches_account_state_rows": len(
            derived_matches_account_state_rows
        ),
        "legacy_buy_curve_derived_mismatch_account_state_rows": len(
            derived_mismatch_account_state_rows
        ),
        "legacy_buy_route_ready_after_reconciliation_rows": len(
            route_ready_after_reconciliation_rows
        ),
        "legacy_buy_route_still_not_ready_after_reconciliation_rows": len(
            route_still_not_ready_after_reconciliation_rows
        ),
        "legacy_buy_route_not_ready_reason_counts": dict(
            sorted(not_ready_reason_counts.items())
        ),
        "legacy_buy_primary_bcv2_leak_rows": len(primary_bcv2_leak_rows),
        "legacy_buy_missing_creatable_user_ata_rows": len(
            missing_creatable_user_ata_rows
        ),
        "legacy_buy_missing_creatable_user_volume_accumulator_rows": len(
            missing_creatable_uva_rows
        ),
        "legacy_buy_missing_ephemeral_payer_rows": len(
            missing_ephemeral_payer_rows
        ),
        "legacy_buy_blocking_missing_required_rows": len(
            [
                row
                for row in attempted_rows
                if row_string_list(row, "fallback_missing_roles")
            ]
        ),
        "legacy_buy_non_blocking_missing_creatable_rows": len(
            non_blocking_creatable_rows
        ),
        "legacy_buy_non_blocking_ephemeral_payer_rows": len(
            non_blocking_ephemeral_payer_rows
        ),
        "legacy_buy_fallback_account_set_ready_rows": len(ready_rows),
        "legacy_buy_route_ready_after_account_set_separation_rows": len(
            ready_rows
        ),
        "selected_fallback_route_ready_rows": len(selected_fallback_ready_rows),
        "selected_fallback_route_handoff_applied_rows": len(
            selected_fallback_handoff_applied_rows
        ),
        "selected_fallback_route_handoff_mismatch_rows": len(
            selected_fallback_handoff_mismatch_rows
        ),
        "selected_fallback_route_handoff_not_applied_rows": len(
            selected_fallback_handoff_not_applied_rows
        ),
        "selected_fallback_route_blocked_by_primary_reason_rows": len(
            selected_fallback_blocked_by_primary_reason_rows
        ),
        "legacy_buy_selected_but_primary_bcv2_terminal_rows": len(
            selected_fallback_blocked_by_primary_reason_rows
        ),
        "selected_legacy_handoff_claimed_rows": len(
            selected_legacy_handoff_claimed_rows
        ),
        "selected_legacy_handoff_validated_rows": len(
            selected_fallback_handoff_applied_rows
        ),
        "selected_legacy_handoff_mismatch_rows": len(
            selected_legacy_handoff_mismatch_rows
        ),
        "selected_legacy_final_manifest_contains_bcv2_rows": len(
            selected_but_primary_bcv2_in_manifest_rows
        ),
        "selected_legacy_final_manifest_contains_primary_route_builder_rows": len(
            selected_final_manifest_contains_primary_route_builder_rows
        ),
        "selected_legacy_request_variant_not_legacy_rows": len(
            selected_but_request_variant_not_legacy_rows
        ),
        "selected_legacy_precheck_hash_mismatch_rows": len(
            selected_but_precheck_hash_mismatch_rows
        ),
        "selected_legacy_simulation_hash_mismatch_rows": len(
            selected_but_simulation_hash_mismatch_rows
        ),
        "no_executable_route_but_simulated_rows": len(
            no_executable_route_but_simulated_rows
        ),
        "legacy_buy_selected_but_request_variant_not_legacy_rows": len(
            selected_but_request_variant_not_legacy_rows
        ),
        "legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows": len(
            selected_but_primary_bcv2_in_manifest_rows
        ),
        "legacy_buy_selected_but_precheck_hash_mismatch_rows": len(
            selected_but_precheck_hash_mismatch_rows
        ),
        "legacy_buy_selected_but_simulation_hash_mismatch_rows": len(
            selected_but_simulation_hash_mismatch_rows
        ),
        "legacy_buy_selected_and_precheck_uses_legacy_account_set_rows": len(
            selected_precheck_uses_legacy_rows
        ),
        "legacy_buy_selected_and_simulation_uses_legacy_account_set_rows": len(
            selected_simulation_uses_legacy_rows
        ),
        "legacy_buy_route_unsupported_builder_layout_rows": len(
            unsupported_builder_layout_rows
        ),
        "legacy_buy_excluded_from_execution_route_universe_rows": len(
            unsupported_builder_layout_rows
        ),
        "legacy_buy_removed_from_fallback_candidates_rows": len(
            removed_from_fallback_candidates_rows
        ),
    }


EXECUTABLE_ROUTE_STATUSES = {"primary_route_ready", "fallback_route_ready"}
NON_EXECUTABLE_ROUTE_STATUSES = {"no_executable_route_account_set"}


def is_no_executable_route_row(row: dict[str, Any]) -> bool:
    return (
        row_string(row, "route_resolution_status") in NON_EXECUTABLE_ROUTE_STATUSES
        or row_string(row, "probe_skip_reason") == "no_executable_route_account_set"
        or row_string(row, "execution_outcome") == "no_executable_route_account_set"
        or "no_executable_route_account_set"
        in (
            row_string(row, "precheck_failure_reason")
            or row_string(row, "execution_account_readiness_reason")
            or row_string(row, "no_executable_route_account_set_reason")
            or ""
        )
    )


def execution_feasibility_status_for_row(row: dict[str, Any]) -> str:
    explicit = row_string(row, "execution_feasibility_status")
    if explicit:
        return explicit
    route_status = row_string(row, "route_resolution_status")
    if route_status in EXECUTABLE_ROUTE_STATUSES or row_string(row, "selected_route_kind"):
        return "executable"
    if is_no_executable_route_row(row):
        return "not_executable_route"
    probe_skip_reason = row_string(row, "probe_skip_reason")
    if probe_skip_reason in {
        "creator_vault_source_not_authoritative",
        "bonding_curve_v2_source_not_authoritative",
        "route_account_source_not_authoritative",
        "missing_execution_route_identity",
    }:
        return "not_executable_route_identity"
    precheck_reason = row_string(row, "precheck_failure_reason") or ""
    if (
        "creator_vault_source_not_authoritative" in precheck_reason
        or "source_not_authoritative" in precheck_reason
    ):
        return "not_executable_route_identity"
    if (
        "execution_account_not_ready" in precheck_reason
        or row_string(row, "execution_account_readiness_status") == "not_ready"
    ):
        return "not_executable_account_readiness"
    if is_simulation_error_entry(row) or row_string(row, "simulation_error_kind"):
        return "simulation_error"
    return "unknown"


def execution_feasibility_reason_for_row(row: dict[str, Any]) -> str:
    explicit = row_string(row, "execution_feasibility_reason")
    if explicit:
        return explicit
    if is_no_executable_route_row(row):
        return "no_executable_route_account_set"
    return (
        row_string(row, "route_resolution_terminal_reason")
        or row_string(row, "fallback_failure_class")
        or row_string(row, "probe_skip_reason")
        or row_string(row, "precheck_failure_reason")
        or row_string(row, "execution_account_readiness_reason")
        or row_string(row, "simulation_error_category")
        or row_string(row, "simulation_error_kind")
        or "unknown"
    )


def lifecycle_label_eligibility_for_row(row: dict[str, Any]) -> str:
    explicit = row_string(row, "lifecycle_label_eligibility")
    if explicit:
        return explicit
    if is_no_executable_route_row(row):
        return "not_lifecycle_label_eligible"
    if row_string(row, "probe_lifecycle_eligibility_status") == "lifecycle_eligible":
        return "lifecycle_label_eligible"
    if row_string(row, "active_shadow_lifecycle_eligibility_status") == "lifecycle_eligible":
        return "lifecycle_label_eligible"
    return "unknown"


def execution_feasibility_payload(rows: list[dict[str, Any]]) -> dict[str, Any]:
    status_counts = Counter(execution_feasibility_status_for_row(row) for row in rows)
    reason_counts = Counter(execution_feasibility_reason_for_row(row) for row in rows)
    lifecycle_counts = Counter(lifecycle_label_eligibility_for_row(row) for row in rows)
    route_executable_rows = sum(
        1 for row in rows if execution_feasibility_status_for_row(row) == "executable"
    )
    route_non_executable_rows = sum(
        1
        for row in rows
        if execution_feasibility_status_for_row(row).startswith("not_executable")
    )
    execution_feasibility_reject_rows = sum(1 for row in rows if is_no_executable_route_row(row))
    return {
        "execution_feasibility_status_counts": dict(sorted(status_counts.items())),
        "execution_feasibility_reason_counts": dict(sorted(reason_counts.items())),
        "lifecycle_label_eligibility_counts": dict(sorted(lifecycle_counts.items())),
        "route_executable_rows": route_executable_rows,
        "route_non_executable_rows": route_non_executable_rows,
        "execution_feasibility_reject_rows": execution_feasibility_reject_rows,
    }


def is_simulation_error_entry(row: dict[str, Any]) -> bool:
    status = row_string(row, "probe_entry_materialization_status")
    execution_outcome = row_string(row, "execution_outcome")
    return (
        status == "simulation_error"
        or execution_outcome == "counterfactual_shadow_probe_simulation_error"
        or bool(row_string(row, "simulation_error_kind"))
        or bool(row_string(row, "simulation_error_category"))
        or bool(row_string(row, "error_class"))
    )


def classify_probe_transport_materialization(row: dict[str, Any], entry_probe_ids: set[str]) -> tuple[str, str]:
    explicit_status = row_string(row, "probe_entry_materialization_status")
    if explicit_status == "simulation_error":
        reason = (
            row_string(row, "simulation_error_category")
            or row_string(row, "error_class")
            or row_string(row, "simulation_error_kind")
            or "simulation_error"
        )
        custom_code = row_string(row, "simulation_error_custom_code")
        if custom_code:
            reason = f"{reason}:custom_{custom_code}"
        return "simulation_error", reason
    if explicit_status in {"entry_materialized", "transport_only", "lifecycle_eligible"}:
        reason = row_string(row, "probe_lifecycle_eligibility_status") or "explicit_entry_status"
        return explicit_status, reason

    probe_id = row_probe_id(row)
    execution_outcome = row_string(row, "execution_outcome")
    error_class = row_string(row, "error_class")
    simulation_error_category = row_string(row, "simulation_error_category")
    simulation_error_kind = row_string(row, "simulation_error_kind")
    simulation_error_custom_code = row_string(row, "simulation_error_custom_code")
    if (
        error_class
        or simulation_error_category
        or simulation_error_kind
        or simulation_error_custom_code
        or execution_outcome == "counterfactual_shadow_probe_simulation_error"
    ):
        reason = simulation_error_category or error_class or simulation_error_kind or "simulation_error"
        if simulation_error_custom_code:
            reason = f"{reason}:custom_{simulation_error_custom_code}"
        return "simulation_error", reason

    if probe_id and probe_id in entry_probe_ids:
        return "entry_materialized", "entry_row_present"

    readiness_status = row_string(row, "execution_account_readiness_status")
    precheck_failure_reason = row_string(row, "precheck_failure_reason")
    if readiness_status == "not_ready" or (
        precheck_failure_reason and "execution_account_not_ready" in precheck_failure_reason
    ):
        return "execution_account_not_ready", precheck_failure_reason or "execution_account_not_ready"

    buy_variant = row_string(row, "buy_variant")
    token_param_role = row_string(row, "token_param_role")
    entry_token_amount_raw = row.get("entry_token_amount_raw")
    if entry_token_amount_raw is None:
        if buy_variant == "routed_exact_sol_in" and token_param_role == "min_tokens_out":
            return (
                "transport_only_missing_token_quantity",
                "routed_exact_sol_in_entry_token_amount_raw_null",
            )
        return "transport_only_missing_token_quantity", "entry_token_amount_raw_null"

    return "unknown", "entry_missing_unclassified"


def probe_entry_materialization(paths: dict[str, list[Path]]) -> dict[str, Any]:
    selection_rows = artifact_rows(paths, "probe_selection")
    transport_rows = artifact_rows(paths, "probe_transport")
    entry_rows = artifact_rows(paths, "probe_entry")
    skip_rows = artifact_rows(paths, "probe_skip")
    lifecycle_rows = artifact_rows(paths, "probe_lifecycle")
    entry_probe_ids = {probe_id for row in entry_rows if (probe_id := row_probe_id(row))}

    status_counts: Counter[str] = Counter()
    reason_counts: Counter[str] = Counter()
    buy_variant_counts: Counter[str] = Counter()
    token_param_role_counts: Counter[str] = Counter()
    creator_vault_authority_status_counts: Counter[str] = Counter()
    creator_vault_mismatch_reason_counts: Counter[str] = Counter()
    creator_identity_source_counts: Counter[str] = Counter()
    bonding_curve_v2_authority_status_counts: Counter[str] = Counter()
    bonding_curve_v2_identity_authority_status_counts: Counter[str] = Counter()
    bonding_curve_v2_mismatch_reason_counts: Counter[str] = Counter()
    bonding_curve_v2_source_counts: Counter[str] = Counter()
    bonding_curve_v2_rpc_load_status_counts: Counter[str] = Counter()
    bonding_curve_v2_rpc_load_ready_counts: Counter[str] = Counter()
    builder_required_curve_account_ready_counts: Counter[str] = Counter()
    builder_required_curve_account_ready_reason_counts: Counter[str] = Counter()
    observed_bcv2_provenance_status_counts: Counter[str] = Counter()
    route_resolution_status_counts: Counter[str] = Counter()
    selected_route_kind_counts: Counter[str] = Counter()
    route_fallback_status_counts: Counter[str] = Counter()
    amount_guard_status_counts: Counter[str] = Counter()
    simulation_error_custom_code_counts: Counter[str] = Counter()
    simulation_error_kind_counts: Counter[str] = Counter()
    simulation_error_account_role_counts: Counter[str] = Counter()
    simulation_error_account_source_counts: Counter[str] = Counter()
    simulation_error_category_counts: Counter[str] = Counter()
    simulation_error_account_narrowing_status_counts: Counter[str] = Counter()
    account_not_found_candidate_raw_counts: Counter[str] = Counter()
    account_not_found_candidate_narrowed_counts: Counter[str] = Counter()
    candidate_class_counts: Counter[str] = Counter()
    candidate_exclusion_reason_counts: Counter[str] = Counter()
    account_set_match_counts: Counter[str] = Counter()
    account_set_mismatch_reason_counts: Counter[str] = Counter()
    rows: list[dict[str, Any]] = []
    for row in transport_rows:
        status, reason = classify_probe_transport_materialization(row, entry_probe_ids)
        status_counts[status] += 1
        reason_counts[reason] += 1
        buy_variant = row_string(row, "buy_variant")
        token_param_role = row_string(row, "token_param_role")
        if buy_variant:
            buy_variant_counts[buy_variant] += 1
        if token_param_role:
            token_param_role_counts[token_param_role] += 1
        creator_vault_authority_status = row_string(row, "creator_vault_authority_status")
        creator_vault_mismatch_reason = row_string(row, "creator_vault_mismatch_reason")
        creator_identity_source = row_string(row, "creator_identity_source")
        bonding_curve_v2_authority_status = row_string(row, "bonding_curve_v2_authority_status")
        bonding_curve_v2_identity_authority_status = row_string(
            row,
            "bonding_curve_v2_identity_authority_status",
        )
        bonding_curve_v2_mismatch_reason = row_string(row, "bonding_curve_v2_mismatch_reason")
        bonding_curve_v2_source = row_string(row, "bonding_curve_v2_source")
        bonding_curve_v2_rpc_load_status = row_string(row, "bonding_curve_v2_rpc_load_status")
        bonding_curve_v2_rpc_load_ready = row_bool_string(
            row,
            "bonding_curve_v2_rpc_load_ready",
        )
        builder_required_curve_account_ready = row_bool_string(
            row,
            "builder_required_curve_account_ready",
        )
        builder_required_curve_account_ready_reason = row_string(
            row,
            "builder_required_curve_account_ready_reason",
        )
        observed_bcv2_provenance_status = row_string(row, "observed_bcv2_provenance_status")
        route_resolution_status = row_string(row, "route_resolution_status")
        selected_route_kind = row_string(row, "selected_route_kind")
        route_fallback_status = (
            row_string(row, "route_fallback_status")
            or row_string(row, "fallback_route_status")
            or row_bool_string(row, "fallback_route_attempted")
            or row_bool_string(row, "route_fallback_attempted")
        )
        amount_guard_status = row_string(row, "amount_guard_status")
        simulation_error_category = row_string(row, "simulation_error_category")
        simulation_error_kind = row_string(row, "simulation_error_kind")
        simulation_error_account_role = row_string(row, "simulation_error_account_role")
        simulation_error_account_source = row_string(row, "simulation_error_account_source")
        simulation_error_custom_code = row_string(row, "simulation_error_custom_code")
        simulation_error_account_narrowing_status = row_string(
            row,
            "simulation_error_account_narrowing_status",
        )
        account_set_match = row_bool_string(row, "account_set_match")
        account_set_mismatch_reason = row_string(row, "account_set_mismatch_reason")
        if creator_vault_authority_status:
            creator_vault_authority_status_counts[creator_vault_authority_status] += 1
        if creator_vault_mismatch_reason:
            creator_vault_mismatch_reason_counts[creator_vault_mismatch_reason] += 1
        if creator_identity_source:
            creator_identity_source_counts[creator_identity_source] += 1
        if bonding_curve_v2_authority_status:
            bonding_curve_v2_authority_status_counts[bonding_curve_v2_authority_status] += 1
        if bonding_curve_v2_identity_authority_status:
            bonding_curve_v2_identity_authority_status_counts[
                bonding_curve_v2_identity_authority_status
            ] += 1
        if bonding_curve_v2_mismatch_reason:
            bonding_curve_v2_mismatch_reason_counts[bonding_curve_v2_mismatch_reason] += 1
        if bonding_curve_v2_source:
            bonding_curve_v2_source_counts[bonding_curve_v2_source] += 1
        if bonding_curve_v2_rpc_load_status:
            bonding_curve_v2_rpc_load_status_counts[bonding_curve_v2_rpc_load_status] += 1
        if bonding_curve_v2_rpc_load_ready:
            bonding_curve_v2_rpc_load_ready_counts[bonding_curve_v2_rpc_load_ready] += 1
        if builder_required_curve_account_ready:
            builder_required_curve_account_ready_counts[builder_required_curve_account_ready] += 1
        if builder_required_curve_account_ready_reason:
            builder_required_curve_account_ready_reason_counts[
                builder_required_curve_account_ready_reason
            ] += 1
        if observed_bcv2_provenance_status:
            observed_bcv2_provenance_status_counts[observed_bcv2_provenance_status] += 1
        if route_resolution_status:
            route_resolution_status_counts[route_resolution_status] += 1
        if selected_route_kind:
            selected_route_kind_counts[selected_route_kind] += 1
        if route_fallback_status:
            route_fallback_status_counts[route_fallback_status] += 1
        if amount_guard_status:
            amount_guard_status_counts[amount_guard_status] += 1
        if simulation_error_category:
            simulation_error_category_counts[simulation_error_category] += 1
        if simulation_error_kind:
            simulation_error_kind_counts[simulation_error_kind] += 1
        if simulation_error_account_role:
            simulation_error_account_role_counts[simulation_error_account_role] += 1
        if simulation_error_account_source:
            simulation_error_account_source_counts[simulation_error_account_source] += 1
        if simulation_error_custom_code:
            simulation_error_custom_code_counts[f"custom_{simulation_error_custom_code}"] += 1
        if simulation_error_account_narrowing_status:
            simulation_error_account_narrowing_status_counts[
                simulation_error_account_narrowing_status
            ] += 1
        raw_candidates = iter_account_candidates(row, "simulation_error_account_candidates_raw")
        narrowed_candidates = iter_account_candidates(
            row,
            "simulation_error_account_candidates_narrowed",
        )
        excluded_candidates = iter_account_candidates(
            row,
            "simulation_error_account_candidates_excluded",
        )
        for candidate in raw_candidates:
            role = candidate.get("role")
            if role:
                account_not_found_candidate_raw_counts[str(role)] += 1
            candidate_class = candidate.get("candidate_class")
            if candidate_class:
                candidate_class_counts[str(candidate_class)] += 1
        if not raw_candidates:
            for candidate in narrowed_candidates + excluded_candidates:
                candidate_class = candidate.get("candidate_class")
                if candidate_class:
                    candidate_class_counts[str(candidate_class)] += 1
        for candidate in narrowed_candidates:
            role = candidate.get("role")
            if role:
                account_not_found_candidate_narrowed_counts[str(role)] += 1
        for candidate in excluded_candidates:
            exclusion_reason = candidate.get("candidate_exclusion_reason")
            if exclusion_reason:
                candidate_exclusion_reason_counts[str(exclusion_reason)] += 1
        if account_set_match:
            account_set_match_counts[account_set_match] += 1
        if account_set_mismatch_reason:
            account_set_mismatch_reason_counts[account_set_mismatch_reason] += 1
        rows.append(
            {
                "probe_id": row_probe_id(row),
                "ab_record_id": row_ab_record_id(row),
                "candidate_id": row_string(row, "candidate_id"),
                "pool_id": row_string(row, "pool_id"),
                "base_mint": row_string(row, "base_mint") or row_string(row, "mint_id"),
                "buy_variant": buy_variant,
                "token_param_role": token_param_role,
                "entry_token_amount_raw": row.get("entry_token_amount_raw"),
                "min_tokens_out": row.get("min_tokens_out"),
                "execution_outcome": row_string(row, "execution_outcome"),
                "error_class": row_string(row, "error_class"),
                "simulation_error_category": simulation_error_category,
                "simulation_error_kind": simulation_error_kind,
                "simulation_error_custom_code": simulation_error_custom_code,
                "simulation_error_account_pubkey": row_string(row, "simulation_error_account_pubkey"),
                "simulation_error_account_role": simulation_error_account_role,
                "simulation_error_account_source": simulation_error_account_source,
                "simulation_error_account_candidates": row.get("simulation_error_account_candidates"),
                "simulation_error_account_candidates_raw": row.get(
                    "simulation_error_account_candidates_raw"
                ),
                "simulation_error_account_candidates_narrowed": row.get(
                    "simulation_error_account_candidates_narrowed"
                ),
                "simulation_error_account_candidates_excluded": row.get(
                    "simulation_error_account_candidates_excluded"
                ),
                "simulation_error_account_narrowing_status": (
                    simulation_error_account_narrowing_status
                ),
                "simulation_error_account_narrowing_reason": row_string(
                    row,
                    "simulation_error_account_narrowing_reason",
                ),
                "simulation_error_actual_account_pubkey": row_string(
                    row,
                    "simulation_error_actual_account_pubkey",
                ),
                "simulation_error_expected_account_pubkey": row_string(
                    row,
                    "simulation_error_expected_account_pubkey",
                ),
                "creator_vault_authority_status": creator_vault_authority_status,
                "creator_vault_actual_pubkey": row_string(row, "creator_vault_actual_pubkey"),
                "creator_vault_expected_pubkey": row_string(row, "creator_vault_expected_pubkey"),
                "creator_vault_mismatch_reason": creator_vault_mismatch_reason,
                "creator_identity_source": creator_identity_source,
                "creator_identity_authoritative": row.get("creator_identity_authoritative"),
                "bonding_curve_v2_pubkey": row_string(row, "bonding_curve_v2_pubkey"),
                "bonding_curve_v2_source": bonding_curve_v2_source,
                "bonding_curve_v2_authority_status": bonding_curve_v2_authority_status,
                "bonding_curve_v2_identity_authority_status": (
                    bonding_curve_v2_identity_authority_status
                ),
                "bonding_curve_v2_mismatch_reason": bonding_curve_v2_mismatch_reason,
                "bonding_curve_pubkey_from_diag": row_string(row, "bonding_curve_pubkey_from_diag"),
                "bonding_curve_pubkey_from_mfs": row_string(row, "bonding_curve_pubkey_from_mfs"),
                "bonding_curve_v2_seen_in_diag": row.get("bonding_curve_v2_seen_in_diag"),
                "bonding_curve_v2_seen_in_mfs": row.get("bonding_curve_v2_seen_in_mfs"),
                "bonding_curve_v2_seen_in_account_state": row.get(
                    "bonding_curve_v2_seen_in_account_state"
                ),
                "bonding_curve_ready": row.get("bonding_curve_ready"),
                "bonding_curve_v2_rpc_load_status": bonding_curve_v2_rpc_load_status,
                "bonding_curve_v2_rpc_load_ready": row.get("bonding_curve_v2_rpc_load_ready"),
                "bonding_curve_v2_ready": row.get("bonding_curve_v2_ready"),
                "builder_required_curve_account_ready": row.get(
                    "builder_required_curve_account_ready"
                ),
                "builder_required_curve_account_ready_reason": (
                    builder_required_curve_account_ready_reason
                ),
                "observed_bcv2_source_tx_signature": row_string(
                    row,
                    "observed_bcv2_source_tx_signature",
                ),
                "observed_bcv2_source_slot": row.get("observed_bcv2_source_slot"),
                "observed_bcv2_source_slot_index": row.get("observed_bcv2_source_slot_index"),
                "observed_bcv2_source_instruction_index": row.get(
                    "observed_bcv2_source_instruction_index"
                ),
                "observed_bcv2_source_program_id": row_string(
                    row,
                    "observed_bcv2_source_program_id",
                ),
                "observed_bcv2_source_discriminator": row_string(
                    row,
                    "observed_bcv2_source_discriminator",
                ),
                "observed_bcv2_source_buy_variant": row_string(
                    row,
                    "observed_bcv2_source_buy_variant",
                ),
                "observed_bcv2_instruction_account_position": row.get(
                    "observed_bcv2_instruction_account_position"
                ),
                "observed_bcv2_message_account_index": row.get(
                    "observed_bcv2_message_account_index"
                ),
                "observed_bcv2_resolved_pubkey": row_string(row, "observed_bcv2_resolved_pubkey"),
                "observed_bcv2_loaded_address_source": row_string(
                    row,
                    "observed_bcv2_loaded_address_source",
                ),
                "observed_bcv2_tx_success": row.get("observed_bcv2_tx_success"),
                "observed_bcv2_meta_err": row_string(row, "observed_bcv2_meta_err"),
                "observed_bcv2_provenance_status": observed_bcv2_provenance_status,
                "amount_guard_status": amount_guard_status,
                "amount_provided_lamports_if_available": row.get(
                    "amount_provided_lamports_if_available"
                ),
                "amount_required_lamports_if_available": row.get(
                    "amount_required_lamports_if_available"
                ),
                "amount_shortfall_lamports_if_available": row.get(
                    "amount_shortfall_lamports_if_available"
                ),
                "execution_account_readiness_status": row_string(
                    row,
                    "execution_account_readiness_status",
                ),
                "precheck_account_set_hash": row_string(row, "precheck_account_set_hash"),
                "prepared_request_account_set_hash": row_string(
                    row,
                    "prepared_request_account_set_hash",
                ),
                "simulation_account_set_hash": row_string(row, "simulation_account_set_hash"),
                "account_set_match": account_set_match,
                "account_set_mismatch_reason": account_set_mismatch_reason,
                "probe_entry_materialization_status": status,
                "probe_entry_materialization_reason": reason,
            }
        )

    skip_reason_counts: Counter[str] = Counter()
    skip_execution_account_readiness_role_counts: Counter[str] = Counter()
    skip_execution_account_readiness_reason_counts: Counter[str] = Counter()
    skip_creator_vault_authority_status_counts: Counter[str] = Counter()
    skip_creator_vault_mismatch_reason_counts: Counter[str] = Counter()
    skip_creator_identity_source_counts: Counter[str] = Counter()
    skip_bonding_curve_v2_authority_status_counts: Counter[str] = Counter()
    skip_bonding_curve_v2_mismatch_reason_counts: Counter[str] = Counter()
    skip_bonding_curve_v2_source_counts: Counter[str] = Counter()
    skip_route_fallback_status_counts: Counter[str] = Counter()
    for row in skip_rows:
        reason = row_string(row, "probe_skip_reason") or row_string(row, "skip_reason")
        if reason:
            skip_reason_counts[reason] += 1
        execution_account_readiness_role = row_string(row, "execution_account_readiness_role")
        execution_account_readiness_reason = row_string(row, "execution_account_readiness_reason")
        if execution_account_readiness_role:
            skip_execution_account_readiness_role_counts[execution_account_readiness_role] += 1
        if execution_account_readiness_reason:
            skip_execution_account_readiness_reason_counts[execution_account_readiness_reason] += 1
        creator_vault_authority_status = row_string(row, "creator_vault_authority_status")
        creator_vault_mismatch_reason = row_string(row, "creator_vault_mismatch_reason")
        creator_identity_source = row_string(row, "creator_identity_source")
        bonding_curve_v2_authority_status = row_string(row, "bonding_curve_v2_authority_status")
        bonding_curve_v2_mismatch_reason = row_string(row, "bonding_curve_v2_mismatch_reason")
        bonding_curve_v2_source = row_string(row, "bonding_curve_v2_source")
        observed_bcv2_provenance_status = row_string(row, "observed_bcv2_provenance_status")
        route_resolution_status = row_string(row, "route_resolution_status")
        selected_route_kind = row_string(row, "selected_route_kind")
        route_fallback_status = (
            row_string(row, "route_fallback_status")
            or row_string(row, "fallback_route_status")
            or row_bool_string(row, "fallback_route_attempted")
            or row_bool_string(row, "route_fallback_attempted")
        )
        if creator_vault_authority_status:
            skip_creator_vault_authority_status_counts[creator_vault_authority_status] += 1
        if creator_vault_mismatch_reason:
            skip_creator_vault_mismatch_reason_counts[creator_vault_mismatch_reason] += 1
        if creator_identity_source:
            skip_creator_identity_source_counts[creator_identity_source] += 1
        if bonding_curve_v2_authority_status:
            skip_bonding_curve_v2_authority_status_counts[bonding_curve_v2_authority_status] += 1
        if bonding_curve_v2_mismatch_reason:
            skip_bonding_curve_v2_mismatch_reason_counts[bonding_curve_v2_mismatch_reason] += 1
        if bonding_curve_v2_source:
            skip_bonding_curve_v2_source_counts[bonding_curve_v2_source] += 1
        if observed_bcv2_provenance_status:
            observed_bcv2_provenance_status_counts[observed_bcv2_provenance_status] += 1
        if route_resolution_status:
            route_resolution_status_counts[route_resolution_status] += 1
        if selected_route_kind:
            selected_route_kind_counts[selected_route_kind] += 1
        if route_fallback_status:
            skip_route_fallback_status_counts[route_fallback_status] += 1

    transport_rows_total = len(transport_rows)
    entry_rows_total = len(entry_rows)
    account_not_found_rows = [row for row in transport_rows if is_account_not_found_row(row)]
    account_not_found_attributed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_account_not_found_attributed"
        or (
            row_string(row, "simulation_error_account_pubkey")
            and row_string(row, "simulation_error_account_role")
        )
    ]
    account_not_found_multi_candidate_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category")
        in {
            "simulation_account_not_found_multi_candidate",
            "simulation_account_not_found_multi_candidate_narrow",
        }
    ]
    exact_after_narrowing_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_account_narrowing_status")
        == "exact_after_narrowing"
    ]
    multi_candidate_narrowed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_account_narrowing_status")
        == "multi_candidate_narrowed"
        or row_string(row, "simulation_error_category")
        == "simulation_account_not_found_multi_candidate_narrow"
    ]
    unattributed_after_narrowing_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_account_narrowing_status")
        == "unattributed_after_narrowing"
    ]
    all_candidates_nonfatal_but_sim_failed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_account_narrowing_status")
        == "all_candidates_nonfatal_but_sim_failed"
        or row_string(row, "simulation_error_category") == "all_candidates_nonfatal_but_sim_failed"
    ]
    account_not_found_unattributed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_account_not_found_unattributed"
        or row_string(row, "simulation_error_account_narrowing_status")
        == "unattributed_after_narrowing"
    ]
    simulation_rpc_visibility_gap_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_rpc_visibility_gap"
    ]
    bonding_curve_v2_account_not_found_after_simulation_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
    ]
    simulation_required_account_not_in_precheck_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category")
        == "simulation_required_account_not_in_precheck"
        or (
            row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
            and row_bool_string(row, "account_set_match") == "true"
        )
    ]
    simulation_account_meta_missing_on_rpc_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_account_meta_missing_on_rpc"
        or row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
    ]
    bonding_curve_v2_precheck_skipped_before_simulation_rows = [
        row
        for row in skip_rows
        if (
            row_string(row, "probe_skip_reason") == "execution_account_not_ready"
            and row_string(row, "execution_account_readiness_role") == "bonding_curve_v2"
        )
        or (
            row_string(row, "precheck_failure_reason") or ""
        ).startswith("execution_account_not_ready:bonding_curve_v2:")
    ]
    route_excluded_bcv2_missing_rows = [
        row
        for row in skip_rows + transport_rows
        if (
            row_string(row, "probe_skip_reason")
            in {
                "bonding_curve_v2_source_not_authoritative",
                "route_account_source_not_authoritative",
                "no_executable_route_account_set",
            }
        )
        or "bonding_curve_v2_source_not_authoritative"
        in (row_string(row, "precheck_failure_reason") or "")
        or row_string(row, "bonding_curve_v2_authority_status")
        in {"builder_only", "derived_unverified"}
    ]
    route_fallback_attempted_rows = [
        row
        for row in skip_rows + transport_rows
        if row_bool_string(row, "fallback_route_attempted") == "true"
        or row_bool_string(row, "route_fallback_attempted") == "true"
        or bool(row_string(row, "route_fallback_status"))
        or bool(row_string(row, "fallback_route_status"))
    ]
    route_fallback_success_rows = [
        row
        for row in route_fallback_attempted_rows
        if row_string(row, "route_resolution_status") == "fallback_route_ready"
        or row_string(row, "route_fallback_status") in {"success", "fallback_success"}
        or row_string(row, "fallback_route_status") in {"success", "fallback_success"}
        or (
            row_string(row, "selected_route_kind") in {"legacy_buy", "LegacyBuy"}
            and row_bool_string(row, "fallback_route_ready") == "true"
        )
        or row_string(row, "fallback_route") in {"legacy_buy", "LegacyBuy"}
    ]
    route_fallback_failed_rows = [
        row
        for row in route_fallback_attempted_rows
        if row not in route_fallback_success_rows
    ]
    route_fallback_decision = fallback_decision_payload(route_fallback_failed_rows)
    primary_route_bcv2_missing_rows = [
        row
        for row in skip_rows + transport_rows
        if "primary_route_bcv2_missing"
        in (
            row_string(row, "no_executable_route_account_set_reason")
            or row_string(row, "precheck_failure_reason")
            or row_string(row, "execution_account_readiness_reason")
            or ""
        )
        or row_string(row, "primary_route_not_ready_reason")
        in {
            "bonding_curve_v2_observed_meta_missing_on_rpc",
            "bonding_curve_v2_identity_authoritative_but_not_load_ready",
        }
    ]
    no_executable_route_account_set_rows = [
        row
        for row in skip_rows + transport_rows
        if row_string(row, "probe_skip_reason") == "no_executable_route_account_set"
        or row_string(row, "execution_outcome") == "no_executable_route_account_set"
        or "no_executable_route_account_set"
        in (row_string(row, "precheck_failure_reason") or "")
    ]
    executable_route_ready_rows = [
        row
        for row in skip_rows + transport_rows
        if row_string(row, "route_resolution_status") in {"primary_route_ready", "fallback_route_ready"}
        or bool(row_string(row, "selected_route_kind"))
    ]
    execution_feasibility = execution_feasibility_payload(skip_rows + transport_rows)
    precheck_simulation_account_set_mismatch_rows = [
        row
        for row in transport_rows
        if row_bool_string(row, "account_set_match") == "false"
    ]
    unexplained_account_set_mismatch_rows = [
        row
        for row in precheck_simulation_account_set_mismatch_rows
        if not row_string(row, "account_set_mismatch_reason")
    ]
    simulation_error_entry_rows = [row for row in entry_rows if is_simulation_error_entry(row)]
    lifecycle_eligible_entry_rows = [
        row
        for row in entry_rows
        if row_string(row, "probe_lifecycle_eligibility_status") == "lifecycle_eligible"
    ]
    successful_probe_entry_rows = [
        row
        for row in entry_rows
        if not is_simulation_error_entry(row)
        and (
            row_string(row, "probe_entry_materialization_status") in {None, "entry_materialized"}
            or row_string(row, "probe_lifecycle_eligibility_status") == "lifecycle_eligible"
        )
    ]
    observed_bcv2_rows = [
        row
        for row in transport_rows + skip_rows
        if row_string(row, "bonding_curve_v2_source") == "observed_tx_account_meta"
        or row_string(row, "observed_bcv2_resolved_pubkey")
    ]
    observed_bcv2_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "observed_bcv2_provenance_status") == "route_compatible"
    ]
    observed_bcv2_not_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "observed_bcv2_provenance_status")
        and row_string(row, "observed_bcv2_provenance_status") != "route_compatible"
    ]
    observed_bcv2_missing_provenance_rows = [
        row
        for row in observed_bcv2_rows
        if not row_string(row, "observed_bcv2_provenance_status")
    ]
    observed_bcv2_instruction_position_present_rows = [
        row
        for row in observed_bcv2_rows
        if row.get("observed_bcv2_instruction_account_position") is not None
    ]
    observed_bcv2_message_index_present_rows = [
        row
        for row in observed_bcv2_rows
        if row.get("observed_bcv2_message_account_index") is not None
    ]
    observed_bcv2_authoritative_without_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "bonding_curve_v2_identity_authority_status")
        == "authoritative_observed_tx"
        and row_string(row, "observed_bcv2_provenance_status") != "route_compatible"
    ]
    legacy_buy = legacy_buy_route_payload(skip_rows + transport_rows, successful_probe_entry_rows)
    working_builder = working_builder_parity_payload(skip_rows + transport_rows + entry_rows)
    return {
        "transport_rows": transport_rows_total,
        "probe_selected_rows": len(selection_rows),
        "entry_rows": entry_rows_total,
        "transport_without_entry_rows": max(transport_rows_total - entry_rows_total, 0),
        "status_counts": dict(sorted(status_counts.items())),
        "reason_counts": dict(sorted(reason_counts.items())),
        "buy_variant_counts": dict(sorted(buy_variant_counts.items())),
        "token_param_role_counts": dict(sorted(token_param_role_counts.items())),
        "creator_vault_authority_status_counts": dict(
            sorted(creator_vault_authority_status_counts.items())
        ),
        "creator_vault_mismatch_reason_counts": dict(
            sorted(creator_vault_mismatch_reason_counts.items())
        ),
        "creator_identity_source_counts": dict(sorted(creator_identity_source_counts.items())),
        "bonding_curve_v2_authority_status_counts": dict(
            sorted(bonding_curve_v2_authority_status_counts.items())
        ),
        "bonding_curve_v2_identity_authority_status_counts": dict(
            sorted(bonding_curve_v2_identity_authority_status_counts.items())
        ),
        "bonding_curve_v2_mismatch_reason_counts": dict(
            sorted(bonding_curve_v2_mismatch_reason_counts.items())
        ),
        "bonding_curve_v2_source_counts": dict(sorted(bonding_curve_v2_source_counts.items())),
        "bonding_curve_v2_rpc_load_status_counts": dict(
            sorted(bonding_curve_v2_rpc_load_status_counts.items())
        ),
        "bonding_curve_v2_rpc_load_ready_counts": dict(
            sorted(bonding_curve_v2_rpc_load_ready_counts.items())
        ),
        "bonding_curve_v2_observed_meta_missing_on_rpc_rows": (
            bonding_curve_v2_mismatch_reason_counts.get(
                "bonding_curve_v2_observed_meta_missing_on_rpc",
                0,
            )
        ),
        "bonding_curve_v2_identity_authoritative_but_not_load_ready_rows": (
            bonding_curve_v2_mismatch_reason_counts.get(
                "bonding_curve_v2_identity_authoritative_but_not_load_ready",
                0,
            )
        ),
        "builder_bcv2_authoritative_observed_tx_rows": (
            bonding_curve_v2_authority_status_counts.get("authoritative_observed_tx", 0)
            + skip_bonding_curve_v2_authority_status_counts.get(
                "authoritative_observed_tx",
                0,
            )
        ),
        "builder_bcv2_authoritative_mfs_rows": (
            bonding_curve_v2_authority_status_counts.get("authoritative_mfs", 0)
            + skip_bonding_curve_v2_authority_status_counts.get("authoritative_mfs", 0)
        ),
        "builder_bcv2_derived_unverified_rows": (
            bonding_curve_v2_authority_status_counts.get("derived_unverified", 0)
            + bonding_curve_v2_authority_status_counts.get("builder_only", 0)
            + skip_bonding_curve_v2_authority_status_counts.get("derived_unverified", 0)
            + skip_bonding_curve_v2_authority_status_counts.get("builder_only", 0)
        ),
        "builder_required_curve_account_ready_counts": dict(
            sorted(builder_required_curve_account_ready_counts.items())
        ),
        "builder_required_curve_account_ready_reason_counts": dict(
            sorted(builder_required_curve_account_ready_reason_counts.items())
        ),
        "observed_bcv2_provenance_status_counts": dict(
            sorted(observed_bcv2_provenance_status_counts.items())
        ),
        "route_resolution_status_counts": dict(sorted(route_resolution_status_counts.items())),
        "selected_route_kind_counts": dict(sorted(selected_route_kind_counts.items())),
        "observed_bcv2_rows": len(observed_bcv2_rows),
        "observed_bcv2_route_compatible_rows": len(observed_bcv2_route_compatible_rows),
        "observed_bcv2_not_route_compatible_rows": len(
            observed_bcv2_not_route_compatible_rows
        ),
        "observed_bcv2_missing_provenance_rows": len(observed_bcv2_missing_provenance_rows),
        "observed_bcv2_instruction_account_position_present_rows": len(
            observed_bcv2_instruction_position_present_rows
        ),
        "observed_bcv2_message_account_index_present_rows": len(
            observed_bcv2_message_index_present_rows
        ),
        "observed_bcv2_authoritative_without_route_compatible_rows": len(
            observed_bcv2_authoritative_without_route_compatible_rows
        ),
        "route_fallback_status_counts": dict(sorted(route_fallback_status_counts.items())),
        **legacy_buy,
        "amount_guard_status_counts": dict(sorted(amount_guard_status_counts.items())),
        "simulation_error_category_counts": dict(sorted(simulation_error_category_counts.items())),
        "simulation_error_kind_counts": dict(sorted(simulation_error_kind_counts.items())),
        "simulation_error_account_role_counts": dict(
            sorted(simulation_error_account_role_counts.items())
        ),
        "simulation_error_account_source_counts": dict(
            sorted(simulation_error_account_source_counts.items())
        ),
        "simulation_error_account_narrowing_status_counts": dict(
            sorted(simulation_error_account_narrowing_status_counts.items())
        ),
        "account_not_found_candidate_raw_counts": dict(
            sorted(account_not_found_candidate_raw_counts.items())
        ),
        "account_not_found_candidate_narrowed_counts": dict(
            sorted(account_not_found_candidate_narrowed_counts.items())
        ),
        "candidate_class_counts": dict(sorted(candidate_class_counts.items())),
        "candidate_exclusion_reason_counts": dict(
            sorted(candidate_exclusion_reason_counts.items())
        ),
        "simulation_error_custom_code_counts": dict(
            sorted(simulation_error_custom_code_counts.items())
        ),
        "account_set_match_counts": dict(sorted(account_set_match_counts.items())),
        "account_set_mismatch_reason_counts": dict(
            sorted(account_set_mismatch_reason_counts.items())
        ),
        "skip_reason_counts": dict(sorted(skip_reason_counts.items())),
        "skip_execution_account_readiness_role_counts": dict(
            sorted(skip_execution_account_readiness_role_counts.items())
        ),
        "skip_execution_account_readiness_reason_counts": dict(
            sorted(skip_execution_account_readiness_reason_counts.items())
        ),
        "skip_creator_vault_authority_status_counts": dict(
            sorted(skip_creator_vault_authority_status_counts.items())
        ),
        "skip_creator_vault_mismatch_reason_counts": dict(
            sorted(skip_creator_vault_mismatch_reason_counts.items())
        ),
        "skip_creator_identity_source_counts": dict(
            sorted(skip_creator_identity_source_counts.items())
        ),
        "skip_bonding_curve_v2_authority_status_counts": dict(
            sorted(skip_bonding_curve_v2_authority_status_counts.items())
        ),
        "skip_bonding_curve_v2_mismatch_reason_counts": dict(
            sorted(skip_bonding_curve_v2_mismatch_reason_counts.items())
        ),
        "skip_bonding_curve_v2_source_counts": dict(
            sorted(skip_bonding_curve_v2_source_counts.items())
        ),
        "skip_route_fallback_status_counts": dict(
            sorted(skip_route_fallback_status_counts.items())
        ),
        "entry_materialized_rows": status_counts.get("entry_materialized", 0),
        "transport_only_missing_token_quantity_rows": status_counts.get(
            "transport_only_missing_token_quantity",
            0,
        ),
        "simulation_error_rows": status_counts.get("simulation_error", 0),
        "execution_account_not_ready_rows": status_counts.get("execution_account_not_ready", 0),
        "unknown_rows": status_counts.get("unknown", 0),
        "account_not_found_rows": len(account_not_found_rows),
        "account_not_found_attributed_rows": len(account_not_found_attributed_rows),
        "account_not_found_multi_candidate_rows": len(account_not_found_multi_candidate_rows),
        "account_not_found_unattributed_rows": len(account_not_found_unattributed_rows),
        "exact_after_narrowing_rows": len(exact_after_narrowing_rows),
        "multi_candidate_narrowed_rows": len(multi_candidate_narrowed_rows),
        "unattributed_after_narrowing_rows": len(unattributed_after_narrowing_rows),
        "all_candidates_nonfatal_but_sim_failed_rows": len(
            all_candidates_nonfatal_but_sim_failed_rows
        ),
        "simulation_rpc_visibility_gap_rows": len(simulation_rpc_visibility_gap_rows),
        "simulation_required_account_not_in_precheck_rows": len(
            simulation_required_account_not_in_precheck_rows
        ),
        "simulation_account_meta_missing_on_rpc_rows": len(
            simulation_account_meta_missing_on_rpc_rows
        ),
        "account_not_found_after_simulation_rows": len(account_not_found_rows),
        "route_excluded_bcv2_missing_rows": len(route_excluded_bcv2_missing_rows),
        "route_fallback_attempted_rows": len(route_fallback_attempted_rows),
        "route_fallback_success_rows": len(route_fallback_success_rows),
        "route_fallback_failed_rows": len(route_fallback_failed_rows),
        **working_builder,
        "fallback_failure_class_counts": route_fallback_decision[
            "fallback_failure_class_counts"
        ],
        "fallback_missing_role_counts": route_fallback_decision["fallback_missing_role_counts"],
        "fallback_missing_pubkey_counts": route_fallback_decision[
            "fallback_missing_pubkey_counts"
        ],
        "fallback_account_source_counts": route_fallback_decision[
            "fallback_account_source_counts"
        ],
        "fallback_simulation_load_account_set_rows": route_fallback_decision[
            "fallback_simulation_load_account_set_rows"
        ],
        "fallback_creatable_account_set_rows": route_fallback_decision[
            "fallback_creatable_account_set_rows"
        ],
        "fallback_required_precheck_account_set_rows": route_fallback_decision[
            "fallback_required_precheck_account_set_rows"
        ],
        "fallback_repairable": route_fallback_decision["fallback_repairable"],
        "recommended_next_path": route_fallback_decision["recommended_next_path"],
        "executable_route_ready_rows": len(executable_route_ready_rows),
        "route_executable_rows": execution_feasibility["route_executable_rows"],
        "route_non_executable_rows": execution_feasibility["route_non_executable_rows"],
        "execution_feasibility_reject_rows": execution_feasibility[
            "execution_feasibility_reject_rows"
        ],
        "execution_feasibility_status_counts": execution_feasibility[
            "execution_feasibility_status_counts"
        ],
        "execution_feasibility_reason_counts": execution_feasibility[
            "execution_feasibility_reason_counts"
        ],
        "lifecycle_label_eligibility_counts": execution_feasibility[
            "lifecycle_label_eligibility_counts"
        ],
        "primary_route_bcv2_missing_rows": len(primary_route_bcv2_missing_rows),
        "no_executable_route_account_set_rows": len(no_executable_route_account_set_rows),
        "bonding_curve_v2_precheck_skipped_before_simulation_rows": len(
            bonding_curve_v2_precheck_skipped_before_simulation_rows
        ),
        "bonding_curve_v2_account_not_found_after_simulation_rows": len(
            bonding_curve_v2_account_not_found_after_simulation_rows
        ),
        "precheck_simulation_account_set_mismatch_rows": len(
            precheck_simulation_account_set_mismatch_rows
        ),
        "unexplained_account_set_mismatch_rows": len(unexplained_account_set_mismatch_rows),
        "successful_probe_entry_rows": len(successful_probe_entry_rows),
        "simulation_error_entry_rows": len(simulation_error_entry_rows),
        "lifecycle_eligible_entry_rows": len(lifecycle_eligible_entry_rows),
        "lifecycle_labeled_rows": len(lifecycle_rows),
        "rows": rows,
    }


def active_shadow_dispatch_diagnostics(paths: dict[str, list[Path]]) -> dict[str, Any]:
    transport_rows = artifact_rows(paths, "shadow_transport")
    entry_rows = artifact_rows(paths, "shadow_entry")
    lifecycle_rows = artifact_rows(paths, "shadow_lifecycle")
    all_rows = transport_rows + entry_rows + lifecycle_rows

    def is_failure(row: dict[str, Any]) -> bool:
        status = row_string(row, "dispatch_status")
        outcome = row_string(row, "simulation_outcome") or row_string(row, "execution_outcome")
        failure_outcomes = {
            "failed",
            "shadow_data_problem",
            "shadow_simulation_failed",
            "shadow_simulation_error",
        }
        return (
            bool(row_string(row, "err"))
            or status == "failed"
            or outcome in failure_outcomes
            or bool(row_string(row, "simulation_error_kind"))
            or bool(row_string(row, "simulation_error_category"))
        )

    failure_rows = [row for row in all_rows if is_failure(row)]
    account_not_found_rows = [row for row in failure_rows if is_account_not_found_row(row)]
    attributed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_account_not_found_attributed"
        or (
            row_string(row, "simulation_error_account_pubkey")
            and row_string(row, "simulation_error_account_role")
        )
    ]
    multi_candidate_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category")
        in {
            "simulation_account_not_found_multi_candidate",
            "simulation_account_not_found_multi_candidate_narrow",
        }
        or row_string(row, "simulation_error_account_narrowing_status")
        == "multi_candidate_narrowed"
    ]
    rpc_visibility_gap_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_rpc_visibility_gap"
    ]
    unattributed_rows = [
        row
        for row in account_not_found_rows
        if row_string(row, "simulation_error_category") == "simulation_account_not_found_unattributed"
        or row_string(row, "simulation_error_account_narrowing_status")
        == "unattributed_after_narrowing"
        or (
            not row_string(row, "simulation_error_account_pubkey")
            and not list(iter_account_candidates(row, "simulation_error_account_candidates"))
            and not list(iter_account_candidates(row, "simulation_error_account_candidates_narrowed"))
            and row_string(row, "simulation_error_category") != "simulation_rpc_visibility_gap"
        )
    ]
    lifecycle_eligible_failure_rows = [
        row
        for row in failure_rows
        if row_string(row, "active_shadow_lifecycle_eligibility_status") == "lifecycle_eligible"
    ]
    precheck_failed_rows = [
        row
        for row in failure_rows
        if row_string(row, "active_shadow_precheck_status") == "precheck_failed"
        or bool(row_string(row, "precheck_failure_reason"))
    ]
    precheck_failed_row_ids = {id(row) for row in precheck_failed_rows}
    runtime_simulation_error_rows = [
        row
        for row in failure_rows
        if id(row) not in precheck_failed_row_ids
        and (
            bool(row_string(row, "simulation_error_kind"))
            or bool(row_string(row, "simulation_error_category"))
            or bool(row_string(row, "err"))
        )
    ]
    simulation_required_account_not_in_precheck_rows = [
        row
        for row in all_rows
        if row_string(row, "account_set_mismatch_reason")
        == "simulation_required_accounts_missing_from_precheck"
    ]
    bonding_curve_v2_precheck_skipped_rows = [
        row
        for row in precheck_failed_rows
        if row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
        or (
            row_string(row, "precheck_failure_reason") or ""
        ).startswith("execution_account_not_ready:bonding_curve_v2:")
    ]
    bonding_curve_v2_account_not_found_after_simulation_rows = [
        row
        for row in account_not_found_rows
        if id(row) not in precheck_failed_row_ids
        and row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
    ]
    successful_entry_rows = [
        row
        for row in entry_rows
        if not is_failure(row)
    ]
    lifecycle_eligible_rows = [
        row
        for row in entry_rows + lifecycle_rows
        if row_string(row, "active_shadow_lifecycle_eligibility_status") == "lifecycle_eligible"
    ]
    role_counts: Counter[str] = Counter()
    category_counts: Counter[str] = Counter()
    precheck_status_counts: Counter[str] = Counter()
    lifecycle_eligibility_counts: Counter[str] = Counter()
    account_set_match_counts: Counter[str] = Counter()
    narrowing_status_counts: Counter[str] = Counter()
    candidate_raw_counts: Counter[str] = Counter()
    candidate_narrowed_counts: Counter[str] = Counter()
    bonding_curve_v2_authority_status_counts: Counter[str] = Counter()
    bonding_curve_v2_identity_authority_status_counts: Counter[str] = Counter()
    bonding_curve_v2_mismatch_reason_counts: Counter[str] = Counter()
    bonding_curve_v2_source_counts: Counter[str] = Counter()
    bonding_curve_v2_rpc_load_status_counts: Counter[str] = Counter()
    bonding_curve_v2_rpc_load_ready_counts: Counter[str] = Counter()
    builder_required_curve_account_ready_counts: Counter[str] = Counter()
    builder_required_curve_account_ready_reason_counts: Counter[str] = Counter()
    observed_bcv2_provenance_status_counts: Counter[str] = Counter()
    route_resolution_status_counts: Counter[str] = Counter()
    selected_route_kind_counts: Counter[str] = Counter()
    route_fallback_status_counts: Counter[str] = Counter()
    for row in failure_rows:
        if role := row_string(row, "simulation_error_account_role"):
            role_counts[role] += 1
        if category := row_string(row, "simulation_error_category"):
            category_counts[category] += 1
        if status := row_string(row, "active_shadow_precheck_status"):
            precheck_status_counts[status] += 1
        if status := row_string(row, "active_shadow_lifecycle_eligibility_status"):
            lifecycle_eligibility_counts[status] += 1
        if match_value := row_bool_string(row, "account_set_match"):
            account_set_match_counts[match_value] += 1
        if narrowing := row_string(row, "simulation_error_account_narrowing_status"):
            narrowing_status_counts[narrowing] += 1
        if status := row_string(row, "bonding_curve_v2_authority_status"):
            bonding_curve_v2_authority_status_counts[status] += 1
        if status := row_string(row, "bonding_curve_v2_identity_authority_status"):
            bonding_curve_v2_identity_authority_status_counts[status] += 1
        if reason := row_string(row, "bonding_curve_v2_mismatch_reason"):
            bonding_curve_v2_mismatch_reason_counts[reason] += 1
        if source := row_string(row, "bonding_curve_v2_source"):
            bonding_curve_v2_source_counts[source] += 1
        if status := row_string(row, "bonding_curve_v2_rpc_load_status"):
            bonding_curve_v2_rpc_load_status_counts[status] += 1
        if ready := row_bool_string(row, "bonding_curve_v2_rpc_load_ready"):
            bonding_curve_v2_rpc_load_ready_counts[ready] += 1
        if ready := row_bool_string(row, "builder_required_curve_account_ready"):
            builder_required_curve_account_ready_counts[ready] += 1
        if reason := row_string(row, "builder_required_curve_account_ready_reason"):
            builder_required_curve_account_ready_reason_counts[reason] += 1
        if status := row_string(row, "observed_bcv2_provenance_status"):
            observed_bcv2_provenance_status_counts[status] += 1
        if status := row_string(row, "route_resolution_status"):
            route_resolution_status_counts[status] += 1
        if route_kind := row_string(row, "selected_route_kind"):
            selected_route_kind_counts[route_kind] += 1
        route_fallback_status = (
            row_string(row, "route_fallback_status")
            or row_string(row, "fallback_route_status")
            or row_bool_string(row, "fallback_route_attempted")
            or row_bool_string(row, "route_fallback_attempted")
        )
        if route_fallback_status:
            route_fallback_status_counts[route_fallback_status] += 1
        for candidate in iter_account_candidates(row, "simulation_error_account_candidates_raw"):
            role = candidate.get("role")
            if role:
                candidate_raw_counts[str(role)] += 1
        for candidate in iter_account_candidates(row, "simulation_error_account_candidates_narrowed"):
            role = candidate.get("role")
            if role:
                candidate_narrowed_counts[str(role)] += 1

    route_excluded_bcv2_missing_rows = [
        row
        for row in failure_rows
        if row_string(row, "active_shadow_precheck_status") == "precheck_failed"
        and (
            row_string(row, "simulation_error_account_role") == "bonding_curve_v2"
            or row_string(row, "bonding_curve_v2_authority_status")
            in {"builder_only", "derived_unverified"}
            or "bonding_curve_v2_source_not_authoritative"
            in (row_string(row, "precheck_failure_reason") or "")
            or "no_executable_route_account_set"
            in (row_string(row, "precheck_failure_reason") or "")
        )
    ]
    route_fallback_attempted_rows = [
        row
        for row in failure_rows
        if row_bool_string(row, "fallback_route_attempted") == "true"
        or row_bool_string(row, "route_fallback_attempted") == "true"
        or bool(row_string(row, "route_fallback_status"))
        or bool(row_string(row, "fallback_route_status"))
    ]
    route_fallback_success_rows = [
        row
        for row in route_fallback_attempted_rows
        if row_string(row, "route_resolution_status") == "fallback_route_ready"
        or row_string(row, "route_fallback_status") in {"success", "fallback_success"}
        or row_string(row, "fallback_route_status") in {"success", "fallback_success"}
        or (
            row_string(row, "selected_route_kind") in {"legacy_buy", "LegacyBuy"}
            and row_bool_string(row, "fallback_route_ready") == "true"
        )
        or row_string(row, "fallback_route") in {"legacy_buy", "LegacyBuy"}
    ]
    route_fallback_failed_rows = [
        row
        for row in route_fallback_attempted_rows
        if row not in route_fallback_success_rows
    ]
    route_fallback_decision = fallback_decision_payload(route_fallback_failed_rows)
    primary_route_bcv2_missing_rows = [
        row
        for row in failure_rows
        if "primary_route_bcv2_missing"
        in (
            row_string(row, "no_executable_route_account_set_reason")
            or row_string(row, "precheck_failure_reason")
            or ""
        )
        or row_string(row, "primary_route_not_ready_reason")
        in {
            "bonding_curve_v2_observed_meta_missing_on_rpc",
            "bonding_curve_v2_identity_authoritative_but_not_load_ready",
        }
    ]
    no_executable_route_account_set_rows = [
        row
        for row in failure_rows
        if row_string(row, "execution_outcome") == "no_executable_route_account_set"
        or "no_executable_route_account_set"
        in (row_string(row, "precheck_failure_reason") or "")
    ]
    executable_route_ready_rows = [
        row
        for row in failure_rows
        if row_string(row, "route_resolution_status") in {"primary_route_ready", "fallback_route_ready"}
        or bool(row_string(row, "selected_route_kind"))
    ]
    execution_feasibility = execution_feasibility_payload(failure_rows)
    observed_bcv2_rows = [
        row
        for row in failure_rows
        if row_string(row, "bonding_curve_v2_source") == "observed_tx_account_meta"
        or row_string(row, "observed_bcv2_resolved_pubkey")
    ]
    observed_bcv2_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "observed_bcv2_provenance_status") == "route_compatible"
    ]
    observed_bcv2_not_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "observed_bcv2_provenance_status")
        and row_string(row, "observed_bcv2_provenance_status") != "route_compatible"
    ]
    observed_bcv2_missing_provenance_rows = [
        row
        for row in observed_bcv2_rows
        if not row_string(row, "observed_bcv2_provenance_status")
    ]
    observed_bcv2_authoritative_without_route_compatible_rows = [
        row
        for row in observed_bcv2_rows
        if row_string(row, "bonding_curve_v2_identity_authority_status")
        == "authoritative_observed_tx"
        and row_string(row, "observed_bcv2_provenance_status") != "route_compatible"
    ]
    legacy_buy = {
        f"active_shadow_{key}": value
        for key, value in legacy_buy_route_payload(
            failure_rows,
            successful_entry_rows,
        ).items()
    }
    working_builder = working_builder_parity_payload(
        failure_rows + successful_entry_rows,
        "active_shadow_",
    )

    return {
        "active_shadow_transport_rows": len(transport_rows),
        "active_shadow_entry_rows": len(entry_rows),
        "active_shadow_lifecycle_rows": len(lifecycle_rows),
        "active_shadow_dispatch_failure_rows": len(failure_rows),
        "active_shadow_precheck_failed_rows": len(precheck_failed_rows),
        "active_shadow_runtime_simulation_error_rows": len(runtime_simulation_error_rows),
        "active_shadow_successful_entry_rows": len(successful_entry_rows),
        "active_shadow_lifecycle_eligible_rows": len(lifecycle_eligible_rows),
        "active_shadow_lifecycle_eligible_failure_rows": len(
            lifecycle_eligible_failure_rows
        ),
        "active_shadow_simulation_required_account_not_in_precheck_count": len(
            simulation_required_account_not_in_precheck_rows
        ),
        "active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows": len(
            bonding_curve_v2_precheck_skipped_rows
        ),
        "active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows": len(
            bonding_curve_v2_account_not_found_after_simulation_rows
        ),
        "active_shadow_account_not_found_rows": len(account_not_found_rows),
        "active_shadow_account_not_found_attributed_rows": len(attributed_rows),
        "active_shadow_account_not_found_multi_candidate_rows": len(multi_candidate_rows),
        "active_shadow_account_not_found_unattributed_rows": len(unattributed_rows),
        "active_shadow_rpc_visibility_gap_rows": len(rpc_visibility_gap_rows),
        "active_shadow_account_not_found_role_counts": dict(sorted(role_counts.items())),
        "active_shadow_simulation_error_category_counts": dict(sorted(category_counts.items())),
        "active_shadow_precheck_status_counts": dict(sorted(precheck_status_counts.items())),
        "active_shadow_lifecycle_eligibility_status_counts": dict(
            sorted(lifecycle_eligibility_counts.items())
        ),
        "active_shadow_account_set_match_counts": dict(sorted(account_set_match_counts.items())),
        "active_shadow_account_narrowing_status_counts": dict(
            sorted(narrowing_status_counts.items())
        ),
        "active_shadow_account_candidate_raw_counts": dict(sorted(candidate_raw_counts.items())),
        "active_shadow_account_candidate_narrowed_counts": dict(
            sorted(candidate_narrowed_counts.items())
        ),
        "active_shadow_bonding_curve_v2_authority_status_counts": dict(
            sorted(bonding_curve_v2_authority_status_counts.items())
        ),
        "active_shadow_bonding_curve_v2_identity_authority_status_counts": dict(
            sorted(bonding_curve_v2_identity_authority_status_counts.items())
        ),
        "active_shadow_bonding_curve_v2_mismatch_reason_counts": dict(
            sorted(bonding_curve_v2_mismatch_reason_counts.items())
        ),
        "active_shadow_bonding_curve_v2_source_counts": dict(
            sorted(bonding_curve_v2_source_counts.items())
        ),
        "active_shadow_bonding_curve_v2_rpc_load_status_counts": dict(
            sorted(bonding_curve_v2_rpc_load_status_counts.items())
        ),
        "active_shadow_bonding_curve_v2_rpc_load_ready_counts": dict(
            sorted(bonding_curve_v2_rpc_load_ready_counts.items())
        ),
        "active_shadow_bonding_curve_v2_observed_meta_missing_on_rpc_rows": (
            bonding_curve_v2_mismatch_reason_counts.get(
                "bonding_curve_v2_observed_meta_missing_on_rpc",
                0,
            )
        ),
        "active_shadow_bonding_curve_v2_identity_authoritative_but_not_load_ready_rows": (
            bonding_curve_v2_mismatch_reason_counts.get(
                "bonding_curve_v2_identity_authoritative_but_not_load_ready",
                0,
            )
        ),
        "active_shadow_builder_bcv2_authoritative_observed_tx_rows": (
            bonding_curve_v2_authority_status_counts.get("authoritative_observed_tx", 0)
        ),
        "active_shadow_builder_bcv2_authoritative_mfs_rows": (
            bonding_curve_v2_authority_status_counts.get("authoritative_mfs", 0)
        ),
        "active_shadow_builder_bcv2_derived_unverified_rows": (
            bonding_curve_v2_authority_status_counts.get("derived_unverified", 0)
            + bonding_curve_v2_authority_status_counts.get("builder_only", 0)
        ),
        "active_shadow_builder_required_curve_account_ready_counts": dict(
            sorted(builder_required_curve_account_ready_counts.items())
        ),
        "active_shadow_builder_required_curve_account_ready_reason_counts": dict(
            sorted(builder_required_curve_account_ready_reason_counts.items())
        ),
        "active_shadow_observed_bcv2_provenance_status_counts": dict(
            sorted(observed_bcv2_provenance_status_counts.items())
        ),
        "active_shadow_route_resolution_status_counts": dict(
            sorted(route_resolution_status_counts.items())
        ),
        "active_shadow_selected_route_kind_counts": dict(sorted(selected_route_kind_counts.items())),
        "active_shadow_observed_bcv2_rows": len(observed_bcv2_rows),
        "active_shadow_observed_bcv2_route_compatible_rows": len(
            observed_bcv2_route_compatible_rows
        ),
        "active_shadow_observed_bcv2_not_route_compatible_rows": len(
            observed_bcv2_not_route_compatible_rows
        ),
        "active_shadow_observed_bcv2_missing_provenance_rows": len(
            observed_bcv2_missing_provenance_rows
        ),
        "active_shadow_observed_bcv2_authoritative_without_route_compatible_rows": len(
            observed_bcv2_authoritative_without_route_compatible_rows
        ),
        "active_shadow_route_fallback_status_counts": dict(
            sorted(route_fallback_status_counts.items())
        ),
        **legacy_buy,
        "active_shadow_route_excluded_bcv2_missing_rows": len(
            route_excluded_bcv2_missing_rows
        ),
        "active_shadow_route_fallback_attempted_rows": len(route_fallback_attempted_rows),
        "active_shadow_route_fallback_success_rows": len(route_fallback_success_rows),
        "active_shadow_route_fallback_failed_rows": len(route_fallback_failed_rows),
        **working_builder,
        "active_shadow_fallback_failure_class_counts": route_fallback_decision[
            "fallback_failure_class_counts"
        ],
        "active_shadow_fallback_missing_role_counts": route_fallback_decision[
            "fallback_missing_role_counts"
        ],
        "active_shadow_fallback_missing_pubkey_counts": route_fallback_decision[
            "fallback_missing_pubkey_counts"
        ],
        "active_shadow_fallback_account_source_counts": route_fallback_decision[
            "fallback_account_source_counts"
        ],
        "active_shadow_fallback_simulation_load_account_set_rows": route_fallback_decision[
            "fallback_simulation_load_account_set_rows"
        ],
        "active_shadow_fallback_creatable_account_set_rows": route_fallback_decision[
            "fallback_creatable_account_set_rows"
        ],
        "active_shadow_fallback_required_precheck_account_set_rows": route_fallback_decision[
            "fallback_required_precheck_account_set_rows"
        ],
        "active_shadow_fallback_repairable": route_fallback_decision["fallback_repairable"],
        "active_shadow_recommended_next_path": route_fallback_decision[
            "recommended_next_path"
        ],
        "active_shadow_executable_route_ready_rows": len(executable_route_ready_rows),
        "active_shadow_route_executable_rows": execution_feasibility["route_executable_rows"],
        "active_shadow_route_non_executable_rows": execution_feasibility[
            "route_non_executable_rows"
        ],
        "active_shadow_execution_feasibility_reject_rows": execution_feasibility[
            "execution_feasibility_reject_rows"
        ],
        "active_buy_execution_infeasible_rows": execution_feasibility[
            "execution_feasibility_reject_rows"
        ],
        "active_shadow_execution_feasibility_status_counts": execution_feasibility[
            "execution_feasibility_status_counts"
        ],
        "active_shadow_execution_feasibility_reason_counts": execution_feasibility[
            "execution_feasibility_reason_counts"
        ],
        "active_shadow_lifecycle_label_eligibility_counts": execution_feasibility[
            "lifecycle_label_eligibility_counts"
        ],
        "active_shadow_primary_route_bcv2_missing_rows": len(primary_route_bcv2_missing_rows),
        "active_shadow_no_executable_route_account_set_rows": len(
            no_executable_route_account_set_rows
        ),
        "active_shadow_account_not_found_after_simulation_rows": len(account_not_found_rows),
    }


def probe_decision_join(paths: dict[str, list[Path]]) -> dict[str, Any]:
    decisions = artifact_rows(paths, "decision")
    decision_by_ab: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for decision in decisions:
        ab_record_id = row_ab_record_id(decision)
        if ab_record_id:
            decision_by_ab[ab_record_id].append(decision)

    required_artifacts = ("probe_selection", "probe_transport", "probe_entry")
    artifact_reports: dict[str, dict[str, Any]] = {}
    for artifact_type in (*required_artifacts, "probe_lifecycle"):
        rows = artifact_rows(paths, artifact_type)
        rows_with_ab = 0
        joined_to_decision = 0
        joined_to_decision_with_v3_payload = 0
        feature_hash_match = 0
        policy_hash_match = 0
        exact_decision_v3_join = 0
        unmatched_rows = 0
        feature_hash_mismatch = 0
        policy_hash_mismatch = 0
        mismatch_reasons: Counter[str] = Counter()
        for row in rows:
            ab_record_id = row_ab_record_id(row)
            if not ab_record_id:
                unmatched_rows += 1
                mismatch_reasons["missing_ab_record_id"] += 1
                continue
            rows_with_ab += 1
            matching_decisions = decision_by_ab.get(ab_record_id, [])
            if not matching_decisions:
                unmatched_rows += 1
                mismatch_reasons["decision_row_not_found"] += 1
                continue
            if len(matching_decisions) > 1:
                mismatch_reasons["multiple_decision_rows_for_ab_record_id"] += 1
            joined_to_decision += 1
            if any(row_has_v3_payload(decision) for decision in matching_decisions):
                joined_to_decision_with_v3_payload += 1
            else:
                mismatch_reasons["decision_row_missing_v3_payload"] += 1

            feature_hash = row_feature_hash(row)
            policy_hash = row_policy_hash(row)
            source_plane = row_decision_plane(row)
            feature_match = feature_hash is not None and any(
                row_feature_hash(decision) == feature_hash for decision in matching_decisions
            )
            policy_match = policy_hash is not None and any(
                row_policy_hash(decision) == policy_hash for decision in matching_decisions
            )
            source_plane_match = source_plane is None or any(
                row_decision_plane(decision) == source_plane for decision in matching_decisions
            )
            if feature_match:
                feature_hash_match += 1
            elif feature_hash is None:
                mismatch_reasons["feature_hash_missing"] += 1
            else:
                feature_hash_mismatch += 1
                mismatch_reasons["feature_hash_mismatch"] += 1
            if policy_match:
                policy_hash_match += 1
            elif policy_hash is None:
                mismatch_reasons["policy_hash_missing"] += 1
            else:
                policy_hash_mismatch += 1
                mismatch_reasons["policy_hash_mismatch"] += 1
            if not source_plane_match:
                mismatch_reasons["source_plane_mismatch"] += 1
            if (
                feature_match
                and policy_match
                and source_plane_match
                and any(row_has_v3_payload(decision) for decision in matching_decisions)
            ):
                exact_decision_v3_join += 1

        artifact_reports[artifact_type] = {
            "rows": len(rows),
            "rows_with_ab_record_id": rows_with_ab,
            "joined_to_decision_by_ab_record_id": joined_to_decision,
            "joined_to_decision_with_v3_payload": joined_to_decision_with_v3_payload,
            "feature_hash_match": feature_hash_match,
            "policy_hash_match": policy_hash_match,
            "exact_decision_v3_join": exact_decision_v3_join,
            "unmatched_rows": unmatched_rows,
            "feature_hash_mismatch": feature_hash_mismatch,
            "policy_hash_mismatch": policy_hash_mismatch,
            "mismatch_reasons": dict(sorted(mismatch_reasons.items())),
            "exact_decision_v3_join_coverage": round(exact_decision_v3_join / len(rows), 6)
            if rows
            else 0.0,
        }

    required_coverages = [
        artifact_reports[artifact_type]["exact_decision_v3_join_coverage"]
        for artifact_type in required_artifacts
        if artifact_reports[artifact_type]["rows"] > 0
    ]
    required_rows_present = all(
        artifact_reports[artifact_type]["rows"] > 0 for artifact_type in required_artifacts
    )
    required_exact_coverage = min(required_coverages) if required_coverages else 0.0
    acceptance = "pass" if required_rows_present and required_exact_coverage >= 1.0 else "fail"
    return {
        "decision_rows": len(decisions),
        "decision_rows_with_ab_record_id": sum(1 for row in decisions if row_ab_record_id(row)),
        "decision_rows_with_v3_payload": sum(1 for row in decisions if row_has_v3_payload(row)),
        "required_probe_artifacts": list(required_artifacts),
        "required_probe_artifacts_present": required_rows_present,
        "required_exact_decision_v3_join_coverage": round(required_exact_coverage, 6),
        "decision_join_acceptance": acceptance,
        "artifacts": artifact_reports,
    }


def readiness(report: dict[str, Any]) -> dict[str, Any]:
    decision_rows = artifact_totals(report, "decision")
    v3_payload_rows = artifact_totals(report, "decision", "v3_replay_payload")
    shadow_entry_rows = artifact_totals(report, "shadow_entry")
    lifecycle_rows = artifact_totals(report, "shadow_lifecycle")
    transport_rows = artifact_totals(report, "shadow_transport")
    candidate_common = report["cross_artifact_intersections"].get("candidate_id", {}).get("common_values", 0)
    exact_ab_common = report["cross_artifact_intersections"].get("ab_record_id", {}).get("common_values", 0)
    quality = report.get("join_key_coverage", {}).get("join_quality") or join_quality(report)
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
    active_shadow = report.get("active_shadow_dispatch_diagnostics", {})
    if active_shadow.get("active_shadow_account_not_found_unattributed_rows", 0) > 0:
        status = "not_ready"
        reasons.append("active_shadow_unattributed_account_not_found")
    if active_shadow.get("active_shadow_lifecycle_eligible_failure_rows", 0) > 0:
        status = "not_ready"
        reasons.append("active_shadow_dispatch_failure_marked_lifecycle_eligible")
    if (
        active_shadow.get(
            "active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows", 0
        )
        > 0
    ):
        status = "not_ready"
        reasons.append("active_shadow_bonding_curve_v2_account_not_found_after_simulation")
    if active_shadow.get("active_shadow_bonding_curve_v2_authority_status_counts", {}).get(
        "builder_only",
        0,
    ) > 0:
        status = "not_ready"
        reasons.append("active_shadow_bonding_curve_v2_source_not_authoritative")
    if (
        active_shadow.get(
            "active_shadow_observed_bcv2_authoritative_without_route_compatible_rows",
            0,
        )
        > 0
    ):
        status = "not_ready"
        reasons.append("active_shadow_observed_bcv2_authoritative_without_route_compatible")
    if exact_ab_common <= 0:
        status = "degraded" if status != "not_ready" else status
        reasons.append("no_common_ab_record_id_across_nonempty_artifacts")
    if candidate_common <= 0 and exact_ab_common <= 0:
        status = "degraded" if status != "not_ready" else status
        reasons.append("no_common_candidate_id_across_nonempty_artifacts")
    if status == "ready_for_lifecycle_feature_join" and quality == "exact_ab_record_id":
        join_key_acceptance = "pass"
    elif status == "not_ready":
        join_key_acceptance = "fail"
    else:
        join_key_acceptance = "degraded"
    return {
        "status": status,
        "reasons": reasons,
        "join_key_acceptance": join_key_acceptance,
        "join_quality": quality,
        "decision_rows": decision_rows,
        "v3_payload_rows": v3_payload_rows,
        "shadow_transport_rows": transport_rows,
        "shadow_entry_rows": shadow_entry_rows,
        "shadow_lifecycle_rows": lifecycle_rows,
        "active_shadow_account_not_found_rows": active_shadow.get(
            "active_shadow_account_not_found_rows",
            0,
        ),
        "active_shadow_account_not_found_attributed_rows": active_shadow.get(
            "active_shadow_account_not_found_attributed_rows",
            0,
        ),
        "active_shadow_account_not_found_unattributed_rows": active_shadow.get(
            "active_shadow_account_not_found_unattributed_rows",
            0,
        ),
        "active_shadow_dispatch_failure_rows": active_shadow.get(
            "active_shadow_dispatch_failure_rows",
            0,
        ),
        "active_shadow_precheck_failed_rows": active_shadow.get(
            "active_shadow_precheck_failed_rows",
            0,
        ),
        "active_shadow_runtime_simulation_error_rows": active_shadow.get(
            "active_shadow_runtime_simulation_error_rows",
            0,
        ),
        "active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows": active_shadow.get(
            "active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows",
            0,
        ),
        "active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows": active_shadow.get(
            "active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows",
            0,
        ),
        "active_shadow_bonding_curve_v2_authority_status_counts": active_shadow.get(
            "active_shadow_bonding_curve_v2_authority_status_counts",
            {},
        ),
        "active_shadow_lifecycle_eligible_failure_rows": active_shadow.get(
            "active_shadow_lifecycle_eligible_failure_rows",
            0,
        ),
        "active_shadow_observed_bcv2_authoritative_without_route_compatible_rows": active_shadow.get(
            "active_shadow_observed_bcv2_authoritative_without_route_compatible_rows",
            0,
        ),
    }


def probe_readiness(report: dict[str, Any]) -> dict[str, Any]:
    coverage = report.get("probe_join_key_coverage", {})
    materialization = report.get("probe_entry_materialization", {})
    selection_rows = coverage.get("probe_selection_rows", 0)
    transport_rows = coverage.get("probe_transport_rows", 0)
    entry_rows = coverage.get("probe_entry_rows", 0)
    quality = coverage.get("probe_join_quality") or probe_join_quality(report)
    decision_join = report.get("probe_decision_join", {})
    decision_join_acceptance = decision_join.get("decision_join_acceptance", "fail")
    required_decision_join_coverage = decision_join.get(
        "required_exact_decision_v3_join_coverage",
        0.0,
    )
    exact_ab_common = report.get("probe_artifact_intersections", {}).get("ab_record_id", {}).get("common_values", 0)
    exact_probe_common = report.get("probe_artifact_intersections", {}).get("probe_id", {}).get("common_values", 0)
    status = "ready_for_probe_transport_entry_join"
    reasons: list[str] = []
    if selection_rows <= 0:
        status = "not_ready"
        reasons.append("missing_probe_selection_rows")
    if transport_rows <= 0:
        status = "not_ready"
        reasons.append("missing_probe_transport_rows")
    if entry_rows <= 0:
        status = "not_ready"
        reasons.append("missing_probe_entry_rows")
    if exact_ab_common <= 0:
        status = "degraded" if status != "not_ready" else status
        reasons.append("no_common_probe_ab_record_id")
    if exact_probe_common <= 0:
        status = "degraded" if status != "not_ready" else status
        reasons.append("no_common_probe_id")
    if decision_join_acceptance != "pass":
        status = "not_ready"
        reasons.append("probe_rows_missing_exact_decision_v3_join")
    if materialization.get("account_not_found_unattributed_rows", 0) > 0:
        status = "not_ready"
        reasons.append("unattributed_account_not_found_blocks_collection")
    if materialization.get("unattributed_after_narrowing_rows", 0) > 0:
        status = "not_ready"
        reasons.append("unattributed_after_narrowing_blocks_collection")
    if materialization.get("multi_candidate_narrowed_rows", 0) > 0:
        status = "not_ready"
        reasons.append("multi_candidate_narrowed_requires_explicit_acceptance")
    if materialization.get("all_candidates_nonfatal_but_sim_failed_rows", 0) > 0:
        status = "not_ready"
        reasons.append("all_candidates_nonfatal_but_sim_failed_requires_rpc_visibility_review")
    if materialization.get("unexplained_account_set_mismatch_rows", 0) > 0:
        status = "not_ready"
        reasons.append("unexplained_precheck_simulation_account_set_mismatch")
    probe_working_builder_invariants = {
        "probe_working_builder_variant_drift_rows": "probe_working_builder_variant_drift",
        "probe_working_builder_legacy_variant_rows": "probe_working_builder_legacy_variant",
        "probe_working_builder_selected_legacy_handoff_rows": "probe_working_builder_selected_legacy_handoff",
        "probe_working_builder_stale_route_diagnostics_rows": "probe_working_builder_stale_route_diagnostics",
    }
    for field, reason in probe_working_builder_invariants.items():
        if materialization.get(field, 0) > 0:
            status = "not_ready"
            reasons.append(reason)
    if materialization.get("bonding_curve_v2_account_not_found_after_simulation_rows", 0) > 0:
        status = "not_ready"
        reasons.append("bonding_curve_v2_account_not_found_after_simulation")
    if materialization.get("bonding_curve_v2_authority_status_counts", {}).get("builder_only", 0) > 0:
        status = "not_ready"
        reasons.append("bonding_curve_v2_source_not_authoritative")
    if materialization.get("skip_bonding_curve_v2_authority_status_counts", {}).get(
        "builder_only",
        0,
    ) > 0:
        status = "not_ready"
        reasons.append("bonding_curve_v2_source_not_authoritative_skip")
    if materialization.get("observed_bcv2_authoritative_without_route_compatible_rows", 0) > 0:
        status = "not_ready"
        reasons.append("observed_bcv2_authoritative_without_route_compatible")
    if status == "ready_for_probe_transport_entry_join" and quality in {
        "exact_probe_id_and_ab_record_id",
        "exact_ab_record_id",
    } and decision_join_acceptance == "pass":
        join_key_acceptance = "pass"
    elif status == "not_ready":
        join_key_acceptance = "fail"
    else:
        join_key_acceptance = "degraded"
    return {
        "status": status,
        "reasons": reasons,
        "join_key_acceptance": join_key_acceptance,
        "join_quality": quality,
        "probe_selection_rows": selection_rows,
        "probe_transport_rows": transport_rows,
        "probe_entry_rows": entry_rows,
        "probe_lifecycle_rows": coverage.get("probe_lifecycle_rows", 0),
        "decision_join_acceptance": decision_join_acceptance,
        "required_exact_decision_v3_join_coverage": required_decision_join_coverage,
        "account_not_found_rows": materialization.get("account_not_found_rows", 0),
        "account_not_found_attributed_rows": materialization.get(
            "account_not_found_attributed_rows",
            0,
        ),
        "account_not_found_multi_candidate_rows": materialization.get(
            "account_not_found_multi_candidate_rows",
            0,
        ),
        "account_not_found_unattributed_rows": materialization.get(
            "account_not_found_unattributed_rows",
            0,
        ),
        "exact_after_narrowing_rows": materialization.get("exact_after_narrowing_rows", 0),
        "multi_candidate_narrowed_rows": materialization.get(
            "multi_candidate_narrowed_rows",
            0,
        ),
        "unattributed_after_narrowing_rows": materialization.get(
            "unattributed_after_narrowing_rows",
            0,
        ),
        "all_candidates_nonfatal_but_sim_failed_rows": materialization.get(
            "all_candidates_nonfatal_but_sim_failed_rows",
            0,
        ),
        "simulation_rpc_visibility_gap_rows": materialization.get(
            "simulation_rpc_visibility_gap_rows",
            0,
        ),
        "simulation_required_account_not_in_precheck_rows": materialization.get(
            "simulation_required_account_not_in_precheck_rows",
            0,
        ),
        "simulation_account_meta_missing_on_rpc_rows": materialization.get(
            "simulation_account_meta_missing_on_rpc_rows",
            0,
        ),
        "bonding_curve_v2_precheck_skipped_before_simulation_rows": materialization.get(
            "bonding_curve_v2_precheck_skipped_before_simulation_rows",
            0,
        ),
        "bonding_curve_v2_account_not_found_after_simulation_rows": materialization.get(
            "bonding_curve_v2_account_not_found_after_simulation_rows",
            0,
        ),
        "bonding_curve_v2_authority_status_counts": materialization.get(
            "bonding_curve_v2_authority_status_counts",
            {},
        ),
        "skip_bonding_curve_v2_authority_status_counts": materialization.get(
            "skip_bonding_curve_v2_authority_status_counts",
            {},
        ),
        "precheck_simulation_account_set_mismatch_rows": materialization.get(
            "precheck_simulation_account_set_mismatch_rows",
            0,
        ),
        "successful_probe_entry_rows": materialization.get("successful_probe_entry_rows", 0),
        "simulation_error_entry_rows": materialization.get("simulation_error_entry_rows", 0),
        "lifecycle_eligible_entry_rows": materialization.get(
            "lifecycle_eligible_entry_rows",
            0,
        ),
        "observed_bcv2_authoritative_without_route_compatible_rows": materialization.get(
            "observed_bcv2_authoritative_without_route_compatible_rows",
            0,
        ),
        "observed_bcv2_provenance_status_counts": materialization.get(
            "observed_bcv2_provenance_status_counts",
            {},
        ),
    }


def execution_feasibility_summary(report: dict[str, Any]) -> dict[str, Any]:
    materialization = report.get("probe_entry_materialization", {})
    active = report.get("active_shadow_dispatch_diagnostics", {})
    decision_rows_total = artifact_totals(report, "decision")
    probe_selected_rows = materialization.get("probe_selected_rows", 0)
    route_executable_rows = materialization.get("route_executable_rows", 0) + active.get(
        "active_shadow_route_executable_rows",
        0,
    )
    route_non_executable_rows = materialization.get("route_non_executable_rows", 0) + active.get(
        "active_shadow_route_non_executable_rows",
        0,
    )
    execution_feasibility_reject_rows = materialization.get(
        "execution_feasibility_reject_rows",
        0,
    ) + active.get("active_shadow_execution_feasibility_reject_rows", 0)
    successful_entry_rows = materialization.get("successful_probe_entry_rows", 0) + active.get(
        "active_shadow_successful_entry_rows",
        0,
    )
    lifecycle_eligible_rows = materialization.get("lifecycle_eligible_entry_rows", 0) + active.get(
        "active_shadow_lifecycle_eligible_rows",
        0,
    )
    lifecycle_labeled_rows = materialization.get("lifecycle_labeled_rows", 0) + active.get(
        "active_shadow_lifecycle_rows",
        0,
    )
    denominator = max(probe_selected_rows, 0)
    execution_feasibility_rate = (
        route_executable_rows / denominator if denominator > 0 else None
    )
    entry_materialization_rate = (
        successful_entry_rows / route_executable_rows if route_executable_rows > 0 else None
    )
    lifecycle_label_rate = (
        lifecycle_labeled_rows / successful_entry_rows if successful_entry_rows > 0 else None
    )
    return {
        "decision_rows_total": decision_rows_total,
        "probe_selected_rows": probe_selected_rows,
        "route_executable_rows": route_executable_rows,
        "route_non_executable_rows": route_non_executable_rows,
        "successful_entry_rows": successful_entry_rows,
        "lifecycle_eligible_rows": lifecycle_eligible_rows,
        "lifecycle_labeled_rows": lifecycle_labeled_rows,
        "buy_quality_labeled_rows": lifecycle_labeled_rows,
        "execution_feasibility_reject_rows": execution_feasibility_reject_rows,
        "active_buy_execution_infeasible_rows": active.get(
            "active_buy_execution_infeasible_rows",
            0,
        ),
        "execution_feasibility_rate": execution_feasibility_rate,
        "entry_materialization_rate": entry_materialization_rate,
        "lifecycle_label_rate": lifecycle_label_rate,
        "probe_execution_feasibility_status_counts": materialization.get(
            "execution_feasibility_status_counts",
            {},
        ),
        "active_shadow_execution_feasibility_status_counts": active.get(
            "active_shadow_execution_feasibility_status_counts",
            {},
        ),
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
    with_ids = [
        artifact_summary(path_type, path)
        for path_type, values in sorted(paths.items())
        for path in values
        if path_type in CANONICAL_JOIN_ARTIFACTS
    ]
    probe_with_ids = [
        artifact_summary(path_type, path)
        for path_type, values in sorted(paths.items())
        for path in values
        if path_type in PROBE_JOIN_ARTIFACTS
    ]
    report = {
        "schema_version": SCHEMA_VERSION,
        "config_path": str(resolved),
        "artifacts": artifacts,
        "cross_artifact_intersections": intersection_counts(with_ids),
        "probe_artifact_intersections": intersection_counts(probe_with_ids),
    }
    report["active_shadow_dispatch_diagnostics"] = active_shadow_dispatch_diagnostics(paths)
    report["join_key_coverage"] = join_key_coverage(report)
    report["readiness"] = readiness(report)
    report["probe_join_key_coverage"] = probe_join_key_coverage(report)
    report["probe_decision_join"] = probe_decision_join(paths)
    report["probe_entry_materialization"] = probe_entry_materialization(paths)
    report["probe_readiness"] = probe_readiness(report)
    report["execution_feasibility"] = execution_feasibility_summary(report)
    return report


def render_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# P3.7-J MFS Lifecycle Join-Key Audit",
        "",
        f"- config: `{report['config_path']}`",
        f"- readiness: `{report['readiness']['status']}`",
        f"- join_key_acceptance: `{report['readiness']['join_key_acceptance']}`",
        f"- join_quality: `{report['join_key_coverage']['join_quality']}`",
        f"- probe_readiness: `{report['probe_readiness']['status']}`",
        f"- probe_join_key_acceptance: `{report['probe_readiness']['join_key_acceptance']}`",
        f"- probe_join_quality: `{report['probe_join_key_coverage']['probe_join_quality']}`",
        f"- probe_decision_join_acceptance: `{report['probe_decision_join']['decision_join_acceptance']}`",
        f"- probe_required_exact_decision_v3_join_coverage: `{report['probe_decision_join']['required_exact_decision_v3_join_coverage']}`",
        f"- probe_entry_materialization_status_counts: `{json.dumps(report['probe_entry_materialization']['status_counts'], ensure_ascii=False, sort_keys=True)}`",
        f"- probe_entry_materialization_reason_counts: `{json.dumps(report['probe_entry_materialization']['reason_counts'], ensure_ascii=False, sort_keys=True)}`",
        f"- full_chain_ab_record_id_coverage: `{report['join_key_coverage']['full_chain_ab_record_id_coverage']}`",
        f"- probe_chain_ab_record_id_coverage: `{report['probe_join_key_coverage']['probe_chain_ab_record_id_coverage']}`",
        f"- probe_chain_probe_id_coverage: `{report['probe_join_key_coverage']['probe_chain_probe_id_coverage']}`",
        f"- readiness_reasons: `{json.dumps(report['readiness']['reasons'], ensure_ascii=False)}`",
        f"- probe_readiness_reasons: `{json.dumps(report['probe_readiness']['reasons'], ensure_ascii=False)}`",
        f"- decision_rows_with_ab_record_id: `{report['join_key_coverage']['decision_rows_with_ab_record_id']}`",
        f"- shadow_transport_rows_with_ab_record_id: `{report['join_key_coverage']['shadow_transport_rows_with_ab_record_id']}`",
        f"- shadow_entry_rows_with_ab_record_id: `{report['join_key_coverage']['shadow_entry_rows_with_ab_record_id']}`",
        f"- shadow_lifecycle_rows_with_ab_record_id: `{report['join_key_coverage']['shadow_lifecycle_rows_with_ab_record_id']}`",
        f"- onchain_lifecycle_rows_with_ab_record_id: `{report['join_key_coverage']['onchain_lifecycle_rows_with_ab_record_id']}`",
        f"- probe_transport_rows_with_ab_record_id: `{report['probe_join_key_coverage']['probe_transport_rows_with_ab_record_id']}`",
        f"- probe_entry_rows_with_ab_record_id: `{report['probe_join_key_coverage']['probe_entry_rows_with_ab_record_id']}`",
        f"- probe_transport_rows_with_probe_id: `{report['probe_join_key_coverage']['probe_transport_rows_with_probe_id']}`",
        f"- probe_entry_rows_with_probe_id: `{report['probe_join_key_coverage']['probe_entry_rows_with_probe_id']}`",
        "",
        "## Artifact Coverage",
        "",
        "| artifact | rows | candidate_id | ab_record_id | probe_id | pool_id | mint | v3_payload | feature_hash |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for artifact_type, items in report["artifacts"].items():
        for item in items:
            counts = item["field_counts"]
            lines.append(
                f"| `{artifact_type}` | {item['rows']} | {counts.get('candidate_id', 0)} | "
                f"{counts.get('ab_record_id', 0)} | {counts.get('probe_id', 0)} | "
                f"{counts.get('pool_id', 0)} | {counts.get('mint', 0)} | "
                f"{counts.get('v3_replay_payload', 0)} | "
                f"{counts.get('feature_snapshot_hash', 0)} |"
            )
    lines.extend(["", "## Cross-Artifact Intersections", ""])
    for key, value in report["cross_artifact_intersections"].items():
        lines.append(f"- `{key}`: `{json.dumps(value, ensure_ascii=False, sort_keys=True)}`")
    lines.extend(["", "## Probe Artifact Intersections", ""])
    for key, value in report["probe_artifact_intersections"].items():
        lines.append(f"- `{key}`: `{json.dumps(value, ensure_ascii=False, sort_keys=True)}`")
    lines.extend(["", "## Probe Decision Join", ""])
    lines.append(
        f"- decision_join_acceptance: `{report['probe_decision_join']['decision_join_acceptance']}`"
    )
    lines.append(
        f"- required_exact_decision_v3_join_coverage: `{report['probe_decision_join']['required_exact_decision_v3_join_coverage']}`"
    )
    for key, value in report["probe_decision_join"]["artifacts"].items():
        lines.append(f"- `{key}`: `{json.dumps(value, ensure_ascii=False, sort_keys=True)}`")
    execution_feasibility = report["execution_feasibility"]
    lines.extend(
        [
            "",
            "## Execution Feasibility",
            "",
            f"- decision_rows_total: `{execution_feasibility['decision_rows_total']}`",
            f"- probe_selected_rows: `{execution_feasibility['probe_selected_rows']}`",
            f"- route_executable_rows: `{execution_feasibility['route_executable_rows']}`",
            f"- route_non_executable_rows: `{execution_feasibility['route_non_executable_rows']}`",
            f"- successful_entry_rows: `{execution_feasibility['successful_entry_rows']}`",
            f"- lifecycle_eligible_rows: `{execution_feasibility['lifecycle_eligible_rows']}`",
            f"- lifecycle_labeled_rows: `{execution_feasibility['lifecycle_labeled_rows']}`",
            f"- buy_quality_labeled_rows: `{execution_feasibility['buy_quality_labeled_rows']}`",
            f"- execution_feasibility_reject_rows: `{execution_feasibility['execution_feasibility_reject_rows']}`",
            f"- active_buy_execution_infeasible_rows: `{execution_feasibility['active_buy_execution_infeasible_rows']}`",
            f"- execution_feasibility_rate: `{execution_feasibility['execution_feasibility_rate']}`",
            f"- entry_materialization_rate: `{execution_feasibility['entry_materialization_rate']}`",
            f"- lifecycle_label_rate: `{execution_feasibility['lifecycle_label_rate']}`",
            f"- probe_execution_feasibility_status_counts: `{json.dumps(execution_feasibility['probe_execution_feasibility_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_execution_feasibility_status_counts: `{json.dumps(execution_feasibility['active_shadow_execution_feasibility_status_counts'], ensure_ascii=False, sort_keys=True)}`",
        ]
    )
    active_shadow = report["active_shadow_dispatch_diagnostics"]
    lines.extend(
        [
            "",
            "## Active Shadow Dispatch Diagnostics",
            "",
            f"- active_shadow_transport_rows: `{active_shadow['active_shadow_transport_rows']}`",
            f"- active_shadow_entry_rows: `{active_shadow['active_shadow_entry_rows']}`",
            f"- active_shadow_lifecycle_rows: `{active_shadow['active_shadow_lifecycle_rows']}`",
            f"- active_shadow_dispatch_failure_rows: `{active_shadow['active_shadow_dispatch_failure_rows']}`",
            f"- active_shadow_precheck_failed_rows: `{active_shadow['active_shadow_precheck_failed_rows']}`",
            f"- active_shadow_runtime_simulation_error_rows: `{active_shadow['active_shadow_runtime_simulation_error_rows']}`",
            f"- active_shadow_successful_entry_rows: `{active_shadow['active_shadow_successful_entry_rows']}`",
            f"- active_shadow_lifecycle_eligible_rows: `{active_shadow['active_shadow_lifecycle_eligible_rows']}`",
            f"- active_shadow_lifecycle_eligible_failure_rows: `{active_shadow['active_shadow_lifecycle_eligible_failure_rows']}`",
            f"- active_shadow_simulation_required_account_not_in_precheck_count: `{active_shadow['active_shadow_simulation_required_account_not_in_precheck_count']}`",
            f"- active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows: `{active_shadow['active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows']}`",
            f"- active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows: `{active_shadow['active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows']}`",
            f"- active_shadow_account_not_found_rows: `{active_shadow['active_shadow_account_not_found_rows']}`",
            f"- active_shadow_account_not_found_attributed_rows: `{active_shadow['active_shadow_account_not_found_attributed_rows']}`",
            f"- active_shadow_account_not_found_multi_candidate_rows: `{active_shadow['active_shadow_account_not_found_multi_candidate_rows']}`",
            f"- active_shadow_account_not_found_unattributed_rows: `{active_shadow['active_shadow_account_not_found_unattributed_rows']}`",
            f"- active_shadow_rpc_visibility_gap_rows: `{active_shadow['active_shadow_rpc_visibility_gap_rows']}`",
            f"- active_shadow_account_not_found_role_counts: `{json.dumps(active_shadow['active_shadow_account_not_found_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_simulation_error_category_counts: `{json.dumps(active_shadow['active_shadow_simulation_error_category_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_precheck_status_counts: `{json.dumps(active_shadow['active_shadow_precheck_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_lifecycle_eligibility_status_counts: `{json.dumps(active_shadow['active_shadow_lifecycle_eligibility_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_account_set_match_counts: `{json.dumps(active_shadow['active_shadow_account_set_match_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_account_narrowing_status_counts: `{json.dumps(active_shadow['active_shadow_account_narrowing_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_account_candidate_raw_counts: `{json.dumps(active_shadow['active_shadow_account_candidate_raw_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_account_candidate_narrowed_counts: `{json.dumps(active_shadow['active_shadow_account_candidate_narrowed_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_authority_status_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_identity_authority_status_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_identity_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_mismatch_reason_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_source_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_rpc_load_status_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_bonding_curve_v2_rpc_load_ready_counts: `{json.dumps(active_shadow['active_shadow_bonding_curve_v2_rpc_load_ready_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_builder_required_curve_account_ready_counts: `{json.dumps(active_shadow['active_shadow_builder_required_curve_account_ready_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_builder_required_curve_account_ready_reason_counts: `{json.dumps(active_shadow['active_shadow_builder_required_curve_account_ready_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_observed_bcv2_provenance_status_counts: `{json.dumps(active_shadow['active_shadow_observed_bcv2_provenance_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_observed_bcv2_rows: `{active_shadow['active_shadow_observed_bcv2_rows']}`",
            f"- active_shadow_observed_bcv2_route_compatible_rows: `{active_shadow['active_shadow_observed_bcv2_route_compatible_rows']}`",
            f"- active_shadow_observed_bcv2_not_route_compatible_rows: `{active_shadow['active_shadow_observed_bcv2_not_route_compatible_rows']}`",
            f"- active_shadow_observed_bcv2_missing_provenance_rows: `{active_shadow['active_shadow_observed_bcv2_missing_provenance_rows']}`",
            f"- active_shadow_observed_bcv2_authoritative_without_route_compatible_rows: `{active_shadow['active_shadow_observed_bcv2_authoritative_without_route_compatible_rows']}`",
            f"- active_shadow_route_fallback_attempted_rows: `{active_shadow['active_shadow_route_fallback_attempted_rows']}`",
            f"- active_shadow_route_fallback_success_rows: `{active_shadow['active_shadow_route_fallback_success_rows']}`",
            f"- active_shadow_route_fallback_failed_rows: `{active_shadow['active_shadow_route_fallback_failed_rows']}`",
            f"- active_shadow_working_builder_parity_rows: `{active_shadow['active_shadow_working_builder_parity_rows']}`",
            f"- active_shadow_working_builder_request_built_rows: `{active_shadow['active_shadow_working_builder_request_built_rows']}`",
            f"- active_shadow_working_builder_buy_variant_counts: `{json.dumps(active_shadow['active_shadow_working_builder_buy_variant_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_probe_working_builder_variant_drift_rows: `{active_shadow['active_shadow_probe_working_builder_variant_drift_rows']}`",
            f"- active_shadow_probe_working_builder_legacy_variant_rows: `{active_shadow['active_shadow_probe_working_builder_legacy_variant_rows']}`",
            f"- active_shadow_probe_working_builder_selected_legacy_handoff_rows: `{active_shadow['active_shadow_probe_working_builder_selected_legacy_handoff_rows']}`",
            f"- active_shadow_probe_working_builder_stale_route_diagnostics_rows: `{active_shadow['active_shadow_probe_working_builder_stale_route_diagnostics_rows']}`",
            f"- active_shadow_legacy_fallback_attempted_rows: `{active_shadow['active_shadow_legacy_fallback_attempted_rows']}`",
            f"- active_shadow_selected_route_handoff_mismatch_rows: `{active_shadow['active_shadow_selected_route_handoff_mismatch_rows']}`",
            f"- active_shadow_working_builder_manifest_missing_required_rows: `{active_shadow['active_shadow_working_builder_manifest_missing_required_rows']}`",
            f"- active_shadow_working_builder_manifest_ready_rows: `{active_shadow['active_shadow_working_builder_manifest_ready_rows']}`",
            f"- active_shadow_working_builder_manifest_contains_bcv2_rows: `{active_shadow['active_shadow_working_builder_manifest_contains_bcv2_rows']}`",
            f"- active_shadow_working_builder_bcv2_source_authority_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_source_authority_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_rpc_load_status_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_reconciliation_class_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_reconciliation_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_pubkey_consistency_status_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_pubkey_consistency_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_precheck_commitment_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_precheck_commitment_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_rpc_error_class_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_rpc_error_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_loaded_address_source_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_loaded_address_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_precheck_age_bucket_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_precheck_age_bucket_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_local_coverage_class_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_local_coverage_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_materialization_class_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_materialization_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_subscription_requested_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_subscription_requested_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_account_update_received_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_account_update_received_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_account_update_mapped_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_account_update_mapped_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_account_state_lookup_performed_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_account_state_lookup_performed_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_account_state_age_bucket_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_account_state_age_bucket_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_mfs_seen_reason_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_mfs_seen_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_diag_seen_reason_counts: `{json.dumps(active_shadow['active_shadow_working_builder_bcv2_diag_seen_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_precheck_pubkey_rows: `{active_shadow['active_shadow_working_builder_bcv2_precheck_pubkey_rows']}`",
            f"- active_shadow_working_builder_bcv2_builder_pubkey_rows: `{active_shadow['active_shadow_working_builder_bcv2_builder_pubkey_rows']}`",
            f"- active_shadow_working_builder_bcv2_observed_pubkey_rows: `{active_shadow['active_shadow_working_builder_bcv2_observed_pubkey_rows']}`",
            f"- active_shadow_working_builder_bcv2_observed_slot_rows: `{active_shadow['active_shadow_working_builder_bcv2_observed_slot_rows']}`",
            f"- active_shadow_working_builder_bcv2_observed_tx_signature_rows: `{active_shadow['active_shadow_working_builder_bcv2_observed_tx_signature_rows']}`",
            f"- active_shadow_working_builder_bcv2_precheck_context_slot_rows: `{active_shadow['active_shadow_working_builder_bcv2_precheck_context_slot_rows']}`",
            f"- active_shadow_working_builder_bcv2_precheck_attempt_count_rows: `{active_shadow['active_shadow_working_builder_bcv2_precheck_attempt_count_rows']}`",
            f"- active_shadow_working_builder_bcv2_precheck_latency_rows: `{active_shadow['active_shadow_working_builder_bcv2_precheck_latency_rows']}`",
            f"- active_shadow_working_builder_bcv2_precheck_age_from_observed_slot_rows: `{active_shadow['active_shadow_working_builder_bcv2_precheck_age_from_observed_slot_rows']}`",
            f"- active_shadow_working_builder_bcv2_loaded_address_source_missing_rows: `{active_shadow['active_shadow_working_builder_bcv2_loaded_address_source_missing_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_lookup_performed_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_lookup_performed_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_seen_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_seen_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_seen_slot_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_seen_slot_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_age_slots_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_age_slots_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_owner_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_owner_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_data_len_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_data_len_rows']}`",
            f"- active_shadow_working_builder_bcv2_subscription_requested_rows: `{active_shadow['active_shadow_working_builder_bcv2_subscription_requested_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_update_received_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_update_received_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_update_mapped_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_update_mapped_rows']}`",
            f"- active_shadow_working_builder_bcv2_rpc_fetch_ready_rows: `{active_shadow['active_shadow_working_builder_bcv2_rpc_fetch_ready_rows']}`",
            f"- active_shadow_working_builder_bcv2_rpc_fetch_missing_rows: `{active_shadow['active_shadow_working_builder_bcv2_rpc_fetch_missing_rows']}`",
            f"- active_shadow_working_builder_bcv2_rpc_fetch_owner_rows: `{active_shadow['active_shadow_working_builder_bcv2_rpc_fetch_owner_rows']}`",
            f"- active_shadow_working_builder_bcv2_rpc_fetch_data_len_rows: `{active_shadow['active_shadow_working_builder_bcv2_rpc_fetch_data_len_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_materialized_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_materialized_rows']}`",
            f"- active_shadow_working_builder_bcv2_mfs_materialized_rows: `{active_shadow['active_shadow_working_builder_bcv2_mfs_materialized_rows']}`",
            f"- active_shadow_working_builder_bcv2_diag_materialized_rows: `{active_shadow['active_shadow_working_builder_bcv2_diag_materialized_rows']}`",
            f"- active_shadow_working_builder_creator_vault_source_authority_counts: `{json.dumps(active_shadow['active_shadow_working_builder_creator_vault_source_authority_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_creator_vault_rpc_load_status_counts: `{json.dumps(active_shadow['active_shadow_working_builder_creator_vault_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows: `{active_shadow['active_shadow_working_builder_bcv2_authoritative_and_load_ready_rows']}`",
            f"- active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows: `{active_shadow['active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows']}`",
            f"- active_shadow_working_builder_bcv2_pubkey_mismatch_rows: `{active_shadow['active_shadow_working_builder_bcv2_pubkey_mismatch_rows']}`",
            f"- active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows: `{active_shadow['active_shadow_working_builder_bcv2_observed_tx_missing_on_rpc_rows']}`",
            f"- active_shadow_working_builder_bcv2_account_state_missing_rows: `{active_shadow['active_shadow_working_builder_bcv2_account_state_missing_rows']}`",
            f"- active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows: `{active_shadow['active_shadow_working_builder_creator_vault_authoritative_and_load_ready_rows']}`",
            f"- active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows: `{active_shadow['active_shadow_working_builder_creator_vault_authoritative_but_missing_on_rpc_rows']}`",
            f"- active_shadow_working_builder_creator_vault_source_mismatch_rows: `{active_shadow['active_shadow_working_builder_creator_vault_source_mismatch_rows']}`",
            f"- active_shadow_working_builder_manifest_ready_after_account_source_repair_rows: `{active_shadow['active_shadow_working_builder_manifest_ready_after_account_source_repair_rows']}`",
            f"- active_shadow_working_builder_manifest_still_not_ready_after_account_source_repair_rows: `{active_shadow['active_shadow_working_builder_manifest_still_not_ready_after_account_source_repair_rows']}`",
            f"- active_shadow_legacy_buy_route_attempted_rows: `{active_shadow['active_shadow_legacy_buy_route_attempted_rows']}`",
            f"- active_shadow_legacy_buy_route_ready_rows: `{active_shadow['active_shadow_legacy_buy_route_ready_rows']}`",
            f"- active_shadow_legacy_buy_route_not_ready_rows: `{active_shadow['active_shadow_legacy_buy_route_not_ready_rows']}`",
            f"- active_shadow_legacy_buy_missing_core_curve_account_rows: `{active_shadow['active_shadow_legacy_buy_missing_core_curve_account_rows']}`",
            f"- active_shadow_legacy_buy_missing_associated_bonding_curve_rows: `{active_shadow['active_shadow_legacy_buy_missing_associated_bonding_curve_rows']}`",
            f"- active_shadow_legacy_buy_authoritative_curve_rows: `{active_shadow['active_shadow_legacy_buy_authoritative_curve_rows']}`",
            f"- active_shadow_legacy_buy_rpc_load_ready_rows: `{active_shadow['active_shadow_legacy_buy_rpc_load_ready_rows']}`",
            f"- active_shadow_legacy_buy_successful_entry_rows: `{active_shadow['active_shadow_legacy_buy_successful_entry_rows']}`",
            f"- active_shadow_legacy_buy_account_set_status_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_account_set_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_curve_source_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_curve_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_curve_authority_status_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_curve_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_curve_rpc_load_status_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_curve_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_curve_authority_readiness_status_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_curve_authority_readiness_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_curve_authoritative_and_load_ready_rows: `{active_shadow['active_shadow_legacy_buy_curve_authoritative_and_load_ready_rows']}`",
            f"- active_shadow_legacy_buy_curve_load_ready_but_authority_unverified_rows: `{active_shadow['active_shadow_legacy_buy_curve_load_ready_but_authority_unverified_rows']}`",
            f"- active_shadow_legacy_buy_curve_authoritative_but_not_checked_rows: `{active_shadow['active_shadow_legacy_buy_curve_authoritative_but_not_checked_rows']}`",
            f"- active_shadow_legacy_buy_curve_derived_matches_account_state_rows: `{active_shadow['active_shadow_legacy_buy_curve_derived_matches_account_state_rows']}`",
            f"- active_shadow_legacy_buy_curve_derived_mismatch_account_state_rows: `{active_shadow['active_shadow_legacy_buy_curve_derived_mismatch_account_state_rows']}`",
            f"- active_shadow_legacy_buy_route_ready_after_reconciliation_rows: `{active_shadow['active_shadow_legacy_buy_route_ready_after_reconciliation_rows']}`",
            f"- active_shadow_legacy_buy_route_still_not_ready_after_reconciliation_rows: `{active_shadow['active_shadow_legacy_buy_route_still_not_ready_after_reconciliation_rows']}`",
            f"- active_shadow_legacy_buy_route_not_ready_reason_counts: `{json.dumps(active_shadow['active_shadow_legacy_buy_route_not_ready_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_legacy_buy_primary_bcv2_leak_rows: `{active_shadow['active_shadow_legacy_buy_primary_bcv2_leak_rows']}`",
            f"- active_shadow_legacy_buy_missing_creatable_user_ata_rows: `{active_shadow['active_shadow_legacy_buy_missing_creatable_user_ata_rows']}`",
            f"- active_shadow_legacy_buy_missing_creatable_user_volume_accumulator_rows: `{active_shadow['active_shadow_legacy_buy_missing_creatable_user_volume_accumulator_rows']}`",
            f"- active_shadow_legacy_buy_missing_ephemeral_payer_rows: `{active_shadow['active_shadow_legacy_buy_missing_ephemeral_payer_rows']}`",
            f"- active_shadow_legacy_buy_blocking_missing_required_rows: `{active_shadow['active_shadow_legacy_buy_blocking_missing_required_rows']}`",
            f"- active_shadow_legacy_buy_non_blocking_missing_creatable_rows: `{active_shadow['active_shadow_legacy_buy_non_blocking_missing_creatable_rows']}`",
            f"- active_shadow_legacy_buy_non_blocking_ephemeral_payer_rows: `{active_shadow['active_shadow_legacy_buy_non_blocking_ephemeral_payer_rows']}`",
            f"- active_shadow_legacy_buy_fallback_account_set_ready_rows: `{active_shadow['active_shadow_legacy_buy_fallback_account_set_ready_rows']}`",
            f"- active_shadow_legacy_buy_route_ready_after_account_set_separation_rows: `{active_shadow['active_shadow_legacy_buy_route_ready_after_account_set_separation_rows']}`",
            f"- active_shadow_legacy_buy_route_unsupported_builder_layout_rows: `{active_shadow['active_shadow_legacy_buy_route_unsupported_builder_layout_rows']}`",
            f"- active_shadow_legacy_buy_excluded_from_execution_route_universe_rows: `{active_shadow['active_shadow_legacy_buy_excluded_from_execution_route_universe_rows']}`",
            f"- active_shadow_legacy_buy_removed_from_fallback_candidates_rows: `{active_shadow['active_shadow_legacy_buy_removed_from_fallback_candidates_rows']}`",
            f"- active_shadow_selected_fallback_route_ready_rows: `{active_shadow['active_shadow_selected_fallback_route_ready_rows']}`",
            f"- active_shadow_selected_fallback_route_handoff_applied_rows: `{active_shadow['active_shadow_selected_fallback_route_handoff_applied_rows']}`",
            f"- active_shadow_selected_fallback_route_handoff_mismatch_rows: `{active_shadow['active_shadow_selected_fallback_route_handoff_mismatch_rows']}`",
            f"- active_shadow_selected_fallback_route_handoff_not_applied_rows: `{active_shadow['active_shadow_selected_fallback_route_handoff_not_applied_rows']}`",
            f"- active_shadow_selected_fallback_route_blocked_by_primary_reason_rows: `{active_shadow['active_shadow_selected_fallback_route_blocked_by_primary_reason_rows']}`",
            f"- active_shadow_legacy_buy_selected_but_primary_bcv2_terminal_rows: `{active_shadow['active_shadow_legacy_buy_selected_but_primary_bcv2_terminal_rows']}`",
            f"- active_shadow_selected_legacy_handoff_claimed_rows: `{active_shadow['active_shadow_selected_legacy_handoff_claimed_rows']}`",
            f"- active_shadow_selected_legacy_handoff_validated_rows: `{active_shadow['active_shadow_selected_legacy_handoff_validated_rows']}`",
            f"- active_shadow_selected_legacy_handoff_mismatch_rows: `{active_shadow['active_shadow_selected_legacy_handoff_mismatch_rows']}`",
            f"- active_shadow_selected_legacy_final_manifest_contains_bcv2_rows: `{active_shadow['active_shadow_selected_legacy_final_manifest_contains_bcv2_rows']}`",
            f"- active_shadow_selected_legacy_final_manifest_contains_primary_route_builder_rows: `{active_shadow['active_shadow_selected_legacy_final_manifest_contains_primary_route_builder_rows']}`",
            f"- active_shadow_selected_legacy_request_variant_not_legacy_rows: `{active_shadow['active_shadow_selected_legacy_request_variant_not_legacy_rows']}`",
            f"- active_shadow_selected_legacy_precheck_hash_mismatch_rows: `{active_shadow['active_shadow_selected_legacy_precheck_hash_mismatch_rows']}`",
            f"- active_shadow_selected_legacy_simulation_hash_mismatch_rows: `{active_shadow['active_shadow_selected_legacy_simulation_hash_mismatch_rows']}`",
            f"- active_shadow_no_executable_route_but_simulated_rows: `{active_shadow['active_shadow_no_executable_route_but_simulated_rows']}`",
            f"- active_shadow_legacy_buy_selected_but_request_variant_not_legacy_rows: `{active_shadow['active_shadow_legacy_buy_selected_but_request_variant_not_legacy_rows']}`",
            f"- active_shadow_legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows: `{active_shadow['active_shadow_legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows']}`",
            f"- active_shadow_legacy_buy_selected_but_precheck_hash_mismatch_rows: `{active_shadow['active_shadow_legacy_buy_selected_but_precheck_hash_mismatch_rows']}`",
            f"- active_shadow_legacy_buy_selected_but_simulation_hash_mismatch_rows: `{active_shadow['active_shadow_legacy_buy_selected_but_simulation_hash_mismatch_rows']}`",
            f"- active_shadow_legacy_buy_selected_and_precheck_uses_legacy_account_set_rows: `{active_shadow['active_shadow_legacy_buy_selected_and_precheck_uses_legacy_account_set_rows']}`",
            f"- active_shadow_legacy_buy_selected_and_simulation_uses_legacy_account_set_rows: `{active_shadow['active_shadow_legacy_buy_selected_and_simulation_uses_legacy_account_set_rows']}`",
            f"- active_shadow_fallback_failure_class_counts: `{json.dumps(active_shadow['active_shadow_fallback_failure_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_fallback_missing_role_counts: `{json.dumps(active_shadow['active_shadow_fallback_missing_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_fallback_account_source_counts: `{json.dumps(active_shadow['active_shadow_fallback_account_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_fallback_repairable: `{active_shadow['active_shadow_fallback_repairable']}`",
            f"- active_shadow_recommended_next_path: `{active_shadow['active_shadow_recommended_next_path']}`",
            f"- active_shadow_executable_route_ready_rows: `{active_shadow['active_shadow_executable_route_ready_rows']}`",
            f"- active_shadow_route_executable_rows: `{active_shadow['active_shadow_route_executable_rows']}`",
            f"- active_shadow_route_non_executable_rows: `{active_shadow['active_shadow_route_non_executable_rows']}`",
            f"- active_shadow_execution_feasibility_reject_rows: `{active_shadow['active_shadow_execution_feasibility_reject_rows']}`",
            f"- active_buy_execution_infeasible_rows: `{active_shadow['active_buy_execution_infeasible_rows']}`",
            f"- active_shadow_execution_feasibility_status_counts: `{json.dumps(active_shadow['active_shadow_execution_feasibility_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_execution_feasibility_reason_counts: `{json.dumps(active_shadow['active_shadow_execution_feasibility_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- active_shadow_lifecycle_label_eligibility_counts: `{json.dumps(active_shadow['active_shadow_lifecycle_label_eligibility_counts'], ensure_ascii=False, sort_keys=True)}`",
        ]
    )
    lines.extend(["", "## Probe Entry Materialization", ""])
    materialization = report["probe_entry_materialization"]
    lines.extend(
        [
            f"- transport_rows: `{materialization['transport_rows']}`",
            f"- entry_rows: `{materialization['entry_rows']}`",
            f"- transport_without_entry_rows: `{materialization['transport_without_entry_rows']}`",
            f"- status_counts: `{json.dumps(materialization['status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- reason_counts: `{json.dumps(materialization['reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- buy_variant_counts: `{json.dumps(materialization['buy_variant_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- token_param_role_counts: `{json.dumps(materialization['token_param_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- creator_vault_authority_status_counts: `{json.dumps(materialization['creator_vault_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- creator_vault_mismatch_reason_counts: `{json.dumps(materialization['creator_vault_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- creator_identity_source_counts: `{json.dumps(materialization['creator_identity_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_authority_status_counts: `{json.dumps(materialization['bonding_curve_v2_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_identity_authority_status_counts: `{json.dumps(materialization['bonding_curve_v2_identity_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_mismatch_reason_counts: `{json.dumps(materialization['bonding_curve_v2_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_source_counts: `{json.dumps(materialization['bonding_curve_v2_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_rpc_load_status_counts: `{json.dumps(materialization['bonding_curve_v2_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- bonding_curve_v2_rpc_load_ready_counts: `{json.dumps(materialization['bonding_curve_v2_rpc_load_ready_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- builder_required_curve_account_ready_counts: `{json.dumps(materialization['builder_required_curve_account_ready_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- builder_required_curve_account_ready_reason_counts: `{json.dumps(materialization['builder_required_curve_account_ready_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- observed_bcv2_provenance_status_counts: `{json.dumps(materialization['observed_bcv2_provenance_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- observed_bcv2_rows: `{materialization['observed_bcv2_rows']}`",
            f"- observed_bcv2_route_compatible_rows: `{materialization['observed_bcv2_route_compatible_rows']}`",
            f"- observed_bcv2_not_route_compatible_rows: `{materialization['observed_bcv2_not_route_compatible_rows']}`",
            f"- observed_bcv2_missing_provenance_rows: `{materialization['observed_bcv2_missing_provenance_rows']}`",
            f"- observed_bcv2_instruction_account_position_present_rows: `{materialization['observed_bcv2_instruction_account_position_present_rows']}`",
            f"- observed_bcv2_message_account_index_present_rows: `{materialization['observed_bcv2_message_account_index_present_rows']}`",
            f"- observed_bcv2_authoritative_without_route_compatible_rows: `{materialization['observed_bcv2_authoritative_without_route_compatible_rows']}`",
            f"- amount_guard_status_counts: `{json.dumps(materialization['amount_guard_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_category_counts: `{json.dumps(materialization['simulation_error_category_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_kind_counts: `{json.dumps(materialization['simulation_error_kind_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_account_role_counts: `{json.dumps(materialization['simulation_error_account_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_account_source_counts: `{json.dumps(materialization['simulation_error_account_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_custom_code_counts: `{json.dumps(materialization['simulation_error_custom_code_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- account_set_match_counts: `{json.dumps(materialization['account_set_match_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- account_set_mismatch_reason_counts: `{json.dumps(materialization['account_set_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- account_not_found_rows: `{materialization['account_not_found_rows']}`",
            f"- account_not_found_attributed_rows: `{materialization['account_not_found_attributed_rows']}`",
            f"- account_not_found_multi_candidate_rows: `{materialization['account_not_found_multi_candidate_rows']}`",
            f"- account_not_found_unattributed_rows: `{materialization['account_not_found_unattributed_rows']}`",
            f"- simulation_rpc_visibility_gap_rows: `{materialization['simulation_rpc_visibility_gap_rows']}`",
            f"- simulation_required_account_not_in_precheck_rows: `{materialization['simulation_required_account_not_in_precheck_rows']}`",
            f"- simulation_account_meta_missing_on_rpc_rows: `{materialization['simulation_account_meta_missing_on_rpc_rows']}`",
            f"- bonding_curve_v2_precheck_skipped_before_simulation_rows: `{materialization['bonding_curve_v2_precheck_skipped_before_simulation_rows']}`",
            f"- bonding_curve_v2_account_not_found_after_simulation_rows: `{materialization['bonding_curve_v2_account_not_found_after_simulation_rows']}`",
            f"- precheck_simulation_account_set_mismatch_rows: `{materialization['precheck_simulation_account_set_mismatch_rows']}`",
            f"- successful_probe_entry_rows: `{materialization['successful_probe_entry_rows']}`",
            f"- simulation_error_entry_rows: `{materialization['simulation_error_entry_rows']}`",
            f"- lifecycle_eligible_entry_rows: `{materialization['lifecycle_eligible_entry_rows']}`",
            f"- route_fallback_attempted_rows: `{materialization['route_fallback_attempted_rows']}`",
            f"- route_fallback_success_rows: `{materialization['route_fallback_success_rows']}`",
            f"- route_fallback_failed_rows: `{materialization['route_fallback_failed_rows']}`",
            f"- working_builder_parity_rows: `{materialization['working_builder_parity_rows']}`",
            f"- working_builder_request_built_rows: `{materialization['working_builder_request_built_rows']}`",
            f"- working_builder_buy_variant_counts: `{json.dumps(materialization['working_builder_buy_variant_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- probe_working_builder_variant_drift_rows: `{materialization['probe_working_builder_variant_drift_rows']}`",
            f"- probe_working_builder_legacy_variant_rows: `{materialization['probe_working_builder_legacy_variant_rows']}`",
            f"- probe_working_builder_selected_legacy_handoff_rows: `{materialization['probe_working_builder_selected_legacy_handoff_rows']}`",
            f"- probe_working_builder_stale_route_diagnostics_rows: `{materialization['probe_working_builder_stale_route_diagnostics_rows']}`",
            f"- legacy_fallback_attempted_rows: `{materialization['legacy_fallback_attempted_rows']}`",
            f"- selected_route_handoff_mismatch_rows: `{materialization['selected_route_handoff_mismatch_rows']}`",
            f"- working_builder_manifest_missing_required_rows: `{materialization['working_builder_manifest_missing_required_rows']}`",
            f"- working_builder_manifest_ready_rows: `{materialization['working_builder_manifest_ready_rows']}`",
            f"- working_builder_manifest_contains_bcv2_rows: `{materialization['working_builder_manifest_contains_bcv2_rows']}`",
            f"- working_builder_bcv2_source_authority_counts: `{json.dumps(materialization['working_builder_bcv2_source_authority_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_rpc_load_status_counts: `{json.dumps(materialization['working_builder_bcv2_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_reconciliation_class_counts: `{json.dumps(materialization['working_builder_bcv2_reconciliation_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_pubkey_consistency_status_counts: `{json.dumps(materialization['working_builder_bcv2_pubkey_consistency_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_precheck_commitment_counts: `{json.dumps(materialization['working_builder_bcv2_precheck_commitment_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_rpc_error_class_counts: `{json.dumps(materialization['working_builder_bcv2_rpc_error_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_loaded_address_source_counts: `{json.dumps(materialization['working_builder_bcv2_loaded_address_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_precheck_age_bucket_counts: `{json.dumps(materialization['working_builder_bcv2_precheck_age_bucket_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_local_coverage_class_counts: `{json.dumps(materialization['working_builder_bcv2_local_coverage_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_materialization_class_counts: `{json.dumps(materialization['working_builder_bcv2_materialization_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_subscription_requested_counts: `{json.dumps(materialization['working_builder_bcv2_subscription_requested_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_account_update_received_counts: `{json.dumps(materialization['working_builder_bcv2_account_update_received_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_account_update_mapped_counts: `{json.dumps(materialization['working_builder_bcv2_account_update_mapped_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_account_state_lookup_performed_counts: `{json.dumps(materialization['working_builder_bcv2_account_state_lookup_performed_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_account_state_age_bucket_counts: `{json.dumps(materialization['working_builder_bcv2_account_state_age_bucket_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_mfs_seen_reason_counts: `{json.dumps(materialization['working_builder_bcv2_mfs_seen_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_diag_seen_reason_counts: `{json.dumps(materialization['working_builder_bcv2_diag_seen_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_precheck_pubkey_rows: `{materialization['working_builder_bcv2_precheck_pubkey_rows']}`",
            f"- working_builder_bcv2_builder_pubkey_rows: `{materialization['working_builder_bcv2_builder_pubkey_rows']}`",
            f"- working_builder_bcv2_observed_pubkey_rows: `{materialization['working_builder_bcv2_observed_pubkey_rows']}`",
            f"- working_builder_bcv2_observed_slot_rows: `{materialization['working_builder_bcv2_observed_slot_rows']}`",
            f"- working_builder_bcv2_observed_tx_signature_rows: `{materialization['working_builder_bcv2_observed_tx_signature_rows']}`",
            f"- working_builder_bcv2_precheck_context_slot_rows: `{materialization['working_builder_bcv2_precheck_context_slot_rows']}`",
            f"- working_builder_bcv2_precheck_attempt_count_rows: `{materialization['working_builder_bcv2_precheck_attempt_count_rows']}`",
            f"- working_builder_bcv2_precheck_latency_rows: `{materialization['working_builder_bcv2_precheck_latency_rows']}`",
            f"- working_builder_bcv2_precheck_age_from_observed_slot_rows: `{materialization['working_builder_bcv2_precheck_age_from_observed_slot_rows']}`",
            f"- working_builder_bcv2_loaded_address_source_missing_rows: `{materialization['working_builder_bcv2_loaded_address_source_missing_rows']}`",
            f"- working_builder_bcv2_account_state_lookup_performed_rows: `{materialization['working_builder_bcv2_account_state_lookup_performed_rows']}`",
            f"- working_builder_bcv2_account_state_seen_rows: `{materialization['working_builder_bcv2_account_state_seen_rows']}`",
            f"- working_builder_bcv2_account_state_seen_slot_rows: `{materialization['working_builder_bcv2_account_state_seen_slot_rows']}`",
            f"- working_builder_bcv2_account_state_age_slots_rows: `{materialization['working_builder_bcv2_account_state_age_slots_rows']}`",
            f"- working_builder_bcv2_account_state_owner_rows: `{materialization['working_builder_bcv2_account_state_owner_rows']}`",
            f"- working_builder_bcv2_account_state_data_len_rows: `{materialization['working_builder_bcv2_account_state_data_len_rows']}`",
            f"- working_builder_bcv2_subscription_requested_rows: `{materialization['working_builder_bcv2_subscription_requested_rows']}`",
            f"- working_builder_bcv2_account_update_received_rows: `{materialization['working_builder_bcv2_account_update_received_rows']}`",
            f"- working_builder_bcv2_account_update_mapped_rows: `{materialization['working_builder_bcv2_account_update_mapped_rows']}`",
            f"- working_builder_bcv2_rpc_fetch_ready_rows: `{materialization['working_builder_bcv2_rpc_fetch_ready_rows']}`",
            f"- working_builder_bcv2_rpc_fetch_missing_rows: `{materialization['working_builder_bcv2_rpc_fetch_missing_rows']}`",
            f"- working_builder_bcv2_rpc_fetch_owner_rows: `{materialization['working_builder_bcv2_rpc_fetch_owner_rows']}`",
            f"- working_builder_bcv2_rpc_fetch_data_len_rows: `{materialization['working_builder_bcv2_rpc_fetch_data_len_rows']}`",
            f"- working_builder_bcv2_account_state_materialized_rows: `{materialization['working_builder_bcv2_account_state_materialized_rows']}`",
            f"- working_builder_bcv2_mfs_materialized_rows: `{materialization['working_builder_bcv2_mfs_materialized_rows']}`",
            f"- working_builder_bcv2_diag_materialized_rows: `{materialization['working_builder_bcv2_diag_materialized_rows']}`",
            f"- working_builder_creator_vault_source_authority_counts: `{json.dumps(materialization['working_builder_creator_vault_source_authority_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_creator_vault_rpc_load_status_counts: `{json.dumps(materialization['working_builder_creator_vault_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- working_builder_bcv2_authoritative_and_load_ready_rows: `{materialization['working_builder_bcv2_authoritative_and_load_ready_rows']}`",
            f"- working_builder_bcv2_authoritative_but_missing_on_rpc_rows: `{materialization['working_builder_bcv2_authoritative_but_missing_on_rpc_rows']}`",
            f"- working_builder_bcv2_pubkey_mismatch_rows: `{materialization['working_builder_bcv2_pubkey_mismatch_rows']}`",
            f"- working_builder_bcv2_observed_tx_missing_on_rpc_rows: `{materialization['working_builder_bcv2_observed_tx_missing_on_rpc_rows']}`",
            f"- working_builder_bcv2_account_state_missing_rows: `{materialization['working_builder_bcv2_account_state_missing_rows']}`",
            f"- working_builder_creator_vault_authoritative_and_load_ready_rows: `{materialization['working_builder_creator_vault_authoritative_and_load_ready_rows']}`",
            f"- working_builder_creator_vault_authoritative_but_missing_on_rpc_rows: `{materialization['working_builder_creator_vault_authoritative_but_missing_on_rpc_rows']}`",
            f"- working_builder_creator_vault_source_mismatch_rows: `{materialization['working_builder_creator_vault_source_mismatch_rows']}`",
            f"- working_builder_manifest_ready_after_account_source_repair_rows: `{materialization['working_builder_manifest_ready_after_account_source_repair_rows']}`",
            f"- working_builder_manifest_still_not_ready_after_account_source_repair_rows: `{materialization['working_builder_manifest_still_not_ready_after_account_source_repair_rows']}`",
            f"- legacy_buy_route_attempted_rows: `{materialization['legacy_buy_route_attempted_rows']}`",
            f"- legacy_buy_route_ready_rows: `{materialization['legacy_buy_route_ready_rows']}`",
            f"- legacy_buy_route_not_ready_rows: `{materialization['legacy_buy_route_not_ready_rows']}`",
            f"- legacy_buy_missing_core_curve_account_rows: `{materialization['legacy_buy_missing_core_curve_account_rows']}`",
            f"- legacy_buy_missing_associated_bonding_curve_rows: `{materialization['legacy_buy_missing_associated_bonding_curve_rows']}`",
            f"- legacy_buy_authoritative_curve_rows: `{materialization['legacy_buy_authoritative_curve_rows']}`",
            f"- legacy_buy_rpc_load_ready_rows: `{materialization['legacy_buy_rpc_load_ready_rows']}`",
            f"- legacy_buy_successful_entry_rows: `{materialization['legacy_buy_successful_entry_rows']}`",
            f"- legacy_buy_account_set_status_counts: `{json.dumps(materialization['legacy_buy_account_set_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_curve_source_counts: `{json.dumps(materialization['legacy_buy_curve_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_curve_authority_status_counts: `{json.dumps(materialization['legacy_buy_curve_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_curve_rpc_load_status_counts: `{json.dumps(materialization['legacy_buy_curve_rpc_load_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_curve_authority_readiness_status_counts: `{json.dumps(materialization['legacy_buy_curve_authority_readiness_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_curve_authoritative_and_load_ready_rows: `{materialization['legacy_buy_curve_authoritative_and_load_ready_rows']}`",
            f"- legacy_buy_curve_load_ready_but_authority_unverified_rows: `{materialization['legacy_buy_curve_load_ready_but_authority_unverified_rows']}`",
            f"- legacy_buy_curve_authoritative_but_not_checked_rows: `{materialization['legacy_buy_curve_authoritative_but_not_checked_rows']}`",
            f"- legacy_buy_curve_derived_matches_account_state_rows: `{materialization['legacy_buy_curve_derived_matches_account_state_rows']}`",
            f"- legacy_buy_curve_derived_mismatch_account_state_rows: `{materialization['legacy_buy_curve_derived_mismatch_account_state_rows']}`",
            f"- legacy_buy_route_ready_after_reconciliation_rows: `{materialization['legacy_buy_route_ready_after_reconciliation_rows']}`",
            f"- legacy_buy_route_still_not_ready_after_reconciliation_rows: `{materialization['legacy_buy_route_still_not_ready_after_reconciliation_rows']}`",
            f"- legacy_buy_route_not_ready_reason_counts: `{json.dumps(materialization['legacy_buy_route_not_ready_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- legacy_buy_primary_bcv2_leak_rows: `{materialization['legacy_buy_primary_bcv2_leak_rows']}`",
            f"- legacy_buy_missing_creatable_user_ata_rows: `{materialization['legacy_buy_missing_creatable_user_ata_rows']}`",
            f"- legacy_buy_missing_creatable_user_volume_accumulator_rows: `{materialization['legacy_buy_missing_creatable_user_volume_accumulator_rows']}`",
            f"- legacy_buy_missing_ephemeral_payer_rows: `{materialization['legacy_buy_missing_ephemeral_payer_rows']}`",
            f"- legacy_buy_blocking_missing_required_rows: `{materialization['legacy_buy_blocking_missing_required_rows']}`",
            f"- legacy_buy_non_blocking_missing_creatable_rows: `{materialization['legacy_buy_non_blocking_missing_creatable_rows']}`",
            f"- legacy_buy_non_blocking_ephemeral_payer_rows: `{materialization['legacy_buy_non_blocking_ephemeral_payer_rows']}`",
            f"- legacy_buy_fallback_account_set_ready_rows: `{materialization['legacy_buy_fallback_account_set_ready_rows']}`",
            f"- legacy_buy_route_ready_after_account_set_separation_rows: `{materialization['legacy_buy_route_ready_after_account_set_separation_rows']}`",
            f"- legacy_buy_route_unsupported_builder_layout_rows: `{materialization['legacy_buy_route_unsupported_builder_layout_rows']}`",
            f"- legacy_buy_excluded_from_execution_route_universe_rows: `{materialization['legacy_buy_excluded_from_execution_route_universe_rows']}`",
            f"- legacy_buy_removed_from_fallback_candidates_rows: `{materialization['legacy_buy_removed_from_fallback_candidates_rows']}`",
            f"- selected_fallback_route_ready_rows: `{materialization['selected_fallback_route_ready_rows']}`",
            f"- selected_fallback_route_handoff_applied_rows: `{materialization['selected_fallback_route_handoff_applied_rows']}`",
            f"- selected_fallback_route_handoff_mismatch_rows: `{materialization['selected_fallback_route_handoff_mismatch_rows']}`",
            f"- selected_fallback_route_handoff_not_applied_rows: `{materialization['selected_fallback_route_handoff_not_applied_rows']}`",
            f"- selected_fallback_route_blocked_by_primary_reason_rows: `{materialization['selected_fallback_route_blocked_by_primary_reason_rows']}`",
            f"- legacy_buy_selected_but_primary_bcv2_terminal_rows: `{materialization['legacy_buy_selected_but_primary_bcv2_terminal_rows']}`",
            f"- selected_legacy_handoff_claimed_rows: `{materialization['selected_legacy_handoff_claimed_rows']}`",
            f"- selected_legacy_handoff_validated_rows: `{materialization['selected_legacy_handoff_validated_rows']}`",
            f"- selected_legacy_handoff_mismatch_rows: `{materialization['selected_legacy_handoff_mismatch_rows']}`",
            f"- selected_legacy_final_manifest_contains_bcv2_rows: `{materialization['selected_legacy_final_manifest_contains_bcv2_rows']}`",
            f"- selected_legacy_final_manifest_contains_primary_route_builder_rows: `{materialization['selected_legacy_final_manifest_contains_primary_route_builder_rows']}`",
            f"- selected_legacy_request_variant_not_legacy_rows: `{materialization['selected_legacy_request_variant_not_legacy_rows']}`",
            f"- selected_legacy_precheck_hash_mismatch_rows: `{materialization['selected_legacy_precheck_hash_mismatch_rows']}`",
            f"- selected_legacy_simulation_hash_mismatch_rows: `{materialization['selected_legacy_simulation_hash_mismatch_rows']}`",
            f"- no_executable_route_but_simulated_rows: `{materialization['no_executable_route_but_simulated_rows']}`",
            f"- legacy_buy_selected_but_request_variant_not_legacy_rows: `{materialization['legacy_buy_selected_but_request_variant_not_legacy_rows']}`",
            f"- legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows: `{materialization['legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows']}`",
            f"- legacy_buy_selected_but_precheck_hash_mismatch_rows: `{materialization['legacy_buy_selected_but_precheck_hash_mismatch_rows']}`",
            f"- legacy_buy_selected_but_simulation_hash_mismatch_rows: `{materialization['legacy_buy_selected_but_simulation_hash_mismatch_rows']}`",
            f"- legacy_buy_selected_and_precheck_uses_legacy_account_set_rows: `{materialization['legacy_buy_selected_and_precheck_uses_legacy_account_set_rows']}`",
            f"- legacy_buy_selected_and_simulation_uses_legacy_account_set_rows: `{materialization['legacy_buy_selected_and_simulation_uses_legacy_account_set_rows']}`",
            f"- fallback_failure_class_counts: `{json.dumps(materialization['fallback_failure_class_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- fallback_missing_role_counts: `{json.dumps(materialization['fallback_missing_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- fallback_account_source_counts: `{json.dumps(materialization['fallback_account_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- fallback_repairable: `{materialization['fallback_repairable']}`",
            f"- recommended_next_path: `{materialization['recommended_next_path']}`",
            f"- executable_route_ready_rows: `{materialization['executable_route_ready_rows']}`",
            f"- probe_selected_rows: `{materialization['probe_selected_rows']}`",
            f"- route_executable_rows: `{materialization['route_executable_rows']}`",
            f"- route_non_executable_rows: `{materialization['route_non_executable_rows']}`",
            f"- execution_feasibility_reject_rows: `{materialization['execution_feasibility_reject_rows']}`",
            f"- execution_feasibility_status_counts: `{json.dumps(materialization['execution_feasibility_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- execution_feasibility_reason_counts: `{json.dumps(materialization['execution_feasibility_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- lifecycle_label_eligibility_counts: `{json.dumps(materialization['lifecycle_label_eligibility_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- lifecycle_labeled_rows: `{materialization['lifecycle_labeled_rows']}`",
            f"- skip_reason_counts: `{json.dumps(materialization['skip_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_execution_account_readiness_role_counts: `{json.dumps(materialization['skip_execution_account_readiness_role_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_vault_authority_status_counts: `{json.dumps(materialization['skip_creator_vault_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_vault_mismatch_reason_counts: `{json.dumps(materialization['skip_creator_vault_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_identity_source_counts: `{json.dumps(materialization['skip_creator_identity_source_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_bonding_curve_v2_authority_status_counts: `{json.dumps(materialization['skip_bonding_curve_v2_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_bonding_curve_v2_mismatch_reason_counts: `{json.dumps(materialization['skip_bonding_curve_v2_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_bonding_curve_v2_source_counts: `{json.dumps(materialization['skip_bonding_curve_v2_source_counts'], ensure_ascii=False, sort_keys=True)}`",
        ]
    )
    if materialization["rows"]:
        lines.extend(
            [
                "",
                "| probe_id | status | reason | buy_variant | token_param_role | entry_token_amount_raw | min_tokens_out |",
                "| --- | --- | --- | --- | --- | ---: | ---: |",
            ]
        )
        for row in materialization["rows"]:
            lines.append(
                f"| `{row.get('probe_id')}` | `{row.get('probe_entry_materialization_status')}` | "
                f"`{row.get('probe_entry_materialization_reason')}` | `{row.get('buy_variant')}` | "
                f"`{row.get('token_param_role')}` | `{row.get('entry_token_amount_raw')}` | "
                f"`{row.get('min_tokens_out')}` |"
            )
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
