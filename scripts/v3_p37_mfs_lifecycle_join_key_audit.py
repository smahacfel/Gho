#!/usr/bin/env python3
"""Audit join-key coverage for a P3.7 V3/MFS + lifecycle collection run."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_config_path, resolve_runtime_path


SCHEMA_VERSION = 4
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
    "execution_account_readiness_status": ("execution_account_readiness_status",),
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
    "execution_account_readiness_status",
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


def classify_probe_transport_materialization(row: dict[str, Any], entry_probe_ids: set[str]) -> tuple[str, str]:
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
    transport_rows = artifact_rows(paths, "probe_transport")
    entry_rows = artifact_rows(paths, "probe_entry")
    skip_rows = artifact_rows(paths, "probe_skip")
    entry_probe_ids = {probe_id for row in entry_rows if (probe_id := row_probe_id(row))}

    status_counts: Counter[str] = Counter()
    reason_counts: Counter[str] = Counter()
    buy_variant_counts: Counter[str] = Counter()
    token_param_role_counts: Counter[str] = Counter()
    creator_vault_authority_status_counts: Counter[str] = Counter()
    creator_vault_mismatch_reason_counts: Counter[str] = Counter()
    creator_identity_source_counts: Counter[str] = Counter()
    amount_guard_status_counts: Counter[str] = Counter()
    simulation_error_custom_code_counts: Counter[str] = Counter()
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
        amount_guard_status = row_string(row, "amount_guard_status")
        simulation_error_custom_code = row_string(row, "simulation_error_custom_code")
        if creator_vault_authority_status:
            creator_vault_authority_status_counts[creator_vault_authority_status] += 1
        if creator_vault_mismatch_reason:
            creator_vault_mismatch_reason_counts[creator_vault_mismatch_reason] += 1
        if creator_identity_source:
            creator_identity_source_counts[creator_identity_source] += 1
        if amount_guard_status:
            amount_guard_status_counts[amount_guard_status] += 1
        if simulation_error_custom_code:
            simulation_error_custom_code_counts[f"custom_{simulation_error_custom_code}"] += 1
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
                "simulation_error_category": row_string(row, "simulation_error_category"),
                "simulation_error_custom_code": simulation_error_custom_code,
                "simulation_error_account_role": row_string(row, "simulation_error_account_role"),
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
                "probe_entry_materialization_status": status,
                "probe_entry_materialization_reason": reason,
            }
        )

    skip_reason_counts: Counter[str] = Counter()
    skip_creator_vault_authority_status_counts: Counter[str] = Counter()
    skip_creator_vault_mismatch_reason_counts: Counter[str] = Counter()
    skip_creator_identity_source_counts: Counter[str] = Counter()
    for row in skip_rows:
        reason = row_string(row, "probe_skip_reason") or row_string(row, "skip_reason")
        if reason:
            skip_reason_counts[reason] += 1
        creator_vault_authority_status = row_string(row, "creator_vault_authority_status")
        creator_vault_mismatch_reason = row_string(row, "creator_vault_mismatch_reason")
        creator_identity_source = row_string(row, "creator_identity_source")
        if creator_vault_authority_status:
            skip_creator_vault_authority_status_counts[creator_vault_authority_status] += 1
        if creator_vault_mismatch_reason:
            skip_creator_vault_mismatch_reason_counts[creator_vault_mismatch_reason] += 1
        if creator_identity_source:
            skip_creator_identity_source_counts[creator_identity_source] += 1

    transport_rows_total = len(transport_rows)
    entry_rows_total = len(entry_rows)
    return {
        "transport_rows": transport_rows_total,
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
        "amount_guard_status_counts": dict(sorted(amount_guard_status_counts.items())),
        "simulation_error_custom_code_counts": dict(
            sorted(simulation_error_custom_code_counts.items())
        ),
        "skip_reason_counts": dict(sorted(skip_reason_counts.items())),
        "skip_creator_vault_authority_status_counts": dict(
            sorted(skip_creator_vault_authority_status_counts.items())
        ),
        "skip_creator_vault_mismatch_reason_counts": dict(
            sorted(skip_creator_vault_mismatch_reason_counts.items())
        ),
        "skip_creator_identity_source_counts": dict(
            sorted(skip_creator_identity_source_counts.items())
        ),
        "entry_materialized_rows": status_counts.get("entry_materialized", 0),
        "transport_only_missing_token_quantity_rows": status_counts.get(
            "transport_only_missing_token_quantity",
            0,
        ),
        "simulation_error_rows": status_counts.get("simulation_error", 0),
        "execution_account_not_ready_rows": status_counts.get("execution_account_not_ready", 0),
        "unknown_rows": status_counts.get("unknown", 0),
        "rows": rows,
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
    }


def probe_readiness(report: dict[str, Any]) -> dict[str, Any]:
    coverage = report.get("probe_join_key_coverage", {})
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
    report["join_key_coverage"] = join_key_coverage(report)
    report["readiness"] = readiness(report)
    report["probe_join_key_coverage"] = probe_join_key_coverage(report)
    report["probe_decision_join"] = probe_decision_join(paths)
    report["probe_entry_materialization"] = probe_entry_materialization(paths)
    report["probe_readiness"] = probe_readiness(report)
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
            f"- amount_guard_status_counts: `{json.dumps(materialization['amount_guard_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- simulation_error_custom_code_counts: `{json.dumps(materialization['simulation_error_custom_code_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_reason_counts: `{json.dumps(materialization['skip_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_vault_authority_status_counts: `{json.dumps(materialization['skip_creator_vault_authority_status_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_vault_mismatch_reason_counts: `{json.dumps(materialization['skip_creator_vault_mismatch_reason_counts'], ensure_ascii=False, sort_keys=True)}`",
            f"- skip_creator_identity_source_counts: `{json.dumps(materialization['skip_creator_identity_source_counts'], ensure_ascii=False, sort_keys=True)}`",
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
