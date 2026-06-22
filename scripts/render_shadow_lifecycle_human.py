#!/usr/bin/env python3
"""Render shadow lifecycle JSONL into per-token human-readable summaries.

The input files are append-only machine logs. This script groups rows by
position, keeps TimeStop V2 windows in order, and adds a compact terminal
summary so operators can inspect one token without reading raw JSONL.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


WINDOW_RECORD = "time_stop_v2_window"
TERMINAL_RECORDS = {"position_closed", "exit_filled", "exit_blocked"}
DISPATCH_RECORD = "shadow_dispatch"


def utc_iso(ms: int | None) -> str | None:
    if ms is None:
        return None
    return dt.datetime.fromtimestamp(ms / 1000, tz=dt.timezone.utc).isoformat()


def fmt(value: Any, digits: int = 3) -> str:
    if value is None:
        return "-"
    if isinstance(value, float):
        if not math.isfinite(value):
            return str(value)
        return f"{value:.{digits}f}"
    return str(value)


def pct(value: Any) -> str:
    if value is None:
        return "-"
    return f"{float(value):.2f}%"


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_no, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                rows.append(
                    {
                        "_source_path": str(path),
                        "_line_no": line_no,
                        "_bad_json": True,
                        "_error": str(exc),
                    }
                )
                continue
            row["_source_path"] = str(path)
            row["_line_no"] = line_no
            rows.append(row)
    return rows


def first_non_null(rows: list[dict[str, Any]], key: str) -> Any:
    for row in rows:
        value = row.get(key)
        if value is not None:
            return value
    return None


def terminal_sort_key(row: dict[str, Any]) -> tuple[int, int]:
    # Prefer position_closed as the canonical terminal summary.
    priority = 0 if row.get("record_type") == "position_closed" else 1
    return (priority, row.get("timestamp_ms") or 0)


def build_window(row: dict[str, Any]) -> dict[str, Any]:
    scheduled = row.get("time_stop_v2_scheduled_check_ms")
    observed = row.get("timestamp_ms")
    return {
        "source_line": row.get("_line_no"),
        "timestamp_ms": observed,
        "timestamp_utc": utc_iso(observed),
        "time_stop_v2_window_index": row.get("time_stop_v2_window_index"),
        "time_stop_v2_position_age_ms": row.get("time_stop_v2_position_age_ms"),
        "time_stop_v2_scheduled_check_ms": scheduled,
        "schedule_lag_ms": (observed - scheduled)
        if observed is not None and scheduled is not None
        else None,
        "time_stop_v2_status": row.get("time_stop_v2_status"),
        "time_stop_v2_subreason": row.get("time_stop_v2_subreason"),
        "time_stop_v2_failed_windows": row.get("time_stop_v2_failed_windows"),
        "time_stop_v2_candidate": row.get("time_stop_v2_candidate"),
        "time_stop_v2_candidate_ts_ms": row.get("time_stop_v2_candidate_ts_ms"),
        "time_stop_v2_candidate_ts_utc": utc_iso(row.get("time_stop_v2_candidate_ts_ms")),
        "time_stop_v2_candidate_subreason": row.get("time_stop_v2_candidate_subreason"),
        "time_stop_v2_checkpoint_slot": row.get("time_stop_v2_checkpoint_slot"),
        "time_stop_v2_latest_slot": row.get("time_stop_v2_latest_slot"),
        "time_stop_v2_checkpoint_timestamp_ms": row.get(
            "time_stop_v2_checkpoint_timestamp_ms"
        ),
        "time_stop_v2_latest_timestamp_ms": row.get("time_stop_v2_latest_timestamp_ms"),
        "exit_sample_slot": row.get("exit_sample_slot"),
        "sample_slot": row.get("sample_slot"),
        "sample_age_ms": row.get("sample_age_ms"),
        "sample_price_state": row.get("sample_price_state"),
        "exit_landed_slot": row.get("exit_landed_slot"),
        "exit_reason_evaluation_ts_ms": row.get("exit_reason_evaluation_ts_ms"),
        "time_stop_v2_price_delta_pct_window": row.get(
            "time_stop_v2_price_delta_pct_window"
        ),
        "time_stop_v2_price_delta_pct_from_entry": row.get(
            "time_stop_v2_price_delta_pct_from_entry"
        ),
        "time_stop_v2_mcap_delta_pct_window": row.get(
            "time_stop_v2_mcap_delta_pct_window"
        ),
        "time_stop_v2_bonding_delta_pct_window": row.get(
            "time_stop_v2_bonding_delta_pct_window"
        ),
        "time_stop_v2_tx_delta_window": row.get("time_stop_v2_tx_delta_window"),
        "time_stop_v2_volume_delta_sol_window": row.get(
            "time_stop_v2_volume_delta_sol_window"
        ),
        "time_stop_v2_avg_volume_per_tx_sol_window": row.get(
            "time_stop_v2_avg_volume_per_tx_sol_window"
        ),
    }


def build_terminal(row: dict[str, Any] | None) -> dict[str, Any] | None:
    if row is None:
        return None
    ts = row.get("exit_reason_evaluation_ts_ms") or row.get("timestamp_ms")
    return {
        "source_line": row.get("_line_no"),
        "record_type": row.get("record_type"),
        "timestamp_ms": row.get("timestamp_ms"),
        "timestamp_utc": utc_iso(row.get("timestamp_ms")),
        "close_reason": row.get("close_reason"),
        "duration_ms": row.get("duration_ms"),
        "final_pnl_pct": row.get("final_pnl_pct"),
        "final_pnl_sol": row.get("final_pnl"),
        "entry_value_sol": row.get("entry_value_sol"),
        "exit_value_sol": row.get("exit_value_sol"),
        "exit_price": row.get("exit_price"),
        "remaining_fraction_bps": row.get("remaining_fraction_bps"),
        "exit_sample_slot": row.get("exit_sample_slot"),
        "sample_slot": row.get("sample_slot"),
        "sample_age_ms": row.get("sample_age_ms"),
        "sample_price_state": row.get("sample_price_state"),
        "exit_landed_slot": row.get("exit_landed_slot"),
        "exit_reason_evaluation_ts_ms": ts,
        "exit_reason_evaluation_ts_utc": utc_iso(ts),
        "time_stop_v2_status": row.get("time_stop_v2_status"),
        "time_stop_v2_subreason": row.get("time_stop_v2_subreason"),
        "time_stop_v2_candidate": row.get("time_stop_v2_candidate"),
        "time_stop_v2_candidate_ts_ms": row.get("time_stop_v2_candidate_ts_ms"),
        "time_stop_v2_candidate_subreason": row.get(
            "time_stop_v2_candidate_subreason"
        ),
    }


def relation_summary(
    windows: list[dict[str, Any]], terminal: dict[str, Any] | None
) -> dict[str, Any]:
    first_candidate = next((w for w in windows if w.get("time_stop_v2_candidate")), None)
    candidate_ts = first_candidate.get("time_stop_v2_candidate_ts_ms") if first_candidate else None
    terminal_ts = terminal.get("exit_reason_evaluation_ts_ms") if terminal else None
    diff_ms = terminal_ts - candidate_ts if candidate_ts is not None and terminal_ts is not None else None

    if terminal is None:
        relation = "still_open_or_no_terminal"
    elif candidate_ts is None:
        relation = "no_time_stop_v2_candidate_before_terminal"
    elif abs(diff_ms or 0) <= 1000:
        relation = "same_moment_plus_minus_1s"
    elif (diff_ms or 0) > 0:
        relation = "time_stop_v2_candidate_before_terminal"
    else:
        relation = "time_stop_v2_candidate_after_terminal"

    candidate_index = (
        first_candidate.get("time_stop_v2_window_index") if first_candidate else None
    )
    alive_after_candidate = [
        w
        for w in windows
        if candidate_index is not None
        and (w.get("time_stop_v2_window_index") or -1) > candidate_index
        and w.get("time_stop_v2_status") == "alive"
    ]

    return {
        "first_time_stop_v2_candidate_window_index": candidate_index,
        "first_time_stop_v2_candidate_ts_ms": candidate_ts,
        "first_time_stop_v2_candidate_ts_utc": utc_iso(candidate_ts),
        "first_time_stop_v2_candidate_subreason": first_candidate.get(
            "time_stop_v2_candidate_subreason"
        )
        if first_candidate
        else None,
        "terminal_minus_v2_candidate_ms": diff_ms,
        "terminal_minus_v2_candidate_seconds": diff_ms / 1000 if diff_ms is not None else None,
        "time_stop_v2_vs_terminal_relation": relation,
        "alive_windows_after_candidate": len(alive_after_candidate),
        "has_alive_after_candidate": bool(alive_after_candidate),
        "status_counts": dict(Counter(w.get("time_stop_v2_status") for w in windows)),
        "subreason_counts": dict(Counter(w.get("time_stop_v2_subreason") for w in windows)),
    }


def numeric(value: Any) -> float | None:
    if value is None:
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    return number if math.isfinite(number) else None


def consecutive_streak_ending_at(
    windows: list[dict[str, Any]], end_index: int, statuses: set[str]
) -> int:
    streak = 0
    for window in reversed(windows[: end_index + 1]):
        if window.get("time_stop_v2_status") not in statuses:
            break
        streak += 1
    return streak


def build_candidate_position(pos: dict[str, Any]) -> dict[str, Any] | None:
    windows = pos.get("time_stop_v2_windows") or []
    terminal = pos.get("terminal") or {}
    summary = pos.get("summary") or {}
    dispatch = pos.get("dispatch") or {}

    candidate_offset = next(
        (idx for idx, window in enumerate(windows) if window.get("time_stop_v2_candidate")),
        None,
    )
    if candidate_offset is None:
        return None

    candidate = windows[candidate_offset]
    after_windows = windows[candidate_offset + 1 :]
    candidate_price = numeric(candidate.get("time_stop_v2_price_delta_pct_from_entry"))
    after_prices = [
        price
        for price in (
            numeric(window.get("time_stop_v2_price_delta_pct_from_entry"))
            for window in after_windows
        )
        if price is not None
    ]
    max_after_price = max(after_prices) if after_prices else None
    terminal_ts = terminal.get("exit_reason_evaluation_ts_ms")
    candidate_ts = candidate.get("time_stop_v2_candidate_ts_ms") or candidate.get("timestamp_ms")
    alive_after_count = sum(
        1 for window in after_windows if window.get("time_stop_v2_status") == "alive"
    )

    return {
        "mint_id": pos.get("mint_id"),
        "pool_id": pos.get("pool_id"),
        "position_id": pos.get("position_id"),
        "candidate_id": pos.get("candidate_id"),
        "source_path": pos.get("source_path"),
        "first_line": pos.get("first_line"),
        "candidate_source_line": candidate.get("source_line"),
        "candidate_window_index": candidate.get("time_stop_v2_window_index"),
        "candidate_ts_ms": candidate_ts,
        "candidate_ts_utc": utc_iso(candidate_ts),
        "candidate_status": candidate.get("time_stop_v2_status"),
        "candidate_subreason": candidate.get("time_stop_v2_candidate_subreason")
        or candidate.get("time_stop_v2_subreason"),
        "candidate_position_age_ms": candidate.get("time_stop_v2_position_age_ms"),
        "candidate_failed_windows": candidate.get("time_stop_v2_failed_windows"),
        "alive_after_candidate": alive_after_count > 0,
        "alive_windows_after_candidate": alive_after_count,
        "rebound_after_candidate_pct": (
            max_after_price - candidate_price
            if max_after_price is not None and candidate_price is not None
            else None
        ),
        "max_price_after_candidate_pct": max_after_price,
        "candidate_price_delta_pct_from_entry": candidate_price,
        "terminal_reason": terminal.get("close_reason"),
        "terminal_pnl_pct": terminal.get("final_pnl_pct"),
        "candidate_to_terminal_ms": (
            terminal_ts - candidate_ts
            if terminal_ts is not None and candidate_ts is not None
            else None
        ),
        "sample_age_at_candidate_ms": candidate.get("sample_age_ms"),
        "schedule_lag_at_candidate_ms": candidate.get("schedule_lag_ms"),
        "decision_to_dispatch_record_ms": dispatch.get("decision_to_dispatch_record_ms"),
        "stale_streak_before_candidate": consecutive_streak_ending_at(
            windows, candidate_offset, {"stale_or_insufficient"}
        ),
        "weak_heartbeat_streak_before_candidate": consecutive_streak_ending_at(
            windows, candidate_offset, {"weak", "heartbeat"}
        ),
        "statuses_before_candidate": [
            window.get("time_stop_v2_status") for window in windows[: candidate_offset + 1]
        ],
        "statuses_after_candidate": [
            window.get("time_stop_v2_status") for window in after_windows
        ],
        "time_stop_v2_vs_terminal_relation": summary.get(
            "time_stop_v2_vs_terminal_relation"
        ),
    }


def build_candidate_positions(positions: list[dict[str, Any]]) -> list[dict[str, Any]]:
    candidates = [
        candidate
        for candidate in (build_candidate_position(pos) for pos in positions)
        if candidate is not None
    ]
    candidates.sort(
        key=lambda row: (
            row.get("candidate_ts_ms") or 0,
            row.get("candidate_source_line") or 0,
            row.get("mint_id") or "",
        )
    )
    return candidates


def build_positions(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    dispatch_by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for row in rows:
        if row.get("_bad_json"):
            continue
        if row.get("record_type") == DISPATCH_RECORD:
            candidate_id = row.get("candidate_id")
            if candidate_id:
                dispatch_by_candidate[candidate_id].append(row)
            continue
        position_id = row.get("position_id")
        if position_id:
            groups[position_id].append(row)

    positions = []
    for position_id, pos_rows in groups.items():
        pos_rows.sort(key=lambda r: (r.get("timestamp_ms") or 0, r.get("_line_no") or 0))
        candidate_id = first_non_null(pos_rows, "candidate_id")
        dispatch_rows = dispatch_by_candidate.get(candidate_id, [])
        dispatch = dispatch_rows[0] if dispatch_rows else None
        all_for_header = ([dispatch] if dispatch else []) + pos_rows
        windows = [build_window(r) for r in pos_rows if r.get("record_type") == WINDOW_RECORD]
        windows.sort(
            key=lambda w: (
                w.get("time_stop_v2_window_index")
                if w.get("time_stop_v2_window_index") is not None
                else 10**9,
                w.get("timestamp_ms") or 0,
            )
        )
        terminal_rows = [r for r in pos_rows if r.get("record_type") in TERMINAL_RECORDS]
        terminal_source = sorted(terminal_rows, key=terminal_sort_key)[0] if terminal_rows else None
        terminal = build_terminal(terminal_source)
        decision_ts = dispatch.get("decision_ts_ms") if dispatch else None
        dispatch_ts = dispatch.get("timestamp_ms") if dispatch else None

        entry = {
            "entry_slot": first_non_null(all_for_header, "entry_slot"),
            "entry_simulation_rpc_slot": first_non_null(
                all_for_header, "entry_simulation_rpc_slot"
            ),
            "entry_market_anchor_slot": first_non_null(
                all_for_header, "entry_market_anchor_slot"
            ),
            "entry_market_anchor_source": first_non_null(
                all_for_header, "entry_market_anchor_source"
            ),
            "entry_landed_slot": first_non_null(all_for_header, "entry_landed_slot"),
            "entry_landed_slot_source": first_non_null(
                all_for_header, "entry_landed_slot_source"
            ),
            "entry_price": first_non_null(all_for_header, "entry_price"),
        }
        latest_window = windows[-1] if windows else None
        position = {
            "mint_id": first_non_null(all_for_header, "mint_id"),
            "pool_id": first_non_null(all_for_header, "pool_id"),
            "position_id": position_id,
            "candidate_id": candidate_id,
            "source_path": first_non_null(pos_rows, "_source_path"),
            "first_line": min(r.get("_line_no") or 0 for r in pos_rows),
            "last_line": max(r.get("_line_no") or 0 for r in pos_rows),
            "record_type_counts": dict(Counter(r.get("record_type") for r in pos_rows)),
            "dispatch": {
                "decision_ts_ms": decision_ts,
                "decision_ts_utc": utc_iso(decision_ts),
                "dispatch_record_ts_ms": dispatch_ts,
                "dispatch_record_ts_utc": utc_iso(dispatch_ts),
                "decision_to_dispatch_record_ms": dispatch_ts - decision_ts
                if dispatch_ts is not None and decision_ts is not None
                else None,
                "dispatch_status": dispatch.get("dispatch_status") if dispatch else None,
                "simulation_outcome": dispatch.get("simulation_outcome") if dispatch else None,
            },
            "entry": entry,
            "sample": {
                "first_window_sample_slot": windows[0].get("sample_slot") if windows else None,
                "latest_window_sample_slot": latest_window.get("sample_slot")
                if latest_window
                else None,
                "terminal_sample_slot": terminal.get("sample_slot") if terminal else None,
            },
            "time_stop_v2_windows": windows,
            "terminal": terminal,
        }
        position["summary"] = relation_summary(windows, terminal)
        positions.append(position)

    positions.sort(key=lambda p: (p["first_line"], p["mint_id"] or ""))
    return positions


def render_candidate_positions_markdown(candidates: list[dict[str, Any]]) -> list[str]:
    out: list[str] = []
    out.append("## TimeStop V2 Candidate Positions")
    out.append("")
    out.append(f"candidate_positions: {len(candidates)}")
    out.append("")
    if not candidates:
        out.append("_No TimeStop V2 candidates found._")
        out.append("")
        return out

    out.append(
        "| mint | line | cand_idx | cand_status | cand_subreason | alive_after | rebound_after | max_px_after | terminal | terminal_pnl | cand_to_terminal_ms | sample_age_ms | schedule_lag_ms | stale_streak | weak_heartbeat_streak | decision_to_dispatch_ms |"
    )
    out.append(
        "|---|---:|---:|---|---|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|"
    )
    for candidate in candidates:
        out.append(
            "| "
            + " | ".join(
                [
                    f"`{fmt(candidate.get('mint_id'))}`",
                    fmt(candidate.get("candidate_source_line")),
                    fmt(candidate.get("candidate_window_index")),
                    fmt(candidate.get("candidate_status")),
                    fmt(candidate.get("candidate_subreason")),
                    fmt(candidate.get("alive_windows_after_candidate")),
                    pct(candidate.get("rebound_after_candidate_pct")),
                    pct(candidate.get("max_price_after_candidate_pct")),
                    fmt(candidate.get("terminal_reason")),
                    pct(candidate.get("terminal_pnl_pct")),
                    fmt(candidate.get("candidate_to_terminal_ms")),
                    fmt(candidate.get("sample_age_at_candidate_ms")),
                    fmt(candidate.get("schedule_lag_at_candidate_ms")),
                    fmt(candidate.get("stale_streak_before_candidate")),
                    fmt(candidate.get("weak_heartbeat_streak_before_candidate")),
                    fmt(candidate.get("decision_to_dispatch_record_ms")),
                ]
            )
            + " |"
        )
    out.append("")
    return out


def render_candidates_only_markdown(candidates: list[dict[str, Any]]) -> str:
    out = render_candidate_positions_markdown(candidates)
    if out:
        out[0] = "# TimeStop V2 Candidate Positions"
    return "\n".join(out)


def render_markdown(positions: list[dict[str, Any]], limit: int | None = None) -> str:
    selected = positions[:limit] if limit else positions
    candidates = build_candidate_positions(positions)
    out: list[str] = []
    out.append("# Shadow Lifecycle Human Report")
    out.append("")
    out.append(f"positions: {len(positions)}")
    if limit:
        out.append(f"shown: {len(selected)}")
    out.append("")
    out.extend(render_candidate_positions_markdown(candidates))

    for idx, pos in enumerate(selected, 1):
        entry = pos["entry"]
        terminal = pos.get("terminal") or {}
        summary = pos["summary"]
        dispatch = pos["dispatch"]
        sample = pos["sample"]
        out.append(f"## {idx}. {pos.get('mint_id')}")
        out.append("")
        out.append(f"- pool_id: `{pos.get('pool_id')}`")
        out.append(f"- position_id: `{pos.get('position_id')}`")
        out.append(f"- source: `{pos.get('source_path')}:{pos.get('first_line')}`")
        out.append(f"- decision_ts_ms: `{fmt(dispatch.get('decision_ts_ms'))}`")
        out.append(f"- dispatch_record_ts_ms: `{fmt(dispatch.get('dispatch_record_ts_ms'))}`")
        out.append(
            f"- decision_to_dispatch_record_ms: `{fmt(dispatch.get('decision_to_dispatch_record_ms'))}`"
        )
        out.append(f"- entry_slot: `{fmt(entry.get('entry_slot'))}`")
        out.append(
            f"- entry_simulation_rpc_slot: `{fmt(entry.get('entry_simulation_rpc_slot'))}`"
        )
        out.append(
            f"- entry_market_anchor_slot: `{fmt(entry.get('entry_market_anchor_slot'))}`"
        )
        out.append(f"- entry_landed_slot: `{fmt(entry.get('entry_landed_slot'))}`")
        out.append(f"- entry_price: `{fmt(entry.get('entry_price'), 12)}`")
        out.append(f"- first_window_sample_slot: `{fmt(sample.get('first_window_sample_slot'))}`")
        out.append(f"- latest_window_sample_slot: `{fmt(sample.get('latest_window_sample_slot'))}`")
        out.append(f"- terminal_sample_slot: `{fmt(sample.get('terminal_sample_slot'))}`")
        out.append("")
        out.append("| idx | age_s | status | subreason | candidate | failed | px_from_entry | px_window | latest_slot | sample_slot | sample_age_ms | exit_landed_slot |")
        out.append("|---:|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|")
        for window in pos["time_stop_v2_windows"]:
            out.append(
                "| "
                + " | ".join(
                    [
                        fmt(window.get("time_stop_v2_window_index")),
                        fmt((window.get("time_stop_v2_position_age_ms") or 0) / 1000, 2)
                        if window.get("time_stop_v2_position_age_ms") is not None
                        else "-",
                        fmt(window.get("time_stop_v2_status")),
                        fmt(window.get("time_stop_v2_subreason")),
                        fmt(window.get("time_stop_v2_candidate")),
                        fmt(window.get("time_stop_v2_failed_windows")),
                        pct(window.get("time_stop_v2_price_delta_pct_from_entry")),
                        pct(window.get("time_stop_v2_price_delta_pct_window")),
                        fmt(window.get("time_stop_v2_latest_slot")),
                        fmt(window.get("sample_slot")),
                        fmt(window.get("sample_age_ms")),
                        fmt(window.get("exit_landed_slot")),
                    ]
                )
                + " |"
            )
        out.append("")
        out.append("Summary:")
        out.append(f"- terminal_close_reason: `{fmt(terminal.get('close_reason'))}`")
        out.append(f"- terminal_final_pnl_pct: `{pct(terminal.get('final_pnl_pct'))}`")
        out.append(f"- terminal_duration_ms: `{fmt(terminal.get('duration_ms'))}`")
        out.append(
            f"- first_v2_candidate_window_index: `{fmt(summary.get('first_time_stop_v2_candidate_window_index'))}`"
        )
        out.append(
            f"- first_v2_candidate_subreason: `{fmt(summary.get('first_time_stop_v2_candidate_subreason'))}`"
        )
        out.append(
            f"- terminal_minus_v2_candidate_seconds: `{fmt(summary.get('terminal_minus_v2_candidate_seconds'), 3)}`"
        )
        out.append(
            f"- time_stop_v2_vs_terminal_relation: `{summary.get('time_stop_v2_vs_terminal_relation')}`"
        )
        out.append(
            f"- alive_windows_after_candidate: `{summary.get('alive_windows_after_candidate')}`"
        )
        out.append(f"- status_counts: `{summary.get('status_counts')}`")
        out.append(f"- subreason_counts: `{summary.get('subreason_counts')}`")
        out.append("")
    return "\n".join(out)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render shadow/probe lifecycle JSONL into per-token human summaries."
    )
    parser.add_argument("paths", nargs="+", type=Path, help="shadow_lifecycle.jsonl paths")
    parser.add_argument("--json", action="store_true", help="emit structured JSON")
    parser.add_argument("--mint", help="filter to one mint_id")
    parser.add_argument("--position-id", help="filter to one position_id")
    parser.add_argument("--limit", type=int, help="limit number of rendered positions")
    parser.add_argument(
        "--candidates-only",
        action="store_true",
        help="emit only the TimeStop V2 candidate positions table/payload",
    )
    parser.add_argument("--output", type=Path, help="write output to file instead of stdout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    rows: list[dict[str, Any]] = []
    for path in args.paths:
        rows.extend(read_jsonl(path))

    positions = build_positions(rows)
    if args.mint:
        positions = [p for p in positions if p.get("mint_id") == args.mint]
    if args.position_id:
        positions = [p for p in positions if p.get("position_id") == args.position_id]

    payload: Any
    if args.json:
        candidate_positions = build_candidate_positions(positions)
        payload = {
            "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "inputs": [str(path) for path in args.paths],
            "positions_total": len(positions),
            "candidate_positions_total": len(candidate_positions),
            "candidate_positions": candidate_positions[: args.limit]
            if args.limit
            else candidate_positions,
        }
        if not args.candidates_only:
            payload["positions"] = positions[: args.limit] if args.limit else positions
        text = json.dumps(payload, ensure_ascii=False, indent=2)
    elif args.candidates_only:
        candidate_positions = build_candidate_positions(positions)
        selected_candidates = candidate_positions[: args.limit] if args.limit else candidate_positions
        text = render_candidates_only_markdown(selected_candidates)
    else:
        text = render_markdown(positions, args.limit)

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text + "\n", encoding="utf-8")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
