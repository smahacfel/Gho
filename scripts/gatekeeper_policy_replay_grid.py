#!/usr/bin/env python3
"""
Replay strict Gatekeeper policy candidates on labeled +40% outcomes.

The replay operates on emitted decision/label JSONL. It is not a replacement for
Rust policy tests; it is an offline grid to decide which candidate deserves a
shadow bake.
"""

from __future__ import annotations

import argparse
import itertools
import json
from pathlib import Path
from typing import Any


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
            if isinstance(row, dict) and row.get("label_valid") is True:
                yield row


def parse_float_list(value: str) -> list[float]:
    return [float(part.strip()) for part in value.split(",") if part.strip()]


def parse_int_list(value: str) -> list[int]:
    return [int(part.strip()) for part in value.split(",") if part.strip()]


def bool_value(value: Any) -> bool:
    return value is True


def num(row: dict[str, Any], key: str, default: float = 0.0) -> float:
    value = row.get(key)
    return float(value) if isinstance(value, (int, float)) else default


def has_degraded(row: dict[str, Any], prefix: str) -> bool:
    reasons = row.get("sybil_metric_degraded_reasons")
    return isinstance(reasons, list) and any(str(reason).startswith(prefix) for reason in reasons)


def base_viability(row: dict[str, Any]) -> bool:
    return (
        bool_value(row.get("core1_passed"))
        and bool_value(row.get("core2_passed"))
        and bool_value(row.get("core3_passed"))
        and row.get("hard_fail_reason") in (None, "")
    )


def alpha_pass(row: dict[str, Any], min_joint: float, min_momentum: float, min_demand: float) -> bool:
    if row.get("alpha_actionable") is False:
        return False
    return (
        num(row, "alpha_joint", 1.0) >= min_joint
        and num(row, "momentum", 1.0) >= min_momentum
        and num(row, "demand", 1.0) >= min_demand
    )


def sybil_combo_veto(row: dict[str, Any], enabled: bool) -> bool:
    if not enabled:
        return False
    flags = str(row.get("sybil_soft_flags") or "")
    low_des = "low_des" in flags
    low_sfd = "low_sfd" in flags
    high_dbia = "high_dbia" in flags
    low_ftdi = "low_ftdi" in flags
    high_cpv = "high_cpv" in flags and not has_degraded(row, "CPV_")
    high_fsc = "high_fsc" in flags and not has_degraded(row, "FSC_")
    return (
        (high_dbia and low_ftdi and low_sfd)
        or (low_des and low_sfd and (high_dbia or low_ftdi))
        or (high_fsc and high_cpv and (low_des or low_sfd))
    )


def prosperity_base(row: dict[str, Any]) -> bool:
    if row.get("prosperity_filter_enabled") is False:
        return True
    if row.get("prosperity_actionable") is False:
        return False
    if row.get("prosperity_pass") is False:
        return False
    mcap = row.get("current_market_cap_sol")
    if isinstance(mcap, (int, float)) and mcap < num(row, "prosperity_min_market_cap_sol", 45.0):
        return False
    cpv = row.get("signer_cross_pool_velocity")
    if isinstance(cpv, (int, float)) and cpv > num(row, "prosperity_max_signer_cross_pool_velocity", 0.5):
        return False
    return True


def prosperity_overlay(row: dict[str, Any], enabled: bool) -> bool:
    if not enabled:
        return True
    if row.get("prosperity_overlay_pass") is False:
        return False
    price_change = row.get("price_change_ratio")
    if isinstance(price_change, (int, float)) and price_change > num(row, "prosperity_overlay_max_price_change_ratio", 2.2):
        return False
    bonding = row.get("bonding_progress_pct")
    if isinstance(bonding, (int, float)) and bonding > num(row, "prosperity_overlay_max_bonding_progress_pct", 85.0):
        return False
    ftdi = row.get("fee_topology_diversity_index")
    if not isinstance(ftdi, (int, float)) or ftdi < num(row, "prosperity_overlay_min_fee_topology_diversity_index", 0.10):
        return False
    sell_buy = row.get("sell_buy_ratio")
    if isinstance(sell_buy, (int, float)) and sell_buy > num(row, "prosperity_overlay_branch23_max_sell_buy_ratio", 0.18):
        return False
    return True


def candidate_pass(row: dict[str, Any], policy: dict[str, Any]) -> bool:
    if not base_viability(row):
        return False
    if int(num(row, "legacy_soft_points")) > policy["max_soft_points"]:
        return False
    if int(num(row, "sybil_soft_points")) > policy["max_sybil_soft_points"]:
        return False
    if sybil_combo_veto(row, policy["sybil_combo_veto"]):
        return False
    if not alpha_pass(row, policy["min_alpha_joint"], policy["min_momentum"], policy["min_demand"]):
        return False
    if not prosperity_base(row):
        return False
    if not prosperity_overlay(row, policy["prosperity_overlay"]):
        return False
    return True


def summarize(rows: list[dict[str, Any]], policy: dict[str, Any]) -> dict[str, Any]:
    selected = [row for row in rows if candidate_pass(row, policy)]
    hits = sum(1 for row in selected if row.get("hit_40_before_stop") is True)
    rugs = sum(1 for row in selected if row.get("rug_or_early_death") is True)
    previous_buy = [row for row in rows if row.get("decision_verdict_buy") is True]
    previous_buy_keys = {row.get("join_key") or row.get("pool_id") for row in previous_buy}
    selected_keys = {row.get("join_key") or row.get("pool_id") for row in selected}
    return {
        **policy,
        "n": len(rows),
        "selected": len(selected),
        "hits": hits,
        "rugs": rugs,
        "precision": hits / len(selected) if selected else None,
        "rug_rate": rugs / len(selected) if selected else None,
        "coverage": len(selected) / len(rows) if rows else 0.0,
        "removed_previous_buys": len(previous_buy_keys - selected_keys),
        "new_selected": len(selected_keys - previous_buy_keys),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--labels", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--min-alpha-joint", default="0.2,0.3,0.4,0.5")
    parser.add_argument("--min-momentum", default="0.2")
    parser.add_argument("--min-demand", default="0.2,0.3")
    parser.add_argument("--max-soft-points", default="3,5,7,255")
    parser.add_argument("--max-sybil-soft-points", default="3,5,6")
    parser.add_argument("--top", type=int, default=20)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    rows = list(iter_jsonl(args.labels))
    policies = []
    for min_alpha_joint, min_momentum, min_demand, max_soft, max_sybil, overlay, combo in itertools.product(
        parse_float_list(args.min_alpha_joint),
        parse_float_list(args.min_momentum),
        parse_float_list(args.min_demand),
        parse_int_list(args.max_soft_points),
        parse_int_list(args.max_sybil_soft_points),
        (False, True),
        (False, True),
    ):
        policies.append(
            {
                "min_alpha_joint": min_alpha_joint,
                "min_momentum": min_momentum,
                "min_demand": min_demand,
                "max_soft_points": max_soft,
                "max_sybil_soft_points": max_sybil,
                "prosperity_overlay": overlay,
                "sybil_combo_veto": combo,
            }
        )
    results = [summarize(rows, policy) for policy in policies]
    results.sort(
        key=lambda row: (
            row["precision"] if row["precision"] is not None else -1.0,
            -(row["rug_rate"] if row["rug_rate"] is not None else 1.0),
            row["selected"],
        ),
        reverse=True,
    )
    report = {"input": str(args.labels), "rows": len(rows), "top": results[: args.top], "all": results}
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)


if __name__ == "__main__":
    main()
