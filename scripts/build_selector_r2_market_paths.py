#!/usr/bin/env python3
"""Build canonical R2 market-path labels for selector candidates."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def _source_ts_ms(row: dict[str, Any]) -> int | None:
    return common.int_or_none(
        common.find_first_key(
            row,
            (
                "ts_ms",
                "timestamp_ms",
                "event_ts_ms",
                "account_update_ts_ms",
                "observed_ts_ms",
            ),
        )
    )


def _sample_price(row: dict[str, Any]) -> float | None:
    return common.float_or_none(
        common.find_first_key(
            row,
            (
                "price_at_decision",
                "price_sol",
                "price",
                "token_price_sol",
                "market_price_sol",
                "virtual_price_sol",
            ),
        )
    )


def _sample_return_pct(row: dict[str, Any]) -> float | None:
    return common.float_or_none(
        common.find_first_key(
            row,
            (
                "return_pct",
                "pnl_pct",
                "net_pnl_pct",
                "price_return_pct",
            ),
        )
    )


def _source_path_source(row: dict[str, Any], *, input_kind: str) -> str:
    explicit = common.str_or_none(
        common.first_top_level(
            row,
            (
                "path_source",
                "r2_source",
                "r2_canonical_source",
                "canonical_path_source",
                "canonical_stream_source",
            ),
        )
    )
    if explicit:
        return explicit
    if input_kind == "diag_account_update":
        return "DIAG_ACCOUNT_UPDATE_RELAY"
    if input_kind == "canonical_snapshot":
        return "canonical_account_state_snapshot"
    return "yellowstone_account_update"


def _record_is_canonical(row: dict[str, Any], *, input_kind: str) -> tuple[bool, str, str]:
    path_source = _source_path_source(row, input_kind=input_kind)
    probe = dict(row)
    probe["path_source"] = path_source
    canonical, provenance = common.r2_source_provenance(probe)
    return canonical, provenance, path_source


def load_source_records(paths: list[Path], *, input_kind: str) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    records: list[dict[str, Any]] = []
    rows_read = 0
    canonical_rows = 0
    noncanonical_rows = 0
    for path in paths:
        for index, row in enumerate(common.iter_json_objects(path), start=1):
            rows_read += 1
            record = dict(row)
            canonical, provenance, path_source = _record_is_canonical(record, input_kind=input_kind)
            record["_selector_source_path"] = str(path)
            record["_selector_source_index"] = index
            record["_selector_input_kind"] = input_kind
            record["_selector_path_source"] = path_source
            record["_selector_source_canonical"] = canonical
            record["_selector_source_provenance"] = provenance
            if canonical:
                canonical_rows += 1
            else:
                noncanonical_rows += 1
            records.append(record)
    return records, {
        "input_kind": input_kind,
        "input_paths": [str(path) for path in paths],
        "rows_read": rows_read,
        "rows_loaded": len(records),
        "canonical_rows": canonical_rows,
        "noncanonical_rows": noncanonical_rows,
    }


def match_records_to_candidates(
    candidates: list[dict[str, Any]],
    records: list[dict[str, Any]],
) -> tuple[dict[str, list[dict[str, Any]]], dict[str, Any]]:
    key_to_candidate: dict[str, str] = {}
    ambiguous_keys: set[str] = set()
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            continue
        for key in common.identity_join_keys(candidate):
            if key in key_to_candidate and key_to_candidate[key] != candidate_id:
                ambiguous_keys.add(key)
            else:
                key_to_candidate[key] = candidate_id
    matched: dict[str, list[dict[str, Any]]] = defaultdict(list)
    unmatched = 0
    ambiguous = 0
    for record in records:
        matched_id = None
        saw_ambiguous = False
        for key in common.identity_join_keys(record):
            if key in ambiguous_keys:
                saw_ambiguous = True
                continue
            if key in key_to_candidate:
                matched_id = key_to_candidate[key]
                break
        if matched_id:
            matched[matched_id].append(record)
        elif saw_ambiguous:
            ambiguous += 1
        else:
            unmatched += 1
    return matched, {
        "source_rows_matched": sum(len(rows) for rows in matched.values()),
        "source_rows_unmatched": unmatched,
        "source_rows_ambiguous": ambiguous,
    }


def _raw_sample_rows(record: dict[str, Any]) -> list[dict[str, Any]]:
    raw = record.get("samples") or record.get("price_path_samples") or record.get("lifecycle_price_samples")
    if isinstance(raw, list):
        return [item for item in raw if isinstance(item, dict)]
    return [record]


def _normalize_samples(
    records: list[dict[str, Any]],
    *,
    decision_ts_ms: int | None,
) -> tuple[list[dict[str, Any]], float | None]:
    raw_samples: list[dict[str, Any]] = []
    for record in records:
        for sample in _raw_sample_rows(record):
            item = dict(sample)
            if "ts_ms" not in item and "timestamp_ms" not in item:
                ts = _source_ts_ms(record)
                if ts is not None:
                    item["ts_ms"] = ts
            if "slot" not in item:
                slot = common.source_slot(record)
                if slot is not None:
                    item["slot"] = slot
            if "price" not in item and "price_sol" not in item:
                price = _sample_price(record)
                if price is not None:
                    item["price"] = price
            if "return_pct" not in item:
                ret = _sample_return_pct(record)
                if ret is not None:
                    item["return_pct"] = ret
            raw_samples.append(item)

    normalized: list[dict[str, Any]] = []
    for sample in raw_samples:
        ts_ms = common.int_or_none(sample.get("ts_ms") or sample.get("timestamp_ms"))
        offset_ms = common.int_or_none(sample.get("offset_ms"))
        if offset_ms is None and ts_ms is not None and decision_ts_ms is not None:
            offset_ms = ts_ms - decision_ts_ms
        price = common.float_or_none(sample.get("price_sol") or sample.get("price"))
        ret = common.float_or_none(sample.get("return_pct"))
        normalized.append(
            {
                "ts_ms": ts_ms,
                "offset_ms": offset_ms,
                "return_pct": ret,
                "price": price,
                "slot": common.int_or_none(sample.get("slot")),
            }
        )
    normalized.sort(key=lambda item: (item.get("offset_ms") is None, item.get("offset_ms") or 0))
    price_at_decision = _price_at_decision(normalized)
    if price_at_decision is not None:
        for sample in normalized:
            if sample.get("return_pct") is None:
                price = common.float_or_none(sample.get("price"))
                if price is not None and price_at_decision > 0.0:
                    sample["return_pct"] = ((price / price_at_decision) - 1.0) * 100.0
    normalized = [sample for sample in normalized if sample.get("return_pct") is not None]
    return normalized, price_at_decision


def _price_at_decision(samples: list[dict[str, Any]]) -> float | None:
    priced = [sample for sample in samples if common.float_or_none(sample.get("price")) is not None]
    if not priced:
        return None
    non_negative = [
        sample
        for sample in priced
        if (offset := common.int_or_none(sample.get("offset_ms"))) is not None and offset >= 0
    ]
    chosen = non_negative[0] if non_negative else priced[0]
    return common.float_or_none(chosen.get("price"))


def _first_hit_ts(
    samples: list[dict[str, Any]],
    *,
    decision_ts_ms: int | None,
    horizon_ms: int,
    threshold: float,
    side: str,
) -> int | None:
    for sample in samples:
        offset = common.int_or_none(sample.get("offset_ms"))
        if offset is not None and (offset < 0 or offset > horizon_ms):
            continue
        ret = common.float_or_none(sample.get("return_pct"))
        if ret is None:
            continue
        hit = ret >= threshold if side == "target" else ret <= -abs(threshold)
        if not hit:
            continue
        ts_ms = common.int_or_none(sample.get("ts_ms"))
        if ts_ms is not None:
            return ts_ms
        if decision_ts_ms is not None and offset is not None:
            return decision_ts_ms + offset
        return None
    return None


def _path_summary(
    samples: list[dict[str, Any]],
    *,
    decision_ts_ms: int | None,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
    price_at_decision: float | None,
) -> dict[str, Any]:
    prices = [price for sample in samples if (price := common.float_or_none(sample.get("price"))) is not None]
    returns = [ret for sample in samples if (ret := common.float_or_none(sample.get("return_pct"))) is not None]
    timestamps = [ts for sample in samples if (ts := common.int_or_none(sample.get("ts_ms"))) is not None]
    target_hit_ts = _first_hit_ts(
        samples,
        decision_ts_ms=decision_ts_ms,
        horizon_ms=horizon_ms,
        threshold=target_net_pct,
        side="target",
    )
    stop_hit_ts = _first_hit_ts(
        samples,
        decision_ts_ms=decision_ts_ms,
        horizon_ms=horizon_ms,
        threshold=stop_net_pct,
        side="stop",
    )
    return {
        "path_start_ts_ms": min(timestamps) if timestamps else None,
        "path_end_ts_ms": max(timestamps) if timestamps else None,
        "price_at_decision": price_at_decision,
        "max_price": max(prices) if prices else None,
        "min_price": min(prices) if prices else None,
        "max_favorable_pnl_pct": max(returns) if returns else None,
        "max_adverse_pnl_pct": min(returns) if returns else None,
        "target_hit_ts_ms": target_hit_ts,
        "stop_hit_ts_ms": stop_hit_ts,
        "target_before_stop": (
            target_hit_ts is not None and (stop_hit_ts is None or target_hit_ts <= stop_hit_ts)
        ),
    }


def _aggregate_flag(records: list[dict[str, Any]], field: str, default: int = 0) -> int:
    values = [common.int_or_none(record.get(field)) for record in records]
    values = [value for value in values if value is not None]
    return max(values) if values else default


def _coverage_ok(records: list[dict[str, Any]], samples: list[dict[str, Any]]) -> bool:
    explicit = [common.bool_or_none(record.get("path_coverage_ok")) for record in records]
    explicit = [value for value in explicit if value is not None]
    if False in explicit:
        return False
    if True in explicit:
        return True
    statuses = [common.str_or_none(record.get("path_status")) for record in records]
    if "stream_incomplete" in statuses or "missing_path" in statuses:
        return False
    return bool(samples)


def _horizon_matured(records: list[dict[str, Any]], samples: list[dict[str, Any]], horizon_ms: int) -> bool:
    explicit = [common.bool_or_none(record.get("horizon_matured")) for record in records]
    explicit = [value for value in explicit if value is not None]
    if False in explicit:
        return False
    if True in explicit:
        return True
    offsets = [offset for sample in samples if (offset := common.int_or_none(sample.get("offset_ms"))) is not None]
    return bool(offsets) and max(offsets) >= horizon_ms


def missing_path_row(
    candidate: dict[str, Any],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    return base_r2_row(
        candidate,
        target_net_pct=target_net_pct,
        stop_net_pct=stop_net_pct,
        horizon_ms=horizon_ms,
        path_source=None,
        path_source_kind=None,
        path_source_provenance="missing_path",
        path_coverage_ok=False,
        horizon_matured=False,
        r2_status="missing_path",
        r2_label=None,
        r2_excluded_reason="no_canonical_market_path",
    )


def noncanonical_source_row(
    candidate: dict[str, Any],
    records: list[dict[str, Any]],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    first = records[0]
    return base_r2_row(
        candidate,
        target_net_pct=target_net_pct,
        stop_net_pct=stop_net_pct,
        horizon_ms=horizon_ms,
        path_source=first.get("_selector_path_source"),
        path_source_kind=first.get("_selector_input_kind"),
        path_source_provenance=first.get("_selector_source_provenance") or "noncanonical_path_source",
        path_coverage_ok=False,
        horizon_matured=False,
        r2_status="noncanonical_source",
        r2_label=None,
        r2_excluded_reason=first.get("_selector_source_provenance") or "noncanonical_source_only",
    )


def base_r2_row(
    candidate: dict[str, Any],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
    path_source: str | None,
    path_source_kind: str | None,
    path_source_provenance: str,
    path_coverage_ok: bool,
    horizon_matured: bool,
    r2_status: str,
    r2_label: str | None,
    r2_excluded_reason: str | None,
) -> dict[str, Any]:
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "r2_market_path_schema_version": common.SCHEMA_VERSION,
        "candidate_id": candidate.get("candidate_id"),
        "mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "pool_id": candidate.get("pool_id"),
        "bonding_curve": candidate.get("bonding_curve"),
        "decision_ts_ms": candidate.get("decision_ts_ms"),
        "decision_slot": candidate.get("decision_slot") or candidate.get("birth_slot"),
        "path_source": path_source,
        "path_source_kind": path_source_kind,
        "path_source_provenance": path_source_provenance,
        "path_start_ts_ms": None,
        "path_end_ts_ms": None,
        "path_coverage_ok": path_coverage_ok,
        "horizon_matured": horizon_matured,
        "stream_gap_count": 0,
        "restart_gap_count": 0,
        "price_at_decision": None,
        "max_price": None,
        "min_price": None,
        "max_favorable_pnl_pct": None,
        "max_adverse_pnl_pct": None,
        "target_hit_ts_ms": None,
        "stop_hit_ts_ms": None,
        "target_before_stop": False,
        "r2_status": r2_status,
        "r2_label": r2_label,
        "r2_label_reason": None,
        "r2_excluded_reason": r2_excluded_reason,
        "target_net_pct": target_net_pct,
        "stop_net_pct": stop_net_pct,
        "horizon_ms": horizon_ms,
        "samples": [],
    }


def canonical_path_row(
    candidate: dict[str, Any],
    records: list[dict[str, Any]],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    decision_ts = common.int_or_none(candidate.get("decision_ts_ms"))
    samples, price_at_decision = _normalize_samples(records, decision_ts_ms=decision_ts)
    first = records[0]
    path_coverage_ok = _coverage_ok(records, samples)
    horizon_matured = _horizon_matured(records, samples, horizon_ms)
    path_source = first.get("_selector_path_source")
    path_source_kind = first.get("_selector_input_kind")
    path_source_provenance = first.get("_selector_source_provenance") or "canonical_stream"
    normalized_path = {
        "candidate_id": candidate.get("candidate_id"),
        "path_source": path_source,
        "path_status": "ok" if path_coverage_ok else "stream_incomplete",
        "path_coverage_ok": path_coverage_ok,
        "horizon_matured": horizon_matured,
        "samples": samples,
    }
    r2 = common.classify_r2(
        normalized_path,
        target_net_pct=target_net_pct,
        stop_net_pct=stop_net_pct,
        horizon_ms=horizon_ms,
    )
    r2_label = common.str_or_none(r2.get("r2_label"))
    r2_status = r2_label if r2_label in {"positive", "negative"} else str(r2.get("r2_status"))
    row = base_r2_row(
        candidate,
        target_net_pct=target_net_pct,
        stop_net_pct=stop_net_pct,
        horizon_ms=horizon_ms,
        path_source=path_source,
        path_source_kind=path_source_kind,
        path_source_provenance=path_source_provenance,
        path_coverage_ok=bool(r2.get("r2_path_coverage_ok")),
        horizon_matured=bool(r2.get("r2_horizon_matured")),
        r2_status=r2_status,
        r2_label=r2_label,
        r2_excluded_reason=r2.get("r2_excluded_reason"),
    )
    row.update(
        _path_summary(
            samples,
            decision_ts_ms=decision_ts,
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
            price_at_decision=price_at_decision,
        )
    )
    row.update(
        {
            "r2_label_reason": r2.get("r2_label_reason"),
            "stream_gap_count": _aggregate_flag(records, "stream_gap_count"),
            "restart_gap_count": _aggregate_flag(records, "restart_gap_count"),
            "source_record_count": len(records),
            "samples": samples,
        }
    )
    return row


def build_r2_market_paths(
    *,
    candidate_universe: Path,
    account_update_paths: list[Path],
    diag_account_update_paths: list[Path],
    canonical_snapshot_paths: list[Path],
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    candidates = list(common.iter_json_objects(candidate_universe))
    records: list[dict[str, Any]] = []
    input_reports = []
    for input_kind, paths in (
        ("account_update", account_update_paths),
        ("diag_account_update", diag_account_update_paths),
        ("canonical_snapshot", canonical_snapshot_paths),
    ):
        loaded, report = load_source_records(paths, input_kind=input_kind)
        records.extend(loaded)
        input_reports.append(report)
    matched, match_report = match_records_to_candidates(candidates, records)
    rows: list[dict[str, Any]] = []
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        if not candidate_id:
            continue
        candidate_records = matched.get(candidate_id, [])
        canonical = [record for record in candidate_records if record.get("_selector_source_canonical") is True]
        noncanonical = [
            record for record in candidate_records if record.get("_selector_source_canonical") is not True
        ]
        if canonical:
            rows.append(
                canonical_path_row(
                    candidate,
                    canonical,
                    target_net_pct=target_net_pct,
                    stop_net_pct=stop_net_pct,
                    horizon_ms=horizon_ms,
                )
            )
        elif noncanonical:
            rows.append(
                noncanonical_source_row(
                    candidate,
                    noncanonical,
                    target_net_pct=target_net_pct,
                    stop_net_pct=stop_net_pct,
                    horizon_ms=horizon_ms,
                )
            )
        else:
            rows.append(
                missing_path_row(
                    candidate,
                    target_net_pct=target_net_pct,
                    stop_net_pct=stop_net_pct,
                    horizon_ms=horizon_ms,
                )
            )

    status_counts = Counter(str(row.get("r2_status") or "unknown") for row in rows)
    label_counts = Counter(str(row.get("r2_label") or "unresolved") for row in rows)
    resolved_rows = sum(1 for row in rows if row.get("r2_label") in {"positive", "negative"})
    canonical_source_rows = sum(report["canonical_rows"] for report in input_reports)
    noncanonical_source_rows = sum(report["noncanonical_rows"] for report in input_reports)
    fail_reasons = []
    if resolved_rows == 0:
        fail_reasons.append("no_resolved_r2_denominator")
    if canonical_source_rows == 0:
        fail_reasons.append("no_canonical_market_path_source")
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "r2_market_path_coverage_v1",
        "status": "PASS" if not fail_reasons else "NO-GO/PENDING_R2_DENOMINATOR",
        "fail_reasons": fail_reasons,
        "candidate_universe": str(candidate_universe),
        "candidate_universe_rows": len(candidates),
        "rows_written": len(rows),
        "r2_config": {
            "profile": "r2_40_40_60s_v1",
            "target_net_pct": target_net_pct,
            "stop_net_pct": stop_net_pct,
            "horizon_ms": horizon_ms,
            "source": "phase2_manifest",
        },
        "input_reports": input_reports,
        "match_report": match_report,
        "canonical_source_rows": canonical_source_rows,
        "noncanonical_source_rows": noncanonical_source_rows,
        "r2_resolved_rows": resolved_rows,
        "r2_positive_rows": label_counts.get("positive", 0),
        "r2_negative_rows": label_counts.get("negative", 0),
        "r2_missing_path_rows": status_counts.get("missing_path", 0),
        "r2_noncanonical_source_rows": status_counts.get("noncanonical_source", 0),
        "r2_stream_incomplete_rows": status_counts.get("stream_incomplete", 0),
        "r2_horizon_unmatured_rows": status_counts.get("horizon_unmatured", 0),
        "r2_status_counts": common.counter_dict(status_counts),
        "r2_label_counts": common.counter_dict(label_counts),
        "r2_ssot_contract": {
            "canonical_sources": [
                "Yellowstone/Geyser AccountUpdates",
                "DIAG_ACCOUNT_UPDATE_RELAY",
                "canonical account-state snapshots",
            ],
            "forbidden_sources": [
                "NLN Program Streams",
                "pumpfun.trade reserves",
                "system.transfers",
                "decision logs",
                "accepted lifecycle rows",
                "unflagged RPC hydration",
                "shadow execution outcome",
                "shadow_ledger_snapshot_*.bin",
            ],
            "rpc_policy": "RPC may be flagged backfill/enrichment only and cannot resolve R2 alone.",
        },
        "row_contract": "one_r2_row_per_candidate_even_when_missing_path",
    }
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--account-update", type=Path, action="append", default=[])
    parser.add_argument("--diag-account-update", type=Path, action="append", default=[])
    parser.add_argument("--canonical-snapshot-jsonl", type=Path, action="append", default=[])
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--coverage-output", type=Path)
    parser.add_argument("--target-net-pct", required=True, type=float)
    parser.add_argument("--stop-net-pct", required=True, type=float)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, manifest = build_r2_market_paths(
        candidate_universe=args.candidate_universe,
        account_update_paths=args.account_update,
        diag_account_update_paths=args.diag_account_update,
        canonical_snapshot_paths=args.canonical_snapshot_jsonl,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
    )
    common.write_jsonl(args.output, rows)
    if args.coverage_output:
        common.write_json(args.coverage_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
