#!/usr/bin/env python3
"""
Build P3.7 Outcome Label v2 rows.

This is an additive offline labeler. It does not rewrite v1 labels, decision
logs, runtime evidence, or Gatekeeper policy. The conservative rule is that a
v1 +40 outcome without a real price/lifecycle path is not promoted to
good_clean. It remains good_dirty until P3.7 can verify MFE/MAE/time-path and
execution feasibility.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

import gatekeeper_outcome_labeler as v1


LABEL_V2_SCHEMA_VERSION = 1
DEFAULT_TARGET_PCT = 40.0
DEFAULT_STOP_PCT = 40.0
DEFAULT_DIRTY_MAE_PCT = -40.0
PRICE_PATH_USABLE_STATUSES = {"ok", "partial"}


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    yield from v1.iter_json_objects(path)


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def bool_or_none(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def price_path_samples(row: dict[str, Any]) -> list[dict[str, Any]]:
    raw = row.get("price_path_samples") or row.get("lifecycle_price_samples") or row.get("samples")
    if not isinstance(raw, list):
        return []
    samples: list[dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            continue
        ts_ms = v1.int_or_none(item.get("ts_ms") or item.get("timestamp_ms"))
        return_pct = v1.float_or_none(item.get("return_pct"))
        price = v1.float_or_none(item.get("price_sol") or item.get("price"))
        if ts_ms is None or return_pct is None:
            continue
        samples.append({"ts_ms": ts_ms, "return_pct": return_pct, "price": price})
    samples.sort(key=lambda item: item["ts_ms"])
    return samples


def usable_price_path(row: dict[str, Any] | None) -> bool:
    if not row:
        return False
    return row.get("path_status") in PRICE_PATH_USABLE_STATUSES and bool(price_path_samples(row))


def select_sample_source(
    threshold: dict[str, Any] | None,
    price_path: dict[str, Any] | None,
) -> tuple[dict[str, Any] | None, str, str | None, str | None]:
    if price_path is not None:
        status = price_path.get("path_status") if isinstance(price_path.get("path_status"), str) else None
        unknown_reason = (
            price_path.get("unknown_reason") if isinstance(price_path.get("unknown_reason"), str) else None
        )
        if usable_price_path(price_path):
            source = price_path.get("path_source") if isinstance(price_path.get("path_source"), str) else None
            return price_path, source or "external_price_path_samples", status, unknown_reason
        return None, "none", status, unknown_reason

    if threshold is not None and price_path_samples(threshold):
        return threshold, "price_path_samples", None, None

    return None, "none", None, None


def path_stats(
    samples: list[dict[str, Any]],
    entry_ts_ms: int | None,
    target_pct: float,
    stop_pct: float,
    source: str,
) -> dict[str, Any]:
    if not samples or entry_ts_ms is None:
        return {
            "source": "none",
            "mfe_pct_10s": None,
            "mfe_pct_30s": None,
            "mfe_pct_60s": None,
            "mae_pct_10s": None,
            "mae_pct_30s": None,
            "mae_pct_60s": None,
            "time_to_mfe_ms": None,
            "time_to_mae_ms": None,
            "hit_plus_20": None,
            "hit_plus_40": None,
            "hit_plus_60": None,
            "hit_stop_20": None,
            "hit_stop_40": None,
            "survived_10s": None,
            "survived_30s": None,
            "survived_60s": None,
            "drawdown_before_plus40": None,
        }

    post = [item for item in samples if item["ts_ms"] >= entry_ts_ms]
    if not post:
        return path_stats([], None, target_pct, stop_pct)

    def window_values(ms: int) -> list[float]:
        end = entry_ts_ms + ms
        return [item["return_pct"] for item in post if item["ts_ms"] <= end]

    def max_in_window(ms: int) -> float | None:
        vals = window_values(ms)
        return max(vals) if vals else None

    def min_in_window(ms: int) -> float | None:
        vals = window_values(ms)
        return min(vals) if vals else None

    all_returns = [item["return_pct"] for item in post]
    max_return = max(all_returns)
    min_return = min(all_returns)
    time_to_mfe_ms = next(
        item["ts_ms"] - entry_ts_ms for item in post if item["return_pct"] == max_return
    )
    time_to_mae_ms = next(
        item["ts_ms"] - entry_ts_ms for item in post if item["return_pct"] == min_return
    )
    plus40_index = next(
        (idx for idx, item in enumerate(post) if item["return_pct"] >= target_pct),
        None,
    )
    drawdown_before_plus40 = (
        min(item["return_pct"] for item in post[: plus40_index + 1])
        if plus40_index is not None
        else None
    )

    return {
        "source": source,
        "mfe_pct_10s": max_in_window(10_000),
        "mfe_pct_30s": max_in_window(30_000),
        "mfe_pct_60s": max_in_window(60_000),
        "mae_pct_10s": min_in_window(10_000),
        "mae_pct_30s": min_in_window(30_000),
        "mae_pct_60s": min_in_window(60_000),
        "time_to_mfe_ms": time_to_mfe_ms,
        "time_to_mae_ms": time_to_mae_ms,
        "hit_plus_20": max_return >= 20.0,
        "hit_plus_40": max_return >= target_pct,
        "hit_plus_60": max_return >= 60.0,
        "hit_stop_20": min_return <= -20.0,
        "hit_stop_40": min_return <= -abs(stop_pct),
        "survived_10s": min_in_window(10_000) is not None and min_in_window(10_000) > -abs(stop_pct),
        "survived_30s": min_in_window(30_000) is not None and min_in_window(30_000) > -abs(stop_pct),
        "survived_60s": min_in_window(60_000) is not None and min_in_window(60_000) > -abs(stop_pct),
        "drawdown_before_plus40": drawdown_before_plus40,
    }


def v1_class(label: dict[str, Any]) -> str:
    if not label.get("label_valid"):
        return "unknown"
    if label.get("hit_40_before_stop") is True:
        return "good_entry"
    if label.get("rug_or_early_death") is True:
        return "bad_entry"
    return "neutral_entry"


def entry_confidence(label: dict[str, Any]) -> str:
    if label.get("entry_price_sol") is None:
        return "missing"
    if label.get("entry_match_usable") and label.get("entry_match_causal"):
        return "usable_causal_match"
    if label.get("entry_match_usable"):
        return "usable_noncausal_match"
    return "unusable_match"


def classify_market_outcome(
    label: dict[str, Any],
    stats: dict[str, Any],
    dirty_mae_pct: float,
) -> tuple[str, str | None, str]:
    if not label.get("label_valid"):
        return "unknown", label.get("label_invalid_reason") or "invalid_label_v1", "invalid"

    has_path = stats["source"] != "none"
    mae_60 = v1.float_or_none(stats.get("mae_pct_60s"))

    if label.get("hit_40_before_stop") is True:
        if has_path and mae_60 is not None and mae_60 > dirty_mae_pct:
            return "good_clean", None, "clean_price_path"
        return "good_dirty", "missing_price_path_for_good_clean", "dirty_threshold_summary"

    if label.get("rug_or_early_death") is True:
        if has_path or label.get("max_adverse_return_pct") is not None:
            return "bad_clean", None, "clean_threshold_summary"
        return "bad_dirty", "missing_adverse_path_details", "dirty_threshold_summary"

    return "neutral_clean", None, "clean_threshold_summary"


def build_label_v2(
    decision: dict[str, Any],
    threshold: dict[str, Any] | None,
    price_path: dict[str, Any] | None = None,
    *,
    target_pct: float,
    stop_pct: float,
    dirty_mae_pct: float,
) -> dict[str, Any]:
    label = v1.threshold_label(decision, threshold, target_pct, stop_pct)
    sample_row, sample_source, price_path_status, price_path_unknown_reason = select_sample_source(
        threshold,
        price_path,
    )
    samples = price_path_samples(sample_row or {})
    stats = path_stats(samples, label.get("entry_ts_ms"), target_pct, stop_pct, sample_source)
    market_class, unknown_reason, label_quality = classify_market_outcome(
        label,
        stats,
        dirty_mae_pct,
    )

    threshold_status = threshold.get("threshold_status") if threshold else "missing_threshold_row"
    threshold_verdict = threshold.get("threshold_verdict") if threshold else None
    max_return = label.get("max_executable_return_pct")
    min_return = label.get("max_adverse_return_pct")

    return {
        "label_v2_schema_version": LABEL_V2_SCHEMA_VERSION,
        "source_label_schema_version": label.get("label_schema_version"),
        "ab_record_id": decision.get("ab_record_id"),
        "join_key": decision.get("join_key"),
        "pool_id": decision.get("pool_id"),
        "base_mint": decision.get("base_mint"),
        "entry_price": label.get("entry_price_sol"),
        "entry_price_source": "hypothetical_entry_price_sol" if label.get("entry_price_sol") is not None else None,
        "entry_price_confidence": entry_confidence(label),
        "entry_ts_ms": label.get("entry_ts_ms"),
        "entry_match_delta_ms": label.get("entry_match_delta_ms"),
        "entry_match_causal": label.get("entry_match_causal"),
        "entry_match_usable": label.get("entry_match_usable"),
        "threshold_status": threshold_status,
        "threshold_verdict": threshold_verdict,
        "threshold_monitor_window_s": threshold.get("threshold_monitor_window_s") if threshold else None,
        "threshold_monitor_window_deadline_s": threshold.get("threshold_monitor_window_deadline_s") if threshold else None,
        "threshold_window_max_return_pct": max_return,
        "threshold_window_min_return_pct": min_return,
        "price_path_source": stats["source"],
        "price_path_status": price_path_status,
        "price_path_unknown_reason": price_path_unknown_reason,
        "mfe_pct_10s": stats["mfe_pct_10s"],
        "mfe_pct_30s": stats["mfe_pct_30s"],
        "mfe_pct_60s": stats["mfe_pct_60s"],
        "mae_pct_10s": stats["mae_pct_10s"],
        "mae_pct_30s": stats["mae_pct_30s"],
        "mae_pct_60s": stats["mae_pct_60s"],
        "time_to_mfe_ms": stats["time_to_mfe_ms"],
        "time_to_mae_ms": stats["time_to_mae_ms"],
        "hit_plus_20": stats["hit_plus_20"] if stats["hit_plus_20"] is not None else (
            max_return is not None and max_return >= 20.0 if label.get("label_valid") else None
        ),
        "hit_plus_40": stats["hit_plus_40"] if stats["hit_plus_40"] is not None else label.get("hit_40"),
        "hit_plus_60": stats["hit_plus_60"] if stats["hit_plus_60"] is not None else (
            max_return is not None and max_return >= 60.0 if label.get("label_valid") else None
        ),
        "hit_stop_20": stats["hit_stop_20"] if stats["hit_stop_20"] is not None else (
            min_return is not None and min_return <= -20.0 if label.get("label_valid") else None
        ),
        "hit_stop_40": stats["hit_stop_40"] if stats["hit_stop_40"] is not None else (
            min_return is not None and min_return <= -abs(stop_pct) if label.get("label_valid") else None
        ),
        "survived_10s": stats["survived_10s"],
        "survived_30s": stats["survived_30s"],
        "survived_60s": stats["survived_60s"],
        "drawdown_before_plus40": stats["drawdown_before_plus40"],
        "exit_feasible": None,
        "label_quality": label_quality,
        "unknown_reason": unknown_reason,
        "market_outcome_class": market_class,
        "execution_quality_class": "pending_p3_7_3_execution_join",
        "decision_quality_class": "pending_p3_7_3_execution_join",
        "v1_outcome_class": v1_class(label),
        "label_valid_v1": label.get("label_valid"),
        "hit_40_before_stop_v1": label.get("hit_40_before_stop"),
        "rug_or_early_death_v1": label.get("rug_or_early_death"),
        "decision_verdict": label.get("decision_verdict"),
        "decision_reason": label.get("decision_reason"),
        "v3_shadow_verdict": decision.get("v3_shadow_verdict"),
        "v3_shadow_reason_code": decision.get("v3_shadow_reason_code"),
    }


def build_labels(
    decisions_path: Path,
    threshold_path: Path,
    output_path: Path,
    *,
    target_pct: float,
    stop_pct: float,
    dirty_mae_pct: float,
    observation_window_ms: set[int] | None = None,
    price_path_samples_path: Path | None = None,
) -> dict[str, Any]:
    decisions = list(iter_jsonl(decisions_path))
    thresholds = list(iter_jsonl(threshold_path))
    price_paths = list(iter_jsonl(price_path_samples_path)) if price_path_samples_path is not None else []
    if observation_window_ms is not None:
        decisions = [
            row
            for row in decisions
            if v1.int_or_none(row.get("observation_window_ms")) in observation_window_ms
        ]
        thresholds = [
            row
            for row in thresholds
            if v1.int_or_none(row.get("observation_window_ms")) in observation_window_ms
        ]
        price_paths = [
            row
            for row in price_paths
            if v1.int_or_none(row.get("observation_window_ms")) in observation_window_ms
        ]

    threshold_by_key = v1.index_rows(thresholds)
    price_path_by_key = v1.index_rows(price_paths)
    rows: list[dict[str, Any]] = []
    transitions: Counter[str] = Counter()
    market_counts: Counter[str] = Counter()
    quality_counts: Counter[str] = Counter()
    unknown_reasons: Counter[str] = Counter()
    price_path_sources: Counter[str] = Counter()

    for decision in decisions:
        threshold = v1.best_match(decision, threshold_by_key)
        price_path = v1.best_match(decision, price_path_by_key)
        row = build_label_v2(
            decision,
            threshold,
            price_path,
            target_pct=target_pct,
            stop_pct=stop_pct,
            dirty_mae_pct=dirty_mae_pct,
        )
        rows.append(row)
        market_counts[row["market_outcome_class"]] += 1
        quality_counts[row["label_quality"]] += 1
        price_path_sources[row["price_path_source"]] += 1
        transitions[f"{row['v1_outcome_class']}->{row['market_outcome_class']}"] += 1
        if row["unknown_reason"]:
            unknown_reasons[row["unknown_reason"]] += 1

    write_jsonl(output_path, rows)
    return {
        "status": "ok",
        "label_v2_schema_version": LABEL_V2_SCHEMA_VERSION,
        "decisions": len(decisions),
        "threshold_rows": len(thresholds),
        "price_path_rows": len(price_paths),
        "written": len(rows),
        "market_outcome_class_counts": dict(sorted(market_counts.items())),
        "label_quality_counts": dict(sorted(quality_counts.items())),
        "price_path_source_counts": dict(sorted(price_path_sources.items())),
        "v1_to_v2_transition_counts": dict(sorted(transitions.items())),
        "unknown_reason_counts": dict(sorted(unknown_reasons.items())),
        "output": str(output_path),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decisions", required=True, type=Path, help="gatekeeper_v2_decisions.jsonl")
    parser.add_argument("--threshold-hits", required=True, type=Path, help="threshold hits or price-path JSONL")
    parser.add_argument("--output", required=True, type=Path, help="output label v2 JSONL")
    parser.add_argument("--summary-output", type=Path, help="optional summary JSON")
    parser.add_argument("--price-path-samples", type=Path, help="optional P3.7 price path samples JSONL")
    parser.add_argument("--lifecycle", type=Path, help="reserved for P3.7.3 execution/lifecycle join")
    parser.add_argument("--shadow-entry", type=Path, help="reserved for P3.7.3 execution/lifecycle join")
    parser.add_argument("--target-pct", type=float, default=DEFAULT_TARGET_PCT)
    parser.add_argument("--stop-pct", type=float, default=DEFAULT_STOP_PCT)
    parser.add_argument("--dirty-mae-pct", type=float, default=DEFAULT_DIRTY_MAE_PCT)
    parser.add_argument(
        "--observation-window-ms",
        type=int,
        action="append",
        help="keep only records from this observation window; repeat for multiple windows",
    )
    return parser.parse_args()


def validate_reserved_inputs(lifecycle: Path | None, shadow_entry: Path | None) -> None:
    if lifecycle is not None or shadow_entry is not None:
        raise NotImplementedError(
            "--lifecycle and --shadow-entry are reserved for P3.7.3 execution feasibility join"
        )


def main() -> None:
    args = parse_args()
    validate_reserved_inputs(args.lifecycle, args.shadow_entry)
    summary = build_labels(
        args.decisions,
        args.threshold_hits,
        args.output,
        target_pct=args.target_pct,
        stop_pct=args.stop_pct,
        dirty_mae_pct=args.dirty_mae_pct,
        price_path_samples_path=args.price_path_samples,
        observation_window_ms=set(args.observation_window_ms)
        if args.observation_window_ms is not None
        else None,
    )
    if args.summary_output:
        args.summary_output.parent.mkdir(parents=True, exist_ok=True)
        args.summary_output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
