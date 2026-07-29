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
import urllib.request
from pathlib import Path
from typing import Iterable


HEALTH_SCHEMA_VERSION = 1
REQUIRED_COUNTERS = (
    "pr1_runtime_bypass_attempt_total",
    "pr1_runtime_candidate_admission_closed_total",
    "pr1_runtime_primary_coverage_gap_total",
)
FORBIDDEN_LOG_MARKERS = (
    "EventEmitter: failed to write event",
    "EventEmitter: writer mutex poisoned; event was not persisted",
    "Seer: unrecovered primary local coverage gap closes new candidate admission",
    "RUG_REALITY_CAPTURE_RUNTIME_FEE_AUTHORITY_UNAVAILABLE",
    "RUG_REALITY_CAPTURE_TYPED_QUOTE_AUTHORITY_UNAVAILABLE",
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


def snapshot(args: argparse.Namespace) -> int:
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
    write_new(Path(args.output), body.rstrip(b"\n"))
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


def validate_event_files(root: Path) -> tuple[bool, list[str]]:
    failures: list[str] = []
    files = event_files(root)
    if not files:
        return False, ["no exec_*.jsonl files found"]
    for path in files:
        raw = path.read_bytes()
        if not raw.endswith(b"\n"):
            failures.append(f"{path}: final newline missing")
            continue
        for line_number, line in enumerate(raw.splitlines(), start=1):
            if not line.strip():
                failures.append(f"{path}:{line_number}: blank JSONL row")
                continue
            try:
                json.loads(line)
            except json.JSONDecodeError as error:
                failures.append(f"{path}:{line_number}: invalid JSONL: {error.msg}")
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
        write_failures += text.count(FORBIDDEN_LOG_MARKERS[0])
        lock_failures += text.count(FORBIDDEN_LOG_MARKERS[1])
        for marker in FORBIDDEN_LOG_MARKERS[2:]:
            if marker in text:
                failures.append(f"{path}: forbidden capture-health marker: {marker}")
    if inspected == 0:
        failures.append("no launcher log files supplied")
    if write_failures:
        failures.append("EventEmitter write failures found in launcher logs")
    if lock_failures:
        failures.append("EventEmitter lock failures found in launcher logs")
    if not controlled_shutdown:
        failures.append("controlled launcher shutdown marker missing")
    return not failures, controlled_shutdown, write_failures, lock_failures, failures


def finalize(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    expected_run_id = manifest.get("run_id")
    expected_output = Path(manifest.get("health_evidence_path", ""))
    requested_output = Path(args.output)
    if not expected_run_id:
        raise RuntimeError("capture manifest has no run_id")
    if expected_output != requested_output:
        raise RuntimeError(
            "--output must exactly equal manifest.health_evidence_path "
            f"({expected_output}), got {requested_output}"
        )

    start_bytes = Path(args.start_metrics).read_bytes()
    end_bytes = Path(args.end_metrics).read_bytes()
    start = parse_prometheus_counters(start_bytes.decode("utf-8"), REQUIRED_COUNTERS)
    end = parse_prometheus_counters(end_bytes.decode("utf-8"), REQUIRED_COUNTERS)
    failures: list[str] = []
    for name in REQUIRED_COUNTERS:
        if name not in start:
            failures.append(f"start metrics snapshot missing {name}")
        elif start[name] != 0:
            failures.append(f"start metrics {name} is non-zero: {start[name]}")
        if name not in end:
            failures.append(f"end metrics snapshot missing {name}")
        elif end[name] != 0:
            failures.append(f"end metrics {name} is non-zero: {end[name]}")

    events_ok, event_failures = validate_event_files(Path(args.events_dir))
    failures.extend(event_failures)
    logs_ok, controlled_shutdown, writer_failures, lock_failures, log_failures = validate_logs(
        [Path(path) for path in args.log]
    )
    failures.extend(log_failures)

    receipt = {
        "schema_version": HEALTH_SCHEMA_VERSION,
        "run_id": expected_run_id,
        "manifest_sha256": sha256_hex(manifest_bytes),
        "start_metrics_sha256": sha256_hex(start_bytes),
        "end_metrics_sha256": sha256_hex(end_bytes),
        "pr1_runtime_bypass_attempt_total": end.get("pr1_runtime_bypass_attempt_total", 0),
        "pr1_runtime_candidate_admission_closed_total": end.get(
            "pr1_runtime_candidate_admission_closed_total", 0
        ),
        "pr1_runtime_primary_coverage_gap_total": end.get(
            "pr1_runtime_primary_coverage_gap_total", 0
        ),
        "event_writer_write_failure_count": writer_failures,
        "event_writer_lock_failure_count": lock_failures,
        "controlled_shutdown": controlled_shutdown,
        "event_files_cleanly_flushed": events_ok,
        "log_evidence_clean": logs_ok,
    }
    write_new(requested_output, json.dumps(receipt, sort_keys=True, indent=2).encode("utf-8"))
    if failures:
        for failure in failures:
            print(f"[invalid] {failure}", file=sys.stderr)
        return 2
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    snapshot_command = commands.add_parser("snapshot", help="save a verified loopback metrics scrape")
    snapshot_command.add_argument("--metrics-url", required=True)
    snapshot_command.add_argument("--output", required=True)
    snapshot_command.add_argument("--timeout-s", type=float, default=5.0)
    snapshot_command.set_defaults(handler=snapshot)

    finalize_command = commands.add_parser(
        "finalize", help="validate capture health and write the manifest-bound receipt"
    )
    finalize_command.add_argument("--manifest", required=True)
    finalize_command.add_argument("--events-dir", required=True)
    finalize_command.add_argument("--start-metrics", required=True)
    finalize_command.add_argument("--end-metrics", required=True)
    finalize_command.add_argument("--log", action="append", required=True)
    finalize_command.add_argument("--output", required=True)
    finalize_command.set_defaults(handler=finalize)
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
