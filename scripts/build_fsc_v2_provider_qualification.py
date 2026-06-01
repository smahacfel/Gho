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
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


NLN_PROVIDER = "NLN"
NATIVE_SOL_TOKEN_ADDRESS = "solana"
DEFAULT_MIN_BENCHMARK_HOURS = 0.0
DEFAULT_CANARY_MINUTES = 45.0
SAMPLED_BLOCK_AUDIT_MODE = "sampled_block_audit"
LAMPORTS_PER_SOL = 1_000_000_000
FSC_PARAMETER_GRID_TTL_SECONDS = [180, 300, 600]
FSC_PARAMETER_GRID_GLOBAL_RECIPIENT_CAPS = [13_000, 50_000, 100_000]
FSC_PARAMETER_GRID_STORE_DUST_LAMPORTS = [1_000_000]
FSC_PARAMETER_GRID_ATTRIBUTION_ABS_LAMPORTS = [5_000_000, 10_000_000, 50_000_000]
FSC_PARAMETER_GRID_REL_TO_BUY = [0.0, 0.10, 0.20]

FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON = "FSC_NO_RETAINED_RECIPIENT_HISTORY"
FSC_FUNDING_STREAM_UNAVAILABLE_REASON = "FSC_FUNDING_STREAM_UNAVAILABLE"
FSC_ROLLING_STATE_UNAVAILABLE_REASON = "FSC_ROLLING_STATE_UNAVAILABLE"
FSC_INSUFFICIENT_KNOWN_SOURCES_REASON = "FSC_INSUFFICIENT_KNOWN_SOURCES"
FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON = "FSC_SAME_SLOT_ORDERING_UNAVAILABLE"
FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON = "FSC_LOW_ATTRIBUTION_CONFIDENCE"
FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON = "FSC_ABS_ATTRIBUTION_TOO_SMALL"
FSC_RELATIVE_FUNDING_TOO_SMALL_REASON = "FSC_RELATIVE_FUNDING_TOO_SMALL"
FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON = "FSC_NO_PREBUY_TRANSFER_IN_WINDOW"


@dataclass(frozen=True)
class NlnBuy:
    buyer: str
    mint: str | None
    amount_lamports: int
    slot: int | None
    tx_index: int | None
    instruction_index: int | None
    signature: str | None
    event_ts_ms: int | None
    order_key: tuple[int, int, int, str]


@dataclass(frozen=True)
class NlnTransfer:
    from_wallet: str
    to_wallet: str
    amount_lamports: int
    slot: int | None
    tx_index: int | None
    instruction_index: int | None
    signature: str | None
    event_ts_ms: int | None
    order_key: tuple[int, int, int, str]


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


def nested_dict(row: dict[str, Any], path: list[str]) -> dict[str, Any] | None:
    current: Any = row
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current if isinstance(current, dict) else None


def funding_source_diagnostics(row: dict[str, Any]) -> dict[str, Any] | None:
    for path in (
        ["funding_source_diagnostics"],
        ["sybil_resistance", "funding_source_diagnostics"],
        ["materialized", "sybil_resistance", "funding_source_diagnostics"],
        ["features", "sybil_resistance", "funding_source_diagnostics"],
    ):
        diagnostics = nested_dict(row, path)
        if diagnostics is not None:
            return diagnostics
    return None


def fsc_snapshot_row(row: dict[str, Any]) -> dict[str, Any] | None:
    fsc = row.get("funding_source_v2")
    if not isinstance(fsc, dict):
        return None
    diagnostics = funding_source_diagnostics(row)
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
        "fsc_funding_lane_watermark_slot": fsc.get("funding_lane_watermark_slot"),
        "fsc_funding_lane_lag_slots": fsc.get("funding_lane_lag_slots"),
        "fsc_stream_epoch": fsc.get("stream_epoch"),
        "fsc_last_transfer_recv_ts_ms": fsc.get("last_transfer_recv_ts_ms"),
        "fsc_last_reconnect_ts_ms": fsc.get("last_reconnect_ts_ms"),
        "fsc_dropped_events": fsc.get("dropped_events"),
        "raw_funding_source_diagnostics": diagnostics,
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
    coverage_notes: list[str] = []
    if status_counts.get("clean", 0) == 0:
        coverage_notes.append("no_clean_fsc_v2_rows")
    watermark_rows = sum(
        1 for row in rows if row.get("fsc_funding_lane_watermark_slot") is not None
    )
    lag_rows = sum(1 for row in rows if row.get("fsc_funding_lane_lag_slots") is not None)
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
        "coverage_notes": coverage_notes,
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
        "lane_health": {
            "watermark_rows": watermark_rows,
            "lag_rows": lag_rows,
            "gap_suspected_rows": sum(1 for row in rows if row.get("fsc_gap_suspected") is True),
            "max_funding_lane_watermark_slot": max(
                (
                    int(row["fsc_funding_lane_watermark_slot"])
                    for row in rows
                    if row.get("fsc_funding_lane_watermark_slot") is not None
                ),
                default=None,
            ),
            "max_stream_epoch": max(
                (
                    int(row["fsc_stream_epoch"])
                    for row in rows
                    if row.get("fsc_stream_epoch") is not None
                ),
                default=None,
            ),
            "max_dropped_events": max(
                (
                    int(row["fsc_dropped_events"])
                    for row in rows
                    if row.get("fsc_dropped_events") is not None
                ),
                default=None,
            ),
            "last_transfer_recv_ts_ms": max(
                (
                    int(row["fsc_last_transfer_recv_ts_ms"])
                    for row in rows
                    if row.get("fsc_last_transfer_recv_ts_ms") is not None
                ),
                default=None,
            ),
            "last_reconnect_ts_ms": max(
                (
                    int(row["fsc_last_reconnect_ts_ms"])
                    for row in rows
                    if row.get("fsc_last_reconnect_ts_ms") is not None
                ),
                default=None,
            ),
        },
        "guardrail": "UNKNOWN and NEUTRAL are reported separately and are not treated as clean zero FSC.",
    }


def excluded_reason_to_miss_reason(reason: str | None) -> str:
    normalized = (reason or "").strip().lower()
    return {
        "funding_lane_unavailable": FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
        "index_cold": FSC_ROLLING_STATE_UNAVAILABLE_REASON,
        "no_buyer_cohort": FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
        "insufficient_non_neutral_support": FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
        "low_coverage": FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
        "neutral_only": FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
        "same_slot_ordering_unavailable": FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
        "low_attribution_confidence": FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
    }.get(normalized, FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON)


def add_reason_count(counter: Counter[str], reason: str, count: int | None) -> None:
    if count is not None and count > 0:
        counter[reason] += int(count)


def build_fsc_unknown_reason_report(rows: list[dict[str, Any]]) -> dict[str, Any]:
    reason_counts: Counter[str] = Counter()
    class_counts: Counter[str] = Counter()
    buyer_sample_count = 0
    known_source_count = 0
    unknown_buyer_count = 0
    structural_unknown_count = 0
    operational_unknown_count = 0
    indeterminate_unknown_count = 0
    diagnostics_rows = 0

    for row in rows:
        diagnostics = row.get("raw_funding_source_diagnostics")
        if isinstance(diagnostics, dict):
            diagnostics_rows += 1
            buyer_sample_count += int_value(diagnostics, "buyer_sample_count") or 0
            known_source_count += int_value(diagnostics, "known_source_count") or 0
            unknown_buyer_count += int_value(diagnostics, "unknown_buyer_count") or 0
            structural_unknown_count += (
                int_value(diagnostics, "structural_unknown_buyer_count") or 0
            )
            operational_unknown_count += (
                int_value(diagnostics, "operational_unknown_buyer_count") or 0
            )
            indeterminate_unknown_count += (
                int_value(diagnostics, "indeterminate_unknown_buyer_count") or 0
            )
            miss_counts = diagnostics.get("miss_reason_counts")
            used_miss_counts = False
            if isinstance(miss_counts, list):
                for entry in miss_counts:
                    if not isinstance(entry, dict):
                        continue
                    reason = string_value(entry, "reason")
                    count = int_value(entry, "count")
                    miss_class = string_value(entry, "class") or "unknown"
                    if reason and count:
                        reason_counts[reason] += count
                        class_counts[miss_class] += count
                        used_miss_counts = True
            if not used_miss_counts:
                add_reason_count(
                    reason_counts,
                    excluded_reason_to_miss_reason(
                        common.str_or_none(row.get("fsc_excluded_reason"))
                    ),
                    int_value(diagnostics, "unknown_buyer_count") or 0,
                )
            continue

        total = int(row.get("fsc_total_buyers") or 0)
        known = int(row.get("fsc_known_buyers") or 0)
        unknown = int(row.get("fsc_unknown_count") or 0)
        buyer_sample_count += total
        known_source_count += known
        unknown_buyer_count += unknown

        same_slot = int(row.get("fsc_same_slot_unorderable_count") or 0)
        low_confidence = int(row.get("fsc_low_confidence_count") or 0)
        raw_fsc = row.get("raw_fsc_v2") if isinstance(row.get("raw_fsc_v2"), dict) else {}
        rel_too_small = int(raw_fsc.get("rel_too_small_count") or 0)
        dust_filtered = int(raw_fsc.get("dust_filtered_count") or 0)
        post_buy_filtered = int(raw_fsc.get("post_buy_filtered_count") or 0)

        add_reason_count(reason_counts, FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON, same_slot)
        add_reason_count(reason_counts, FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON, low_confidence)
        add_reason_count(reason_counts, FSC_RELATIVE_FUNDING_TOO_SMALL_REASON, rel_too_small)
        add_reason_count(reason_counts, FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON, dust_filtered)
        add_reason_count(reason_counts, FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON, post_buy_filtered)

        explained = same_slot + low_confidence + rel_too_small + dust_filtered + post_buy_filtered
        remaining_unknown = max(0, unknown - explained)
        add_reason_count(
            reason_counts,
            excluded_reason_to_miss_reason(common.str_or_none(row.get("fsc_excluded_reason"))),
            remaining_unknown,
        )

    top_reason = reason_counts.most_common(1)[0][0] if reason_counts else None
    if top_reason == FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON:
        interpretation = "Most buyers have no retained direct native-SOL inbound transfer in the active FSC window."
    elif top_reason == FSC_FUNDING_STREAM_UNAVAILABLE_REASON:
        interpretation = "Funding lane was unavailable for the dominant share of unresolved FSC buyer samples."
    elif top_reason == FSC_ROLLING_STATE_UNAVAILABLE_REASON:
        interpretation = "Funding lane/index was still cold for the dominant share of unresolved FSC buyer samples."
    elif top_reason == FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON:
        interpretation = "Strict same-slot ordering could not prove transfer-before-buy for many buyer samples."
    elif top_reason == FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON:
        interpretation = "Funding attribution was ambiguous between competing inbound sources."
    else:
        interpretation = "Unresolved FSC buyer samples are diagnostic; low coverage is not treated as clean zero FSC."

    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_unknown_reason_v2",
        "status": "PASS" if rows else "NO-GO",
        "buyer_sample_count": buyer_sample_count,
        "known_source_count": known_source_count,
        "known_rate": (known_source_count / buyer_sample_count) if buyer_sample_count else None,
        "unknown_buyer_count": unknown_buyer_count,
        "unknown_rate": (unknown_buyer_count / buyer_sample_count) if buyer_sample_count else None,
        "structural_unknown_buyer_count": structural_unknown_count,
        "operational_unknown_buyer_count": operational_unknown_count,
        "indeterminate_unknown_buyer_count": indeterminate_unknown_count,
        "diagnostics_rows": diagnostics_rows,
        "fallback_derived_rows": max(0, len(rows) - diagnostics_rows),
        "reason_counts": common.counter_dict(reason_counts),
        "class_counts": common.counter_dict(class_counts),
        "top_reason": top_reason,
        "interpretation": interpretation,
        "low_coverage_blocks_capture": False,
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
    min_audit_slots: int,
    min_audit_transfer_events: int,
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
    if not audit_rows:
        return {
            "selector_schema_version": common.SCHEMA_VERSION,
            "artifact": "nln_provider_benchmark_v1",
            "status": "NOT_AVAILABLE",
            "blocking": False,
            "reason": "no_external_audit_source_configured",
            "claim": "no independent provider-completeness claim",
            "fail_reasons": [],
            "min_benchmark_hours": min_benchmark_hours,
            "min_audit_slots": min_audit_slots,
            "min_audit_transfer_events": min_audit_transfer_events,
            "observed_duration_hours": duration_hours,
            "nln_rows": len(nln_rows),
            "audit_rows": 0,
            "shared_event_keys": 0,
            "keyable_nln_event_keys": len(nln_index),
            "keyable_audit_event_keys": 0,
            "incomplete_nln_event_key_count": len(nln_incomplete),
            "incomplete_audit_event_key_count": 0,
            "incomplete_nln_event_key_classification": (
                "excluded_from_transfer_event_key_benchmark"
                if nln_incomplete
                else "none"
            ),
            "unkeyed_nln_non_transfer_rows": nln_unkeyed_non_transfer,
            "duplicate_nln_event_key_count": nln_duplicate_keys,
            "topic_counts": common.counter_dict(topic_counts),
            "samples": {
                "incomplete_nln_event_keys": nln_incomplete[:20],
            },
            "audit_contract": "Independent provider benchmark is not available without an external Chainstack/raw Yellowstone/archive-capable audit source.",
        }
    fail_reasons: list[str] = []
    if not nln_rows:
        fail_reasons.append("nln_rows_missing")
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
    audit_modes = Counter(str(row.get("audit_mode") or "missing") for row in audit_rows)
    sampled_audit_rows = [
        row for row in audit_rows if str(row.get("audit_mode") or "") == SAMPLED_BLOCK_AUDIT_MODE
    ]
    audit_slots_sampled_from_rows = len(
        {
            slot
            for slot in (int_value(row, "slot") for row in sampled_audit_rows)
            if slot is not None
        }
    )
    audit_slots_sampled = max(
        [audit_slots_sampled_from_rows]
        + [
            value
            for value in (
                int_value(row, "audit_slots_sampled_total", "audit_slots_sampled")
                for row in sampled_audit_rows
            )
            if value is not None
        ]
    )
    sampled_audit_index, _, _, _ = index_by_event_key(sampled_audit_rows)
    audit_transfer_event_keys = len(sampled_audit_index)
    if audit_rows and not sampled_audit_rows:
        fail_reasons.append("audit_mode_not_sampled_block_audit")
    if sampled_audit_rows and (
        audit_slots_sampled < min_audit_slots
        and audit_transfer_event_keys < min_audit_transfer_events
    ):
        fail_reasons.append("audit_sample_below_minimum")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "nln_provider_benchmark_v1",
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "min_benchmark_hours": min_benchmark_hours,
        "min_audit_slots": min_audit_slots,
        "min_audit_transfer_events": min_audit_transfer_events,
        "observed_duration_hours": duration_hours,
        "nln_rows": len(nln_rows),
        "audit_rows": len(audit_rows),
        "audit_sampling_mode": SAMPLED_BLOCK_AUDIT_MODE if sampled_audit_rows else None,
        "audit_mode_counts": common.counter_dict(audit_modes),
        "audit_slots_sampled": audit_slots_sampled,
        "audit_transfer_event_keys": audit_transfer_event_keys,
        "shared_event_keys": len(shared),
        "keyable_nln_event_keys": len(nln_index),
        "keyable_audit_event_keys": len(audit_index),
        "incomplete_nln_event_key_count": len(nln_incomplete),
        "incomplete_audit_event_key_count": len(audit_incomplete),
        "incomplete_nln_event_key_classification": (
            "excluded_from_transfer_event_key_benchmark"
            if nln_incomplete
            else "none"
        ),
        "incomplete_audit_event_key_classification": (
            "audit_source_incomplete_fail_closed"
            if audit_incomplete
            else "none"
        ),
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
        "audit_contract": "Provider benchmark coverage PASS requires archive-capable sampled_block_audit rows; observed_event_fidelity is diagnostic-only.",
    }


def build_topic_liveness_report(
    *,
    create_rows: list[dict[str, Any]],
    trade_rows: list[dict[str, Any]],
    transfer_rows: list[dict[str, Any]],
    normalization_error_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    topic_inputs = {
        "pumpfun.create": create_rows,
        "pumpfun.trade": trade_rows,
        "system.transfers": transfer_rows,
    }
    duration_by_topic: dict[str, float | None] = {}
    first_ts_by_topic: dict[str, int | None] = {}
    last_ts_by_topic: dict[str, int | None] = {}
    for name, rows in topic_inputs.items():
        times = [event_time_for_benchmark(row) for row in rows]
        clean = [value for value in times if value is not None]
        first_ts_by_topic[name] = min(clean) if clean else None
        last_ts_by_topic[name] = max(clean) if clean else None
        duration_by_topic[name] = (
            (max(clean) - min(clean)) / 3_600_000.0 if len(clean) >= 2 else 0.0
        )
    normalization_by_topic = Counter(
        str(row.get("topic_kind") or row.get("topic") or "unknown")
        for row in normalization_error_rows
    )
    notes = []
    if not create_rows:
        notes.append("create_coverage_no_rows")
    if not trade_rows:
        notes.append("trade_coverage_no_rows")
    if not transfer_rows:
        notes.append("transfer_coverage_no_rows")
    trade_status = "PASS" if trade_rows else "NO-GO"
    transfer_status = "PASS" if transfer_rows else "NO-GO"
    create_status = "PASS" if create_rows else "NO_COVERAGE"
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "nln_topic_liveness_v1",
        "status": "PASS" if trade_rows and transfer_rows else "NO-GO",
        "notes": notes,
        "pumpfun_trade_rows": len(trade_rows),
        "system_transfer_rows": len(transfer_rows),
        "pumpfun_create_rows": len(create_rows),
        "trade_status": trade_status,
        "transfer_status": transfer_status,
        "create_status": create_status,
        "nln_create_used_for_birth": False,
        "fsc_required_topics_status": "PASS" if trade_rows and transfer_rows else "NO-GO",
        "create_rows": len(create_rows),
        "trade_rows": len(trade_rows),
        "transfer_rows": len(transfer_rows),
        "normalization_error_rows": len(normalization_error_rows),
        "normalization_errors_by_topic": common.counter_dict(normalization_by_topic),
        "first_ts_ms_by_topic": first_ts_by_topic,
        "last_ts_ms_by_topic": last_ts_by_topic,
        "duration_hours_by_topic": duration_by_topic,
        "benchmark_boundary": "Event-key provider benchmark is transfer-only; create/trade liveness is reported here and does not poison transfer coverage.",
    }


def fake_zero_fsc_row(row: dict[str, Any]) -> bool:
    value = row.get("fsc_count")
    if not isinstance(value, (int, float)) or float(value) != 0.0:
        return False
    status = str(row.get("fsc_status") or "").lower()
    total_buyers = int(row.get("fsc_total_buyers") or 0)
    unknown = int(row.get("fsc_unknown_count") or 0)
    non_neutral = int(row.get("fsc_known_non_neutral_buyers") or 0)
    return status != "clean" or (total_buyers > 0 and unknown >= total_buyers) or non_neutral < 2


def bool_flag(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() in {"1", "true", "yes", "on", "enabled"}
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return value != 0
    return False


def build_capture_canary_report(
    *,
    create_rows: list[dict[str, Any]],
    trade_rows: list[dict[str, Any]],
    transfer_rows: list[dict[str, Any]],
    decision_rows: list[dict[str, Any]],
    fsc_rows: list[dict[str, Any]],
    topic_liveness: dict[str, Any],
    canary_minutes: float,
    fsc_decision_enabled: bool,
    fsc_hard_reject_enabled: bool,
) -> dict[str, Any]:
    fake_zero_rows = [row for row in fsc_rows if fake_zero_fsc_row(row)]
    shadow_counterfactual_rows = sum(
        1
        for row in decision_rows
        if row.get("shadow_fsc_v2_policy_signal") is not None
        or row.get("shadow_fsc_v2_soft_points_if_enabled") is not None
        or row.get("shadow_fsc_v2_reason_if_enabled") is not None
    )
    active_score_enabled_count = sum(
        1
        for row in decision_rows
        if bool_flag(row.get("fsc_v2_decision_enabled"))
        or bool_flag(row.get("funding_source_v2_decision_enabled"))
        or bool_flag(row.get("active_fsc_v2_policy_signal"))
    )
    fail_reasons: list[str] = []
    if not trade_rows:
        fail_reasons.append("no_trade_rows")
    if not transfer_rows:
        fail_reasons.append("no_transfer_rows")
    if not decision_rows:
        fail_reasons.append("no_decision_rows")
    if not fsc_rows:
        fail_reasons.append("no_fsc_snapshots")
    if fake_zero_rows:
        fail_reasons.append("fake_zero_fsc_detected")
    if active_score_enabled_count:
        fail_reasons.append("active_fsc_policy_signal_detected")
    if fsc_decision_enabled:
        fail_reasons.append("fsc_decision_enabled")
    if fsc_hard_reject_enabled:
        fail_reasons.append("fsc_hard_reject_enabled")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_capture_canary_v1",
        "status": "PASS" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "canary_minutes": canary_minutes,
        "trade_rows": len(trade_rows),
        "transfer_rows": len(transfer_rows),
        "create_rows": len(create_rows),
        "decision_rows": len(decision_rows),
        "fsc_snapshot_rows": len(fsc_rows),
        "funding_source_v2_present_rows": len(fsc_rows),
        "fake_zero_fsc_count": len(fake_zero_rows),
        "active_score_enabled_count": active_score_enabled_count,
        "shadow_counterfactual_rows": shadow_counterfactual_rows,
        "decision_enabled": fsc_decision_enabled,
        "hard_reject_enabled": fsc_hard_reject_enabled,
        "policy_activation": "OFF",
        "low_coverage_blocks_capture": False,
        "nln_trade_liveness": topic_liveness.get("trade_status"),
        "nln_transfer_liveness": topic_liveness.get("transfer_status"),
        "nln_create_liveness": topic_liveness.get("create_status"),
        "samples": {
            "fake_zero_fsc": fake_zero_rows[:20],
        },
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
        has_eventual = any(row.get("snapshot_mode") == "eventual_postfill" for row in rows)
        if has_eventual:
            fail_reasons.append("no_candidate_with_both_decision_time_and_eventual_snapshots")
        else:
            fail_reasons.append("eventual_postfill_snapshots_missing")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "decision_time_vs_eventual_fsc_v1",
        "status": (
            "PASS"
            if not fail_reasons
            else (
                "PENDING_NOT_BLOCKING_FOR_PHASE1"
                if fail_reasons == ["eventual_postfill_snapshots_missing"]
                else "NO-GO"
            )
        ),
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


def event_order_key(row: dict[str, Any]) -> tuple[int, int, int, str]:
    return (
        int_value(row, "slot") if int_value(row, "slot") is not None else 2**63 - 1,
        int_value(row, "tx_index", "txIndex")
        if int_value(row, "tx_index", "txIndex") is not None
        else 2**31 - 1,
        int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index")
        if int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index")
        is not None
        else 2**31 - 1,
        string_value(row, "signature", "tx_signature") or "",
    )


def nln_trade_is_buy(row: dict[str, Any]) -> bool:
    is_buy = first_value(row, ["is_buy", "isBuy"])
    if isinstance(is_buy, bool):
        return is_buy
    if isinstance(is_buy, str) and is_buy.strip().lower() in {"true", "1", "yes"}:
        return True
    side = (string_value(row, "ix_name", "ixName", "side") or "").strip().lower()
    return side.startswith("buy")


def parse_nln_buy(row: dict[str, Any]) -> NlnBuy | None:
    if not nln_trade_is_buy(row):
        return None
    buyer = string_value(row, "user", "buyer", "signer")
    if not buyer:
        return None
    amount = int_value(row, "sol_amount", "solAmount", "sol_amount_lamports", "max_sol_cost") or 0
    return NlnBuy(
        buyer=buyer,
        mint=string_value(row, "mint", "base_mint", "token_mint"),
        amount_lamports=amount,
        slot=int_value(row, "slot"),
        tx_index=int_value(row, "tx_index", "txIndex"),
        instruction_index=int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index"),
        signature=string_value(row, "signature", "tx_signature"),
        event_ts_ms=event_ts_ms(row),
        order_key=event_order_key(row),
    )


def parse_nln_transfer(row: dict[str, Any]) -> NlnTransfer | None:
    token_address = string_value(row, "token_address", "tokenAddress")
    if token_address != NATIVE_SOL_TOKEN_ADDRESS:
        return None
    from_wallet = string_value(row, "from_wallet", "fromWallet", "source_wallet", "from")
    to_wallet = string_value(row, "to_wallet", "toWallet", "recipient_wallet", "to")
    amount = int_value(row, "amount", "amount_lamports", "lamports")
    if not from_wallet or not to_wallet or amount is None:
        return None
    return NlnTransfer(
        from_wallet=from_wallet,
        to_wallet=to_wallet,
        amount_lamports=amount,
        slot=int_value(row, "slot"),
        tx_index=int_value(row, "tx_index", "txIndex"),
        instruction_index=int_value(row, "instruction_index", "instructionIndex", "outer_instruction_index"),
        signature=string_value(row, "signature", "tx_signature"),
        event_ts_ms=event_ts_ms(row),
        order_key=event_order_key(row),
    )


def first_buys_by_mint_buyer(trade_rows: list[dict[str, Any]]) -> dict[tuple[str | None, str], NlnBuy]:
    first: dict[tuple[str | None, str], NlnBuy] = {}
    for row in trade_rows:
        buy = parse_nln_buy(row)
        if buy is None:
            continue
        key = (buy.mint, buy.buyer)
        current = first.get(key)
        if current is None or buy.order_key < current.order_key:
            first[key] = buy
    return first


def native_transfers_by_recipient(
    transfer_rows: list[dict[str, Any]],
    *,
    global_recipient_cap: int | None,
) -> tuple[dict[str, list[NlnTransfer]], int, int]:
    transfers = [transfer for row in transfer_rows if (transfer := parse_nln_transfer(row)) is not None]
    last_order_by_recipient: dict[str, tuple[int, int, int, str]] = {}
    for transfer in transfers:
        current = last_order_by_recipient.get(transfer.to_wallet)
        if current is None or transfer.order_key > current:
            last_order_by_recipient[transfer.to_wallet] = transfer.order_key
    retained_recipients: set[str] | None = None
    evicted_recipient_count = 0
    if global_recipient_cap is not None and len(last_order_by_recipient) > global_recipient_cap:
        retained_recipients = {
            recipient
            for recipient, _ in sorted(
                last_order_by_recipient.items(),
                key=lambda item: item[1],
                reverse=True,
            )[:global_recipient_cap]
        }
        evicted_recipient_count = len(last_order_by_recipient) - len(retained_recipients)
    by_recipient: dict[str, list[NlnTransfer]] = defaultdict(list)
    for transfer in transfers:
        if retained_recipients is not None and transfer.to_wallet not in retained_recipients:
            continue
        by_recipient[transfer.to_wallet].append(transfer)
    for recipient in by_recipient:
        by_recipient[recipient].sort(key=lambda transfer: transfer.order_key)
    return by_recipient, len(transfers), evicted_recipient_count


def transfer_precedes_buy(transfer: NlnTransfer, buy: NlnBuy) -> bool | None:
    if transfer.slot is not None and buy.slot is not None:
        if transfer.slot < buy.slot:
            return True
        if transfer.slot > buy.slot:
            return False
        if transfer.tx_index is not None and buy.tx_index is not None:
            return transfer.tx_index < buy.tx_index
        if (
            transfer.signature
            and buy.signature
            and transfer.signature == buy.signature
            and transfer.instruction_index is not None
            and buy.instruction_index is not None
        ):
            return transfer.instruction_index < buy.instruction_index
        return None
    return None


def nln_native_join_summary(
    *,
    trade_rows: list[dict[str, Any]],
    transfer_rows: list[dict[str, Any]],
    ttl_seconds: int,
    min_abs_attribution_lamports: int,
    min_rel_to_buy: float,
    global_recipient_cap: int | None,
) -> dict[str, Any]:
    buys = first_buys_by_mint_buyer(trade_rows)
    transfers_by_recipient, native_transfer_count, evicted_recipient_count = native_transfers_by_recipient(
        transfer_rows,
        global_recipient_cap=global_recipient_cap,
    )
    known_buyers = 0
    no_history = 0
    same_slot_unorderable = 0
    timestamp_unverifiable = 0
    no_prebuy = 0
    ttl_filtered = 0
    abs_too_small = 0
    rel_too_small = 0
    top_sources: Counter[str] = Counter()

    for buy in buys.values():
        history = transfers_by_recipient.get(buy.buyer) or []
        if not history:
            no_history += 1
            continue
        valid_source_seen = False
        ordered_candidate_seen = False
        ttl_candidate_seen = False
        amount_candidate_seen = False
        unorderable_for_buy = False
        ts_missing_for_buy = False
        for transfer in history:
            precedes = transfer_precedes_buy(transfer, buy)
            if precedes is None:
                unorderable_for_buy = True
                continue
            if not precedes:
                continue
            ordered_candidate_seen = True
            if buy.event_ts_ms is None or transfer.event_ts_ms is None:
                ts_missing_for_buy = True
                continue
            if transfer.event_ts_ms < buy.event_ts_ms - ttl_seconds * 1000:
                continue
            ttl_candidate_seen = True
            rel_floor = int(math.ceil(float(buy.amount_lamports) * min_rel_to_buy))
            if transfer.amount_lamports < min_abs_attribution_lamports:
                abs_too_small += 1
                continue
            if transfer.amount_lamports < rel_floor:
                rel_too_small += 1
                continue
            amount_candidate_seen = True
            valid_source_seen = True
            top_sources[transfer.from_wallet] += 1
        if valid_source_seen:
            known_buyers += 1
        elif unorderable_for_buy and not ordered_candidate_seen:
            same_slot_unorderable += 1
        elif ts_missing_for_buy and ordered_candidate_seen:
            timestamp_unverifiable += 1
        elif not ordered_candidate_seen:
            no_prebuy += 1
        elif not ttl_candidate_seen:
            ttl_filtered += 1
        elif not amount_candidate_seen:
            pass

    total_buyers = len(buys)
    return {
        "ttl_seconds": ttl_seconds,
        "global_recipient_cap": global_recipient_cap,
        "min_abs_attribution_lamports": min_abs_attribution_lamports,
        "min_rel_to_buy": min_rel_to_buy,
        "total_buyers": total_buyers,
        "native_transfer_rows": native_transfer_count,
        "known_buyers": known_buyers,
        "known_rate": (known_buyers / total_buyers) if total_buyers else None,
        "unknown_buyer_count": max(0, total_buyers - known_buyers),
        "no_retained_recipient_history_count": no_history,
        "same_slot_unorderable_count": same_slot_unorderable,
        "timestamp_unverifiable_count": timestamp_unverifiable,
        "no_prebuy_transfer_in_window_count": no_prebuy,
        "ttl_filtered_count": ttl_filtered,
        "abs_too_small_count": abs_too_small,
        "rel_too_small_count": rel_too_small,
        "global_recipient_evicted_count": evicted_recipient_count,
        "top_funder": top_sources.most_common(1)[0][0] if top_sources else None,
        "top_funder_count": top_sources.most_common(1)[0][1] if top_sources else 0,
    }


def build_fsc_parameter_grid_report(
    *,
    trade_rows: list[dict[str, Any]],
    transfer_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    variants = []
    for ttl_seconds in FSC_PARAMETER_GRID_TTL_SECONDS:
        for global_cap in FSC_PARAMETER_GRID_GLOBAL_RECIPIENT_CAPS:
            for store_dust in FSC_PARAMETER_GRID_STORE_DUST_LAMPORTS:
                for min_abs in FSC_PARAMETER_GRID_ATTRIBUTION_ABS_LAMPORTS:
                    for rel_to_buy in FSC_PARAMETER_GRID_REL_TO_BUY:
                        summary = nln_native_join_summary(
                            trade_rows=trade_rows,
                            transfer_rows=transfer_rows,
                            ttl_seconds=ttl_seconds,
                            min_abs_attribution_lamports=min_abs,
                            min_rel_to_buy=rel_to_buy,
                            global_recipient_cap=global_cap,
                        )
                        summary["min_abs_store_lamports"] = store_dust
                        variants.append(summary)
    best = max(
        variants,
        key=lambda row: row.get("known_rate") if row.get("known_rate") is not None else -1.0,
        default=None,
    )
    baseline = next(
        (
            row
            for row in variants
            if row["ttl_seconds"] == 300
            and row["global_recipient_cap"] == 100_000
            and row["min_abs_attribution_lamports"] == 10_000_000
            and row["min_rel_to_buy"] == 0.20
        ),
        None,
    )
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_parameter_grid_v1",
        "status": "PASS" if trade_rows and transfer_rows else "NO-GO",
        "variant_count": len(variants),
        "ttl_seconds_grid": FSC_PARAMETER_GRID_TTL_SECONDS,
        "global_recipient_cap_grid": FSC_PARAMETER_GRID_GLOBAL_RECIPIENT_CAPS,
        "min_abs_store_lamports_grid": FSC_PARAMETER_GRID_STORE_DUST_LAMPORTS,
        "min_abs_attribution_lamports_grid": FSC_PARAMETER_GRID_ATTRIBUTION_ABS_LAMPORTS,
        "min_rel_to_buy_grid": FSC_PARAMETER_GRID_REL_TO_BUY,
        "baseline_variant": baseline,
        "best_known_rate_variant": best,
        "variants": variants,
        "purpose": "Diagnostic-only NLN-native sensitivity grid; it does not tune active Gatekeeper policy.",
    }


def build_nln_native_fsc_join_sanity_report(
    *,
    trade_rows: list[dict[str, Any]],
    transfer_rows: list[dict[str, Any]],
    fsc_coverage: dict[str, Any],
) -> dict[str, Any]:
    summary = nln_native_join_summary(
        trade_rows=trade_rows,
        transfer_rows=transfer_rows,
        ttl_seconds=300,
        min_abs_attribution_lamports=10_000_000,
        min_rel_to_buy=0.20,
        global_recipient_cap=100_000,
    )
    runtime_known_rate = fsc_coverage.get("known_non_neutral_rate")
    nln_known_rate = summary.get("known_rate")
    if isinstance(runtime_known_rate, (int, float)) and isinstance(nln_known_rate, (int, float)):
        if nln_known_rate > runtime_known_rate * 2 and (nln_known_rate - runtime_known_rate) > 0.02:
            interpretation = "NLN-native join sees materially higher funding coverage than runtime FSC; inspect buyer identity/session alignment."
        elif nln_known_rate < 0.05 and runtime_known_rate < 0.05:
            interpretation = "Both NLN-native and runtime FSC have low direct native-SOL coverage; likely market/single-hop limitation rather than only session alignment."
        else:
            interpretation = "NLN-native and runtime FSC coverage are broadly comparable for this capture sample."
    else:
        interpretation = "Join comparison is incomplete because at least one side lacks a known-rate denominator."
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "nln_native_fsc_join_sanity_v1",
        "status": "PASS" if trade_rows and transfer_rows else "NO-GO",
        "runtime_session_fsc_known_rate": runtime_known_rate,
        "runtime_session_total_buyers": fsc_coverage.get("total_buyers"),
        "runtime_session_known_non_neutral_buyers": fsc_coverage.get("known_non_neutral_buyers"),
        "nln_native_fsc_known_rate": nln_known_rate,
        "nln_native_summary": summary,
        "interpretation": interpretation,
        "guardrail": "Diagnostic-only; does not claim provider completeness and does not enable FSC policy.",
    }


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
    normalization_error_rows = rows_from_paths(args.nln_normalization_error)
    audit_rows = rows_from_paths(args.audit_event)
    eventual_rows = rows_from_paths(args.eventual_fsc_snapshot)

    outputs = {
        "pumpfun_create_raw_v1": raw_dir / "pumpfun_create_raw_v1.jsonl",
        "pumpfun_trade_raw_v1": raw_dir / "pumpfun_trade_raw_v1.jsonl",
        "system_transfers_raw_v1": raw_dir / "system_transfers_raw_v1.jsonl",
        "nln_candidate_birth_v1": dataset_dir / "nln_candidate_birth_v1.jsonl",
        "funding_events_v1": dataset_dir / "funding_events_v1.jsonl",
        "fsc_snapshots_v2": dataset_dir / "fsc_snapshots_v2.jsonl",
        "fsc_coverage_v2": report_dir / "fsc_coverage_v2.json",
        "fsc_unknown_reason_v2": report_dir / "fsc_unknown_reason_v2.json",
        "fsc_parameter_grid_v1": report_dir / "fsc_parameter_grid_v1.json",
        "nln_native_fsc_join_sanity_v1": report_dir / "nln_native_fsc_join_sanity_v1.json",
        "nln_topic_liveness_v1": report_dir / "nln_topic_liveness_v1.json",
        "fsc_capture_canary_v1": report_dir / "fsc_capture_canary_v1.json",
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
    fsc_rows.extend(eventual_rows)
    common.write_jsonl(outputs["nln_candidate_birth_v1"], birth_rows)
    common.write_jsonl(outputs["funding_events_v1"], funding_rows)
    common.write_jsonl(outputs["fsc_snapshots_v2"], fsc_rows)

    fsc_coverage = build_fsc_coverage_report(fsc_rows, decision_rows=len(decision_rows))
    fsc_unknown_reason = build_fsc_unknown_reason_report(fsc_rows)
    fsc_parameter_grid = build_fsc_parameter_grid_report(
        trade_rows=trade_rows,
        transfer_rows=transfer_rows,
    )
    nln_native_join_sanity = build_nln_native_fsc_join_sanity_report(
        trade_rows=trade_rows,
        transfer_rows=transfer_rows,
        fsc_coverage=fsc_coverage,
    )
    topic_liveness = build_topic_liveness_report(
        create_rows=create_rows,
        trade_rows=trade_rows,
        transfer_rows=transfer_rows,
        normalization_error_rows=normalization_error_rows,
    )
    capture_canary = build_capture_canary_report(
        create_rows=create_rows,
        trade_rows=trade_rows,
        transfer_rows=transfer_rows,
        decision_rows=decision_rows,
        fsc_rows=fsc_rows,
        topic_liveness=topic_liveness,
        canary_minutes=args.canary_minutes,
        fsc_decision_enabled=args.fsc_decision_enabled,
        fsc_hard_reject_enabled=args.fsc_hard_reject_enabled,
    )
    benchmark = build_provider_benchmark(
        nln_rows=transfer_rows,
        audit_rows=audit_rows,
        min_benchmark_hours=args.min_benchmark_hours,
        min_audit_slots=args.min_audit_slots,
        min_audit_transfer_events=args.min_audit_transfer_events,
    )
    decision_vs_eventual = build_decision_time_vs_eventual(fsc_rows)
    common.write_json(outputs["fsc_coverage_v2"], fsc_coverage)
    common.write_json(outputs["fsc_unknown_reason_v2"], fsc_unknown_reason)
    common.write_json(outputs["fsc_parameter_grid_v1"], fsc_parameter_grid)
    common.write_json(outputs["nln_native_fsc_join_sanity_v1"], nln_native_join_sanity)
    common.write_json(outputs["nln_topic_liveness_v1"], topic_liveness)
    common.write_json(outputs["fsc_capture_canary_v1"], capture_canary)
    common.write_json(outputs["nln_provider_benchmark_v1"], benchmark)
    common.write_json(outputs["decision_time_vs_eventual_fsc_v1"], decision_vs_eventual)

    stage_reports = {
        "fsc_coverage_v2": fsc_coverage,
        "fsc_unknown_reason_v2": fsc_unknown_reason,
        "fsc_parameter_grid_v1": fsc_parameter_grid,
        "nln_native_fsc_join_sanity_v1": nln_native_join_sanity,
        "nln_topic_liveness_v1": topic_liveness,
        "fsc_capture_canary_v1": capture_canary,
        "nln_provider_benchmark_v1": benchmark,
        "decision_time_vs_eventual_fsc_v1": decision_vs_eventual,
    }
    blocking_stage_names = {
        "fsc_coverage_v2",
        "fsc_unknown_reason_v2",
        "fsc_parameter_grid_v1",
        "nln_native_fsc_join_sanity_v1",
        "nln_topic_liveness_v1",
        "fsc_capture_canary_v1",
    }
    blocking_fail_reasons = [
        f"{name}:{report.get('status')}"
        for name, report in stage_reports.items()
        if name in blocking_stage_names and report.get("status") != "PASS"
    ]
    pending_reasons = [
        f"{name}:{report.get('status')}"
        for name, report in stage_reports.items()
        if report.get("status") == "PENDING_NOT_BLOCKING_FOR_PHASE1"
    ]
    manifest_status = "NO-GO" if blocking_fail_reasons else "PASS_FOR_PHASE1_EVIDENCE"
    provider_benchmark_status = str(benchmark.get("status") or "missing")
    component_statuses = {
        "fsc_pr8_capture_canary": capture_canary.get("status"),
        "fsc_stream_integrity": topic_liveness.get("status"),
        "fsc_materialization": "PASS" if fsc_rows else "NO-GO",
        "fsc_unknown_reason_report": fsc_unknown_reason.get("status"),
        "fsc_parameter_grid": fsc_parameter_grid.get("status"),
        "fsc_nln_native_join_sanity": nln_native_join_sanity.get("status"),
        "fsc_no_fake_zero": "PASS"
        if capture_canary.get("fake_zero_fsc_count") == 0
        else "NO-GO",
        "nln_trade_liveness": topic_liveness.get("trade_status"),
        "nln_transfer_liveness": topic_liveness.get("transfer_status"),
        "nln_create_liveness": topic_liveness.get("create_status"),
        "provider_independent_benchmark": provider_benchmark_status,
        "provider_independent_benchmark_blocking": False,
        "fsc_policy_activation": "OFF",
        "phase1_dataset_unblock": "PASS" if manifest_status != "NO-GO" else "NO-GO",
    }
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "fsc_provider_qualification_manifest_v1",
        "scope": args.scope,
        "status": manifest_status,
        "fail_reasons": blocking_fail_reasons,
        "pending_reasons": pending_reasons,
        "component_statuses": component_statuses,
        "fsc_pr8_capture_canary": component_statuses["fsc_pr8_capture_canary"],
        "fsc_stream_integrity": component_statuses["fsc_stream_integrity"],
        "fsc_materialization": component_statuses["fsc_materialization"],
        "fsc_unknown_reason_report": component_statuses["fsc_unknown_reason_report"],
        "fsc_parameter_grid": component_statuses["fsc_parameter_grid"],
        "fsc_nln_native_join_sanity": component_statuses["fsc_nln_native_join_sanity"],
        "fsc_no_fake_zero": component_statuses["fsc_no_fake_zero"],
        "nln_trade_liveness": component_statuses["nln_trade_liveness"],
        "nln_transfer_liveness": component_statuses["nln_transfer_liveness"],
        "nln_create_liveness": component_statuses["nln_create_liveness"],
        "provider_independent_benchmark": component_statuses["provider_independent_benchmark"],
        "fsc_policy_activation": component_statuses["fsc_policy_activation"],
        "phase1_dataset_unblock": component_statuses["phase1_dataset_unblock"],
        "capture_evidence_status": (
            "PASS"
            if manifest_status != "NO-GO"
            else "NO-GO"
        ),
        "provider_policy_qualification": "NOT_CLAIMED",
        "runtime_impact": "offline_artifact_builder_only; no Gatekeeper, execution, or runtime config changes",
        "active_gatekeeper_fsc_v2": "disabled",
        "r2_ssot_contract": "Program Streams are not R2 SSOT; use raw Yellowstone/DIAG/canonical account-state snapshots for R2.",
        "input_provenance": {
            "nln_create": [file_provenance(path) for path in args.nln_create],
            "nln_trade": [file_provenance(path) for path in args.nln_trade],
            "nln_transfer": [file_provenance(path) for path in args.nln_transfer],
            "decision_log": [file_provenance(path) for path in args.decision_log],
            "nln_normalization_error": [
                file_provenance(path) for path in args.nln_normalization_error
            ],
            "audit_event": [file_provenance(path) for path in args.audit_event],
            "eventual_fsc_snapshot": [
                file_provenance(path) for path in args.eventual_fsc_snapshot
            ],
        },
        "row_counts": {
            "nln_create_rows": len(create_rows),
            "nln_trade_rows": len(trade_rows),
            "nln_transfer_rows": len(transfer_rows),
            "decision_rows": len(decision_rows),
            "normalization_error_rows": len(normalization_error_rows),
            "audit_rows": len(audit_rows),
            "eventual_fsc_snapshot_rows": len(eventual_rows),
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
    parser.add_argument("--nln-normalization-error", type=Path, action="append", default=[])
    parser.add_argument("--decision-log", type=Path, action="append", default=[])
    parser.add_argument("--eventual-fsc-snapshot", type=Path, action="append", default=[])
    parser.add_argument(
        "--audit-event",
        type=Path,
        action="append",
        default=[],
        help="Chainstack/raw Yellowstone/archive-capable audit JSONL rows for benchmark comparison.",
    )
    parser.add_argument("--min-benchmark-hours", type=float, default=DEFAULT_MIN_BENCHMARK_HOURS)
    parser.add_argument("--min-audit-slots", type=int, default=1000)
    parser.add_argument("--min-audit-transfer-events", type=int, default=10_000)
    parser.add_argument("--canary-minutes", type=float, default=DEFAULT_CANARY_MINUTES)
    parser.add_argument("--fsc-decision-enabled", action="store_true")
    parser.add_argument("--fsc-hard-reject-enabled", action="store_true")
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
    return 0 if manifest["status"] != "NO-GO" else 2


if __name__ == "__main__":
    raise SystemExit(main())
