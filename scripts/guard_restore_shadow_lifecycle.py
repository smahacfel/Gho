#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
import re
import subprocess
import sys
from collections import Counter, deque
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

try:
    from shadow_run_report import load_toml, resolve_runtime_path
except ModuleNotFoundError:  # pragma: no cover - defensive for unusual import paths
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from shadow_run_report import load_toml, resolve_runtime_path


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = Path("configs/rollout/shadow-burnin.toml")
GUARD_NAME = "restore_shadow_lifecycle"
PASS_STATUS = "PASS"
NOT_REQUIRED_STATUS = "RESTORE_GUARD_NOT_REQUIRED_FOR_THIS_DIFF"
FAIL_TESTS = "FAIL_TESTS"
FAIL_PREFLIGHT = "FAIL_PREFLIGHT"
FAIL_CONFIG_CONTRACT = "FAIL_CONFIG_CONTRACT"
FAIL_RUNTIME_REQUIRED = "FAIL_RUNTIME_REQUIRED"
FAIL_RUNTIME_SMOKE = "FAIL_RUNTIME_SMOKE"
FAIL_RUNTIME_ARTIFACTS = "FAIL_RUNTIME_ARTIFACTS"
FAIL_REPORTER_NO_ROWS = "FAIL_REPORTER_NO_ROWS"
FAIL_REPORTER_TRUTH = "FAIL_REPORTER_TRUTH"
INCONCLUSIVE_ENV_OR_CONFIG = "INCONCLUSIVE_ENV_OR_CONFIG"

CRITICAL_RESTORE_FILES = [
    "configs/rollout/shadow-burnin.toml",
    "off-chain/components/seer/src/types.rs",
    "off-chain/components/seer/src/binary_parser.rs",
    "ghost-launcher/src/events.rs",
    "ghost-launcher/src/components/seer.rs",
    "ghost-launcher/src/oracle_runtime.rs",
    "ghost-launcher/src/components/trigger/component.rs",
    "ghost-launcher/src/components/trigger/shadow_run.rs",
    "off-chain/components/trigger/src/direct_buy_builder.rs",
    "off-chain/components/trigger/src/lib.rs",
    "scripts/shadow_run_report.py",
    "scripts/shadow_onchain_lifecycle_report.py",
    "scripts/check_selector_lifecycle_canary.py",
    "scripts/start_selector_lifecycle_run.py",
]

CRITICAL_RESTORE_FILE_PATTERNS = [
    "configs/rollout/shadow-burnin-v3-selector-dataset-*.toml",
]

TARGETED_TEST_COMMANDS = [
    ["cargo", "test", "-q", "-p", "ghost-launcher", "--lib", "restore_legacy_buy"],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "legacy_buy_missing_remaining_accounts_is_not_executable",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "telemetry_only_pool_transaction_cannot_unlock_legacy_buy",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "routed_exact_sol_in_missing_bcv2_still_not_executable",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "p37_route_resolver_primary_bcv2_missing_selects_validated_legacy_fallback",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "p37_route_resolver_primary_bcv2_manifest_missing_selects_validated_legacy_fallback_without_precheck_reason",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "--lib",
        "selected_legacy_buy_handoff_happens_before_precheck",
    ],
    [
        "cargo",
        "test",
        "-q",
        "-p",
        "ghost-launcher",
        "components::trigger::shadow_run::tests::p5_precheck_failure_writes_not_dispatched_lifecycle_record",
    ],
]

BAD_RUNTIME_MARKERS = {
    "ResourceExhausted": "ResourceExhausted",
    "relative URL without a base": "relative URL without a base",
    "Custom(6062)": "Custom(6062)",
    "custom program error: 0x17ae": "custom program error: 0x17ae",
    "0x17ae": "0x17ae",
    "unsupported_legacy_buy_layout_requires_bcv2": "unsupported_legacy_buy_layout_requires_bcv2",
}
GOOD_RUNTIME_MARKERS = {
    "buy_remaining_account_count=2": "buy_remaining_account_count=2",
    "DIAG_ACCOUNT_UPDATE_RELAY": "DIAG_ACCOUNT_UPDATE_RELAY",
}
ACCEPTED_CLOSE_REASONS = {"Target", "StopLoss", "TimeStop"}
NON_CLAIMS = [
    "production_readiness",
    "live_execution",
    "market_recall",
    "Gatekeeper_tuning",
    "FSC_policy",
    "NLN_raw_capture",
]


@dataclass
class CommandResult:
    command: list[str]
    exit_code: int | None
    status: str
    log_path: str
    error: str | None = None


@dataclass
class ArtifactPaths:
    shadow_buys: Path
    shadow_entries: Path
    shadow_lifecycle: Path
    system_log: Path
    oracle_log: Path


@dataclass
class RuntimeSnapshots:
    shadow_buys_lines: int
    shadow_entries_lines: int
    shadow_lifecycle_lines: int
    log_sizes: dict[str, int]


@dataclass
class ReporterValidation:
    status: str
    rows_written: int
    close_truth_coverage: str
    truth_status_resolved_rows: int
    truth_source_canonical_rows: int
    gatekeeper_buy_context_found_rows: int
    final_pnl_pct_present_rows: int
    exit_fills_total: int
    accepted_close_reason_rows: int
    errors: list[str]


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def normalize_repo_path(path: str) -> str:
    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def guard_required_for_changed_files(changed_files: Iterable[str]) -> tuple[bool, list[str]]:
    critical = set(CRITICAL_RESTORE_FILES)
    touched = [
        normalize_repo_path(path)
        for path in changed_files
        if normalize_repo_path(path) in critical
        or any(
            fnmatch.fnmatch(normalize_repo_path(path), pattern)
            for pattern in CRITICAL_RESTORE_FILE_PATTERNS
        )
    ]
    return bool(touched), sorted(set(touched))


def changed_files_from_git_diff(root: Path, diff_spec: str) -> list[str]:
    proc = subprocess.run(
        ["git", "diff", "--name-only", diff_spec],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or f"git diff failed for {diff_spec}")
    return [line.strip() for line in proc.stdout.splitlines() if line.strip()]


def exit_code_for_status(status: str) -> int:
    if status in {PASS_STATUS, NOT_REQUIRED_STATUS}:
        return 0
    if status == INCONCLUSIVE_ENV_OR_CONFIG:
        return 2
    return 1


def classify_preflight_failure(text: str) -> str:
    lowered = text.lower()
    env_patterns = (
        "missing env",
        "environment variable",
        "required env",
        "env var",
        "rpc",
        "grpc",
        "getversion",
        "provider",
        "connection refused",
        "connection reset",
        "timed out",
        "timeout",
        "tls",
        "dns",
        "resolve",
        "no such host",
        "insufficient disk",
        "no space left",
        "disk",
        "balance",
        "insufficient",
        "keypair",
        "secret",
        "metrics.port",
        "port",
    )
    if any(pattern in lowered for pattern in env_patterns):
        return INCONCLUSIVE_ENV_OR_CONFIG
    return FAIL_PREFLIGHT


def validate_shadow_run_config_contract(config: dict[str, Any]) -> tuple[str, list[str]]:
    trigger_shadow = config.get("trigger", {}).get("shadow_run", {})
    if trigger_shadow.get("enabled") is not True:
        return PASS_STATUS, []

    errors: list[str] = []
    payer_strategy = trigger_shadow.get("payer_strategy")
    if payer_strategy != "configured":
        errors.append(
            "trigger.shadow_run.payer_strategy must be configured for lifecycle-capable shadow simulation"
        )

    timeout_ms = trigger_shadow.get("timeout_ms")
    if not isinstance(timeout_ms, int) or timeout_ms < 5000:
        errors.append("trigger.shadow_run.timeout_ms must be >= 5000 for lifecycle guard configs")

    max_concurrent = trigger_shadow.get("max_concurrent")
    if not isinstance(max_concurrent, int) or max_concurrent > 1:
        errors.append("trigger.shadow_run.max_concurrent must be <= 1 for lifecycle guard configs")

    return (PASS_STATUS if not errors else FAIL_CONFIG_CONTRACT), errors


def resolve_repo_path(root: Path, raw: Path) -> Path:
    return raw.resolve() if raw.is_absolute() else (root / raw).resolve()


def resolve_artifact_paths(root: Path, config_arg: Path) -> tuple[Path, ArtifactPaths]:
    config_path = resolve_repo_path(root, config_arg)
    config = load_toml(config_path)
    shadow_cfg = config.get("execution", {}).get("shadow", {})
    logging_cfg = config.get("logging", {})
    shadow_buys = resolve_runtime_path(
        config_path,
        config.get("trigger", {})
        .get("shadow_run", {})
        .get("output_path", "logs/shadow_run/buys.jsonl"),
    )
    shadow_entries = resolve_runtime_path(
        config_path,
        shadow_cfg.get("entry_log_path", "logs/shadow_run/shadow_entries.jsonl"),
    )
    shadow_lifecycle = resolve_runtime_path(
        config_path,
        shadow_cfg.get("lifecycle_log_path")
        or str(shadow_entries.with_name("shadow_lifecycle.jsonl")),
    )
    system_log = resolve_runtime_path(
        config_path,
        logging_cfg.get("file_path", "logs/system.log"),
    )
    oracle_log = resolve_runtime_path(
        config_path,
        logging_cfg.get("oracle_log_path", "logs/oracle.log"),
    )
    return config_path, ArtifactPaths(
        shadow_buys=shadow_buys,
        shadow_entries=shadow_entries,
        shadow_lifecycle=shadow_lifecycle,
        system_log=system_log,
        oracle_log=oracle_log,
    )


def count_lines(path: Path) -> int:
    if not path.exists():
        return 0
    with path.open("rb") as fh:
        return sum(1 for _ in fh)


def log_family(base_path: Path) -> list[Path]:
    if not base_path.parent.exists():
        return [base_path]
    paths = [path for path in base_path.parent.glob(base_path.name + "*") if path.is_file()]
    if base_path not in paths:
        paths.append(base_path)
    return sorted(set(paths))


def snapshot_runtime(paths: ArtifactPaths) -> RuntimeSnapshots:
    log_sizes: dict[str, int] = {}
    for path in [*log_family(paths.system_log), *log_family(paths.oracle_log)]:
        log_sizes[str(path)] = path.stat().st_size if path.exists() else 0
    return RuntimeSnapshots(
        shadow_buys_lines=count_lines(paths.shadow_buys),
        shadow_entries_lines=count_lines(paths.shadow_entries),
        shadow_lifecycle_lines=count_lines(paths.shadow_lifecycle),
        log_sizes=log_sizes,
    )


def read_appended_log_text(paths: ArtifactPaths, before: RuntimeSnapshots) -> str:
    chunks: list[str] = []
    for path in [*log_family(paths.system_log), *log_family(paths.oracle_log)]:
        if not path.exists():
            continue
        previous_size = before.log_sizes.get(str(path), 0)
        current_size = path.stat().st_size
        if current_size <= previous_size:
            continue
        with path.open("rb") as fh:
            fh.seek(previous_size)
            chunks.append(fh.read().decode("utf-8", errors="ignore"))
    return "\n".join(chunks)


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                try:
                    value, _ = decoder.raw_decode(line)
                except json.JSONDecodeError:
                    continue
            if isinstance(value, dict):
                yield value


def tail_jsonl(path: Path, count: int) -> list[dict[str, Any]]:
    if count <= 0 or not path.exists():
        return []
    rows: deque[dict[str, Any]] = deque(maxlen=count)
    for row in iter_jsonl(path):
        rows.append(row)
    return list(rows)


def row_string(row: dict[str, Any], key: str) -> str:
    value = row.get(key)
    return value if isinstance(value, str) else ""


def row_has_reason(row: dict[str, Any], needle: str) -> bool:
    keys = (
        "no_executable_route_account_set_reason",
        "precheck_failure_reason",
        "route_account_contract_failure_reason",
        "simulation_error_message",
        "execution_feasibility_reason",
        "err",
        "error",
    )
    return any(needle in row_string(row, key) for key in keys)


def summarize_lifecycle_rows(rows: list[dict[str, Any]]) -> dict[str, Any]:
    selected_route_kind = Counter(row.get("selected_route_kind") for row in rows)
    primary_route_kind = Counter(row.get("primary_route_kind") for row in rows)
    fallback_route_kind = Counter(row.get("fallback_route_kind") for row in rows)
    route_resolution_status = Counter(row.get("route_resolution_status") for row in rows)
    execution_feasibility_status = Counter(row.get("execution_feasibility_status") for row in rows)
    legacy_account_status = Counter(row.get("legacy_buy_account_set_status") for row in rows)
    dispatch_status = Counter(row.get("dispatch_status") or row.get("shadow_dispatch_status") for row in rows)
    simulation_outcome = Counter(row.get("simulation_outcome") for row in rows)

    legacy_buy_rows = sum(
        1
        for row in rows
        if row.get("selected_route_kind") == "legacy_buy"
        or row.get("primary_route_kind") == "legacy_buy"
        or row.get("fallback_route_kind") == "legacy_buy"
        or row.get("legacy_buy_account_set_status") in {"ready", "not_ready"}
    )
    legacy_buy_complete_rows = sum(1 for row in rows if row.get("legacy_buy_account_set_status") == "ready")
    legacy_buy_executable_rows = sum(
        1
        for row in rows
        if row.get("selected_route_kind") == "legacy_buy"
        and row.get("execution_feasibility_status") == "executable"
    )
    return {
        "rows": len(rows),
        "legacy_buy_rows": legacy_buy_rows,
        "legacy_buy_complete_rows": legacy_buy_complete_rows,
        "legacy_buy_executable_rows": legacy_buy_executable_rows,
        "unsupported_legacy_buy_layout_requires_bcv2_rows": sum(
            1 for row in rows if row_has_reason(row, "unsupported_legacy_buy_layout_requires_bcv2")
        ),
        "fallback_route_ready_rows": route_resolution_status.get("fallback_route_ready", 0),
        "dispatch_attempted_rows": sum(1 for row in rows if row.get("dispatch_attempted") is True),
        "simulation_attempted_rows": sum(1 for row in rows if row.get("simulation_attempted") is True),
        "selected_route_kind_counts": counter_to_json(selected_route_kind),
        "primary_route_kind_counts": counter_to_json(primary_route_kind),
        "fallback_route_kind_counts": counter_to_json(fallback_route_kind),
        "route_resolution_status_counts": counter_to_json(route_resolution_status),
        "execution_feasibility_status_counts": counter_to_json(execution_feasibility_status),
        "legacy_buy_account_set_status_counts": counter_to_json(legacy_account_status),
        "dispatch_status_counts": counter_to_json(dispatch_status),
        "simulation_outcome_counts": counter_to_json(simulation_outcome),
    }


def counter_to_json(counter: Counter[Any]) -> dict[str, int]:
    return {str(key) if key is not None else "null": count for key, count in sorted(counter.items(), key=lambda item: str(item[0]))}


def count_markers(text: str) -> dict[str, int]:
    markers = {**BAD_RUNTIME_MARKERS, **GOOD_RUNTIME_MARKERS}
    return {name: text.count(pattern) for name, pattern in markers.items()}


def validate_runtime_artifacts(
    artifact_deltas: dict[str, int],
    marker_counts: dict[str, int],
    lifecycle_matrix: dict[str, Any],
) -> tuple[str, list[str]]:
    errors: list[str] = []
    if artifact_deltas.get("shadow_buys_delta", 0) <= 0:
        errors.append("shadow_buys_delta <= 0")
    if artifact_deltas.get("shadow_entries_delta", 0) <= 0:
        errors.append("shadow_entries_delta <= 0")
    if artifact_deltas.get("shadow_lifecycle_delta", 0) <= 0:
        errors.append("shadow_lifecycle_delta <= 0")
    if artifact_deltas.get("diag_account_update_relay_delta", 0) <= 0:
        errors.append("DIAG_ACCOUNT_UPDATE_RELAY_delta <= 0")
    for marker in BAD_RUNTIME_MARKERS:
        if marker_counts.get(marker, 0) > 0:
            errors.append(f"{marker} > 0")
    if marker_counts.get("buy_remaining_account_count=2", 0) <= 0:
        errors.append("buy_remaining_account_count=2 <= 0")
    if lifecycle_matrix.get("legacy_buy_executable_rows", 0) <= 0:
        errors.append("selected_route_kind=legacy_buy executable rows <= 0")
    if lifecycle_matrix.get("dispatch_attempted_rows", 0) <= 0:
        errors.append("dispatch_attempted rows <= 0")
    if lifecycle_matrix.get("simulation_attempted_rows", 0) <= 0:
        errors.append("simulation_attempted rows <= 0")
    if lifecycle_matrix.get("unsupported_legacy_buy_layout_requires_bcv2_rows", 0) > 0:
        errors.append("unsupported_legacy_buy_layout_requires_bcv2 lifecycle rows > 0")
    return (PASS_STATUS if not errors else FAIL_RUNTIME_ARTIFACTS), errors


def nested_value(row: dict[str, Any], *keys: str) -> Any:
    value: Any = row
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def final_pnl_present(row: dict[str, Any]) -> bool:
    return row.get("final_pnl_pct") is not None or nested_value(row, "shadow", "final_pnl_pct") is not None


def exit_fills_len(row: dict[str, Any]) -> int:
    fills = row.get("exit_fills")
    if isinstance(fills, list):
        return len(fills)
    value = row.get("exit_fills_len")
    return int(value) if isinstance(value, int) else 0


def parse_close_truth_coverage(text: str, rows_written: int, resolved_rows: int) -> str:
    match = re.search(r"close_truth_coverage=([0-9]+/[0-9]+)", text)
    if match:
        return match.group(1)
    return f"{resolved_rows}/{rows_written}"


def validate_reporter_rows(
    rows: list[dict[str, Any]],
    *,
    min_rows_written: int,
    require_resolved: bool,
    reporter_stdout: str = "",
) -> ReporterValidation:
    rows_written = len(rows)
    truth_status_resolved_rows = sum(1 for row in rows if row.get("truth_status") == "resolved")
    truth_source_canonical_rows = sum(
        1 for row in rows if row.get("truth_source") == "canonical_account_state_snapshot"
    )
    gatekeeper_buy_context_found_rows = sum(
        1 for row in rows if nested_value(row, "timing", "gatekeeper_buy_context_found") is True
    )
    final_pnl_pct_present_rows = sum(1 for row in rows if final_pnl_present(row))
    exit_fills_total = sum(exit_fills_len(row) for row in rows)
    accepted_close_reason_rows = sum(1 for row in rows if row.get("close_reason") in ACCEPTED_CLOSE_REASONS)
    errors: list[str] = []
    status = PASS_STATUS
    if rows_written < min_rows_written:
        status = FAIL_REPORTER_NO_ROWS
        errors.append(f"rows_written={rows_written} < min_rows_written={min_rows_written}")
    if require_resolved:
        if truth_status_resolved_rows <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("truth_status=resolved rows <= 0")
        if truth_source_canonical_rows <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("truth_source=canonical_account_state_snapshot rows <= 0")
        if gatekeeper_buy_context_found_rows <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("gatekeeper_buy_context_found=true rows <= 0")
        if final_pnl_pct_present_rows <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("final_pnl_pct present rows <= 0")
        if exit_fills_total <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("exit_fills total <= 0")
        if accepted_close_reason_rows <= 0:
            status = FAIL_REPORTER_TRUTH if status == PASS_STATUS else status
            errors.append("accepted close_reason rows <= 0")
    return ReporterValidation(
        status=status,
        rows_written=rows_written,
        close_truth_coverage=parse_close_truth_coverage(
            reporter_stdout,
            rows_written,
            truth_status_resolved_rows,
        ),
        truth_status_resolved_rows=truth_status_resolved_rows,
        truth_source_canonical_rows=truth_source_canonical_rows,
        gatekeeper_buy_context_found_rows=gatekeeper_buy_context_found_rows,
        final_pnl_pct_present_rows=final_pnl_pct_present_rows,
        exit_fills_total=exit_fills_total,
        accepted_close_reason_rows=accepted_close_reason_rows,
        errors=errors,
    )


def run_command_to_log(command: list[str], *, cwd: Path, log_path: Path) -> CommandResult:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with log_path.open("w", encoding="utf-8", errors="ignore") as log_fh:
            proc = subprocess.Popen(
                command,
                cwd=cwd,
                stdout=log_fh,
                stderr=subprocess.STDOUT,
                text=True,
            )
            exit_code = proc.wait()
    except OSError as exc:
        log_path.write_text(str(exc), encoding="utf-8")
        return CommandResult(
            command=command,
            exit_code=None,
            status=INCONCLUSIVE_ENV_OR_CONFIG,
            log_path=str(log_path),
            error=str(exc),
        )
    return CommandResult(
        command=command,
        exit_code=exit_code,
        status=PASS_STATUS if exit_code == 0 else "FAIL",
        log_path=str(log_path),
    )


def read_text(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="ignore")


def git_head(root: Path) -> str | None:
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    return proc.stdout.strip() if proc.returncode == 0 else None


def default_output_dir(root: Path) -> Path:
    return root / "reports" / "selector" / f"restore_lifecycle_guard_{utc_timestamp()}"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Restore shadow lifecycle regression guard.")
    parser.add_argument("--root", type=Path, default=REPO_ROOT, help=f"Repository root (default: {REPO_ROOT})")
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Restore rollout config (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument("--timeout-seconds", type=int, default=600)
    parser.add_argument("--output-dir", type=Path, help="Output directory for guard artifacts")
    parser.add_argument("--json", action="store_true", help="Print JSON report to stdout")
    parser.add_argument("--skip-runtime", action="store_true", help="Run tests/static guard only")
    parser.add_argument("--skip-tests", action="store_true", help="Run runtime smoke/reporter only")
    parser.add_argument("--min-rows-written", type=int, default=1)
    parser.add_argument("--require-resolved", action="store_true", default=True)
    parser.add_argument(
        "--allow-unresolved",
        dest="require_resolved",
        action="store_false",
        help="Do not require resolved truth rows in reporter output",
    )
    parser.add_argument(
        "--changed-files-from-git-diff",
        help="Diff spec such as origin/main...HEAD. Non-critical diffs can skip the guard.",
    )
    return parser


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_markdown(path: Path, payload: dict[str, Any]) -> None:
    lines = [
        "# Restore Shadow Lifecycle Guard",
        "",
        f"- status: `{payload.get('status')}`",
        f"- claim: `{payload.get('claim')}`",
        f"- head: `{payload.get('head')}`",
        f"- config: `{payload.get('config')}`",
        f"- output_dir: `{payload.get('output_dir')}`",
        "",
        "## Tests",
        "",
        f"- status: `{payload.get('tests', {}).get('status')}`",
        f"- commands: `{len(payload.get('tests', {}).get('commands', []))}`",
        "",
        "## Config Contract",
        "",
        f"- status: `{payload.get('config_contract', {}).get('status')}`",
        "",
        "## Runtime Smoke",
        "",
        f"- preflight: `{payload.get('preflight', {}).get('status')}`",
        f"- runtime: `{payload.get('runtime_smoke', {}).get('status')}`",
        f"- timeout_seconds: `{payload.get('runtime_smoke', {}).get('timeout_seconds')}`",
        f"- exit_code: `{payload.get('runtime_smoke', {}).get('exit_code')}`",
        "",
        "## Artifact Deltas",
        "",
    ]
    for key, value in (payload.get("artifact_deltas") or {}).items():
        lines.append(f"- {key}: `{value}`")
    lines.extend(["", "## Legacy Contract Matrix", ""])
    for key, value in (payload.get("legacy_contract_matrix") or {}).items():
        if isinstance(value, dict):
            continue
        lines.append(f"- {key}: `{value}`")
    lines.extend(["", "## Reporter", ""])
    for key, value in (payload.get("reporter") or {}).items():
        if isinstance(value, (dict, list)):
            continue
        lines.append(f"- {key}: `{value}`")
    errors = payload.get("errors") or []
    if errors:
        lines.extend(["", "## Errors", ""])
        for error in errors:
            lines.append(f"- {error}")
    lines.extend(["", "## Non-Claims", ""])
    for item in payload.get("non_claims", []):
        lines.append(f"- `{item}`")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def base_report(args: argparse.Namespace, root: Path, config_path: Path, output_dir: Path) -> dict[str, Any]:
    return {
        "guard": GUARD_NAME,
        "status": "RUNNING",
        "head": git_head(root),
        "config": str(config_path.relative_to(root) if config_path.is_relative_to(root) else config_path),
        "output_dir": str(output_dir),
        "tests": {"status": "SKIPPED", "commands": []},
        "critical_files": {
            "diff_spec": args.changed_files_from_git_diff,
            "guard_required": None,
            "changed_critical_files": [],
        },
        "config_contract": {"status": "PENDING", "errors": []},
        "preflight": {"status": "SKIPPED", "exit_code": None},
        "runtime_smoke": {"status": "SKIPPED", "timeout_seconds": args.timeout_seconds, "exit_code": None},
        "artifact_deltas": {},
        "legacy_contract_matrix": {},
        "reporter": {},
        "claim": "RESTORE_PATH_GUARD_INCOMPLETE",
        "non_claims": NON_CLAIMS,
        "errors": [],
        "artifacts": {},
    }


def finish(report: dict[str, Any], output_dir: Path, status: str, *, json_stdout: bool) -> int:
    report["status"] = status
    if status == PASS_STATUS:
        report["claim"] = (
            "RESTORE_PATH_STATIC_GUARD_PASS"
            if report.get("runtime_smoke", {}).get("status") == "SKIPPED"
            else "RESTORE_PATH_GUARD_PASS"
        )
    elif status == NOT_REQUIRED_STATUS:
        report["claim"] = NOT_REQUIRED_STATUS
    else:
        report["claim"] = f"RESTORE_GUARD_FAIL:{status}"
    json_path = output_dir / "restore_lifecycle_guard_v1.json"
    md_path = output_dir / "RESTORE_LIFECYCLE_GUARD.md"
    report["artifacts"]["json"] = str(json_path)
    report["artifacts"]["markdown"] = str(md_path)
    write_json(json_path, report)
    write_markdown(md_path, report)
    if json_stdout:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    else:
        print(report["claim"])
        print(f"status={status} report={json_path}")
    return exit_code_for_status(status)


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    root = args.root.resolve()
    output_dir = resolve_repo_path(root, args.output_dir) if args.output_dir else default_output_dir(root)
    output_dir.mkdir(parents=True, exist_ok=True)

    try:
        config_path, artifact_paths = resolve_artifact_paths(root, args.config)
    except Exception as exc:
        report = {
            "guard": GUARD_NAME,
            "status": INCONCLUSIVE_ENV_OR_CONFIG,
            "head": git_head(root),
            "config": str(args.config),
            "output_dir": str(output_dir),
            "tests": {"status": "SKIPPED", "commands": []},
            "preflight": {"status": "SKIPPED", "exit_code": None},
            "runtime_smoke": {"status": "SKIPPED", "timeout_seconds": args.timeout_seconds, "exit_code": None},
            "artifact_deltas": {},
            "legacy_contract_matrix": {},
            "reporter": {},
            "claim": "RESTORE_GUARD_FAIL:INCONCLUSIVE_ENV_OR_CONFIG",
            "non_claims": NON_CLAIMS,
            "errors": [f"cannot resolve config/artifacts: {exc}"],
            "artifacts": {},
        }
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG, json_stdout=args.json)

    report = base_report(args, root, config_path, output_dir)
    report["artifacts"].update(
        {
            "shadow_buys": str(artifact_paths.shadow_buys),
            "shadow_entries": str(artifact_paths.shadow_entries),
            "shadow_lifecycle": str(artifact_paths.shadow_lifecycle),
            "system_log": str(artifact_paths.system_log),
            "oracle_log": str(artifact_paths.oracle_log),
        }
    )

    try:
        config_contract_status, config_contract_errors = validate_shadow_run_config_contract(
            load_toml(config_path)
        )
    except Exception as exc:
        report["config_contract"] = {
            "status": INCONCLUSIVE_ENV_OR_CONFIG,
            "errors": [f"cannot validate shadow_run config contract: {exc}"],
        }
        report["errors"].extend(report["config_contract"]["errors"])
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG, json_stdout=args.json)

    report["config_contract"] = {
        "status": config_contract_status,
        "errors": config_contract_errors,
    }
    if config_contract_status != PASS_STATUS:
        report["errors"].extend(config_contract_errors)
        return finish(report, output_dir, config_contract_status, json_stdout=args.json)

    if args.changed_files_from_git_diff:
        try:
            changed = changed_files_from_git_diff(root, args.changed_files_from_git_diff)
        except RuntimeError as exc:
            report["errors"].append(str(exc))
            return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG, json_stdout=args.json)
        required, changed_critical = guard_required_for_changed_files(changed)
        report["critical_files"].update(
            {
                "changed_files": changed,
                "guard_required": required,
                "changed_critical_files": changed_critical,
            }
        )
        if not required:
            return finish(report, output_dir, NOT_REQUIRED_STATUS, json_stdout=args.json)
        if args.skip_runtime:
            report["errors"].append(
                "critical restore files changed but --skip-runtime was requested"
            )
            return finish(report, output_dir, FAIL_RUNTIME_REQUIRED, json_stdout=args.json)

    if not args.skip_tests:
        test_results: list[dict[str, Any]] = []
        for idx, command in enumerate(TARGETED_TEST_COMMANDS, start=1):
            result = run_command_to_log(
                command,
                cwd=root,
                log_path=output_dir / "commands" / f"test_{idx:02d}.log",
            )
            test_results.append(asdict(result))
            if result.exit_code != 0:
                report["tests"] = {"status": FAIL_TESTS, "commands": test_results}
                report["errors"].append(f"targeted test failed: {' '.join(command)}")
                return finish(report, output_dir, FAIL_TESTS, json_stdout=args.json)
        report["tests"] = {"status": PASS_STATUS, "commands": test_results}

    if args.skip_runtime:
        return finish(report, output_dir, PASS_STATUS, json_stdout=args.json)

    preflight_cmd = [
        "cargo",
        "run",
        "-p",
        "ghost-launcher",
        "--bin",
        "ghost-launcher",
        "--",
        "--config",
        str(config_path),
        "--preflight",
    ]
    preflight_result = run_command_to_log(
        preflight_cmd,
        cwd=root,
        log_path=output_dir / "commands" / "preflight.log",
    )
    report["preflight"] = {
        "status": PASS_STATUS if preflight_result.exit_code == 0 else "FAIL",
        "exit_code": preflight_result.exit_code,
        "command": preflight_cmd,
        "log_path": preflight_result.log_path,
    }
    if preflight_result.exit_code != 0:
        status = classify_preflight_failure(read_text(Path(preflight_result.log_path)))
        report["preflight"]["status"] = status
        report["errors"].append(f"preflight failed with exit_code={preflight_result.exit_code}")
        return finish(report, output_dir, status, json_stdout=args.json)

    before = snapshot_runtime(artifact_paths)
    smoke_log = output_dir / "commands" / "runtime_smoke.log"
    runtime_cmd = [
        "timeout",
        f"{args.timeout_seconds}s",
        "cargo",
        "run",
        "-p",
        "ghost-launcher",
        "--bin",
        "ghost-launcher",
        "--",
        "--config",
        str(config_path),
    ]
    runtime_result = run_command_to_log(runtime_cmd, cwd=root, log_path=smoke_log)
    report["runtime_smoke"] = {
        "status": PASS_STATUS if runtime_result.exit_code == 124 else "FAIL",
        "timeout_seconds": args.timeout_seconds,
        "exit_code": runtime_result.exit_code,
        "command": runtime_cmd,
        "log_path": runtime_result.log_path,
    }
    if runtime_result.exit_code != 124:
        report["errors"].append(f"runtime smoke exit_code={runtime_result.exit_code}, expected 124")
        return finish(report, output_dir, FAIL_RUNTIME_SMOKE, json_stdout=args.json)

    after = snapshot_runtime(artifact_paths)
    smoke_text = read_text(smoke_log)
    appended_log_text = read_appended_log_text(artifact_paths, before)
    marker_counts = count_markers(smoke_text + "\n" + appended_log_text)
    lifecycle_delta = max(0, after.shadow_lifecycle_lines - before.shadow_lifecycle_lines)
    lifecycle_rows = tail_jsonl(artifact_paths.shadow_lifecycle, lifecycle_delta)
    lifecycle_matrix = summarize_lifecycle_rows(lifecycle_rows)
    artifact_deltas = {
        "shadow_buys_delta": after.shadow_buys_lines - before.shadow_buys_lines,
        "shadow_entries_delta": after.shadow_entries_lines - before.shadow_entries_lines,
        "shadow_lifecycle_delta": lifecycle_delta,
        "diag_account_update_relay_delta": marker_counts.get("DIAG_ACCOUNT_UPDATE_RELAY", 0),
    }
    report["artifact_deltas"] = artifact_deltas
    report["legacy_contract_matrix"] = lifecycle_matrix
    report["runtime_markers"] = marker_counts
    artifact_status, artifact_errors = validate_runtime_artifacts(
        artifact_deltas,
        marker_counts,
        lifecycle_matrix,
    )
    if artifact_status != PASS_STATUS:
        report["errors"].extend(artifact_errors)
        return finish(report, output_dir, artifact_status, json_stdout=args.json)

    reporter_output = output_dir / "restore_shadow_lifecycle_report.jsonl"
    reporter_summary = output_dir / "restore_raportneu.json"
    reporter_cmd = [
        sys.executable,
        str(root / "scripts" / "shadow_onchain_lifecycle_report.py"),
        "--config",
        str(config_path),
        "--output",
        str(reporter_output),
        "--outcome-summary-output",
        str(reporter_summary),
    ]
    reporter_result = run_command_to_log(
        reporter_cmd,
        cwd=root,
        log_path=output_dir / "commands" / "reporter.log",
    )
    reporter_stdout = read_text(Path(reporter_result.log_path))
    if reporter_result.exit_code != 0:
        report["reporter"] = {
            "status": "FAIL",
            "exit_code": reporter_result.exit_code,
            "command": reporter_cmd,
            "log_path": reporter_result.log_path,
            "output": str(reporter_output),
            "outcome_summary_output": str(reporter_summary),
        }
        report["errors"].append(f"reporter failed with exit_code={reporter_result.exit_code}")
        return finish(report, output_dir, FAIL_REPORTER_TRUTH, json_stdout=args.json)

    reporter_rows = list(iter_jsonl(reporter_output))
    reporter_validation = validate_reporter_rows(
        reporter_rows,
        min_rows_written=args.min_rows_written,
        require_resolved=args.require_resolved,
        reporter_stdout=reporter_stdout,
    )
    report["reporter"] = {
        "status": reporter_validation.status,
        "exit_code": reporter_result.exit_code,
        "command": reporter_cmd,
        "log_path": reporter_result.log_path,
        "output": str(reporter_output),
        "outcome_summary_output": str(reporter_summary),
        "rows_written": reporter_validation.rows_written,
        "close_truth_coverage": reporter_validation.close_truth_coverage,
        "truth_status_resolved_rows": reporter_validation.truth_status_resolved_rows,
        "truth_source_canonical_rows": reporter_validation.truth_source_canonical_rows,
        "gatekeeper_buy_context_found_rows": reporter_validation.gatekeeper_buy_context_found_rows,
        "final_pnl_pct_present_rows": reporter_validation.final_pnl_pct_present_rows,
        "exit_fills_total": reporter_validation.exit_fills_total,
        "accepted_close_reason_rows": reporter_validation.accepted_close_reason_rows,
    }
    if reporter_validation.status != PASS_STATUS:
        report["errors"].extend(reporter_validation.errors)
        return finish(report, output_dir, reporter_validation.status, json_stdout=args.json)

    return finish(report, output_dir, PASS_STATUS, json_stdout=args.json)


if __name__ == "__main__":
    raise SystemExit(main())
