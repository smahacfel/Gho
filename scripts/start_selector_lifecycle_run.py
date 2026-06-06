#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

try:
    import check_selector_lifecycle_canary as lifecycle_canary
    import guard_restore_shadow_lifecycle as restore_guard
except ModuleNotFoundError:  # pragma: no cover - defensive for unusual import paths
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import check_selector_lifecycle_canary as lifecycle_canary
    import guard_restore_shadow_lifecycle as restore_guard


REPO_ROOT = Path(__file__).resolve().parents[1]
PASS_STATUS = "PASS"
FAIL_CONFIG_CONTRACT = "FAIL_CONFIG_CONTRACT"
FAIL_PREFLIGHT = "FAIL_PREFLIGHT"
FAIL_EVENT_CANARY = "FAIL_EVENT_CANARY"
FAIL_LIFECYCLE_PROOF = "FAIL_LIFECYCLE_PROOF"
FAIL_TMUX = "FAIL_TMUX"
INCONCLUSIVE_ENV_OR_CONFIG = "INCONCLUSIVE_ENV_OR_CONFIG"

RUN_STATE_RUNNING = "RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF"
RUN_STATE_KILLED = "RUN_KILLED_AFTER_FAILED_CANARY"


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def resolve_repo_path(root: Path, raw: Path) -> Path:
    return raw.resolve() if raw.is_absolute() else (root / raw).resolve()


def json_default(value: Any) -> Any:
    if isinstance(value, Path):
        return str(value)
    raise TypeError(f"cannot JSON encode {type(value).__name__}")


def exit_code_for_status(status: str) -> int:
    if status == PASS_STATUS:
        return 0
    if status == INCONCLUSIVE_ENV_OR_CONFIG:
        return 2
    return 1


def output_dir_default(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"run_lifecycle_guard_{utc_timestamp()}"


def free_gb(path: Path) -> float:
    usage = shutil.disk_usage(path)
    return usage.free / (1024**3)


def git_head(root: Path) -> str | None:
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return None
    value = proc.stdout.strip()
    return value or None


def mtime_utc(path: Path) -> str | None:
    if not path.exists():
        return None
    return datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()


def run_release_build_before_start(root: Path, output_dir: Path, launcher: Path) -> dict[str, Any]:
    started_at = datetime.now(timezone.utc)
    result = run_command(
        ["cargo", "build", "--release", "-p", "ghost-launcher"],
        cwd=root,
        log_path=output_dir / "commands" / "cargo_build_release_ghost_launcher.log",
    )
    finished_at = datetime.now(timezone.utc)
    # Cargo may legitimately no-op when the release binary is already up to date.
    # The freshness contract is that this launcher ran Cargo successfully before
    # start and the expected release binary exists; binary_mtime is provenance,
    # not a rebuild-required gate.
    build_fresh = result["exit_code"] == 0 and launcher.exists()
    return {
        "status": PASS_STATUS if build_fresh else INCONCLUSIVE_ENV_OR_CONFIG,
        "command": result["command"],
        "exit_code": result["exit_code"],
        "log_path": result["log_path"],
        "started_at_utc": started_at.isoformat(),
        "finished_at_utc": finished_at.isoformat(),
        "runtime_binary": str(launcher),
        "binary_exists": launcher.exists(),
        "binary_mtime_utc": mtime_utc(launcher),
        "git_head_at_build": git_head(root),
        "build_freshness_status": PASS_STATUS if build_fresh else "FAIL_STALE_OR_MISSING_BINARY",
    }


def validate_scope_contract(
    *,
    scope: str,
    config_path: Path,
    config: dict[str, Any],
    artifact_paths: restore_guard.ArtifactPaths,
) -> tuple[str, list[str]]:
    errors: list[str] = []
    raw = config_path.read_text(encoding="utf-8", errors="ignore")
    if scope not in raw:
        errors.append(f"scope {scope} not found in config text")
    for label, path in {
        "shadow_buys": artifact_paths.shadow_buys,
        "shadow_entries": artifact_paths.shadow_entries,
        "shadow_lifecycle": artifact_paths.shadow_lifecycle,
        "system_log": artifact_paths.system_log,
        "oracle_log": artifact_paths.oracle_log,
    }.items():
        if scope not in str(path):
            errors.append(f"{label} path does not contain scope {scope}: {path}")
    logging_cfg = config.get("logging", {})
    if logging_cfg.get("level") != "info":
        errors.append("logging.level must be info for DIAG/R2 lifecycle canaries")
    execution_cfg = config.get("execution", {})
    execution_mode = str(execution_cfg.get("execution_mode") or "").lower()
    if execution_mode != "shadow":
        errors.append("execution.execution_mode must be shadow")
    trigger_cfg = config.get("trigger", {})
    entry_mode = str(
        trigger_cfg.get("entry_mode") or config.get("entry_mode") or execution_cfg.get("entry_mode") or ""
    ).lower()
    if entry_mode != "shadow_only":
        errors.append("entry_mode must be shadow_only")
    return (PASS_STATUS if not errors else FAIL_CONFIG_CONTRACT), errors


def run_command(command: list[str], *, cwd: Path, log_path: Path) -> dict[str, Any]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8", errors="ignore") as log_fh:
        proc = subprocess.run(
            command,
            cwd=cwd,
            check=False,
            text=True,
            stdout=log_fh,
            stderr=subprocess.STDOUT,
        )
    return {
        "command": command,
        "exit_code": proc.returncode,
        "log_path": str(log_path),
    }


def tmux_session_exists(session: str) -> bool:
    proc = subprocess.run(
        ["tmux", "has-session", "-t", session],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return proc.returncode == 0


def kill_tmux_session(session: str) -> None:
    subprocess.run(["tmux", "kill-session", "-t", session], check=False)


def start_tmux_session(
    *,
    root: Path,
    session: str,
    launcher: Path,
    config_path: Path,
    runtime_log: Path,
) -> dict[str, Any]:
    runtime_log.parent.mkdir(parents=True, exist_ok=True)
    command = (
        f"cd {shlex.quote(str(root))} && "
        f"RUST_LOG=info {shlex.quote(str(launcher))} "
        f"--config {shlex.quote(str(config_path))} "
        f">> {shlex.quote(str(runtime_log))} 2>&1"
    )
    proc = subprocess.run(
        ["tmux", "new", "-d", "-s", session, command],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    return {
        "command": ["tmux", "new", "-d", "-s", session, command],
        "exit_code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "runtime_log": str(runtime_log),
    }


def run_canary_command(
    *,
    root: Path,
    scope: str,
    config_path: Path,
    baseline_path: Path,
    output_dir: Path,
    phase: str,
    min_reporter_rows: int,
) -> dict[str, Any]:
    command = [
        sys.executable,
        str(root / "scripts" / "check_selector_lifecycle_canary.py"),
        "--root",
        str(root),
        "--scope",
        scope,
        "--config",
        str(config_path),
        "--baseline",
        str(baseline_path),
        "--output-dir",
        str(output_dir),
        "--phase",
        phase,
        "--min-reporter-rows",
        str(min_reporter_rows),
        "--json",
    ]
    proc = subprocess.run(command, cwd=root, check=False, capture_output=True, text=True)
    payload: dict[str, Any] = {
        "command": command,
        "exit_code": proc.returncode,
        "stdout_bytes": len(proc.stdout.encode("utf-8", errors="ignore")),
        "stderr": proc.stderr[-4000:] if proc.stderr else "",
    }
    try:
        parsed = json.loads(proc.stdout)
        payload["json"] = {
            "status": parsed.get("status"),
            "claim": parsed.get("claim"),
            "artifacts": parsed.get("artifacts"),
            "errors": parsed.get("errors"),
            "event_canary": parsed.get("event_canary"),
            "lifecycle_canary": parsed.get("lifecycle_canary"),
            "reporter": parsed.get("reporter"),
        }
    except json.JSONDecodeError:
        payload["json"] = None
    return payload


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True, default=json_default) + "\n", encoding="utf-8")


def write_markdown(path: Path, payload: dict[str, Any]) -> None:
    lines = [
        "# Selector Lifecycle Run Launcher",
        "",
        f"- status: `{payload.get('status')}`",
        f"- claim: `{payload.get('claim')}`",
        f"- run_state: `{payload.get('run_state')}`",
        f"- scope: `{payload.get('scope')}`",
        f"- config: `{payload.get('config')}`",
        f"- tmux_session: `{payload.get('tmux_session')}`",
        "",
        "## Gates",
        "",
    ]
    for key in ("storage", "config_contract", "scope_contract", "static_guard", "preflight", "event_canary", "lifecycle_canary"):
        value = payload.get(key) or {}
        status = value.get("status") if isinstance(value, dict) else None
        lines.append(f"- {key}: `{status}`")
    lines.extend(
        [
            "",
            "## Runtime Binary",
            "",
            f"- runtime_binary: `{payload.get('runtime_binary')}`",
            f"- build_release_before_start: `{payload.get('build_release_before_start')}`",
            f"- build_freshness_status: `{payload.get('build_freshness_status')}`",
            f"- git_head_at_build: `{payload.get('git_head_at_build')}`",
            f"- git_head_at_launch: `{payload.get('git_head_at_launch')}`",
            f"- binary_mtime_utc: `{payload.get('binary_mtime_utc')}`",
        ]
    )
    errors = payload.get("errors") or []
    if errors:
        lines.extend(["", "## Errors", ""])
        for error in errors:
            lines.append(f"- {error}")
    lines.extend(
        [
            "",
            "## Procedure",
            "",
            "A selector lifecycle run is valid only after this launcher writes PASS.",
            "Manual tmux starts are not accepted for lifecycle-capable selector runs.",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def finish(report: dict[str, Any], output_dir: Path, status: str) -> int:
    report["status"] = status
    if status == PASS_STATUS:
        if report.get("run_state") == "DRY_RUN_NOT_STARTED":
            report["claim"] = "SELECTOR_LIFECYCLE_RUN_STATIC_PREFLIGHT_PASS"
        else:
            report["claim"] = "SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF"
    else:
        report["claim"] = f"SELECTOR_LIFECYCLE_RUN_START_FAIL:{status}"
    json_path = output_dir / "RUN_LIFECYCLE_LAUNCHER_REPORT.json"
    md_path = output_dir / "RUN_LIFECYCLE_LAUNCHER_REPORT.md"
    report["artifacts"]["json"] = str(json_path)
    report["artifacts"]["markdown"] = str(md_path)
    write_json(json_path, report)
    write_markdown(md_path, report)
    print(report["claim"])
    print(f"status={status} report={json_path}")
    return exit_code_for_status(status)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Start a selector dataset run only after lifecycle safety gates.")
    parser.add_argument("--root", type=Path, default=REPO_ROOT)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--tmux-session", required=True)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--launcher-binary", type=Path, default=Path("target/release/ghost-launcher"))
    parser.add_argument("--min-free-gb", type=float, default=35.0)
    parser.add_argument("--event-canary-seconds", type=int, default=900)
    parser.add_argument("--lifecycle-proof-timeout-seconds", type=int, default=3600)
    parser.add_argument("--lifecycle-poll-seconds", type=int, default=60)
    parser.add_argument("--min-reporter-rows", type=int, default=1)
    parser.add_argument("--allow-existing-session", action="store_true")
    parser.add_argument("--skip-static-tests", action="store_true")
    parser.add_argument(
        "--build-release-before-start",
        action="store_true",
        help="Run cargo build --release -p ghost-launcher before preflight/start and record binary provenance.",
    )
    parser.add_argument("--dry-run", action="store_true", help="Run static/preflight gates and snapshot only; do not start tmux")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = args.root.resolve()
    config_path = resolve_repo_path(root, args.config)
    launcher = resolve_repo_path(root, args.launcher_binary)
    output_dir = resolve_repo_path(root, args.output_dir) if args.output_dir else output_dir_default(root, args.scope)
    output_dir.mkdir(parents=True, exist_ok=True)
    baseline_path = output_dir / "baseline_before_start.json"
    runtime_log = output_dir / "runtime.log"
    report: dict[str, Any] = {
        "schema_version": 1,
        "guard": "selector_lifecycle_run_launcher",
        "status": "RUNNING",
        "claim": "SELECTOR_LIFECYCLE_RUN_START_INCOMPLETE",
        "run_state": "NOT_STARTED",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "scope": args.scope,
        "config": str(config_path),
        "tmux_session": args.tmux_session,
        "output_dir": str(output_dir),
        "runtime_binary": str(launcher),
        "build_release_before_start": args.build_release_before_start,
        "build_freshness_status": "NOT_REQUESTED",
        "git_head_at_build": None,
        "git_head_at_launch": None,
        "binary_mtime_utc": mtime_utc(launcher),
        "build_freshness": {},
        "storage": {},
        "config_contract": {},
        "scope_contract": {},
        "static_guard": {},
        "preflight": {},
        "baseline": str(baseline_path),
        "event_canary": {},
        "lifecycle_canary": {},
        "errors": [],
        "artifacts": {"runtime_log": str(runtime_log)},
    }

    current_free_gb = free_gb(root)
    report["storage"] = {
        "status": PASS_STATUS if current_free_gb >= args.min_free_gb else INCONCLUSIVE_ENV_OR_CONFIG,
        "free_gb": current_free_gb,
        "min_free_gb": args.min_free_gb,
    }
    if current_free_gb < args.min_free_gb:
        report["errors"].append(f"free_gb={current_free_gb:.2f} < min_free_gb={args.min_free_gb:.2f}")
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG)

    if args.build_release_before_start:
        build_report = run_release_build_before_start(root, output_dir, launcher)
        report["build_freshness"] = build_report
        report["build_freshness_status"] = build_report["build_freshness_status"]
        report["git_head_at_build"] = build_report.get("git_head_at_build")
        report["binary_mtime_utc"] = build_report.get("binary_mtime_utc")
        if build_report["status"] != PASS_STATUS:
            report["errors"].append("release build freshness check failed")
            return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG)

    if not launcher.exists():
        report["errors"].append(f"launcher binary missing: {launcher}")
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG)
    report["binary_mtime_utc"] = mtime_utc(launcher)
    if not args.build_release_before_start:
        report["build_freshness_status"] = "NOT_REQUESTED"

    try:
        resolved_config, artifact_paths = restore_guard.resolve_artifact_paths(root, config_path)
        config = restore_guard.load_toml(resolved_config)
    except Exception as exc:
        report["errors"].append(f"cannot load config/artifact paths: {exc}")
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG)

    config_status, config_errors = restore_guard.validate_shadow_run_config_contract(config)
    report["config_contract"] = {"status": config_status, "errors": config_errors}
    if config_status != PASS_STATUS:
        report["errors"].extend(config_errors)
        return finish(report, output_dir, config_status)

    scope_status, scope_errors = validate_scope_contract(
        scope=args.scope,
        config_path=resolved_config,
        config=config,
        artifact_paths=artifact_paths,
    )
    report["scope_contract"] = {"status": scope_status, "errors": scope_errors}
    if scope_status != PASS_STATUS:
        report["errors"].extend(scope_errors)
        return finish(report, output_dir, scope_status)

    static_guard_cmd = [
        sys.executable,
        str(root / "scripts" / "guard_restore_shadow_lifecycle.py"),
        "--root",
        str(root),
        "--config",
        str(resolved_config),
        "--skip-runtime",
        "--output-dir",
        str(output_dir / "static_guard"),
        "--json",
    ]
    if args.skip_static_tests:
        static_guard_cmd.insert(static_guard_cmd.index("--skip-runtime"), "--skip-tests")
    proc = subprocess.run(static_guard_cmd, cwd=root, check=False, capture_output=True, text=True)
    report["static_guard"] = {
        "status": PASS_STATUS if proc.returncode == 0 else "FAIL",
        "exit_code": proc.returncode,
        "command": static_guard_cmd,
        "stdout_bytes": len(proc.stdout.encode("utf-8", errors="ignore")),
        "stderr": proc.stderr[-4000:] if proc.stderr else "",
    }
    if proc.returncode != 0:
        report["errors"].append("static guard failed")
        return finish(report, output_dir, FAIL_CONFIG_CONTRACT)

    preflight_cmd = [
        "cargo",
        "run",
        "-p",
        "ghost-launcher",
        "--bin",
        "ghost-launcher",
        "--",
        "--config",
        str(resolved_config),
        "--preflight",
    ]
    preflight = run_command(preflight_cmd, cwd=root, log_path=output_dir / "commands" / "preflight.log")
    report["preflight"] = {
        "status": PASS_STATUS if preflight["exit_code"] == 0 else "FAIL",
        **preflight,
    }
    if preflight["exit_code"] != 0:
        report["errors"].append(f"preflight exit_code={preflight['exit_code']}")
        return finish(report, output_dir, FAIL_PREFLIGHT)

    baseline = lifecycle_canary.build_snapshot(root, args.scope, resolved_config)
    write_json(baseline_path, baseline)
    report["baseline_snapshot_summary"] = {
        "event_counts": baseline.get("event_counts"),
        "bad_event_json": baseline.get("bad_event_json"),
        "shadow_buys_lines": baseline.get("shadow_buys_lines"),
        "shadow_entries_lines": baseline.get("shadow_entries_lines"),
        "shadow_lifecycle_lines": baseline.get("shadow_lifecycle_lines"),
        "log_file_count": len(baseline.get("log_sizes") or {}),
    }

    if args.dry_run:
        report["run_state"] = "DRY_RUN_NOT_STARTED"
        return finish(report, output_dir, PASS_STATUS)

    if tmux_session_exists(args.tmux_session) and not args.allow_existing_session:
        report["errors"].append(f"tmux session already exists: {args.tmux_session}")
        return finish(report, output_dir, FAIL_TMUX)

    if tmux_session_exists(args.tmux_session) and args.allow_existing_session:
        kill_tmux_session(args.tmux_session)

    report["git_head_at_launch"] = git_head(root)
    if args.build_release_before_start and report.get("git_head_at_build") != report.get("git_head_at_launch"):
        report["errors"].append("git HEAD changed between release build and launch")
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG)

    start = start_tmux_session(
        root=root,
        session=args.tmux_session,
        launcher=launcher,
        config_path=resolved_config,
        runtime_log=runtime_log,
    )
    report["tmux_start"] = start
    if start["exit_code"] != 0:
        report["errors"].append(start.get("stderr") or "tmux start failed")
        return finish(report, output_dir, FAIL_TMUX)
    report["run_state"] = "RUNNING_AWAITING_EVENT_CANARY"

    event_eta = datetime.now(timezone.utc) + timedelta(seconds=args.event_canary_seconds)
    print(
        "selector lifecycle run started; "
        f"event canary at {event_eta.isoformat()} UTC; "
        f"session={args.tmux_session}"
    )
    time.sleep(args.event_canary_seconds)

    event_output_dir = output_dir / "event_canary"
    event_result = run_canary_command(
        root=root,
        scope=args.scope,
        config_path=resolved_config,
        baseline_path=baseline_path,
        output_dir=event_output_dir,
        phase="event",
        min_reporter_rows=args.min_reporter_rows,
    )
    report["event_canary"] = event_result
    if event_result["exit_code"] != 0:
        kill_tmux_session(args.tmux_session)
        report["run_state"] = RUN_STATE_KILLED
        report["errors"].append("event canary failed")
        return finish(report, output_dir, FAIL_EVENT_CANARY)

    report["run_state"] = "RUNNING_AWAITING_LIFECYCLE_PROOF"
    deadline = time.monotonic() + args.lifecycle_proof_timeout_seconds
    while time.monotonic() < deadline:
        lifecycle_output_dir = output_dir / "lifecycle_canary"
        lifecycle_result = run_canary_command(
            root=root,
            scope=args.scope,
            config_path=resolved_config,
            baseline_path=baseline_path,
            output_dir=lifecycle_output_dir,
            phase="lifecycle",
            min_reporter_rows=args.min_reporter_rows,
        )
        report["lifecycle_canary"] = lifecycle_result
        if lifecycle_result["exit_code"] == 0:
            report["run_state"] = RUN_STATE_RUNNING
            return finish(report, output_dir, PASS_STATUS)
        time.sleep(max(1, args.lifecycle_poll_seconds))

    kill_tmux_session(args.tmux_session)
    report["run_state"] = RUN_STATE_KILLED
    report["errors"].append("lifecycle proof timeout expired")
    return finish(report, output_dir, FAIL_LIFECYCLE_PROOF)


if __name__ == "__main__":
    raise SystemExit(main())
