#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

try:
    import guard_restore_shadow_lifecycle as restore_guard
except ModuleNotFoundError:  # pragma: no cover - defensive for unusual import paths
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import guard_restore_shadow_lifecycle as restore_guard


REPO_ROOT = Path(__file__).resolve().parents[1]
PASS_STATUS = "PASS"
FAIL_EVENT_CANARY = "FAIL_EVENT_CANARY"
FAIL_LIFECYCLE_PROOF = "FAIL_LIFECYCLE_PROOF"
FAIL_RUNTIME_ARTIFACTS = "FAIL_RUNTIME_ARTIFACTS"
FAIL_REPORTER_NO_ROWS = "FAIL_REPORTER_NO_ROWS"
FAIL_REPORTER_TRUTH = "FAIL_REPORTER_TRUTH"
INCONCLUSIVE_ENV_OR_CONFIG = "INCONCLUSIVE_ENV_OR_CONFIG"

REQUIRED_EVENT_KINDS = ("NewPoolDetected", "Candidate", "PoolTransaction")
BAD_DELTA_MARKERS = {
    "AccountNotFound": "AccountNotFound",
    "ResourceExhausted": "ResourceExhausted",
    "relative URL without a base": "relative URL without a base",
    "Custom(6062)": "Custom(6062)",
    "custom program error: 0x17ae": "custom program error: 0x17ae",
    "0x17ae": "0x17ae",
    "unsupported_legacy_buy_layout_requires_bcv2": "unsupported_legacy_buy_layout_requires_bcv2",
}
ACCEPTED_CLOSE_REASONS = {"Target", "StopLoss", "TimeStop"}
EVENT_PASS_CLAIM = "SELECTOR_EVENT_CANARY_PASS"
LIFECYCLE_PASS_CLAIM = "SELECTOR_LIFECYCLE_CANARY_PASS"


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def exit_code_for_status(status: str) -> int:
    if status == PASS_STATUS:
        return 0
    if status == INCONCLUSIVE_ENV_OR_CONFIG:
        return 2
    return 1


def resolve_repo_path(root: Path, raw: Path) -> Path:
    return raw.resolve() if raw.is_absolute() else (root / raw).resolve()


def json_default(value: Any) -> Any:
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, Counter):
        return dict(value)
    raise TypeError(f"cannot JSON encode {type(value).__name__}")


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    yield from restore_guard.iter_jsonl(path)


def event_kind_value(value: Any) -> str | None:
    if isinstance(value, str) and 0 < len(value) <= 128:
        return value
    return None


def detect_event_kind(row: dict[str, Any]) -> str:
    kind = (
        event_kind_value(row.get("kind"))
        or event_kind_value(row.get("type"))
        or event_kind_value(row.get("event_type"))
    )
    payload = row.get("payload")
    if not kind and isinstance(payload, dict):
        kind = (
            event_kind_value(payload.get("kind"))
            or event_kind_value(payload.get("type"))
            or event_kind_value(payload.get("event_type"))
        )
    if kind:
        return str(kind)
    text = json.dumps(row, sort_keys=True)
    for candidate in REQUIRED_EVENT_KINDS:
        if candidate in text:
            return candidate
    return "unknown"


def count_event_kinds(root: Path, scope: str) -> tuple[Counter[str], int]:
    counts: Counter[str] = Counter()
    bad_json = 0
    events_dir = root / "datasets" / "events" / scope
    for path in sorted(events_dir.glob("*.jsonl")):
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for raw_line in fh:
                line = raw_line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    bad_json += 1
                    continue
                if isinstance(row, dict):
                    counts[detect_event_kind(row)] += 1
    return counts, bad_json


def count_lines(path: Path) -> int:
    return restore_guard.count_lines(path)


def build_snapshot(root: Path, scope: str, config: Path) -> dict[str, Any]:
    config_path, artifact_paths = restore_guard.resolve_artifact_paths(root, config)
    runtime = restore_guard.snapshot_runtime(artifact_paths)
    event_counts, bad_json = count_event_kinds(root, scope)
    return {
        "schema_version": 1,
        "scope": scope,
        "config": str(config_path),
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "event_counts": dict(event_counts),
        "bad_event_json": bad_json,
        "shadow_buys_lines": runtime.shadow_buys_lines,
        "shadow_entries_lines": runtime.shadow_entries_lines,
        "shadow_lifecycle_lines": runtime.shadow_lifecycle_lines,
        "log_sizes": runtime.log_sizes,
    }


def load_baseline(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {
            "event_counts": {},
            "bad_event_json": 0,
            "shadow_buys_lines": 0,
            "shadow_entries_lines": 0,
            "shadow_lifecycle_lines": 0,
            "log_sizes": {},
        }
    return json.loads(path.read_text(encoding="utf-8"))


def runtime_snapshot_from_baseline(baseline: dict[str, Any]) -> restore_guard.RuntimeSnapshots:
    return restore_guard.RuntimeSnapshots(
        shadow_buys_lines=int(baseline.get("shadow_buys_lines") or 0),
        shadow_entries_lines=int(baseline.get("shadow_entries_lines") or 0),
        shadow_lifecycle_lines=int(baseline.get("shadow_lifecycle_lines") or 0),
        log_sizes={
            str(key): int(value)
            for key, value in (baseline.get("log_sizes") or {}).items()
            if isinstance(value, int)
        },
    )


def counter_delta(current: Counter[str], before: dict[str, Any]) -> dict[str, int]:
    baseline = Counter({str(k): int(v) for k, v in before.items()})
    keys = set(current) | set(baseline)
    return {key: int(current.get(key, 0) - baseline.get(key, 0)) for key in sorted(keys)}


def read_jsonl_since(path: Path, baseline_lines: int) -> list[dict[str, Any]]:
    rows = list(iter_jsonl(path))
    if baseline_lines <= 0:
        return rows
    return rows[baseline_lines:]


def count_text_markers(text: str, markers: dict[str, str]) -> dict[str, int]:
    return {name: text.count(pattern) for name, pattern in markers.items()}


def row_text(rows: Iterable[dict[str, Any]]) -> str:
    return "\n".join(json.dumps(row, sort_keys=True) for row in rows)


def summarize_lifecycle_delta(rows: list[dict[str, Any]]) -> dict[str, Any]:
    record_type = Counter(row.get("record_type") for row in rows)
    dispatch_status = Counter(row.get("dispatch_status") or row.get("shadow_dispatch_status") for row in rows)
    simulation_outcome = Counter(row.get("simulation_outcome") for row in rows)
    route_resolution_status = Counter(row.get("route_resolution_status") for row in rows)
    execution_feasibility_status = Counter(row.get("execution_feasibility_status") for row in rows)
    selected_route_kind = Counter(row.get("selected_route_kind") for row in rows)
    truth_status = Counter(row.get("truth_status") for row in rows)
    truth_source = Counter(row.get("truth_source") for row in rows)
    close_reason = Counter(row.get("close_reason") for row in rows)
    text = row_text(rows)
    bad_markers = count_text_markers(text, BAD_DELTA_MARKERS)
    return {
        "rows": len(rows),
        "record_type_counts": restore_guard.counter_to_json(record_type),
        "dispatch_status_counts": restore_guard.counter_to_json(dispatch_status),
        "simulation_outcome_counts": restore_guard.counter_to_json(simulation_outcome),
        "route_resolution_status_counts": restore_guard.counter_to_json(route_resolution_status),
        "execution_feasibility_status_counts": restore_guard.counter_to_json(execution_feasibility_status),
        "selected_route_kind_counts": restore_guard.counter_to_json(selected_route_kind),
        "truth_status_counts": restore_guard.counter_to_json(truth_status),
        "truth_source_counts": restore_guard.counter_to_json(truth_source),
        "close_reason_counts": restore_guard.counter_to_json(close_reason),
        "bad_marker_counts": bad_markers,
        "legacy_buy_executable_rows": sum(
            1
            for row in rows
            if row.get("selected_route_kind") == "legacy_buy"
            and row.get("execution_feasibility_status") == "executable"
        ),
        "shadow_dispatch_closed_rows": dispatch_status.get("closed", 0),
        "position_closed_rows": record_type.get("position_closed", 0),
        "exit_filled_rows": record_type.get("exit_filled", 0),
        "truth_status_resolved_rows": truth_status.get("resolved", 0),
        "truth_source_canonical_rows": truth_source.get("canonical_account_state_snapshot", 0),
        "accepted_close_reason_rows": sum(close_reason.get(reason, 0) for reason in ACCEPTED_CLOSE_REASONS),
        "final_pnl_pct_present_rows": sum(1 for row in rows if row.get("final_pnl_pct") is not None),
    }


def validate_event_canary(event_delta: dict[str, int], diag_delta: int, bad_event_json_delta: int) -> tuple[str, list[str]]:
    errors: list[str] = []
    for kind in REQUIRED_EVENT_KINDS:
        if event_delta.get(kind, 0) <= 0:
            errors.append(f"{kind}_delta <= 0")
    if diag_delta <= 0:
        errors.append("DIAG_ACCOUNT_UPDATE_RELAY_delta <= 0")
    if bad_event_json_delta != 0:
        errors.append(f"bad_event_json_delta={bad_event_json_delta}")
    return (PASS_STATUS if not errors else FAIL_EVENT_CANARY), errors


def validate_lifecycle_canary(
    artifact_deltas: dict[str, int],
    lifecycle_summary: dict[str, Any],
    bad_marker_counts: dict[str, int] | None = None,
) -> tuple[str, list[str]]:
    errors: list[str] = []
    for key in ("shadow_buys_delta", "shadow_entries_delta", "shadow_lifecycle_delta"):
        if artifact_deltas.get(key, 0) <= 0:
            errors.append(f"{key} <= 0")
    for marker, count in (bad_marker_counts or lifecycle_summary.get("bad_marker_counts", {})).items():
        if count > 0:
            errors.append(f"{marker}_delta > 0")
    if lifecycle_summary.get("legacy_buy_executable_rows", 0) <= 0:
        errors.append("legacy_buy executable rows <= 0")
    if lifecycle_summary.get("shadow_dispatch_closed_rows", 0) <= 0:
        errors.append("shadow_dispatch closed rows <= 0")
    if lifecycle_summary.get("position_closed_rows", 0) <= 0:
        errors.append("position_closed rows <= 0")
    if lifecycle_summary.get("exit_filled_rows", 0) <= 0:
        errors.append("exit_filled rows <= 0")
    if lifecycle_summary.get("truth_status_resolved_rows", 0) <= 0:
        errors.append("truth_status=resolved lifecycle rows <= 0")
    if lifecycle_summary.get("truth_source_canonical_rows", 0) <= 0:
        errors.append("truth_source=canonical_account_state_snapshot lifecycle rows <= 0")
    if lifecycle_summary.get("final_pnl_pct_present_rows", 0) <= 0:
        errors.append("final_pnl_pct lifecycle rows <= 0")
    if lifecycle_summary.get("accepted_close_reason_rows", 0) <= 0:
        errors.append("accepted close_reason lifecycle rows <= 0")
    return (PASS_STATUS if not errors else FAIL_LIFECYCLE_PROOF), errors


def run_reporter(
    *,
    root: Path,
    config_path: Path,
    output_dir: Path,
    min_rows_written: int,
) -> tuple[dict[str, Any], str, list[str]]:
    reporter_output = output_dir / "selector_lifecycle_canary_report.jsonl"
    reporter_summary = output_dir / "selector_lifecycle_canary_summary.json"
    reporter_log = output_dir / "commands" / "reporter.log"
    reporter_log.parent.mkdir(parents=True, exist_ok=True)
    command = [
        sys.executable,
        str(root / "scripts" / "shadow_onchain_lifecycle_report.py"),
        "--config",
        str(config_path),
        "--output",
        str(reporter_output),
        "--outcome-summary-output",
        str(reporter_summary),
    ]
    with reporter_log.open("w", encoding="utf-8", errors="ignore") as log_fh:
        proc = subprocess.run(
            command,
            cwd=root,
            check=False,
            text=True,
            stdout=log_fh,
            stderr=subprocess.STDOUT,
        )
    errors: list[str] = []
    if proc.returncode != 0:
        errors.append(f"reporter exit_code={proc.returncode}")
        return (
            {
                "status": FAIL_REPORTER_TRUTH,
                "exit_code": proc.returncode,
                "command": command,
                "log_path": str(reporter_log),
                "output": str(reporter_output),
                "outcome_summary_output": str(reporter_summary),
            },
            FAIL_REPORTER_TRUTH,
            errors,
        )
    rows = list(iter_jsonl(reporter_output))
    validation = restore_guard.validate_reporter_rows(
        rows,
        min_rows_written=min_rows_written,
        require_resolved=True,
        reporter_stdout=reporter_log.read_text(encoding="utf-8", errors="ignore"),
    )
    payload = {
        "status": validation.status,
        "exit_code": proc.returncode,
        "command": command,
        "log_path": str(reporter_log),
        "output": str(reporter_output),
        "outcome_summary_output": str(reporter_summary),
        "rows_written": validation.rows_written,
        "close_truth_coverage": validation.close_truth_coverage,
        "truth_status_resolved_rows": validation.truth_status_resolved_rows,
        "truth_source_canonical_rows": validation.truth_source_canonical_rows,
        "gatekeeper_buy_context_found_rows": validation.gatekeeper_buy_context_found_rows,
        "final_pnl_pct_present_rows": validation.final_pnl_pct_present_rows,
        "exit_fills_total": validation.exit_fills_total,
        "accepted_close_reason_rows": validation.accepted_close_reason_rows,
    }
    return payload, validation.status, validation.errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Validate selector run event/lifecycle canary deltas.")
    parser.add_argument("--root", type=Path, default=REPO_ROOT)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, help="Baseline snapshot JSON captured before run start")
    parser.add_argument("--snapshot-output", type=Path, help="Write a baseline snapshot and exit")
    parser.add_argument("--output-dir", type=Path, help="Directory for canary reports")
    parser.add_argument("--phase", choices=("event", "lifecycle", "full"), default="full")
    parser.add_argument("--min-reporter-rows", type=int, default=1)
    parser.add_argument("--json", action="store_true")
    return parser


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True, default=json_default) + "\n", encoding="utf-8")


def write_markdown(path: Path, payload: dict[str, Any]) -> None:
    lines = [
        "# Selector Lifecycle Canary Proof",
        "",
        f"- status: `{payload.get('status')}`",
        f"- claim: `{payload.get('claim')}`",
        f"- phase: `{payload.get('phase')}`",
        f"- scope: `{payload.get('scope')}`",
        f"- config: `{payload.get('config')}`",
        "",
        "## Event Canary",
        "",
    ]
    for key, value in (payload.get("event_canary") or {}).items():
        if isinstance(value, (dict, list)):
            lines.append(f"- {key}: `{json.dumps(value, sort_keys=True)}`")
        else:
            lines.append(f"- {key}: `{value}`")
    lines.extend(["", "## Lifecycle Canary", ""])
    lifecycle = payload.get("lifecycle_canary") or {}
    for key in (
        "status",
        "shadow_buys_delta",
        "shadow_entries_delta",
        "shadow_lifecycle_delta",
        "legacy_buy_executable_rows",
        "shadow_dispatch_closed_rows",
        "position_closed_rows",
        "exit_filled_rows",
        "truth_status_resolved_rows",
        "truth_source_canonical_rows",
        "final_pnl_pct_present_rows",
        "accepted_close_reason_rows",
    ):
        if key in lifecycle:
            lines.append(f"- {key}: `{lifecycle[key]}`")
    reporter = payload.get("reporter") or {}
    lines.extend(["", "## Reporter", ""])
    for key, value in reporter.items():
        if isinstance(value, (dict, list)):
            continue
        lines.append(f"- {key}: `{value}`")
    errors = payload.get("errors") or []
    if errors:
        lines.extend(["", "## Errors", ""])
        for error in errors:
            lines.append(f"- {error}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def finish(report: dict[str, Any], output_dir: Path, status: str, *, json_stdout: bool) -> int:
    report["status"] = status
    if status == PASS_STATUS:
        report["claim"] = EVENT_PASS_CLAIM if report.get("phase") == "event" else LIFECYCLE_PASS_CLAIM
    else:
        report["claim"] = f"SELECTOR_LIFECYCLE_CANARY_FAIL:{status}"
    json_path = output_dir / "RUN_LIFECYCLE_CANARY_PROOF.json"
    md_path = output_dir / "RUN_LIFECYCLE_CANARY_PROOF.md"
    report["artifacts"]["json"] = str(json_path)
    report["artifacts"]["markdown"] = str(md_path)
    write_json(json_path, report)
    write_markdown(md_path, report)
    if json_stdout:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True, default=json_default))
    else:
        print(report["claim"])
        print(f"status={status} report={json_path}")
    return exit_code_for_status(status)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = args.root.resolve()
    config_path = resolve_repo_path(root, args.config)
    output_dir = resolve_repo_path(root, args.output_dir) if args.output_dir else (
        root / "reports" / "selector" / args.scope / f"lifecycle_canary_{utc_timestamp()}"
    )
    output_dir.mkdir(parents=True, exist_ok=True)

    if args.snapshot_output:
        snapshot_path = resolve_repo_path(root, args.snapshot_output)
        snapshot = build_snapshot(root, args.scope, config_path)
        write_json(snapshot_path, snapshot)
        if args.json:
            print(json.dumps(snapshot, ensure_ascii=False, indent=2, sort_keys=True))
        else:
            print(f"snapshot={snapshot_path}")
        return 0

    try:
        baseline = load_baseline(resolve_repo_path(root, args.baseline) if args.baseline else None)
        current_snapshot = build_snapshot(root, args.scope, config_path)
        resolved_config, artifact_paths = restore_guard.resolve_artifact_paths(root, config_path)
        before_runtime = runtime_snapshot_from_baseline(baseline)
        appended_log_text = restore_guard.read_appended_log_text(artifact_paths, before_runtime)
    except Exception as exc:
        report = {
            "status": INCONCLUSIVE_ENV_OR_CONFIG,
            "claim": f"SELECTOR_LIFECYCLE_CANARY_FAIL:{INCONCLUSIVE_ENV_OR_CONFIG}",
            "phase": args.phase,
            "scope": args.scope,
            "config": str(config_path),
            "baseline": str(args.baseline) if args.baseline else None,
            "event_canary": {},
            "lifecycle_canary": {},
            "reporter": {},
            "errors": [str(exc)],
            "artifacts": {},
        }
        return finish(report, output_dir, INCONCLUSIVE_ENV_OR_CONFIG, json_stdout=args.json)

    event_delta = counter_delta(
        Counter({str(k): int(v) for k, v in current_snapshot.get("event_counts", {}).items()}),
        baseline.get("event_counts") or {},
    )
    bad_event_json_delta = int(current_snapshot.get("bad_event_json", 0)) - int(baseline.get("bad_event_json", 0) or 0)
    log_marker_counts = count_text_markers(appended_log_text, {**BAD_DELTA_MARKERS, "DIAG_ACCOUNT_UPDATE_RELAY": "DIAG_ACCOUNT_UPDATE_RELAY"})
    diag_delta = log_marker_counts.get("DIAG_ACCOUNT_UPDATE_RELAY", 0)
    event_status, event_errors = validate_event_canary(event_delta, diag_delta, bad_event_json_delta)

    artifact_deltas = {
        "shadow_buys_delta": int(current_snapshot["shadow_buys_lines"]) - int(baseline.get("shadow_buys_lines", 0) or 0),
        "shadow_entries_delta": int(current_snapshot["shadow_entries_lines"]) - int(baseline.get("shadow_entries_lines", 0) or 0),
        "shadow_lifecycle_delta": int(current_snapshot["shadow_lifecycle_lines"]) - int(baseline.get("shadow_lifecycle_lines", 0) or 0),
    }
    lifecycle_rows = read_jsonl_since(
        artifact_paths.shadow_lifecycle,
        int(baseline.get("shadow_lifecycle_lines", 0) or 0),
    )
    buys_rows = read_jsonl_since(
        artifact_paths.shadow_buys,
        int(baseline.get("shadow_buys_lines", 0) or 0),
    )
    entries_rows = read_jsonl_since(
        artifact_paths.shadow_entries,
        int(baseline.get("shadow_entries_lines", 0) or 0),
    )
    lifecycle_summary = summarize_lifecycle_delta(lifecycle_rows)
    runtime_delta_text = "\n".join(
        [
            appended_log_text,
            row_text(buys_rows),
            row_text(entries_rows),
            row_text(lifecycle_rows),
        ]
    )
    runtime_bad_marker_counts = count_text_markers(runtime_delta_text, BAD_DELTA_MARKERS)
    lifecycle_status, lifecycle_errors = validate_lifecycle_canary(
        artifact_deltas,
        lifecycle_summary,
        runtime_bad_marker_counts,
    )

    report = {
        "schema_version": 1,
        "guard": "selector_lifecycle_canary",
        "status": "RUNNING",
        "claim": "SELECTOR_LIFECYCLE_CANARY_INCOMPLETE",
        "phase": args.phase,
        "scope": args.scope,
        "config": str(resolved_config),
        "baseline": str(args.baseline) if args.baseline else None,
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "event_canary": {
            "status": event_status,
            "event_delta": event_delta,
            "bad_event_json_delta": bad_event_json_delta,
            "diag_account_update_relay_delta": diag_delta,
        },
        "lifecycle_canary": {
            "status": lifecycle_status,
            **artifact_deltas,
            **lifecycle_summary,
            "bad_marker_counts": runtime_bad_marker_counts,
        },
        "reporter": {"status": "SKIPPED"},
        "errors": [],
        "artifacts": {
            "shadow_buys": str(artifact_paths.shadow_buys),
            "shadow_entries": str(artifact_paths.shadow_entries),
            "shadow_lifecycle": str(artifact_paths.shadow_lifecycle),
            "system_log": str(artifact_paths.system_log),
            "oracle_log": str(artifact_paths.oracle_log),
        },
    }

    if args.phase in {"event", "full"} and event_status != PASS_STATUS:
        report["errors"].extend(event_errors)
        return finish(report, output_dir, event_status, json_stdout=args.json)
    if args.phase == "event":
        return finish(report, output_dir, PASS_STATUS, json_stdout=args.json)

    if lifecycle_status != PASS_STATUS:
        report["errors"].extend(lifecycle_errors)
        return finish(report, output_dir, lifecycle_status, json_stdout=args.json)

    reporter_payload, reporter_status, reporter_errors = run_reporter(
        root=root,
        config_path=config_path,
        output_dir=output_dir,
        min_rows_written=args.min_reporter_rows,
    )
    report["reporter"] = reporter_payload
    if reporter_status != PASS_STATUS:
        report["errors"].extend(reporter_errors)
        return finish(report, output_dir, reporter_status, json_stdout=args.json)

    return finish(report, output_dir, PASS_STATUS, json_stdout=args.json)


if __name__ == "__main__":
    raise SystemExit(main())
