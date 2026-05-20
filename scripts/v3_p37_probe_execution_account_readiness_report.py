#!/usr/bin/env python3
"""Report P3.7-J3 probe execution-account readiness.

This audit is intentionally offline and read-only.  It correlates selected
counterfactual probes with their source V3/MFS decision rows and the
required-account precheck failures emitted by the probe runtime.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

from shadow_run_report import load_toml, resolve_runtime_path


SCHEMA_VERSION = 1
DECISION_FILE_NAMES = ("gatekeeper_v2_decisions.jsonl", "gatekeeper_v2_buys.jsonl")
STRICT_EXECUTION_ROLES = {
    "bonding_curve_v2",
    "creator_vault",
    "bonding_curve",
    "associated_bonding_curve",
    "global_config",
    "fee_recipient",
    "global_volume_accumulator",
    "fee_config",
    "fee_program",
    "buyback_fee_recipient",
    "mint",
    "token_program",
}
BUILDER_DERIVED_ROLES = {
    "bonding_curve": "DirectBuyBuilder PDA: [bonding-curve, mint]",
    "bonding_curve_v2": "DirectBuyBuilder PDA: [bonding-curve-v2, mint]",
    "creator_vault": "DirectBuyBuilder PDA: [creator-vault, creator_pubkey]",
    "global_volume_accumulator": "DirectBuyBuilder PDA: [global-volume-accumulator]",
    "user_volume_accumulator": "DirectBuyBuilder PDA: [user-volume-accumulator, payer]",
    "fee_config": "DirectBuyBuilder PDA: [fee-config, fee-seed]",
    "buyback_fee_recipient": "DirectBuyBuilder routed recipient: [payer, mint]",
}
MFS_ROLE_PATHS = {
    "bonding_curve_v2": (),
    "creator_vault": (),
    "bonding_curve": (),
    "associated_bonding_curve": (),
}
ROUTE_PRECHECK_REASONS = {
    "missing_execution_route_identity": (
        "missing_execution_route_identity",
        "Derived account overrides do not carry a decision-time buy route identity",
    ),
    "missing_routed_associated_bonding_curve": (
        "missing_routed_associated_bonding_curve",
        "Routed exact-SOL-in probe lacks the associated bonding curve identity",
    ),
    "missing_creator_pubkey": (
        "missing_creator_pubkey",
        "Probe cannot derive creator-vault-dependent routed accounts without creator_pubkey",
    ),
    "missing_bonding_curve": (
        "missing_legacy_bonding_curve",
        "Legacy buy route lacks the legacy buy curve identity",
    ),
}


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


def parse_missing_required_account(reason: str | None) -> tuple[str | None, str | None]:
    if not reason:
        return None, None
    prefixes = ("missing_required_account:", "execution_account_not_ready:")
    prefix = next((candidate for candidate in prefixes if reason.startswith(candidate)), None)
    if prefix is None:
        return None, None
    tail = reason[len(prefix) :]
    parts = tail.split(":", 1)
    if len(parts) != 2:
        return None, None
    role, pubkey = parts
    return role or None, pubkey or None


def recursive_contains_value(value: Any, needle: str) -> bool:
    if value is None:
        return False
    if isinstance(value, str):
        return value == needle
    if isinstance(value, dict):
        return any(recursive_contains_value(v, needle) for v in value.values())
    if isinstance(value, list):
        return any(recursive_contains_value(v, needle) for v in value)
    return False


def recursive_contains_key(value: Any, key: str) -> bool:
    if isinstance(value, dict):
        return key in value or any(recursive_contains_key(v, key) for v in value.values())
    if isinstance(value, list):
        return any(recursive_contains_key(v, key) for v in value)
    return False


def flatten_decision_logs(decision_root: Path, explicit_logs: list[Path]) -> list[tuple[Path, int, dict[str, Any]]]:
    paths = explicit_logs or [
        path
        for name in DECISION_FILE_NAMES
        for path in sorted(decision_root.rglob(name))
    ]
    rows: list[tuple[Path, int, dict[str, Any]]] = []
    for path in paths:
        for index, row in enumerate(iter_jsonl(path)):
            rows.append((path, index, row))
    return rows


def decision_lookup(
    decisions: list[tuple[Path, int, dict[str, Any]]],
    selection: dict[str, Any],
) -> tuple[Path | None, int | None, dict[str, Any] | None, dict[str, Any]]:
    ab_record_id = selection.get("ab_record_id") or selection.get("source_ab_record_id")
    feature_hash = selection.get("source_v3_feature_snapshot_hash") or selection.get(
        "v3_feature_snapshot_hash"
    )
    policy_hash = selection.get("source_v3_policy_config_hash") or selection.get(
        "v3_policy_config_hash"
    )
    candidates = [
        (path, index, row)
        for path, index, row in decisions
        if row.get("ab_record_id") == ab_record_id
    ]
    exact = [
        (path, index, row)
        for path, index, row in candidates
        if (not feature_hash or row.get("v3_feature_snapshot_hash") == feature_hash)
        and (not policy_hash or row.get("v3_policy_config_hash") == policy_hash)
    ]
    chosen = exact[0] if exact else (candidates[0] if candidates else (None, None, None))
    diagnostics = {
        "candidate_decision_rows_for_ab_record_id": len(candidates),
        "exact_decision_v3_rows": len(exact),
        "feature_hash_match": bool(exact),
        "policy_hash_match": bool(exact),
        "decision_lookup_status": "exact" if exact else ("ab_only" if candidates else "missing"),
    }
    return (*chosen, diagnostics)


def scan_logs(log_paths: list[Path], pubkey: str | None) -> dict[str, Any]:
    if not pubkey:
        return {
            "raw_log_occurrences": 0,
            "diag_account_update_occurrences": 0,
            "first_raw_log_context": None,
        }
    raw_count = 0
    diag_count = 0
    first_context = None
    for path in log_paths:
        if not path.exists() or not path.is_file():
            continue
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                if pubkey not in line:
                    continue
                raw_count += 1
                if first_context is None:
                    first_context = f"{path.name}: {line.strip()[:360]}"
                if "DIAG_ACCOUNT_UPDATE_RELAY" in line:
                    diag_count += 1
    return {
        "raw_log_occurrences": raw_count,
        "diag_account_update_occurrences": diag_count,
        "first_raw_log_context": first_context,
    }


def role_source(role: str | None) -> str:
    if not role:
        return "unknown"
    if role in BUILDER_DERIVED_ROLES:
        return BUILDER_DERIVED_ROLES[role]
    if role in {"payer_pubkey", "user_ata"}:
        return "Trigger prepared request account"
    return "PreparedBuyRequest transaction account set"


def classify_missing_account(
    role: str | None,
    missing_pubkey: str | None,
    decision_row: dict[str, Any] | None,
    log_scan: dict[str, Any],
    reason: str | None = None,
) -> tuple[str, list[str], str]:
    if reason in ROUTE_PRECHECK_REASONS:
        classification, basis = ROUTE_PRECHECK_REASONS[reason]
        return (
            classification,
            [reason],
            basis,
        )
    if not role or not missing_pubkey:
        return "unknown", ["missing_required_account_reason_absent"], "No missing account reason"
    if role not in STRICT_EXECUTION_ROLES:
        return (
            "unknown",
            [f"role_not_classified_as_strict_execution:{role}"],
            "Role requires a dedicated semantic decision before classification",
        )

    reasons: list[str] = []
    snapshot = (decision_row or {}).get("v3_materialized_feature_snapshot") or {}
    in_snapshot_value = recursive_contains_value(snapshot, missing_pubkey)
    role_key_present = recursive_contains_key(snapshot, role)
    if not in_snapshot_value and not role_key_present:
        reasons.append(f"not_materialized_in_v3_mfs:{role}")

    if decision_row is not None:
        account_features = snapshot.get("account_features") or {}
        curve_readiness = snapshot.get("curve_readiness") or {}
        if account_features.get("update_count", 0) == 0:
            reasons.append("account_features_update_count_zero")
        if not curve_readiness.get("curve_data_known", (decision_row or {}).get("curve_data_known")):
            reasons.append("curve_data_unknown")
        if role == "creator_vault" and not (decision_row.get("dev_pubkey") or "").strip():
            reasons.append("creator_pubkey_missing_in_decision_row")

    if log_scan.get("diag_account_update_occurrences", 0) == 0:
        reasons.append("no_diag_account_update_for_required_pubkey")

    if reason and reason.startswith("execution_account_not_ready:"):
        return (
            "execution_account_not_ready",
            reasons,
            "Runtime classified the strict execution account as unavailable before probe dispatch",
        )
    if role in BUILDER_DERIVED_ROLES and log_scan.get("diag_account_update_occurrences", 0) == 0:
        return (
            "override_present_but_account_missing_on_rpc",
            reasons,
            "Runtime built the account into the prepared transaction, but precheck/RPC did not find the account",
        )
    if "not_materialized_in_v3_mfs:" + role in reasons:
        return (
            "not_materialized",
            reasons,
            "The decision snapshot does not carry this execution-account identity",
        )
    return (
        "unknown",
        reasons or ["no_specific_failure_reason"],
        "The existing artifacts are insufficient for a stricter classification",
    )


def extract_decision_fields(row: dict[str, Any] | None) -> dict[str, Any]:
    if not row:
        return {}
    snapshot = row.get("v3_materialized_feature_snapshot") or {}
    account_features = snapshot.get("account_features") or {}
    curve_readiness = snapshot.get("curve_readiness") or {}
    evidence_status = snapshot.get("evidence_status") or {}
    return {
        "decision_plane": row.get("decision_plane"),
        "decision_verdict_buy": row.get("decision_verdict_buy"),
        "verdict_type": row.get("verdict_type"),
        "reason_code": row.get("reason_code"),
        "dev_pubkey": row.get("dev_pubkey"),
        "curve_data_known_top_level": row.get("curve_data_known"),
        "curve_finality_top_level": row.get("curve_finality"),
        "mfs_present": bool(snapshot),
        "account_features_update_count": account_features.get("update_count"),
        "account_features_state_phase": account_features.get("state_phase"),
        "mfs_curve_finality": account_features.get("curve_finality"),
        "curve_readiness_is_ready": curve_readiness.get("is_ready"),
        "curve_readiness_curve_data_known": curve_readiness.get("curve_data_known"),
        "curve_readiness_freshness": curve_readiness.get("freshness"),
        "curve_readiness_finality": curve_readiness.get("finality"),
        "curve_readiness_wait_elapsed_ms": curve_readiness.get("wait_elapsed_ms"),
        "evidence_account_state_status": (evidence_status.get("account_state") or {}).get("status"),
        "evidence_curve_status": (evidence_status.get("curve") or {}).get("status"),
        "has_bonding_curve_v2_field_in_mfs": recursive_contains_key(snapshot, "bonding_curve_v2"),
        "has_creator_vault_field_in_mfs": recursive_contains_key(snapshot, "creator_vault"),
    }


def selected_probe_report(
    selection: dict[str, Any],
    skip_by_probe_id: dict[str, dict[str, Any]],
    decisions: list[tuple[Path, int, dict[str, Any]]],
    log_paths: list[Path],
) -> dict[str, Any]:
    probe_id = selection.get("probe_id")
    skip = skip_by_probe_id.get(str(probe_id), {})
    if not skip and selection.get("event_type") == "probe_skipped":
        skip = selection
    role, missing_pubkey = parse_missing_required_account(skip.get("precheck_failure_reason"))
    decision_path, decision_index, decision_row, join_diag = decision_lookup(decisions, selection)
    log_scan = scan_logs(log_paths, missing_pubkey)
    precheck_failure_reason = skip.get("precheck_failure_reason")
    classification, reasons, recommendation_basis = classify_missing_account(
        role,
        missing_pubkey,
        decision_row,
        log_scan,
        precheck_failure_reason,
    )
    snapshot = (decision_row or {}).get("v3_materialized_feature_snapshot") or {}
    return {
        "ab_record_id": selection.get("ab_record_id"),
        "probe_id": probe_id,
        "probe_bucket": selection.get("probe_bucket"),
        "active_verdict_type": selection.get("active_verdict_type"),
        "v3_shadow_verdict": selection.get("v3_shadow_verdict"),
        "v3_shadow_reason_code": selection.get("v3_shadow_reason_code"),
        "pool_id": selection.get("pool_id"),
        "base_mint": selection.get("base_mint") or selection.get("mint_id"),
        "v3_feature_snapshot_hash": selection.get("source_v3_feature_snapshot_hash")
        or selection.get("v3_feature_snapshot_hash"),
        "v3_policy_config_hash": selection.get("source_v3_policy_config_hash")
        or selection.get("v3_policy_config_hash"),
        "probe_skip_reason": skip.get("probe_skip_reason"),
        "source_probe_event_type": selection.get("event_type"),
        "precheck_failure_reason": precheck_failure_reason,
        "execution_account_readiness_status": skip.get("execution_account_readiness_status"),
        "execution_account_readiness_role": skip.get("execution_account_readiness_role"),
        "execution_account_readiness_pubkey": skip.get("execution_account_readiness_pubkey"),
        "execution_account_readiness_reason": skip.get("execution_account_readiness_reason"),
        "missing_account_role": role,
        "missing_account_pubkey": missing_pubkey,
        "missing_account_source": role_source(role),
        "missing_account_classification": classification,
        "classification_reasons": reasons,
        "recommendation_basis": recommendation_basis,
        "present_in_v3_mfs_as_value": recursive_contains_value(snapshot, missing_pubkey or ""),
        "role_field_present_in_v3_mfs": recursive_contains_key(snapshot, role or ""),
        "present_in_prepared_request_account_set": bool(role and missing_pubkey),
        "present_in_account_overrides": role
        in {"global_config", "fee_recipient", "creator_pubkey", "associated_bonding_curve"},
        "account_update_observed_for_required_pubkey": log_scan.get(
            "diag_account_update_occurrences", 0
        )
        > 0,
        "required_pubkey_raw_log_occurrences": log_scan.get("raw_log_occurrences", 0),
        "required_pubkey_diag_account_update_occurrences": log_scan.get(
            "diag_account_update_occurrences", 0
        ),
        "first_raw_log_context": log_scan.get("first_raw_log_context"),
        "decision_log_path": str(decision_path) if decision_path else None,
        "decision_row_index": decision_index,
        "decision_join": join_diag,
        "decision_fields": extract_decision_fields(decision_row),
    }


def render_markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    namespace = payload.get("probe_namespace") or "unknown"
    lines = [
        "# RAPORT P3.7-J3 Probe Execution-Account Readiness",
        "",
        f"Date: {payload['date']}",
        f"Namespace: `{namespace}`",
        "",
        "Status:",
        "",
        "```text",
        f"P3.7-J3 execution-account readiness audit: {summary['status']}",
        "runtime smoke status must be read from the paired smoke/join-key report",
        "Full / bounded collection: HOLD",
        "Phase B / P2 / live / tuning: NO-GO",
        "```",
        "",
        "## Inputs",
        "",
        f"- config: `{payload['config_path']}`",
        f"- probe_selection: `{payload['probe_selection_path']}`",
        f"- probe_skips: `{payload['probe_skips_path']}`",
        f"- decision_root: `{payload['decision_root']}`",
        "",
        "## Summary",
        "",
        "```text",
        f"selected_probe_rows = {summary['selected_probe_rows']}",
        f"pre_scan_precheck_skip_rows = {summary['pre_scan_precheck_skip_rows']}",
        f"audited_probe_rows = {summary['audited_probe_rows']}",
        f"diagnosed_selected_probe_rows = {summary['diagnosed_selected_probe_rows']}",
        f"exact_decision_v3_join_rows = {summary['exact_decision_v3_join_rows']}",
        f"missing_account_roles = {summary['missing_account_roles']}",
        f"classifications = {summary['classifications']}",
        "```",
        "",
        "## Per-Probe Diagnosis",
        "",
        "| probe | role | classification | pubkey | decision join | account updates | reason |",
        "| --- | --- | --- | --- | --- | ---: | --- |",
    ]
    for row in payload["selected_probe_diagnostics"]:
        join_status = row["decision_join"].get("decision_lookup_status")
        role = row.get("missing_account_role") or "none"
        pubkey = row.get("missing_account_pubkey") or "none"
        probe = str(row.get("probe_id") or "")[:10]
        updates = row.get("required_pubkey_diag_account_update_occurrences", 0)
        reason = row.get("precheck_failure_reason") or "none"
        lines.append(
            f"| `{probe}` | `{role}` | `{row['missing_account_classification']}` | "
            f"`{pubkey}` | `{join_status}` | {updates} | `{reason}` |"
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "This report is an offline probe-readiness audit. It classifies selected",
            "counterfactual probes and pre-scan skips by exact decision/V3 join status,",
            "required-account role, and explicit precheck reason.",
            "",
            "Rows classified as `unknown` in this report are selected probes that were",
            "not stopped by execution-account precheck. They must be interpreted with",
            "the paired probe transport/entry and simulation-error reports.",
            "",
            "## Decision",
            "",
            "Do not bypass required-account precheck. Do not use this report alone to",
            "start collection.",
            "",
            "If `execution_account_not_ready` dominates and no probe transport/entry rows",
            "exist, the next step is account-readiness/materialization work. If transport",
            "and entry rows exist, classify any simulation errors before scaling.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--probe-selection")
    parser.add_argument("--probe-skips")
    parser.add_argument("--decision-log", action="append", default=[])
    parser.add_argument("--output-json", required=True)
    parser.add_argument("--output-md", required=True)
    args = parser.parse_args()

    config_path = Path(args.config).resolve()
    repo_root = Path.cwd().resolve()
    config = load_toml(config_path)
    probe_cfg = config.get("p37_shadow_probe") or {}
    oracle_cfg = config.get("oracle") or {}
    logging_cfg = config.get("logging") or {}

    selection_path = (
        Path(args.probe_selection)
        if args.probe_selection
        else resolve_runtime_path(config_path, probe_cfg.get("selection_log_path"))
    )
    skips_path = (
        Path(args.probe_skips)
        if args.probe_skips
        else resolve_runtime_path(config_path, probe_cfg.get("skip_log_path"))
    )
    decision_root = resolve_runtime_path(config_path, oracle_cfg.get("decision_log_path"))
    explicit_decision_logs = [Path(path) for path in args.decision_log]
    decisions = flatten_decision_logs(decision_root, explicit_decision_logs)
    selections = [row for row in iter_jsonl(selection_path) if row.get("event_type") == "probe_selected"]
    skips = list(iter_jsonl(skips_path))
    skip_by_probe_id = {
        str(row.get("probe_id")): row
        for row in skips
        if row.get("probe_id") is not None
    }
    selection_probe_ids = {str(row.get("probe_id")) for row in selections if row.get("probe_id")}
    pre_scan_precheck_skips = [
        row
        for row in skips
        if row.get("probe_id") is not None
        and str(row.get("probe_id")) not in selection_probe_ids
        and row.get("probe_skip_reason") == "probe_execution_precheck_failed"
    ]

    log_paths = []
    for key in ("file_path", "oracle_log_path"):
        raw = logging_cfg.get(key)
        if raw:
            base = resolve_runtime_path(config_path, raw)
            log_paths.extend(sorted(base.parent.glob(base.name + "*")))

    audited_rows = selections + pre_scan_precheck_skips
    diagnostics = [
        selected_probe_report(selection, skip_by_probe_id, decisions, log_paths)
        for selection in audited_rows
    ]
    classifications = Counter(row["missing_account_classification"] for row in diagnostics)
    roles = Counter(row.get("missing_account_role") or "none" for row in diagnostics)
    exact_join_rows = sum(
        1
        for row in diagnostics
        if row["decision_join"].get("decision_lookup_status") == "exact"
    )
    diagnosed_rows = sum(1 for row in diagnostics if row.get("missing_account_role"))
    payload = {
        "schema_version": SCHEMA_VERSION,
        "date": "2026-05-20",
        "config_path": str(config_path),
        "probe_namespace": probe_cfg.get("namespace"),
        "probe_selection_path": str(selection_path),
        "probe_skips_path": str(skips_path),
        "decision_root": str(decision_root),
        "summary": {
            "status": "PASS",
            "selected_probe_rows": len(selections),
            "pre_scan_precheck_skip_rows": len(pre_scan_precheck_skips),
            "audited_probe_rows": len(audited_rows),
            "diagnosed_selected_probe_rows": diagnosed_rows,
            "exact_decision_v3_join_rows": exact_join_rows,
            "missing_account_roles": dict(roles),
            "classifications": dict(classifications),
            "decision_logs_scanned": len({str(path) for path, _, _ in decisions}),
            "decision_rows_scanned": len(decisions),
            "log_files_scanned": [str(path) for path in log_paths],
            "recommended_next_stage": "read paired smoke and simulation-error report",
            "collection_gate": "HOLD",
        },
        "selected_probe_diagnostics": diagnostics,
    }
    output_json = Path(args.output_json)
    output_md = Path(args.output_md)
    write_json(output_json, payload)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_md.write_text(render_markdown(payload), encoding="utf-8")


if __name__ == "__main__":
    main()
