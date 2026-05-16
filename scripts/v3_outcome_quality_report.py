#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import v3_shadow_report


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"
BLOCKING_V3_VERDICTS = {"REJECT", "PENDING"}
ENTRY_V3_VERDICTS = {"BUY", "EARLY_BUY"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "P3.5 primary-only V3 outcome quality report. Joins V3 decision rows "
            "with outcome labels or shadow lifecycle economics without activating promotion."
        )
    )
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decisions-log", type=Path)
    parser.add_argument(
        "--outcome-labels",
        type=Path,
        help="Optional labels JSONL, for example output from gatekeeper_outcome_labeler.py.",
    )
    parser.add_argument(
        "--shadow-lifecycle",
        type=Path,
        help="Optional shadow_lifecycle.jsonl. Defaults to [execution.shadow].lifecycle_log_path if present.",
    )
    parser.add_argument(
        "--success-pnl-pct",
        type=float,
        default=0.0,
        help="Minimum lifecycle final_pnl_pct treated as good_entry when only lifecycle is available.",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def iter_jsonl(path: Path | None) -> Iterable[dict[str, Any]]:
    if path is None or not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for raw_line in fh:
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(value, dict):
                yield value


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def float_or_none(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def resolve_cli_path(path: Path | None) -> Path | None:
    if path is None:
        return None
    if path.is_absolute():
        return path
    return (REPO_ROOT / path).resolve()


def resolve_config_path(config_path: Path) -> Path:
    return config_path if config_path.is_absolute() else (REPO_ROOT / config_path).resolve()


def resolve_primary_log(config_path: Path, decisions_log: Path | None) -> Path:
    resolved = v3_shadow_report.resolve_decisions_log(config_path, decisions_log)
    if decisions_log is not None and not resolved.exists():
        raise FileNotFoundError(f"explicit decisions log not found: {resolved}")
    return resolved


def resolve_lifecycle_log(config_path: Path, shadow_lifecycle: Path | None) -> Path | None:
    config_path = resolve_config_path(config_path)
    explicit = resolve_cli_path(shadow_lifecycle)
    if explicit is not None:
        return explicit
    if not config_path.exists():
        return None
    config = v3_shadow_report.load_toml(config_path)
    raw = config.get("execution", {}).get("shadow", {}).get("lifecycle_log_path")
    if not isinstance(raw, str) or not raw:
        return None
    return v3_shadow_report.resolve_path(config_path, raw)


def join_keys(row: dict[str, Any]) -> list[str]:
    keys: list[str] = []
    for field in ("join_key", "ab_record_id"):
        value = str_or_none(row.get(field))
        if value:
            keys.append(f"{field}:{value}")

    pool_id = str_or_none(row.get("pool_id"))
    base_mint = str_or_none(row.get("base_mint"))
    first_seen = row.get("first_seen_ts_ms")
    if pool_id and base_mint:
        keys.append(f"pool_mint:{pool_id}:{base_mint}")
        if isinstance(first_seen, (int, float)):
            keys.append(f"pool_mint_seen:{pool_id}:{base_mint}:{int(first_seen)}")
    elif pool_id:
        keys.append(f"pool:{pool_id}")

    candidate_id = candidate_id_from_row(row)
    if candidate_id:
        keys.append(f"candidate:{candidate_id}")
    return keys


def candidate_id_from_row(row: dict[str, Any]) -> str | None:
    for field in ("execution_candidate_id", "candidate_id", "lifecycle_candidate_id"):
        value = str_or_none(row.get(field))
        if value:
            return value
    return None


def index_by_join_key(rows: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        for key in join_keys(row):
            indexed.setdefault(key, row)
    return indexed


def best_match(row: dict[str, Any], indexed: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
    for key in join_keys(row):
        match = indexed.get(key)
        if match is not None:
            return match
    return None


def lifecycle_index(path: Path | None) -> dict[str, dict[str, Any]]:
    latest: dict[str, dict[str, Any]] = {}
    for row in iter_jsonl(path):
        candidate_id = candidate_id_from_row(row)
        if not candidate_id:
            continue
        if row.get("record_type") in {None, "position_closed"}:
            latest[candidate_id] = row
    return latest


def label_from_outcome_row(row: dict[str, Any] | None) -> dict[str, Any]:
    if row is None:
        return {"outcome_label": "unknown", "label_source": "missing"}
    if row.get("label_valid") is not True:
        return {
            "outcome_label": "unknown",
            "label_source": "outcome_labels",
            "label_invalid_reason": row.get("label_invalid_reason"),
        }
    if row.get("hit_40_before_stop") is True:
        return {"outcome_label": "good_entry", "label_source": "outcome_labels"}
    if row.get("rug_or_early_death") is True:
        return {"outcome_label": "bad_entry", "label_source": "outcome_labels"}
    return {"outcome_label": "neutral_entry", "label_source": "outcome_labels"}


def label_from_lifecycle_row(row: dict[str, Any] | None, success_pnl_pct: float) -> dict[str, Any]:
    if row is None:
        return {"outcome_label": "unknown", "label_source": "missing"}
    final_pnl_pct = lifecycle_final_pnl_pct(row)
    net_pnl_sol = float_or_none(row.get("net_pnl_sol"))
    if final_pnl_pct is not None:
        label = "good_entry" if final_pnl_pct >= success_pnl_pct else "bad_entry"
    elif net_pnl_sol is not None:
        label = "good_entry" if net_pnl_sol >= 0.0 else "bad_entry"
    else:
        label = "unknown"
    return {
        "outcome_label": label,
        "label_source": "shadow_lifecycle",
        "lifecycle_close_reason": row.get("close_reason"),
        "lifecycle_final_pnl_pct": final_pnl_pct,
        "lifecycle_net_pnl_sol": net_pnl_sol,
    }


def lifecycle_final_pnl_pct(row: dict[str, Any]) -> float | None:
    explicit = float_or_none(row.get("final_pnl_pct"))
    if explicit is not None:
        return explicit
    entry_value = float_or_none(row.get("entry_value_sol"))
    exit_value = float_or_none(row.get("exit_value_sol"))
    if entry_value is None or exit_value is None or entry_value <= 0.0:
        return None
    return ((exit_value - entry_value) / entry_value) * 100.0


def outcome_label_for_row(
    row: dict[str, Any],
    label_index: dict[str, dict[str, Any]],
    lifecycle_by_candidate: dict[str, dict[str, Any]],
    success_pnl_pct: float,
) -> dict[str, Any]:
    label = label_from_outcome_row(best_match(row, label_index))
    if label["outcome_label"] != "unknown":
        return label

    candidate_id = candidate_id_from_row(row)
    lifecycle = lifecycle_by_candidate.get(candidate_id) if candidate_id else None
    lifecycle_label = label_from_lifecycle_row(lifecycle, success_pnl_pct)
    if lifecycle_label["outcome_label"] != "unknown":
        return lifecycle_label
    return label


def active_verdict(row: dict[str, Any]) -> str:
    return v3_shadow_report.active_verdict(row)


def v3_verdict(row: dict[str, Any]) -> str:
    return str(row.get("v3_shadow_verdict") or "missing").upper()


def v3_reason(row: dict[str, Any]) -> str:
    return str(row.get("v3_shadow_reason_code") or "missing")


def classify_v3_effect(row: dict[str, Any], label: str) -> str:
    verdict = v3_verdict(row)
    if label == "unknown":
        return "inconclusive"
    if label == "neutral_entry":
        return "v3_neutral_no_target"
    if verdict in BLOCKING_V3_VERDICTS:
        return "v3_helped_avoided_bad_entry" if label == "bad_entry" else "v3_hurt_blocked_good_entry"
    if verdict in ENTRY_V3_VERDICTS:
        return "v3_helped_selected_good_entry" if label == "good_entry" else "v3_hurt_selected_bad_entry"
    return "inconclusive"


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def matrix_dict(matrix: dict[str, Counter[str]]) -> dict[str, dict[str, int]]:
    return {key: counter_dict(counter) for key, counter in sorted(matrix.items())}


def build_quality_rows(
    v3_rows: list[dict[str, Any]],
    label_index: dict[str, dict[str, Any]],
    lifecycle_by_candidate: dict[str, dict[str, Any]],
    success_pnl_pct: float,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in v3_rows:
        label = outcome_label_for_row(row, label_index, lifecycle_by_candidate, success_pnl_pct)
        effect = classify_v3_effect(row, str(label["outcome_label"]))
        rows.append(
            {
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "join_key": row.get("join_key"),
                "active_verdict": active_verdict(row),
                "active_reason": row.get("reason_code") or row.get("verdict_type"),
                "v3_verdict": v3_verdict(row),
                "v3_reason": v3_reason(row),
                "outcome_label": label["outcome_label"],
                "label_source": label["label_source"],
                "v3_effect": effect,
                "lifecycle_final_pnl_pct": label.get("lifecycle_final_pnl_pct"),
                "lifecycle_net_pnl_sol": label.get("lifecycle_net_pnl_sol"),
                "label_invalid_reason": label.get("label_invalid_reason"),
            }
        )
    return rows


def summarize_quality(rows: list[dict[str, Any]]) -> dict[str, Any]:
    effect_counts = Counter(str(row["v3_effect"]) for row in rows)
    label_counts = Counter(str(row["outcome_label"]) for row in rows)
    source_counts = Counter(str(row["label_source"]) for row in rows)
    by_reason: dict[str, Counter[str]] = defaultdict(Counter)
    by_active_v3: dict[str, Counter[str]] = defaultdict(Counter)
    for row in rows:
        by_reason[str(row["v3_reason"])][str(row["v3_effect"])] += 1
        by_active_v3[f"{row['active_verdict']}->{row['v3_verdict']}"][str(row["outcome_label"])] += 1

    known = len(rows) - label_counts.get("unknown", 0)
    total = len(rows)
    if total == 0:
        p3_5_status = "no_v3_rows"
    elif known == 0:
        p3_5_status = "insufficient_outcome_data"
    else:
        p3_5_status = "outcome_quality_ready"

    return {
        "p3_5_status": p3_5_status,
        "v3_rows": total,
        "known_outcome_rows": known,
        "outcome_label_coverage": round(known / total, 6) if total else 0.0,
        "effect_counts": counter_dict(effect_counts),
        "outcome_label_counts": counter_dict(label_counts),
        "label_source_counts": counter_dict(source_counts),
        "by_v3_reason": matrix_dict(by_reason),
        "by_active_to_v3": matrix_dict(by_active_v3),
        "sponsor_summary": {
            "avoided_bad_entries": effect_counts.get("v3_helped_avoided_bad_entry", 0),
            "blocked_good_entries": effect_counts.get("v3_hurt_blocked_good_entry", 0),
            "selected_good_entries": effect_counts.get("v3_helped_selected_good_entry", 0),
            "selected_bad_entries": effect_counts.get("v3_hurt_selected_bad_entry", 0),
            "neutral_entries": effect_counts.get("v3_neutral_no_target", 0),
            "inconclusive": effect_counts.get("inconclusive", 0),
        },
    }


def build_report(
    config_path: Path,
    decisions_log: Path | None = None,
    outcome_labels: Path | None = None,
    shadow_lifecycle: Path | None = None,
    success_pnl_pct: float = 0.0,
) -> dict[str, Any]:
    config_path = resolve_config_path(config_path)
    primary_log = resolve_primary_log(config_path, decisions_log)
    decision_rows, bad_rows = v3_shadow_report.load_jsonl(primary_log)
    v3_rows = [row for row in decision_rows if v3_shadow_report.has_v3_fields(row)]
    label_path = resolve_cli_path(outcome_labels)
    lifecycle_path = resolve_lifecycle_log(config_path, shadow_lifecycle)
    label_rows = list(iter_jsonl(label_path))
    label_index = index_by_join_key(label_rows)
    lifecycle_by_candidate = lifecycle_index(lifecycle_path)
    quality_rows = build_quality_rows(v3_rows, label_index, lifecycle_by_candidate, success_pnl_pct)
    quality = summarize_quality(quality_rows)
    return {
        "status": "ok" if v3_rows else "no_v3_rows",
        "inputs": {
            "config_path": str(config_path),
            "decisions_log": str(primary_log),
            "outcome_labels": str(label_path) if label_path else None,
            "shadow_lifecycle": str(lifecycle_path) if lifecycle_path else None,
            "success_pnl_pct": success_pnl_pct,
        },
        "data_quality": {
            "decision_rows": len(decision_rows),
            "bad_decision_rows": bad_rows,
            "v3_rows": len(v3_rows),
            "outcome_label_rows": len(label_rows),
            "outcome_label_join_keys": len(label_index),
            "lifecycle_rows": len(lifecycle_by_candidate),
        },
        "quality": quality,
        "sample_rows": quality_rows[:25],
        "runtime_contract": {
            "fsc_de_scoped": True,
            "primary_only_validation": True,
            "active_policy_changed": False,
            "promotion_activated": False,
            "no_p2_promotion": True,
        },
    }


def print_text(report: dict[str, Any]) -> None:
    quality = report["quality"]
    sponsor = quality["sponsor_summary"]
    print(f"status={report['status']}")
    print(f"p3_5_status={quality['p3_5_status']}")
    print(f"v3_rows={quality['v3_rows']}")
    print(f"outcome_label_coverage={quality['outcome_label_coverage']}")
    print(f"avoided_bad_entries={sponsor['avoided_bad_entries']}")
    print(f"blocked_good_entries={sponsor['blocked_good_entries']}")
    print(f"neutral_entries={sponsor['neutral_entries']}")
    print(f"inconclusive={sponsor['inconclusive']}")


def main() -> None:
    args = parse_args()
    report = build_report(
        args.config,
        args.decisions_log,
        args.outcome_labels,
        args.shadow_lifecycle,
        args.success_pnl_pct,
    )
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_text(report)


if __name__ == "__main__":
    main()
