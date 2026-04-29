#!/usr/bin/env python3
"""
Validate Gatekeeper +40% labels with walk-forward, permutation, bootstrap, and regimes.
"""

from __future__ import annotations

import argparse
import json
import random
from statistics import median
from pathlib import Path
from typing import Any, Callable


def iter_jsonl(path: Path):
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def bool_label(row: dict[str, Any], key: str) -> bool | None:
    value = row.get(key)
    return value if isinstance(value, bool) else None


def ts_ms(row: dict[str, Any]) -> int:
    for key in ("entry_ts_ms", "observation_end_ts_ms", "first_seen_ts_ms"):
        value = row.get(key)
        if isinstance(value, (int, float)):
            return int(value)
    return 0


def is_buy(row: dict[str, Any]) -> bool:
    value = row.get("decision_verdict_buy")
    if isinstance(value, bool):
        return value
    return row.get("verdict_type") == "BUY"


def precision(rows: list[dict[str, Any]], selector: Callable[[dict[str, Any]], bool]) -> float | None:
    selected = [row for row in rows if selector(row)]
    if not selected:
        return None
    hits = sum(1 for row in selected if bool_label(row, "hit_40_before_stop") is True)
    return hits / len(selected)


def rate(rows: list[dict[str, Any]], key: str, selector: Callable[[dict[str, Any]], bool]) -> float | None:
    selected = [row for row in rows if selector(row)]
    if not selected:
        return None
    positives = sum(1 for row in selected if bool_label(row, key) is True)
    return positives / len(selected)


def coverage(rows: list[dict[str, Any]], selector: Callable[[dict[str, Any]], bool]) -> float:
    if not rows:
        return 0.0
    return sum(1 for row in rows if selector(row)) / len(rows)


def median_time_to_40(rows: list[dict[str, Any]], selector: Callable[[dict[str, Any]], bool]) -> float | None:
    values = [
        float(row["time_to_40_pct_ms"])
        for row in rows
        if selector(row) and isinstance(row.get("time_to_40_pct_ms"), (int, float))
    ]
    return median(values) if values else None


def metrics(rows: list[dict[str, Any]], selector: Callable[[dict[str, Any]], bool] = is_buy) -> dict[str, Any]:
    selected_count = sum(1 for row in rows if selector(row))
    hits = sum(1 for row in rows if selector(row) and bool_label(row, "hit_40_before_stop") is True)
    return {
        "n": len(rows),
        "selected": selected_count,
        "hits": hits,
        "precision": precision(rows, selector),
        "coverage": coverage(rows, selector),
        "rug_rate": rate(rows, "rug_or_early_death", selector),
        "median_time_to_40_ms": median_time_to_40(rows, selector),
    }


def bootstrap_ci(
    rows: list[dict[str, Any]],
    selector: Callable[[dict[str, Any]], bool],
    iterations: int,
    seed: int,
) -> dict[str, float | None]:
    if not rows or iterations <= 0:
        return {"precision_p05": None, "precision_p50": None, "precision_p95": None}
    rng = random.Random(seed)
    values: list[float] = []
    for _ in range(iterations):
        sample = [rows[rng.randrange(len(rows))] for _ in rows]
        value = precision(sample, selector)
        if value is not None:
            values.append(value)
    if not values:
        return {"precision_p05": None, "precision_p50": None, "precision_p95": None}
    values.sort()
    return {
        "precision_p05": percentile(values, 0.05),
        "precision_p50": percentile(values, 0.50),
        "precision_p95": percentile(values, 0.95),
    }


def percentile(values: list[float], q: float) -> float:
    if not values:
        raise ValueError("empty values")
    idx = min(len(values) - 1, max(0, round((len(values) - 1) * q)))
    return values[idx]


def walk_forward(rows: list[dict[str, Any]], folds: int) -> list[dict[str, Any]]:
    if folds <= 1 or len(rows) < folds:
        return []
    ordered = sorted(rows, key=ts_ms)
    fold_size = max(1, len(ordered) // folds)
    out: list[dict[str, Any]] = []
    for fold in range(1, folds):
        test_start = fold * fold_size
        test_end = (fold + 1) * fold_size if fold < folds - 1 else len(ordered)
        train = ordered[:test_start]
        test = ordered[test_start:test_end]
        out.append(
            {
                "fold": fold,
                "train": metrics(train),
                "test": metrics(test),
            }
        )
    return out


def permutation_test(rows: list[dict[str, Any]], iterations: int, seed: int) -> dict[str, Any]:
    selected = [row for row in rows if is_buy(row)]
    if not selected or iterations <= 0:
        return {"observed_precision": None, "permuted_mean_precision": None, "p_value": None}
    labels = [bool_label(row, "hit_40_before_stop") is True for row in rows]
    observed = precision(rows, is_buy)
    rng = random.Random(seed)
    permuted_values: list[float] = []
    selected_indices = [idx for idx, row in enumerate(rows) if is_buy(row)]
    for _ in range(iterations):
        shuffled = labels[:]
        rng.shuffle(shuffled)
        hits = sum(1 for idx in selected_indices if shuffled[idx])
        permuted_values.append(hits / len(selected_indices))
    p_value = None
    if observed is not None:
        p_value = sum(1 for value in permuted_values if value >= observed) / len(permuted_values)
    return {
        "observed_precision": observed,
        "permuted_mean_precision": sum(permuted_values) / len(permuted_values),
        "p_value": p_value,
    }


def bucket(row: dict[str, Any], key: str) -> str:
    if key == "dev_known":
        value = row.get("dev_wallet_known")
        return "known" if value is True else "unknown"
    if key == "cpv_ready":
        degraded = row.get("sybil_metric_degraded_reasons")
        if isinstance(degraded, list) and any("CPV_" in str(reason) for reason in degraded):
            return "degraded"
        return "ready"
    if key == "bonding":
        value = row.get("bonding_progress_pct")
        if not isinstance(value, (int, float)):
            return "unknown"
        if value < 40:
            return "lt40"
        if value < 70:
            return "40_70"
        return "gte70"
    if key == "mcap":
        value = row.get("current_market_cap_sol")
        if not isinstance(value, (int, float)):
            return "unknown"
        if value < 45:
            return "lt45"
        if value < 60:
            return "45_60"
        return "gte60"
    return "all"


def regimes(rows: list[dict[str, Any]]) -> dict[str, dict[str, dict[str, Any]]]:
    out: dict[str, dict[str, dict[str, Any]]] = {}
    for key in ("dev_known", "cpv_ready", "bonding", "mcap"):
        groups: dict[str, list[dict[str, Any]]] = {}
        for row in rows:
            groups.setdefault(bucket(row, key), []).append(row)
        out[key] = {name: metrics(group) for name, group in sorted(groups.items())}
    return out


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--labels", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--folds", type=int, default=5)
    parser.add_argument("--bootstrap", type=int, default=1000)
    parser.add_argument("--permutations", type=int, default=1000)
    parser.add_argument("--seed", type=int, default=42)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    rows = [row for row in iter_jsonl(args.labels) if row.get("label_valid") is True]
    report = {
        "input": str(args.labels),
        "overall": metrics(rows),
        "bootstrap": bootstrap_ci(rows, is_buy, args.bootstrap, args.seed),
        "permutation": permutation_test(rows, args.permutations, args.seed),
        "walk_forward": walk_forward(rows, args.folds),
        "regimes": regimes(rows),
    }
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)


if __name__ == "__main__":
    main()
