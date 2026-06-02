#!/usr/bin/env python3
"""Build selector candidate_universe_v1 JSONL from offline evidence artifacts."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


def load_source_rows(
    paths: list[Path],
    *,
    source_kind: str,
    require_birth_event: bool,
    allow_degraded_events: bool = False,
    window_start_ms: int | None = None,
    window_end_ms: int | None = None,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    skipped = Counter()
    rows_read = 0
    for path in paths:
        for index, row in enumerate(common.iter_json_objects(path), start=1):
            rows_read += 1
            is_birth = common.is_birth_create_event(row)
            if require_birth_event and not is_birth and not allow_degraded_events:
                skipped["non_birth_create_event"] += 1
                continue
            item = common.candidate_universe_row(
                row,
                source_path=str(path),
                source_index=index,
            )
            window_ts = common.int_or_none(
                item.get("birth_ts_ms") if require_birth_event else item.get("decision_ts_ms")
            )
            if window_start_ms is not None or window_end_ms is not None:
                if window_ts is None:
                    skipped["missing_window_timestamp"] += 1
                    continue
                if window_start_ms is not None and window_ts < window_start_ms:
                    skipped["before_window"] += 1
                    continue
                if window_end_ms is not None and window_ts > window_end_ms:
                    skipped["after_window"] += 1
                    continue
            item["universe_source_kind"] = source_kind
            item["birth_create_event_verified"] = is_birth
            rows.append(item)
    return rows, {
        "rows_read": rows_read,
        "rows_loaded": len(rows),
        "skipped_counts": common.counter_dict(skipped),
    }


def build_universe(
    *,
    event_paths: list[Path],
    decision_paths: list[Path],
    allow_degraded_events: bool = False,
    allow_decision_universe: bool = False,
    allow_incomplete_universe: bool = False,
    window_start_ms: int | None = None,
    window_end_ms: int | None = None,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    source_rows, event_load = load_source_rows(
        event_paths,
        source_kind="event_artifact" if not allow_degraded_events else "event_artifact_degraded",
        require_birth_event=True,
        allow_degraded_events=allow_degraded_events,
        window_start_ms=window_start_ms,
        window_end_ms=window_end_ms,
    )
    decision_load = {"rows_read": 0, "rows_loaded": 0, "skipped_counts": {}}
    decision_only_rows_skipped = 0
    decision_context_rows_joined = 0
    decision_context_rows_ambiguous = 0
    decision_context_join_key_counts: Counter[str] = Counter()
    decision_context_join_samples: list[dict[str, Any]] = []
    if decision_paths:
        decision_rows, decision_load = load_source_rows(
            decision_paths,
            source_kind="decision_log_context",
            require_birth_event=False,
            window_start_ms=window_start_ms,
            window_end_ms=window_end_ms,
        )
        if allow_decision_universe:
            for row in decision_rows:
                row["universe_source_kind"] = "decision_log_degraded"
            source_rows.extend(decision_rows)
        else:
            event_denominator_rows, _event_denominator_merge = common.merge_candidate_rows(source_rows)
            universe_index, universe_ambiguous = common.build_identity_join_index(
                event_denominator_rows
            )
            matched_decisions = []
            for row in decision_rows:
                matched, join_key, ambiguous = common.lookup_identity_join(
                    row, universe_index, universe_ambiguous
                )
                if matched is None:
                    decision_only_rows_skipped += 1
                    if ambiguous:
                        decision_context_rows_ambiguous += 1
                    continue
                normalized = dict(row)
                original_candidate_id = common.str_or_none(normalized.get("candidate_id"))
                normalized["candidate_id"] = matched.get("candidate_id")
                normalized["decision_context_join_key"] = join_key
                normalized["decision_context_original_candidate_id"] = original_candidate_id
                normalized["universe_source_kind"] = "decision_log_context"
                matched_decisions.append(normalized)
                decision_context_rows_joined += 1
                if join_key:
                    decision_context_join_key_counts[join_key.split(":", 1)[0]] += 1
                if len(decision_context_join_samples) < 20:
                    decision_context_join_samples.append(
                        {
                            "join_key": join_key,
                            "candidate_id": matched.get("candidate_id"),
                            "decision_candidate_id": original_candidate_id,
                            "base_mint": normalized.get("base_mint"),
                            "pool_id": normalized.get("pool_id"),
                        }
                    )
            source_rows.extend(matched_decisions)
    rows, merge_report = common.merge_candidate_rows(source_rows)
    status_counts = Counter(str(row.get("candidate_universe_status") or "unknown") for row in rows)
    quote_counts = Counter(str(row.get("quote_mint") or "missing") for row in rows)
    event_denominator_rows_after_dedupe = sum(
        1 for row in rows if row.get("universe_source_kind") != "decision_log_degraded"
    )
    decision_logs_created_denominator_rows = (
        max(0, len(rows) - event_denominator_rows_after_dedupe)
        if allow_decision_universe
        else 0
    )
    candidate_ids_from_decision_only = decision_logs_created_denominator_rows
    denominator_invariant_failures = []
    if decision_logs_created_denominator_rows != 0:
        denominator_invariant_failures.append("decision_logs_created_denominator_rows_nonzero")
    if candidate_ids_from_decision_only != 0:
        denominator_invariant_failures.append("candidate_ids_from_decision_only_nonzero")
    if not allow_decision_universe and event_denominator_rows_after_dedupe != len(rows):
        denominator_invariant_failures.append("rows_written_not_equal_event_denominator_rows_after_dedupe")
    fail_reasons = []
    if status_counts.get("ok", 0) == 0:
        fail_reasons.append("no_ok_birth_create_rows")
    if merge_report["collisions"]:
        fail_reasons.append("identity_collisions")
    if status_counts.get("universe_incomplete", 0) > 0 and not allow_incomplete_universe:
        fail_reasons.append("universe_incomplete_rows")
    if allow_degraded_events:
        fail_reasons.append("degraded_non_birth_events_allowed")
    if allow_decision_universe:
        fail_reasons.append("decision_log_used_as_universe_denominator")
    fail_reasons.extend(denominator_invariant_failures)
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "candidate_universe_v1",
        "status": "ok" if not fail_reasons else "NO-GO",
        "fail_reasons": fail_reasons,
        "scope_kind": "windowed" if window_start_ms is not None or window_end_ms is not None else "full",
        "window_start_ts_ms": window_start_ms,
        "window_end_ts_ms": window_end_ms,
        "window_filter": {
            "event_artifact_timestamp": "birth_ts_ms",
            "decision_context_timestamp": "decision_ts_ms",
            "missing_timestamp": "skipped_not_denominator",
        },
        "denominator_source": "event_artifact_only"
        if not allow_decision_universe
        else "decision_log_degraded_no_go",
        "event_denominator_rows_after_dedupe": event_denominator_rows_after_dedupe,
        "decision_logs_created_denominator_rows": decision_logs_created_denominator_rows,
        "candidate_ids_from_decision_only": candidate_ids_from_decision_only,
        "denominator_invariant_status": "PASS"
        if not denominator_invariant_failures
        else "NO-GO",
        "universe_contract": {
            "cohort": "SOL-paired pump.fun bonding-curve birth/create events",
            "birth_create_event_filter": sorted(common.BIRTH_EVENT_TYPES),
            "decision_logs": "context_only_not_denominator_by_default",
            "missing_birth_quote_or_timestamp": "universe_incomplete_fail_closed",
        },
        "r2_ssot_contract": {
            "canonical_sources": [
                "Yellowstone/Geyser AccountUpdates",
                "DIAG_ACCOUNT_UPDATE_RELAY",
                "canonical account-state snapshots",
            ],
            "rpc_policy": "rpc_is_backfill_or_enrichment_only_with_provenance_flag",
        },
        "input_event_paths": [str(path) for path in event_paths],
        "input_decision_paths": [str(path) for path in decision_paths],
        "event_load": event_load,
        "decision_load": decision_load,
        "decision_only_rows_skipped": decision_only_rows_skipped,
        "decision_context_rows_joined": decision_context_rows_joined,
        "decision_context_rows_ambiguous": decision_context_rows_ambiguous,
        "decision_context_join_key_counts": common.counter_dict(decision_context_join_key_counts),
        "decision_context_join_samples": decision_context_join_samples,
        "rows_loaded_for_merge": len(source_rows),
        "rows_written": len(rows),
        "status_counts": common.counter_dict(status_counts),
        "quote_mint_counts": common.counter_dict(quote_counts),
        "duplicates": merge_report["duplicates"],
        "identity_collisions": merge_report["collisions"],
    }
    return rows, manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--events", type=Path, action="append", default=[], help="event artifact JSONL")
    parser.add_argument(
        "--decisions",
        type=Path,
        action="append",
        default=[],
        help="optional decision JSONL used only as degraded context",
    )
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument(
        "--allow-degraded-events",
        action="store_true",
        help="NO-GO mode: include non-birth event rows for diagnostics only.",
    )
    parser.add_argument(
        "--allow-decision-universe",
        action="store_true",
        help="NO-GO mode: allow decision logs to create denominator rows.",
    )
    parser.add_argument(
        "--allow-incomplete-universe",
        action="store_true",
        help="Diagnostic mode: do not fail manifest solely on universe_incomplete rows.",
    )
    parser.add_argument("--window-start-ms", type=int)
    parser.add_argument("--window-end-ms", type=int)
    parser.add_argument("--json", action="store_true", help="print manifest JSON")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows, manifest = build_universe(
        event_paths=args.events,
        decision_paths=args.decisions,
        allow_degraded_events=args.allow_degraded_events,
        allow_decision_universe=args.allow_decision_universe,
        allow_incomplete_universe=args.allow_incomplete_universe,
        window_start_ms=args.window_start_ms,
        window_end_ms=args.window_end_ms,
    )
    common.write_jsonl(args.output, rows)
    if args.manifest_output:
        common.write_json(args.manifest_output, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = run(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "ok" else 2


if __name__ == "__main__":
    raise SystemExit(main())
