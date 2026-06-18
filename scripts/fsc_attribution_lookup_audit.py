#!/usr/bin/env python3
"""Offline FSC lookup-attribution audit.

This script is offline-only. It joins durable FSC lookup candidates with
durable funding events and reports whether inbound funding exists for the
lookup wallet in 5/15/30/60 minute windows before the buy-event timestamp.
"""

from __future__ import annotations

import argparse
import csv
import json
from bisect import bisect_right
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


WINDOWS_MS = {
    "5m": 5 * 60 * 1000,
    "15m": 15 * 60 * 1000,
    "30m": 30 * 60 * 1000,
    "60m": 60 * 60 * 1000,
}

CSV_COLUMNS = [
    "decision_id",
    "lookup_wallet",
    "decision_ts_ms",
    "buy_event_ts_ms",
    "found_5m",
    "found_15m",
    "found_30m",
    "found_60m",
    "latest_funding_age_ms",
    "funding_amount_lamports",
    "source_wallet",
    "miss_reason",
    "diagnosed_bottleneck",
]


@dataclass(frozen=True)
class FundingEvent:
    recipient_wallet: str
    source_wallet: str
    lamports: int
    ts_ms: int
    signature: str | None
    slot: int | None


@dataclass(frozen=True)
class LookupRow:
    decision_id: str
    lookup_wallet: str | None
    decision_ts_ms: int | None
    miss_reason: str | None
    lookup_result: str | None
    buy_event_ts_ms: int | None = None


def read_jsonl_paths(paths: Iterable[Path]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in paths:
        if not path or not path.exists():
            continue
        with path.open("r", encoding="utf-8") as handle:
            for line_no, line in enumerate(handle, start=1):
                text = line.strip()
                if not text:
                    continue
                try:
                    value = json.loads(text)
                except json.JSONDecodeError as exc:
                    raise ValueError(f"{path}:{line_no}: invalid JSONL row: {exc}") from exc
                if isinstance(value, dict):
                    rows.append(value)
    return rows


def as_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str):
        try:
            return int(float(value))
        except ValueError:
            return None
    return None


def as_str(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def extract_ts_ms(row: dict[str, Any]) -> int | None:
    for key in ("ts_ms", "event_ts_ms", "decision_ts_ms", "block_time_ms"):
        value = as_int(row.get(key))
        if value is not None:
            return value
    block_time = as_int(row.get("block_time"))
    if block_time is not None:
        return block_time * 1000
    return None


def funding_event_from_row(row: dict[str, Any]) -> FundingEvent | None:
    recipient = as_str(row.get("recipient_wallet")) or as_str(row.get("to_wallet"))
    source = as_str(row.get("source_wallet")) or as_str(row.get("from_wallet"))
    lamports = as_int(row.get("lamports")) or as_int(row.get("amount_lamports"))
    ts_ms = extract_ts_ms(row)
    if not recipient or not source or lamports is None or ts_ms is None:
        return None
    return FundingEvent(
        recipient_wallet=recipient,
        source_wallet=source,
        lamports=lamports,
        ts_ms=ts_ms,
        signature=as_str(row.get("signature")),
        slot=as_int(row.get("slot")),
    )


def decision_id_from_row(row: dict[str, Any], fallback_prefix: str, index: int) -> str:
    return (
        as_str(row.get("decision_id"))
        or as_str(row.get("ab_record_id"))
        or as_str(row.get("join_key"))
        or f"{fallback_prefix}:{index}"
    )


def lookup_row_from_sidecar(row: dict[str, Any], index: int) -> LookupRow:
    lookup_wallet = (
        as_str(row.get("selected_lookup_wallet"))
        or as_str(row.get("lookup_wallet"))
    )
    miss_reason = (
        as_str(row.get("diagnostic_miss_reason"))
        or as_str(row.get("miss_reason"))
    )
    return LookupRow(
        decision_id=decision_id_from_row(row, "lookup_sidecar", index),
        lookup_wallet=lookup_wallet,
        decision_ts_ms=as_int(row.get("decision_ts_ms")),
        buy_event_ts_ms=as_int(row.get("buy_event_ts_ms")),
        miss_reason=miss_reason,
        lookup_result=as_str(row.get("lookup_result")),
    )


def lookup_rows_from_decisions(rows: list[dict[str, Any]]) -> list[LookupRow]:
    out: list[LookupRow] = []
    for row_index, row in enumerate(rows, start=1):
        diagnostics = row.get("funding_source_diagnostics")
        if not isinstance(diagnostics, dict):
            continue
        lookup_diagnostics = diagnostics.get("lookup_diagnostics")
        if not isinstance(lookup_diagnostics, list):
            continue
        decision_ts_ms = as_int(row.get("observation_end_ts_ms")) or as_int(
            row.get("first_seen_ts_ms")
        )
        decision_id = decision_id_from_row(row, "decision", row_index)
        for diagnostic in lookup_diagnostics:
            if not isinstance(diagnostic, dict):
                continue
            lookup_wallet = (
                as_str(diagnostic.get("selected_lookup_wallet"))
                or as_str(diagnostic.get("lookup_wallet"))
            )
            out.append(
                LookupRow(
                    decision_id=decision_id,
                    lookup_wallet=lookup_wallet,
                    decision_ts_ms=decision_ts_ms,
                    buy_event_ts_ms=as_int(diagnostic.get("buy_event_ts_ms")),
                    miss_reason=(
                        as_str(diagnostic.get("diagnostic_miss_reason"))
                        or as_str(diagnostic.get("miss_reason"))
                    ),
                    lookup_result=as_str(diagnostic.get("lookup_result")),
                )
            )
    return out


def load_lookup_rows(
    lookup_sidecar_rows: list[dict[str, Any]],
    decision_rows: list[dict[str, Any]],
    buy_rows: list[dict[str, Any]],
) -> list[LookupRow]:
    if lookup_sidecar_rows:
        return [
            lookup_row_from_sidecar(row, index)
            for index, row in enumerate(lookup_sidecar_rows, start=1)
        ]
    return lookup_rows_from_decisions(decision_rows + buy_rows)


def stream_funding_events_for_lookup_wallets(
    paths: Iterable[Path],
    lookup_wallets: Iterable[str | None],
) -> tuple[list[FundingEvent], int, int]:
    wanted_wallets = {wallet for wallet in lookup_wallets if wallet}
    if not wanted_wallets:
        return [], 0, 0

    retained: list[FundingEvent] = []
    rows_scanned = 0
    events_parsed = 0
    for path in paths:
        if not path or not path.exists():
            continue
        with path.open("r", encoding="utf-8") as handle:
            for line_no, line in enumerate(handle, start=1):
                text = line.strip()
                if not text:
                    continue
                rows_scanned += 1
                try:
                    value = json.loads(text)
                except json.JSONDecodeError as exc:
                    raise ValueError(f"{path}:{line_no}: invalid JSONL row: {exc}") from exc
                if not isinstance(value, dict):
                    continue
                event = funding_event_from_row(value)
                if event is None:
                    continue
                events_parsed += 1
                if event.recipient_wallet in wanted_wallets:
                    retained.append(event)
    return retained, rows_scanned, events_parsed


def build_funding_index(events: Iterable[FundingEvent]) -> dict[str, list[FundingEvent]]:
    indexed: dict[str, list[FundingEvent]] = defaultdict(list)
    for event in events:
        indexed[event.recipient_wallet].append(event)
    for wallet_events in indexed.values():
        wallet_events.sort(key=lambda event: event.ts_ms)
    return indexed


def diagnose_bottleneck(
    lookup: LookupRow,
    events_before_decision: list[FundingEvent],
    found_5m: bool,
    found_60m: bool,
    latest_age_ms: int | None,
) -> str:
    if not lookup.lookup_wallet:
        return "LOOKUP_WALLET_MISSING"
    if found_5m and lookup.lookup_result == "hit":
        return "ATTRIBUTION_HIT"
    if found_5m:
        if lookup.miss_reason in {
            "INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD",
            "INBOUND_EXISTS_BUT_BELOW_ABS_ATTRIBUTION_THRESHOLD",
            "INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD",
            "FSC_ABS_ATTRIBUTION_TOO_SMALL",
            "FSC_RELATIVE_FUNDING_TOO_SMALL",
        }:
            return "THRESHOLD_FILTER"
        if lookup.miss_reason in {"SAME_SLOT_ORDERING", "FSC_SAME_SLOT_ORDERING_UNAVAILABLE"}:
            return "ORDERING_SEMANTICS"
        return "LOOKUP_OR_STORE_SEMANTICS"
    if found_60m:
        return "LOOKBACK_WINDOW_TOO_SHORT"
    if events_before_decision:
        if latest_age_ms is not None and latest_age_ms > WINDOWS_MS["60m"]:
            return "DIRECT_FUNDING_OLDER_THAN_60M"
        return "DIRECT_FUNDING_OUTSIDE_AUDIT_WINDOW"
    return "DIRECT_FUNDING_NOT_OBSERVED_60M"


def lookup_reference_ts_ms(lookup: LookupRow) -> int | None:
    return lookup.buy_event_ts_ms if lookup.buy_event_ts_ms is not None else lookup.decision_ts_ms


def audit_lookup_rows(
    lookup_rows: list[LookupRow],
    funding_events: list[FundingEvent],
) -> list[dict[str, Any]]:
    funding_index = build_funding_index(funding_events)
    output: list[dict[str, Any]] = []
    for lookup in lookup_rows:
        wallet_events = funding_index.get(lookup.lookup_wallet or "", [])
        reference_ts_ms = lookup_reference_ts_ms(lookup)
        events_before_decision: list[FundingEvent] = []
        if reference_ts_ms is not None:
            timestamps = [event.ts_ms for event in wallet_events]
            end = bisect_right(timestamps, reference_ts_ms)
            events_before_decision = wallet_events[:end]

        latest = events_before_decision[-1] if events_before_decision else None
        latest_age_ms = (
            reference_ts_ms - latest.ts_ms
            if latest is not None and reference_ts_ms is not None
            else None
        )
        found = {}
        for label, window_ms in WINDOWS_MS.items():
            found[label] = latest_age_ms is not None and 0 <= latest_age_ms <= window_ms

        diagnosed_bottleneck = diagnose_bottleneck(
            lookup,
            events_before_decision,
            bool(found["5m"]),
            bool(found["60m"]),
            latest_age_ms,
        )
        miss_reason = lookup.miss_reason
        if miss_reason is None and not events_before_decision:
            miss_reason = "NO_INBOUND_TRANSFER_OBSERVED"

        output.append(
            {
                "decision_id": lookup.decision_id,
                "lookup_wallet": lookup.lookup_wallet or "",
                "decision_ts_ms": lookup.decision_ts_ms or "",
                "buy_event_ts_ms": lookup.buy_event_ts_ms or "",
                "found_5m": bool(found["5m"]),
                "found_15m": bool(found["15m"]),
                "found_30m": bool(found["30m"]),
                "found_60m": bool(found["60m"]),
                "latest_funding_age_ms": latest_age_ms if latest_age_ms is not None else "",
                "funding_amount_lamports": latest.lamports if latest else "",
                "source_wallet": latest.source_wallet if latest else "",
                "miss_reason": miss_reason or "",
                "diagnosed_bottleneck": diagnosed_bottleneck,
            }
        )
    return output


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=CSV_COLUMNS)
        writer.writeheader()
        for row in rows:
            writer.writerow({key: row.get(key, "") for key in CSV_COLUMNS})


def write_markdown(
    path: Path,
    rows: list[dict[str, Any]],
    *,
    funding_events_count: int,
    lookup_rows_count: int,
    funding_rows_scanned: int | None = None,
    funding_events_parsed: int | None = None,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    bottlenecks = Counter(row["diagnosed_bottleneck"] for row in rows)
    miss_reasons = Counter(row["miss_reason"] or "none" for row in rows)
    found_counts = {
        key: sum(1 for row in rows if row[f"found_{key}"])
        for key in ("5m", "15m", "30m", "60m")
    }
    lines = [
        "# FSC Attribution Lookup Audit",
        "",
        "## Scope",
        "",
        "- purpose: offline FSC attribution lookup autopsy",
        "- policy/execution/send path: not used",
        f"- funding_events_rows_scanned: {funding_rows_scanned if funding_rows_scanned is not None else funding_events_count}",
        f"- funding_events_rows_parsed: {funding_events_parsed if funding_events_parsed is not None else funding_events_count}",
        f"- funding_events_rows_retained_for_lookup_wallets: {funding_events_count}",
        f"- lookup_rows: {lookup_rows_count}",
        f"- audited_rows: {len(rows)}",
        "",
        "## Window Joins",
        "",
        f"- found_5m: {found_counts['5m']}",
        f"- found_15m: {found_counts['15m']}",
        f"- found_30m: {found_counts['30m']}",
        f"- found_60m: {found_counts['60m']}",
        "",
        "## Diagnosed Bottlenecks",
        "",
    ]
    lines.extend(f"- {key}: {value}" for key, value in bottlenecks.most_common())
    lines.extend(["", "## Miss Reasons", ""])
    lines.extend(f"- {key}: {value}" for key, value in miss_reasons.most_common())
    lines.extend(["", "## Sample Rows", ""])
    lines.append(
        "| decision_id | lookup_wallet | found_5m | found_60m | latest_funding_age_ms | miss_reason | diagnosed_bottleneck |"
    )
    lines.append("|---|---|---:|---:|---:|---|---|")
    for row in rows[:25]:
        lines.append(
            "| {decision_id} | {lookup_wallet} | {found_5m} | {found_60m} | {latest_funding_age_ms} | {miss_reason} | {diagnosed_bottleneck} |".format(
                **row
            )
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decisions", nargs="*", type=Path, default=[])
    parser.add_argument("--buys", nargs="*", type=Path, default=[])
    parser.add_argument("--funding-events", nargs="+", type=Path, required=True)
    parser.add_argument("--lookup-candidates", nargs="*", type=Path, default=[])
    parser.add_argument(
        "--output-md",
        type=Path,
        default=Path("FSC_ATTRIBUTION_LOOKUP_AUDIT.md"),
    )
    parser.add_argument(
        "--output-csv",
        type=Path,
        default=Path("FSC_ATTRIBUTION_LOOKUP_AUDIT.csv"),
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_arg_parser().parse_args(argv)
    decision_rows = read_jsonl_paths(args.decisions)
    buy_rows = read_jsonl_paths(args.buys)
    lookup_sidecar_rows = read_jsonl_paths(args.lookup_candidates)
    lookup_rows = load_lookup_rows(lookup_sidecar_rows, decision_rows, buy_rows)
    funding_events, funding_rows_scanned, funding_events_parsed = (
        stream_funding_events_for_lookup_wallets(
            args.funding_events,
            (lookup.lookup_wallet for lookup in lookup_rows),
        )
    )
    audit_rows = audit_lookup_rows(lookup_rows, funding_events)
    write_csv(args.output_csv, audit_rows)
    write_markdown(
        args.output_md,
        audit_rows,
        funding_events_count=len(funding_events),
        lookup_rows_count=len(lookup_rows),
        funding_rows_scanned=funding_rows_scanned,
        funding_events_parsed=funding_events_parsed,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
