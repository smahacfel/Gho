#!/usr/bin/env python3
"""
Build Gatekeeper outcome labels for +40% calibration.

The script joins Gatekeeper decision rows with threshold outcome rows produced by
the offline threshold fetcher. It intentionally treats missing or non-causal
entry matches as invalid labels instead of filling them with optimistic defaults.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable


LABEL_SCHEMA_VERSION = 1
DEFAULT_TARGET_PCT = 40.0
DEFAULT_STOP_PCT = 40.0


def iter_json_objects(path: Path) -> Iterable[dict[str, Any]]:
    decoder = json.JSONDecoder()
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            index = 0
            while index < len(raw):
                try:
                    obj, next_index = decoder.raw_decode(raw, index)
                except json.JSONDecodeError:
                    break
                if isinstance(obj, dict):
                    yield obj
                index = next_index
                while index < len(raw) and raw[index].isspace():
                    index += 1


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def int_or_none(value: Any) -> int | None:
    return int(value) if isinstance(value, (int, float)) else None


def float_or_none(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def normalize_verdict(row: dict[str, Any]) -> str:
    verdict_type = row.get("verdict_type")
    if isinstance(verdict_type, str) and verdict_type:
        return verdict_type
    decision_buy = row.get("decision_verdict_buy")
    if isinstance(decision_buy, bool):
        return "BUY" if decision_buy else "REJECT"
    return "UNKNOWN"


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

    candidate_id = str_or_none(row.get("execution_candidate_id")) or str_or_none(row.get("candidate_id"))
    if candidate_id:
        keys.append(f"candidate:{candidate_id}")
    return keys


def index_rows(rows: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        for key in join_keys(row):
            indexed.setdefault(key, row)
    return indexed


def best_match(row: dict[str, Any], index: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
    for key in join_keys(row):
        match = index.get(key)
        if match is not None:
            return match
    return None


def threshold_label(
    decision: dict[str, Any],
    threshold: dict[str, Any] | None,
    target_pct: float,
    stop_pct: float,
) -> dict[str, Any]:
    verdict = normalize_verdict(decision)
    threshold_verdict = threshold.get("threshold_verdict") if threshold else None
    threshold_status = threshold.get("threshold_status") if threshold else "missing_threshold_row"

    entry_price = float_or_none(threshold.get("hypothetical_entry_price_sol")) if threshold else None
    entry_ts_ms = int_or_none(threshold.get("hypothetical_entry_target_ts_ms")) if threshold else None
    entry_match_delta_ms = (
        int_or_none(threshold.get("hypothetical_entry_match_delta_ms")) if threshold else None
    )
    entry_quality_usable = bool(threshold.get("analysis_entry_match_quality_usable")) if threshold else False
    entry_causal = isinstance(entry_match_delta_ms, int) and entry_match_delta_ms <= 0
    entry_usable = entry_quality_usable and entry_causal

    max_return_pct = float_or_none(threshold.get("threshold_window_max_return_pct")) if threshold else None
    min_return_pct = float_or_none(threshold.get("threshold_window_min_return_pct")) if threshold else None
    hit_after_entry_s = float_or_none(threshold.get("threshold_hit_after_entry_s")) if threshold else None

    hit_40 = bool(max_return_pct is not None and max_return_pct >= target_pct)
    stopped_before_40 = bool(min_return_pct is not None and min_return_pct <= -abs(stop_pct))
    threshold_ok = threshold_verdict == "OK"
    threshold_nok = threshold_verdict == "NOK"

    label_valid = bool(
        threshold is not None
        and threshold_status in {"ok", "no_threshold_hit", "no_post_entry_tx"}
        and entry_price is not None
        and entry_price > 0.0
        and entry_usable
    )

    hit_40_before_stop = bool(label_valid and threshold_ok and not stopped_before_40)
    rug_or_early_death = bool(label_valid and (threshold_nok or stopped_before_40))

    return {
        "label_schema_version": LABEL_SCHEMA_VERSION,
        "label_target_pct": target_pct,
        "label_stop_pct": abs(stop_pct),
        "label_valid": label_valid,
        "label_invalid_reason": None
        if label_valid
        else invalid_reason(threshold, threshold_status, entry_price, entry_usable),
        "hit_40": hit_40 if label_valid else None,
        "hit_40_before_stop": hit_40_before_stop if label_valid else None,
        "rug_or_early_death": rug_or_early_death if label_valid else None,
        "max_executable_return_pct": max_return_pct,
        "max_adverse_return_pct": min_return_pct,
        "min_return_before_40_pct": min_return_pct if hit_40_before_stop else None,
        "time_to_40_pct_ms": int(round(hit_after_entry_s * 1000.0))
        if hit_40_before_stop and hit_after_entry_s is not None
        else None,
        "entry_price_sol": entry_price,
        "entry_ts_ms": entry_ts_ms,
        "entry_match_delta_ms": entry_match_delta_ms,
        "entry_match_usable": entry_usable,
        "entry_match_causal": entry_causal,
        "entry_match_selection": threshold.get("hypothetical_entry_match_selection") if threshold else None,
        "threshold_verdict": threshold_verdict,
        "threshold_status": threshold_status,
        "decision_verdict": verdict,
        "decision_verdict_buy": verdict == "BUY",
        "decision_reason": decision.get("decision_reason"),
        "verdict_type": decision.get("verdict_type"),
    }


def invalid_reason(
    threshold: dict[str, Any] | None,
    threshold_status: Any,
    entry_price: float | None,
    entry_usable: bool,
) -> str:
    if threshold is None:
        return "missing_threshold_row"
    if entry_price is None or entry_price <= 0.0:
        return "missing_or_invalid_entry_price"
    if not entry_usable:
        return "entry_match_not_usable"
    return str(threshold_status or "threshold_status_invalid")


def lifecycle_index(path: Path | None) -> dict[str, dict[str, Any]]:
    if path is None:
        return {}
    rows = list(iter_json_objects(path))
    latest: dict[str, dict[str, Any]] = {}
    for row in rows:
        candidate_id = str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        if row.get("record_type") == "position_closed":
            latest[candidate_id] = row
    return latest


def attach_lifecycle(out: dict[str, Any], decision: dict[str, Any], lifecycle: dict[str, dict[str, Any]]) -> None:
    candidate_id = str_or_none(decision.get("execution_candidate_id")) or str_or_none(decision.get("candidate_id"))
    row = lifecycle.get(candidate_id) if candidate_id else None
    out["lifecycle_candidate_id"] = candidate_id
    out["lifecycle_close_reason"] = row.get("close_reason") if row else None
    out["lifecycle_final_pnl_pct"] = row.get("final_pnl_pct") if row else None
    out["lifecycle_net_pnl_sol"] = row.get("net_pnl_sol") if row else None


def build_labels(
    decisions_path: Path,
    threshold_path: Path,
    lifecycle_path: Path | None,
    output_path: Path,
    target_pct: float,
    stop_pct: float,
    observation_window_ms: set[int] | None,
) -> dict[str, int]:
    decisions = list(iter_json_objects(decisions_path))
    if observation_window_ms is not None:
        decisions = [
            row
            for row in decisions
            if int_or_none(row.get("observation_window_ms")) in observation_window_ms
        ]
    threshold_rows = list(iter_json_objects(threshold_path))
    if observation_window_ms is not None:
        threshold_rows = [
            row
            for row in threshold_rows
            if int_or_none(row.get("observation_window_ms")) in observation_window_ms
        ]
    threshold_by_key = index_rows(threshold_rows)
    lifecycle_by_candidate = lifecycle_index(lifecycle_path)

    counters = {
        "decisions": len(decisions),
        "threshold_rows": len(threshold_rows),
        "written": 0,
        "label_valid": 0,
        "hit_40_before_stop": 0,
        "rug_or_early_death": 0,
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8") as fh:
        for decision in decisions:
            threshold = best_match(decision, threshold_by_key)
            label = threshold_label(decision, threshold, target_pct, stop_pct)
            out = dict(decision)
            out.update(label)
            attach_lifecycle(out, decision, lifecycle_by_candidate)
            fh.write(json.dumps(out, ensure_ascii=False, sort_keys=True) + "\n")
            counters["written"] += 1
            if label["label_valid"]:
                counters["label_valid"] += 1
            if label["hit_40_before_stop"] is True:
                counters["hit_40_before_stop"] += 1
            if label["rug_or_early_death"] is True:
                counters["rug_or_early_death"] += 1

    return counters


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decisions", required=True, type=Path, help="gatekeeper_v2_decisions.jsonl")
    parser.add_argument("--threshold-hits", required=True, type=Path, help="pool_threshold_hits*.jsonl")
    parser.add_argument("--lifecycle", type=Path, help="optional shadow_lifecycle.jsonl")
    parser.add_argument("--output", required=True, type=Path, help="output labels JSONL")
    parser.add_argument("--target-pct", type=float, default=DEFAULT_TARGET_PCT)
    parser.add_argument("--stop-pct", type=float, default=DEFAULT_STOP_PCT)
    parser.add_argument(
        "--observation-window-ms",
        type=int,
        action="append",
        help="keep only records from this observation window; repeat for multiple windows",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    counters = build_labels(
        decisions_path=args.decisions,
        threshold_path=args.threshold_hits,
        lifecycle_path=args.lifecycle,
        output_path=args.output,
        target_pct=args.target_pct,
        stop_pct=args.stop_pct,
        observation_window_ms=set(args.observation_window_ms)
        if args.observation_window_ms is not None
        else None,
    )
    print(json.dumps(counters, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
