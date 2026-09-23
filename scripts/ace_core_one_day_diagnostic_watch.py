#!/usr/bin/env python3
"""Stop one ACE diagnostic launcher on the first IPC egress saturation marker.

The marker is emitted by the launcher process, so this watcher intentionally
tails only ``launcher.stdout.log``. It neither changes runtime control nor
interprets Oracle or system logs as saturation evidence.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path


SATURATION_MARKER = "IPC_EGRESS_SATURATED"


@dataclass(frozen=True)
class WatchResult:
    reason: str
    marker_seen: bool
    stopped_pid: int | None
    stopped_at_unix_ms: int


def now_unix_ms() -> int:
    return time.time_ns() // 1_000_000


def snapshot_metrics(metrics_url: str | None, output_path: Path | None) -> str | None:
    if metrics_url is None or output_path is None:
        return None
    try:
        with urllib.request.urlopen(metrics_url, timeout=3) as response:
            raw_metrics = response.read()
    except (OSError, urllib.error.URLError) as error:
        return f"metrics_snapshot_failed:{error}"

    output_path.parent.mkdir(parents=True, exist_ok=True)
    # The snapshot is evidence for the diagnostic classification.  It must be
    # durable before this watcher asks the launcher to stop the metrics server.
    with output_path.open("wb") as stream:
        stream.write(raw_metrics)
        stream.flush()
        os.fsync(stream.fileno())
    return None


def preflight_metrics(metrics_url: str) -> str | None:
    """Verify the configured endpoint before starting to watch saturation."""
    try:
        with urllib.request.urlopen(metrics_url, timeout=3) as response:
            response.read(1)
    except (OSError, urllib.error.URLError) as error:
        return f"metrics_preflight_failed:{error}"
    return None


def contains_saturation_marker(raw: bytes) -> bool:
    return SATURATION_MARKER.encode("utf-8") in raw


def write_result(path: Path | None, result: WatchResult, metrics_error: str | None) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = asdict(result)
    payload["metrics_snapshot_error"] = metrics_error
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def request_controlled_stop(pid: int, dry_run: bool) -> int | None:
    if dry_run:
        return None
    os.kill(pid, signal.SIGINT)
    return pid


def watch(
    *,
    launcher_stdout_log: Path,
    launcher_pid: int,
    timeout_seconds: float,
    poll_seconds: float,
    metrics_url: str | None,
    metrics_output: Path | None,
    status_path: Path | None,
    dry_run: bool,
) -> WatchResult:
    deadline = time.monotonic() + timeout_seconds
    offset = 0

    while time.monotonic() < deadline:
        try:
            with launcher_stdout_log.open("rb") as stream:
                stream.seek(offset)
                chunk = stream.read()
                offset = stream.tell()
        except FileNotFoundError:
            chunk = b""

        if contains_saturation_marker(chunk):
            metrics_error = snapshot_metrics(metrics_url, metrics_output)
            result = WatchResult(
                reason="ipc_egress_saturated",
                marker_seen=True,
                stopped_pid=request_controlled_stop(launcher_pid, dry_run),
                stopped_at_unix_ms=now_unix_ms(),
            )
            write_result(status_path, result, metrics_error)
            return result

        time.sleep(poll_seconds)

    metrics_error = snapshot_metrics(metrics_url, metrics_output)
    result = WatchResult(
        reason="timeout",
        marker_seen=False,
        stopped_pid=request_controlled_stop(launcher_pid, dry_run),
        stopped_at_unix_ms=now_unix_ms(),
    )
    write_result(status_path, result, metrics_error)
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--launcher-stdout-log", type=Path, required=True)
    parser.add_argument("--launcher-pid", type=int, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=600.0)
    parser.add_argument("--poll-seconds", type=float, default=0.25)
    parser.add_argument("--metrics-url")
    parser.add_argument("--metrics-output", type=Path)
    parser.add_argument(
        "--preflight-metrics",
        action="store_true",
        help="require a successful GET to --metrics-url before watching",
    )
    parser.add_argument("--status-path", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.poll_seconds <= 0:
        parser.error("--poll-seconds must be positive")
    if (args.metrics_url is None) != (args.metrics_output is None):
        parser.error("--metrics-url and --metrics-output must be supplied together")
    if args.preflight_metrics and args.metrics_url is None:
        parser.error("--preflight-metrics requires --metrics-url")
    return args


def main() -> int:
    args = parse_args()
    if args.preflight_metrics:
        error = preflight_metrics(args.metrics_url)
        if error is not None:
            print(error, file=sys.stderr)
            return 2
    result = watch(
        launcher_stdout_log=args.launcher_stdout_log,
        launcher_pid=args.launcher_pid,
        timeout_seconds=args.timeout_seconds,
        poll_seconds=args.poll_seconds,
        metrics_url=args.metrics_url,
        metrics_output=args.metrics_output,
        status_path=args.status_path,
        dry_run=args.dry_run,
    )
    print(json.dumps(asdict(result), sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
