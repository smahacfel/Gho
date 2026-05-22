#!/usr/bin/env python3
"""Summarize P3.7-L1 reject/PDD/gate diagnostics for a rollout run.

This script is intentionally read-only. It does not recompute Gatekeeper policy
or infer the first kill gate from partial metrics; it reports fields emitted by
the policy/evaluation path and persisted by DecisionLogger.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

try:
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None

try:
    import blake3  # type: ignore
except ModuleNotFoundError:  # pragma: no cover
    blake3 = None


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = (
    REPO_ROOT
    / "configs"
    / "rollout"
    / "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml"
)
DECISION_FILE_NAMES = ("gatekeeper_v2_decisions.jsonl", "gatekeeper_v2_buys.jsonl")
NON_BUY_VERDICTS = ("REJECT", "TIMEOUT", "PENDING")
QUALITY_GATE_MIN_COVERAGE = 95.0
SPIKE_RATIO_QUALITY_VALUES = {
    "ok",
    "earlier_rate_zero",
    "insufficient_earlier_window",
    "insufficient_recent_window",
    "unavailable",
}
BASELINE_LEFT_GATE_FIELDS = (
    "max_hhi",
    "min_bonding_progress_pct",
    "min_market_cap_sol",
    "min_tx_count",
    "min_unique_signers",
    "alpha",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Report P3.7-L1 reject diagnostics and diagnostic-field coverage."
    )
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decisions-log", type=Path)
    parser.add_argument("--output-jsonl", type=Path)
    parser.add_argument("--summary-json", type=Path)
    parser.add_argument("--summary-md", type=Path)
    parser.add_argument("--json", action="store_true")
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
            current = root
            for part in line[1:-1].split("."):
                if part:
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


def resolve_path(base: Path, raw: str | Path) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (base.parent / path).resolve()


def resolve_config_path(path: Path) -> Path:
    if path.is_absolute():
        return path
    return (REPO_ROOT / path).resolve()


def resolve_decisions_log(config_path: Path, explicit_path: Path | None = None) -> Path:
    config_path = resolve_config_path(config_path)
    if explicit_path is not None:
        return resolve_path(config_path, explicit_path)
    if not config_path.exists():
        return resolve_path(config_path, "logs/decisions") / DECISION_FILE_NAMES[0]

    config = load_toml(config_path)
    decision_root = resolve_path(
        config_path,
        config.get("oracle", {}).get("decision_log_path", "logs/decisions"),
    )
    for file_name in DECISION_FILE_NAMES:
        direct = decision_root / file_name
        if direct.exists():
            return direct
    if not decision_root.exists():
        return decision_root / DECISION_FILE_NAMES[0]

    candidates: list[Path] = []
    for file_name in DECISION_FILE_NAMES:
        candidates.extend(path for path in decision_root.rglob(file_name) if path.is_file())
    if not candidates:
        return decision_root / DECISION_FILE_NAMES[0]
    candidates.sort(
        key=lambda path: (path.stat().st_mtime, len(path.parts), str(path)),
        reverse=True,
    )
    return candidates[0]


def file_hash(path: Path) -> str | None:
    if not path.exists():
        return None
    data = path.read_bytes()
    if blake3 is not None:
        return blake3.blake3(data).hexdigest()
    b3sum = shutil.which("b3sum")
    if b3sum:
        try:
            result = subprocess.run(
                [b3sum, str(path)],
                check=True,
                capture_output=True,
                text=True,
            )
        except (OSError, subprocess.CalledProcessError):
            return None
        return result.stdout.split()[0] if result.stdout.split() else None
    return None


def rollout_namespace(config: dict[str, Any], config_path: Path) -> str:
    return str(config.get("p37_shadow_probe", {}).get("namespace") or config_path.stem)


def r16_artifact_paths(config_path: Path, config: dict[str, Any]) -> dict[str, Path]:
    probe = config.get("p37_shadow_probe", {})
    execution_shadow = config.get("execution", {}).get("shadow", {})
    namespace = rollout_namespace(config, config_path)
    default_dir = REPO_ROOT / "logs" / "shadow_run" / namespace

    def cfg_path(section: dict[str, Any], key: str, fallback: str) -> Path:
        raw = section.get(key)
        return resolve_path(config_path, raw) if raw else default_dir / fallback

    return {
        "probe_selection": cfg_path(probe, "selection_log_path", "probe_selection.jsonl"),
        "probe_skips": cfg_path(probe, "skip_log_path", "probe_skips.jsonl"),
        "probe_transport": cfg_path(probe, "transport_log_path", "probe_transport.jsonl"),
        "probe_entries": cfg_path(probe, "entry_log_path", "probe_shadow_entries.jsonl"),
        "probe_lifecycle": cfg_path(probe, "lifecycle_log_path", "probe_shadow_lifecycle.jsonl"),
        "active_shadow_entries": cfg_path(
            execution_shadow, "entry_log_path", "shadow_entries.jsonl"
        ),
        "active_shadow_lifecycle": cfg_path(
            execution_shadow, "lifecycle_log_path", "shadow_lifecycle.jsonl"
        ),
        "lifecycle_labels": default_dir / "p3_7_shadow_lifecycle_labels.jsonl",
    }


def iter_jsonl(path: Path) -> tuple[list[dict[str, Any]], int]:
    rows: list[dict[str, Any]] = []
    malformed = 0
    if not path.exists():
        return rows, malformed
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if isinstance(value, dict):
                rows.append(value)
            else:
                malformed += 1
    return rows, malformed


def first_present(row: dict[str, Any], *fields: str) -> Any:
    for field in fields:
        value = row.get(field)
        if value is None:
            continue
        if isinstance(value, str) and value == "":
            continue
        return value
    return None


def as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.lower() == "true"
    return bool(value)


def pct(numerator: int, denominator: int) -> float:
    if denominator == 0:
        return 100.0
    return round((numerator / denominator) * 100.0, 3)


def active_verdict(row: dict[str, Any]) -> str:
    return str(
        first_present(
            row,
            "verdict_type",
            "legacy_live_verdict_type",
            "v25_shadow_verdict_type",
            "v3_shadow_verdict",
        )
        or "UNKNOWN"
    )


def is_terminal_non_buy(row: dict[str, Any]) -> bool:
    verdict = active_verdict(row).upper()
    if verdict in {"BUY", "EARLY_BUY"}:
        return False
    return verdict.startswith(NON_BUY_VERDICTS) or row.get("reason_code") is not None


def gate_trace_summary(row: dict[str, Any]) -> list[dict[str, Any]]:
    trace = row.get("gatekeeper_gate_trace")
    if not isinstance(trace, list):
        return []
    result: list[dict[str, Any]] = []
    for entry in trace:
        if not isinstance(entry, dict):
            continue
        result.append(
            {
                "order_idx": entry.get("order_idx"),
                "gate": entry.get("gate"),
                "status": entry.get("status"),
                "hard_or_soft": entry.get("hard_or_soft"),
                "metric_name": entry.get("metric_name"),
                "observed_value": entry.get("observed_value"),
                "threshold_value": entry.get("threshold_value"),
                "threshold_source": entry.get("threshold_source"),
                "reason_code": entry.get("reason_code"),
            }
        )
    return result


def row_has_pdd_drift_context(row: dict[str, Any]) -> bool:
    if row.get("pdd_entry_drift_threshold_source") is not None:
        return True
    for entry in gate_trace_summary(row):
        metric_name = str(entry.get("metric_name") or "").lower()
        gate = str(entry.get("gate") or "").lower()
        if "entry_drift" in metric_name or (gate == "pdd" and "drift" in metric_name):
            return True
    if row.get("pdd_entry_drift_pct") is not None:
        return True
    reason = " ".join(
        str(value or "")
        for value in (
            row.get("reason"),
            row.get("reason_chain"),
            row.get("reason_code"),
            row.get("gatekeeper_first_kill_reason"),
            row.get("gatekeeper_terminal_gate"),
        )
    ).upper()
    return "ENTRY_DRIFT" in reason or "PDD_ENTRY_DRIFT" in reason


def top3_pct(row: dict[str, Any]) -> Any:
    pdd_top3 = row.get("pdd_whale_top3_pct")
    if pdd_top3 is not None:
        return pdd_top3
    value = row.get("top3_volume_pct")
    if isinstance(value, (int, float)) and 0.0 <= value <= 1.0:
        return value * 100.0
    return value


def reject_diagnostic(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "ab_record_id": row.get("ab_record_id"),
        "pool_id": row.get("pool_id"),
        "base_mint": row.get("base_mint"),
        "decision_plane": row.get("decision_plane"),
        "verdict_type": active_verdict(row),
        "reason": first_present(row, "reason", "reason_chain", "reject_reason"),
        "reason_code": row.get("reason_code"),
        "elapsed_ms_since_anchor": row.get("pdd_entry_drift_elapsed_ms"),
        "anchor_price": row.get("pdd_entry_drift_anchor_price"),
        "current_price": row.get("pdd_entry_drift_current_price"),
        "anchor_ts_ms": row.get("pdd_entry_drift_anchor_ts_ms"),
        "current_ts_ms": row.get("pdd_entry_drift_current_ts_ms"),
        "entry_drift_pct": row.get("pdd_entry_drift_pct"),
        "entry_drift_static_max_pct": row.get("pdd_entry_drift_static_max_pct"),
        "entry_drift_elapsed_max_pct": row.get("pdd_entry_drift_elapsed_max_pct"),
        "entry_drift_effective_max_pct": row.get("pdd_entry_drift_effective_max_pct"),
        "entry_drift_threshold_source": row.get("pdd_entry_drift_threshold_source"),
        "hhi": row.get("hhi"),
        "max_hhi": row.get("max_hhi"),
        "top3_pct": top3_pct(row),
        "single_whale_pct": row.get("pdd_whale_single_max_pct"),
        "spike_detected": row.get("pdd_spike_detected"),
        "spike_ratio": row.get("pdd_spike_ratio"),
        "pdd_spike_ratio_quality": row.get("pdd_spike_ratio_quality"),
        "spike_recent_rate": row.get("pdd_spike_recent_rate"),
        "spike_earlier_rate": row.get("pdd_spike_earlier_rate"),
        "ramping_detected": row.get("pdd_ramping_detected"),
        "bonding_progress_pct": row.get("bonding_progress_pct"),
        "market_cap_sol": row.get("current_market_cap_sol"),
        "which_gate_killed_first": row.get("gatekeeper_first_kill_gate"),
        "gatekeeper_first_kill_reason": row.get("gatekeeper_first_kill_reason"),
        "gatekeeper_terminal_gate": row.get("gatekeeper_terminal_gate"),
        "rollout_profile": row.get("rollout_profile"),
        "run_id": row.get("run_id"),
        "session_id": row.get("session_id"),
        "v3_policy_config_hash": row.get("v3_policy_config_hash"),
        "brain_config_path": row.get("brain_config_path"),
        "brain_config_hash": row.get("brain_config_hash"),
        "gate_trace": gate_trace_summary(row),
    }


def count_populated(rows: Iterable[dict[str, Any]], fields: tuple[str, ...]) -> int:
    count = 0
    for row in rows:
        if all(row.get(field) is not None for field in fields):
            count += 1
    return count


def count_present(rows: Iterable[dict[str, Any]], field: str) -> int:
    return sum(1 for row in rows if row.get(field) is not None)


def count_values(rows: Iterable[dict[str, Any]], *fields: str) -> dict[str, int]:
    counter: Counter[str] = Counter()
    for row in rows:
        value = first_present(row, *fields)
        counter[str(value if value is not None else "missing")] += 1
    return dict(sorted(counter.items()))


def namespace_value(row: dict[str, Any]) -> Any:
    return first_present(row, "rollout_namespace", "rollout_profile", "namespace")


def namespace_coverage(rows: list[dict[str, Any]], expected_namespace: str) -> dict[str, Any]:
    matching = sum(1 for row in rows if namespace_value(row) == expected_namespace)
    return {
        "rows": len(rows),
        "matching_namespace_rows": matching,
        "coverage_pct": pct(matching, len(rows)),
        "namespace_counts": count_values(rows, "rollout_namespace", "rollout_profile", "namespace"),
        "run_id_counts": count_values(rows, "run_id"),
        "session_id_counts": count_values(rows, "session_id"),
        "brain_config_hash_counts": count_values(rows, "brain_config_hash"),
    }


def ab_record_id(row: dict[str, Any]) -> str | None:
    value = first_present(row, "ab_record_id", "source_ab_record_id")
    return str(value) if value is not None else None


def policy_hash(row: dict[str, Any]) -> Any:
    return first_present(row, "v3_policy_config_hash", "source_v3_policy_config_hash")


def brain_config_path_matches(value: Any, expected_path: Path | None) -> bool:
    if value is None or value == "":
        return False
    if expected_path is None:
        return True
    raw = Path(str(value))
    if raw == expected_path or str(raw) == str(expected_path):
        return True
    if raw.name == expected_path.name:
        return True
    try:
        return raw.resolve() == expected_path.resolve()
    except OSError:
        return False


def identity_coverage(
    rows: list[dict[str, Any]],
    expected_namespace: str,
    expected_run_id: str | None,
    expected_session_id: str | None,
    expected_brain_config_path: Path | None,
) -> dict[str, Any]:
    total = len(rows)
    namespace_ok = sum(1 for row in rows if namespace_value(row) == expected_namespace)
    run_id_ok = sum(
        1
        for row in rows
        if expected_run_id is None or row.get("run_id") == expected_run_id
    )
    session_id_ok = sum(
        1
        for row in rows
        if expected_session_id is None or row.get("session_id") == expected_session_id
    )
    brain_path_ok = sum(
        1
        for row in rows
        if brain_config_path_matches(row.get("brain_config_path"), expected_brain_config_path)
    )
    brain_hash_ok = sum(1 for row in rows if first_present(row, "brain_config_hash") is not None)
    policy_hash_ok = sum(1 for row in rows if policy_hash(row) is not None)
    status = (
        "PASS"
        if total == 0
        or (
            namespace_ok == total
            and run_id_ok == total
            and session_id_ok == total
            and brain_path_ok == total
            and brain_hash_ok == total
            and policy_hash_ok == total
        )
        else "FAIL"
    )
    return {
        "rows": total,
        "status": status,
        "namespace_coverage_pct": pct(namespace_ok, total),
        "run_id_coverage_pct": pct(run_id_ok, total),
        "session_id_coverage_pct": pct(session_id_ok, total),
        "brain_config_path_coverage_pct": pct(brain_path_ok, total),
        "brain_config_hash_coverage_pct": pct(brain_hash_ok, total),
        "v3_policy_config_hash_coverage_pct": pct(policy_hash_ok, total),
        "namespace_counts": count_values(rows, "rollout_namespace", "rollout_profile", "namespace"),
        "run_id_counts": count_values(rows, "run_id"),
        "session_id_counts": count_values(rows, "session_id"),
        "brain_config_path_counts": count_values(rows, "brain_config_path"),
        "brain_config_hash_counts": count_values(rows, "brain_config_hash"),
        "v3_policy_config_hash_counts": dict(
            sorted(Counter(str(policy_hash(row) or "missing") for row in rows).items())
        ),
    }


def label_quality_counts(label_rows: list[dict[str, Any]]) -> Counter[str]:
    counts: Counter[str] = Counter()
    for row in label_rows:
        value = first_present(row, "buy_quality", "label", "quality_class", "outcome_label")
        counts[str(value or "missing")] += 1
    return counts


def is_good_label(value: str) -> bool:
    normalized = value.lower()
    return normalized in {"good", "dirty_good", "buy_quality_good", "buy_quality_dirty_good"}


def baseline_left_gate_distribution(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    result: dict[str, Counter[str]] = {field: Counter() for field in BASELINE_LEFT_GATE_FIELDS}
    for row in rows:
        trace = gate_trace_summary(row)
        for entry in trace:
            gate = str(entry.get("gate") or "").lower()
            metric = str(entry.get("metric_name") or "").lower()
            status = str(entry.get("status") or "missing")
            observed = entry.get("observed_value")
            threshold = entry.get("threshold_value")
            if "hhi" in metric or "hhi" in gate:
                result["max_hhi"][status] += 1
            if "bonding_progress" in metric or "bonding_progress" in gate:
                result["min_bonding_progress_pct"][status] += 1
            if "market_cap" in metric or "market_cap" in gate:
                result["min_market_cap_sol"][status] += 1
            if "tx_count" in metric or "phase1_quantity" in gate:
                result["min_tx_count"][status] += 1
            if "unique_signers" in metric or "phase1_quantity" in gate:
                result["min_unique_signers"][status] += 1
            if "alpha" in gate or "alpha" in metric:
                result["alpha"][status] += 1
            if observed is not None and threshold is not None:
                bucket = f"{status}:observed_vs_threshold"
                if "hhi" in metric or "hhi" in gate:
                    result["max_hhi"][bucket] += 1
                if "market_cap" in metric or "market_cap" in gate:
                    result["min_market_cap_sol"][bucket] += 1
    return {key: dict(sorted(value.items())) for key, value in result.items()}


def artifact_malformed_counts(paths: dict[str, Path]) -> tuple[dict[str, list[dict[str, Any]]], dict[str, int]]:
    rows_by_name: dict[str, list[dict[str, Any]]] = {}
    malformed_by_name: dict[str, int] = {}
    for name, path in paths.items():
        rows, malformed = iter_jsonl(path)
        rows_by_name[name] = rows
        malformed_by_name[name] = malformed
    return rows_by_name, malformed_by_name


def build_summary(
    config_path: Path,
    config: dict[str, Any],
    decision_log: Path,
    rows: list[dict[str, Any]],
    malformed_rows: int,
    rejects: list[dict[str, Any]],
) -> dict[str, Any]:
    artifact_paths = r16_artifact_paths(config_path, config)
    artifact_rows, artifact_malformed = artifact_malformed_counts(artifact_paths)
    namespace = rollout_namespace(config, config_path)
    brain_config_path_raw = config.get("ghost_brain_config_path")
    brain_config_path = resolve_path(config_path, brain_config_path_raw) if brain_config_path_raw else None
    expected_brain_hash = file_hash(brain_config_path) if brain_config_path else None
    probe_config = config.get("p37_shadow_probe", {})
    expected_run_id = str(probe_config.get("run_id") or "") or None
    expected_session_id = str(probe_config.get("session_id") or "") or None

    pdd_drift_rows = [row for row in rows if row_has_pdd_drift_context(row)]
    spike_rows = [
        row
        for row in rows
        if row.get("pdd_spike_ratio_quality") is not None or as_bool(row.get("pdd_spike_detected"))
    ]
    whale_rows = [
        row
        for row in rows
        if row.get("pdd_whale_single_max_pct") is not None or row.get("pdd_whale_top3_pct") is not None
    ]
    terminal_with_gate = [
        row
        for row in rejects
        if row.get("gatekeeper_first_kill_gate") is not None
        or row.get("gatekeeper_terminal_gate") is not None
    ]

    pdd_anchor_rows = count_populated(
        pdd_drift_rows,
        (
            "pdd_entry_drift_elapsed_ms",
            "pdd_entry_drift_anchor_price",
            "pdd_entry_drift_current_price",
        ),
    )
    gate_coverage = pct(len(terminal_with_gate), len(rejects))
    pdd_coverage = pct(pdd_anchor_rows, len(pdd_drift_rows))
    spike_quality_rows = sum(
        1
        for row in spike_rows
        if str(row.get("pdd_spike_ratio_quality") or "") in SPIKE_RATIO_QUALITY_VALUES
    )
    spike_quality_coverage = pct(spike_quality_rows, len(spike_rows))
    spike_ratio_rows_requiring_ratio = [
        row for row in spike_rows if row.get("pdd_spike_ratio_quality") == "ok"
    ]
    spike_coverage = pct(
        count_populated(spike_ratio_rows_requiring_ratio, ("pdd_spike_ratio",)),
        len(spike_ratio_rows_requiring_ratio),
    )
    whale_coverage = pct(
        count_populated(whale_rows, ("pdd_whale_single_max_pct",)),
        len(whale_rows),
    )
    quality_failures: list[str] = []
    if pdd_coverage < QUALITY_GATE_MIN_COVERAGE:
        quality_failures.append("pdd_entry_drift_anchor_coverage_below_95pct")
    if spike_coverage < QUALITY_GATE_MIN_COVERAGE:
        quality_failures.append("pdd_spike_ratio_coverage_below_95pct")
    if spike_quality_coverage < QUALITY_GATE_MIN_COVERAGE:
        quality_failures.append("pdd_spike_ratio_quality_coverage_below_95pct")
    if whale_coverage < QUALITY_GATE_MIN_COVERAGE:
        quality_failures.append("pdd_whale_single_max_pct_coverage_below_95pct")
    if gate_coverage < QUALITY_GATE_MIN_COVERAGE:
        quality_failures.append("gatekeeper_first_or_terminal_gate_coverage_below_95pct")

    first_kill_counts = Counter(
        str(row.get("gatekeeper_first_kill_gate") or "missing") for row in rejects
    )
    terminal_gate_counts = Counter(
        str(row.get("gatekeeper_terminal_gate") or "missing") for row in rejects
    )
    reason_code_counts = Counter(str(row.get("reason_code") or "missing") for row in rejects)
    verdict_counts = Counter(active_verdict(row) for row in rows)
    reject_verdict_counts = Counter(active_verdict(row) for row in rejects)
    policy_hash_counts = Counter(
        str(row.get("v3_policy_config_hash") or "missing") for row in rows
    )
    brain_hash_counts = Counter(str(row.get("brain_config_hash") or "missing") for row in rows)
    buy_rows = [row for row in rows if active_verdict(row).upper() in {"BUY", "EARLY_BUY"}]
    buy_ab_record_ids = {value for value in (ab_record_id(row) for row in buy_rows) if value}
    buy_shadow_entry_rows = [
        row
        for row in artifact_rows["active_shadow_entries"]
        if ab_record_id(row) in buy_ab_record_ids
    ]
    buy_lifecycle_rows = [
        row
        for row in artifact_rows["active_shadow_lifecycle"]
        if ab_record_id(row) in buy_ab_record_ids
    ]
    probe_selection_by_probe_id = {
        str(row.get("probe_id")): row
        for row in artifact_rows["probe_selection"]
        if row.get("probe_id") is not None
    }
    reject_pending_probe_lifecycle_rows = []
    for row in artifact_rows["probe_lifecycle"]:
        probe_id = str(row.get("probe_id") or "")
        selection = probe_selection_by_probe_id.get(probe_id)
        verdict = str((selection or {}).get("active_verdict_type") or "").upper()
        if verdict.startswith("REJECT") or verdict.startswith("PENDING") or verdict == "TIMEOUT":
            reject_pending_probe_lifecycle_rows.append(row)

    label_counts = label_quality_counts(artifact_rows["lifecycle_labels"])
    good_or_dirty_good = sum(
        count for label, count in label_counts.items() if is_good_label(label)
    )
    all_artifacts_for_hash = list(rows)
    rows_by_identity_scope: dict[str, list[dict[str, Any]]] = {"decisions": rows}
    for name in (
        "probe_selection",
        "probe_transport",
        "probe_entries",
        "probe_lifecycle",
        "active_shadow_entries",
        "active_shadow_lifecycle",
    ):
        all_artifacts_for_hash.extend(artifact_rows[name])
        rows_by_identity_scope[name] = artifact_rows[name]
    all_policy_hash_counts = Counter(
        str(policy_hash(row) or "missing")
        for row in all_artifacts_for_hash
    )
    all_brain_hash_counts = Counter(
        str(row.get("brain_config_hash") or "missing") for row in all_artifacts_for_hash
    )
    identity_coverage_by_scope = {
        name: identity_coverage(
            scoped_rows,
            namespace,
            expected_run_id,
            expected_session_id,
            brain_config_path,
        )
        for name, scoped_rows in rows_by_identity_scope.items()
    }
    identity_status = (
        "PASS"
        if all(scope["status"] == "PASS" for scope in identity_coverage_by_scope.values())
        else "FAIL"
    )
    active_hash_status = (
        "PASS"
        if len(all_artifacts_for_hash) > 0
        and identity_status == "PASS"
        and len([key for key in all_brain_hash_counts if key != "missing"]) == 1
        and len([key for key in all_policy_hash_counts if key != "missing"]) == 1
        else "FAIL"
    )
    namespace_coverage_by_artifact = {
        name: namespace_coverage(rows_for_name, namespace)
        for name, rows_for_name in artifact_rows.items()
    }

    return {
        "schema_version": 1,
        "config_path": str(config_path),
        "namespace": namespace,
        "expected_brain_config_path": str(brain_config_path) if brain_config_path else None,
        "expected_brain_config_hash": expected_brain_hash,
        "expected_run_id": expected_run_id,
        "expected_session_id": expected_session_id,
        "decision_log": str(decision_log),
        "decision_rows": len(rows),
        "malformed_rows": malformed_rows,
        "artifact_paths": {name: str(path) for name, path in artifact_paths.items()},
        "artifact_rows": {name: len(value) for name, value in artifact_rows.items()},
        "artifact_malformed_rows": artifact_malformed,
        "terminal_reject_or_timeout_rows": len(rejects),
        "r16_buy_verdict_count": len(buy_rows),
        "r16_buy_shadow_entry_count": len(buy_shadow_entry_rows),
        "r16_buy_lifecycle_close_count": len(buy_lifecycle_rows),
        "r16_buy_shadow_entry_unmatched_count": len(artifact_rows["active_shadow_entries"])
        - len(buy_shadow_entry_rows),
        "r16_buy_lifecycle_unmatched_count": len(artifact_rows["active_shadow_lifecycle"])
        - len(buy_lifecycle_rows),
        "r16_reject_pending_probe_lifecycle_count": len(reject_pending_probe_lifecycle_rows),
        "verdict_counts": dict(sorted(verdict_counts.items())),
        "reject_verdict_counts": dict(sorted(reject_verdict_counts.items())),
        "v3_policy_config_hash_counts_decisions": dict(sorted(policy_hash_counts.items())),
        "brain_config_hash_counts_decisions": dict(sorted(brain_hash_counts.items())),
        "v3_policy_config_hash_counts_all_r16_artifacts": dict(sorted(all_policy_hash_counts.items())),
        "brain_config_hash_counts_all_r16_artifacts": dict(sorted(all_brain_hash_counts.items())),
        "r16_artifact_identity_status": identity_status,
        "r16_artifact_identity_coverage_by_scope": identity_coverage_by_scope,
        "single_active_hash_status": active_hash_status,
        "namespace_coverage_by_artifact": namespace_coverage_by_artifact,
        "lifecycle_label_quality_counts": dict(sorted(label_counts.items())),
        "good_or_dirty_good_label_rows": good_or_dirty_good,
        "baseline_left_gate_distribution_if_zero_good": baseline_left_gate_distribution(rows)
        if good_or_dirty_good == 0
        else {},
        "first_kill_gate_counts": dict(sorted(first_kill_counts.items())),
        "terminal_gate_counts": dict(sorted(terminal_gate_counts.items())),
        "reason_code_counts": dict(sorted(reason_code_counts.items())),
        "pdd_drift_rows": len(pdd_drift_rows),
        "pdd_drift_anchor_rows": pdd_anchor_rows,
        "spike_detected_rows": len(spike_rows),
        "whale_diagnostic_rows": len(whale_rows),
        "diagnostic_quality": {
            "status": "PASS" if not quality_failures else "FAIL",
            "failures": quality_failures,
            "pdd_entry_drift_anchor_coverage_pct": pdd_coverage,
            "spike_ratio_coverage_pct": spike_coverage,
            "spike_ratio_quality_coverage_pct": spike_quality_coverage,
            "whale_single_max_pct_coverage_pct": whale_coverage,
            "gatekeeper_first_or_terminal_gate_coverage_pct": gate_coverage,
        },
    }


def default_output_paths(config_path: Path) -> tuple[Path, Path, Path]:
    config_path = resolve_config_path(config_path)
    config = load_toml(config_path) if config_path.exists() else {}
    namespace = (
        config.get("p37_shadow_probe", {}).get("namespace")
        or config_path.stem
    )
    out_dir = REPO_ROOT / "logs" / "shadow_run" / str(namespace)
    return (
        out_dir / "p3_7_l1_per_reject_diagnostics.jsonl",
        out_dir / "p3_7_l1_reject_diagnostics_summary.json",
        out_dir / "p3_7_l1_reject_diagnostics_summary.md",
    )


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_md(path: Path, summary: dict[str, Any]) -> None:
    quality = summary["diagnostic_quality"]
    lines = [
        "# P3.7-L1 Reject Diagnostics Summary",
        "",
        f"- namespace: `{summary['namespace']}`",
        f"- config_path: `{summary['config_path']}`",
        f"- expected_brain_config_path: `{summary['expected_brain_config_path']}`",
        f"- expected_brain_config_hash: `{summary['expected_brain_config_hash']}`",
        f"- expected_run_id: `{summary['expected_run_id']}`",
        f"- expected_session_id: `{summary['expected_session_id']}`",
        f"- decision_log: `{summary['decision_log']}`",
        f"- decision_rows: {summary['decision_rows']}",
        f"- malformed_rows: {summary['malformed_rows']}",
        f"- terminal_reject_or_timeout_rows: {summary['terminal_reject_or_timeout_rows']}",
        f"- r16_buy_verdict_count: {summary['r16_buy_verdict_count']}",
        f"- r16_buy_shadow_entry_count: {summary['r16_buy_shadow_entry_count']}",
        f"- r16_buy_lifecycle_close_count: {summary['r16_buy_lifecycle_close_count']}",
        f"- r16_buy_shadow_entry_unmatched_count: {summary['r16_buy_shadow_entry_unmatched_count']}",
        f"- r16_buy_lifecycle_unmatched_count: {summary['r16_buy_lifecycle_unmatched_count']}",
        f"- r16_reject_pending_probe_lifecycle_count: {summary['r16_reject_pending_probe_lifecycle_count']}",
        f"- r16_artifact_identity_status: {summary['r16_artifact_identity_status']}",
        f"- single_active_hash_status: {summary['single_active_hash_status']}",
        f"- diagnostic_quality_status: {quality['status']}",
        f"- pdd_entry_drift_anchor_coverage_pct: {quality['pdd_entry_drift_anchor_coverage_pct']}",
        f"- spike_ratio_coverage_pct: {quality['spike_ratio_coverage_pct']}",
        f"- spike_ratio_quality_coverage_pct: {quality['spike_ratio_quality_coverage_pct']}",
        f"- whale_single_max_pct_coverage_pct: {quality['whale_single_max_pct_coverage_pct']}",
        f"- gatekeeper_first_or_terminal_gate_coverage_pct: {quality['gatekeeper_first_or_terminal_gate_coverage_pct']}",
        "",
        "## Policy Hashes",
        "",
        "### Decision Rows",
        "",
    ]
    for key, value in summary["v3_policy_config_hash_counts_decisions"].items():
        lines.append(f"- v3_policy_config_hash `{key}`: {value}")
    for key, value in summary["brain_config_hash_counts_decisions"].items():
        lines.append(f"- brain_config_hash `{key}`: {value}")
    lines.extend(["", "### All R16 Artifacts", ""])
    for key, value in summary["v3_policy_config_hash_counts_all_r16_artifacts"].items():
        lines.append(f"- v3_policy_config_hash `{key}`: {value}")
    for key, value in summary["brain_config_hash_counts_all_r16_artifacts"].items():
        lines.append(f"- brain_config_hash `{key}`: {value}")
    lines.extend(["", "## Artifact Identity Coverage", ""])
    for scope, coverage in summary["r16_artifact_identity_coverage_by_scope"].items():
        lines.append(f"### {scope}")
        lines.append(f"- status: {coverage['status']}")
        lines.append(f"- rows: {coverage['rows']}")
        lines.append(f"- namespace_coverage_pct: {coverage['namespace_coverage_pct']}")
        lines.append(f"- run_id_coverage_pct: {coverage['run_id_coverage_pct']}")
        lines.append(f"- session_id_coverage_pct: {coverage['session_id_coverage_pct']}")
        lines.append(f"- brain_config_path_coverage_pct: {coverage['brain_config_path_coverage_pct']}")
        lines.append(f"- brain_config_hash_coverage_pct: {coverage['brain_config_hash_coverage_pct']}")
        lines.append(f"- v3_policy_config_hash_coverage_pct: {coverage['v3_policy_config_hash_coverage_pct']}")
        lines.append("")
    lines.extend([
        "",
        "## Artifact Rows",
        "",
    ])
    for key, value in summary["artifact_rows"].items():
        malformed = summary["artifact_malformed_rows"].get(key, 0)
        lines.append(f"- {key}: {value} rows, malformed={malformed}")
    lines.extend([
        "",
        "## Lifecycle Labels",
        "",
    ])
    for key, value in summary["lifecycle_label_quality_counts"].items():
        lines.append(f"- {key}: {value}")
    if summary["baseline_left_gate_distribution_if_zero_good"]:
        lines.extend(["", "## Baseline-Left Gate Distribution", ""])
        for gate, counts in summary["baseline_left_gate_distribution_if_zero_good"].items():
            lines.append(f"### {gate}")
            for key, value in counts.items():
                lines.append(f"- {key}: {value}")
            lines.append("")
    lines.extend([
        "## First Kill Gates",
        "",
    ])
    for key, value in summary["first_kill_gate_counts"].items():
        lines.append(f"- {key}: {value}")
    lines.extend(["", "## Terminal Gates", ""])
    for key, value in summary["terminal_gate_counts"].items():
        lines.append(f"- {key}: {value}")
    lines.extend(["", "## Reason Codes", ""])
    for key, value in summary["reason_code_counts"].items():
        lines.append(f"- {key}: {value}")
    if quality["failures"]:
        lines.extend(["", "## Quality Failures", ""])
        for failure in quality["failures"]:
            lines.append(f"- {failure}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    config_path = resolve_config_path(args.config)
    config = load_toml(config_path) if config_path.exists() else {}
    decision_log = resolve_decisions_log(config_path, args.decisions_log)
    rows, malformed_rows = iter_jsonl(decision_log)
    rejects = [row for row in rows if is_terminal_non_buy(row)]
    diagnostics = [reject_diagnostic(row) for row in rejects]
    summary = build_summary(config_path, config, decision_log, rows, malformed_rows, rejects)

    default_jsonl, default_summary_json, default_summary_md = default_output_paths(config_path)
    output_jsonl = args.output_jsonl or default_jsonl
    summary_json = args.summary_json or default_summary_json
    summary_md = args.summary_md or default_summary_md
    write_jsonl(output_jsonl, diagnostics)
    write_json(summary_json, summary)
    write_md(summary_md, summary)

    if args.json:
        payload = dict(summary)
        payload["outputs"] = {
            "per_reject_diagnostics_jsonl": str(output_jsonl),
            "summary_json": str(summary_json),
            "summary_md": str(summary_md),
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True))
    else:
        quality = summary["diagnostic_quality"]
        print(
            "P3.7-L1 reject diagnostics: "
            f"{quality['status']} rows={summary['terminal_reject_or_timeout_rows']} "
            f"decision_log={decision_log}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
