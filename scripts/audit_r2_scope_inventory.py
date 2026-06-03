#!/usr/bin/env python3
"""Audit selector source scopes for event/decision/lifecycle/DIAG overlap.

This script is intentionally offline-only.  It does not build a selector
dataset, does not create labels, and does not treat decision logs as a
denominator source.  Its job is to find windows where the event denominator and
canonical R2 account-state evidence overlap in time.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common
from build_selector_canonical_r2_source import parse_diag_line, parse_iso_to_ms


DEFAULT_SCOPES = [
    "shadow-burnin-v3-p1",
    "shadow-burnin-v3-fsc-capture-nln-pro-2stream",
    "shadow-burnin-v3-fsc-capture-nln-r3-12h-alchemy-2stream",
    "shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke",
]

DIAG_ID_RE = re.compile(
    r"\bDIAG_ACCOUNT_UPDATE_RELAY\b .*?\bbase_mint=(?P<base_mint>\S+) "
    r"bonding_curve=(?P<bonding_curve>\S+)"
)


@dataclass
class TimeStats:
    rows: int = 0
    timestamped_rows: int = 0
    missing_timestamp_rows: int = 0
    min_ts_ms: int | None = None
    max_ts_ms: int | None = None

    def observe(self, ts_ms: int | None) -> None:
        self.rows += 1
        if ts_ms is None:
            self.missing_timestamp_rows += 1
            return
        self.timestamped_rows += 1
        if self.min_ts_ms is None or ts_ms < self.min_ts_ms:
            self.min_ts_ms = ts_ms
        if self.max_ts_ms is None or ts_ms > self.max_ts_ms:
            self.max_ts_ms = ts_ms


@dataclass
class BucketStats:
    event: TimeStats = field(default_factory=TimeStats)
    decision: TimeStats = field(default_factory=TimeStats)
    lifecycle: TimeStats = field(default_factory=TimeStats)
    diag: TimeStats = field(default_factory=TimeStats)
    new_pool_detected_rows: int = 0
    candidate_rows: int = 0
    buy_rows: int = 0
    reject_rows: int = 0
    timeout_rows: int = 0
    accepted_lifecycle_resolved_rows: int = 0
    diag_rows_without_parseable_timestamp: int = 0
    event_identity_keys: set[tuple[str, str]] = field(default_factory=set)
    diag_identity_keys: set[tuple[str, str]] = field(default_factory=set)


def bucket_day(ts_ms: int) -> str:
    return datetime.fromtimestamp(ts_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d")


def bucket_hour(ts_ms: int) -> str:
    return datetime.fromtimestamp(ts_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%dT%H:00:00Z")


def iso_ms(ts_ms: int | None) -> str | None:
    if ts_ms is None:
        return None
    return datetime.fromtimestamp(ts_ms / 1000, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def overlap_ms(left: TimeStats, right: TimeStats) -> int:
    if (
        left.min_ts_ms is None
        or left.max_ts_ms is None
        or right.min_ts_ms is None
        or right.max_ts_ms is None
    ):
        return 0
    return max(0, min(left.max_ts_ms, right.max_ts_ms) - max(left.min_ts_ms, right.min_ts_ms))


def identity_key(row: dict[str, Any]) -> tuple[str, str] | None:
    identity = common.extract_identity(row)
    base_mint = common.str_or_none(identity.get("base_mint"))
    bonding_curve = common.str_or_none(identity.get("bonding_curve"))
    if base_mint and bonding_curve:
        return base_mint, bonding_curve
    return None


def update_bucket(
    buckets: dict[str, BucketStats],
    key: str,
    update: TimeStats,
    ts_ms: int | None,
) -> BucketStats:
    bucket = buckets[key]
    update.observe(ts_ms)
    return bucket


def add_timestamped_bucket(
    *,
    by_day: dict[str, BucketStats],
    by_hour: dict[str, BucketStats],
    ts_ms: int | None,
    section: str,
) -> list[BucketStats]:
    if ts_ms is None:
        return []
    day = by_day[bucket_day(ts_ms)]
    hour = by_hour[bucket_hour(ts_ms)]
    getattr(day, section).observe(ts_ms)
    getattr(hour, section).observe(ts_ms)
    return [day, hour]


def timestamp_from_row(row: dict[str, Any], fields: Iterable[str]) -> int | None:
    value = common.int_or_none(common.find_first_key(row, fields))
    if value is not None:
        return value
    for key in ("timestamp", "created_at", "event_time"):
        raw = common.find_first_key(row, (key,))
        parsed = parse_iso_to_ms(raw) if isinstance(raw, str) else None
        if parsed is not None:
            return parsed
    return None


def decision_verdict_class(row: dict[str, Any]) -> str:
    decision_buy = common.bool_or_none(common.find_first_key(row, ("decision_verdict_buy",)))
    if decision_buy is True:
        return "BUY"
    verdict = common.find_first_key(
        row,
        (
            "verdict_type",
            "gatekeeper_verdict",
            "terminal_verdict",
            "verdict",
            "decision",
            "decision_status",
        ),
    )
    text = str(verdict or "").upper()
    if "TIMEOUT" in text:
        return "TIMEOUT"
    if "BUY" in text or "EARLY_BUY" in text:
        return "BUY"
    if "REJECT" in text or "HARD_FAIL" in text:
        return "REJECT"
    return "UNKNOWN"


def event_class(row: dict[str, Any]) -> str:
    normalized = common.normalized_event_type(row)
    if normalized in {"newpooldetected", "new_pool_detected"}:
        return "NEW_POOL"
    if "candidate" in normalized:
        return "CANDIDATE"
    return "OTHER"


def add_identity_to_buckets(
    buckets: list[BucketStats],
    *,
    event_key: tuple[str, str] | None = None,
    diag_key: tuple[str, str] | None = None,
) -> None:
    for bucket in buckets:
        if event_key is not None:
            bucket.event_identity_keys.add(event_key)
        if diag_key is not None:
            bucket.diag_identity_keys.add(diag_key)


def summarize_bucket(bucket: BucketStats) -> dict[str, Any]:
    event_diag_overlap = overlap_ms(bucket.event, bucket.diag)
    identity_overlap = len(bucket.event_identity_keys & bucket.diag_identity_keys)
    return {
        "event_rows": bucket.event.rows,
        "event_timestamped_rows": bucket.event.timestamped_rows,
        "event_missing_timestamp_rows": bucket.event.missing_timestamp_rows,
        "event_min_ts_ms": bucket.event.min_ts_ms,
        "event_max_ts_ms": bucket.event.max_ts_ms,
        "event_min_ts": iso_ms(bucket.event.min_ts_ms),
        "event_max_ts": iso_ms(bucket.event.max_ts_ms),
        "NewPoolDetected_rows": bucket.new_pool_detected_rows,
        "Candidate_rows": bucket.candidate_rows,
        "decision_rows": bucket.decision.rows,
        "decision_timestamped_rows": bucket.decision.timestamped_rows,
        "BUY_rows": bucket.buy_rows,
        "REJECT_rows": bucket.reject_rows,
        "TIMEOUT_rows": bucket.timeout_rows,
        "accepted_lifecycle_rows": bucket.lifecycle.rows,
        "accepted_lifecycle_resolved_rows": bucket.accepted_lifecycle_resolved_rows,
        "DIAG_ACCOUNT_UPDATE_RELAY_rows": bucket.diag.rows,
        "DIAG_timestamped_rows": bucket.diag.timestamped_rows,
        "DIAG_rows_without_parseable_timestamp": bucket.diag_rows_without_parseable_timestamp,
        "DIAG_min_ts_ms": bucket.diag.min_ts_ms,
        "DIAG_max_ts_ms": bucket.diag.max_ts_ms,
        "DIAG_min_ts": iso_ms(bucket.diag.min_ts_ms),
        "DIAG_max_ts": iso_ms(bucket.diag.max_ts_ms),
        "temporal_overlap_ms": event_diag_overlap,
        "candidate_identity_overlap_estimate": identity_overlap,
        "event_identity_key_count": len(bucket.event_identity_keys),
        "diag_identity_key_count": len(bucket.diag_identity_keys),
        "candidate_window_acceptance": (
            "PASS"
            if bucket.new_pool_detected_rows > 0
            and bucket.candidate_rows > 0
            and bucket.diag.timestamped_rows > 0
            and event_diag_overlap > 0
            else "NO-GO"
        ),
    }


def scan_events(scope: str, root: Path, total: BucketStats, by_day: dict[str, BucketStats], by_hour: dict[str, BucketStats]) -> dict[str, Any]:
    event_dir = root / "datasets" / "events" / scope
    paths = sorted(event_dir.glob("*.jsonl")) if event_dir.exists() else []
    type_counts: Counter[str] = Counter()
    for path in paths:
        for row in common.iter_json_objects(path):
            ts_ms = common.source_ts_ms(row)
            total.event.observe(ts_ms)
            classification = event_class(row)
            type_counts[common.normalized_event_type(row)] += 1
            if classification == "NEW_POOL":
                total.new_pool_detected_rows += 1
            elif classification == "CANDIDATE":
                total.candidate_rows += 1
            key = identity_key(row)
            if key is not None:
                total.event_identity_keys.add(key)
            buckets = add_timestamped_bucket(by_day=by_day, by_hour=by_hour, ts_ms=ts_ms, section="event")
            for bucket in buckets:
                if classification == "NEW_POOL":
                    bucket.new_pool_detected_rows += 1
                elif classification == "CANDIDATE":
                    bucket.candidate_rows += 1
            add_identity_to_buckets(buckets, event_key=key)
    return {
        "event_dir": str(event_dir),
        "event_paths_count": len(paths),
        "event_type_counts": common.counter_dict(type_counts),
    }


def scan_decisions(scope: str, root: Path, total: BucketStats, by_day: dict[str, BucketStats], by_hour: dict[str, BucketStats]) -> dict[str, Any]:
    decision_dir = root / "logs" / "rollout" / scope / "decisions"
    paths = sorted(decision_dir.rglob("gatekeeper_v2_decisions.jsonl")) if decision_dir.exists() else []
    verdict_counts: Counter[str] = Counter()
    for path in paths:
        for row in common.iter_json_objects(path):
            ts_ms = timestamp_from_row(
                row,
                (
                    "decision_ts_ms",
                    "observation_end_ts_ms",
                    "decision_timestamp_ms",
                    "timestamp_ms",
                    "ts_ms",
                ),
            )
            verdict = decision_verdict_class(row)
            verdict_counts[verdict] += 1
            total.decision.observe(ts_ms)
            if verdict == "BUY":
                total.buy_rows += 1
            elif verdict == "REJECT":
                total.reject_rows += 1
            elif verdict == "TIMEOUT":
                total.timeout_rows += 1
            buckets = add_timestamped_bucket(by_day=by_day, by_hour=by_hour, ts_ms=ts_ms, section="decision")
            for bucket in buckets:
                if verdict == "BUY":
                    bucket.buy_rows += 1
                elif verdict == "REJECT":
                    bucket.reject_rows += 1
                elif verdict == "TIMEOUT":
                    bucket.timeout_rows += 1
    return {
        "decision_dir": str(decision_dir),
        "decision_paths_count": len(paths),
        "decision_verdict_counts": common.counter_dict(verdict_counts),
    }


def scan_lifecycle(scope: str, root: Path, total: BucketStats, by_day: dict[str, BucketStats], by_hour: dict[str, BucketStats]) -> dict[str, Any]:
    path = root / "logs" / "shadow_run" / scope / "shadow_onchain_lifecycle_report_all.jsonl"
    status_counts: Counter[str] = Counter()
    truth_counts: Counter[str] = Counter()
    if not path.exists():
        return {
            "lifecycle_report": str(path),
            "lifecycle_report_exists": False,
            "analysis_status_counts": {},
            "truth_status_counts": {},
        }
    for row in common.iter_json_objects(path):
        ts_ms = timestamp_from_row(
            row,
            (
                "decision_ts_ms",
                "entry_execution_ts_ms",
                "close_ts_ms",
                "timestamp_ms",
                "ts_ms",
            ),
        )
        status = str(common.find_first_key(row, ("analysis_status",)) or "unknown")
        truth = str(common.find_first_key(row, ("truth_status",)) or "unknown")
        status_counts[status] += 1
        truth_counts[truth] += 1
        total.lifecycle.observe(ts_ms)
        if truth == "resolved":
            total.accepted_lifecycle_resolved_rows += 1
        buckets = add_timestamped_bucket(by_day=by_day, by_hour=by_hour, ts_ms=ts_ms, section="lifecycle")
        for bucket in buckets:
            if truth == "resolved":
                bucket.accepted_lifecycle_resolved_rows += 1
    return {
        "lifecycle_report": str(path),
        "lifecycle_report_exists": True,
        "analysis_status_counts": common.counter_dict(status_counts),
        "truth_status_counts": common.counter_dict(truth_counts),
    }


def diag_identity_from_line(line: str) -> tuple[str, str] | None:
    match = DIAG_ID_RE.search(line)
    if not match:
        return None
    return match.group("base_mint"), match.group("bonding_curve")


def scan_diag(scope: str, root: Path, total: BucketStats, by_day: dict[str, BucketStats], by_hour: dict[str, BucketStats]) -> dict[str, Any]:
    rollout_dir = root / "logs" / "rollout" / scope
    paths = []
    if rollout_dir.exists():
        paths = sorted(
            path
            for path in rollout_dir.iterdir()
            if path.is_file() and (path.name.startswith("system.log") or path.name.startswith("oracle.log"))
        )
    line_rows_read = 0
    diag_rows = 0
    timestamp_parse_failures = 0
    for path in paths:
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line_number, line in enumerate(fh, start=1):
                line_rows_read += 1
                if "DIAG_ACCOUNT_UPDATE_RELAY" not in line:
                    continue
                diag_rows += 1
                key = diag_identity_from_line(line)
                update = parse_diag_line(line, source_path=str(path), source_line=line_number)
                ts_ms = update.timestamp_ms if update is not None else None
                total.diag.observe(ts_ms)
                if key is not None:
                    total.diag_identity_keys.add(key)
                if ts_ms is None:
                    total.diag_rows_without_parseable_timestamp += 1
                    timestamp_parse_failures += 1
                    continue
                buckets = add_timestamped_bucket(by_day=by_day, by_hour=by_hour, ts_ms=ts_ms, section="diag")
                add_identity_to_buckets(buckets, diag_key=key)
    return {
        "diag_log_dir": str(rollout_dir),
        "diag_log_paths": [str(path) for path in paths],
        "diag_log_paths_count": len(paths),
        "diag_log_line_rows_read": line_rows_read,
        "diag_rows": diag_rows,
        "diag_timestamp_parse_failures": timestamp_parse_failures,
        "diag_timestamp_policy": "only DIAG rows with parseable line timestamps can prove temporal overlap",
    }


def summarize_scope(scope: str, root: Path) -> dict[str, Any]:
    total = BucketStats()
    by_day: dict[str, BucketStats] = defaultdict(BucketStats)
    by_hour: dict[str, BucketStats] = defaultdict(BucketStats)
    inputs = {}
    inputs.update(scan_events(scope, root, total, by_day, by_hour))
    inputs.update(scan_decisions(scope, root, total, by_day, by_hour))
    inputs.update(scan_lifecycle(scope, root, total, by_day, by_hour))
    inputs.update(scan_diag(scope, root, total, by_day, by_hour))
    summary = summarize_bucket(total)
    return {
        "scope": scope,
        "inputs": inputs,
        "summary": summary,
        "by_day": {key: summarize_bucket(value) for key, value in sorted(by_day.items())},
        "by_hour": {key: summarize_bucket(value) for key, value in sorted(by_hour.items())},
    }


def candidate_windows(scopes: list[dict[str, Any]]) -> list[dict[str, Any]]:
    windows = []
    for scope_report in scopes:
        scope = scope_report["scope"]
        for hour, payload in scope_report["by_hour"].items():
            if payload["candidate_window_acceptance"] != "PASS":
                continue
            windows.append(
                {
                    "scope": scope,
                    "window_bucket": hour,
                    "window_start_ts": hour,
                    "window_kind": "hour",
                    "NewPoolDetected_rows": payload["NewPoolDetected_rows"],
                    "Candidate_rows": payload["Candidate_rows"],
                    "decision_rows": payload["decision_rows"],
                    "BUY_rows": payload["BUY_rows"],
                    "accepted_lifecycle_rows": payload["accepted_lifecycle_rows"],
                    "accepted_lifecycle_resolved_rows": payload["accepted_lifecycle_resolved_rows"],
                    "DIAG_ACCOUNT_UPDATE_RELAY_rows": payload["DIAG_ACCOUNT_UPDATE_RELAY_rows"],
                    "DIAG_timestamped_rows": payload["DIAG_timestamped_rows"],
                    "temporal_overlap_ms": payload["temporal_overlap_ms"],
                    "candidate_identity_overlap_estimate": payload[
                        "candidate_identity_overlap_estimate"
                    ],
                    "ranking_reason": (
                        "event_candidate_diag_overlap_with_lifecycle"
                        if payload["accepted_lifecycle_resolved_rows"] > 0
                        else "event_candidate_diag_overlap_without_lifecycle"
                    ),
                }
            )
    return sorted(
        windows,
        key=lambda item: (
            item["accepted_lifecycle_resolved_rows"],
            item["candidate_identity_overlap_estimate"],
            item["temporal_overlap_ms"],
            item["Candidate_rows"],
            item["NewPoolDetected_rows"],
        ),
        reverse=True,
    )


def build_report(root: Path, scopes: list[str]) -> dict[str, Any]:
    scope_reports = [summarize_scope(scope, root) for scope in scopes]
    windows = candidate_windows(scope_reports)
    return {
        "artifact": "r2_scope_inventory_v1",
        "generated_at": datetime.now(tz=timezone.utc).isoformat().replace("+00:00", "Z"),
        "root": str(root),
        "scopes_requested": scopes,
        "scope_count": len(scope_reports),
        "selection_contract": {
            "required_for_full_phase1_phase2": [
                "NewPoolDetected_rows > 0",
                "Candidate_rows > 0",
                "DIAG_timestamped_rows > 0",
                "temporal_overlap_ms > 0",
            ],
            "ranking_preference": [
                "accepted_lifecycle_resolved_rows",
                "candidate_identity_overlap_estimate",
                "temporal_overlap_ms",
                "Candidate_rows",
                "NewPoolDetected_rows",
            ],
            "r2_ssot": "DIAG_ACCOUNT_UPDATE_RELAY / canonical account-state evidence only",
            "non_denominator_sources": [
                "decision logs",
                "accepted lifecycle rows",
                "NLN Program Streams",
                "RPC hydration",
                "ShadowLedger bin",
            ],
        },
        "candidate_windows": windows,
        "candidate_window_count": len(windows),
        "scopes": scope_reports,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--scope", action="append", default=[])
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("reports/selector/r2_scope_inventory_v1.json"),
    )
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    scopes = args.scope or DEFAULT_SCOPES
    report = build_report(args.root, scopes)
    common.write_json(args.output, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
