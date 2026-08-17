#!/usr/bin/env python3
"""Fail-closed supervisor for one standalone Pump Research capture.

The supervisor owns the exact child returned by ``subprocess.Popen``.  It
never discovers that child by process name, never detaches it behind GNU
``timeout`` and calls ``waitpid()`` exactly once after pidfd readiness.  The
resulting operator receipt preserves raw wait status plus exit code or signal.

This helper starts capture only.  It has no certify, qualification, export or
strategy path.
"""

from __future__ import annotations

import argparse
import ctypes
import fcntl
import hashlib
import json
import os
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time
import tomllib
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import BinaryIO, Callable, Sequence


SCHEMA_VERSION = 1
_PR_SET_PDEATHSIG = 1
_LEGACY_CREDENTIAL_NAMES = (
    "GHOST_SEER_GRPC_X_TOKEN",
    "GHOST_RPC_AUTH_TOKEN",
)
_CAPTURE_LOCK_FILENAME = ".pump-research-capture.lock"
_CAPTURE_LOCK_SCOPE = "canonical_output_directory_v1"
_ENVIRONMENT_NAME_V1 = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")

_FAILURE_CHILD_EXIT_NONZERO = "CHILD_EXIT_NONZERO"
_FAILURE_NEW_RUN_COUNT_NOT_ONE = "NEW_RUN_COUNT_NOT_ONE"
_FAILURE_RAW_DIRECTORY_MISSING = "RAW_DIRECTORY_MISSING"
_FAILURE_COMPLETION_RECEIPT_MISSING = "COMPLETION_RECEIPT_MISSING"
_FAILURE_COMPLETION_RECEIPT_INVALID = "COMPLETION_RECEIPT_INVALID"
_FAILURE_COMPLETION_RUN_ID_MISMATCH = "COMPLETION_RUN_ID_MISMATCH"
_FAILURE_COMPLETION_STATUS_NOT_COMPLETE = "COMPLETION_STATUS_NOT_COMPLETE"
_FAILURE_CLEAN_SHUTDOWN_FALSE = "CLEAN_SHUTDOWN_FALSE"
_FAILURE_PARTIAL_PATH_PRESENT = "PARTIAL_PATH_PRESENT"


def now_ms() -> int:
    return time.time_ns() // 1_000_000


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_new_json(path: Path, payload: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(payload, handle, sort_keys=True, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.chmod(path, 0o600)


def parent_death_signal_preexec(expected_parent_pid: int):
    """Bind the capture lifetime to this foreground supervisor on Linux."""

    def configure() -> None:
        if os.getppid() != expected_parent_pid:
            os._exit(126)
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.prctl(_PR_SET_PDEATHSIG, int(signal.SIGTERM), 0, 0, 0) != 0:
            os._exit(126)
        if os.getppid() != expected_parent_pid:
            os._exit(126)

    return configure


@dataclass(frozen=True)
class SupervisedProcessOutcome:
    child_pid: int
    started_wall_ms: int
    ended_wall_ms: int
    elapsed_ms: int
    shutdown_reason: str | None
    shutdown_signal: int | None
    shutdown_sent_wall_ms: int | None
    forced_kill: bool
    raw_wait_status: int
    returncode: int
    process_exit_status: int | None
    process_termination_signal: int | None
    minimum_observed_free_bytes: int
    free_bytes_after: int
    wait_call_count: int


@dataclass(frozen=True)
class CapturePostconditionV1:
    operator_capture_success: bool
    operator_failure_code: str | None
    new_run_ids: tuple[str, ...]
    validated_run_id: str | None
    completion_receipt_path: str | None
    completion_receipt_sha256: str | None
    completion_status: str | None
    completion_clean_shutdown: bool | None
    partial_path_count: int | None


def supervise_process(
    command: Sequence[str],
    *,
    log_path: Path,
    child_environment: dict[str, str],
    duration_seconds: float,
    drain_timeout_seconds: float,
    disk_path: Path,
    disk_floor_bytes: int,
    disk_poll_seconds: float,
    external_shutdown_reason: Callable[[], str | None] | None = None,
    free_bytes_reader: Callable[[Path], int] | None = None,
    after_spawn: Callable[[int], None] | None = None,
) -> SupervisedProcessOutcome:
    """Run and reap one exact child without process-name discovery.

    A pidfd is used only to observe exit readiness.  The child is reaped once,
    at the final ``waitpid()`` call.  Both the raw Linux wait status and its
    normalized exit-or-signal interpretation are preserved in the receipt.
    """

    if duration_seconds <= 0:
        raise ValueError("duration_seconds must be positive")
    if drain_timeout_seconds <= 0:
        raise ValueError("drain_timeout_seconds must be positive")
    if disk_floor_bytes < 0:
        raise ValueError("disk_floor_bytes must not be negative")
    if disk_poll_seconds <= 0:
        raise ValueError("disk_poll_seconds must be positive")
    if not hasattr(os, "pidfd_open"):
        raise RuntimeError("pump research supervisor requires Linux pidfd_open")

    read_free = free_bytes_reader or (lambda path: shutil.disk_usage(path).free)
    started_wall_ms = now_ms()
    start_monotonic = time.monotonic()
    duration_deadline = start_monotonic + duration_seconds
    next_disk_check = start_monotonic + disk_poll_seconds
    minimum_free = read_free(disk_path)
    shutdown_reason: str | None = None
    shutdown_signal: int | None = None
    shutdown_sent_wall_ms: int | None = None
    drain_deadline: float | None = None
    forced_kill = False
    child: subprocess.Popen[bytes] | None = None
    wait_call_count = 0

    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("xb") as log:
        os.chmod(log_path, 0o600)
        child = subprocess.Popen(
            list(command),
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            env=child_environment,
            start_new_session=True,
            preexec_fn=parent_death_signal_preexec(os.getpid()),
        )
        pidfd = os.pidfd_open(child.pid, 0)
        if after_spawn is not None:
            after_spawn(child.pid)
        selector = selectors.DefaultSelector()
        selector.register(pidfd, selectors.EVENT_READ)
        try:
            while True:
                now = time.monotonic()
                timeout_candidates = [max(0.0, next_disk_check - now)]
                if shutdown_reason is None:
                    timeout_candidates.append(max(0.0, duration_deadline - now))
                elif drain_deadline is not None:
                    timeout_candidates.append(max(0.0, drain_deadline - now))
                timeout = min(timeout_candidates + [0.25])
                if selector.select(timeout):
                    break

                now = time.monotonic()
                if now >= next_disk_check:
                    observed_free = read_free(disk_path)
                    minimum_free = min(minimum_free, observed_free)
                    next_disk_check = now + disk_poll_seconds
                    if observed_free < disk_floor_bytes and shutdown_reason is None:
                        shutdown_reason = "disk_floor"

                if shutdown_reason is None and external_shutdown_reason is not None:
                    shutdown_reason = external_shutdown_reason()

                if shutdown_reason is None and now >= duration_deadline:
                    shutdown_reason = "duration_elapsed"

                if shutdown_reason is not None and shutdown_signal is None:
                    shutdown_signal = int(signal.SIGINT)
                    shutdown_sent_wall_ms = now_ms()
                    drain_deadline = now + drain_timeout_seconds
                    try:
                        os.kill(child.pid, signal.SIGINT)
                    except ProcessLookupError:
                        pass

                if drain_deadline is not None and now >= drain_deadline:
                    forced_kill = True
                    try:
                        os.kill(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    # After SIGKILL, wait for pidfd readiness without inventing
                    # another timeout or reaping through poll().
                    while not selector.select(0.25):
                        continue
                    break

            wait_call_count += 1
            waited_pid, raw_wait_status = os.waitpid(child.pid, 0)
            if waited_pid != child.pid:
                raise RuntimeError(
                    f"waitpid reaped unexpected PID {waited_pid}; expected {child.pid}"
                )
            returncode = os.waitstatus_to_exitcode(raw_wait_status)
            # The exact child was already reaped above.  Mark the Popen object
            # complete so its destructor cannot perform a second wait.
            child.returncode = returncode
        finally:
            selector.close()
            os.close(pidfd)

    ended_wall_ms = now_ms()
    free_after = read_free(disk_path)
    minimum_free = min(minimum_free, free_after)
    return SupervisedProcessOutcome(
        child_pid=child.pid,
        started_wall_ms=started_wall_ms,
        ended_wall_ms=ended_wall_ms,
        elapsed_ms=ended_wall_ms - started_wall_ms,
        shutdown_reason=shutdown_reason,
        shutdown_signal=shutdown_signal,
        shutdown_sent_wall_ms=shutdown_sent_wall_ms,
        forced_kill=forced_kill,
        raw_wait_status=raw_wait_status,
        returncode=returncode,
        process_exit_status=returncode if returncode >= 0 else None,
        process_termination_signal=-returncode if returncode < 0 else None,
        minimum_observed_free_bytes=minimum_free,
        free_bytes_after=free_after,
        wait_call_count=wait_call_count,
    )


def regular_non_symlink(path: Path, label: str) -> None:
    if not path.is_file() or path.is_symlink():
        raise RuntimeError(f"{label} must be a regular non-symlink file: {path}")


def active_pump_capture_pids() -> list[int]:
    """Detect competing captures without using truncated process names."""

    found: list[int] = []
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            executable = Path((process / "exe").resolve()).name
            arguments = [
                value.decode(errors="replace")
                for value in (process / "cmdline").read_bytes().split(b"\0")
                if value
            ]
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if executable == "pump-research-tape" and "capture" in arguments:
            found.append(int(process.name))
    return sorted(found)


def raw_run_directories(output_dir: Path) -> set[str]:
    return {
        path.name
        for path in output_dir.glob("pump-research-*")
        if path.is_dir() and not path.is_symlink()
    }


def acquire_output_capture_lock(output_dir: Path) -> tuple[BinaryIO, Path]:
    """Acquire the one capture lock shared by every operator directory.

    ``output_dir`` is canonicalized by the caller.  The lock therefore follows
    the physical dataset root rather than an arbitrary operator-log location.
    It is acquired before process discovery, the pre-run snapshot and Popen.
    """

    lock_path = output_dir / _CAPTURE_LOCK_FILENAME
    flags = os.O_RDWR | os.O_CREAT | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0)
    try:
        lock_fd = os.open(lock_path, flags, 0o600)
    except OSError as error:
        raise RuntimeError(
            f"cannot open capture lock for output directory: {output_dir}"
        ) from error

    try:
        if not stat.S_ISREG(os.fstat(lock_fd).st_mode):
            raise RuntimeError(f"capture lock must be a regular file: {lock_path}")
        os.fchmod(lock_fd, 0o600)
        try:
            fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError(
                f"capture lock is already held for output directory: {output_dir}"
            ) from error
        return os.fdopen(lock_fd, "r+b", closefd=True), lock_path
    except Exception:
        os.close(lock_fd)
        raise


def capture_child_environment(
    parent_environment: dict[str, str],
    configured_credential_names: tuple[str, ...],
) -> dict[str, str]:
    """Create the exact capture-child environment before ``Popen``.

    Dedicated credential names declared by the standalone capture config are
    intentionally preserved for the capture child.  Legacy aliases are never
    inherited, even when they exist in the supervisor's parent environment.
    """

    if any(
        name in _LEGACY_CREDENTIAL_NAMES for name in configured_credential_names
    ):
        raise RuntimeError("capture config must not use a legacy credential environment name")
    child_environment = dict(parent_environment)
    for name in _LEGACY_CREDENTIAL_NAMES:
        child_environment.pop(name, None)
    return child_environment


def capture_postcondition(
    output_dir: Path,
    runs_before: set[str],
    child_returncode: int,
) -> CapturePostconditionV1:
    """Classify the exact child outcome without mutating its raw run."""

    new_run_ids = tuple(sorted(raw_run_directories(output_dir) - runs_before))
    if child_returncode != 0:
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_CHILD_EXIT_NONZERO,
            new_run_ids=new_run_ids,
            validated_run_id=None,
            completion_receipt_path=None,
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=None,
        )
    if len(new_run_ids) != 1:
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_NEW_RUN_COUNT_NOT_ONE,
            new_run_ids=new_run_ids,
            validated_run_id=None,
            completion_receipt_path=None,
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=None,
        )

    run_id = new_run_ids[0]
    run_dir = output_dir / run_id
    raw_dir = run_dir / "raw"
    if not raw_dir.is_dir() or raw_dir.is_symlink():
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_RAW_DIRECTORY_MISSING,
            new_run_ids=new_run_ids,
            validated_run_id=run_id,
            completion_receipt_path=None,
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=None,
        )

    partial_path_count = sum(1 for _ in run_dir.rglob("*.partial"))
    completion_path = raw_dir / "run_completion_receipt.json"
    if not completion_path.is_file() or completion_path.is_symlink():
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_COMPLETION_RECEIPT_MISSING,
            new_run_ids=new_run_ids,
            validated_run_id=run_id,
            completion_receipt_path=str(completion_path),
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=partial_path_count,
        )
    try:
        completion_bytes = completion_path.read_bytes()
        completion = json.loads(completion_bytes)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_COMPLETION_RECEIPT_INVALID,
            new_run_ids=new_run_ids,
            validated_run_id=run_id,
            completion_receipt_path=str(completion_path),
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=partial_path_count,
        )
    if not isinstance(completion, dict):
        return CapturePostconditionV1(
            operator_capture_success=False,
            operator_failure_code=_FAILURE_COMPLETION_RECEIPT_INVALID,
            new_run_ids=new_run_ids,
            validated_run_id=run_id,
            completion_receipt_path=str(completion_path),
            completion_receipt_sha256=None,
            completion_status=None,
            completion_clean_shutdown=None,
            partial_path_count=partial_path_count,
        )

    completion_sha256 = hashlib.sha256(completion_bytes).hexdigest()
    completion_status = completion.get("status")
    completion_clean_shutdown = completion.get("clean_shutdown")
    if completion.get("run_id") != run_id:
        failure_code = _FAILURE_COMPLETION_RUN_ID_MISMATCH
    elif completion_status != "Complete":
        failure_code = _FAILURE_COMPLETION_STATUS_NOT_COMPLETE
    elif completion_clean_shutdown is not True:
        failure_code = _FAILURE_CLEAN_SHUTDOWN_FALSE
    elif partial_path_count != 0:
        failure_code = _FAILURE_PARTIAL_PATH_PRESENT
    else:
        failure_code = None
    return CapturePostconditionV1(
        operator_capture_success=failure_code is None,
        operator_failure_code=failure_code,
        new_run_ids=new_run_ids,
        validated_run_id=run_id,
        completion_receipt_path=str(completion_path),
        completion_receipt_sha256=completion_sha256,
        completion_status=(completion_status if isinstance(completion_status, str) else None),
        completion_clean_shutdown=(
            completion_clean_shutdown
            if isinstance(completion_clean_shutdown, bool)
            else None
        ),
        partial_path_count=partial_path_count,
    )


def config_credentials(config: dict[str, object]) -> tuple[str, ...]:
    names: list[str] = []
    for field in ("grpc_auth_token_env", "rpc_auth_token_env"):
        value = config.get(field)
        if value is None:
            continue
        if not isinstance(value, str) or not value.strip() or value.strip() != value:
            raise RuntimeError(f"capture config {field} must be a non-empty environment name")
        if _ENVIRONMENT_NAME_V1.fullmatch(value) is None:
            raise RuntimeError(f"capture config {field} is not a valid environment name")
        if value not in names:
            names.append(value)
    return tuple(names)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--provenance-receipt", type=Path, required=True)
    parser.add_argument("--operator-dir", type=Path, required=True)
    parser.add_argument("--duration-seconds", type=float, required=True)
    parser.add_argument("--drain-timeout-seconds", type=float, default=120.0)
    parser.add_argument("--start-free-min-bytes", type=int, required=True)
    parser.add_argument("--disk-floor-bytes", type=int, required=True)
    parser.add_argument("--disk-poll-seconds", type=float, default=20.0)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    os.umask(0o077)
    regular_non_symlink(arguments.binary, "sealed binary")
    regular_non_symlink(arguments.config, "external capture config")
    regular_non_symlink(arguments.provenance_receipt, "preflight receipt")
    if arguments.operator_dir.exists():
        raise RuntimeError(f"operator directory already exists: {arguments.operator_dir}")
    if arguments.start_free_min_bytes < arguments.disk_floor_bytes:
        raise RuntimeError("start-free-min-bytes must be at least disk-floor-bytes")

    config_bytes = arguments.config.read_bytes()
    config = tomllib.loads(config_bytes.decode("utf-8"))
    output_dir_value = config.get("output_dir")
    if not isinstance(output_dir_value, str) or not output_dir_value:
        raise RuntimeError("capture config has no output_dir")
    configured_output_dir = Path(output_dir_value)
    if not configured_output_dir.is_dir():
        raise RuntimeError(
            f"capture output directory does not exist: {configured_output_dir}"
        )
    output_dir = configured_output_dir.resolve(strict=True)

    credential_names = config_credentials(config)
    missing = [name for name in credential_names if not os.environ.get(name)]
    if missing:
        raise RuntimeError(
            "required dedicated capture credential variables are unset: " + ", ".join(missing)
        )
    child_environment = capture_child_environment(dict(os.environ), credential_names)
    lock_handle, lock_path = acquire_output_capture_lock(output_dir)
    if active_pump_capture_pids():
        raise RuntimeError("another pump-research-tape capture is already active")
    free_bytes_before = shutil.disk_usage(output_dir).free
    if free_bytes_before < arguments.start_free_min_bytes:
        raise RuntimeError(
            f"capture output has {free_bytes_before} free bytes, below start minimum "
            f"{arguments.start_free_min_bytes}"
        )
    runs_before = raw_run_directories(output_dir)

    arguments.operator_dir.mkdir(parents=True, mode=0o700)
    os.chmod(arguments.operator_dir, 0o700)

    launch_receipt = arguments.operator_dir / "operator_launch_receipt_v1.json"
    execution_receipt = arguments.operator_dir / "operator_execution_receipt_v1.json"
    capture_log = arguments.operator_dir / "capture.log"
    write_new_json(
        launch_receipt,
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "pump_research_capture_supervisor_launch_v1",
            "started_wall_ms": now_ms(),
            "sealed_binary_sha256": sha256_file(arguments.binary),
            "preflight_receipt_sha256": sha256_file(arguments.provenance_receipt),
            "external_config_sha256": hashlib.sha256(config_bytes).hexdigest(),
            "duration_seconds": arguments.duration_seconds,
            "drain_timeout_seconds": arguments.drain_timeout_seconds,
            "start_free_min_bytes": arguments.start_free_min_bytes,
            "disk_floor_bytes": arguments.disk_floor_bytes,
            "free_bytes_before": free_bytes_before,
            "disk_poll_seconds": arguments.disk_poll_seconds,
            "capture_lock_scope": _CAPTURE_LOCK_SCOPE,
            "capture_lock_path": str(lock_path),
            "credential_environment_names": list(credential_names),
            "credential_values_persisted": False,
        },
    )

    pending_signal: list[int] = []

    def request_shutdown(signum: int, _frame: object) -> None:
        if not pending_signal:
            pending_signal.append(signum)

    previous_handlers = {
        signum: signal.signal(signum, request_shutdown)
        for signum in (signal.SIGINT, signal.SIGTERM)
    }

    def scrub_parent_credentials(_child_pid: int) -> None:
        for name in (*credential_names, *_LEGACY_CREDENTIAL_NAMES):
            os.environ.pop(name, None)
            child_environment.pop(name, None)

    command = [
        str(arguments.binary),
        "capture",
        "--config",
        str(arguments.config),
        "--provenance-receipt",
        str(arguments.provenance_receipt),
    ]
    try:
        outcome = supervise_process(
            command,
            log_path=capture_log,
            child_environment=child_environment,
            duration_seconds=arguments.duration_seconds,
            drain_timeout_seconds=arguments.drain_timeout_seconds,
            disk_path=output_dir,
            disk_floor_bytes=arguments.disk_floor_bytes,
            disk_poll_seconds=arguments.disk_poll_seconds,
            external_shutdown_reason=lambda: (
                f"operator_signal_{pending_signal[0]}" if pending_signal else None
            ),
            after_spawn=scrub_parent_credentials,
        )
    finally:
        scrub_parent_credentials(0)
        for signum, previous in previous_handlers.items():
            signal.signal(signum, previous)

    postcondition = capture_postcondition(output_dir, runs_before, outcome.returncode)
    write_new_json(
        execution_receipt,
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "pump_research_capture_supervisor_execution_v1",
            **asdict(outcome),
            "credentials_unset_in_supervisor": all(
                name not in os.environ for name in (*credential_names, *_LEGACY_CREDENTIAL_NAMES)
            ),
            "capture_lock_scope": _CAPTURE_LOCK_SCOPE,
            "capture_lock_path": str(lock_path),
            **asdict(postcondition),
            "post_capture_pipeline_started": False,
        },
    )
    if outcome.returncode < 0:
        supervisor_returncode = 128 + (-outcome.returncode)
    elif outcome.returncode != 0:
        supervisor_returncode = outcome.returncode
    else:
        supervisor_returncode = 0 if postcondition.operator_capture_success else 1
    lock_handle.close()
    return supervisor_returncode


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:  # noqa: BLE001 - CLI must persist a visible failure.
        print(f"pump research capture supervisor failed: {error}", file=sys.stderr)
        raise SystemExit(1)
