#!/usr/bin/env python3
"""Build FSC v2 provider-qualification artifacts from durable JSONL evidence.

This script is offline-only. It does not subscribe to NLN, does not call RPC,
does not mutate runtime state, and does not authorize FSC scoring. Its purpose
is to freeze the evidence needed to decide later whether NLN Program Streams
are good enough for FSC v2 capture.
"""

from __future__ import annotations

import argparse
import json
import math
import os
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


NLN_PROVIDER = "NLN"
NATIVE_SOL_TOKEN_ADDRESS = "solana"
DEFAULT_MIN_BENCHMARK_HOURS = 24.0


def rows_from_paths(paths: list[Path]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in paths:
        for index, row in enumerate(common.iter_json_objects(path), start=1):
            copy = dict(row)
            copy["_source_path"] = str(path)
            copy["_source_index"] = index
            rows.append(copy)
    return rows


def payload(row: dict[str, Any]) -> dict[str, Any]:
    for key in ("payload_json", "payload", "data", "event"):
        value = row.get(key)
        if isinstance(value, dict):
            return value
    return row


def first_value(row: dict[str, Any], names: Iterable[str]) -> Any:
    body = payload(row)
    for key in names:
        value = row.get(key)
        if value not in (None, ""):
            return value
        value = body.get(key)
        if value not in (None, ""):
            return value
    return common.find_first_key(row, names)


def string_value(row: dict[str, Any], *names: str) -> str | None:
    value = first_value(row, names)
    if isinstance(value, str) and value:
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return str(value)
    return None


def parse_int_like(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return int(value)
    if isinstance(value, str):
        cleaned = value.strip()
        if not cleaned:
            return None
        try:
            return int(cleaned, 10)
        except ValueError:
            try:
                parsed = float(cleaned)
            except ValueError:
                return None
            if math.isfinite(parsed):
                return int(parsed)
    return None


def parse_float_like(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    if isinstance(value, str):
        cleaned = value.strip()
        if not cleaned:
            return None
        try:
            parsed = float(cleaned)
        except ValueError:
            return None
        if math.isfinite(parsed):
            return parsed
    return None


def int_value(row: dict[str, Any], *names: str) -> int | None:
    return parse_int_like(first_value(row, names))


def float_value(row: dict[str, Any], *names: str) -> float | None:
    return parse_float_like(first_value(row, names))


def topic(row: dict[str, Any], fallback: str) -> str:
    return string_value(row, "topic") or fallback


def event_ts_ms(row: dict[str, Any]) -> int | None:
    block_time = int_value(row, "block_time", "blockTime")
    if block_time is not None:
        return block_time * 1000
    return int_value(
        row,
        "event_ts_ms",
        "timestamp_ms",
        "timestampMs",
        "provider_ts_ms",
        "recv_ts_ms",
        "decode_ts_ms",
        "ts_ms",
    )


def raw_log_row(row: dict[str, Any], *, provider_topic: str) -> dict[str, Any]:
    out = dict(row)
    out.pop("_source_path", None)
    out.pop("_source_index", None)
    out.setdefault("provider", NLN_PROVIDER)
    out.setdefault("topic", topic(row, provider_topic))
    return out


def copy_raw_topic(rows: list[dict[str, Any]], output: Path, *, provider_topic: str) -> int:
    return common.write_jsonl(output, (raw_log_row(row, provider_topic=provider_topic) for row in rows))


def link_or_copy_raw_topic(
    input_paths: list[Path],
    rows: list[dict[str, Any]],
    output: Path,
    *,
    provider_topic: str,
) -> int:
    """Materialize raw topic evidence without duplicating live capture bytes.

    PR8 live capture writes durable JSONL under logs/nln_capture/<scope>.  When
    there is a single source path on the same filesystem, the official
    logs/nln/<scope> artifact can be a hardlink to that durable source.  This
    preserves the .jsonl file contract while avoiding a second copy of high
    volume Program Stream rows during 24h qualification runs.
    """
    existing_inputs = [path for path in input_paths if path.exists()]
    if len(existing_inputs) == 1:
        source = existing_inputs[0]
        output.parent.mkdir(parents=True, exist_ok=True)
        try:
            if output.exists() or output.is_symlink():
                output.unlink()
            os.link(source, output)
            return len(rows)
        except OSError:
            pass
    return copy_raw_topic(rows, output, provider_topic=provider_topic)


def candidate_birth_row(row: dict[str, Any], *, provider_topic: str) -> dict[str, Any]:
    mint = string_value(row, "mint", "base_mint", "token_mint")
    bonding_curve = string_value(row, "bonding_curve", "bondingCurve", "bonding_curve_pubkey")
    birth = event_ts_ms(row)
    identity = {
        "base_mint": mint,
        "bonding_curve": bonding_curve,
        "pool_id": bonding_curve or string_value(row, "pool_id", "pool_amm_id", "amm_id"),
        "birth_ts_ms": birth,
    }
    candidate_id, candidate_id_source = common.deterministic_candidate_id(identity)
    missing = [
        field
        for field, value in (
            ("base_mint", mint),
            ("bonding_curve", bonding_curve),
            ("birth_ts_ms", birth),
        )
        if value in (None, "")
    ]
    if candidate_id is None:
        candidate_id = f"incomplete:{common.stable_hash(identity)}"
        candidate_id_source = "incomplete_identity_hash"
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "nln_candidate_birth_v1",
        "candidate_id": candidate_id,
        "candidate_id_source": candidate_id_source,
        "candidate_birth_status": "ok" if not missing else "universe_incomplete",
        "candidate_identity_missing_fields": missing,
        "cohort": "pumpfun_bonding_curve_sol_v1",
        "cohort_in_scope": not missing,
        "provider": NLN_PROVIDER,
        "source_topic": topic(row, provider_topic),
        "source_kind": "nln_program_stream",
        "source_path": row.get("_source_path"),
        "source_index": row.get("_source_index"),
        "signature": string_value(row, "signature", "tx_signature"),
        "slot": int_value(row, "slot"),
        "tx_index": int_value(row, "tx_index", "txIndex"),
        "birth_ts_ms": birth,
        "base_mint": mint,
        "mint_id": mint,
        "pool_id": identity["pool_id"],
        "bonding_curve": bonding_curve,
        "creator": string_value(row, "creator", "creator_wallet", "user"),
        "quote_mint": "SOL",
        "quote_mint_source": "verified_nln_pumpfun_create_topic",
    }


def funding_event_row(row: dict[str, Any], *, provider_topic: str) -> dict[str, Any] | None:
    token_address = string_value(row, "token_address", "tokenAddress")
    if token_address != NATIVE_SOL_TOKEN_ADDRESS:
        return None
    amount = int_value(row, "amount", "amount_lamports", "lamports")
    from_wallet = string_value(row, "from_wallet", "fromWallet", "source_wallet", "from")
    to_wallet = string_value(row, "to_wallet", "toWallet", "recipient_wallet", "to")
    signature = string_value(row, "signature", "tx_signature")
    slot = int_value(row, "slot")
    tx_index = int_value(row, "tx_index", "txIndex")
    instruction_index = int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index")
    missing = [
        field
        for field, value in (
            ("from_wallet", from_wallet),
            ("to_wallet", to_wallet),
            ("amount_lamports", amount),
            ("signature", signature),
            ("slot", slot),
            ("tx_index", tx_index),
            ("instruction_index", instruction_index),
        )
        if value in (None, "")
    ]
    event_id = common.stable_hash(
        {
            "signature": signature,
            "instruction_index": instruction_index,
            "from_wallet": from_wallet,
            "to_wallet": to_wallet,
            "amount_lamports": amount,
        }
    )
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "funding_events_v1",
        "funding_event_status": "ok" if not missing else "incomplete",
        "missing_fields": missing,
        "provider": NLN_PROVIDER,
        "source_topic": topic(row, provider_topic),
        "source_kind": "nln_program_stream",
        "source_path": row.get("_source_path"),
        "source_index": row.get("_source_index"),
        "event_id": event_id,
        "signature": signature,
        "slot": slot,
        "tx_index": tx_index,
        "instruction_index": instruction_index,
        "event_ts_ms": event_ts_ms(row),
        "from_wallet": from_wallet,
        "to_wallet": to_wallet,
        "amount_lamports": amount,
        "token_address": token_address,
        "asset": "native_sol",
        "event_order_key": [slot, tx_index, instruction_index, signature],
    }


def fsc_snapshot_row(row: dict[str, Any]) -> dict[str, Any] | None:
    fsc = row.get("funding_source_v2")
    if not isinstance(fsc, dict):
        return None
    candidate_id = string_value(row, "candidate_id", "execution_candidate_id", "join_key")
    mint = string_value(row, "base_mint", "mint_id", "token_mint")
    cutoff_ts = int_value(row, "decision_ts_ms", "decision_timestamp_ms", "timestamp", "ts_ms")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_snapshots_v2",
        "candidate_id": candidate_id,
        "mint": mint,
        "pool_id": string_value(row, "pool_id", "pool_amm_id", "amm_id"),
        "snapshot_kind": "decision",
        "snapshot_mode": fsc.get("snapshot_mode"),
        "feature_cutoff_ts_ms": cutoff_ts,
        "feature_cutoff_slot": fsc.get("max_buy_slot"),
        "provider": fsc.get("provider") or NLN_PROVIDER,
        "source_topics": fsc.get("source_topics") or [],
        "fsc_available": fsc.get("hhi_norm_count") is not None,
        "fsc_coverage_ok": fsc.get("status") == "clean",
        "fsc_excluded_reason": fsc.get("excluded_reason"),
        "fsc_status": fsc.get("status"),
        "fsc_total_buyers": fsc.get("total_buyers"),
        "fsc_known_buyers": fsc.get("known_buyers"),
        "fsc_known_non_neutral_buyers": fsc.get("known_non_neutral_buyers"),
        "fsc_unknown_count": fsc.get("unknown_count"),
        "fsc_neutral_count": fsc.get("neutral_count"),
        "fsc_low_confidence_count": fsc.get("low_confidence_count"),
        "fsc_same_slot_unorderable_count": fsc.get("same_slot_unorderable_count"),
        "fsc_known_coverage": fsc.get("known_coverage"),
        "fsc_non_neutral_known_coverage": fsc.get("non_neutral_known_coverage"),
        "fsc_count": fsc.get("hhi_norm_count"),
        "fsc_sol_weighted": fsc.get("hhi_norm_sol_weighted_excess"),
        "fsc_top_funder": fsc.get("top_funder"),
        "fsc_top_funder_count": fsc.get("top_funder_count"),
        "fsc_top_funder_buy_sol": fsc.get("top_funder_buy_sol"),
        "fsc_min_funding_lamports": fsc.get("min_abs_attribution_lamports"),
        "fsc_ttl_seconds": fsc.get("ttl_seconds"),
        "fsc_neutral_funder_set_hash": fsc.get("neutral_funder_set_hash"),
        "fsc_config_hash": fsc.get("config_hash"),
        "fsc_index_warm": fsc.get("index_warm"),
        "fsc_gap_suspected": fsc.get("gap_suspected"),
        "raw_fsc_v2": fsc,
    }


def average(values: Iterable[float | int | None]) -> float | None:
    clean = [float(value) for value in values if isinstance(value, (int, float))]
    if not clean:
        return None
    return sum(clean) / len(clean)


def build_fsc_coverage_report(rows: list[dict[str, Any]], *, decision_rows: int) -> dict[str, Any]:
    status_counts = Counter(str(row.get("fsc_status") or "missing") for row in rows)
    excluded_counts = Counter(str(row.get("fsc_excluded_reason") or "none") for row in rows)
    total_buyers = sum(int(row.get("fsc_total_buyers") or 0) for row in rows)
    unknown = sum(int(row.get("fsc_unknown_count") or 0) for row in rows)
    neutral = sum(int(row.get("fsc_neutral_count") or 0) for row in rows)
    non_neutral = sum(int(row.get("fsc_known_non_neutral_buyers") or 0) for row in rows)
    fail_reasons: list[str] = []
    if decision_rows == 0:
        fail_reasons.append("decision_logs_missing")
    if not rows:
        fail_reasons.append("fsc_v2_rows_missing")
    if status_counts.get("clean", 0) == 0:
        fail_reasons.append("no_clean_fsc_v2_rows")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_coverage_v2",
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "decision_rows": decision_rows,
        "fsc_snapshot_rows": len(rows),
        "fsc_available_rows": sum(1 for row in rows if row.get("fsc_available") is True),
        "status_counts": common.counter_dict(status_counts),
        "excluded_reason_counts": common.counter_dict(excluded_counts),
        "total_buyers": total_buyers,
        "unknown_count": unknown,
        "neutral_count": neutral,
        "known_non_neutral_buyers": non_neutral,
        "unknown_rate": (unknown / total_buyers) if total_buyers else None,
        "neutral_rate": (neutral / total_buyers) if total_buyers else None,
        "known_non_neutral_rate": (non_neutral / total_buyers) if total_buyers else None,
        "avg_known_coverage": average(row.get("fsc_known_coverage") for row in rows),
        "avg_non_neutral_known_coverage": average(
            row.get("fsc_non_neutral_known_coverage") for row in rows
        ),
        "guardrail": "UNKNOWN and NEUTRAL are reported separately and are not treated as clean zero FSC.",
    }


def is_transfer_like(row: dict[str, Any]) -> bool:
    topic_value = string_value(row, "topic", "source_topic")
    if topic_value and "transfer" in topic_value.lower():
        return True
    return any(
        first_value(row, names) not in (None, "")
        for names in (
            ("from_wallet", "fromWallet", "source_wallet", "from"),
            ("to_wallet", "toWallet", "recipient_wallet", "to"),
            ("amount", "amount_lamports", "lamports"),
            ("token_address", "tokenAddress"),
        )
    )


def event_key_components(row: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    signature = string_value(row, "signature", "tx_signature")
    instruction = int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index")
    tx_index = int_value(row, "tx_index", "txIndex")
    from_wallet = string_value(row, "from_wallet", "fromWallet", "source_wallet", "from")
    to_wallet = string_value(row, "to_wallet", "toWallet", "recipient_wallet", "to")
    amount = int_value(row, "amount", "amount_lamports", "lamports")
    parts = {
        "signature": signature,
        "tx_index": tx_index,
        "instruction_index": instruction,
        "from_wallet": from_wallet,
        "to_wallet": to_wallet,
        "amount_lamports": amount,
    }
    missing = [field for field, value in parts.items() if value in (None, "")]
    return parts, missing


def event_key(row: dict[str, Any]) -> str | None:
    if not is_transfer_like(row):
        return None
    parts, missing = event_key_components(row)
    if missing:
        return None
    return (
        f"{parts['signature']}:{parts['tx_index']}:{parts['instruction_index']}:"
        f"{parts['from_wallet']}:{parts['to_wallet']}:{parts['amount_lamports']}"
    )


def event_time_for_benchmark(row: dict[str, Any]) -> int | None:
    return int_value(row, "recv_ts_ms", "received_ts_ms", "arrival_ts_ms") or event_ts_ms(row)


def incomplete_key_sample(row: dict[str, Any], missing: list[str]) -> dict[str, Any]:
    return {
        "source_path": row.get("_source_path"),
        "source_index": row.get("_source_index"),
        "topic": string_value(row, "topic", "source_topic"),
        "signature": string_value(row, "signature", "tx_signature"),
        "missing_fields": missing,
    }


def index_by_event_key(
    rows: list[dict[str, Any]],
) -> tuple[dict[str, dict[str, Any]], list[dict[str, Any]], int, int]:
    indexed: dict[str, dict[str, Any]] = {}
    incomplete: list[dict[str, Any]] = []
    unkeyed_non_transfer = 0
    duplicate_keys = 0
    for row in rows:
        key = event_key(row)
        if key:
            if key in indexed:
                duplicate_keys += 1
            else:
                indexed[key] = row
            continue
        if is_transfer_like(row):
            _, missing = event_key_components(row)
            incomplete.append(incomplete_key_sample(row, missing))
        else:
            unkeyed_non_transfer += 1
    return indexed, incomplete, unkeyed_non_transfer, duplicate_keys


def build_provider_benchmark(
    *,
    nln_rows: list[dict[str, Any]],
    audit_rows: list[dict[str, Any]],
    min_benchmark_hours: float,
) -> dict[str, Any]:
    topic_counts = Counter(topic(row, string_value(row, "topic") or "unknown_topic") for row in nln_rows)
    nln_index, nln_incomplete, nln_unkeyed_non_transfer, nln_duplicate_keys = index_by_event_key(
        nln_rows
    )
    audit_index, audit_incomplete, audit_unkeyed_non_transfer, audit_duplicate_keys = (
        index_by_event_key(audit_rows)
    )
    shared = sorted(set(nln_index) & set(audit_index))
    missing_on_nln = sorted(set(audit_index) - set(nln_index))
    missing_on_audit = sorted(set(nln_index) - set(audit_index))
    deltas: list[int] = []
    nln_first = 0
    audit_first = 0
    ties = 0
    for key in shared:
        nln_ts = event_time_for_benchmark(nln_index[key])
        audit_ts = event_time_for_benchmark(audit_index[key])
        if nln_ts is None or audit_ts is None:
            continue
        delta = nln_ts - audit_ts
        deltas.append(delta)
        if delta < 0:
            nln_first += 1
        elif delta > 0:
            audit_first += 1
        else:
            ties += 1
    times = [event_time_for_benchmark(row) for row in nln_rows]
    clean_times = [value for value in times if value is not None]
    duration_hours = ((max(clean_times) - min(clean_times)) / 3_600_000.0) if len(clean_times) >= 2 else 0.0
    fail_reasons: list[str] = []
    if not nln_rows:
        fail_reasons.append("nln_rows_missing")
    if not audit_rows:
        fail_reasons.append("audit_rows_missing")
    if nln_incomplete:
        fail_reasons.append("incomplete_nln_event_key")
    if audit_incomplete:
        fail_reasons.append("incomplete_audit_event_key")
    if nln_rows and not nln_index:
        fail_reasons.append("no_keyable_nln_transfer_rows")
    if audit_rows and not audit_index:
        fail_reasons.append("no_keyable_audit_transfer_rows")
    if duration_hours < min_benchmark_hours:
        fail_reasons.append("benchmark_duration_below_minimum")
    if not shared and audit_index:
        fail_reasons.append("no_overlap_with_audit_source")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "nln_provider_benchmark_v1",
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "min_benchmark_hours": min_benchmark_hours,
        "observed_duration_hours": duration_hours,
        "nln_rows": len(nln_rows),
        "audit_rows": len(audit_rows),
        "shared_event_keys": len(shared),
        "keyable_nln_event_keys": len(nln_index),
        "keyable_audit_event_keys": len(audit_index),
        "incomplete_nln_event_key_count": len(nln_incomplete),
        "incomplete_audit_event_key_count": len(audit_incomplete),
        "unkeyed_nln_non_transfer_rows": nln_unkeyed_non_transfer,
        "unkeyed_audit_non_transfer_rows": audit_unkeyed_non_transfer,
        "duplicate_nln_event_key_count": nln_duplicate_keys,
        "duplicate_audit_event_key_count": audit_duplicate_keys,
        "missing_on_nln": len(missing_on_nln),
        "missing_on_audit": len(missing_on_audit),
        "nln_first_count": nln_first,
        "audit_first_count": audit_first,
        "tie_count": ties,
        "delta_ms_p50": percentile(deltas, 0.50),
        "delta_ms_p90": percentile(deltas, 0.90),
        "delta_ms_p99": percentile(deltas, 0.99),
        "topic_counts": common.counter_dict(topic_counts),
        "samples": {
            "missing_on_nln": missing_on_nln[:20],
            "missing_on_audit": missing_on_audit[:20],
            "incomplete_nln_event_keys": nln_incomplete[:20],
            "incomplete_audit_event_keys": audit_incomplete[:20],
        },
        "audit_contract": "Audit rows must come from Chainstack/raw Yellowstone/archive-capable source; NLN RPC is not sufficient coverage proof.",
    }


def percentile(values: list[int], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * q)))
    return float(ordered[index])


def build_decision_time_vs_eventual(rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_candidate: dict[str, dict[str, dict[str, Any]]] = defaultdict(dict)
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        mode = common.str_or_none(row.get("snapshot_mode"))
        if candidate_id and mode:
            by_candidate[candidate_id][mode] = row
    comparisons = []
    for candidate_id, modes in sorted(by_candidate.items()):
        decision = modes.get("decision_time")
        eventual = modes.get("eventual_postfill")
        if not decision or not eventual:
            continue
        comparisons.append(
            {
                "candidate_id": candidate_id,
                "decision_time_fsc_count": decision.get("fsc_count"),
                "eventual_fsc_count": eventual.get("fsc_count"),
                "decision_time_status": decision.get("fsc_status"),
                "eventual_status": eventual.get("fsc_status"),
                "fsc_count_delta": numeric_delta(eventual.get("fsc_count"), decision.get("fsc_count")),
            }
        )
    fail_reasons = []
    if not rows:
        fail_reasons.append("fsc_snapshots_missing")
    if not comparisons:
        fail_reasons.append("no_candidate_with_both_decision_time_and_eventual_snapshots")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "decision_time_vs_eventual_fsc_v1",
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "fsc_snapshot_rows": len(rows),
        "comparison_rows": len(comparisons),
        "nonzero_delta_count": sum(
            1 for row in comparisons if row.get("fsc_count_delta") not in (None, 0.0)
        ),
        "samples": comparisons[:50],
    }


def numeric_delta(left: Any, right: Any) -> float | None:
    left_f = common.float_or_none(left)
    right_f = common.float_or_none(right)
    if left_f is None or right_f is None:
        return None
    return left_f - right_f


def file_provenance(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {"path": None, "exists": False}
    return {
        "path": str(path),
        "exists": path.exists(),
        "size_bytes": path.stat().st_size if path.exists() and path.is_file() else None,
    }


def build_artifacts(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    raw_dir = args.root / "logs" / "nln" / args.scope
    dataset_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)
    raw_dir.mkdir(parents=True, exist_ok=True)

    create_rows = rows_from_paths(args.nln_create)
    trade_rows = rows_from_paths(args.nln_trade)
    transfer_rows = rows_from_paths(args.nln_transfer)
    decision_rows = rows_from_paths(args.decision_log)
    audit_rows = rows_from_paths(args.audit_event)

    outputs = {
        "pumpfun_create_raw_v1": raw_dir / "pumpfun_create_raw_v1.jsonl",
        "pumpfun_trade_raw_v1": raw_dir / "pumpfun_trade_raw_v1.jsonl",
        "system_transfers_raw_v1": raw_dir / "system_transfers_raw_v1.jsonl",
        "nln_candidate_birth_v1": dataset_dir / "nln_candidate_birth_v1.jsonl",
        "funding_events_v1": dataset_dir / "funding_events_v1.jsonl",
        "fsc_snapshots_v2": dataset_dir / "fsc_snapshots_v2.jsonl",
        "fsc_coverage_v2": report_dir / "fsc_coverage_v2.json",
        "nln_provider_benchmark_v1": report_dir / "nln_provider_benchmark_v1.json",
        "decision_time_vs_eventual_fsc_v1": report_dir / "decision_time_vs_eventual_fsc_v1.json",
        "fsc_provider_qualification_manifest_v1": report_dir / "fsc_provider_qualification_manifest_v1.json",
    }

    link_or_copy_raw_topic(
        args.nln_create,
        create_rows,
        outputs["pumpfun_create_raw_v1"],
        provider_topic=args.create_topic,
    )
    link_or_copy_raw_topic(
        args.nln_trade,
        trade_rows,
        outputs["pumpfun_trade_raw_v1"],
        provider_topic=args.trade_topic,
    )
    link_or_copy_raw_topic(
        args.nln_transfer,
        transfer_rows,
        outputs["system_transfers_raw_v1"],
        provider_topic=args.transfer_topic,
    )

    birth_rows = [candidate_birth_row(row, provider_topic=args.create_topic) for row in create_rows]
    funding_rows = [
        event
        for event in (funding_event_row(row, provider_topic=args.transfer_topic) for row in transfer_rows)
        if event is not None
    ]
    fsc_rows = [row for row in (fsc_snapshot_row(row) for row in decision_rows) if row is not None]
    common.write_jsonl(outputs["nln_candidate_birth_v1"], birth_rows)
    common.write_jsonl(outputs["funding_events_v1"], funding_rows)
    common.write_jsonl(outputs["fsc_snapshots_v2"], fsc_rows)

    fsc_coverage = build_fsc_coverage_report(fsc_rows, decision_rows=len(decision_rows))
    benchmark = build_provider_benchmark(
        nln_rows=create_rows + trade_rows + transfer_rows,
        audit_rows=audit_rows,
        min_benchmark_hours=args.min_benchmark_hours,
    )
    decision_vs_eventual = build_decision_time_vs_eventual(fsc_rows)
    common.write_json(outputs["fsc_coverage_v2"], fsc_coverage)
    common.write_json(outputs["nln_provider_benchmark_v1"], benchmark)
    common.write_json(outputs["decision_time_vs_eventual_fsc_v1"], decision_vs_eventual)

    stage_reports = {
        "fsc_coverage_v2": fsc_coverage,
        "nln_provider_benchmark_v1": benchmark,
        "decision_time_vs_eventual_fsc_v1": decision_vs_eventual,
    }
    fail_reasons = [
        f"{name}:{report.get('status')}"
        for name, report in stage_reports.items()
        if report.get("status") not in {"PASS", "ok"}
    ]
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_provider_qualification_manifest_v1",
        "scope": args.scope,
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "runtime_impact": "offline_artifact_builder_only; no Gatekeeper, execution, or runtime config changes",
        "active_gatekeeper_fsc_v2": "disabled",
        "r2_ssot_contract": "Program Streams are not R2 SSOT; use raw Yellowstone/DIAG/canonical account-state snapshots for R2.",
        "input_provenance": {
            "nln_create": [file_provenance(path) for path in args.nln_create],
            "nln_trade": [file_provenance(path) for path in args.nln_trade],
            "nln_transfer": [file_provenance(path) for path in args.nln_transfer],
            "decision_log": [file_provenance(path) for path in args.decision_log],
            "audit_event": [file_provenance(path) for path in args.audit_event],
        },
        "row_counts": {
            "nln_create_rows": len(create_rows),
            "nln_trade_rows": len(trade_rows),
            "nln_transfer_rows": len(transfer_rows),
            "decision_rows": len(decision_rows),
            "audit_rows": len(audit_rows),
            "candidate_birth_rows": len(birth_rows),
            "funding_event_rows": len(funding_rows),
            "fsc_snapshot_rows": len(fsc_rows),
        },
        "outputs": {key: str(path) for key, path in outputs.items()},
        "stage_reports": stage_reports,
    }
    common.write_json(outputs["fsc_provider_qualification_manifest_v1"], manifest)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--nln-create", type=Path, action="append", default=[])
    parser.add_argument("--nln-trade", type=Path, action="append", default=[])
    parser.add_argument("--nln-transfer", type=Path, action="append", default=[])
    parser.add_argument("--decision-log", type=Path, action="append", default=[])
    parser.add_argument(
        "--audit-event",
        type=Path,
        action="append",
        default=[],
        help="Chainstack/raw Yellowstone/archive-capable audit JSONL rows for benchmark comparison.",
    )
    parser.add_argument("--min-benchmark-hours", type=float, default=DEFAULT_MIN_BENCHMARK_HOURS)
    parser.add_argument("--create-topic", default="prod.rpc.solana.pumpfun.create")
    parser.add_argument("--trade-topic", default="prod.rpc.solana.pumpfun.trade")
    parser.add_argument("--transfer-topic", default="prod.rpc.solana.system.transfers")
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = build_artifacts(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
