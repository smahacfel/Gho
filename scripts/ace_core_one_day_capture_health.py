#!/usr/bin/env python3
"""Materialize the minimal, offline ACE capture-health evidence.

This tool has no runtime connection to Ghost.  During a capture, use
``snapshot`` twice against the launcher loopback endpoint: once after the
health metrics are visible and once immediately before controlled shutdown.
After shutdown, ``finalize`` verifies the two immutable snapshots, JSONL flush
discipline, and selected launcher logs, then writes the single health receipt
reserved by the immutable capture manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import time
import urllib.request
from pathlib import Path
from typing import Iterable


HEALTH_SCHEMA_VERSION = 2
METRICS_SNAPSHOT_SCHEMA_VERSION = 1
REQUIRED_COUNTERS = (
    "pr1_runtime_bypass_attempt_total",
    "pr1_runtime_candidate_admission_closed_total",
    "pr1_runtime_primary_coverage_gap_total",
)
CAPTURE_KINDS = ("smoke", "day1")
SMOKE_MIN_DURATION_MS = 120_000
SMOKE_MAX_DURATION_MS = 300_000
DAY1_MIN_DURATION_MS = 86_400_000
EVENT_WRITER_WRITE_FAILURE_MARKER = "EventEmitter: failed to write event"
EVENT_WRITER_LOCK_FAILURE_MARKER = "EventEmitter: writer mutex poisoned; event was not persisted"
FORBIDDEN_LOG_MARKERS = (
    "Seer: unrecovered primary local coverage gap closes new candidate admission",
    "RUG_REALITY_CAPTURE_RUNTIME_FEE_AUTHORITY_UNAVAILABLE",
    "RUG_REALITY_CAPTURE_TYPED_QUOTE_AUTHORITY_UNAVAILABLE",
    "RUG_SCALP_RUNTIME_FEE_AUTHORITY_INVALIDATED",
    "RUG_SCALP_RUNTIME_FEE_AUTHORITY_CHANGE_REQUESTED_CONTROLLED_SHUTDOWN",
    "Oracle Runtime failed before shutdown signal",
    "Oracle Runtime task failed before shutdown signal",
    "Oracle Runtime component shutdown failed",
)
FORBIDDEN_LOG_PATTERNS = (
    re.compile(r"Component shutdown completed with \d+ failure\(s\) or forced abort\(s\)"),
)
CONTROLLED_SHUTDOWN_MARKER = "Ghost Launcher shutdown complete"


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write_new(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as handle:
        handle.write(data)
        handle.write(b"\n")
        handle.flush()


def load_manifest(path: Path) -> tuple[bytes, dict[str, object]]:
    manifest_bytes = path.read_bytes()
    try:
        manifest = json.loads(manifest_bytes)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"capture manifest is not valid JSON: {error.msg}") from error
    if not isinstance(manifest, dict):
        raise RuntimeError("capture manifest must be a JSON object")
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not run_id:
        raise RuntimeError("capture manifest has no run_id")
    return manifest_bytes, manifest


def snapshot(args: argparse.Namespace) -> int:
    manifest_bytes, manifest = load_manifest(Path(args.manifest))
    request = urllib.request.Request(args.metrics_url, method="GET")
    with urllib.request.urlopen(request, timeout=args.timeout_s) as response:
        if response.status != 200:
            raise RuntimeError(f"metrics endpoint returned HTTP {response.status}")
        body = response.read()
    values = parse_prometheus_counters(body.decode("utf-8"), REQUIRED_COUNTERS)
    missing = [name for name in REQUIRED_COUNTERS if name not in values]
    if missing:
        raise RuntimeError(
            "metrics endpoint does not expose required ACE health series: " + ", ".join(missing)
        )
    snapshot_record = {
        "schema_version": METRICS_SNAPSHOT_SCHEMA_VERSION,
        "run_id": manifest["run_id"],
        "manifest_sha256": sha256_hex(manifest_bytes),
        "phase": args.phase,
        "capture_kind": args.capture_kind,
        "captured_at_unix_ms": time.time_ns() // 1_000_000,
        "counters": values,
        "raw_metrics_sha256": sha256_hex(body),
    }
    write_new(
        Path(args.output),
        json.dumps(snapshot_record, sort_keys=True, indent=2).encode("utf-8"),
    )
    return 0


def parse_prometheus_counters(payload: str, names: Iterable[str]) -> dict[str, int]:
    values: dict[str, int] = {}
    for name in names:
        pattern = re.compile(rf"^{re.escape(name)}\s+([0-9]+(?:\.0+)?)\s*$")
        for line in payload.splitlines():
            match = pattern.match(line)
            if match:
                values[name] = int(float(match.group(1)))
                break
    return values


def event_files(root: Path) -> list[Path]:
    return sorted(path for path in root.rglob("exec_*.jsonl") if path.is_file())


def has_ace_smoke_trade_shape(payload: object) -> bool:
    if not isinstance(payload, dict):
        return False
    if payload.get("success") is not True or payload.get("is_synthetic") is not False:
        return False
    required_order = (
        "slot",
        "tx_index",
        "outer_instruction_index",
        "inner_group_index",
        "event_ordinal",
    )
    required_reserves = (
        "virtual_sol_reserves",
        "virtual_token_reserves",
        "real_sol_reserves",
        "real_token_reserves",
    )
    required_balances = ("signer_pre_balance_lamports", "signer_post_balance_lamports")
    return all(payload.get(name) is not None for name in required_order + required_reserves + required_balances)


def validate_event_files(root: Path) -> tuple[bool, list[str]]:
    failures: list[str] = []
    files = event_files(root)
    if not files:
        return False, ["no exec_*.jsonl files found"]
    birth_count = 0
    pool_transaction_count = 0
    probe_ready_pool_transaction_count = 0
    for path in files:
        raw = path.read_bytes()
        if not raw:
            continue
        if not raw.endswith(b"\n"):
            failures.append(f"{path}: final newline missing")
            continue
        for line_number, line in enumerate(raw.splitlines(), start=1):
            if not line.strip():
                failures.append(f"{path}:{line_number}: blank JSONL row")
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError as error:
                failures.append(f"{path}:{line_number}: invalid JSONL: {error.msg}")
                continue
            kind = event.get("kind") if isinstance(event, dict) else None
            if not isinstance(kind, dict):
                continue
            if kind.get("type") == "NewPoolDetected":
                birth_count += 1
            elif kind.get("type") == "PoolTransaction":
                pool_transaction_count += 1
                if has_ace_smoke_trade_shape(kind.get("payload")):
                    probe_ready_pool_transaction_count += 1
    if birth_count == 0:
        failures.append("no NewPoolDetected birth evidence found in exec_*.jsonl")
    if pool_transaction_count == 0:
        failures.append("no PoolTransaction evidence found in exec_*.jsonl")
    if probe_ready_pool_transaction_count == 0:
        failures.append(
            "no successful non-synthetic PoolTransaction with balances, full order key, and full reserves"
        )
    return not failures, failures


def iter_logs(paths: Iterable[Path]) -> Iterable[Path]:
    for path in paths:
        if path.is_file():
            yield path
        elif path.is_dir():
            yield from sorted(candidate for candidate in path.rglob("*.log") if candidate.is_file())


def validate_logs(paths: Iterable[Path]) -> tuple[bool, bool, int, int, list[str]]:
    failures: list[str] = []
    controlled_shutdown = False
    write_failures = 0
    lock_failures = 0
    inspected = 0
    for path in iter_logs(paths):
        inspected += 1
        text = path.read_text(encoding="utf-8", errors="replace")
        controlled_shutdown = controlled_shutdown or CONTROLLED_SHUTDOWN_MARKER in text
        write_failures += text.count(EVENT_WRITER_WRITE_FAILURE_MARKER)
        lock_failures += text.count(EVENT_WRITER_LOCK_FAILURE_MARKER)
        for marker in FORBIDDEN_LOG_MARKERS:
            if marker in text:
                failures.append(f"{path}: forbidden capture-health marker: {marker}")
        for pattern in FORBIDDEN_LOG_PATTERNS:
            if pattern.search(text):
                failures.append(f"{path}: forbidden capture-health marker: {pattern.pattern}")
    if inspected == 0:
        failures.append("no launcher log files supplied")
    if write_failures:
        failures.append("EventEmitter write failures found in launcher logs")
    if lock_failures:
        failures.append("EventEmitter lock failures found in launcher logs")
    if not controlled_shutdown:
        failures.append("controlled launcher shutdown marker missing")
    return not failures, controlled_shutdown, write_failures, lock_failures, failures


def load_snapshot(
    path: Path,
    *,
    expected_run_id: str,
    expected_manifest_sha256: str,
    expected_phase: str,
    expected_capture_kind: str,
) -> tuple[dict[str, object] | None, list[str]]:
    failures: list[str] = []
    try:
        snapshot_bytes = path.read_bytes()
    except OSError as error:
        return None, [f"{expected_phase} metrics snapshot unreadable: {error}"]
    try:
        snapshot = json.loads(snapshot_bytes)
    except json.JSONDecodeError as error:
        return None, [f"{expected_phase} metrics snapshot is invalid JSON: {error.msg}"]
    if not isinstance(snapshot, dict):
        return None, [f"{expected_phase} metrics snapshot must be a JSON object"]
    if snapshot.get("schema_version") != METRICS_SNAPSHOT_SCHEMA_VERSION:
        failures.append(f"{expected_phase} metrics snapshot schema mismatch")
    if snapshot.get("run_id") != expected_run_id:
        failures.append(f"{expected_phase} metrics snapshot run_id mismatch")
    if snapshot.get("manifest_sha256") != expected_manifest_sha256:
        failures.append(f"{expected_phase} metrics snapshot manifest hash mismatch")
    if snapshot.get("phase") != expected_phase:
        failures.append(f"{expected_phase} metrics snapshot phase mismatch")
    if snapshot.get("capture_kind") != expected_capture_kind:
        failures.append(f"{expected_phase} metrics snapshot capture kind mismatch")
    if type(snapshot.get("captured_at_unix_ms")) is not int or snapshot["captured_at_unix_ms"] <= 0:
        failures.append(f"{expected_phase} metrics snapshot timestamp missing or invalid")
    if not isinstance(snapshot.get("raw_metrics_sha256"), str) or not snapshot["raw_metrics_sha256"]:
        failures.append(f"{expected_phase} metrics snapshot raw metrics hash missing")
    counters = snapshot.get("counters")
    if not isinstance(counters, dict):
        failures.append(f"{expected_phase} metrics snapshot counters missing")
    else:
        for name in REQUIRED_COUNTERS:
            value = counters.get(name)
            if type(value) is not int:
                failures.append(f"{expected_phase} metrics snapshot missing {name}")
            elif value != 0:
                failures.append(f"{expected_phase} metrics {name} is non-zero: {value}")
    if failures:
        return None, failures
    snapshot["snapshot_sha256"] = sha256_hex(snapshot_bytes)
    return snapshot, []


def validate_capture_duration(capture_kind: str, start_ms: int, end_ms: int) -> tuple[int | None, list[str]]:
    failures: list[str] = []
    if end_ms <= start_ms:
        return None, ["metrics snapshots are not strictly ordered in time"]
    duration_ms = end_ms - start_ms
    if capture_kind == "smoke":
        if not SMOKE_MIN_DURATION_MS <= duration_ms <= SMOKE_MAX_DURATION_MS:
            failures.append(
                f"smoke duration {duration_ms}ms is outside [{SMOKE_MIN_DURATION_MS}, {SMOKE_MAX_DURATION_MS}]"
            )
    elif capture_kind == "day1":
        if duration_ms < DAY1_MIN_DURATION_MS:
            failures.append(f"day1 duration {duration_ms}ms is below {DAY1_MIN_DURATION_MS}")
    else:
        failures.append(f"unsupported capture kind: {capture_kind}")
    return duration_ms, failures


def finalize(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    manifest_bytes, manifest = load_manifest(manifest_path)
    expected_run_id = manifest["run_id"]
    expected_output = Path(manifest.get("health_evidence_path", ""))
    requested_output = Path(args.output)
    if expected_output != requested_output:
        raise RuntimeError(
            "--output must exactly equal manifest.health_evidence_path "
            f"({expected_output}), got {requested_output}"
        )

    failures: list[str] = []
    manifest_sha256 = sha256_hex(manifest_bytes)
    start, start_failures = load_snapshot(
        Path(args.start_metrics),
        expected_run_id=expected_run_id,
        expected_manifest_sha256=manifest_sha256,
        expected_phase="start",
        expected_capture_kind=args.capture_kind,
    )
    end, end_failures = load_snapshot(
        Path(args.end_metrics),
        expected_run_id=expected_run_id,
        expected_manifest_sha256=manifest_sha256,
        expected_phase="end",
        expected_capture_kind=args.capture_kind,
    )
    failures.extend(start_failures)
    failures.extend(end_failures)
    duration_ms: int | None = None
    if start is not None and end is not None:
        duration_ms, duration_failures = validate_capture_duration(
            args.capture_kind,
            start["captured_at_unix_ms"],
            end["captured_at_unix_ms"],
        )
        failures.extend(duration_failures)

    events_ok, event_failures = validate_event_files(Path(args.events_dir))
    failures.extend(event_failures)
    logs_ok, controlled_shutdown, writer_failures, lock_failures, log_failures = validate_logs(
        [Path(path) for path in args.log]
    )
    failures.extend(log_failures)

    if failures:
        for failure in failures:
            print(f"[invalid] {failure}", file=sys.stderr)
        # A failed finalization must not leave a superficially valid receipt at
        # the manifest-reserved path.  The probe therefore sees a missing
        # receipt and marks the capture INVALID_CAPTURE.
        return 2

    assert start is not None and end is not None and duration_ms is not None
    receipt = {
        "schema_version": HEALTH_SCHEMA_VERSION,
        "run_id": expected_run_id,
        "manifest_sha256": manifest_sha256,
        "capture_kind": args.capture_kind,
        "start_snapshot_sha256": start["snapshot_sha256"],
        "end_snapshot_sha256": end["snapshot_sha256"],
        "start_metrics_sha256": start["raw_metrics_sha256"],
        "end_metrics_sha256": end["raw_metrics_sha256"],
        "start_captured_at_unix_ms": start["captured_at_unix_ms"],
        "end_captured_at_unix_ms": end["captured_at_unix_ms"],
        "duration_ms": duration_ms,
        "pr1_runtime_bypass_attempt_total": end["counters"]["pr1_runtime_bypass_attempt_total"],
        "pr1_runtime_candidate_admission_closed_total": end["counters"][
            "pr1_runtime_candidate_admission_closed_total"
        ],
        "pr1_runtime_primary_coverage_gap_total": end["counters"][
            "pr1_runtime_primary_coverage_gap_total"
        ],
        "event_writer_write_failure_count": writer_failures,
        "event_writer_lock_failure_count": lock_failures,
        "controlled_shutdown": controlled_shutdown,
        "event_files_cleanly_flushed": events_ok,
        "log_evidence_clean": logs_ok,
    }
    write_new(requested_output, json.dumps(receipt, sort_keys=True, indent=2).encode("utf-8"))
    return 0


def verify_probe(args: argparse.Namespace) -> int:
    summary_path = Path(args.summary)
    try:
        summary = json.loads(summary_path.read_bytes())
    except json.JSONDecodeError as error:
        raise RuntimeError(f"probe summary is not valid JSON: {error.msg}") from error
    if not isinstance(summary, dict):
        raise RuntimeError("probe summary must be a JSON object")
    if summary.get("capture_status") != "VALID_CAPTURE":
        raise RuntimeError("probe summary capture_status is not VALID_CAPTURE")
    invalid_reasons = summary.get("capture_invalid_reasons")
    if not isinstance(invalid_reasons, list) or invalid_reasons:
        raise RuntimeError("probe summary contains capture_invalid_reasons")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    snapshot_command = commands.add_parser(
        "snapshot", help="save a manifest-bound loopback metrics snapshot"
    )
    snapshot_command.add_argument("--manifest", required=True)
    snapshot_command.add_argument("--phase", required=True, choices=("start", "end"))
    snapshot_command.add_argument("--capture-kind", required=True, choices=CAPTURE_KINDS)
    snapshot_command.add_argument("--metrics-url", required=True)
    snapshot_command.add_argument("--output", required=True)
    snapshot_command.add_argument("--timeout-s", type=float, default=5.0)
    snapshot_command.set_defaults(handler=snapshot)

    finalize_command = commands.add_parser(
        "finalize", help="validate capture health and write the manifest-bound receipt"
    )
    finalize_command.add_argument("--manifest", required=True)
    finalize_command.add_argument("--capture-kind", required=True, choices=CAPTURE_KINDS)
    finalize_command.add_argument("--events-dir", required=True)
    finalize_command.add_argument("--start-metrics", required=True)
    finalize_command.add_argument("--end-metrics", required=True)
    finalize_command.add_argument("--log", action="append", required=True)
    finalize_command.add_argument("--output", required=True)
    finalize_command.set_defaults(handler=finalize)

    verify_probe_command = commands.add_parser(
        "verify-probe", help="require a valid-capture result from the actual offline ACE probe"
    )
    verify_probe_command.add_argument("--summary", required=True)
    verify_probe_command.set_defaults(handler=verify_probe)
    return parser.parse_args()


def main() -> int:
    try:
        args = parse_args()
        return int(args.handler(args))
    except Exception as error:  # fail closed with a concise operator error
        print(f"[error] ACE capture health check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
