#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

try:
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"
DECISIONS_LOG_NAME = "gatekeeper_v2_decisions.jsonl"
PREFERRED_PLANE = "v25_shadow"
FALLBACK_PLANE = "legacy_live"
EXECUTION_SUCCESS_STATUSES = {"confirmed", "landed", "success", "executed"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Summarize Ghost Decision Stack V3 P0 shadow sidecar fields."
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Rollout config path (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument(
        "--decisions-log",
        type=Path,
        help="Explicit gatekeeper decision JSONL path. Overrides --config resolution.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON report.",
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
            current = root
            for part in line[1:-1].split("."):
                part = part.strip()
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


def resolve_decisions_log(config_path: Path, explicit_path: Path | None = None) -> Path:
    config_path = config_path if config_path.is_absolute() else (REPO_ROOT / config_path).resolve()
    if explicit_path is not None:
        return resolve_path(config_path, explicit_path)
    if not config_path.exists():
        return resolve_path(config_path, "logs/decisions.jsonl") / DECISIONS_LOG_NAME

    config = load_toml(config_path)
    decisions_dir = resolve_path(
        config_path, config.get("oracle", {}).get("decision_log_path", "logs/decisions.jsonl")
    )
    direct = decisions_dir / DECISIONS_LOG_NAME
    if direct.exists():
        return direct
    if not decisions_dir.exists():
        return direct

    candidates = [
        candidate for candidate in decisions_dir.rglob(DECISIONS_LOG_NAME) if candidate.is_file()
    ]
    preferred = [candidate for candidate in candidates if PREFERRED_PLANE in candidate.parts]
    if preferred:
        candidates = preferred
    if not candidates:
        return direct
    candidates.sort(
        key=lambda candidate: (candidate.stat().st_mtime, len(candidate.parts), str(candidate)),
        reverse=True,
    )
    return candidates[0]


def load_jsonl(path: Path) -> tuple[list[dict[str, Any]], int]:
    rows: list[dict[str, Any]] = []
    bad_rows = 0
    if not path.exists():
        return rows, bad_rows
    with path.open("r", encoding="utf-8") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                bad_rows += 1
                continue
            if isinstance(value, dict):
                rows.append(value)
            else:
                bad_rows += 1
    return rows, bad_rows


def row_key(row: dict[str, Any]) -> tuple[Any, ...]:
    if row.get("ab_record_id"):
        return ("ab_record_id", row.get("ab_record_id"))
    return (
        "fallback",
        row.get("pool_id"),
        row.get("join_key"),
        row.get("observation_start_ts_ms"),
    )


def plane_rank(row: dict[str, Any]) -> int:
    plane = row.get("decision_plane")
    if plane == PREFERRED_PLANE:
        return 3
    if has_v3_fields(row):
        return 2
    if plane == FALLBACK_PLANE:
        return 1
    return 0


def deduplicate_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    selected: dict[tuple[Any, ...], dict[str, Any]] = {}
    for row in rows:
        key = row_key(row)
        existing = selected.get(key)
        if existing is None or plane_rank(row) > plane_rank(existing):
            selected[key] = row
    return list(selected.values())


def has_v3_fields(row: dict[str, Any]) -> bool:
    return any(key.startswith("v3_shadow_") for key in row)


def active_verdict(row: dict[str, Any]) -> str:
    verdict_type = str(row.get("verdict_type") or "").upper()
    verdict_buy = row.get("decision_verdict_buy")
    if verdict_buy is True or verdict_type == "BUY":
        return "BUY"
    if verdict_type.startswith("TIMEOUT") or verdict_buy is None:
        return "TIMEOUT"
    if verdict_buy is False or verdict_type.startswith("REJECT"):
        return "REJECT"
    return "UNKNOWN"


def confidence_bucket(value: Any) -> str:
    if not isinstance(value, (int, float)):
        return "missing"
    value = max(0.0, min(1.0, float(value)))
    if value == 0.0:
        return "0"
    if value <= 0.25:
        return "0_to_0_25"
    if value <= 0.50:
        return "0_25_to_0_50"
    if value <= 0.75:
        return "0_50_to_0_75"
    return "0_75_to_1_00"


def evidence_summary(value: Any) -> tuple[dict[str, Counter[str]], Counter[str]]:
    by_feature: dict[str, Counter[str]] = defaultdict(Counter)
    missing_degraded = Counter()
    if not isinstance(value, dict):
        missing_degraded["missing_evidence_status"] += 1
        return by_feature, missing_degraded

    for feature_name, feature_status in value.items():
        if not isinstance(feature_status, dict):
            by_feature[str(feature_name)]["malformed"] += 1
            continue
        status = str(feature_status.get("status", "missing")).lower()
        by_feature[str(feature_name)][status] += 1
        if status in {"degraded", "unavailable"}:
            for reason in feature_status.get("degraded_reasons", []) or []:
                missing_degraded[f"degraded:{reason}"] += 1
            for reason in feature_status.get("unavailable_reasons", []) or []:
                missing_degraded[f"unavailable:{reason}"] += 1
    return by_feature, missing_degraded


def manipulation_bucket(value: Any) -> str:
    if not isinstance(value, dict):
        return "missing"
    true_flags = [
        key
        for key, field_value in value.items()
        if isinstance(field_value, bool) and field_value and key != "sybil_evidence_degraded"
    ]
    if true_flags:
        return "contradiction"
    if value.get("sybil_evidence_degraded") is True:
        return "sybil_evidence_degraded"
    return "no_contradiction"


def organic_bucket(value: Any) -> str:
    if not isinstance(value, dict):
        return "missing"
    if not value.get("sequence_available", False):
        return "sequence_missing"
    if (
        value.get("t1_vs_t0_unique_signer_delta", -1) >= 0
        and value.get("t2_vs_t1_unique_signer_delta", -1) >= 0
        and value.get("tx_count_growth_ratio", 0.0) >= 1.0
        and value.get("unique_signer_growth_ratio", 0.0) >= 1.0
    ):
        return "broadening_positive"
    return "broadening_insufficient"


def execution_success(row: dict[str, Any]) -> bool:
    raw = row.get("shadow_execution_outcome") or row.get("execution_status")
    if raw is None:
        return False
    return str(raw).strip().lower() in EXECUTION_SUCCESS_STATUSES


def build_report_from_rows(rows: list[dict[str, Any]], bad_rows: int = 0) -> dict[str, Any]:
    deduped = deduplicate_rows(rows)
    v3_rows = [row for row in deduped if has_v3_fields(row)]
    status = "ok" if v3_rows else ("no_v3_fields" if deduped else "no_rows")

    active_vs_v3: dict[str, Counter[str]] = defaultdict(Counter)
    reason_codes = Counter()
    stages = Counter()
    risk_statuses = Counter()
    opportunity_statuses = Counter()
    confidence_buckets = Counter()
    confidence_cap_reasons = Counter()
    manipulation = Counter()
    organic = Counter()
    evidence_by_feature: dict[str, Counter[str]] = defaultdict(Counter)
    missing_degraded = Counter()
    execution_outcomes = Counter()
    execution_success_count = 0

    for row in v3_rows:
        active = active_verdict(row)
        v3_verdict = str(row.get("v3_shadow_verdict") or "missing")
        active_vs_v3[active][v3_verdict] += 1
        reason_codes[str(row.get("v3_shadow_reason_code") or "missing")] += 1
        stages[str(row.get("v3_shadow_stage") or "missing")] += 1
        risk_statuses[str(row.get("v3_shadow_risk_status") or "missing")] += 1
        opportunity_statuses[str(row.get("v3_shadow_opportunity_status") or "missing")] += 1
        confidence_buckets[confidence_bucket(row.get("v3_shadow_confidence"))] += 1
        for reason in row.get("v3_shadow_confidence_cap_reasons", []) or []:
            confidence_cap_reasons[str(reason)] += 1
        manipulation[manipulation_bucket(row.get("v3_shadow_manipulation_contradictions"))] += 1
        organic[organic_bucket(row.get("v3_shadow_organic_broadening"))] += 1
        feature_counts, reason_counts = evidence_summary(row.get("v3_shadow_evidence_status"))
        for feature, counts in feature_counts.items():
            evidence_by_feature[feature].update(counts)
        missing_degraded.update(reason_counts)
        execution_outcomes[str(row.get("shadow_execution_outcome") or "missing")] += 1
        if execution_success(row):
            execution_success_count += 1

    return {
        "status": status,
        "counts": {
            "raw_rows": len(rows),
            "bad_rows": bad_rows,
            "deduped_rows": len(deduped),
            "duplicate_rows_removed": max(0, len(rows) - len(deduped)),
            "v3_rows": len(v3_rows),
            "no_v3_rows": len(deduped) - len(v3_rows),
        },
        "active_vs_v3_verdict": counters_to_dict(active_vs_v3),
        "v3_reason_codes": dict(sorted(reason_codes.items())),
        "v3_stages": dict(sorted(stages.items())),
        "v3_risk_statuses": dict(sorted(risk_statuses.items())),
        "v3_opportunity_statuses": dict(sorted(opportunity_statuses.items())),
        "confidence_buckets": dict(sorted(confidence_buckets.items())),
        "confidence_cap_reasons": dict(sorted(confidence_cap_reasons.items())),
        "evidence_status_by_feature": counters_to_dict(evidence_by_feature),
        "missing_degraded_evidence": dict(sorted(missing_degraded.items())),
        "manipulation_contradictions": dict(sorted(manipulation.items())),
        "organic_broadening": dict(sorted(organic.items())),
        "execution": {
            "outcomes": dict(sorted(execution_outcomes.items())),
            "success_count": execution_success_count,
            "note": "submitted/no_dispatch/no_execution/missing are not success",
        },
    }


def counters_to_dict(counters: dict[str, Counter[str]]) -> dict[str, dict[str, int]]:
    return {key: dict(sorted(counter.items())) for key, counter in sorted(counters.items())}


def build_report(config_path: Path, decisions_log: Path | None = None) -> dict[str, Any]:
    resolved_log = resolve_decisions_log(config_path, decisions_log)
    rows, bad_rows = load_jsonl(resolved_log)
    report = build_report_from_rows(rows, bad_rows)
    report["inputs"] = {
        "config_path": str(config_path),
        "decisions_log": str(resolved_log),
    }
    return report


def print_text(report: dict[str, Any]) -> None:
    counts = report["counts"]
    print(f"V3 shadow report status: {report['status']}")
    print(
        "Rows: raw={raw_rows} deduped={deduped_rows} v3={v3_rows} no_v3={no_v3_rows} bad={bad_rows}".format(
            **counts
        )
    )
    print("Active vs V3 verdict:")
    for active, breakdown in report["active_vs_v3_verdict"].items():
        print(f"  {active}: {breakdown}")
    print(f"V3 reason codes: {report['v3_reason_codes']}")
    print(f"V3 stages: {report['v3_stages']}")
    print(f"V3 risk statuses: {report['v3_risk_statuses']}")
    print(f"V3 opportunity statuses: {report['v3_opportunity_statuses']}")
    print(f"Confidence buckets: {report['confidence_buckets']}")
    print(f"Confidence cap reasons: {report['confidence_cap_reasons']}")
    print(f"Evidence by feature: {report['evidence_status_by_feature']}")
    print(f"Missing/degraded evidence: {report['missing_degraded_evidence']}")
    print(f"Manipulation contradictions: {report['manipulation_contradictions']}")
    print(f"Organic broadening: {report['organic_broadening']}")
    print(f"Execution: {report['execution']}")


def main() -> int:
    args = parse_args()
    report = build_report(args.config, args.decisions_log)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_text(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
