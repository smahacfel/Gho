#!/usr/bin/env python3
import argparse
import json
import sys
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Any
from urllib import error as urllib_error
from urllib import request as urllib_request

try:
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"
BUY_LOG_NAME = "gatekeeper_v2_buys.jsonl"
DECISIONS_LOG_NAME = "gatekeeper_v2_decisions.jsonl"

REQUIRED_RECOVERY_MARKERS = (
    "Runtime durability profile resolved",
    "WAL replay complete",
    "Runtime recovery complete",
)
RECOVERY_FAILURE_MARKERS = (
    "WAL replay failed",
    "ShadowLedger restore failed",
    "Runtime recovery failed",
    "runtime_recovery_mode=cold_start",
)
LIVE_SIDE_EFFECT_MARKERS = (
    "LIVE FIRE",
    "BUNDLE SIEDZI W BLOKU",
)
EVENT_BUS_LAG_MARKERS = ("lagged by",)
SAFETY_REJECTION_MARKERS = ("Trigger: BUY rejected by bulkhead safety",)
LOG_SESSION_GRACE_MS = 60_000


@dataclass
class Inputs:
    config_path: Path
    execution_mode: str
    entry_mode: str
    runtime_lane: str
    decisions_dir: Path
    buys_log: Path
    decisions_log: Path
    shadow_log: Path
    shadow_lifecycle_log: Path
    events_dir: Path
    system_log: Path
    metrics_text: Path | None
    min_net_pnl_sol: float | None
    max_position_size_sol: float
    emergency_floor_sol: float
    position_size_buffer_sol: float
    session_run_id: str | None
    session_start_ms: int | None
    session_end_ms: int | None


@dataclass
class GateResult:
    passed: bool
    details: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build formal shadow/paper burn-in go/no-go report from rollout artifacts."
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Launcher config used for the session (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument(
        "--metrics-text",
        type=Path,
        help="Optional Prometheus text snapshot path captured during/after the session",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON report instead of text summary",
    )
    parser.add_argument(
        "--min-net-pnl-sol",
        type=float,
        default=None,
        help=(
            "Explicit minimum accepted aggregate net PnL for economics gate. "
            "When omitted, the report derives a non-fatal loss floor from "
            "[trigger].position_size_buffer_sol in the rollout config."
        ),
    )
    parser.add_argument(
        "--session-end-ms",
        type=int,
        help=(
            "Optional upper bound for the analyzed session window in unix ms. "
            "Useful to freeze a completed burn-in slice before a later in-flight candidate started."
        ),
    )
    return parser.parse_args()


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is not None:
        with path.open("rb") as fh:
            return tomllib.load(fh)
    return load_basic_toml(path)


def load_basic_toml(path: Path) -> dict[str, Any]:
    root: dict[str, Any] = {}
    current = root
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            parts = [part.strip() for part in line[1:-1].split(".") if part.strip()]
            current = root
            for part in parts:
                current = current.setdefault(part, {})
            continue
        if "=" not in line:
            continue
        key, value = [part.strip() for part in line.split("=", 1)]
        current[key] = parse_basic_toml_value(value)
    return root


def parse_basic_toml_value(raw: str) -> Any:
    if raw.startswith('"') and raw.endswith('"'):
        return raw[1:-1]
    lowered = raw.lower()
    if lowered == "true":
        return True
    if lowered == "false":
        return False
    try:
        if "." in raw:
            return float(raw)
        return int(raw)
    except ValueError:
        return raw


def resolve_config_path(config_path: Path) -> Path:
    return config_path if config_path.is_absolute() else (REPO_ROOT / config_path).resolve()


def resolve_inputs(args: argparse.Namespace) -> Inputs:
    config_path = resolve_config_path(args.config)
    config = load_toml(config_path)

    execution_mode = config.get("execution", {}).get("execution_mode", "paper")
    entry_mode = config.get("trigger", {}).get("entry_mode", "shadow_only")
    runtime_lane = "shadow" if str(execution_mode).lower() == "shadow" else "paper"
    trigger_cfg = config.get("trigger", {})
    shadow_cfg = config.get("execution", {}).get("shadow", {})
    decisions_dir = resolve_runtime_path(
        config_path, config.get("oracle", {}).get("decision_log_path", "logs/decisions.jsonl")
    )
    shadow_log = resolve_runtime_path(
        config_path,
        config.get("trigger", {}).get("shadow_run", {}).get("output_path", "logs/shadow_run/buys.jsonl"),
    )
    shadow_entry_log = resolve_runtime_path(
        config_path,
        shadow_cfg.get("entry_log_path", "logs/shadow_run/shadow_entries.jsonl"),
    )
    shadow_lifecycle_log = resolve_runtime_path(
        config_path,
        shadow_cfg.get("lifecycle_log_path") or derive_shadow_lifecycle_log_path(shadow_entry_log),
    )
    events_dir = resolve_runtime_path(
        config_path, config.get("execution", {}).get("events", {}).get("output_dir", "datasets/events")
    )
    configured_system_log = resolve_runtime_path(
        config_path, config.get("logging", {}).get("file_path", "logs/system.log")
    )
    system_log = resolve_existing_log_path(configured_system_log)
    metrics_text = (
        resolve_runtime_path(config_path, str(args.metrics_text))
        if args.metrics_text
        else system_log.parent / "metrics.prom"
    )
    metrics_config = config.get("metrics", {})
    if metrics_text is not None and not metrics_text.exists() and metrics_config.get("enabled", False):
        maybe_capture_metrics_snapshot(
            metrics_text,
            str(metrics_config.get("bind", "127.0.0.1")),
            int(metrics_config.get("port", 9090)),
        )

    session_run_id, session_start_ms = detect_latest_run_scope(events_dir)

    return Inputs(
        config_path=config_path,
        execution_mode=execution_mode,
        entry_mode=entry_mode,
        runtime_lane=runtime_lane,
        decisions_dir=decisions_dir,
        buys_log=decisions_dir / BUY_LOG_NAME,
        decisions_log=decisions_dir / DECISIONS_LOG_NAME,
        shadow_log=shadow_log,
        shadow_lifecycle_log=shadow_lifecycle_log,
        events_dir=events_dir,
        system_log=system_log,
        metrics_text=metrics_text,
        min_net_pnl_sol=args.min_net_pnl_sol,
        max_position_size_sol=float(trigger_cfg.get("max_position_size_sol", 0.0) or 0.0),
        emergency_floor_sol=float(trigger_cfg.get("emergency_floor_sol", 0.0) or 0.0),
        position_size_buffer_sol=float(trigger_cfg.get("position_size_buffer_sol", 0.0) or 0.0),
        session_run_id=session_run_id,
        session_start_ms=session_start_ms,
        session_end_ms=args.session_end_ms,
    )


def resolve_effective_min_net_pnl_sol(inputs: Inputs) -> tuple[float, str]:
    if inputs.min_net_pnl_sol is not None:
        return float(inputs.min_net_pnl_sol), "explicit_cli"
    if inputs.position_size_buffer_sol > 0.0:
        return -inputs.position_size_buffer_sol, "derived_from_position_size_buffer_sol"
    return 0.0, "default_zero_no_buffer"


def resolve_runtime_path(config_path: Path, raw: str) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (config_path.parent / path).resolve()


def derive_shadow_lifecycle_log_path(entry_log_path: Path) -> str:
    if entry_log_path.name == "shadow_entries.jsonl":
        return str(entry_log_path.with_name("shadow_lifecycle.jsonl"))
    if entry_log_path.name:
        return str(entry_log_path.with_name(f"{entry_log_path.name}.lifecycle.jsonl"))
    return str(entry_log_path / "shadow_lifecycle.jsonl")


def resolve_existing_log_path(path: Path) -> Path:
    if path.exists():
        return path
    candidates = [
        candidate
        for candidate in path.parent.glob(f"{path.name}*")
        if candidate.is_file() and candidate.name.startswith(path.name)
    ]
    if not candidates:
        return path
    candidates.sort(key=lambda candidate: (candidate.stat().st_mtime, candidate.name), reverse=True)
    return candidates[0]


def maybe_capture_metrics_snapshot(path: Path, bind: str, port: int) -> None:
    host = bind.strip("[]") or "127.0.0.1"
    if host in {"0.0.0.0", "::", "*"}:
        host = "127.0.0.1"
    url = f"http://{host}:{port}/metrics"
    try:
        with urllib_request.urlopen(url, timeout=3) as response:
            payload = response.read()
    except (urllib_error.URLError, TimeoutError, OSError):
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def extract_numeric_suffix(value: str | None) -> int | None:
    if not value:
        return None
    try:
        return int(value.rsplit("-", 1)[1])
    except (IndexError, ValueError):
        return None


def extract_candidate_ts_ms(candidate_id: str | None) -> int | None:
    if not candidate_id:
        return None
    try:
        return int(candidate_id.rsplit("_", 1)[1])
    except (IndexError, ValueError):
        return None


def detect_latest_run_scope(events_dir: Path) -> tuple[str | None, int | None]:
    latest_run_id: str | None = None
    latest_start_ms: int | None = None
    if not events_dir.exists():
        return latest_run_id, latest_start_ms

    for path in sorted(events_dir.rglob("*.jsonl")):
        with path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not isinstance(event, dict):
                    continue
                run_id = event.get("envelope", {}).get("run_id")
                start_ms = extract_numeric_suffix(run_id)
                if start_ms is None:
                    continue
                if latest_start_ms is None or start_ms > latest_start_ms:
                    latest_run_id = run_id
                    latest_start_ms = start_ms

    return latest_run_id, latest_start_ms


def load_jsonl(path: Path) -> tuple[list[dict[str, Any]], int]:
    rows: list[dict[str, Any]] = []
    bad = 0
    if not path.exists():
        return rows, bad
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if isinstance(value, dict):
                rows.append(value)
            else:
                bad += 1
    return rows, bad


def count_jsonl_rows(path: Path) -> tuple[int, int]:
    count = 0
    bad = 0
    if not path.exists():
        return count, bad
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if isinstance(value, dict):
                count += 1
            else:
                bad += 1
    return count, bad


def scan_buy_log(
    path: Path, session_start_ms: int | None, session_end_ms: int | None
) -> tuple[list[str], int, int]:
    candidate_ids: list[str] = []
    count = 0
    bad = 0
    if not path.exists():
        return candidate_ids, count, bad
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if not isinstance(row, dict):
                bad += 1
                continue
            candidate_id = row.get("execution_candidate_id")
            candidate_ts = extract_candidate_ts_ms(candidate_id)
            if session_start_ms is not None and (candidate_ts is None or candidate_ts < session_start_ms):
                continue
            if session_end_ms is not None and (candidate_ts is None or candidate_ts > session_end_ms):
                continue
            count += 1
            if candidate_id:
                candidate_ids.append(candidate_id)
    return candidate_ids, count, bad


def scan_shadow_log(
    path: Path, session_start_ms: int | None, session_end_ms: int | None
) -> tuple[list[str], set[str], int, int, int]:
    candidate_ids: list[str] = []
    success_ids: set[str] = set()
    count = 0
    bad = 0
    live_signature_count = 0
    if not path.exists():
        return candidate_ids, success_ids, count, bad, live_signature_count
    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if not isinstance(row, dict):
                bad += 1
                continue
            decision_ts_ms = row.get("decision_ts_ms")
            candidate_ts = extract_candidate_ts_ms(row.get("candidate_id"))
            row_ts = int(decision_ts_ms) if isinstance(decision_ts_ms, (int, float)) else candidate_ts
            if session_start_ms is not None and (row_ts is None or row_ts < session_start_ms):
                continue
            if session_end_ms is not None and (row_ts is None or row_ts > session_end_ms):
                continue
            count += 1
            candidate_id = row.get("candidate_id")
            if candidate_id:
                candidate_ids.append(candidate_id)
                if not row.get("error_class"):
                    success_ids.add(candidate_id)
            if row.get("live_signature"):
                live_signature_count += 1
    return candidate_ids, success_ids, count, bad, live_signature_count


def normalize_lane_name(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    lane = value.strip().lower()
    return lane or None


def lane_matches_filter(lane_value: Any, lane_filter: str | None) -> bool:
    if lane_filter is None:
        return True
    lane = normalize_lane_name(lane_value)
    if lane_filter == "paper":
        return lane in {"paper", "single"}
    return lane == lane_filter


def scan_event_dir(
    events_dir: Path,
    session_start_ms: int | None = None,
    session_end_ms: int | None = None,
    lane_filter: str | None = None,
) -> tuple[dict[str, dict[str, Any]], int, int]:
    candidates: dict[str, dict[str, Any]] = defaultdict(
        lambda: {
            "candidate": 0,
            "entry_submitted": 0,
            "entry_filled": 0,
            "opened": 0,
            "closed": 0,
            "net_pnl_sol": None,
            "gross_pnl_sol": None,
            "estimated_costs_sol": None,
            "close_reason": None,
        }
    )
    parsed_rows = 0
    bad_rows = 0
    if not events_dir.exists():
        return {}, parsed_rows, bad_rows

    for path in sorted(events_dir.rglob("*.jsonl")):
        with path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    bad_rows += 1
                    continue
                parsed_rows += 1
                envelope = event.get("envelope", {}) if isinstance(event, dict) else {}
                if not lane_matches_filter(envelope.get("lane"), lane_filter):
                    continue
                if session_start_ms is not None:
                    candidate_ts = extract_candidate_ts_ms(envelope.get("candidate_id"))
                    event_time_ms = envelope.get("event_time_ms")
                    run_start_ms = extract_numeric_suffix(envelope.get("run_id"))
                    row_ts = next(
                        (
                            ts
                            for ts in (
                                candidate_ts,
                                int(event_time_ms) if isinstance(event_time_ms, (int, float)) else None,
                                run_start_ms,
                            )
                            if ts is not None
                        ),
                        None,
                    )
                    if row_ts is not None and row_ts < session_start_ms:
                        continue
                    if row_ts is None:
                        continue
                if session_end_ms is not None:
                    candidate_ts = extract_candidate_ts_ms(envelope.get("candidate_id"))
                    event_time_ms = envelope.get("event_time_ms")
                    run_start_ms = extract_numeric_suffix(envelope.get("run_id"))
                    row_ts = next(
                        (
                            ts
                            for ts in (
                                candidate_ts,
                                int(event_time_ms) if isinstance(event_time_ms, (int, float)) else None,
                                run_start_ms,
                            )
                            if ts is not None
                        ),
                        None,
                    )
                    if row_ts is not None and row_ts > session_end_ms:
                        continue
                candidate_id = (
                    envelope.get("candidate_id")
                    if isinstance(event, dict)
                    else None
                )
                kind = event.get("kind", {}) if isinstance(event, dict) else {}
                event_type = kind.get("type")
                payload = kind.get("payload", {})
                if not candidate_id or not event_type:
                    continue
                row = candidates[candidate_id]
                if event_type == "Candidate":
                    row["candidate"] += 1
                elif event_type == "EntrySubmitted":
                    row["entry_submitted"] += 1
                elif event_type == "EntryFilled":
                    row["entry_filled"] += 1
                elif event_type == "PositionOpened":
                    row["opened"] += 1
                elif event_type == "PositionClosed":
                    row["closed"] += 1
                    row["net_pnl_sol"] = payload.get("net_pnl_sol")
                    row["gross_pnl_sol"] = payload.get("gross_pnl_sol")
                    row["estimated_costs_sol"] = payload.get("estimated_costs_sol")
                    row["close_reason"] = payload.get("reason")
    return dict(candidates), parsed_rows, bad_rows


def scan_shadow_lifecycle_log(
    path: Path,
    session_start_ms: int | None = None,
    session_end_ms: int | None = None,
) -> tuple[dict[str, dict[str, Any]], int, int]:
    candidates: dict[str, dict[str, Any]] = defaultdict(
        lambda: {
            "exit_filled": 0,
            "exit_blocked": 0,
            "closed": 0,
            "net_pnl_sol": None,
            "gross_pnl_sol": None,
            "estimated_costs_sol": None,
            "entry_value_sol": None,
            "exit_value_sol": None,
            "close_reason": None,
            "truth_status": None,
        }
    )
    parsed_rows = 0
    bad_rows = 0
    if not path.exists():
        return {}, parsed_rows, bad_rows

    with path.open("r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                bad_rows += 1
                continue
            if not isinstance(row, dict):
                bad_rows += 1
                continue
            parsed_rows += 1
            row_ts = next(
                (
                    ts
                    for ts in (
                        int(row["timestamp_ms"]) if isinstance(row.get("timestamp_ms"), (int, float)) else None,
                        extract_candidate_ts_ms(row.get("candidate_id")),
                    )
                    if ts is not None
                ),
                None,
            )
            if session_start_ms is not None and (row_ts is None or row_ts < session_start_ms):
                continue
            if session_end_ms is not None and (row_ts is None or row_ts > session_end_ms):
                continue
            candidate_id = row.get("candidate_id")
            record_type = row.get("record_type")
            if not candidate_id or not isinstance(record_type, str):
                continue
            candidate = candidates[candidate_id]
            if record_type == "exit_filled":
                candidate["exit_filled"] += 1
            elif record_type == "exit_blocked":
                candidate["exit_blocked"] += 1
                candidate["truth_status"] = row.get("truth_status")
            elif record_type == "position_closed":
                candidate["closed"] += 1
                candidate["net_pnl_sol"] = row.get("net_pnl_sol")
                candidate["gross_pnl_sol"] = row.get("gross_pnl_sol")
                candidate["estimated_costs_sol"] = row.get("estimated_costs_sol")
                candidate["entry_value_sol"] = row.get("entry_value_sol")
                candidate["exit_value_sol"] = row.get("exit_value_sol")
                candidate["close_reason"] = row.get("close_reason")
                candidate["truth_status"] = row.get("truth_status")
    return dict(candidates), parsed_rows, bad_rows


def parse_metrics_text(path: Path | None) -> dict[str, float]:
    metrics: dict[str, float] = defaultdict(float)
    if path is None or not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split()
            if len(parts) != 2:
                continue
            name = parts[0].split("{", 1)[0]
            try:
                metrics[name] += float(parts[1])
            except ValueError:
                continue
    return dict(metrics)


def parse_log_timestamp_ms(line: str) -> int | None:
    head = line.split(" ", 1)[0]
    if "T" not in head:
        return None
    try:
        return int(datetime.fromisoformat(head.replace("Z", "+00:00")).timestamp() * 1000)
    except ValueError:
        return None


def scan_log_indicators(
    path: Path,
    session_start_ms: int | None = None,
    session_end_ms: int | None = None,
) -> dict[str, list[str]]:
    required_found: set[str] = set()
    failure_found: set[str] = set()
    live_found: set[str] = set()
    lag_found: set[str] = set()
    safety_found: set[str] = set()
    if not path.exists():
        return {
            "required": [],
            "failure": [],
            "live": [],
            "lag": [],
            "safety": [],
        }

    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if session_start_ms is not None:
                line_ts = parse_log_timestamp_ms(line)
                if line_ts is not None and line_ts < session_start_ms:
                    continue
            if session_end_ms is not None:
                line_ts = parse_log_timestamp_ms(line)
                if line_ts is not None and line_ts > session_end_ms:
                    continue
            lower_line = line.lower()
            for marker in REQUIRED_RECOVERY_MARKERS:
                if marker in line:
                    required_found.add(marker)
            for marker in RECOVERY_FAILURE_MARKERS:
                if marker in line:
                    failure_found.add(marker)
            for marker in LIVE_SIDE_EFFECT_MARKERS:
                if marker in line:
                    live_found.add(marker)
            for marker in EVENT_BUS_LAG_MARKERS:
                if marker in lower_line:
                    lag_found.add(marker)
            for marker in SAFETY_REJECTION_MARKERS:
                if marker in line:
                    safety_found.add(marker)

    return {
        "required": sorted(required_found),
        "failure": sorted(failure_found),
        "live": sorted(live_found),
        "lag": sorted(lag_found),
        "safety": sorted(safety_found),
    }


def filter_buy_rows(
    rows: list[dict[str, Any]], session_start_ms: int | None, session_end_ms: int | None
) -> list[dict[str, Any]]:
    if session_start_ms is None:
        return rows
    filtered: list[dict[str, Any]] = []
    for row in rows:
        candidate_ts = extract_candidate_ts_ms(row.get("execution_candidate_id"))
        if candidate_ts is None:
            continue
        if candidate_ts < session_start_ms:
            continue
        if session_end_ms is not None and candidate_ts > session_end_ms:
            continue
        filtered.append(row)
    return filtered


def filter_shadow_rows(
    rows: list[dict[str, Any]], session_start_ms: int | None, session_end_ms: int | None
) -> list[dict[str, Any]]:
    if session_start_ms is None:
        return rows
    filtered: list[dict[str, Any]] = []
    for row in rows:
        decision_ts_ms = row.get("decision_ts_ms")
        if isinstance(decision_ts_ms, (int, float)):
            numeric_decision_ts = int(decision_ts_ms)
            if numeric_decision_ts >= session_start_ms and (
                session_end_ms is None or numeric_decision_ts <= session_end_ms
            ):
                filtered.append(row)
                continue
        candidate_ts = extract_candidate_ts_ms(row.get("candidate_id"))
        if candidate_ts is not None and candidate_ts >= session_start_ms and (
            session_end_ms is None or candidate_ts <= session_end_ms
        ):
            filtered.append(row)
    return filtered


def duplicate_count(values: list[str]) -> int:
    counts = Counter(values)
    return sum(count - 1 for count in counts.values() if count > 1)


def build_report(inputs: Inputs) -> dict[str, Any]:
    decision_rows, decision_bad = count_jsonl_rows(inputs.decisions_log)
    decision_candidate_ids, buy_rows, buy_bad = scan_buy_log(
        inputs.buys_log, inputs.session_start_ms, inputs.session_end_ms
    )
    (
        shadow_candidate_ids,
        shadow_success_ids,
        shadow_rows,
        shadow_bad,
        live_signature_count,
    ) = scan_shadow_log(inputs.shadow_log, inputs.session_start_ms, inputs.session_end_ms)
    runtime_candidates, event_rows, event_bad = scan_event_dir(
        inputs.events_dir,
        inputs.session_start_ms,
        inputs.session_end_ms,
        lane_filter=inputs.runtime_lane,
    )
    lifecycle_candidates, lifecycle_rows, lifecycle_bad = scan_shadow_lifecycle_log(
        inputs.shadow_lifecycle_log,
        inputs.session_start_ms,
        inputs.session_end_ms,
    )
    metrics = parse_metrics_text(inputs.metrics_text)
    effective_min_net_pnl_sol, economics_floor_source = resolve_effective_min_net_pnl_sol(inputs)
    log_window_start_ms = None
    if inputs.session_start_ms is not None:
        log_window_start_ms = max(0, inputs.session_start_ms - LOG_SESSION_GRACE_MS)
    log_window_end_ms = inputs.session_end_ms
    log_indicators = scan_log_indicators(inputs.system_log, log_window_start_ms, log_window_end_ms)
    metrics_present = (
        inputs.metrics_text is not None and inputs.metrics_text.exists() and bool(metrics)
    )

    runtime_event_ids = set(runtime_candidates)
    runtime_admitted_ids = {
        candidate_id
        for candidate_id, row in runtime_candidates.items()
        if row["entry_submitted"] > 0
        or row["entry_filled"] > 0
        or row["opened"] > 0
        or row["closed"] > 0
    }
    runtime_completed_ids = {
        candidate_id
        for candidate_id, row in runtime_candidates.items()
        if row["opened"] > 0 and row["closed"] > 0
    }
    runtime_inflight_ids = sorted(runtime_admitted_ids - runtime_completed_ids)
    economics_candidates = lifecycle_candidates if inputs.runtime_lane == "shadow" else runtime_candidates
    runtime_closed_rows = [row for row in economics_candidates.values() if row["closed"] > 0]

    total_net_pnl_sol = sum(
        row["net_pnl_sol"]
        for row in runtime_closed_rows
        if isinstance(row.get("net_pnl_sol"), (int, float))
    )
    total_gross_pnl_sol = sum(
        row["gross_pnl_sol"]
        for row in runtime_closed_rows
        if isinstance(row.get("gross_pnl_sol"), (int, float))
    )
    total_estimated_costs_sol = sum(
        row["estimated_costs_sol"]
        for row in runtime_closed_rows
        if isinstance(row.get("estimated_costs_sol"), (int, float))
    )
    missing_shadow_for_decisions = sorted(set(decision_candidate_ids) - set(shadow_candidate_ids))
    missing_runtime_for_shadow = sorted(shadow_success_ids - runtime_event_ids)
    eventbus_lag_total = metrics.get("eventbus_lag_total", 0.0)
    provider_stall_total = metrics.get("provider_stall_total", 0.0)
    recovery_markers_present = log_indicators["required"]
    recovery_failure_markers = log_indicators["failure"]
    live_side_effect_markers = log_indicators["live"]
    lag_markers = log_indicators["lag"]
    safety_markers = log_indicators["safety"]
    safety_rejections_total = metrics.get("trigger_buy_safety_rejections_total", 0.0)

    artifacts_gate = GateResult(
        passed=all(
            [
                inputs.buys_log.exists(),
                inputs.decisions_log.exists(),
                inputs.shadow_log.exists(),
                inputs.shadow_lifecycle_log.exists() if inputs.runtime_lane == "shadow" else True,
                inputs.events_dir.exists(),
                inputs.system_log.exists(),
                metrics_present,
            ]
        ),
        details=(
            f"buys_log={inputs.buys_log.exists()} decisions_log={inputs.decisions_log.exists()} "
            f"shadow_log={inputs.shadow_log.exists()} "
            f"shadow_lifecycle_log={inputs.shadow_lifecycle_log.exists()} "
            f"events_dir={inputs.events_dir.exists()} system_log={inputs.system_log.exists()} "
            f"metrics={metrics_present}"
        ),
    )
    recovery_gate = GateResult(
        passed=len(recovery_markers_present) == len(REQUIRED_RECOVERY_MARKERS)
        and not recovery_failure_markers,
        details=(
            f"markers={recovery_markers_present or ['<none>']} "
            f"failures={recovery_failure_markers or ['<none>']}"
        ),
    )
    safety_gate = GateResult(
        passed=safety_rejections_total == 0.0 and not safety_markers,
        details=(
            f"safety_rejections_total={safety_rejections_total} "
            f"log_markers={safety_markers or ['<none>']} "
            "scope=explicit_bulkhead_violations_only"
        ),
    )
    eventbus_gate = GateResult(
        passed=(eventbus_lag_total == 0.0) and not lag_markers,
        details=f"eventbus_lag_total={eventbus_lag_total} log_markers={lag_markers or ['<none>']}",
    )
    trace_gate = GateResult(
        passed=not missing_shadow_for_decisions and not missing_runtime_for_shadow,
        details=(
            f"decision_without_shadow={len(missing_shadow_for_decisions)} "
            f"shadow_without_{inputs.runtime_lane}={len(missing_runtime_for_shadow)}"
        ),
    )
    lifecycle_gate = GateResult(
        passed=bool(runtime_completed_ids) and not runtime_inflight_ids,
        details=(
            f"shadow_success={len(shadow_success_ids)} "
            f"{inputs.runtime_lane}_seen={len(runtime_event_ids)} "
            f"{inputs.runtime_lane}_admitted={len(runtime_admitted_ids)} "
            f"{inputs.runtime_lane}_completed={len(runtime_completed_ids)} "
            f"{inputs.runtime_lane}_inflight={len(runtime_inflight_ids)}"
        ),
    )
    duplicate_gate = GateResult(
        passed=duplicate_count(decision_candidate_ids) == 0 and duplicate_count(shadow_candidate_ids) == 0,
        details=(
            f"duplicate_decisions={duplicate_count(decision_candidate_ids)} "
            f"duplicate_shadow={duplicate_count(shadow_candidate_ids)}"
        ),
    )
    side_effect_gate = GateResult(
        passed=live_signature_count == 0 and not live_side_effect_markers,
        details=(
            f"live_signature_count={live_signature_count} "
            f"log_markers={live_side_effect_markers or ['<none>']}"
        ),
    )
    economics_gate = GateResult(
        passed=bool(runtime_closed_rows)
        and all(row.get("net_pnl_sol") is not None for row in runtime_closed_rows)
        and total_net_pnl_sol >= effective_min_net_pnl_sol,
        details=(
            f"runtime_lane={inputs.runtime_lane} "
            f"closed_positions={len(runtime_closed_rows)} total_net_pnl_sol={total_net_pnl_sol:.9f} "
            f"total_costs_sol={total_estimated_costs_sol:.9f} "
            f"min_net_pnl_sol={effective_min_net_pnl_sol:.9f} "
            f"economics_floor_source={economics_floor_source}"
        ),
    )

    gates = {
        "mandatory_artifacts": asdict(artifacts_gate),
        "recovery_contract": asdict(recovery_gate),
        "safety_violations": asdict(safety_gate),
        "no_eventbus_lag": asdict(eventbus_gate),
        "trace_correlation": asdict(trace_gate),
        "runtime_lifecycle_complete": asdict(lifecycle_gate),
        "no_duplicate_fire": asdict(duplicate_gate),
        "no_live_side_effects": asdict(side_effect_gate),
        "economics_not_fatal": asdict(economics_gate),
    }
    verdict = "GO" if all(gate["passed"] for gate in gates.values()) else "NO-GO"

    return {
        "profile": {
            "config_path": str(inputs.config_path),
            "execution_mode": inputs.execution_mode,
            "entry_mode": inputs.entry_mode,
            "runtime_lane": inputs.runtime_lane,
            "min_net_pnl_sol": effective_min_net_pnl_sol,
            "configured_min_net_pnl_sol": inputs.min_net_pnl_sol,
            "economics_floor_source": economics_floor_source,
            "max_position_size_sol": inputs.max_position_size_sol,
            "emergency_floor_sol": inputs.emergency_floor_sol,
            "position_size_buffer_sol": inputs.position_size_buffer_sol,
            "session_run_id": inputs.session_run_id,
            "session_start_ms": inputs.session_start_ms,
            "session_end_ms": inputs.session_end_ms,
        },
        "artifacts": {
            "buys_log": str(inputs.buys_log),
            "decisions_log": str(inputs.decisions_log),
            "shadow_log": str(inputs.shadow_log),
            "shadow_lifecycle_log": str(inputs.shadow_lifecycle_log),
            "events_dir": str(inputs.events_dir),
            "system_log": str(inputs.system_log),
            "metrics_text": str(inputs.metrics_text) if inputs.metrics_text else None,
            "bad_buy_rows": buy_bad,
            "bad_decision_rows": decision_bad,
            "bad_shadow_rows": shadow_bad,
            "bad_event_rows": event_bad,
            "bad_lifecycle_rows": lifecycle_bad,
        },
        "summary": {
            "buy_rows": buy_rows,
            "decision_rows": decision_rows,
            "shadow_rows": shadow_rows,
            "shadow_success": len(shadow_success_ids),
            "event_rows": event_rows,
            "lifecycle_rows": lifecycle_rows,
            "runtime_lane": inputs.runtime_lane,
            "runtime_candidates": len(runtime_candidates),
            "runtime_seen": len(runtime_event_ids),
            "runtime_admitted": len(runtime_admitted_ids),
            "runtime_completed": len(runtime_completed_ids),
            "runtime_closed": len(runtime_closed_rows),
            "runtime_inflight": runtime_inflight_ids,
            "total_net_pnl_sol": total_net_pnl_sol,
            "total_gross_pnl_sol": total_gross_pnl_sol,
            "total_estimated_costs_sol": total_estimated_costs_sol,
            "provider_stall_total": provider_stall_total,
            "safety_rejections_total": safety_rejections_total,
            "missing_shadow_for_decisions": missing_shadow_for_decisions,
            "missing_runtime_for_shadow": missing_runtime_for_shadow,
        },
        "gates": gates,
        "notes": {
            "safety_gate_scope": (
                "Automatic gate covers explicit bulkhead safety violations only. "
                "Operator must still judge whether repeated safety rejects indicate config drift "
                "or semantically valid setups being filtered too aggressively."
            )
        },
        "verdict": verdict,
    }


def format_text_report(report: dict[str, Any]) -> str:
    profile = report["profile"]
    summary = report["summary"]
    runtime_lane = profile["runtime_lane"]
    lines = [
        f"{runtime_lane.title()} Burn-in Report",
        (
            "Profile: "
            f"execution_mode={profile['execution_mode']} "
            f"entry_mode={profile['entry_mode']} "
            f"runtime_lane={runtime_lane} "
            f"min_net_pnl_sol={profile['min_net_pnl_sol']:.9f}"
        ),
        (
            "Summary: "
            f"buy_rows={summary['buy_rows']} "
            f"shadow_rows={summary['shadow_rows']} "
            f"shadow_success={summary['shadow_success']} "
            f"runtime_completed={summary['runtime_completed']} "
            f"total_net_pnl_sol={summary['total_net_pnl_sol']:.9f}"
        ),
        "Gates:",
    ]
    for name, gate in report["gates"].items():
        status = "PASS" if gate["passed"] else "FAIL"
        lines.append(f"- {name}: {status} ({gate['details']})")
    lines.append(f"Note: {report['notes']['safety_gate_scope']}")
    lines.append(f"VERDICT: {report['verdict']}")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    inputs = resolve_inputs(args)
    report = build_report(inputs)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(format_text_report(report))
    return 0 if report["verdict"] == "GO" else 2


if __name__ == "__main__":
    raise SystemExit(main())
