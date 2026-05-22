#!/usr/bin/env python3
"""Build the P3.7-L1R12 bonding_curve_v2 coverage matrix.

This script is a follow-up to the L1R10 reconciliation report. L1R10 answered
whether the builder-provided `bonding_curve_v2` was seen by DIAG/MFS. L1R12 adds
the route decision question: does the builder `bonding_curve_v2` exist on RPC
as a simulation-load account, or is the route/builder selecting an account that
does not exist?

RPC checks are current preflight evidence, not decision-time policy features.
They are safe here because this script validates shadow/probe execution
feasibility, not Gatekeeper scoring.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import v3_p37_bonding_curve_v2_reconciliation as recon


SCHEMA_VERSION = 1


def row_identity(row: dict[str, Any]) -> str:
    return "|".join(
        str(row.get(key) or "")
        for key in (
            "plane",
            "ab_record_id",
            "builder_bonding_curve_v2_pubkey",
        )
    )


def tx_meta_account_16_pubkey(row: dict[str, Any]) -> str | None:
    if row.get("account_index") == 16:
        pubkey = row.get("builder_bonding_curve_v2_pubkey")
        return pubkey if isinstance(pubkey, str) and pubkey else None
    return None


def owner_layout_status(row: dict[str, Any]) -> str:
    rpc = row.get("rpc_current") or {}
    if rpc.get("rpc_current_status") != "present":
        return "not_applicable"
    owner_expected = row.get("owner_expected")
    owner_actual = rpc.get("rpc_current_owner")
    if owner_expected and owner_actual and owner_expected != owner_actual:
        return "owner_mismatch"
    if owner_expected and owner_actual == owner_expected:
        return "owner_ok_layout_not_checked"
    return "owner_present_layout_not_checked"


def classify_matrix_row(row: dict[str, Any]) -> tuple[str, list[str]]:
    rpc = row.get("rpc_current") or {}
    mfs = row.get("mfs") or {}
    diag = row.get("diag") or {}
    reasons: list[str] = []

    tx_meta_16 = tx_meta_account_16_pubkey(row)
    builder_pubkey = row.get("builder_bonding_curve_v2_pubkey")
    if tx_meta_16 and tx_meta_16 != builder_pubkey:
        reasons.append("tx_meta_account_16_differs_from_builder_bcv2")
        return "tx_meta_builder_bcv2_mismatch", reasons
    if tx_meta_16:
        reasons.append("tx_meta_account_16_matches_builder_bcv2")

    if diag.get("diag_seen_other_curve_pubkey_for_mint"):
        reasons.append("diag_seen_other_curve_for_same_mint")
    if not diag.get("diag_seen_exact_pubkey"):
        reasons.append("diag_did_not_see_exact_builder_bcv2")
    if not mfs.get("mfs_contains_bonding_curve_v2_key"):
        reasons.append("mfs_missing_bonding_curve_v2_identity")
    if not mfs.get("mfs_contains_builder_bcv2_pubkey"):
        reasons.append("mfs_missing_builder_bcv2_pubkey")

    rpc_status = rpc.get("rpc_current_status", "not_checked")
    if rpc_status == "present":
        reasons.append("builder_bcv2_exists_on_rpc")
        owner_status = owner_layout_status(row)
        if owner_status == "owner_mismatch":
            reasons.append("builder_bcv2_owner_mismatch")
            return "builder_bcv2_owner_layout_invalid", reasons
        if not mfs.get("mfs_contains_builder_bcv2_pubkey"):
            return "builder_bcv2_not_materialized_but_rpc_exists", reasons
        if not diag.get("diag_seen_exact_pubkey"):
            reasons.append("diag_absence_can_be_expected_for_readonly_account_meta")
            return "builder_bcv2_not_seen_in_diag_expected_readonly", reasons
        return "builder_bcv2_exists_on_rpc", reasons

    if rpc_status == "missing":
        reasons.append("builder_bcv2_missing_on_rpc")
        return "builder_bcv2_missing_on_rpc", reasons

    if rpc_status == "error":
        reasons.append("rpc_preflight_error")
        return "builder_bcv2_rpc_preflight_error", reasons

    if diag.get("diag_seen_other_curve_pubkey_for_mint"):
        return "builder_bcv2_route_identity_mismatch_or_unchecked_rpc", reasons
    return "builder_bcv2_unknown", reasons or ["no_matrix_evidence"]


def route_decision(rows: list[dict[str, Any]], rpc_checked: bool) -> str:
    classes = Counter(row.get("matrix_classification") for row in rows)
    if not rows:
        return "no_bcv2_blocker_rows_return_to_policy_diagnostics"
    if classes.get("tx_meta_builder_bcv2_mismatch", 0) > 0:
        return "prepared_request_manifest_handoff_repair"
    if classes.get("builder_bcv2_owner_layout_invalid", 0) > 0:
        return "route_builder_derivation_repair"
    if classes.get("builder_bcv2_missing_on_rpc", 0) == len(rows):
        return "route_builder_source_repair_or_route_fallback"
    if classes.get("builder_bcv2_not_materialized_but_rpc_exists", 0) > 0:
        return "rpc_readiness_source_and_mfs_materialization"
    if classes.get("builder_bcv2_exists_on_rpc", 0) > 0:
        return "rpc_readiness_source_fix"
    if classes.get("builder_bcv2_rpc_preflight_error", 0) > 0 or not rpc_checked:
        return "rerun_matrix_with_accessible_rpc_preflight"
    return "route_identity_or_materialization_investigation"


def build_matrix(
    config_path: Path,
    rpc_check_current: bool,
    rpc_url: str | None,
) -> dict[str, Any]:
    config_path = config_path.resolve()
    config = recon.load_toml(config_path)
    namespace = recon.resolve_namespace(config, config_path)
    paths = recon.resolve_paths(config_path, config, namespace)
    log_paths = recon.discover_log_paths(paths["rollout_root"])
    decisions = recon.flatten_decision_logs(paths["decision_root"])
    by_ab, by_mint = recon.build_decision_indexes(decisions)
    diag_index = recon.build_diag_index(log_paths)
    cases = recon.collect_cases(paths)
    pubkeys = sorted({case["builder_bonding_curve_v2_pubkey"] for case in cases})

    rpc_results: dict[str, Any] = {}
    rpc_checked = False
    if rpc_check_current and pubkeys:
        rpc_checked = True
        effective_rpc_url = rpc_url
        if not effective_rpc_url:
            effective_rpc_url = (((config.get("trigger") or {}).get("shadow_run") or {}).get("shadow_rpc_url"))
        if not effective_rpc_url:
            effective_rpc_url = ((config.get("seer") or {}).get("rpc_endpoint"))
        if not isinstance(effective_rpc_url, str) or not effective_rpc_url:
            rpc_results = {
                pubkey: {
                    "rpc_current_status": "error",
                    "rpc_current_error": "rpc_url_unavailable",
                }
                for pubkey in pubkeys
            }
        else:
            try:
                rpc_results = recon.rpc_get_multiple_accounts(effective_rpc_url, pubkeys)
            except Exception as exc:  # pragma: no cover - environment dependent
                rpc_results = {
                    pubkey: {
                        "rpc_current_status": "error",
                        "rpc_current_error": str(exc),
                    }
                    for pubkey in pubkeys
                }

    reconciled = recon.reconcile_cases(
        [dict(case) for case in cases],
        by_ab,
        by_mint,
        diag_index,
        log_paths,
        rpc_results,
    )

    rows: list[dict[str, Any]] = []
    for row in reconciled:
        matrix_classification, matrix_reasons = classify_matrix_row(row)
        diag = row.get("diag") or {}
        mfs = row.get("mfs") or {}
        rpc = row.get("rpc_current") or {}
        matrix_row = {
            "plane": row.get("plane"),
            "artifact_sources": row.get("artifact_sources"),
            "ab_record_id": row.get("ab_record_id"),
            "pool_id": row.get("pool_id"),
            "mint": row.get("base_mint"),
            "buy_variant": row.get("buy_variant"),
            "route_kind": row.get("route_kind"),
            "instruction_index": row.get("instruction_index"),
            "account_index": row.get("account_index"),
            "builder_bonding_curve_v2_pubkey": row.get("builder_bonding_curve_v2_pubkey"),
            "tx_meta_account_16_pubkey": tx_meta_account_16_pubkey(row),
            "tx_meta_account_16_matches_builder_bcv2": (
                tx_meta_account_16_pubkey(row) == row.get("builder_bonding_curve_v2_pubkey")
                if tx_meta_account_16_pubkey(row)
                else None
            ),
            "diag_bonding_curve_pubkey": (
                row.get("builder_bonding_curve_v2_pubkey")
                if diag.get("diag_seen_exact_pubkey")
                else None
            ),
            "diag_other_curve_pubkeys_for_mint": diag.get("diag_other_curve_pubkeys_for_mint"),
            "mfs_bonding_curve_pubkeys": mfs.get("mfs_bonding_curve_pubkeys"),
            "mfs_bonding_curve_v2_pubkeys": mfs.get("mfs_bonding_curve_v2_pubkeys"),
            "mfs_contains_builder_bcv2_pubkey": mfs.get("mfs_contains_builder_bcv2_pubkey"),
            "account_state_core_bonding_curve_v2_pubkey": None,
            "rpc_get_account_exists": rpc.get("rpc_current_status") == "present",
            "rpc_get_account_status": rpc.get("rpc_current_status", "not_checked"),
            "rpc_get_account_owner": rpc.get("rpc_current_owner"),
            "rpc_get_account_data_len": rpc.get("rpc_current_data_len"),
            "rpc_get_account_error": rpc.get("rpc_current_error"),
            "owner_layout_status": owner_layout_status(row),
            "diag_seen_exact_builder_bcv2": diag.get("diag_seen_exact_pubkey"),
            "diag_seen_other_curve_for_mint": diag.get("diag_seen_other_curve_pubkey_for_mint"),
            "classification_l1r10": row.get("classification"),
            "classification_l1r10_reasons": row.get("classification_reasons"),
            "matrix_classification": matrix_classification,
            "matrix_reasons": matrix_reasons,
        }
        rows.append(matrix_row)

    summary = summarize_matrix(rows, diag_index, rpc_checked)
    return {
        "schema_version": SCHEMA_VERSION,
        "date": datetime.now(timezone.utc).isoformat(),
        "config_path": str(config_path),
        "namespace": namespace,
        "paths": {key: str(value) for key, value in paths.items()},
        "decision_rows_loaded": len(decisions),
        "diag_account_update_total": diag_index.get("diag_account_update_total", 0),
        "summary": summary,
        "rows": rows,
    }


def summarize_matrix(rows: list[dict[str, Any]], diag_index: dict[str, Any], rpc_checked: bool) -> dict[str, Any]:
    class_counts = Counter(row["matrix_classification"] for row in rows)
    reason_counts = Counter(reason for row in rows for reason in row.get("matrix_reasons") or [])
    rpc_counts = Counter(row.get("rpc_get_account_status", "not_checked") for row in rows)
    plane_counts = Counter(row.get("plane") for row in rows)
    active_rows = [row for row in rows if row.get("plane") == "active_shadow"]
    probe_rows = [row for row in rows if row.get("plane") == "probe"]
    return {
        "status": "matrix_ready" if rows else "no_bonding_curve_v2_blocker_rows",
        "matrix_rows": len(rows),
        "active_shadow_rows": len(active_rows),
        "probe_rows": len(probe_rows),
        "rpc_current_checked": rpc_checked,
        "rpc_get_account_status_counts": dict(sorted(rpc_counts.items())),
        "matrix_classifications": dict(sorted(class_counts.items())),
        "matrix_reasons": dict(sorted(reason_counts.items())),
        "plane_counts": dict(sorted((str(k), v) for k, v in plane_counts.items())),
        "diag_account_update_total": diag_index.get("diag_account_update_total", 0),
        "diag_seen_exact_builder_bcv2_rows": sum(
            1 for row in rows if row.get("diag_seen_exact_builder_bcv2")
        ),
        "diag_seen_other_curve_for_mint_rows": sum(
            1 for row in rows if row.get("diag_seen_other_curve_for_mint")
        ),
        "mfs_contains_builder_bcv2_pubkey_rows": sum(
            1 for row in rows if row.get("mfs_contains_builder_bcv2_pubkey")
        ),
        "tx_meta_account_16_matches_builder_bcv2_rows": sum(
            1 for row in rows if row.get("tx_meta_account_16_matches_builder_bcv2")
        ),
        "recommended_next_path": route_decision(rows, rpc_checked),
    }


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def render_markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    lines = [
        "# RAPORT P3.7-L1R12 BondingCurveV2 Coverage Matrix / Route Decision",
        "",
        f"Date: {payload['date']}",
        f"Namespace: `{payload['namespace']}`",
        "",
        "## Status",
        "",
        "```text",
        f"L1R12 status = {summary['status']}",
        f"matrix_rows = {summary['matrix_rows']}",
        f"active_shadow_rows = {summary['active_shadow_rows']}",
        f"probe_rows = {summary['probe_rows']}",
        f"rpc_current_checked = {summary['rpc_current_checked']}",
        f"rpc_get_account_status_counts = {summary['rpc_get_account_status_counts']}",
        f"matrix_classifications = {summary['matrix_classifications']}",
        f"recommended_next_path = {summary['recommended_next_path']}",
        "L2 / collection / Phase B / P2 / live / tuning = HOLD / NO-GO",
        "```",
        "",
        "## Coverage Matrix Summary",
        "",
        "```text",
        f"diag_account_update_total = {summary['diag_account_update_total']}",
        f"diag_seen_exact_builder_bcv2_rows = {summary['diag_seen_exact_builder_bcv2_rows']}",
        f"diag_seen_other_curve_for_mint_rows = {summary['diag_seen_other_curve_for_mint_rows']}",
        f"mfs_contains_builder_bcv2_pubkey_rows = {summary['mfs_contains_builder_bcv2_pubkey_rows']}",
        f"tx_meta_account_16_matches_builder_bcv2_rows = {summary['tx_meta_account_16_matches_builder_bcv2_rows']}",
        f"matrix_reasons = {summary['matrix_reasons']}",
        "```",
        "",
        "## Interpretation",
        "",
    ]
    if summary["matrix_rows"] == 0:
        lines.append("No `bonding_curve_v2` blocker rows were found in the analyzed artifacts.")
    elif summary["recommended_next_path"] == "route_builder_source_repair_or_route_fallback":
        lines.extend(
            [
                "Every analyzed builder `bonding_curve_v2` pubkey is missing on the current",
                "RPC preflight. DIAG saw another bonding curve for the same mint, but not",
                "the exact builder account. This points away from simple AccountUpdate",
                "coverage and toward route-builder source repair or a route fallback.",
                "",
                "The next fix should answer why the route builder derives/selects these",
                "`bonding_curve_v2` pubkeys and whether this route should be excluded until",
                "a valid simulation-load account exists.",
            ]
        )
    elif summary["recommended_next_path"] == "rpc_readiness_source_and_mfs_materialization":
        lines.extend(
            [
                "At least one builder `bonding_curve_v2` exists on RPC but is not",
                "materialized in MFS. DIAG absence may be expected if the account is a",
                "read-only transaction meta. The next fix should use RPC simulation-load",
                "readiness and add explicit MFS/account-identity materialization.",
            ]
        )
    else:
        lines.append(
            "The matrix does not yet reduce to a single repair path; inspect the row sample below."
        )
    lines.extend(["", "## Sample Rows", ""])
    for row in payload["rows"][:10]:
        lines.extend(
            [
                "```text",
                f"plane = {row.get('plane')}",
                f"artifact_sources = {row.get('artifact_sources')}",
                f"ab_record_id = {row.get('ab_record_id')}",
                f"mint = {row.get('mint')}",
                f"pool_id = {row.get('pool_id')}",
                f"builder_bonding_curve_v2_pubkey = {row.get('builder_bonding_curve_v2_pubkey')}",
                f"tx_meta_account_16_pubkey = {row.get('tx_meta_account_16_pubkey')}",
                f"tx_meta_account_16_matches_builder_bcv2 = {row.get('tx_meta_account_16_matches_builder_bcv2')}",
                f"diag_other_curve_pubkeys_for_mint = {row.get('diag_other_curve_pubkeys_for_mint')}",
                f"rpc_get_account_status = {row.get('rpc_get_account_status')}",
                f"rpc_get_account_owner = {row.get('rpc_get_account_owner')}",
                f"rpc_get_account_data_len = {row.get('rpc_get_account_data_len')}",
                f"matrix_classification = {row.get('matrix_classification')}",
                f"matrix_reasons = {row.get('matrix_reasons')}",
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
    parser.add_argument("--rpc-check-current", action="store_true")
    parser.add_argument("--rpc-url")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    payload = build_matrix(args.config, args.rpc_check_current, args.rpc_url)
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
