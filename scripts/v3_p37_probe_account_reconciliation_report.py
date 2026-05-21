#!/usr/bin/env python3
"""Reconcile P3.7-J3 probe account truth across DIAG/MFS/route/RPC.

This report is read-only.  It explains why a counterfactual probe candidate can
have local account truth in DIAG logs while the probe path still skips before
transport/entry.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_runtime_path
from v3_p37_probe_execution_account_readiness_report import (
    build_account_update_index,
    decision_lookup,
    flatten_decision_logs,
    infer_expected_account,
    iter_jsonl,
    lookup_diag_account_updates,
    readiness_latency,
    recursive_contains_key,
    recursive_contains_value,
)


SCHEMA_VERSION = 1


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def parse_missing_required_account(reason: str | None) -> tuple[str | None, str | None]:
    if not reason:
        return None, None
    for prefix in ("missing_required_account:", "execution_account_not_ready:"):
        if reason.startswith(prefix):
            tail = reason[len(prefix) :]
            parts = tail.split(":", 1)
            if len(parts) == 2:
                return parts[0] or None, parts[1] or None
    return None, None


def mfs_route_account_summary(decision_row: dict[str, Any] | None, expected_pubkey: str | None) -> dict[str, Any]:
    snapshot = (decision_row or {}).get("v3_materialized_feature_snapshot") or {}
    account_features = snapshot.get("account_features") or {}
    curve_readiness = snapshot.get("curve_readiness") or {}
    evidence_status = snapshot.get("evidence_status") or {}
    expected_value_present = (
        recursive_contains_value(snapshot, expected_pubkey) if expected_pubkey else False
    )
    return {
        "mfs_present": bool(snapshot),
        "mfs_expected_pubkey_present_as_value": expected_value_present,
        "mfs_has_bonding_curve_field": recursive_contains_key(snapshot, "bonding_curve"),
        "mfs_has_bonding_curve_v2_field": recursive_contains_key(snapshot, "bonding_curve_v2"),
        "mfs_has_creator_vault_field": recursive_contains_key(snapshot, "creator_vault"),
        "mfs_has_associated_bonding_curve_field": recursive_contains_key(
            snapshot,
            "associated_bonding_curve",
        ),
        "mfs_has_buy_variant_field": recursive_contains_key(snapshot, "buy_variant"),
        "mfs_has_route_kind_field": recursive_contains_key(snapshot, "route_kind"),
        "mfs_account_features_update_count": account_features.get("update_count"),
        "mfs_account_features_curve_finality": account_features.get("curve_finality"),
        "mfs_account_features_state_phase": account_features.get("state_phase"),
        "mfs_curve_readiness_is_ready": curve_readiness.get("is_ready"),
        "mfs_curve_readiness_curve_data_known": curve_readiness.get("curve_data_known"),
        "mfs_curve_readiness_freshness": curve_readiness.get("freshness"),
        "mfs_curve_readiness_finality": curve_readiness.get("finality"),
        "mfs_evidence_account_state_status": (evidence_status.get("account_state") or {}).get(
            "status"
        ),
        "mfs_evidence_curve_status": (evidence_status.get("curve") or {}).get("status"),
    }


def classify_reconciliation(row: dict[str, Any]) -> tuple[str, str, str]:
    reason = row["precheck_failure_reason"]
    diag_seen = row["diag_seen"]
    mfs = row["mfs"]
    prepared_status = row["prepared_request_status"]
    rpc_status = row["rpc_precheck_status"]

    if reason == "missing_bonding_curve":
        if diag_seen and prepared_status == "not_built_pre_route_precheck":
            if not mfs["mfs_expected_pubkey_present_as_value"]:
                return (
                    "mfs_has_account_but_overrides_missing",
                    "diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override",
                    "route_override_propagation",
                )
            return (
                "mfs_has_account_but_overrides_missing",
                "diag_seen_before_decision_but_legacy_curve_override_missing",
                "route_override_propagation",
            )
        if not diag_seen:
            return (
                "account_coverage_missing",
                "legacy_curve_not_seen_in_diag_for_candidate",
                "account_coverage_data_plane_repair",
            )
    if reason == "missing_execution_route_identity":
        return (
            "route_mismatch",
            "buy_variant_or_route_identity_missing_before_request_build",
            "route_identity_propagation",
        )
    if reason and reason.startswith(("missing_required_account:", "execution_account_not_ready:")):
        if diag_seen and rpc_status == "rpc_processed_missing":
            return (
                "diag_seen_rpc_missing",
                "local_diag_observed_account_but_rpc_processed_precheck_missing",
                "rpc_visibility_reconciliation",
            )
        if diag_seen:
            return (
                "commitment_or_rpc_visibility_gap",
                "local_diag_observed_account_but_probe_precheck_did_not_accept_it",
                "rpc_visibility_reconciliation",
            )
        return (
            "builder_required_account_not_in_mfs",
            "strict_required_account_missing_without_diag_evidence",
            "execution_account_readiness_materialization",
        )
    return ("unknown", "insufficient_artifact_context", "manual_investigation")


def reconcile_row(
    skip: dict[str, Any],
    decisions: list[tuple[Path, int, dict[str, Any]]],
    account_update_index: dict[str, Any],
    transport_by_probe_id: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    reason = skip.get("precheck_failure_reason")
    parsed_role, parsed_pubkey = parse_missing_required_account(reason)
    expected_role, expected_pubkey, expected_source = infer_expected_account(
        skip,
        reason,
        parsed_role,
        parsed_pubkey,
    )
    decision_path, decision_index, decision_row, join_diag = decision_lookup(decisions, skip)
    diag_records = lookup_diag_account_updates(
        account_update_index,
        skip.get("base_mint") or skip.get("mint_id"),
        expected_pubkey,
    )
    latency = readiness_latency(
        diag_records,
        skip.get("decision_ts_ms"),
        skip.get("probe_selected_ts_ms"),
    )
    transport = transport_by_probe_id.get(str(skip.get("probe_id")))
    prepared_status = "not_built_pre_route_precheck" if not transport else "transport_recorded"
    rpc_status = "not_run_prepared_request_not_built"
    if reason and reason.startswith(("missing_required_account:", "execution_account_not_ready:")):
        rpc_status = "rpc_processed_missing"
    if transport and transport.get("execution_account_readiness_status") == "ready":
        rpc_status = "ready"
    record = {
        "ab_record_id": skip.get("ab_record_id"),
        "probe_id": skip.get("probe_id"),
        "pool_id": skip.get("pool_id"),
        "base_mint": skip.get("base_mint") or skip.get("mint_id"),
        "decision_ts_ms": skip.get("decision_ts_ms"),
        "probe_selected_ts_ms": skip.get("probe_selected_ts_ms"),
        "precheck_failure_reason": reason,
        "missing_role": expected_role,
        "missing_pubkey": expected_pubkey,
        "expected_account_source": expected_source,
        "source_v3_feature_snapshot_hash": skip.get("source_v3_feature_snapshot_hash")
        or skip.get("v3_feature_snapshot_hash"),
        "source_v3_policy_config_hash": skip.get("source_v3_policy_config_hash")
        or skip.get("v3_policy_config_hash"),
        "decision_log_path": str(decision_path) if decision_path else None,
        "decision_row_index": decision_index,
        "decision_join": join_diag,
        "prepared_request_status": prepared_status,
        "prepared_request_pubkey": transport.get("bonding_curve") if transport else None,
        "prepared_request_account_role": expected_role if transport else None,
        "prepared_request_buy_variant": transport.get("buy_variant") if transport else None,
        "prepared_request_route_kind": transport.get("route_kind") if transport else None,
        "prepared_request_required_account_roles": transport.get("required_account_roles")
        if transport
        else None,
        "rpc_precheck_status": rpc_status,
        "rpc_precheck_commitment": "processed" if rpc_status != "not_run_prepared_request_not_built" else None,
        "diag_seen": bool(diag_records),
        "diag_seen_before_decision": latency["ready_before_decision"],
        "diag_seen_before_probe_selected": latency["ready_before_probe_selected"],
        "diag_seen_occurrences": latency["diag_account_update_occurrences"],
        "diag_seen_ts_ms": latency["first_account_update_ts_ms"],
        "diag_seen_slot": diag_records[0].get("slot") if diag_records else None,
        "diag_source": "DIAG_ACCOUNT_UPDATE_RELAY" if diag_records else None,
        "diag_parser_role": "bonding_curve" if diag_records else None,
        "readiness_latency": latency,
        "mfs": mfs_route_account_summary(decision_row, expected_pubkey),
    }
    classification, detail, next_fix = classify_reconciliation(record)
    record["classification"] = classification
    record["classification_detail"] = detail
    record["recommended_fix_path"] = next_fix
    return record


def choose_next_fix(records: list[dict[str, Any]]) -> str:
    fix_counts = Counter(row["recommended_fix_path"] for row in records)
    if not fix_counts:
        return "manual_investigation"
    return fix_counts.most_common(1)[0][0]


def render_markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    lines = [
        "# RAPORT P3.7-J3K2 Account Coverage / Route Identity Reconciliation",
        "",
        f"Date: {payload['date']}",
        f"Namespace: `{payload['probe_namespace']}`",
        "",
        "Status:",
        "",
        "```text",
        f"J3K2 reconciliation: {summary['status']}",
        f"recommended_next_fix_path = {summary['recommended_next_fix_path']}",
        "collection / Phase B / P2 / live / tuning: HOLD / NO-GO",
        "```",
        "",
        "## Summary",
        "",
        "```text",
        f"audited_missing_account_rows = {summary['audited_missing_account_rows']}",
        f"exact_decision_v3_join_rows = {summary['exact_decision_v3_join_rows']}",
        f"classifications = {summary['classifications']}",
        f"recommended_fix_paths = {summary['recommended_fix_paths']}",
        f"diag_seen_before_decision_rows = {summary['diag_seen_before_decision_rows']}",
        f"prepared_request_not_built_rows = {summary['prepared_request_not_built_rows']}",
        "```",
        "",
        "## Interpretation",
        "",
        "Q6-r2 already proved counterfactual probe transport/entry for ready rows.",
        "This report explains the dominant skip class. If `missing_bonding_curve`",
        "rows are seen in DIAG before decision but still skip before request build,",
        "the blocker is route/materialization/override handoff rather than bounded",
        "wait or RPC simulation itself.",
        "",
        "## Reconciliation Rows",
        "",
        "| probe | role | classification | detail | diag before decision | prepared status | fix |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in payload["rows"][:200]:
        lines.append(
            f"| `{str(row.get('probe_id') or '')[:10]}` | `{row.get('missing_role')}` | "
            f"`{row['classification']}` | `{row['classification_detail']}` | "
            f"`{row['diag_seen_before_decision']}` | `{row['prepared_request_status']}` | "
            f"`{row['recommended_fix_path']}` |"
        )
    if len(payload["rows"]) > 200:
        lines.append(f"| ... | ... | ... | ... | ... | ... | remaining rows: {len(payload['rows']) - 200} |")
    lines.extend(
        [
            "",
            "## Decision",
            "",
            "Do not run another blind timeout. Do not scale collection. The next fix",
            "must target the dominant handoff class reported above.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--probe-skips")
    parser.add_argument("--probe-transport")
    parser.add_argument("--decision-log", action="append", default=[])
    parser.add_argument("--output-json", required=True)
    parser.add_argument("--output-md", required=True)
    args = parser.parse_args()

    config_path = Path(args.config).resolve()
    config = load_toml(config_path)
    probe_cfg = config.get("p37_shadow_probe") or {}
    oracle_cfg = config.get("oracle") or {}
    logging_cfg = config.get("logging") or {}
    skips_path = (
        Path(args.probe_skips)
        if args.probe_skips
        else resolve_runtime_path(config_path, probe_cfg.get("skip_log_path"))
    )
    transport_path = (
        Path(args.probe_transport)
        if args.probe_transport
        else resolve_runtime_path(config_path, probe_cfg.get("transport_log_path"))
    )
    decision_root = resolve_runtime_path(config_path, oracle_cfg.get("decision_log_path"))
    decisions = flatten_decision_logs(decision_root, [Path(path) for path in args.decision_log])
    log_paths = []
    for key in ("file_path", "oracle_log_path"):
        raw = logging_cfg.get(key)
        if raw:
            base = resolve_runtime_path(config_path, raw)
            log_paths.extend(sorted(base.parent.glob(base.name + "*")))
    account_update_index = build_account_update_index(log_paths)
    transport_by_probe_id = {
        str(row.get("probe_id")): row
        for row in iter_jsonl(transport_path)
        if row.get("probe_id") is not None
    }
    relevant_reasons = {
        "missing_bonding_curve",
        "missing_execution_route_identity",
        "missing_creator_pubkey",
        "missing_routed_associated_bonding_curve",
    }
    skipped_rows = []
    for row in iter_jsonl(skips_path):
        reason = row.get("precheck_failure_reason")
        if reason in relevant_reasons or (
            isinstance(reason, str)
            and reason.startswith(("missing_required_account:", "execution_account_not_ready:"))
        ):
            skipped_rows.append(row)
    rows = [
        reconcile_row(row, decisions, account_update_index, transport_by_probe_id)
        for row in skipped_rows
    ]
    classifications = Counter(row["classification"] for row in rows)
    fix_paths = Counter(row["recommended_fix_path"] for row in rows)
    payload = {
        "schema_version": SCHEMA_VERSION,
        "date": "2026-05-21",
        "config_path": str(config_path),
        "probe_namespace": probe_cfg.get("namespace"),
        "probe_skips_path": str(skips_path),
        "probe_transport_path": str(transport_path),
        "decision_root": str(decision_root),
        "summary": {
            "status": "PASS",
            "audited_missing_account_rows": len(rows),
            "exact_decision_v3_join_rows": sum(
                1 for row in rows if row["decision_join"].get("decision_lookup_status") == "exact"
            ),
            "classifications": dict(classifications),
            "recommended_fix_paths": dict(fix_paths),
            "recommended_next_fix_path": choose_next_fix(rows),
            "diag_seen_before_decision_rows": sum(
                1 for row in rows if row.get("diag_seen_before_decision")
            ),
            "prepared_request_not_built_rows": sum(
                1
                for row in rows
                if row.get("prepared_request_status") == "not_built_pre_route_precheck"
            ),
            "collection_gate": "HOLD",
        },
        "rows": rows,
    }
    output_json = Path(args.output_json)
    output_md = Path(args.output_md)
    write_json(output_json, payload)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_md.write_text(render_markdown(payload), encoding="utf-8")


if __name__ == "__main__":
    main()
