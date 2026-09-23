#!/usr/bin/env python3
"""Bounded, non-authoritative supervisor for an observe-only ACE capture.

The supervisor observes a launcher; it never decides that a temporary
``/metrics`` outage is a runtime failure and it never sends a shutdown for
that reason.  It records last-known-good scrape evidence plus the actual
process/lifecycle outcome so the offline finalizer can distinguish an
endpoint disappearing during controlled shutdown from the root cause.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


STATUS_SCHEMA_VERSION = 1
METRICS_SNAPSHOT_SCHEMA_VERSION = 1
METRICS_RETRY_MAX_S = 30.0
REQUIRED_COUNTERS = (
    "pr1_runtime_bypass_attempt_total",
    "pr1_runtime_candidate_admission_closed_total",
    "pr1_runtime_primary_coverage_gap_total",
    "ace_capture_segment_invalid_total",
)


def now_ms() -> int:
    return time.time_ns() // 1_000_000


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def classify_metrics_error(error: Exception) -> str:
    text = str(error).lower()
    if isinstance(error, TimeoutError) or "timed out" in text or "timeout" in text:
        return "timeout"
    if isinstance(error, urllib.error.HTTPError):
        if error.code == 429:
            return "http_429"
        return f"http_{error.code}"
    if "connection refused" in text or "connection reset" in text or "dns" in text:
        return "transport"
    return "endpoint_unavailable"


def metrics_retry_delay_s(base_poll_s: float, consecutive_failures: int) -> float:
    """Bound scrape pressure while preserving independent process liveness."""
    exponent = max(consecutive_failures - 1, 0)
    return min(base_poll_s * (2**min(exponent, 5)), METRICS_RETRY_MAX_S)


def fetch_metrics(url: str, timeout_s: float) -> bytes:
    request = urllib.request.Request(url, method="GET")
    with urllib.request.urlopen(request, timeout=timeout_s) as response:
        if response.status != 200:
            raise RuntimeError(f"metrics endpoint returned HTTP {response.status}")
        return response.read()


def write_atomic_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("x", encoding="utf-8") as handle:
        json.dump(payload, handle, sort_keys=True, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def write_new_json(path: Path, payload: dict[str, Any]) -> None:
    """Persist an immutable, manifest-bound artifact exactly once."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(payload, handle, sort_keys=True, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def parse_prometheus_counters(payload: str) -> dict[str, int]:
    values: dict[str, int] = {}
    for name in REQUIRED_COUNTERS:
        pattern = re.compile(rf"^{re.escape(name)}\s+([0-9]+(?:\.0+)?)\s*$")
        for line in payload.splitlines():
            match = pattern.match(line)
            if match:
                values[name] = int(float(match.group(1)))
                break
    return values


def write_bound_metrics_snapshot(
    path: Path,
    *,
    manifest: dict[str, Any],
    manifest_sha256: str,
    capture_kind: str,
    phase: str,
    body: bytes,
    captured_at_unix_ms: int | None = None,
    source: str = "direct",
) -> None:
    if source not in {"direct", "last_known_good"}:
        raise RuntimeError(f"unsupported metrics snapshot source: {source}")
    values = parse_prometheus_counters(body.decode("utf-8"))
    missing = [name for name in REQUIRED_COUNTERS if name not in values]
    if missing:
        raise RuntimeError(
            "metrics endpoint does not expose required ACE health series: " + ", ".join(missing)
        )
    write_new_json(
        path,
        {
            "schema_version": METRICS_SNAPSHOT_SCHEMA_VERSION,
            "run_id": manifest["run_id"],
            "manifest_sha256": manifest_sha256,
            "phase": phase,
            "capture_kind": capture_kind,
            # A fallback end snapshot must retain the time at which its
            # counters were actually observed.  Never relabel an older
            # last-known-good scrape as a fresh post-failure measurement.
            "captured_at_unix_ms": captured_at_unix_ms or now_ms(),
            "source": source,
            "counters": values,
            "raw_metrics_sha256": sha256_hex(body),
        },
    )


def read_manifest(path: Path) -> tuple[dict[str, Any], str]:
    raw = path.read_bytes()
    manifest = json.loads(raw)
    if not isinstance(manifest, dict):
        raise RuntimeError("capture manifest must be a JSON object")
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not run_id:
        raise RuntimeError("capture manifest has no run_id")
    return manifest, sha256_hex(raw)


@dataclass
class MetricsState:
    consecutive_failures: int = 0
    total_failures: int = 0
    last_success_at_unix_ms: int | None = None
    last_success_metrics_sha256: str | None = None
    last_error_class: str | None = None
    last_error: str | None = None

    def success(self, body: bytes) -> None:
        self.consecutive_failures = 0
        self.last_success_at_unix_ms = now_ms()
        self.last_success_metrics_sha256 = sha256_hex(body)
        self.last_error_class = None
        self.last_error = None

    def failure(self, error: Exception) -> None:
        self.consecutive_failures += 1
        self.total_failures += 1
        self.last_error_class = classify_metrics_error(error)
        self.last_error = str(error)


@dataclass
class LifecycleState:
    run_id: str
    manifest_sha256: str
    capture_kind: str
    launcher_pid: int
    started_at_unix_ms: int = field(default_factory=now_ms)
    finished_at_unix_ms: int | None = None
    launcher_returncode: int | None = None
    shutdown_requested: bool = False
    shutdown_signal: str | None = None
    # Explicit supervisor-owned reason for a normal controlled shutdown.  It
    # is distinct from `exit_reason`, which is derived from launcher logs.
    stop_reason: str | None = None
    endpoint_state: str = "unknown"
    exit_reason: str | None = None
    end_snapshot_source: str | None = None
    end_snapshot_age_ms: int | None = None
    metrics: MetricsState = field(default_factory=MetricsState)

    def as_json(self) -> dict[str, Any]:
        payload = asdict(self)
        payload["schema_version"] = STATUS_SCHEMA_VERSION
        return payload


def derive_launcher_exit_reason(paths: list[Path], returncode: int | None) -> str:
    markers = (
        "RUG_SCALP_RUNTIME_FEE_AUTHORITY_CHANGE_REQUESTED_CONTROLLED_SHUTDOWN",
        "RUG_SCALP_RUNTIME_FEE_AUTHORITY_INVALIDATED",
        "Oracle Runtime failed before shutdown signal",
        "Oracle Runtime task failed before shutdown signal",
        "Seer component failed before shutdown signal",
        "Seer component task failed before shutdown signal",
        "Component shutdown completed with",
        "Ghost Launcher shutdown complete",
    )
    for path in paths:
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for marker in markers:
            if marker in text:
                return marker
    if returncode is None:
        return "launcher_still_running"
    if returncode == 0:
        return "launcher_exited_zero_without_known_marker"
    return f"launcher_exit_{returncode}"


def record_scrape(state: LifecycleState, metrics_url: str, timeout_s: float) -> bytes | None:
    try:
        body = fetch_metrics(metrics_url, timeout_s)
    except Exception as error:  # Never grants the supervisor shutdown authority.
        state.metrics.failure(error)
        return None
    state.metrics.success(body)
    state.endpoint_state = "healthy"
    return body


def wait_for_manifest(
    path: Path,
    process: subprocess.Popen[bytes],
    timeout_s: float,
) -> tuple[dict[str, Any], str]:
    """Wait only for startup materialization, never for a metrics response."""
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if path.is_file():
            return read_manifest(path)
        if process.poll() is not None:
            raise RuntimeError(
                f"launcher exited before creating immutable capture manifest: rc={process.returncode}"
            )
        time.sleep(0.1)
    raise RuntimeError(f"capture manifest was not materialized within {timeout_s}s: {path}")


def prospective_stop_evidence_is_valid(
    path: Path,
    *,
    manifest: dict[str, Any],
    manifest_sha256: str,
    base_contract_sha256: str,
    amendment_sha256: str,
    feature_scale_sha256: str,
) -> bool:
    """Validate only the immutable monitor-to-supervisor handoff boundary.

    The Rust final evaluator recomputes the candidate-order cohort after
    shutdown.  The supervisor checks enough provenance here to ensure a stale
    or foreign evidence file cannot own the launcher shutdown signal.
    """
    try:
        evidence = json.loads(path.read_bytes())
    except (OSError, json.JSONDecodeError):
        return False
    if not isinstance(evidence, dict):
        return False
    return (
        evidence.get("schema") == "ace_ev_v2_prospective_stop_evidence_v1"
        and evidence.get("run_id") == manifest.get("run_id")
        and evidence.get("manifest_sha256") == manifest_sha256
        and evidence.get("base_contract_sha256") == base_contract_sha256
        and evidence.get("amendment_sha256") == amendment_sha256
        and evidence.get("feature_scale_sha256") == feature_scale_sha256
        and evidence.get("implementation_sha") == manifest.get("implementation_sha")
        and evidence.get("target_terminal_outcomes") == 1000
        and evidence.get("terminal_outcome_count") == 1000
        and isinstance(evidence.get("cohort_candidate_order_sha256"), str)
        and bool(evidence.get("cohort_candidate_order_sha256"))
        and isinstance(evidence.get("complete_file_prefixes"), list)
        and bool(evidence.get("complete_file_prefixes"))
    )


def supervise(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    stdout_path = Path(args.stdout)
    stderr_path = Path(args.stderr)
    status_path = Path(args.status_output)
    start_snapshot_path = Path(args.start_snapshot)
    end_snapshot_path = Path(args.end_snapshot)
    for path, label in (
        (status_path, "status output"),
        (start_snapshot_path, "start metrics snapshot"),
        (end_snapshot_path, "end metrics snapshot"),
    ):
        if path.exists():
            raise RuntimeError(f"{label} already exists: {path}")
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stderr_path.parent.mkdir(parents=True, exist_ok=True)
    if stdout_path.exists() or stderr_path.exists():
        raise RuntimeError("supervisor stdout/stderr outputs must be fresh")

    with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
        process = subprocess.Popen(
            args.launcher_command,
            stdout=stdout,
            stderr=stderr,
            start_new_session=True,
        )
        try:
            manifest, manifest_sha256 = wait_for_manifest(
                manifest_path, process, args.manifest_ready_timeout_s
            )
        except Exception as error:
            # There is no manifest-bound health receipt possible in this case,
            # but preserve the real launcher outcome for operators rather than
            # manufacturing a metrics-root-cause story.
            state = LifecycleState(
                run_id="manifest_not_materialized",
                manifest_sha256="",
                capture_kind=args.capture_kind,
                launcher_pid=process.pid,
            )
            state.exit_reason = f"manifest_materialization_failed:{error}"
            if process.poll() is None:
                state.shutdown_requested = True
                state.shutdown_signal = "SIGINT_AFTER_MANIFEST_FAILURE"
                os.killpg(process.pid, signal.SIGINT)
                try:
                    process.wait(timeout=args.shutdown_timeout_s)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=args.shutdown_timeout_s)
            state.launcher_returncode = process.poll()
            state.finished_at_unix_ms = now_ms()
            write_atomic_json(status_path, state.as_json())
            return 2

        state = LifecycleState(
            run_id=manifest["run_id"],
            manifest_sha256=manifest_sha256,
            capture_kind=args.capture_kind,
            launcher_pid=process.pid,
        )
        start = time.monotonic()
        max_duration_s = args.max_duration_s if args.capture_kind == "prospective" else args.duration_s
        assert max_duration_s is not None
        start_snapshot_taken = False
        end_snapshot_taken = False
        last_known_good: tuple[bytes, int] | None = None
        try:
            while process.poll() is None and time.monotonic() - start < max_duration_s:
                body = record_scrape(state, args.metrics_url, args.metrics_timeout_s)
                if body is not None:
                    assert state.metrics.last_success_at_unix_ms is not None
                    last_known_good = (body, state.metrics.last_success_at_unix_ms)
                if body is not None and not start_snapshot_taken:
                    try:
                        write_bound_metrics_snapshot(
                            start_snapshot_path,
                            manifest=manifest,
                            manifest_sha256=manifest_sha256,
                            capture_kind=args.capture_kind,
                            phase="start",
                            body=body,
                        )
                        start_snapshot_taken = True
                    except Exception as error:
                        # Snapshot schema completeness is a finalization
                        # concern; do not turn it into process authority.
                        state.metrics.failure(error)
                write_atomic_json(status_path, state.as_json())
                if (
                    args.capture_kind == "prospective"
                    and start_snapshot_taken
                    and args.stop_evidence_path
                    and Path(args.stop_evidence_path).is_file()
                    and prospective_stop_evidence_is_valid(
                        Path(args.stop_evidence_path),
                        manifest=manifest,
                        manifest_sha256=manifest_sha256,
                        base_contract_sha256=args.prospective_base_contract_sha256,
                        amendment_sha256=args.prospective_amendment_sha256,
                        feature_scale_sha256=args.prospective_feature_scale_sha256,
                    )
                ):
                    state.stop_reason = "target_reached"
                    break
                if body is None:
                    time.sleep(
                        metrics_retry_delay_s(args.poll_s, state.metrics.consecutive_failures)
                    )
                else:
                    time.sleep(args.poll_s)

            # A final scrape is deliberately attempted before SIGINT.  If it
            # fails, the lifecycle state preserves the error but the process is
            # still allowed to shut down cleanly.
            if process.poll() is None:
                if state.stop_reason is None:
                    state.stop_reason = (
                        "max_duration_insufficient_yield"
                        if args.capture_kind == "prospective"
                        else "duration_elapsed"
                    )
                body = record_scrape(state, args.metrics_url, args.metrics_timeout_s)
                if body is not None:
                    assert state.metrics.last_success_at_unix_ms is not None
                    last_known_good = (body, state.metrics.last_success_at_unix_ms)
                    try:
                        write_bound_metrics_snapshot(
                            end_snapshot_path,
                            manifest=manifest,
                            manifest_sha256=manifest_sha256,
                            capture_kind=args.capture_kind,
                            phase="end",
                            body=body,
                            captured_at_unix_ms=state.metrics.last_success_at_unix_ms,
                        )
                        end_snapshot_taken = True
                        state.end_snapshot_source = "direct"
                        state.end_snapshot_age_ms = 0
                    except Exception as error:
                        state.metrics.failure(error)
                elif last_known_good is not None:
                    last_body, last_success_at_unix_ms = last_known_good
                    age_ms = now_ms() - last_success_at_unix_ms
                    if age_ms <= int(args.last_known_good_max_age_s * 1_000):
                        try:
                            write_bound_metrics_snapshot(
                                end_snapshot_path,
                                manifest=manifest,
                                manifest_sha256=manifest_sha256,
                                capture_kind=args.capture_kind,
                                phase="end",
                                body=last_body,
                                captured_at_unix_ms=last_success_at_unix_ms,
                                source="last_known_good",
                            )
                            end_snapshot_taken = True
                            state.end_snapshot_source = "last_known_good"
                            state.end_snapshot_age_ms = age_ms
                            state.endpoint_state = "final_scrape_unavailable_last_known_good"
                        except Exception as error:
                            state.metrics.failure(error)
                state.shutdown_requested = True
                state.shutdown_signal = "SIGINT"
                write_atomic_json(status_path, state.as_json())
                os.killpg(process.pid, signal.SIGINT)
            try:
                process.wait(timeout=args.shutdown_timeout_s)
            except subprocess.TimeoutExpired:
                state.shutdown_signal = "SIGTERM_AFTER_TIMEOUT"
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=args.shutdown_timeout_s)
        finally:
            state.launcher_returncode = process.poll()
            state.finished_at_unix_ms = now_ms()
            if state.shutdown_requested and not end_snapshot_taken and state.metrics.total_failures:
                state.endpoint_state = "unavailable_during_or_before_controlled_shutdown"
            elif state.metrics.total_failures:
                state.endpoint_state = "intermittently_unavailable"
            state.exit_reason = derive_launcher_exit_reason(
                [stdout_path, stderr_path], state.launcher_returncode
            )
            write_atomic_json(status_path, state.as_json())
    return 0 if state.launcher_returncode == 0 else 2


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument(
        "--capture-kind",
        choices=("smoke", "soak", "yield_qualification", "day1", "prospective"),
        required=True,
    )
    parser.add_argument("--metrics-url", required=True)
    parser.add_argument("--status-output", required=True)
    parser.add_argument(
        "--start-snapshot",
        required=True,
        help="fresh manifest-bound start metrics snapshot written by the supervisor",
    )
    parser.add_argument(
        "--end-snapshot",
        required=True,
        help="fresh manifest-bound pre-SIGINT metrics snapshot written by the supervisor",
    )
    parser.add_argument("--stdout", required=True)
    parser.add_argument("--stderr", required=True)
    parser.add_argument("--duration-s", type=float)
    parser.add_argument(
        "--max-duration-s",
        type=float,
        help="required only for prospective; includes its bounded outcome drain",
    )
    parser.add_argument(
        "--stop-evidence-path",
        help="immutable Rust monitor evidence which can trigger prospective shutdown",
    )
    parser.add_argument("--prospective-base-contract-sha256")
    parser.add_argument("--prospective-amendment-sha256")
    parser.add_argument("--prospective-feature-scale-sha256")
    parser.add_argument("--poll-s", type=float, default=1.0)
    parser.add_argument("--metrics-timeout-s", type=float, default=2.0)
    parser.add_argument(
        "--last-known-good-max-age-s",
        type=float,
        default=35.0,
        help="maximum truthful age of a pre-SIGINT fallback metrics snapshot",
    )
    parser.add_argument("--shutdown-timeout-s", type=float, default=120.0)
    parser.add_argument("--manifest-ready-timeout-s", type=float, default=120.0)
    parser.add_argument("launcher_command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not args.launcher_command:
        parser.error("launcher command is required after --")
    if args.launcher_command[0] == "--":
        args.launcher_command = args.launcher_command[1:]
    if not args.launcher_command:
        parser.error("launcher command is required after --")
    if args.capture_kind == "prospective":
        if args.duration_s is not None:
            parser.error("prospective uses --max-duration-s, not --duration-s")
        if args.max_duration_s is None or args.max_duration_s <= 0:
            parser.error("prospective requires positive --max-duration-s")
        if not args.stop_evidence_path:
            parser.error("prospective requires --stop-evidence-path")
        if not all(
            isinstance(value, str) and len(value) == 64 and all(char in "0123456789abcdef" for char in value)
            for value in (
                args.prospective_base_contract_sha256,
                args.prospective_amendment_sha256,
                args.prospective_feature_scale_sha256,
            )
        ):
            parser.error("prospective requires three lowercase SHA-256 provenance arguments")
    elif args.duration_s is None or args.duration_s <= 0 or args.max_duration_s is not None:
        parser.error("non-prospective captures require positive --duration-s and no --max-duration-s")
    if (
        (args.duration_s is not None and args.duration_s <= 0)
        or args.poll_s <= 0
        or args.metrics_timeout_s <= 0
        or args.last_known_good_max_age_s <= 0
        or args.manifest_ready_timeout_s <= 0
    ):
        parser.error("durations and timeouts must be positive")
    return args


def main() -> int:
    try:
        return supervise(parse_args())
    except Exception as error:
        print(f"[error] ACE capture supervisor failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
