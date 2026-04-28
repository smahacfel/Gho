#!/usr/bin/env python3
from __future__ import annotations

import argparse
import bisect
import json
import math
import re
import shutil
import statistics as st
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable, Iterator

from shadow_run_report import (
    BUY_LOG_NAME,
    DEFAULT_CONFIG,
    detect_latest_run_scope,
    derive_shadow_lifecycle_log_path,
    load_toml,
    resolve_config_path,
    resolve_runtime_path,
)

LAMPORTS_PER_SOL = 1_000_000_000
PUMP_TOKEN_DECIMAL_FACTOR = 1_000_000
PUMP_TOKEN_DECIMAL_FACTOR_F64 = float(PUMP_TOKEN_DECIMAL_FACTOR)
LEGACY_PRICE_SCALE = PUMP_TOKEN_DECIMAL_FACTOR_F64
DEFAULT_MAX_BUY_MATCH_DRIFT_MS = 60_000
ISO_TS_RE = re.compile(
    r"^(?P<head>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?P<fraction>\.\d+)?(?P<tz>Z|[+-]\d{2}:\d{2})$"
)
DIAG_ACCOUNT_UPDATE_RELAY_RE = re.compile(
    r"^(?P<timestamp>\S+).*\bDIAG_ACCOUNT_UPDATE_RELAY\b "
    r"base_mint=(?P<base_mint>\S+) bonding_curve=(?P<bonding_curve>\S+) "
    r"slot=(?P<slot>\d+) sol_reserves=(?P<sol_reserves>\d+) "
    r"token_reserves=(?P<token_reserves>\d+) complete=(?P<complete>\d+) "
    r"curve_finality=(?P<curve_finality>\S+)"
)


@dataclass(slots=True)
class Inputs:
    config_path: Path
    gatekeeper_buys_log: Path
    shadow_transport_log: Path
    shadow_entry_log: Path
    shadow_lifecycle_log: Path
    events_dir: Path
    system_log_base: Path
    session_run_id: str | None
    session_start_ms: int | None
    session_end_ms: int | None
    output_path: Path
    max_truth_gap_ms: int | None


@dataclass(slots=True)
class ShadowTransportRecord:
    candidate_id: str
    base_mint: str
    pool_id: str
    decision_ts_ms: int | None
    sim_started_ts_ms: int | None
    sim_finished_ts_ms: int | None
    amount_lamports: int | None
    error_class: str | None
    live_signature: str | None


@dataclass(slots=True)
class ShadowEntryRecord:
    candidate_id: str
    pool_id: str
    mint_id: str
    entry_price: float | None
    slot: int | None
    timestamp_ms: int | None
    execution_outcome: str | None


@dataclass(slots=True)
class ExitFillRow:
    ordinal: int
    candidate_id: str
    position_id: str | None
    pool_id: str
    mint_id: str
    timestamp_ms: int | None
    sample_timestamp_ms: int | None
    sample_slot: int | None
    sample_age_ms: int | None
    fraction_bps: int | None
    remaining_fraction_bps: int | None
    entry_price: float | None
    exit_price: float | None
    entry_value_sol: float | None
    exit_value_sol: float | None
    truth_status: str | None
    truth_source: str | None
    sample_price_state: str | None


@dataclass(slots=True)
class PositionClosedRow:
    candidate_id: str
    position_id: str | None
    pool_id: str
    mint_id: str
    timestamp_ms: int | None
    sample_timestamp_ms: int | None
    sample_slot: int | None
    entry_price: float | None
    entry_value_sol: float | None
    exit_value_sol: float | None
    gross_pnl_sol: float | None
    net_pnl_sol: float | None
    estimated_costs_sol: float | None
    final_pnl: float | None
    final_pnl_pct: float | None
    duration_ms: int | None
    close_reason: str | None
    total_exits: int | None
    truth_status: str | None
    truth_source: str | None
    sample_price_state: str | None


@dataclass(slots=True)
class LifecycleBundle:
    candidate_id: str
    exit_fills: list[ExitFillRow] = field(default_factory=list)
    position_closed: PositionClosedRow | None = None


@dataclass(slots=True)
class ScopeStats:
    candidate_count: int
    first_candidate_ts_ms: int | None
    latest_candidate_ts_ms: int | None
    closed_positions: int
    resolved_close_truth: int
    failed_close_truth: int
    latest_close_ts_ms: int | None


@dataclass(slots=True)
class GatekeeperBuyRow:
    pool_id: str
    base_mint: str
    first_seen_ts_ms: int | None
    observation_start_ts_ms: int | None
    observation_end_ts_ms: int | None
    curve_t0_event_ts_ms: int | None
    timestamp_ms: int | None
    shadow_execution_outcome: str | None
    decision_verdict_buy: bool
    verdict_type: str | None
    decision_reason: str | None


@dataclass(slots=True)
class DiagUpdate:
    timestamp_ms: int
    base_mint: str
    bonding_curve: str
    slot: int
    sol_reserves_lamports: int
    token_reserves_raw: int
    complete: int
    curve_finality: str

    def spot_price_sol(self) -> float | None:
        if self.sol_reserves_lamports <= 0 or self.token_reserves_raw <= 0:
            return None
        return lamports_to_sol(self.sol_reserves_lamports) / raw_to_display_tokens(
            self.token_reserves_raw
        )


@dataclass(slots=True)
class MatchedTruth:
    update: DiagUpdate
    delta_ms: int
    direction: str


@dataclass(slots=True)
class DiagTimeline:
    timestamps_ms: list[int]
    updates: list[DiagUpdate]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Correlate full-lifecycle shadow positions with historical DIAG_ACCOUNT_UPDATE_RELAY "
            "truth and write one JSONL row per analyzed position."
        )
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Launcher config used for the session (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help=(
            "Output JSONL path. When omitted, the script writes next to shadow_entries.jsonl "
            "using the active session scope in the file name."
        ),
    )
    parser.add_argument(
        "--session-start-ms",
        type=int,
        help=(
            "Optional lower bound for analyzed candidate_ids. "
            "When omitted, the script defaults to the latest event-bus run."
        ),
    )
    parser.add_argument(
        "--session-end-ms",
        type=int,
        help="Optional upper bound for analyzed candidate_ids.",
    )
    parser.add_argument(
        "--all-sessions",
        action="store_true",
        help="Analyze all available sessions instead of defaulting to the latest run.",
    )
    parser.add_argument(
        "--max-truth-gap-ms",
        type=int,
        help=(
            "Optional hard filter for DIAG match distance. "
            "Rows with entry or exit truth farther away than this threshold are skipped."
        ),
    )
    return parser.parse_args()


def resolve_inputs(args: argparse.Namespace) -> Inputs:
    config_path = resolve_config_path(args.config)
    config = load_toml(config_path)
    shadow_cfg = config.get("execution", {}).get("shadow", {})
    trigger_shadow_cfg = config.get("trigger", {}).get("shadow_run", {})
    decision_dir = resolve_runtime_path(
        config_path,
        config.get("oracle", {}).get("decision_log_path", "logs/decisions"),
    )
    shadow_entry_log = resolve_runtime_path(
        config_path,
        shadow_cfg.get("entry_log_path", "logs/shadow_run/shadow_entries.jsonl"),
    )
    shadow_lifecycle_log = resolve_runtime_path(
        config_path,
        shadow_cfg.get("lifecycle_log_path") or derive_shadow_lifecycle_log_path(shadow_entry_log),
    )
    shadow_transport_log = resolve_runtime_path(
        config_path,
        trigger_shadow_cfg.get("output_path", "logs/shadow_run/buys.jsonl"),
    )
    events_dir = resolve_runtime_path(
        config_path,
        config.get("execution", {}).get("events", {}).get("output_dir", "datasets/events"),
    )
    system_log_base = resolve_runtime_path(
        config_path, config.get("logging", {}).get("file_path", "logs/system.log")
    )
    session_run_id = None
    session_start_ms = args.session_start_ms
    if session_start_ms is None and not args.all_sessions:
        session_run_id, session_start_ms = detect_latest_run_scope(events_dir)
    session_end_ms = args.session_end_ms
    if session_start_ms is not None and session_end_ms is not None and session_end_ms < session_start_ms:
        raise SystemExit("--session-end-ms must be >= --session-start-ms")
    if args.output is None:
        scope = str(session_start_ms) if session_start_ms is not None else "all"
        output_path = shadow_entry_log.parent / f"shadow_onchain_lifecycle_report_{scope}.jsonl"
    else:
        output_path = resolve_runtime_path(config_path, str(args.output))
    return Inputs(
        config_path=config_path,
        gatekeeper_buys_log=decision_dir / BUY_LOG_NAME,
        shadow_transport_log=shadow_transport_log,
        shadow_entry_log=shadow_entry_log,
        shadow_lifecycle_log=shadow_lifecycle_log,
        events_dir=events_dir,
        system_log_base=system_log_base,
        session_run_id=session_run_id,
        session_start_ms=session_start_ms,
        session_end_ms=session_end_ms,
        output_path=output_path,
        max_truth_gap_ms=args.max_truth_gap_ms,
    )


def parse_iso_to_ms(value: str | None) -> int | None:
    if not isinstance(value, str):
        return None
    match = ISO_TS_RE.match(value.strip())
    if not match:
        return None
    head = match.group("head")
    fraction = match.group("fraction") or ""
    if fraction:
        fraction = "." + fraction[1:7].ljust(6, "0")
    tz = match.group("tz")
    if tz == "Z":
        tz = "+00:00"
    try:
        return int(datetime.fromisoformat(f"{head}{fraction}{tz}").timestamp() * 1000)
    except ValueError:
        return None


def parse_candidate_id(candidate_id: str | None) -> tuple[str, str, int] | None:
    if not isinstance(candidate_id, str):
        return None
    parts = candidate_id.rsplit("_", 2)
    if len(parts) != 3 or not parts[2].isdigit():
        return None
    return parts[0], parts[1], int(parts[2])


def extract_candidate_ts_ms(candidate_id: str | None) -> int | None:
    parsed = parse_candidate_id(candidate_id)
    return parsed[2] if parsed else None


def in_window(
    candidate_id: str | None,
    row_ts_ms: int | None,
    session_start_ms: int | None,
    session_end_ms: int | None,
) -> bool:
    candidate_ts_ms = extract_candidate_ts_ms(candidate_id)
    effective_ts = next((ts for ts in (candidate_ts_ms, row_ts_ms) if ts is not None), None)
    if session_start_ms is not None and (effective_ts is None or effective_ts < session_start_ms):
        return False
    if session_end_ms is not None and (effective_ts is None or effective_ts > session_end_ms):
        return False
    return True


def iter_jsonl_rows(path: Path) -> Iterator[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def iter_json_concat_rows(path: Path) -> Iterator[dict[str, Any]]:
    if not path.exists():
        return
    data = path.read_text(encoding="utf-8", errors="ignore")
    decoder = json.JSONDecoder()
    index = 0
    length = len(data)
    while index < length:
        while index < length and data[index].isspace():
            index += 1
        if index >= length:
            break
        try:
            row, next_index = decoder.raw_decode(data, index)
        except json.JSONDecodeError:
            index += 1
            continue
        if isinstance(row, dict):
            yield row
        index = next_index


def float_or_none(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def int_or_none(value: Any) -> int | None:
    return int(value) if isinstance(value, (int, float)) else None


def lamports_to_sol(value: int | float) -> float:
    return float(value) / LAMPORTS_PER_SOL


def raw_to_display_tokens(value: int | float) -> float:
    return float(value) / PUMP_TOKEN_DECIMAL_FACTOR_F64


def display_to_raw_tokens(value: float) -> int:
    return max(1, int(round(value * PUMP_TOKEN_DECIMAL_FACTOR_F64)))


def pct_diff(actual: float | None, reference: float | None) -> float | None:
    if actual is None or reference is None or not math.isfinite(actual) or not math.isfinite(reference):
        return None
    if reference == 0.0:
        return None
    return ((actual / reference) - 1.0) * 100.0


def choose_shadow_price_multiplier(logged_price: float, reference_price: float | None) -> float:
    if reference_price is None or reference_price <= 0.0 or not math.isfinite(reference_price):
        return LEGACY_PRICE_SCALE if logged_price < 1e-10 else 1.0
    candidates = (1.0, LEGACY_PRICE_SCALE)

    def score(multiplier: float) -> float:
        normalized = logged_price * multiplier
        if normalized <= 0.0 or not math.isfinite(normalized):
            return float("inf")
        ratio = normalized / reference_price
        if ratio <= 0.0 or not math.isfinite(ratio):
            return float("inf")
        return abs(math.log10(ratio))

    return min(candidates, key=score)


def shadow_price_with_multiplier(price: float | None, multiplier: float) -> float | None:
    if price is None or price <= 0.0 or not math.isfinite(price):
        return None
    return price * multiplier


def simulate_buy_tokens_raw(sol_in_lamports: int, update: DiagUpdate) -> int:
    if sol_in_lamports <= 0 or update.sol_reserves_lamports <= 0 or update.token_reserves_raw <= 0:
        return 0
    fee = sol_in_lamports // 100
    effective_sol = sol_in_lamports - fee
    invariant = update.sol_reserves_lamports * update.token_reserves_raw
    new_sol_reserves = update.sol_reserves_lamports + effective_sol
    if new_sol_reserves <= 0:
        return 0
    new_token_reserves = invariant // new_sol_reserves
    tokens_out = update.token_reserves_raw - new_token_reserves
    return max(0, tokens_out)


def calculate_sell_sol_out_lamports(tokens_in_raw: int, update: DiagUpdate) -> int:
    if tokens_in_raw <= 0 or update.sol_reserves_lamports <= 0 or update.token_reserves_raw <= 0:
        return 0
    invariant = update.sol_reserves_lamports * update.token_reserves_raw
    new_token_reserves = update.token_reserves_raw + tokens_in_raw
    if new_token_reserves <= 0:
        return 0
    new_sol_reserves = invariant // new_token_reserves
    sol_out = update.sol_reserves_lamports - new_sol_reserves
    fee = sol_out // 100
    return max(0, sol_out - fee)


def buy_executable_price_sol(sol_in_lamports: int, update: DiagUpdate) -> tuple[float | None, int]:
    tokens_out_raw = simulate_buy_tokens_raw(sol_in_lamports, update)
    if tokens_out_raw <= 0:
        return None, 0
    price_sol = lamports_to_sol(sol_in_lamports) / raw_to_display_tokens(tokens_out_raw)
    return price_sol, tokens_out_raw


def sell_executable_price_sol(tokens_in_raw: int, update: DiagUpdate) -> tuple[float | None, int]:
    sol_out_lamports = calculate_sell_sol_out_lamports(tokens_in_raw, update)
    if sol_out_lamports <= 0 or tokens_in_raw <= 0:
        return None, 0
    price_sol = lamports_to_sol(sol_out_lamports) / raw_to_display_tokens(tokens_in_raw)
    return price_sol, sol_out_lamports


def load_shadow_transport_records(inputs: Inputs) -> dict[str, ShadowTransportRecord]:
    records: dict[str, ShadowTransportRecord] = {}
    for row in iter_jsonl_rows(inputs.shadow_transport_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        decision_ts_ms = int_or_none(row.get("decision_ts_ms"))
        if not in_window(candidate_id, decision_ts_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        records[candidate_id] = ShadowTransportRecord(
            candidate_id=candidate_id,
            base_mint=str(row.get("base_mint", "")),
            pool_id=str(row.get("pool_amm_id", "")),
            decision_ts_ms=decision_ts_ms,
            sim_started_ts_ms=int_or_none(row.get("sim_started_ts_ms")),
            sim_finished_ts_ms=int_or_none(row.get("sim_finished_ts_ms")),
            amount_lamports=int_or_none(row.get("amount_lamports")),
            error_class=row.get("error_class") if isinstance(row.get("error_class"), str) else None,
            live_signature=row.get("live_signature")
            if isinstance(row.get("live_signature"), str)
            else None,
        )
    return records


def load_shadow_entries(inputs: Inputs) -> dict[str, ShadowEntryRecord]:
    records: dict[str, ShadowEntryRecord] = {}
    for row in iter_json_concat_rows(inputs.shadow_entry_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        timestamp_ms = int_or_none(row.get("timestamp_ms"))
        if not in_window(candidate_id, timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        records[candidate_id] = ShadowEntryRecord(
            candidate_id=candidate_id,
            pool_id=str(row.get("pool_id", "")),
            mint_id=str(row.get("mint_id", "")),
            entry_price=float_or_none(row.get("entry_price")),
            slot=int_or_none(row.get("slot")),
            timestamp_ms=timestamp_ms,
            execution_outcome=row.get("execution_outcome")
            if isinstance(row.get("execution_outcome"), str)
            else None,
        )
    return records


def load_lifecycle(inputs: Inputs) -> dict[str, LifecycleBundle]:
    bundles: dict[str, LifecycleBundle] = {}
    exit_ordinal = 0
    for row in iter_json_concat_rows(inputs.shadow_lifecycle_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        timestamp_ms = int_or_none(row.get("timestamp_ms"))
        if not in_window(candidate_id, timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        record_type = row.get("record_type")
        if not isinstance(record_type, str):
            continue
        bundle = bundles.setdefault(candidate_id, LifecycleBundle(candidate_id=candidate_id))
        if record_type == "exit_filled":
            bundle.exit_fills.append(
                ExitFillRow(
                    ordinal=exit_ordinal,
                    candidate_id=candidate_id,
                    position_id=row.get("position_id")
                    if isinstance(row.get("position_id"), str)
                    else None,
                    pool_id=str(row.get("pool_id", "")),
                    mint_id=str(row.get("mint_id", "")),
                    timestamp_ms=timestamp_ms,
                    sample_timestamp_ms=int_or_none(row.get("sample_timestamp_ms")),
                    sample_slot=int_or_none(row.get("sample_slot")),
                    sample_age_ms=int_or_none(row.get("sample_age_ms")),
                    fraction_bps=int_or_none(row.get("fraction_bps")),
                    remaining_fraction_bps=int_or_none(row.get("remaining_fraction_bps")),
                    entry_price=float_or_none(row.get("entry_price")),
                    exit_price=float_or_none(row.get("exit_price")),
                    entry_value_sol=float_or_none(row.get("entry_value_sol")),
                    exit_value_sol=float_or_none(row.get("exit_value_sol")),
                    truth_status=row.get("truth_status")
                    if isinstance(row.get("truth_status"), str)
                    else None,
                    truth_source=row.get("truth_source")
                    if isinstance(row.get("truth_source"), str)
                    else None,
                    sample_price_state=row.get("sample_price_state")
                    if isinstance(row.get("sample_price_state"), str)
                    else None,
                )
            )
            exit_ordinal += 1
        elif record_type == "position_closed":
            bundle.position_closed = PositionClosedRow(
                candidate_id=candidate_id,
                position_id=row.get("position_id") if isinstance(row.get("position_id"), str) else None,
                pool_id=str(row.get("pool_id", "")),
                mint_id=str(row.get("mint_id", "")),
                timestamp_ms=timestamp_ms,
                sample_timestamp_ms=int_or_none(row.get("sample_timestamp_ms")),
                sample_slot=int_or_none(row.get("sample_slot")),
                entry_price=float_or_none(row.get("entry_price")),
                entry_value_sol=float_or_none(row.get("entry_value_sol")),
                exit_value_sol=float_or_none(row.get("exit_value_sol")),
                gross_pnl_sol=float_or_none(row.get("gross_pnl_sol")),
                net_pnl_sol=float_or_none(row.get("net_pnl_sol")),
                estimated_costs_sol=float_or_none(row.get("estimated_costs_sol")),
                final_pnl=float_or_none(row.get("final_pnl")),
                final_pnl_pct=float_or_none(row.get("final_pnl_pct")),
                duration_ms=int_or_none(row.get("duration_ms")),
                close_reason=row.get("close_reason")
                if isinstance(row.get("close_reason"), str)
                else None,
                total_exits=int_or_none(row.get("total_exits")),
                truth_status=row.get("truth_status")
                if isinstance(row.get("truth_status"), str)
                else None,
                truth_source=row.get("truth_source")
                if isinstance(row.get("truth_source"), str)
                else None,
                sample_price_state=row.get("sample_price_state")
                if isinstance(row.get("sample_price_state"), str)
                else None,
            )
    return bundles


def build_scope_stats(
    transport_by_candidate: dict[str, ShadowTransportRecord],
    entry_by_candidate: dict[str, ShadowEntryRecord],
    lifecycle_by_candidate: dict[str, LifecycleBundle],
) -> ScopeStats:
    candidate_ids = (
        set(transport_by_candidate.keys())
        | set(entry_by_candidate.keys())
        | set(lifecycle_by_candidate.keys())
    )
    candidate_timestamps = sorted(
        ts
        for ts in (
            extract_candidate_ts_ms(candidate_id) for candidate_id in candidate_ids
        )
        if ts is not None
    )

    closed_positions = 0
    resolved_close_truth = 0
    failed_close_truth = 0
    latest_close_ts_ms: int | None = None

    for bundle in lifecycle_by_candidate.values():
        closed = bundle.position_closed
        if closed is None:
            continue
        closed_positions += 1
        if closed.truth_status == "resolved":
            resolved_close_truth += 1
        elif closed.truth_status == "failure":
            failed_close_truth += 1
        close_ts_ms = next(
            (
                ts
                for ts in (
                    closed.timestamp_ms,
                    closed.sample_timestamp_ms,
                    extract_candidate_ts_ms(bundle.candidate_id),
                )
                if ts is not None
            ),
            None,
        )
        if close_ts_ms is not None and (
            latest_close_ts_ms is None or close_ts_ms > latest_close_ts_ms
        ):
            latest_close_ts_ms = close_ts_ms

    return ScopeStats(
        candidate_count=len(candidate_ids),
        first_candidate_ts_ms=candidate_timestamps[0] if candidate_timestamps else None,
        latest_candidate_ts_ms=candidate_timestamps[-1] if candidate_timestamps else None,
        closed_positions=closed_positions,
        resolved_close_truth=resolved_close_truth,
        failed_close_truth=failed_close_truth,
        latest_close_ts_ms=latest_close_ts_ms,
    )


def load_gatekeeper_buys(inputs: Inputs) -> dict[tuple[str, str], list[GatekeeperBuyRow]]:
    rows_by_key: dict[tuple[str, str], list[GatekeeperBuyRow]] = defaultdict(list)
    for row in iter_jsonl_rows(inputs.gatekeeper_buys_log):
        base_mint = row.get("base_mint")
        pool_id = row.get("pool_id")
        if not isinstance(base_mint, str) or not isinstance(pool_id, str):
            continue
        first_seen_ts_ms = int_or_none(row.get("first_seen_ts_ms"))
        timestamp_ms = parse_iso_to_ms(row.get("timestamp"))
        if not in_window(None, first_seen_ts_ms or timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        rows_by_key[(base_mint, pool_id)].append(
            GatekeeperBuyRow(
                pool_id=pool_id,
                base_mint=base_mint,
                first_seen_ts_ms=first_seen_ts_ms,
                observation_start_ts_ms=int_or_none(row.get("observation_start_ts_ms")),
                observation_end_ts_ms=int_or_none(row.get("observation_end_ts_ms")),
                curve_t0_event_ts_ms=int_or_none(row.get("curve_t0_event_ts_ms")),
                timestamp_ms=timestamp_ms,
                shadow_execution_outcome=row.get("shadow_execution_outcome")
                if isinstance(row.get("shadow_execution_outcome"), str)
                else None,
                decision_verdict_buy=bool(row.get("decision_verdict_buy")),
                verdict_type=row.get("verdict_type")
                if isinstance(row.get("verdict_type"), str)
                else None,
                decision_reason=row.get("decision_reason")
                if isinstance(row.get("decision_reason"), str)
                else None,
            )
        )
    for rows in rows_by_key.values():
        rows.sort(
            key=lambda row: (
                row.timestamp_ms if row.timestamp_ms is not None else -1,
                row.first_seen_ts_ms if row.first_seen_ts_ms is not None else -1,
            )
        )
    return rows_by_key


def iter_system_log_paths(base_path: Path) -> list[Path]:
    candidates = [
        path
        for path in base_path.parent.glob(f"{base_path.name}*")
        if path.is_file() and path.name.startswith(base_path.name)
    ]
    candidates.sort(key=lambda path: path.name)
    return candidates


def load_diag_updates(
    system_log_paths: Iterable[Path], relevant_mints: set[str]
) -> dict[str, DiagTimeline]:
    updates_by_mint: dict[str, list[DiagUpdate]] = defaultdict(list)
    if relevant_mints:
        rg_updates = load_diag_updates_with_rg(system_log_paths, relevant_mints)
        if rg_updates is not None:
            return rg_updates
    for path in system_log_paths:
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
                if not match:
                    continue
                base_mint = match.group("base_mint")
                if relevant_mints and base_mint not in relevant_mints:
                    continue
                timestamp_ms = parse_iso_to_ms(match.group("timestamp"))
                if timestamp_ms is None:
                    continue
                updates_by_mint[base_mint].append(
                    DiagUpdate(
                        timestamp_ms=timestamp_ms,
                        base_mint=base_mint,
                        bonding_curve=match.group("bonding_curve"),
                        slot=int(match.group("slot")),
                        sol_reserves_lamports=int(match.group("sol_reserves")),
                        token_reserves_raw=int(match.group("token_reserves")),
                        complete=int(match.group("complete")),
                        curve_finality=match.group("curve_finality"),
                    )
                )
    for updates in updates_by_mint.values():
        updates.sort(key=lambda update: update.timestamp_ms)
    return {
        mint: DiagTimeline(
            timestamps_ms=[update.timestamp_ms for update in updates],
            updates=updates,
        )
        for mint, updates in updates_by_mint.items()
    }


def load_diag_updates_with_rg(
    system_log_paths: Iterable[Path], relevant_mints: set[str]
) -> dict[str, DiagTimeline] | None:
    rg_path = shutil.which("rg")
    paths = [path for path in system_log_paths if path.exists()]
    if rg_path is None or not paths or not relevant_mints:
        return None
    mint_pattern = "|".join(sorted(re.escape(mint) for mint in relevant_mints))
    pattern = rf"DIAG_ACCOUNT_UPDATE_RELAY base_mint=({mint_pattern}) "
    command = [rg_path, "-a", "--no-heading", "-H", "-e", pattern, *[str(path) for path in paths]]
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
    except OSError:
        return None
    if completed.returncode not in (0, 1):
        return None

    updates_by_mint: dict[str, list[DiagUpdate]] = defaultdict(list)
    for raw_line in completed.stdout.splitlines():
        _, _, line = raw_line.partition(":")
        match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
        if not match:
            continue
        timestamp_ms = parse_iso_to_ms(match.group("timestamp"))
        if timestamp_ms is None:
            continue
        base_mint = match.group("base_mint")
        updates_by_mint[base_mint].append(
            DiagUpdate(
                timestamp_ms=timestamp_ms,
                base_mint=base_mint,
                bonding_curve=match.group("bonding_curve"),
                slot=int(match.group("slot")),
                sol_reserves_lamports=int(match.group("sol_reserves")),
                token_reserves_raw=int(match.group("token_reserves")),
                complete=int(match.group("complete")),
                curve_finality=match.group("curve_finality"),
            )
        )
    for updates in updates_by_mint.values():
        updates.sort(key=lambda update: update.timestamp_ms)
    return {
        mint: DiagTimeline(
            timestamps_ms=[update.timestamp_ms for update in updates],
            updates=updates,
        )
        for mint, updates in updates_by_mint.items()
    }


def match_gatekeeper_buy(
    rows: list[GatekeeperBuyRow],
    decision_ts_ms: int | None,
    execution_ts_ms: int | None,
) -> GatekeeperBuyRow | None:
    if not rows:
        return None
    anchor_ts_ms = next((value for value in (execution_ts_ms, decision_ts_ms) if value is not None), None)
    if anchor_ts_ms is None:
        return None
    best: GatekeeperBuyRow | None = None
    best_score: tuple[int, int, int] | None = None
    for row in rows:
        if not row.decision_verdict_buy:
            continue
        row_anchor_ts = next(
            (
                value
                for value in (
                    row.timestamp_ms,
                    row.observation_end_ts_ms,
                    row.first_seen_ts_ms,
                )
                if value is not None
            ),
            None,
        )
        if row_anchor_ts is None:
            continue
        if abs(row_anchor_ts - anchor_ts_ms) > DEFAULT_MAX_BUY_MATCH_DRIFT_MS:
            continue
        future_penalty = 0
        if decision_ts_ms is not None and row.first_seen_ts_ms is not None and row.first_seen_ts_ms > decision_ts_ms:
            future_penalty = row.first_seen_ts_ms - decision_ts_ms
        outcome_penalty = 0 if row.shadow_execution_outcome == "shadow_simulated" else 1
        score = (future_penalty, outcome_penalty, abs(row_anchor_ts - anchor_ts_ms))
        if best_score is None or score < best_score:
            best = row
            best_score = score
    return best


def find_causal_truth(timeline: DiagTimeline | None, target_ts_ms: int | None) -> MatchedTruth | None:
    if timeline is None or not timeline.updates or target_ts_ms is None:
        return None
    index = bisect.bisect_right(timeline.timestamps_ms, target_ts_ms) - 1
    if index >= 0:
        update = timeline.updates[index]
    else:
        update = timeline.updates[0]
    delta_ms = target_ts_ms - update.timestamp_ms
    direction = "before" if delta_ms >= 0 else "after"
    return MatchedTruth(update=update, delta_ms=delta_ms, direction=direction)


def analyze_positions(
    inputs: Inputs,
    transport_by_candidate: dict[str, ShadowTransportRecord],
    entry_by_candidate: dict[str, ShadowEntryRecord],
    lifecycle_by_candidate: dict[str, LifecycleBundle],
    gatekeeper_buys_by_key: dict[tuple[str, str], list[GatekeeperBuyRow]],
    diag_updates_by_mint: dict[str, DiagTimeline],
) -> tuple[list[dict[str, Any]], Counter[str]]:
    rows: list[dict[str, Any]] = []
    skipped = Counter()
    for candidate_id in sorted(lifecycle_by_candidate, key=lambda value: extract_candidate_ts_ms(value) or 0):
        bundle = lifecycle_by_candidate[candidate_id]
        closed = bundle.position_closed
        if closed is None:
            skipped["missing_position_closed"] += 1
            continue
        if closed.truth_status != "resolved":
            skipped["close_truth_not_resolved"] += 1
            continue
        resolved_exit_fills = [fill for fill in bundle.exit_fills if fill.truth_status == "resolved"]
        if not resolved_exit_fills:
            skipped["no_resolved_exit_fills"] += 1
            continue

        transport = transport_by_candidate.get(candidate_id)
        if transport is None:
            skipped["missing_transport_record"] += 1
            continue
        if transport.error_class:
            skipped["transport_record_has_error"] += 1
            continue

        parsed_candidate = parse_candidate_id(candidate_id)
        if parsed_candidate is None:
            skipped["unparseable_candidate_id"] += 1
            continue
        mint_id, pool_id, _ = parsed_candidate
        if closed.mint_id and closed.mint_id != mint_id:
            mint_id = closed.mint_id
        if closed.pool_id and closed.pool_id != pool_id:
            pool_id = closed.pool_id

        entry = entry_by_candidate.get(candidate_id)
        raw_shadow_entry_price = next(
            (
                price
                for price in (
                    entry.entry_price if entry is not None else None,
                    closed.entry_price,
                    resolved_exit_fills[0].entry_price,
                )
                if price is not None and price > 0.0 and math.isfinite(price)
            ),
            None,
        )
        if raw_shadow_entry_price is None:
            skipped["missing_entry_price"] += 1
            continue
        entry_value_sol = next(
            (
                value
                for value in (
                    closed.entry_value_sol,
                    lamports_to_sol(transport.amount_lamports) if transport.amount_lamports else None,
                )
                if value is not None and value > 0.0 and math.isfinite(value)
            ),
            None,
        )
        if entry_value_sol is None:
            skipped["missing_entry_value"] += 1
            continue

        entry_execution_ts_ms = next(
            (
                value
                for value in (
                    transport.sim_finished_ts_ms,
                    closed.timestamp_ms - closed.duration_ms
                    if closed.timestamp_ms is not None and closed.duration_ms is not None
                    else None,
                    entry.timestamp_ms if entry is not None else None,
                    transport.decision_ts_ms,
                )
                if value is not None
            ),
            None,
        )
        if entry_execution_ts_ms is None:
            skipped["missing_entry_execution_ts"] += 1
            continue

        gatekeeper_buy = match_gatekeeper_buy(
            gatekeeper_buys_by_key.get((mint_id, pool_id), []),
            transport.decision_ts_ms,
            entry_execution_ts_ms,
        )
        mint_timeline = diag_updates_by_mint.get(mint_id)
        if mint_timeline is None:
            skipped["missing_diag_updates"] += 1
            continue

        entry_truth = find_causal_truth(mint_timeline, entry_execution_ts_ms)
        if entry_truth is None:
            skipped["missing_entry_truth"] += 1
            continue
        if inputs.max_truth_gap_ms is not None and abs(entry_truth.delta_ms) > inputs.max_truth_gap_ms:
            skipped["entry_truth_too_far"] += 1
            continue

        trade_amount_lamports = next(
            (
                value
                for value in (
                    transport.amount_lamports,
                    int(round(entry_value_sol * LAMPORTS_PER_SOL)),
                )
                if value is not None and value > 0
            ),
            None,
        )
        if trade_amount_lamports is None:
            skipped["missing_trade_amount_lamports"] += 1
            continue

        onchain_entry_exec_price_sol, onchain_entry_tokens_raw = buy_executable_price_sol(
            trade_amount_lamports, entry_truth.update
        )
        if onchain_entry_exec_price_sol is None or onchain_entry_tokens_raw <= 0:
            skipped["entry_executable_quote_failed"] += 1
            continue

        entry_multiplier = choose_shadow_price_multiplier(
            raw_shadow_entry_price, onchain_entry_exec_price_sol
        )
        shadow_entry_price_sol = shadow_price_with_multiplier(raw_shadow_entry_price, entry_multiplier)
        if shadow_entry_price_sol is None or shadow_entry_price_sol <= 0.0:
            skipped["invalid_shadow_entry_price"] += 1
            continue

        shadow_entry_tokens_display = entry_value_sol / shadow_entry_price_sol
        if not math.isfinite(shadow_entry_tokens_display) or shadow_entry_tokens_display <= 0.0:
            skipped["invalid_shadow_entry_token_qty"] += 1
            continue
        shadow_entry_tokens_raw_estimated = display_to_raw_tokens(shadow_entry_tokens_display)

        exit_fill_rows: list[dict[str, Any]] = []
        shadow_exit_value_sum = 0.0
        onchain_exit_value_sum = 0.0
        total_exit_tokens_display = 0.0
        weighted_onchain_exit_spot_sum = 0.0
        weighted_shadow_exit_spot_invariant_ok = True
        exit_truth_gap_values: list[int] = []

        for fill_index, fill in enumerate(sorted(resolved_exit_fills, key=lambda row: row.ordinal), start=1):
            fill_entry_value_sol = next(
                (
                    value
                    for value in (
                        fill.entry_value_sol,
                        entry_value_sol * ((fill.fraction_bps or 0) / 10_000.0)
                        if fill.fraction_bps is not None
                        else None,
                    )
                    if value is not None and value > 0.0 and math.isfinite(value)
                ),
                None,
            )
            if fill_entry_value_sol is None:
                skipped["missing_exit_fill_entry_value"] += 1
                exit_fill_rows = []
                break

            fill_tokens_display = fill_entry_value_sol / shadow_entry_price_sol
            if not math.isfinite(fill_tokens_display) or fill_tokens_display <= 0.0:
                skipped["invalid_exit_fill_token_qty"] += 1
                exit_fill_rows = []
                break
            fill_tokens_raw = display_to_raw_tokens(fill_tokens_display)
            fill_target_ts_ms = next(
                (value for value in (fill.sample_timestamp_ms, fill.timestamp_ms) if value is not None),
                None,
            )
            exit_truth = find_causal_truth(mint_timeline, fill_target_ts_ms)
            if exit_truth is None:
                skipped["missing_exit_truth"] += 1
                exit_fill_rows = []
                break
            if inputs.max_truth_gap_ms is not None and abs(exit_truth.delta_ms) > inputs.max_truth_gap_ms:
                skipped["exit_truth_too_far"] += 1
                exit_fill_rows = []
                break

            onchain_exit_exec_price_sol, onchain_exit_sol_out_lamports = sell_executable_price_sol(
                fill_tokens_raw, exit_truth.update
            )
            if onchain_exit_exec_price_sol is None or onchain_exit_sol_out_lamports <= 0:
                skipped["exit_executable_quote_failed"] += 1
                exit_fill_rows = []
                break

            shadow_fill_exit_price_sol = shadow_price_with_multiplier(fill.exit_price, entry_multiplier)
            if shadow_fill_exit_price_sol is None:
                shadow_fill_exit_price_sol = (
                    fill.exit_value_sol / fill_tokens_display if fill.exit_value_sol is not None else None
                )
            shadow_fill_exit_value_sol = next(
                (
                    value
                    for value in (
                        fill.exit_value_sol,
                        shadow_fill_exit_price_sol * fill_tokens_display
                        if shadow_fill_exit_price_sol is not None
                        else None,
                    )
                    if value is not None and value >= 0.0 and math.isfinite(value)
                ),
                None,
            )
            if shadow_fill_exit_value_sol is None:
                skipped["missing_shadow_exit_value"] += 1
                exit_fill_rows = []
                break

            onchain_spot_price_sol = exit_truth.update.spot_price_sol()
            if (
                shadow_fill_exit_price_sol is not None
                and onchain_spot_price_sol is not None
                and shadow_fill_exit_price_sol > onchain_spot_price_sol
            ):
                weighted_shadow_exit_spot_invariant_ok = False

            shadow_exit_value_sum += shadow_fill_exit_value_sol
            onchain_exit_value_sum += lamports_to_sol(onchain_exit_sol_out_lamports)
            total_exit_tokens_display += fill_tokens_display
            if onchain_spot_price_sol is not None:
                weighted_onchain_exit_spot_sum += onchain_spot_price_sol * fill_tokens_display
            exit_truth_gap_values.append(abs(exit_truth.delta_ms))

            exit_fill_rows.append(
                {
                    "fill_index": fill_index,
                    "fraction_bps": fill.fraction_bps,
                    "remaining_fraction_bps": fill.remaining_fraction_bps,
                    "shadow_entry_value_sol": fill_entry_value_sol,
                    "shadow_exit_value_sol": shadow_fill_exit_value_sol,
                    "shadow_exit_price_logged": fill.exit_price,
                    "shadow_exit_price_sol": shadow_fill_exit_price_sol,
                    "shadow_exit_vs_onchain_executable_pct": pct_diff(
                        shadow_fill_exit_price_sol, onchain_exit_exec_price_sol
                    ),
                    "shadow_exit_vs_onchain_spot_pct": pct_diff(
                        shadow_fill_exit_price_sol, onchain_spot_price_sol
                    ),
                    "tokens_sold_display": fill_tokens_display,
                    "tokens_sold_raw_estimated": fill_tokens_raw,
                    "target_ts_ms": fill_target_ts_ms,
                    "target_sample_ts_ms": fill.sample_timestamp_ms,
                    "target_sample_slot": fill.sample_slot,
                    "target_sample_age_ms": fill.sample_age_ms,
                    "truth_status": fill.truth_status,
                    "truth_source": fill.truth_source,
                    "sample_price_state": fill.sample_price_state,
                    "onchain_match_ts_ms": exit_truth.update.timestamp_ms,
                    "onchain_match_delta_ms": exit_truth.delta_ms,
                    "onchain_match_direction": exit_truth.direction,
                    "onchain_match_slot": exit_truth.update.slot,
                    "onchain_curve_finality": exit_truth.update.curve_finality,
                    "onchain_spot_price_sol": onchain_spot_price_sol,
                    "onchain_executable_price_sol": onchain_exit_exec_price_sol,
                    "onchain_executable_value_sol": lamports_to_sol(onchain_exit_sol_out_lamports),
                }
            )

        if not exit_fill_rows:
            continue
        if total_exit_tokens_display <= 0.0 or not math.isfinite(total_exit_tokens_display):
            skipped["invalid_total_exit_qty"] += 1
            continue

        shadow_effective_exit_price_sol = shadow_exit_value_sum / total_exit_tokens_display
        onchain_effective_exit_price_sol = onchain_exit_value_sum / total_exit_tokens_display
        weighted_onchain_exit_spot_price_sol = (
            weighted_onchain_exit_spot_sum / total_exit_tokens_display
            if weighted_onchain_exit_spot_sum > 0.0
            else None
        )

        reported_exit_value_sol = closed.exit_value_sol
        reported_entry_value_sol = closed.entry_value_sol
        close_ts_ms = closed.timestamp_ms
        detection_to_execution_ms = (
            entry_execution_ts_ms - gatekeeper_buy.first_seen_ts_ms
            if gatekeeper_buy is not None and gatekeeper_buy.first_seen_ts_ms is not None
            else None
        )
        decision_to_execution_ms = (
            entry_execution_ts_ms - transport.decision_ts_ms
            if transport.decision_ts_ms is not None
            else None
        )
        row = {
            "schema_version": 1,
            "analysis_status": "ok",
            "candidate_id": candidate_id,
            "position_id": closed.position_id,
            "mint_id": mint_id,
            "pool_id": pool_id,
            "close_reason": closed.close_reason,
            "truth_status": closed.truth_status,
            "truth_source": closed.truth_source,
            "sample_price_state": closed.sample_price_state,
            "timing": {
                "first_seen_ts_ms": gatekeeper_buy.first_seen_ts_ms if gatekeeper_buy is not None else None,
                "curve_t0_event_ts_ms": gatekeeper_buy.curve_t0_event_ts_ms
                if gatekeeper_buy is not None
                else None,
                "observation_start_ts_ms": gatekeeper_buy.observation_start_ts_ms
                if gatekeeper_buy is not None
                else None,
                "observation_end_ts_ms": gatekeeper_buy.observation_end_ts_ms
                if gatekeeper_buy is not None
                else None,
                "decision_ts_ms": transport.decision_ts_ms,
                "sim_started_ts_ms": transport.sim_started_ts_ms,
                "sim_finished_ts_ms": transport.sim_finished_ts_ms,
                "entry_execution_ts_ms": entry_execution_ts_ms,
                "close_ts_ms": close_ts_ms,
                "position_duration_ms": closed.duration_ms,
                "detection_to_execution_ms": detection_to_execution_ms,
                "decision_to_execution_ms": decision_to_execution_ms,
                "gatekeeper_buy_context_found": gatekeeper_buy is not None,
            },
            "shadow": {
                "execution_outcome": entry.execution_outcome if entry is not None else None,
                "entry_price_logged": raw_shadow_entry_price,
                "entry_price_multiplier": entry_multiplier,
                "entry_price_sol": shadow_entry_price_sol,
                "entry_value_sol": entry_value_sol,
                "reported_entry_value_sol": reported_entry_value_sol,
                "entry_token_amount_display_estimated": shadow_entry_tokens_display,
                "entry_token_amount_raw_estimated": shadow_entry_tokens_raw_estimated,
                "effective_exit_price_sol": shadow_effective_exit_price_sol,
                "exit_value_sol_from_fills": shadow_exit_value_sum,
                "reported_exit_value_sol": reported_exit_value_sol,
                "gross_pnl_sol": closed.gross_pnl_sol,
                "net_pnl_sol": closed.net_pnl_sol,
                "estimated_costs_sol": closed.estimated_costs_sol,
                "final_pnl_sol": closed.final_pnl,
                "final_pnl_pct": closed.final_pnl_pct,
                "total_exits": closed.total_exits,
            },
            "onchain": {
                "source": "diag_account_update_relay",
                "entry": {
                    "match_ts_ms": entry_truth.update.timestamp_ms,
                    "match_delta_ms": entry_truth.delta_ms,
                    "match_direction": entry_truth.direction,
                    "match_slot": entry_truth.update.slot,
                    "curve_finality": entry_truth.update.curve_finality,
                    "spot_price_sol": entry_truth.update.spot_price_sol(),
                    "executable_price_sol": onchain_entry_exec_price_sol,
                    "token_amount_display": raw_to_display_tokens(onchain_entry_tokens_raw),
                    "token_amount_raw": onchain_entry_tokens_raw,
                },
                "exit": {
                    "effective_executable_price_sol": onchain_effective_exit_price_sol,
                    "effective_spot_price_sol": weighted_onchain_exit_spot_price_sol,
                    "total_executable_value_sol": onchain_exit_value_sum,
                    "fill_count": len(exit_fill_rows),
                    "max_abs_truth_gap_ms": max(exit_truth_gap_values) if exit_truth_gap_values else None,
                },
            },
            "drift_pct": {
                "entry_vs_onchain_executable": pct_diff(
                    shadow_entry_price_sol, onchain_entry_exec_price_sol
                ),
                "entry_vs_onchain_spot": pct_diff(
                    shadow_entry_price_sol, entry_truth.update.spot_price_sol()
                ),
                "entry_token_amount_vs_onchain_executable": pct_diff(
                    shadow_entry_tokens_display, raw_to_display_tokens(onchain_entry_tokens_raw)
                ),
                "exit_vs_onchain_executable": pct_diff(
                    shadow_effective_exit_price_sol, onchain_effective_exit_price_sol
                ),
                "exit_vs_onchain_spot": pct_diff(
                    shadow_effective_exit_price_sol, weighted_onchain_exit_spot_price_sol
                ),
            },
            "consistency": {
                "shadow_exit_value_matches_closed_sol": (
                    shadow_exit_value_sum - reported_exit_value_sol
                    if reported_exit_value_sol is not None
                    else None
                ),
                "all_shadow_exit_prices_leq_onchain_spot": weighted_shadow_exit_spot_invariant_ok,
            },
            "exit_fills": exit_fill_rows,
        }
        rows.append(row)
    return rows, skipped


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=False))
            fh.write("\n")


def summarize(
    rows: list[dict[str, Any]],
    skipped: Counter[str],
    inputs: Inputs,
    scope_stats: ScopeStats,
) -> str:
    close_truth_coverage_pct = (
        100.0 * scope_stats.resolved_close_truth / scope_stats.closed_positions
        if scope_stats.closed_positions
        else None
    )
    lines = [
        "Shadow lifecycle on-chain report",
        f"scope_start_ms={inputs.session_start_ms} scope_end_ms={inputs.session_end_ms}",
        f"session_run_id={inputs.session_run_id}",
        f"rows_written={len(rows)} output={inputs.output_path}",
        (
            "scope_candidates="
            f"{scope_stats.candidate_count} "
            f"candidate_ts_first={scope_stats.first_candidate_ts_ms} "
            f"candidate_ts_last={scope_stats.latest_candidate_ts_ms}"
        ),
        (
            "close_truth_coverage="
            f"{scope_stats.resolved_close_truth}/{scope_stats.closed_positions} "
            f"failed={scope_stats.failed_close_truth} "
            f"pct={close_truth_coverage_pct:.2f}"
            if close_truth_coverage_pct is not None
            else f"close_truth_coverage=0/0 failed={scope_stats.failed_close_truth} pct=n/a"
        ),
    ]
    if scope_stats.latest_close_ts_ms is not None:
        lines.append(f"latest_close_ts_ms={scope_stats.latest_close_ts_ms}")
    if rows:
        entry_drifts = [
            row["drift_pct"]["entry_vs_onchain_executable"]
            for row in rows
            if isinstance(row["drift_pct"].get("entry_vs_onchain_executable"), (int, float))
        ]
        exit_drifts = [
            row["drift_pct"]["exit_vs_onchain_executable"]
            for row in rows
            if isinstance(row["drift_pct"].get("exit_vs_onchain_executable"), (int, float))
        ]
        truth_gaps = [
            abs(row["onchain"]["entry"]["match_delta_ms"])
            for row in rows
            if isinstance(row["onchain"]["entry"].get("match_delta_ms"), (int, float))
        ]
        exit_truth_gaps = [
            row["onchain"]["exit"]["max_abs_truth_gap_ms"]
            for row in rows
            if isinstance(row["onchain"]["exit"].get("max_abs_truth_gap_ms"), (int, float))
        ]
        if entry_drifts:
            lines.append(format_stat_line("entry_drift_pct", entry_drifts))
        if exit_drifts:
            lines.append(format_stat_line("exit_drift_pct", exit_drifts))
        if truth_gaps:
            lines.append(format_stat_line("entry_truth_gap_ms", truth_gaps))
        if exit_truth_gaps:
            lines.append(format_stat_line("exit_truth_gap_ms", exit_truth_gaps))
    if skipped:
        skipped_text = " ".join(f"{reason}={count}" for reason, count in skipped.most_common())
        lines.append(f"skipped {skipped_text}")
    return "\n".join(lines)


def format_stat_line(name: str, values: list[float]) -> str:
    ordered_abs = sorted(abs(value) for value in values)
    p95_index = max(0, math.ceil(len(ordered_abs) * 0.95) - 1)
    p95_abs = ordered_abs[p95_index]
    return (
        f"{name}: count={len(values)} mean={st.mean(values):.6f} "
        f"median={st.median(values):.6f} p95_abs={p95_abs:.6f}"
    )


def main() -> int:
    args = parse_args()
    inputs = resolve_inputs(args)
    transport_by_candidate = load_shadow_transport_records(inputs)
    entry_by_candidate = load_shadow_entries(inputs)
    lifecycle_by_candidate = load_lifecycle(inputs)
    scope_stats = build_scope_stats(
        transport_by_candidate,
        entry_by_candidate,
        lifecycle_by_candidate,
    )
    gatekeeper_buys_by_key = load_gatekeeper_buys(inputs)
    relevant_mints = {
        bundle.position_closed.mint_id
        for bundle in lifecycle_by_candidate.values()
        if bundle.position_closed is not None and bundle.position_closed.mint_id
    }
    if not relevant_mints:
        write_jsonl(inputs.output_path, [])
        print(
            summarize([], Counter({"no_closed_positions_in_scope": 1}), inputs, scope_stats)
        )
        return 0
    system_log_paths = iter_system_log_paths(inputs.system_log_base)
    diag_updates_by_mint = load_diag_updates(system_log_paths, relevant_mints)
    rows, skipped = analyze_positions(
        inputs,
        transport_by_candidate,
        entry_by_candidate,
        lifecycle_by_candidate,
        gatekeeper_buys_by_key,
        diag_updates_by_mint,
    )
    rows.sort(
        key=lambda row: (
            row["timing"]["close_ts_ms"] if row["timing"].get("close_ts_ms") is not None else 0,
            row["candidate_id"],
        )
    )
    write_jsonl(inputs.output_path, rows)
    print(summarize(rows, skipped, inputs, scope_stats))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
