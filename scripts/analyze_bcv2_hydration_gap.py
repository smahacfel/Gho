#!/usr/bin/env python3
"""Analyze BCV2 exact-watch / RPC hydration evidence from Ghost logs.

This is an offline audit tool. It does not call RPC, does not emit runtime
evidence, and does not infer per-pubkey subscribe inclusion from aggregate
provider counters without marking it as audit-only inference.
"""

from __future__ import annotations

import argparse
import glob
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = "bcv2_hydration_gap_v1"
SYSTEM_PROGRAM = "11111111111111111111111111111111"

FIELD_RE = re.compile(r"([A-Za-z0-9_]+)=([^ \t]+)")


def parse_fields(line: str) -> dict[str, str]:
    return {match.group(1): match.group(2).rstrip(",") for match in FIELD_RE.finditer(line)}


def parse_int(value: str | None) -> int | None:
    if value is None or value == "none":
        return None
    try:
        return int(value)
    except ValueError:
        return None


def unwrap_option(value: str | None) -> str | None:
    if value is None or value == "None":
        return None
    if value.startswith('Some("') and value.endswith('")'):
        return value[len('Some("') : -2]
    if value.startswith("Some(") and value.endswith(")"):
        return value[len("Some(") : -1]
    return value


@dataclass
class SubscribeEvent:
    line_no: int
    from_slot: int | None
    bcv2_sent: int
    bcv2_dropped: int
    tracked_bcv2: int
    marker: str


@dataclass
class Bcv2Record:
    pubkey: str
    signatures: set[str] = field(default_factory=set)
    buy_variants: Counter[str] = field(default_factory=Counter)
    provenance_statuses: Counter[str] = field(default_factory=Counter)
    registered_count: int = 0
    first_registered_line: int | None = None
    last_registered_line: int | None = None
    observed_slot_min: int | None = None
    observed_slot_max: int | None = None
    registry_version_min: int | None = None
    registry_version_max: int | None = None
    source_instruction_indexes: Counter[str] = field(default_factory=Counter)
    instruction_account_positions: Counter[str] = field(default_factory=Counter)
    message_account_indexes: Counter[str] = field(default_factory=Counter)
    resolved_pubkeys: Counter[str] = field(default_factory=Counter)

    hydration_ready_count: int = 0
    hydration_missing_count: int = 0
    hydration_error_classes: Counter[str] = field(default_factory=Counter)
    hydration_attempts: Counter[str] = field(default_factory=Counter)
    hydration_context_slot_min: int | None = None
    hydration_context_slot_max: int | None = None
    hydration_latency_ms_max: int | None = None
    ready_owners: Counter[str] = field(default_factory=Counter)
    ready_data_lens: Counter[str] = field(default_factory=Counter)

    account_update_count: int = 0
    account_update_owner_counts: Counter[str] = field(default_factory=Counter)
    account_update_data_len_counts: Counter[str] = field(default_factory=Counter)
    account_update_slot_min: int | None = None
    account_update_slot_max: int | None = None

    enrich_signatures: set[str] = field(default_factory=set)
    enrich_buy_variants: Counter[str] = field(default_factory=Counter)
    enrich_bcv2_values: Counter[str] = field(default_factory=Counter)

    included_in_subscribe_inferred: bool = False
    dropped_over_cap_inferred: bool = False
    subscribe_inference_note: str | None = None

    def update_min_max(self, attr_min: str, attr_max: str, value: int | None) -> None:
        if value is None:
            return
        current_min = getattr(self, attr_min)
        current_max = getattr(self, attr_max)
        setattr(self, attr_min, value if current_min is None else min(current_min, value))
        setattr(self, attr_max, value if current_max is None else max(current_max, value))


def record_for(records: dict[str, Bcv2Record], pubkey: str) -> Bcv2Record:
    if pubkey not in records:
        records[pubkey] = Bcv2Record(pubkey=pubkey)
    return records[pubkey]


def iter_log_lines(paths: Iterable[Path]) -> Iterable[tuple[Path, int, str]]:
    for path in paths:
        with path.open("r", encoding="utf-8", errors="replace") as handle:
            for line_no, line in enumerate(handle, 1):
                yield path, line_no, line.rstrip("\n")


def analyze_paths(paths: Iterable[Path]) -> dict[str, Any]:
    records: dict[str, Bcv2Record] = {}
    subscribe_events: list[SubscribeEvent] = []
    signature_to_pubkeys: dict[str, set[str]] = defaultdict(set)
    marker_counts: Counter[str] = Counter()
    input_paths: list[str] = []
    total_lines = 0

    for path, line_no, line in iter_log_lines(paths):
        if not input_paths or input_paths[-1] != str(path):
            input_paths.append(str(path))
        total_lines += 1

        if "BCV2_EXACT_WATCH_REGISTERED" in line:
            marker_counts["BCV2_EXACT_WATCH_REGISTERED"] += 1
            fields = parse_fields(line)
            pubkey = fields.get("pubkey")
            if not pubkey:
                marker_counts["registered_missing_pubkey"] += 1
                continue
            rec = record_for(records, pubkey)
            rec.registered_count += 1
            rec.first_registered_line = line_no if rec.first_registered_line is None else min(rec.first_registered_line, line_no)
            rec.last_registered_line = line_no if rec.last_registered_line is None else max(rec.last_registered_line, line_no)
            signature = fields.get("signature")
            if signature:
                rec.signatures.add(signature)
                signature_to_pubkeys[signature].add(pubkey)
            buy_variant = fields.get("buy_variant")
            if buy_variant:
                rec.buy_variants[buy_variant] += 1
            rec.update_min_max("observed_slot_min", "observed_slot_max", parse_int(fields.get("observed_slot")))
            rec.update_min_max("registry_version_min", "registry_version_max", parse_int(fields.get("registry_version")))
            instruction_index = fields.get("instruction_index")
            if instruction_index:
                rec.source_instruction_indexes[instruction_index] += 1
            continue

        if "BCV2_RPC_HYDRATION_READY" in line or "BCV2_RPC_HYDRATION_MISSING" in line:
            ready = "BCV2_RPC_HYDRATION_READY" in line
            marker_counts["BCV2_RPC_HYDRATION_READY" if ready else "BCV2_RPC_HYDRATION_MISSING"] += 1
            fields = parse_fields(line)
            pubkey = fields.get("pubkey")
            if not pubkey:
                marker_counts["hydration_missing_pubkey"] += 1
                continue
            rec = record_for(records, pubkey)
            signature = fields.get("signature")
            if signature:
                rec.signatures.add(signature)
                signature_to_pubkeys[signature].add(pubkey)
            context_slot = parse_int(fields.get("context_slot"))
            rec.update_min_max("hydration_context_slot_min", "hydration_context_slot_max", context_slot)
            latency_ms = parse_int(fields.get("latency_ms"))
            if latency_ms is not None:
                rec.hydration_latency_ms_max = (
                    latency_ms
                    if rec.hydration_latency_ms_max is None
                    else max(rec.hydration_latency_ms_max, latency_ms)
                )
            attempt = fields.get("attempt")
            attempt_count = fields.get("attempt_count")
            if attempt or attempt_count:
                rec.hydration_attempts[f"{attempt or 'none'}/{attempt_count or 'none'}"] += 1
            if ready:
                rec.hydration_ready_count += 1
                owner = fields.get("owner")
                data_len = fields.get("data_len")
                if owner:
                    rec.ready_owners[owner] += 1
                if data_len:
                    rec.ready_data_lens[data_len] += 1
            else:
                rec.hydration_missing_count += 1
                rec.hydration_error_classes[fields.get("error_class", "unknown")] += 1
            continue

        if "BCV2_ACCOUNT_UPDATE_RECEIVED" in line:
            marker_counts["BCV2_ACCOUNT_UPDATE_RECEIVED"] += 1
            fields = parse_fields(line)
            pubkey = fields.get("pubkey")
            if not pubkey:
                marker_counts["account_update_missing_pubkey"] += 1
                continue
            rec = record_for(records, pubkey)
            rec.account_update_count += 1
            owner = fields.get("owner")
            data_len = fields.get("data_len")
            if owner:
                rec.account_update_owner_counts[owner] += 1
            if data_len:
                rec.account_update_data_len_counts[data_len] += 1
            rec.update_min_max("account_update_slot_min", "account_update_slot_max", parse_int(fields.get("slot")))
            continue

        if "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED" in line or "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED" in line:
            dropped_marker = "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED" in line
            marker = "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED" if dropped_marker else "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED"
            marker_counts[marker] += 1
            fields = parse_fields(line)
            subscribe_events.append(
                SubscribeEvent(
                    line_no=line_no,
                    from_slot=parse_int(fields.get("from_slot")),
                    bcv2_sent=parse_int(fields.get("bcv2_sent")) or 0,
                    bcv2_dropped=parse_int(fields.get("bcv2_dropped")) or 0,
                    tracked_bcv2=parse_int(fields.get("tracked_bcv2")) or 0,
                    marker=marker,
                )
            )
            continue

        if "ENRICH_RESULT" in line:
            marker_counts["ENRICH_RESULT"] += 1
            fields = parse_fields(line)
            signature = fields.get("sig")
            pubkeys = signature_to_pubkeys.get(signature or "", set())
            bcv2 = unwrap_option(fields.get("bcv2"))
            if bcv2:
                pubkeys = set(pubkeys)
                pubkeys.add(bcv2)
            for pubkey in pubkeys:
                rec = record_for(records, pubkey)
                if signature:
                    rec.enrich_signatures.add(signature)
                buy_variant = unwrap_option(fields.get("buy_variant"))
                if buy_variant:
                    rec.enrich_buy_variants[buy_variant] += 1
                if bcv2:
                    rec.enrich_bcv2_values[bcv2] += 1

    apply_subscribe_inference(records, subscribe_events)
    rows = [record_to_row(rec) for rec in sorted(records.values(), key=lambda item: item.pubkey)]
    bucket_counts = Counter(row["primary_bucket"] for row in rows)
    error_class_counts: Counter[str] = Counter()
    for row in rows:
        error_class_counts.update(row["hydration_error_classes"])

    status = "PASS" if rows and bucket_counts.get("unclassified", 0) == 0 else "NO-GO"
    if not rows:
        status = "NO-GO"

    return {
        "schema": SCHEMA_VERSION,
        "status": status,
        "input_paths": input_paths,
        "total_lines_scanned": total_lines,
        "marker_counts": dict(marker_counts),
        "unique_bcv2_pubkeys": len(rows),
        "primary_bucket_counts": dict(bucket_counts),
        "hydration_error_class_counts": dict(error_class_counts),
        "audit_notes": [
            "subscribe inclusion/drop attribution is audit-only because subscribe markers are aggregate and do not carry pubkeys",
            "RPC hydration is execution-readiness/backfill evidence, not R2 SSOT",
        ],
        "rows": rows,
    }


def apply_subscribe_inference(records: dict[str, Bcv2Record], subscribe_events: list[SubscribeEvent]) -> None:
    if not subscribe_events:
        return
    for rec in records.values():
        if rec.first_registered_line is None:
            continue
        later_events = [event for event in subscribe_events if event.line_no >= rec.first_registered_line]
        if not later_events:
            continue
        rec.included_in_subscribe_inferred = any(event.bcv2_sent > 0 for event in later_events)
        rec.dropped_over_cap_inferred = any(event.bcv2_dropped > 0 for event in later_events)
        if rec.included_in_subscribe_inferred or rec.dropped_over_cap_inferred:
            rec.subscribe_inference_note = "audit_only_aggregate_subscribe_marker_after_registration"


def has_ready_account_update(rec: Bcv2Record) -> bool:
    for owner, owner_count in rec.account_update_owner_counts.items():
        if owner == SYSTEM_PROGRAM:
            continue
        for data_len, data_len_count in rec.account_update_data_len_counts.items():
            if parse_int(data_len) and owner_count > 0 and data_len_count > 0:
                return True
    return False


def has_system_empty_account_update(rec: Bcv2Record) -> bool:
    if rec.account_update_count == 0:
        return False
    owners = set(rec.account_update_owner_counts)
    data_lens = set(rec.account_update_data_len_counts)
    return owners == {SYSTEM_PROGRAM} and data_lens == {"0"}


def classify_record(rec: Bcv2Record) -> tuple[str, list[str]]:
    reasons: list[str] = []
    if rec.registered_count == 0:
        return "unclassified", ["no_exact_watch_registration"]
    if rec.hydration_ready_count > 0:
        return "rpc_ready", ["hydration_ready"]
    if has_ready_account_update(rec):
        return "account_update_ready_not_propagated", ["same_pubkey_account_update_ready_without_rpc_ready"]
    if has_system_empty_account_update(rec):
        return "exact_watch_system_empty", ["same_pubkey_account_update_system_owner_data_len_zero"]
    if rec.dropped_over_cap_inferred and not rec.included_in_subscribe_inferred:
        return "exact_watch_dropped", ["aggregate_subscribe_drop_after_registration"]
    if rec.dropped_over_cap_inferred:
        return "exact_watch_capacity_pressure", ["aggregate_subscribe_drop_after_registration"]
    provider_errors = [
        key
        for key in rec.hydration_error_classes
        if key.startswith("rpc_") or key in {"timeout", "provider_timeout", "queue_full", "worker_closed"}
    ]
    non_missing_provider_errors = [
        key
        for key in provider_errors
        if key
        not in {
            "rpc_missing_initial",
            "rpc_missing_after_retry",
            "missing_on_rpc",
        }
    ]
    if non_missing_provider_errors:
        return "provider_error", sorted(non_missing_provider_errors)
    if rec.hydration_error_classes.get("rpc_missing_after_retry", 0) > 0:
        return "retry_exhausted_missing", ["rpc_missing_after_retry"]
    if rec.hydration_error_classes.get("rpc_missing_initial", 0) > 0:
        return "initial_rpc_missing_only", ["rpc_missing_initial"]
    if rec.hydration_error_classes.get("missing_on_rpc", 0) > 0:
        return "initial_rpc_missing_only", ["legacy_missing_on_rpc_without_retry_metadata"]
    reasons.append("no_ready_no_missing_no_account_update")
    return "unclassified", reasons


def counter_to_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def record_to_row(rec: Bcv2Record) -> dict[str, Any]:
    bucket, reasons = classify_record(rec)
    return {
        "pubkey": rec.pubkey,
        "primary_bucket": bucket,
        "bucket_reasons": reasons,
        "registered_count": rec.registered_count,
        "signatures": sorted(rec.signatures),
        "buy_variants": counter_to_dict(rec.buy_variants),
        "provenance_statuses": counter_to_dict(rec.provenance_statuses),
        "observed_slot_min": rec.observed_slot_min,
        "observed_slot_max": rec.observed_slot_max,
        "registry_version_min": rec.registry_version_min,
        "registry_version_max": rec.registry_version_max,
        "hydration_ready_count": rec.hydration_ready_count,
        "hydration_missing_count": rec.hydration_missing_count,
        "hydration_error_classes": counter_to_dict(rec.hydration_error_classes),
        "hydration_attempts": counter_to_dict(rec.hydration_attempts),
        "hydration_context_slot_min": rec.hydration_context_slot_min,
        "hydration_context_slot_max": rec.hydration_context_slot_max,
        "hydration_latency_ms_max": rec.hydration_latency_ms_max,
        "ready_owners": counter_to_dict(rec.ready_owners),
        "ready_data_lens": counter_to_dict(rec.ready_data_lens),
        "account_update_count": rec.account_update_count,
        "account_update_owner_counts": counter_to_dict(rec.account_update_owner_counts),
        "account_update_data_len_counts": counter_to_dict(rec.account_update_data_len_counts),
        "account_update_slot_min": rec.account_update_slot_min,
        "account_update_slot_max": rec.account_update_slot_max,
        "included_in_subscribe_inferred": rec.included_in_subscribe_inferred,
        "dropped_over_cap_inferred": rec.dropped_over_cap_inferred,
        "subscribe_inference_note": rec.subscribe_inference_note,
        "enrich_buy_variants": counter_to_dict(rec.enrich_buy_variants),
        "enrich_bcv2_values": counter_to_dict(rec.enrich_bcv2_values),
    }


def resolve_paths(args: argparse.Namespace) -> list[Path]:
    paths: list[Path] = []
    for pattern in args.log_glob:
        paths.extend(Path(path) for path in sorted(glob.glob(pattern)))
    paths.extend(Path(path) for path in args.log)
    deduped: list[Path] = []
    seen: set[Path] = set()
    for path in paths:
        resolved = path.resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        deduped.append(path)
    missing = [str(path) for path in deduped if not path.exists()]
    if missing:
        raise FileNotFoundError(f"missing log paths: {missing}")
    if not deduped:
        raise ValueError("no log paths provided")
    return deduped


def write_outputs(payload: dict[str, Any], output_dir: Path) -> tuple[Path, Path]:
    output_dir.mkdir(parents=True, exist_ok=True)
    summary_path = output_dir / "bcv2_hydration_gap_v1.json"
    rows_path = output_dir / "bcv2_hydration_gap_v1.jsonl"
    summary_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    with rows_path.open("w", encoding="utf-8") as handle:
        for row in payload["rows"]:
            handle.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")
    return summary_path, rows_path


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True, help="Scope label used under reports/bcv2_hydration/<scope>.")
    parser.add_argument("--log", action="append", default=[], help="Log file to scan. May be repeated.")
    parser.add_argument("--log-glob", action="append", default=[], help="Glob of log files to scan. May be repeated.")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Output directory. Defaults to reports/bcv2_hydration/<scope>.",
    )
    parser.add_argument("--json", action="store_true", help="Print compact summary JSON to stdout.")
    return parser


def main() -> int:
    args = build_arg_parser().parse_args()
    paths = resolve_paths(args)
    payload = analyze_paths(paths)
    payload["scope"] = args.scope
    output_dir = args.output_dir or Path("reports") / "bcv2_hydration" / args.scope
    summary_path, rows_path = write_outputs(payload, output_dir)
    if args.json:
        print(
            json.dumps(
                {
                    "status": payload["status"],
                    "summary_path": str(summary_path),
                    "rows_path": str(rows_path),
                    "unique_bcv2_pubkeys": payload["unique_bcv2_pubkeys"],
                    "primary_bucket_counts": payload["primary_bucket_counts"],
                    "marker_counts": payload["marker_counts"],
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
