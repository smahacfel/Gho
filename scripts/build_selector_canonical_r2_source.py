#!/usr/bin/env python3
"""Export canonical DIAG account-state evidence into selector R2 source JSONL."""

from __future__ import annotations

import argparse
import glob
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


LAMPORTS_PER_SOL = 1_000_000_000
PUMP_TOKEN_DECIMAL_FACTOR = 1_000_000
DIAG_ACCOUNT_UPDATE_RELAY_RE = re.compile(
    r"^(?P<timestamp>\S+).*\bDIAG_ACCOUNT_UPDATE_RELAY\b "
    r"base_mint=(?P<base_mint>\S+) bonding_curve=(?P<bonding_curve>\S+) "
    r"slot=(?P<slot>\d+) sol_reserves=(?P<sol_reserves>\d+) "
    r"token_reserves=(?P<token_reserves>\d+) complete=(?P<complete>\d+) "
    r"curve_finality=(?P<curve_finality>\S+)"
)
ISO_TS_RE = re.compile(
    r"^(?P<head>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?P<fraction>\.\d+)?(?P<tz>Z|[+-]\d{2}:\d{2})$"
)


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
    source_path: str
    source_line: int

    def spot_price_sol(self) -> float | None:
        if self.sol_reserves_lamports <= 0 or self.token_reserves_raw <= 0:
            return None
        return (self.sol_reserves_lamports / LAMPORTS_PER_SOL) / (
            self.token_reserves_raw / PUMP_TOKEN_DECIMAL_FACTOR
        )


def parse_iso_to_ms(value: str | None) -> int | None:
    if not isinstance(value, str):
        return None
    match = ISO_TS_RE.match(value.strip())
    if not match:
        return None
    fraction = match.group("fraction") or ""
    if fraction:
        fraction = "." + fraction[1:7].ljust(6, "0")
    tz = match.group("tz")
    if tz == "Z":
        tz = "+00:00"
    try:
        return int(datetime.fromisoformat(f"{match.group('head')}{fraction}{tz}").timestamp() * 1000)
    except ValueError:
        return None


def parse_diag_line(line: str, *, source_path: str, source_line: int) -> DiagUpdate | None:
    match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
    if not match:
        return None
    timestamp_ms = parse_iso_to_ms(match.group("timestamp"))
    if timestamp_ms is None:
        return None
    return DiagUpdate(
        timestamp_ms=timestamp_ms,
        base_mint=match.group("base_mint"),
        bonding_curve=match.group("bonding_curve"),
        slot=int(match.group("slot")),
        sol_reserves_lamports=int(match.group("sol_reserves")),
        token_reserves_raw=int(match.group("token_reserves")),
        complete=int(match.group("complete")),
        curve_finality=match.group("curve_finality"),
        source_path=source_path,
        source_line=source_line,
    )


def expand_paths(root: Path, explicit_paths: Iterable[Path], patterns: Iterable[str]) -> list[Path]:
    paths: dict[str, Path] = {}
    for path in explicit_paths:
        resolved = path if path.is_absolute() else root / path
        paths[str(resolved)] = resolved
    for pattern in patterns:
        probe = pattern if Path(pattern).is_absolute() else str(root / pattern)
        for raw in glob.glob(probe):
            path = Path(raw)
            paths[str(path)] = path
    return sorted(paths.values(), key=lambda path: str(path))


def load_diag_updates(paths: list[Path]) -> tuple[dict[tuple[str, str], list[DiagUpdate]], dict[str, Any]]:
    by_key: dict[tuple[str, str], list[DiagUpdate]] = defaultdict(list)
    rows_read = 0
    parsed_rows = 0
    invalid_price_rows = 0
    missing_paths = []
    for path in paths:
        if not path.exists() or not path.is_file():
            missing_paths.append(str(path))
            continue
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line_number, line in enumerate(fh, start=1):
                rows_read += 1
                update = parse_diag_line(line, source_path=str(path), source_line=line_number)
                if update is None:
                    continue
                parsed_rows += 1
                if update.spot_price_sol() is None:
                    invalid_price_rows += 1
                    continue
                by_key[(update.base_mint, update.bonding_curve)].append(update)
    for updates in by_key.values():
        updates.sort(key=lambda item: (item.timestamp_ms, item.slot, item.source_line))
    return by_key, {
        "diag_log_paths": [str(path) for path in paths],
        "diag_log_missing_paths": missing_paths,
        "diag_log_line_rows_read": rows_read,
        "diag_rows_parsed": parsed_rows,
        "diag_rows_invalid_price": invalid_price_rows,
        "diag_identity_keys": len(by_key),
    }


def candidate_key(candidate: dict[str, Any]) -> tuple[str, str] | None:
    base_mint = common.str_or_none(candidate.get("base_mint") or candidate.get("mint_id"))
    bonding_curve = common.str_or_none(candidate.get("bonding_curve"))
    if base_mint and bonding_curve:
        return base_mint, bonding_curve
    return None


def source_sample(update: DiagUpdate, *, decision_ts_ms: int) -> dict[str, Any]:
    return {
        "ts_ms": update.timestamp_ms,
        "offset_ms": update.timestamp_ms - decision_ts_ms,
        "slot": update.slot,
        "price": update.spot_price_sol(),
        "sol_reserves_lamports": update.sol_reserves_lamports,
        "token_reserves_raw": update.token_reserves_raw,
        "complete": update.complete,
        "curve_finality": update.curve_finality,
        "source_path": update.source_path,
        "source_line": update.source_line,
    }


def select_samples(
    updates: list[DiagUpdate],
    *,
    decision_ts_ms: int,
    horizon_ms: int,
    pre_decision_ms: int,
) -> list[dict[str, Any]]:
    start_ts = decision_ts_ms - max(pre_decision_ms, 0)
    end_ts = decision_ts_ms + horizon_ms
    samples = [
        source_sample(update, decision_ts_ms=decision_ts_ms)
        for update in updates
        if start_ts <= update.timestamp_ms <= end_ts
    ]
    return [sample for sample in samples if sample.get("price") is not None]


def count_stream_gaps(samples: list[dict[str, Any]], *, max_sample_gap_ms: int) -> int:
    if max_sample_gap_ms <= 0 or len(samples) < 2:
        return 0
    timestamps = [
        ts for sample in samples if (ts := common.int_or_none(sample.get("ts_ms"))) is not None
    ]
    timestamps.sort()
    return sum(1 for left, right in zip(timestamps, timestamps[1:]) if right - left > max_sample_gap_ms)


def build_source_rows(
    *,
    candidate_universe: Path,
    diag_log_paths: list[Path],
    horizon_ms: int,
    pre_decision_ms: int,
    max_sample_gap_ms: int,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    candidates = list(common.iter_json_objects(candidate_universe))
    updates_by_key, diag_report = load_diag_updates(diag_log_paths)
    key_counts = Counter(key for key in (candidate_key(candidate) for candidate in candidates) if key)
    rows: list[dict[str, Any]] = []
    candidate_status = Counter()
    candidate_unmatched_samples: list[dict[str, Any]] = []
    ambiguous_identity_samples: list[dict[str, Any]] = []
    for candidate in candidates:
        candidate_id = common.str_or_none(candidate.get("candidate_id"))
        key = candidate_key(candidate)
        decision_ts_ms = common.int_or_none(candidate.get("decision_ts_ms"))
        if not candidate_id or key is None:
            candidate_status["candidate_identity_incomplete"] += 1
            continue
        if key_counts[key] > 1:
            candidate_status["candidate_identity_ambiguous"] += 1
            if len(ambiguous_identity_samples) < 10:
                ambiguous_identity_samples.append({"candidate_id": candidate_id, "base_mint": key[0], "bonding_curve": key[1]})
            continue
        if decision_ts_ms is None:
            candidate_status["candidate_missing_decision_ts_ms"] += 1
            continue
        updates = updates_by_key.get(key, [])
        if not updates:
            candidate_status["missing_path"] += 1
            if len(candidate_unmatched_samples) < 10:
                candidate_unmatched_samples.append({"candidate_id": candidate_id, "base_mint": key[0], "bonding_curve": key[1]})
            continue
        samples = select_samples(
            updates,
            decision_ts_ms=decision_ts_ms,
            horizon_ms=horizon_ms,
            pre_decision_ms=pre_decision_ms,
        )
        if not samples:
            candidate_status["missing_path_in_window"] += 1
            continue
        stream_gap_count = count_stream_gaps(samples, max_sample_gap_ms=max_sample_gap_ms)
        max_offset = max(
            offset for sample in samples if (offset := common.int_or_none(sample.get("offset_ms"))) is not None
        )
        horizon_matured = max_offset >= horizon_ms
        path_coverage_ok = stream_gap_count == 0
        if not path_coverage_ok:
            path_status = "stream_incomplete"
        elif not horizon_matured:
            path_status = "horizon_unmatured"
        else:
            path_status = "ok"
        candidate_status[path_status] += 1
        rows.append(
            {
                "selector_schema_version": common.SCHEMA_VERSION,
                "canonical_r2_source_schema_version": common.SCHEMA_VERSION,
                "candidate_id": candidate_id,
                "base_mint": key[0],
                "mint": key[0],
                "pool_id": candidate.get("pool_id"),
                "bonding_curve": key[1],
                "decision_ts_ms": decision_ts_ms,
                "decision_slot": candidate.get("decision_slot") or candidate.get("birth_slot"),
                "path_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                "canonical_stream_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                "path_source_provenance": "canonical_stream",
                "path_status": path_status,
                "path_coverage_ok": path_coverage_ok,
                "horizon_matured": horizon_matured,
                "stream_gap_count": stream_gap_count,
                "restart_gap_count": 0,
                "gap_classification": "sample_gap" if stream_gap_count else None,
                "censoring_status": None if horizon_matured else "horizon_unmatured",
                "source_record_count": len(samples),
                "source_update_count_total_for_identity": len(updates),
                "samples": samples,
            }
        )
    source_rows_written = len(rows)
    fail_reasons = []
    if not diag_log_paths:
        fail_reasons.append("no_diag_log_paths")
    if diag_report["diag_rows_parsed"] == 0:
        fail_reasons.append("no_diag_account_update_relay_rows")
    if source_rows_written == 0:
        fail_reasons.append("no_candidate_matched_canonical_source")
    if candidate_status.get("ok", 0) == 0:
        fail_reasons.append("no_horizon_matured_candidate_paths")
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "canonical_r2_source_manifest_v1",
        "status": "PASS" if not fail_reasons else "NO-GO/PENDING_R2_SOURCE",
        "fail_reasons": fail_reasons,
        "candidate_universe": str(candidate_universe),
        "candidate_universe_rows": len(candidates),
        "horizon_ms": horizon_ms,
        "pre_decision_ms": pre_decision_ms,
        "max_sample_gap_ms": max_sample_gap_ms,
        "source_kind": "diag_account_update_relay_to_canonical_snapshot_jsonl",
        "output_contract": "compatible_with_build_selector_r2_market_paths --canonical-snapshot-jsonl",
        "r2_ssot": True,
        "forbidden_sources_not_used": [
            "NLN Program Streams",
            "pumpfun.trade reserves",
            "system.transfers",
            "decision logs",
            "accepted lifecycle rows",
            "unflagged RPC hydration",
            "shadow execution outcome",
            "shadow_ledger_snapshot_*.bin",
        ],
        "diag_report": diag_report,
        "source_rows_written": source_rows_written,
        "candidate_status_counts": common.counter_dict(candidate_status),
        "candidate_missing_path_rows": candidate_status.get("missing_path", 0)
        + candidate_status.get("missing_path_in_window", 0),
        "candidate_horizon_unmatured_rows": candidate_status.get("horizon_unmatured", 0),
        "candidate_stream_incomplete_rows": candidate_status.get("stream_incomplete", 0),
        "candidate_ok_rows": candidate_status.get("ok", 0),
        "candidate_unmatched_samples": candidate_unmatched_samples,
        "ambiguous_identity_samples": ambiguous_identity_samples,
    }
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--candidate-universe", required=True, type=Path)
    parser.add_argument("--diag-log", type=Path, action="append", default=[])
    parser.add_argument("--diag-log-glob", action="append", default=[])
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument("--horizon-ms", required=True, type=int)
    parser.add_argument("--pre-decision-ms", type=int, default=0)
    parser.add_argument("--max-sample-gap-ms", type=int, default=0)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    diag_log_paths = expand_paths(args.root, args.diag_log, args.diag_log_glob)
    rows, manifest = build_source_rows(
        candidate_universe=args.candidate_universe,
        diag_log_paths=diag_log_paths,
        horizon_ms=args.horizon_ms,
        pre_decision_ms=args.pre_decision_ms,
        max_sample_gap_ms=args.max_sample_gap_ms,
    )
    manifest["output"] = str(args.output)
    manifest["rows_written"] = common.write_jsonl(args.output, rows)
    if args.manifest_output:
        common.write_json(args.manifest_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
