#!/usr/bin/env python3
"""Shared offline contracts for the pump.fun selector dataset pipeline.

This module is intentionally runtime-inert.  It normalizes durable JSONL
evidence into selector-dataset rows without changing Gatekeeper, IWIM, execution,
or lifecycle behavior.
"""

from __future__ import annotations

import hashlib
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1
WRAPPED_SOL_MINT = "So11111111111111111111111111111111111111112"
SOL_QUOTE_ALIASES = {"SOL", "WSOL", WRAPPED_SOL_MINT}
BIRTH_EVENT_TYPES = {
    "newpooldetected",
    "new_pool_detected",
    "poolcreated",
    "pool_created",
    "createpool",
    "create_pool",
    "pumpfuncreate",
    "pump_fun_create",
    "pumpfun_create",
    "bondingcurvebirth",
    "bonding_curve_birth",
    "poolbirth",
    "pool_birth",
}
SNAPSHOT_OFFSETS_MS = {
    "birth+5s": 5_000,
    "birth+15s": 15_000,
    "birth+30s": 30_000,
    "birth+60s": 60_000,
}
DISALLOWED_FEATURE_FIELDS = {
    "close_reason",
    "final_pnl_pct",
    "truth_status",
    "truth_source",
    "r1_label",
    "r1_label_reason",
    "r2_label",
    "r2_label_reason",
    "r2_status",
    "label_resolved",
    "execution_outcome",
    "shadow_execution_outcome",
    "entry_execution_ts_ms",
    "close_ts_ms",
    "position_duration_ms",
}
CANONICAL_R2_SOURCE_MARKERS = (
    "yellowstone",
    "geyser",
    "diag_account_update_relay",
    "canonical_account_state",
    "account_update_canonical",
    "accountupdate",
)
PRECISION_R2_DENOMINATOR_CONTRACT = (
    "split=holdout AND selector_accept=true AND cohort_in_scope=true AND "
    "stream_completeness_ok=true AND label_resolved=true AND "
    "r2_label in {positive, negative}"
)


def iter_json_objects(path: Path | None) -> Iterable[dict[str, Any]]:
    """Yield JSON objects from a JSONL file, tolerating concatenated objects."""
    if path is None or not path.exists():
        return
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            index = 0
            while index < len(raw):
                try:
                    obj, next_index = decoder.raw_decode(raw, index)
                except json.JSONDecodeError:
                    break
                if isinstance(obj, dict):
                    yield obj
                index = next_index
                while index < len(raw) and raw[index].isspace():
                    index += 1


def read_jsonl(path: Path | None) -> list[dict[str, Any]]:
    return list(iter_json_objects(path))


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")
            count += 1
    return count


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def nested(row: dict[str, Any], *keys: str) -> Any:
    value: Any = row
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def int_or_none(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return int(value)
    return None


def float_or_none(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def find_first_key(value: Any, names: Iterable[str], *, depth: int = 0) -> Any:
    """Find the first matching key in nested JSON evidence.

    This is used only for offline normalization across historical artifact
    shapes.  It deliberately returns the first explicit field rather than
    inferring values from unrelated text.
    """
    if depth > 6:
        return None
    wanted = tuple(names)
    if isinstance(value, dict):
        for key in wanted:
            if key in value and value[key] not in (None, ""):
                return value[key]
        for item in value.values():
            found = find_first_key(item, wanted, depth=depth + 1)
            if found not in (None, ""):
                return found
    elif isinstance(value, list):
        for item in value:
            found = find_first_key(item, wanted, depth=depth + 1)
            if found not in (None, ""):
                return found
    return None


def event_type(row: dict[str, Any]) -> str:
    for key_path in (("kind", "type"), ("type",), ("event_type",), ("kind",)):
        value = nested(row, *key_path) if len(key_path) > 1 else row.get(key_path[0])
        if isinstance(value, str) and value:
            return value
    return "unknown"


def normalized_event_type(row: dict[str, Any]) -> str:
    return event_type(row).replace("-", "_").replace(" ", "_").lower()


def is_birth_create_event(row: dict[str, Any]) -> bool:
    explicit = bool_or_none(find_first_key(row, ("is_birth_event", "is_create_event")))
    if explicit is True:
        return True
    return normalized_event_type(row) in BIRTH_EVENT_TYPES


def source_ts_ms(row: dict[str, Any]) -> int | None:
    return int_or_none(
        find_first_key(
            row,
            (
                "birth_ts_ms",
                "created_ts_ms",
                "create_ts_ms",
                "curve_t0_event_ts_ms",
                "first_seen_ts_ms",
                "event_time_ms",
                "timestamp_ms",
                "ts_ms",
            ),
        )
    )


def source_slot(row: dict[str, Any]) -> int | None:
    return int_or_none(find_first_key(row, ("slot", "event_slot", "create_slot")))


def extract_identity(row: dict[str, Any]) -> dict[str, Any]:
    candidate_id = str_or_none(
        find_first_key(row, ("candidate_id", "execution_candidate_id", "lifecycle_candidate_id"))
    )
    base_mint = str_or_none(find_first_key(row, ("base_mint", "mint_id", "mint", "token_mint")))
    pool_id = str_or_none(find_first_key(row, ("pool_id", "pool_amm_id", "amm_id", "pool")))
    bonding_curve = str_or_none(
        find_first_key(row, ("bonding_curve", "bonding_curve_pubkey", "canonical_bonding_curve"))
    )
    quote_mint = str_or_none(find_first_key(row, ("quote_mint", "quoteMint")))
    birth_ts = int_or_none(
        find_first_key(
            row,
            (
                "birth_ts_ms",
                "created_ts_ms",
                "create_ts_ms",
                "curve_t0_event_ts_ms",
                "first_seen_ts_ms",
                "event_time_ms",
                "timestamp_ms",
            ),
        )
    )
    decision_ts = int_or_none(
        find_first_key(row, ("decision_ts_ms", "observation_end_ts_ms", "decision_timestamp_ms"))
    )
    return {
        "candidate_id": candidate_id,
        "base_mint": base_mint,
        "pool_id": pool_id,
        "bonding_curve": bonding_curve,
        "quote_mint": quote_mint,
        "birth_ts_ms": birth_ts,
        "decision_ts_ms": decision_ts,
        "slot": source_slot(row),
    }


def quote_mint_is_sol(value: str | None) -> bool:
    if value is None:
        return False
    cleaned = value.strip()
    return cleaned == WRAPPED_SOL_MINT or cleaned.upper() in {"SOL", "WSOL"}


def deterministic_candidate_id(identity: dict[str, Any]) -> tuple[str | None, str]:
    explicit = str_or_none(identity.get("candidate_id"))
    if explicit:
        return explicit, "existing_candidate_id"
    base_mint = str_or_none(identity.get("base_mint"))
    bonding_curve = str_or_none(identity.get("bonding_curve"))
    birth_ts = int_or_none(identity.get("birth_ts_ms"))
    if base_mint and bonding_curve and birth_ts is not None:
        return f"{base_mint}:{bonding_curve}:{birth_ts}", "mint_bonding_curve_birth_ts"
    return None, "missing_identity"


def identity_fingerprint(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        row.get("base_mint"),
        row.get("bonding_curve"),
        row.get("pool_id"),
        row.get("birth_ts_ms"),
    )


def identity_conflict(existing: dict[str, Any], incoming: dict[str, Any]) -> bool:
    for field in ("base_mint", "bonding_curve", "pool_id", "birth_ts_ms"):
        left = existing.get(field)
        right = incoming.get(field)
        if left not in (None, "") and right not in (None, "") and left != right:
            return True
    return False


def stable_hash(payload: Any) -> str:
    encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def candidate_universe_row(row: dict[str, Any], *, source_path: str, source_index: int) -> dict[str, Any]:
    identity = extract_identity(row)
    candidate_id, id_source = deterministic_candidate_id(identity)
    quote = str_or_none(identity.get("quote_mint"))
    missing = [
        field
        for field in ("base_mint", "bonding_curve", "birth_ts_ms", "quote_mint")
        if identity.get(field) in (None, "")
    ]
    cohort_in_scope = quote_mint_is_sol(quote)
    if missing:
        status = "universe_incomplete"
    elif not cohort_in_scope:
        status = "non_sol_quote"
    else:
        status = "ok"
    payload = nested(row, "kind", "payload") or row.get("payload") or {}
    verdict = find_first_key(row, ("gatekeeper_verdict", "verdict_type"))
    decision_buy = bool_or_none(find_first_key(row, ("decision_verdict_buy",)))
    if decision_buy is None and isinstance(verdict, str):
        decision_buy = verdict.upper() in {"BUY", "EARLY_BUY"}
    return {
        "selector_schema_version": SCHEMA_VERSION,
        "candidate_id": candidate_id,
        "candidate_id_source": id_source,
        "candidate_identity_hash": stable_hash(identity),
        "candidate_universe_status": status,
        "candidate_identity_missing_fields": missing,
        "cohort": "pumpfun_bonding_curve_sol_v1",
        "cohort_in_scope": cohort_in_scope and not missing,
        "base_mint": identity.get("base_mint"),
        "mint_id": identity.get("base_mint"),
        "pool_id": identity.get("pool_id"),
        "bonding_curve": identity.get("bonding_curve"),
        "quote_mint": quote,
        "quote_mint_is_sol": quote_mint_is_sol(quote),
        "birth_ts_ms": identity.get("birth_ts_ms"),
        "birth_slot": identity.get("slot"),
        "decision_ts_ms": identity.get("decision_ts_ms"),
        "gatekeeper_verdict": verdict,
        "decision_verdict_buy": decision_buy,
        "decision_reason": find_first_key(row, ("decision_reason", "reason_code")),
        "event_type": event_type(row),
        "event_source": source_path,
        "event_source_index": source_index,
        "event_id": str_or_none(nested(row, "envelope", "event_id")),
        "run_id": str_or_none(nested(row, "envelope", "run_id")),
        "stream_completeness_ok": status == "ok",
        "duplicate_status": "primary",
        "raw_source_kind": payload.get("source") if isinstance(payload, dict) else None,
    }


def merge_candidate_rows(rows: Iterable[dict[str, Any]]) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    merged: dict[str, dict[str, Any]] = {}
    anonymous: list[dict[str, Any]] = []
    collisions: list[dict[str, Any]] = []
    duplicates = 0
    for row in rows:
        candidate_id = str_or_none(row.get("candidate_id"))
        if not candidate_id:
            row = dict(row)
            row["candidate_id"] = f"incomplete:{row.get('candidate_identity_hash')}"
            row["candidate_id_source"] = "incomplete_identity_hash"
            anonymous.append(row)
            continue
        if candidate_id not in merged:
            merged[candidate_id] = dict(row)
            continue
        existing = merged[candidate_id]
        duplicates += 1
        if identity_conflict(existing, row):
            existing["duplicate_status"] = "identity_collision"
            existing["candidate_universe_status"] = "universe_incomplete"
            collisions.append(
                {
                    "candidate_id": candidate_id,
                    "existing": identity_fingerprint(existing),
                    "incoming": identity_fingerprint(row),
                }
            )
            continue
        existing["duplicate_status"] = "deduped"
        for key, value in row.items():
            if existing.get(key) in (None, "") and value not in (None, ""):
                existing[key] = value
    all_rows = list(merged.values()) + anonymous
    all_rows.sort(key=lambda row: (int_or_none(row.get("birth_ts_ms")) or 0, str(row.get("candidate_id"))))
    return all_rows, {"duplicates": duplicates, "collisions": collisions}


def flat_final_pnl_pct(row: dict[str, Any]) -> float | None:
    top_level = float_or_none(row.get("final_pnl_pct"))
    if top_level is not None:
        return top_level
    return float_or_none(nested(row, "shadow", "final_pnl_pct"))


def execution_realized(row: dict[str, Any]) -> bool:
    explicit = bool_or_none(row.get("execution_realized"))
    if explicit is not None:
        return explicit
    return bool(
        row.get("analysis_status") == "ok"
        and row.get("truth_status") == "resolved"
        and str_or_none(row.get("close_reason"))
        and flat_final_pnl_pct(row) is not None
    )


def classify_r1(row: dict[str, Any], *, pnl_target_net_pct: float) -> dict[str, Any]:
    truth_status = row.get("truth_status")
    close_reason = str_or_none(row.get("close_reason"))
    pnl = flat_final_pnl_pct(row)
    realized = execution_realized(row)
    base = {
        "execution_realized": realized,
        "r1_label": None,
        "r1_label_reason": None,
        "r1_excluded_reason": None,
        "r1_gray_reason": None,
    }
    if truth_status != "resolved":
        base["r1_excluded_reason"] = "truth_status_not_resolved"
        return base
    if not realized:
        base["r1_excluded_reason"] = "execution_not_realized"
        return base
    if pnl is None:
        base["r1_excluded_reason"] = "missing_final_pnl_pct"
        return base
    if close_reason == "Target" or pnl >= pnl_target_net_pct:
        base["r1_label"] = "positive"
        base["r1_label_reason"] = "target_or_pnl_target"
    elif close_reason == "StopLoss":
        base["r1_label"] = "negative"
        base["r1_label_reason"] = "stop_loss"
    elif pnl <= 0.0:
        base["r1_label"] = "negative"
        base["r1_label_reason"] = "non_positive_pnl"
    elif close_reason == "TimeStop" and pnl < pnl_target_net_pct:
        base["r1_label"] = "negative"
        base["r1_label_reason"] = "time_stop_below_target"
    else:
        base["r1_gray_reason"] = "positive_below_target"
    return base


def project_accepted_lifecycle_row(row: dict[str, Any], *, pnl_target_net_pct: float) -> dict[str, Any]:
    timing = row.get("timing") if isinstance(row.get("timing"), dict) else {}
    shadow = row.get("shadow") if isinstance(row.get("shadow"), dict) else {}
    out = {
        "selector_schema_version": SCHEMA_VERSION,
        "accepted_lifecycle_schema_version": SCHEMA_VERSION,
        "candidate_id": row.get("candidate_id"),
        "position_id": row.get("position_id"),
        "base_mint": row.get("mint_id") or row.get("base_mint"),
        "mint_id": row.get("mint_id") or row.get("base_mint"),
        "pool_id": row.get("pool_id"),
        "quote_mint": row.get("quote_mint"),
        "bonding_curve": row.get("bonding_curve"),
        "analysis_status": row.get("analysis_status"),
        "close_reason": row.get("close_reason"),
        "truth_status": row.get("truth_status"),
        "truth_source": row.get("truth_source"),
        "truth_dataset_kind": row.get("truth_dataset_kind"),
        "sample_price_state": row.get("sample_price_state"),
        "first_seen_ts_ms": timing.get("first_seen_ts_ms") or row.get("first_seen_ts_ms"),
        "curve_t0_event_ts_ms": timing.get("curve_t0_event_ts_ms") or row.get("curve_t0_event_ts_ms"),
        "observation_start_ts_ms": timing.get("observation_start_ts_ms"),
        "observation_end_ts_ms": timing.get("observation_end_ts_ms"),
        "decision_ts_ms": timing.get("decision_ts_ms") or row.get("decision_ts_ms"),
        "entry_execution_ts_ms": timing.get("entry_execution_ts_ms"),
        "close_ts_ms": timing.get("close_ts_ms"),
        "position_duration_ms": timing.get("position_duration_ms"),
        "final_pnl_pct": shadow.get("final_pnl_pct") if "final_pnl_pct" in shadow else row.get("final_pnl_pct"),
        "final_pnl_sol": shadow.get("final_pnl_sol"),
        "execution_outcome": shadow.get("execution_outcome") or row.get("execution_outcome"),
    }
    out.update(classify_r1(row, pnl_target_net_pct=pnl_target_net_pct))
    return out


def row_timestamp_ms(row: dict[str, Any]) -> int | None:
    return source_ts_ms(row)


def candidate_match_keys(row: dict[str, Any]) -> set[str]:
    keys = set()
    for field in ("candidate_id", "base_mint", "mint_id", "pool_id"):
        value = str_or_none(row.get(field)) or str_or_none(find_first_key(row, (field,)))
        if value:
            keys.add(f"{field}:{value}")
    return keys


def index_rows_by_candidate(rows: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        candidate_id = str_or_none(row.get("candidate_id"))
        if candidate_id:
            indexed[candidate_id] = row
    return indexed


def index_events_by_candidate(
    events: Iterable[dict[str, Any]],
    candidates: Iterable[dict[str, Any]],
) -> dict[str, list[dict[str, Any]]]:
    lookup: dict[str, str] = {}
    for candidate in candidates:
        candidate_id = str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            continue
        for key in candidate_match_keys(candidate):
            lookup.setdefault(key, candidate_id)
    indexed: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for event in events:
        matched_id = None
        for key in candidate_match_keys(event):
            matched_id = lookup.get(key)
            if matched_id:
                break
        if matched_id:
            indexed[matched_id].append(event)
    for rows in indexed.values():
        rows.sort(key=lambda item: row_timestamp_ms(item) or 0)
    return indexed


def event_side(row: dict[str, Any]) -> str | None:
    side = str_or_none(find_first_key(row, ("side", "trade_side", "direction")))
    if side:
        normalized = side.lower()
        if "buy" in normalized:
            return "buy"
        if "sell" in normalized:
            return "sell"
    is_buy = bool_or_none(find_first_key(row, ("is_buy", "buy")))
    if is_buy is True:
        return "buy"
    if is_buy is False:
        return "sell"
    return None


def event_quote_amount(row: dict[str, Any]) -> float:
    value = float_or_none(
        find_first_key(
            row,
            (
                "quote_amount",
                "quote_amount_sol",
                "amount_sol",
                "volume_sol",
                "sol_amount",
                "trade_amount_sol",
            ),
        )
    )
    return abs(value) if value is not None else 0.0


def latest_numeric(rows: list[dict[str, Any]], fields: tuple[str, ...]) -> float | None:
    for row in reversed(rows):
        value = float_or_none(find_first_key(row, fields))
        if value is not None:
            return value
    return None


def build_feature_snapshot(
    candidate: dict[str, Any],
    events: list[dict[str, Any]],
    *,
    snapshot_kind: str,
    cutoff_ts_ms: int,
) -> dict[str, Any]:
    birth_ts = int_or_none(candidate.get("birth_ts_ms"))
    cutoff_events = [
        row
        for row in events
        if (row_ts := row_timestamp_ms(row)) is not None and row_ts <= cutoff_ts_ms
    ]
    observed_timestamps = [
        ts for row in cutoff_events if (ts := row_timestamp_ms(row)) is not None
    ]
    observed_slots = [
        slot for row in cutoff_events if (slot := source_slot(row)) is not None
    ]
    latest_ts = max(observed_timestamps) if observed_timestamps else None
    latest_slot = max(observed_slots) if observed_slots else None
    tx_events = [row for row in cutoff_events if event_side(row) in {"buy", "sell"}]
    buys = [row for row in tx_events if event_side(row) == "buy"]
    sells = [row for row in tx_events if event_side(row) == "sell"]
    window_start = birth_ts if birth_ts is not None else cutoff_ts_ms

    def amount_until(ms: int, *, side: str | None = None) -> float:
        if birth_ts is None:
            return 0.0
        end = min(cutoff_ts_ms, birth_ts + ms)
        total = 0.0
        for row in tx_events:
            ts = row_timestamp_ms(row)
            if ts is None or ts < birth_ts or ts > end:
                continue
            if side is not None and event_side(row) != side:
                continue
            sign = 1.0 if event_side(row) == "buy" else -1.0
            total += sign * event_quote_amount(row)
        return total

    unique_buyers = {
        str_or_none(find_first_key(row, ("signer", "buyer", "wallet", "owner", "trader")))
        for row in buys
    }
    unique_buyers.discard(None)
    wallet_amounts: dict[str, float] = defaultdict(float)
    buyer_amounts: dict[str, float] = defaultdict(float)
    total_abs = 0.0
    total_buy = 0.0
    for row in tx_events:
        wallet = str_or_none(find_first_key(row, ("signer", "buyer", "seller", "wallet", "owner", "trader")))
        amount = event_quote_amount(row)
        if wallet:
            wallet_amounts[wallet] += amount
        total_abs += amount
    for row in buys:
        buyer = str_or_none(find_first_key(row, ("signer", "buyer", "wallet", "owner", "trader")))
        amount = event_quote_amount(row)
        if buyer:
            buyer_amounts[buyer] += amount
        total_buy += amount
    creator = str_or_none(find_first_key(candidate, ("creator", "creator_pubkey", "dev_pubkey", "create_user")))
    creator_sold = any(
        event_side(row) == "sell"
        and creator
        and str_or_none(find_first_key(row, ("signer", "seller", "wallet", "owner", "trader"))) == creator
        for row in cutoff_events
    )
    creator_sold = creator_sold or any(
        bool_or_none(find_first_key(row, ("creator_sold_early_flag",))) is True
        for row in cutoff_events
    )
    top1_wallet_share = max(wallet_amounts.values()) / total_abs if total_abs > 0.0 and wallet_amounts else None
    buyer_hhi = (
        sum((amount / total_buy) ** 2 for amount in buyer_amounts.values())
        if total_buy > 0.0 and buyer_amounts
        else None
    )
    elapsed_s = max((cutoff_ts_ms - window_start) / 1000.0, 0.001)
    feature_observed_lag_ms = cutoff_ts_ms - latest_ts if latest_ts is not None else None
    incomplete_reasons = []
    if not cutoff_events:
        incomplete_reasons.append("no_cutoff_events")
    if latest_slot is None:
        incomplete_reasons.append("missing_feature_cutoff_slot")
    if feature_observed_lag_ms is None:
        incomplete_reasons.append("missing_feature_observed_lag_ms")
    elif feature_observed_lag_ms < 0:
        incomplete_reasons.append("feature_source_after_cutoff")
    snapshot_status = "feature_snapshot_incomplete" if incomplete_reasons else "ok"
    snapshot = {
        "selector_schema_version": SCHEMA_VERSION,
        "feature_snapshot_schema_version": SCHEMA_VERSION,
        "candidate_id": candidate.get("candidate_id"),
        "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "pool_id": candidate.get("pool_id"),
        "bonding_curve": candidate.get("bonding_curve"),
        "quote_mint": candidate.get("quote_mint"),
        "quote_mint_is_sol": quote_mint_is_sol(str_or_none(candidate.get("quote_mint"))),
        "snapshot_kind": snapshot_kind,
        "feature_cutoff_ts_ms": cutoff_ts_ms,
        "feature_cutoff_slot": latest_slot,
        "feature_source": "selector_offline_event_rollup",
        "feature_observed_lag_ms": feature_observed_lag_ms,
        "feature_source_max_ts_ms": latest_ts,
        "feature_source_max_slot": latest_slot,
        "feature_snapshot_status": snapshot_status,
        "feature_snapshot_incomplete_reason": "|".join(incomplete_reasons) if incomplete_reasons else None,
        "feature_time_provenance_ok": snapshot_status == "ok",
        "source_event_count": len(cutoff_events),
        "tx_event_count": len(tx_events),
        "curve_progress_pct": latest_numeric(
            cutoff_events,
            ("curve_progress_pct", "bonding_curve_progress_pct", "bonding_curve_progress"),
        ),
        "net_quote_in_15s": amount_until(15_000),
        "net_quote_in_30s": amount_until(30_000),
        "trade_rate": len(tx_events) / elapsed_s,
        "unique_buyers": len(unique_buyers),
        "sell_share": len(sells) / len(tx_events) if tx_events else None,
        "top1_wallet_share": top1_wallet_share,
        "buyer_hhi": buyer_hhi,
        "creator_sold_early_flag": creator_sold,
    }
    assert_no_feature_leakage(snapshot)
    return snapshot


def assert_no_feature_leakage(row: dict[str, Any]) -> None:
    leaked = sorted(DISALLOWED_FEATURE_FIELDS.intersection(row.keys()))
    if leaked:
        raise ValueError(f"feature snapshot contains leakage fields: {', '.join(leaked)}")


def feature_temporal_violations(feature_rows: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    violations: list[dict[str, Any]] = []
    for idx, row in enumerate(feature_rows, start=1):
        candidate_id = row.get("candidate_id")
        leaked = sorted(DISALLOWED_FEATURE_FIELDS.intersection(row.keys()))
        if leaked:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "disallowed_feature_fields",
                    "fields": leaked,
                }
            )
        cutoff = int_or_none(row.get("feature_cutoff_ts_ms"))
        source_max_ts = int_or_none(row.get("feature_source_max_ts_ms"))
        observed_lag = int_or_none(row.get("feature_observed_lag_ms"))
        if cutoff is None:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "missing_feature_cutoff_ts_ms",
                }
            )
        if row.get("feature_source") in (None, ""):
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "missing_feature_source",
                }
            )
        if row.get("feature_cutoff_slot") is None:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "missing_feature_cutoff_slot",
                }
            )
        if observed_lag is None:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "missing_feature_observed_lag_ms",
                }
            )
        elif observed_lag < 0:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "negative_feature_observed_lag_ms",
                    "feature_observed_lag_ms": observed_lag,
                }
            )
        if cutoff is not None and source_max_ts is not None and source_max_ts > cutoff:
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "feature_source_after_cutoff",
                    "feature_cutoff_ts_ms": cutoff,
                    "feature_source_max_ts_ms": source_max_ts,
                }
            )
        if row.get("feature_snapshot_status") != "ok":
            violations.append(
                {
                    "row": idx,
                    "candidate_id": candidate_id,
                    "violation": "feature_snapshot_incomplete",
                    "reason": row.get("feature_snapshot_incomplete_reason"),
                }
            )
    return violations


def price_path_samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = row.get("samples") or row.get("price_path_samples") or row.get("lifecycle_price_samples")
    if not isinstance(raw, list):
        return []
    out: list[dict[str, Any]] = []
    entry_ts = int_or_none(row.get("entry_ts_ms"))
    entry_price = float_or_none(row.get("entry_price") or row.get("entry_price_sol"))
    for item in raw:
        if not isinstance(item, dict):
            continue
        ts_ms = int_or_none(item.get("ts_ms") or item.get("timestamp_ms"))
        offset_ms = int_or_none(item.get("offset_ms"))
        if ts_ms is None and entry_ts is not None and offset_ms is not None:
            ts_ms = entry_ts + offset_ms
        return_pct = float_or_none(item.get("return_pct"))
        price = float_or_none(item.get("price_sol") or item.get("price"))
        if return_pct is None and price is not None and entry_price is not None and entry_price > 0.0:
            return_pct = ((price / entry_price) - 1.0) * 100.0
        if return_pct is None:
            continue
        out.append({"ts_ms": ts_ms, "offset_ms": offset_ms, "return_pct": return_pct, "price": price})
    out.sort(key=lambda item: (item.get("ts_ms") is None, item.get("ts_ms") or item.get("offset_ms") or 0))
    return out


def _has_canonical_r2_marker(source: str) -> bool:
    lowered = source.lower()
    return any(marker in lowered for marker in CANONICAL_R2_SOURCE_MARKERS)


def _source_mentions_rpc(source: str) -> bool:
    return "rpc" in source.lower()


def r2_source_provenance(row: dict[str, Any]) -> tuple[bool, str]:
    for field in ("r2_canonical_source", "canonical_path_source", "canonical_stream_source"):
        explicit = str_or_none(row.get(field))
        if explicit:
            if _source_mentions_rpc(explicit):
                return False, "rpc_backfill_only"
            if _has_canonical_r2_marker(explicit):
                return True, "canonical_stream"
            return False, "noncanonical_path_source"
    source = str(row.get("path_source") or row.get("r2_source") or "").lower()
    if not source:
        return False, "missing_path_source"
    if _source_mentions_rpc(source):
        return False, "rpc_backfill_only"
    if _has_canonical_r2_marker(source):
        return True, "canonical_stream"
    return False, "noncanonical_path_source"


def r2_source_is_canonical(row: dict[str, Any]) -> bool:
    canonical, _reason = r2_source_provenance(row)
    return canonical


def classify_r2(
    price_path: dict[str, Any] | None,
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    if price_path is None:
        return {
            "r2_label": None,
            "r2_status": "missing_path",
            "r2_label_reason": None,
            "r2_excluded_reason": "missing_path",
            "r2_path_coverage_ok": False,
            "r2_horizon_matured": False,
            "r2_source_canonical": False,
            "r2_source_provenance": "missing_path",
        }
    source_canonical, source_provenance = r2_source_provenance(price_path)
    if not source_canonical:
        reason = (
            "rpc_backfill_only_not_r2_ssot"
            if source_provenance == "rpc_backfill_only" or price_path.get("rpc_backfill")
            else source_provenance
        )
        return {
            "r2_label": None,
            "r2_status": "missing_path",
            "r2_label_reason": None,
            "r2_excluded_reason": reason,
            "r2_path_coverage_ok": False,
            "r2_horizon_matured": False,
            "r2_source_canonical": False,
            "r2_source_provenance": source_provenance,
        }
    samples = price_path_samples(price_path)
    coverage = bool_or_none(price_path.get("path_coverage_ok"))
    if coverage is None:
        coverage = price_path.get("path_status") == "ok" and bool(samples)
    if not coverage:
        status = str_or_none(price_path.get("r2_status")) or (
            "missing_path" if not samples else "stream_incomplete"
        )
        return {
            "r2_label": None,
            "r2_status": status,
            "r2_label_reason": None,
            "r2_excluded_reason": status,
            "r2_path_coverage_ok": False,
            "r2_horizon_matured": False,
            "r2_source_canonical": True,
            "r2_source_provenance": source_provenance,
        }
    horizon_matured = bool_or_none(price_path.get("horizon_matured"))
    if horizon_matured is None:
        offsets = [
            offset
            for sample in samples
            if (offset := int_or_none(sample.get("offset_ms"))) is not None
        ]
        max_offset = max(offsets) if offsets else None
        horizon_matured = max_offset is not None and max_offset >= horizon_ms
    if not horizon_matured:
        return {
            "r2_label": None,
            "r2_status": "horizon_unmatured",
            "r2_label_reason": None,
            "r2_excluded_reason": "horizon_unmatured",
            "r2_path_coverage_ok": True,
            "r2_horizon_matured": False,
            "r2_source_canonical": True,
            "r2_source_provenance": source_provenance,
        }
    for sample in samples:
        offset = int_or_none(sample.get("offset_ms"))
        if offset is not None and offset > horizon_ms:
            continue
        ret = float_or_none(sample.get("return_pct"))
        if ret is None:
            continue
        if ret <= -abs(stop_net_pct):
            return {
                "r2_label": "negative",
                "r2_status": "resolved",
                "r2_label_reason": "stop_before_target",
                "r2_excluded_reason": None,
                "r2_path_coverage_ok": True,
                "r2_horizon_matured": True,
                "r2_source_canonical": True,
                "r2_source_provenance": source_provenance,
            }
        if ret >= target_net_pct:
            return {
                "r2_label": "positive",
                "r2_status": "resolved",
                "r2_label_reason": "target_before_stop",
                "r2_excluded_reason": None,
                "r2_path_coverage_ok": True,
                "r2_horizon_matured": True,
                "r2_source_canonical": True,
                "r2_source_provenance": source_provenance,
            }
    return {
        "r2_label": "negative",
        "r2_status": "resolved",
        "r2_label_reason": "no_target_by_horizon",
        "r2_excluded_reason": None,
        "r2_path_coverage_ok": True,
        "r2_horizon_matured": True,
        "r2_source_canonical": True,
        "r2_source_provenance": source_provenance,
    }


def choose_temporal_split(rows: list[dict[str, Any]]) -> dict[str, str]:
    ordered = sorted(
        rows,
        key=lambda row: (
            int_or_none(row.get("birth_ts_ms"))
            or int_or_none(row.get("decision_ts_ms"))
            or 0,
            str(row.get("candidate_id")),
        ),
    )
    total = len(ordered)
    splits: dict[str, str] = {}
    for idx, row in enumerate(ordered):
        candidate_id = str(row.get("candidate_id"))
        frac = idx / total if total else 0.0
        if frac < 0.70:
            split = "train"
        elif frac < 0.85:
            split = "validation"
        else:
            split = "holdout"
        splits[candidate_id] = split
    return splits


def precision_r2_denominator(row: dict[str, Any]) -> bool:
    return bool(
        row.get("split") == "holdout"
        and row.get("selector_accept") is True
        and row.get("cohort_in_scope") is True
        and row.get("stream_completeness_ok") is True
        and row.get("label_resolved") is True
        and row.get("r2_label") in {"positive", "negative"}
    )


def r2_counts(rows: Iterable[dict[str, Any]]) -> dict[str, Any]:
    selected = [row for row in rows if precision_r2_denominator(row)]
    tp = sum(1 for row in selected if row.get("r2_label") == "positive")
    fp = sum(1 for row in selected if row.get("r2_label") == "negative")
    return {
        "selected_count": len(selected),
        "tp_r2": tp,
        "fp_r2": fp,
        "precision_r2": tp / (tp + fp) if (tp + fp) else None,
    }
