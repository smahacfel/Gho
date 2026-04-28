#!/usr/bin/env python3
import argparse
import json
import re
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from json import JSONDecodeError
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONTRACT = REPO_ROOT / "configs" / "refactor" / "phase4_proof_gate.json"
DEFAULT_COVERAGE_AUDIT = REPO_ROOT / "logs" / "decisions.jsonl" / "seer_runtime_coverage_audit.jsonl"
DEFAULT_COV_OUTPUT = REPO_ROOT / "logs" / "decisions.jsonl" / "coverages"
FINAL_COV_GLOB = "coverage*.jsonl"
FINAL_COV_NAME_RE = re.compile(r"^coverage\d{6}:\d{2}:\d{2}\.jsonl$")
REQUIRED_COV_HEADER_FIELDS = (
    "avg_coverage",
    "coverage_complete",
    "unresolved_count",
    "coverage_status_counts",
)
REQUIRED_AUDIT_DIAGNOSTIC_FIELDS = (
    "canonical_update_count",
    "canonical_first_update_latency_ms",
    "live_account_update_count",
    "timed_out_without_canonical_updates",
    "seer_account_updates_before_mapping_total",
    "seer_account_updates_pending_replay_total",
    "seer_account_updates_pending_replay_send_failed_total",
    "seer_account_updates_pending_parse_failed_total",
)


@dataclass
class CheckResult:
    kind: str
    name: str
    passed: bool
    details: str
    observed: Any = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Refactor proof/acceptance gate for canonical ingest health and truth-source closure."
    )
    parser.add_argument(
        "command",
        choices=("summary", "structural-check", "runtime-check", "proof-check"),
    )
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument(
        "--coverage-audit",
        type=Path,
        default=DEFAULT_COVERAGE_AUDIT,
        help=f"seer_runtime_coverage_audit JSONL (default: {DEFAULT_COVERAGE_AUDIT})",
    )
    parser.add_argument(
        "--cov-output",
        type=Path,
        default=None,
        help=(
            "cov.py output JSONL or directory containing coverage*.jsonl "
            f"(default directory: {DEFAULT_COV_OUTPUT})"
        ),
    )
    parser.add_argument(
        "--since-ms",
        type=int,
        default=0,
        help="Ignore coverage audit rows with t0_ms older than this epoch millisecond cutoff.",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def load_contract(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def slice_block(text: str, start: str, end: str) -> tuple[str, str | None]:
    start_idx = text.find(start)
    if start_idx == -1:
        return "", f"start marker not found: {start}"
    end_idx = text.find(end, start_idx)
    if end_idx == -1:
        return "", f"end marker not found after start: {end}"
    return text[start_idx:end_idx], None


def resolve_path(repo_root: Path, raw: str) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (repo_root / path).resolve()


def _line_since_ms(line: str) -> int | None:
    match = re.search(r'"t0_ms"\s*:\s*(\d+)', line)
    if match is None:
        return None
    return int(match.group(1))


def _parse_json_objects_from_line(line: str, *, start_idx: int = 0) -> list[dict[str, Any]]:
    decoder = json.JSONDecoder()
    idx = start_idx
    size = len(line)
    rows: list[dict[str, Any]] = []

    while idx < size:
        while idx < size and line[idx].isspace():
            idx += 1
        if idx >= size:
            break
        payload, end = decoder.raw_decode(line, idx)
        if isinstance(payload, dict):
            rows.append(payload)
        idx = end
    return rows


def _salvage_jsonl_line(line: str, *, since_ms: int) -> list[dict[str, Any]] | None:
    if since_ms <= 0:
        return None

    prefix_since_ms = _line_since_ms(line)
    if prefix_since_ms is None or prefix_since_ms >= since_ms:
        return None

    for start_idx in range(1, len(line)):
        if line[start_idx] != "{":
            continue
        try:
            parsed = _parse_json_objects_from_line(line, start_idx=start_idx)
        except JSONDecodeError:
            continue
        eligible = [
            row for row in parsed if int(row.get("t0_ms", 0) or 0) >= since_ms
        ]
        if eligible:
            return eligible
    return []


def load_jsonl(path: Path, *, since_ms: int = 0) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as fh:
        for idx, raw_line in enumerate(fh, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                payload = json.loads(line)
            except JSONDecodeError as exc:
                salvaged = _salvage_jsonl_line(line, since_ms=since_ms)
                if salvaged is not None:
                    rows.extend(salvaged)
                    continue
                raise ValueError(f"invalid JSONL at {path}:{idx}: {exc.msg}") from exc
            if isinstance(payload, dict):
                rows.append(payload)
    return rows


def parse_cov_output_timestamp(name: str) -> datetime | None:
    match = FINAL_COV_NAME_RE.fullmatch(name)
    if match is None:
        return None
    timestamp = name.removeprefix("coverage").removesuffix(".jsonl")
    try:
        return datetime.strptime(timestamp, "%d%m%y:%H:%M")
    except ValueError:
        return None


def _parse_iso8601_ts_ms(value: Any) -> int | None:
    if not isinstance(value, str):
        return None
    candidate = value.strip()
    if not candidate:
        return None
    try:
        normalized = candidate.replace("Z", "+00:00")
        dt = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1000)


def cov_row_event_ms(row: dict[str, Any]) -> int | None:
    envelope = row.get("envelope")
    if isinstance(envelope, dict):
        event_time_ms = envelope.get("event_time_ms")
        if isinstance(event_time_ms, int):
            return event_time_ms

    for key in (
        "observation_start_ts_ms",
        "first_seen_ts_ms",
        "ab_t0_event_ts_ms",
        "event_time_ms",
    ):
        value = row.get(key)
        if isinstance(value, int):
            return value

    return _parse_iso8601_ts_ms(row.get("timestamp"))


def coverage_status_is_hard_error(status: Any) -> bool:
    return status in (None, "missing_rpc_total_tx")


def coverage_status_is_resolved(status: Any) -> bool:
    return status in (
        "ok",
        "overflow_observed_gt_onchain",
        "overflow_observed_gt_onchain_confirmed",
    )


def coverage_status_is_warning(status: Any) -> bool:
    return status in (
        "overflow_observed_gt_onchain",
        "overflow_observed_gt_onchain_confirmed",
    )


def coverage_value_for_header(row: dict[str, Any]) -> float | None:
    coverage_ratio = row.get("coverage_ratio")
    if isinstance(coverage_ratio, (int, float)) and not isinstance(coverage_ratio, bool):
        return float(coverage_ratio)

    if not coverage_status_is_resolved(row.get("coverage_ratio_status")):
        return None

    coverage_ratio_clamped = row.get("coverage_ratio_clamped")
    if isinstance(coverage_ratio_clamped, (int, float)) and not isinstance(coverage_ratio_clamped, bool):
        return float(coverage_ratio_clamped)

    return None


def rebuild_cov_header(cov_rows: list[dict[str, Any]]) -> dict[str, Any]:
    results = cov_rows[1:] if cov_rows else []
    coverage_values = [
        coverage_value
        for row in results
        if (coverage_value := coverage_value_for_header(row)) is not None
    ]
    coverage_status_counts = Counter(
        row.get("coverage_ratio_status", "unknown") for row in results
    )
    unresolved_count = sum(
        1
        for row in results
        if row.get("rpc_fetch_error") is not None
        or coverage_status_is_hard_error(row.get("coverage_ratio_status"))
    )
    warning_count = sum(
        1
        for row in results
        if row.get("rpc_fetch_error") is None
        and coverage_status_is_warning(row.get("coverage_ratio_status"))
    )
    return {
        "avg_coverage": (
            sum(coverage_values) / len(coverage_values) if coverage_values else None
        ),
        "coverage_complete": unresolved_count == 0,
        "unresolved_count": unresolved_count,
        "warning_count": warning_count,
        "coverage_status_counts": dict(coverage_status_counts),
    }


def filter_cov_rows_for_since(
    cov_rows: list[dict[str, Any]], *, since_ms: int
) -> list[dict[str, Any]]:
    if since_ms <= 0 or not cov_rows:
        return cov_rows
    if len(cov_rows) <= 1:
        return cov_rows
    filtered_rows = [
        row for row in cov_rows[1:] if (cov_row_event_ms(row) or 0) >= since_ms
    ]
    if not filtered_rows:
        return []
    return [rebuild_cov_header([cov_rows[0], *filtered_rows]), *filtered_rows]


def infer_since_ms_from_cov_rows(cov_rows: list[dict[str, Any]]) -> int:
    if not cov_rows:
        return 0

    header = cov_rows[0]
    for key in ("source_cohort_min_ts_ms", "source_since_ms"):
        value = header.get(key)
        if isinstance(value, int) and value > 0:
            return value

    event_times = [
        event_ms
        for row in cov_rows[1:]
        if (event_ms := cov_row_event_ms(row)) is not None and event_ms > 0
    ]
    if event_times:
        return min(event_times)

    return 0


def build_cov_cohort_index(cov_rows: list[dict[str, Any]]) -> dict[str, Any]:
    records = cov_rows[1:] if len(cov_rows) > 1 else []
    join_keys = {
        value
        for row in records
        if isinstance((value := row.get("join_key")), str) and value
    }
    pool_ids = {
        value
        for row in records
        if isinstance((value := row.get("pool_id")), str) and value
    }
    base_mints = {
        value
        for row in records
        if isinstance((value := row.get("base_mint")), str) and value
    }
    return {
        "enabled": bool(join_keys or pool_ids or base_mints),
        "join_keys": join_keys,
        "pool_ids": pool_ids,
        "base_mints": base_mints,
    }


def row_matches_cov_cohort(row: dict[str, Any], cohort: dict[str, Any]) -> bool:
    if not cohort.get("enabled"):
        return True

    join_key = row.get("join_key")
    if isinstance(join_key, str) and join_key in cohort["join_keys"]:
        return True

    pool_id = row.get("pool_id")
    if isinstance(pool_id, str) and pool_id in cohort["pool_ids"]:
        return True

    base_mint = row.get("base_mint")
    if isinstance(base_mint, str) and base_mint in cohort["base_mints"]:
        return True

    return False


def resolve_cov_output_path(path: Path | None, *, since_ms: int = 0) -> Path:
    if path is None:
        path = DEFAULT_COV_OUTPUT
    resolved = path.resolve()
    if resolved.is_dir():
        candidates = [
            candidate
            for candidate in resolved.glob(FINAL_COV_GLOB)
            if candidate.is_file()
            and not candidate.name.endswith(".partial.jsonl")
            and FINAL_COV_NAME_RE.fullmatch(candidate.name)
        ]
        if not candidates:
            raise FileNotFoundError(
                "final cov output not found in directory "
                "(expected timestamped non-partial coverageDDMMYY:HH:MM.jsonl): "
                f"{resolved}"
            )
        if since_ms > 0:
            cohort_candidates: list[tuple[Path, int, int]] = []
            for candidate in candidates:
                try:
                    rows = load_jsonl(candidate)
                except ValueError:
                    continue
                filtered = filter_cov_rows_for_since(rows, since_ms=since_ms)
                if len(filtered) <= 1:
                    continue
                latest_event_ms = max(cov_row_event_ms(row) or 0 for row in filtered[1:])
                cohort_candidates.append((candidate, len(filtered) - 1, latest_event_ms))
            if cohort_candidates:
                return max(
                    cohort_candidates,
                    key=lambda item: (
                        item[1],
                        item[2],
                        parse_cov_output_timestamp(item[0].name) or datetime.min,
                        item[0].name,
                    ),
                )[0]
        return max(
            candidates,
            key=lambda candidate: (
                parse_cov_output_timestamp(candidate.name) or datetime.min,
                candidate.name,
            ),
        )
    if resolved.name.endswith(".partial.jsonl"):
        raise FileNotFoundError(f"cov output must point to finalized jsonl, not partial: {resolved}")
    return resolved


def missing_cov_header_fields(cov_header: dict[str, Any]) -> list[str]:
    return [field for field in REQUIRED_COV_HEADER_FIELDS if field not in cov_header]


def missing_audit_diagnostic_fields(row: dict[str, Any]) -> list[str]:
    diagnostics = row.get("diagnostics")
    if not isinstance(diagnostics, dict):
        return list(REQUIRED_AUDIT_DIAGNOSTIC_FIELDS)
    return [field for field in REQUIRED_AUDIT_DIAGNOSTIC_FIELDS if field not in diagnostics]


def check_required_metric(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    path = resolve_path(repo_root, entry["path"])
    text = read_text(path)
    passed = entry["needle"] in text
    return CheckResult(
        kind="required_metric",
        name=entry["name"],
        passed=passed,
        details=f"path={entry['path']}",
    )


def load_declared_fallbacks(repo_root: Path) -> list[dict[str, str]]:
    path = repo_root / "ghost-launcher" / "src" / "components" / "fallback_contract.rs"
    text = read_text(path)
    out: list[dict[str, str]] = []
    lines = [line.strip() for line in text.splitlines()]
    current: dict[str, str] | None = None
    for line in lines:
        if line.startswith("ShadowFallbackContract {"):
            current = {}
            continue
        if current is None:
            continue
        if line.startswith("site: "):
            current["site"] = line.split('"')[1]
        elif line.startswith("category: ShadowFallbackCategory::"):
            category = line.split("::", 1)[1].rstrip(",")
            current["category"] = "".join(
                ("_" + ch.lower() if ch.isupper() else ch) for ch in category
            ).lstrip("_")
        elif line.startswith("helper: "):
            current["helper"] = line.split('"')[1]
        elif line.startswith("},"):
            if current:
                out.append(current)
            current = None
    return out


def check_fallback_sites(repo_root: Path, contract: dict[str, Any]) -> list[CheckResult]:
    contract_path = repo_root / "ghost-launcher" / "src" / "components" / "fallback_contract.rs"
    required_sites = contract.get("required_fallback_sites", [])
    if not contract_path.exists():
        if not required_sites:
            return []
        return [
            CheckResult(
                kind="fallback_site",
                name="fallback_contract_present",
                passed=False,
                details=f"fallback contract file missing: {contract_path}",
            )
        ]

    declared = load_declared_fallbacks(repo_root)
    by_site = {entry["site"]: entry for entry in declared}
    checks: list[CheckResult] = []
    hidden_primary = 0
    for entry in required_sites:
        observed = by_site.get(entry["site"])
        passed = (
            observed is not None
            and observed.get("category") == entry["category"]
            and observed.get("helper") == entry["helper"]
        )
        checks.append(
            CheckResult(
                kind="fallback_site",
                name=f"fallback_site_{entry['site']}",
                passed=passed,
                details=(
                    f"expected category={entry['category']} helper={entry['helper']}"
                ),
                observed=observed,
            )
        )
    for entry in declared:
        if entry.get("category") == "hidden_primary":
            hidden_primary += 1
    checks.append(
        CheckResult(
            kind="fallback_hidden_primary",
            name="fallback_hidden_primary_sites",
            passed=hidden_primary <= contract["runtime_thresholds"]["max_hidden_primary_sites"],
            details="declared fallback contract must not contain hidden_primary sites",
            observed=hidden_primary,
        )
    )
    return checks


def check_block_absence(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    path = resolve_path(repo_root, entry["path"])
    text = read_text(path)
    block, error = slice_block(text, entry["start"], entry["end"])
    if error is not None:
        return CheckResult(
            kind="block_absence",
            name=entry["name"],
            passed=False,
            details=f"{entry['description']} path={entry['path']} error={error}",
        )
    passed = entry["needle"] not in block
    return CheckResult(
        kind="block_absence",
        name=entry["name"],
        passed=passed,
        details=(
            f"{entry['description']} path={entry['path']} "
            f"start={entry['start']} end={entry['end']}"
        ),
    )


def build_runtime_report(
    audit_rows: list[dict[str, Any]],
    cov_rows: list[dict[str, Any]],
    contract: dict[str, Any],
    since_ms: int = 0,
) -> dict[str, Any]:
    cov_rows = filter_cov_rows_for_since(cov_rows, since_ms=since_ms)
    thresholds = contract["runtime_thresholds"]
    cohort = build_cov_cohort_index(cov_rows)
    if cohort["enabled"] and not any(
        isinstance(row.get("join_key"), str)
        or isinstance(row.get("pool_id"), str)
        or isinstance(row.get("base_mint"), str)
        for row in audit_rows
    ):
        cohort["enabled"] = False
    cov_header = cov_rows[0] if cov_rows else {}
    cov_missing_fields = missing_cov_header_fields(cov_header) if cov_rows else list(REQUIRED_COV_HEADER_FIELDS)
    cov_avg_coverage = cov_header.get("avg_coverage")
    cov_complete = bool(cov_header.get("coverage_complete"))
    cov_unresolved_count = int(cov_header.get("unresolved_count", 0))
    cov_status_counts = cov_header.get("coverage_status_counts", {})

    windows = [
        row
        for row in audit_rows
        if row.get("audit_type") == "seer_runtime_coverage_audit"
        and int(row.get("t0_ms", 0) or 0) >= since_ms
        and row_matches_cov_cohort(row, cohort)
    ]
    ok_windows = [row for row in windows if row.get("audit_status") == "ok"]
    stale_windows = [row for row in windows if int(row.get("schema_version", 0)) < 3]
    incomplete_windows = [
        {
            "window_id": row.get("window_id"),
            "missing": missing_audit_diagnostic_fields(row),
        }
        for row in windows
        if int(row.get("schema_version", 0)) >= 3 and missing_audit_diagnostic_fields(row)
    ]
    eligible_ok_windows = [
        row
        for row in ok_windows
        if int(row.get("schema_version", 0)) >= 3 and not missing_audit_diagnostic_fields(row)
    ]
    truth_windows = [
        row
        for row in eligible_ok_windows
        if int(row.get("chain_truth_count", 0)) > int(row.get("chain_truth_failed_count", 0))
    ]
    truth_windows_without_canonical_update = sum(
        1
        for row in truth_windows
        if int(row.get("diagnostics", {}).get("canonical_update_count", 0)) <= 0
    )
    timeout_zero_update_windows = sum(
        1
        for row in eligible_ok_windows
        if bool(row.get("diagnostics", {}).get("timed_out_without_canonical_updates"))
    )
    invariant_breaks = sum(
        int(row.get("invariants", {}).get("missing_reason_fallbacks", 0))
        for row in eligible_ok_windows
    )
    pending_replay_send_failed = sum(
        int(
            row.get("diagnostics", {}).get(
                "seer_account_updates_pending_replay_send_failed_total", 0
            )
        )
        for row in eligible_ok_windows
    )
    pending_replay_parse_failed = sum(
        int(
            row.get("diagnostics", {}).get(
                "seer_account_updates_pending_parse_failed_total", 0
            )
        )
        for row in eligible_ok_windows
    )
    accepted_sum = sum(int(row.get("runtime_accepted_count", 0)) for row in truth_windows)
    truth_sum = sum(int(row.get("chain_truth_count", 0)) for row in truth_windows)

    checks = [
        CheckResult(
            kind="coverage",
            name="cov_records_present",
            passed=bool(cov_rows),
            details="cov.py output must contain at least one JSONL row",
            observed=len(cov_rows),
        ),
        CheckResult(
            kind="coverage",
            name="cov_header_complete",
            passed=not cov_missing_fields,
            details="cov.py header must contain finalized contract-required fields",
            observed=cov_missing_fields,
        ),
        CheckResult(
            kind="coverage",
            name="cov_avg_coverage",
            passed=isinstance(cov_avg_coverage, (int, float))
            and float(cov_avg_coverage) >= thresholds["min_cov_avg_coverage"],
            details=(
                f"cov.py avg_coverage must be >= {thresholds['min_cov_avg_coverage']}"
            ),
            observed=cov_avg_coverage,
        ),
        CheckResult(
            kind="coverage",
            name="cov_complete",
            passed=(not thresholds["require_cov_complete"]) or cov_complete,
            details="cov.py header must report coverage_complete=true",
            observed=cov_complete,
        ),
        CheckResult(
            kind="coverage",
            name="cov_unresolved_count",
            passed=cov_unresolved_count <= thresholds["max_cov_unresolved_count"],
            details="cov.py unresolved_count must stay within threshold",
            observed=cov_unresolved_count,
        ),
        CheckResult(
            kind="runtime",
            name="coverage_audit_schema_fresh",
            passed=not stale_windows,
            details="coverage_audit rows must use schema_version >= 3",
            observed=len(stale_windows),
        ),
        CheckResult(
            kind="runtime",
            name="coverage_audit_diagnostics_complete",
            passed=not incomplete_windows,
            details="coverage_audit rows must contain all contract-required diagnostics fields",
            observed=incomplete_windows[:5],
        ),
        CheckResult(
            kind="runtime",
            name="coverage_windows_present",
            passed=len(eligible_ok_windows) > 0,
            details="at least one schema-v3 ok coverage-audit window is required",
            observed=len(eligible_ok_windows),
        ),
        CheckResult(
            kind="runtime",
            name="truth_windows_present",
            passed=len(truth_windows) > 0,
            details="at least one truth-bearing coverage window is required",
            observed=len(truth_windows),
        ),
        CheckResult(
            kind="runtime",
            name="timeout_without_canonical_updates_windows",
            passed=timeout_zero_update_windows
            <= thresholds["max_timeout_without_canonical_updates_windows"],
            details=(
                "timed_out_without_canonical_updates windows must stay within threshold"
            ),
            observed=timeout_zero_update_windows,
        ),
        CheckResult(
            kind="runtime",
            name="truth_windows_without_canonical_updates",
            passed=truth_windows_without_canonical_update
            <= thresholds["max_truth_windows_without_canonical_updates"],
            details="truth-bearing windows must stay within the zero-canonical-update threshold",
            observed=truth_windows_without_canonical_update,
        ),
        CheckResult(
            kind="runtime",
            name="runtime_invariant_breaks",
            passed=invariant_breaks <= thresholds["max_runtime_invariant_breaks"],
            details="coverage audit invariants must not report fallback/broken reasons",
            observed=invariant_breaks,
        ),
        CheckResult(
            kind="runtime",
            name="pending_replay_send_failed",
            passed=pending_replay_send_failed <= thresholds["max_pending_replay_send_failed"],
            details="pending replay send failures must stay within threshold",
            observed=pending_replay_send_failed,
        ),
        CheckResult(
            kind="runtime",
            name="pending_replay_parse_failed",
            passed=pending_replay_parse_failed <= thresholds["max_pending_replay_parse_failed"],
            details="pending replay parse failures must stay within threshold",
            observed=pending_replay_parse_failed,
        ),
    ]

    return {
        "passed": all(check.passed for check in checks),
        "checks": [asdict(check) for check in checks],
        "aggregate": {
            "since_ms": since_ms,
            "cov_avg_coverage": cov_avg_coverage,
            "cov_complete": cov_complete,
            "cov_unresolved_count": cov_unresolved_count,
            "cov_status_counts": cov_status_counts,
            "cov_missing_fields": cov_missing_fields,
            "windows_total": len(windows),
            "windows_ok": len(ok_windows),
            "windows_ok_schema_v3": len(eligible_ok_windows),
            "stale_windows": len(stale_windows),
            "incomplete_windows": len(incomplete_windows),
            "truth_windows": len(truth_windows),
            "truth_windows_without_canonical_updates": truth_windows_without_canonical_update,
            "truth_sum": truth_sum,
            "accepted_sum": accepted_sum,
            "timeout_without_canonical_updates_windows": timeout_zero_update_windows,
            "pending_replay_send_failed": pending_replay_send_failed,
            "pending_replay_parse_failed": pending_replay_parse_failed,
        },
    }


def build_structural_report(repo_root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    checks = [check_required_metric(repo_root, entry) for entry in contract.get("required_metrics", [])]
    checks.extend(check_fallback_sites(repo_root, contract))
    checks.extend(
        check_block_absence(repo_root, entry)
        for entry in contract.get("block_absence_checks", [])
    )
    return {
        "passed": all(check.passed for check in checks),
        "checks": [asdict(check) for check in checks],
        "phase_gates": contract.get("phase_gates", []),
    }


def build_summary(contract: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_name": contract["schema_name"],
        "schema_version": contract["schema_version"],
        "plan_path": contract["plan_path"],
        "audit_path": contract["audit_path"],
        "description": contract.get("description"),
        "phase_gates": contract["phase_gates"],
        "required_metrics": contract["required_metrics"],
        "required_fallback_sites": contract["required_fallback_sites"],
        "block_absence_checks": contract.get("block_absence_checks", []),
        "runtime_thresholds": contract["runtime_thresholds"],
    }


def print_text_report(report: dict[str, Any], prefix: str) -> None:
    if "phase_gates" in report:
        print(f"[ok] {prefix}.phase_gates=" + " -> ".join(report["phase_gates"]))
    for check in report["checks"]:
        status = "ok" if check["passed"] else "fail"
        line = f"[{status}] {prefix}.{check['name']}: {check['details']}"
        if check.get("observed") is not None:
            line += f" observed={check['observed']}"
        print(line)


def report_prefix(contract: dict[str, Any]) -> str:
    schema_name = str(contract.get("schema_name", "contract"))
    parts = schema_name.split(".")
    if len(parts) >= 3:
        return parts[2].replace("-", "_")
    return schema_name.replace("-", "_").replace(".", "_")


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    contract = load_contract(args.contract.resolve())
    prefix = report_prefix(contract)

    if args.command == "summary":
        payload = build_summary(contract)
        if args.json:
            json.dump(payload, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print(json.dumps(payload, indent=2))
        return 0

    if args.command == "structural-check":
        report = build_structural_report(repo_root, contract)
        if args.json:
            json.dump(report, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print_text_report(report, f"{prefix}.proof")
        return 0 if report["passed"] else 1

    coverage_path = args.coverage_audit.resolve()
    try:
        cov_path = resolve_cov_output_path(args.cov_output, since_ms=args.since_ms)
    except FileNotFoundError:
        cov_path = (args.cov_output or DEFAULT_COV_OUTPUT).resolve()
    missing_paths = []
    if not coverage_path.exists():
        missing_paths.append(f"coverage audit file missing: {coverage_path}")
    if not cov_path.exists():
        missing_paths.append(f"cov.py output missing: {cov_path}")
    if missing_paths:
        report = {
            "passed": False,
            "checks": [
                asdict(CheckResult(kind="runtime", name="runtime_inputs_present", passed=False, details="; ".join(missing_paths)))
            ],
        }
        if args.json:
            json.dump(report, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print_text_report(report, f"{prefix}.runtime")
        return 1

    parse_errors = []
    try:
        cov_rows = load_jsonl(cov_path)
    except ValueError as exc:
        cov_rows = []
        parse_errors.append(str(exc))
    effective_since_ms = args.since_ms
    if cov_rows and effective_since_ms <= 0:
        inferred_since_ms = infer_since_ms_from_cov_rows(cov_rows)
        if inferred_since_ms > 0:
            effective_since_ms = inferred_since_ms
    try:
        audit_rows = load_jsonl(coverage_path, since_ms=effective_since_ms)
    except ValueError as exc:
        audit_rows = []
        parse_errors.append(str(exc))
    if parse_errors:
        report = {
            "passed": False,
            "checks": [
                asdict(
                    CheckResult(
                        kind="runtime",
                        name="runtime_inputs_present",
                        passed=False,
                        details="; ".join(parse_errors),
                    )
                )
            ],
        }
        if args.json:
            json.dump(report, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print_text_report(report, f"{prefix}.runtime")
        return 1
    runtime_report = build_runtime_report(
        audit_rows, cov_rows, contract, since_ms=effective_since_ms
    )
    if args.command == "runtime-check":
        if args.json:
            json.dump(runtime_report, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print_text_report(runtime_report, f"{prefix}.runtime")
        return 0 if runtime_report["passed"] else 1

    structural_report = build_structural_report(repo_root, contract)
    proof_report = {
        "passed": structural_report["passed"] and runtime_report["passed"],
        "structural": structural_report,
        "runtime": runtime_report,
    }
    if args.json:
        json.dump(proof_report, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        print_text_report(structural_report, f"{prefix}.proof")
        print_text_report(runtime_report, f"{prefix}.runtime")
    return 0 if proof_report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
