#!/usr/bin/env python3
"""Offline Gatekeeper policy autopsy against R2 outcomes.

This analysis is diagnostic-only.  It distinguishes toxic concentration from
concentrated early demand and evaluates counterfactual policy variants without
changing runtime, Gatekeeper, execution, send path, or thresholds.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Callable

import audit_gatekeeper_decision_vs_r2 as gk_r2
import selector_pipeline_common as common


ARTIFACT = "gatekeeper_r2_policy_autopsy_v1"
MD_ARTIFACT = "GATEKEEPER_R2_POLICY_AUTOPSY.md"
TOP3_BREAKDOWN_CSV = "hard_fail_extreme_top3_positive_negative_breakdown_v1.csv"
CORE_BREAKDOWN_CSV = "core_fail_axis_breakdown_v1.csv"
IWIM_BREAKDOWN_CSV = "iwim_low_conf_breakdown_v1.csv"
FALSE_BUY_CSV = "false_buy_breakdown_v1.csv"
VARIANTS_CSV = "counterfactual_policy_variants_v1.csv"
DEFAULT_SELECTOR_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r23-score-tail-v1-r1-cutoff-check2"
DEFAULT_RUNTIME_SCOPE = "shadow-burnin-v3-score-tail-v1-r1-cutoff-check2-snapshot"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--selector-scope", default=DEFAULT_SELECTOR_SCOPE)
    parser.add_argument("--runtime-scope", default=DEFAULT_RUNTIME_SCOPE)
    parser.add_argument("--decision-plane", default="legacy_live", choices=("legacy_live", "v25_shadow"))
    parser.add_argument("--nearest-tolerance-ms", type=int, default=120_000)
    parser.add_argument("--min-anti-signal-lift-pp", type=float, default=0.05)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--top3-output", default=None)
    parser.add_argument("--core-output", default=None)
    parser.add_argument("--iwim-output", default=None)
    parser.add_argument("--false-buy-output", default=None)
    parser.add_argument("--variants-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def report_dir(root: Path, selector_scope: str) -> Path:
    return root / "reports" / "selector" / selector_scope


def dataset_dir(root: Path, selector_scope: str) -> Path:
    return root / "datasets" / "selector" / selector_scope


def default_outputs(root: Path, selector_scope: str, args: argparse.Namespace) -> dict[str, Path]:
    out_dir = report_dir(root, selector_scope)
    return {
        "json": Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json",
        "md": Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT,
        "hard_fail_extreme_top3": Path(args.top3_output) if args.top3_output else out_dir / TOP3_BREAKDOWN_CSV,
        "core_fail_axis": Path(args.core_output) if args.core_output else out_dir / CORE_BREAKDOWN_CSV,
        "iwim_low_conf": Path(args.iwim_output) if args.iwim_output else out_dir / IWIM_BREAKDOWN_CSV,
        "false_buy": Path(args.false_buy_output) if args.false_buy_output else out_dir / FALSE_BUY_CSV,
        "counterfactual_variants": Path(args.variants_output) if args.variants_output else out_dir / VARIANTS_CSV,
    }


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def numeric(row: dict[str, Any] | None, *fields: str) -> float | None:
    if row is None:
        return None
    for field in fields:
        value = row.get(field)
        if isinstance(value, bool):
            return 1.0 if value else 0.0
        value = common.float_or_none(value)
        if value is not None:
            return value
    return None


def bool_value(row: dict[str, Any] | None, field: str) -> bool | None:
    if row is None:
        return None
    value = row.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return bool(value)
    return None


def funding_status(row: dict[str, Any]) -> str:
    funding = row.get("funding_source_v2")
    if isinstance(funding, dict):
        return str(funding.get("status") or funding.get("excluded_reason") or "unknown")
    return "missing"


def reason_code(row: dict[str, Any]) -> str:
    decision = row["decision"]
    return str(
        decision.get("reason_code")
        or decision.get("gatekeeper_first_kill_reason")
        or decision.get("verdict_type")
        or ""
    )


def is_resolved(row: dict[str, Any]) -> bool:
    return row.get("r2_class") in {"positive", "negative"}


def is_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_class") == "positive"


def is_negative(row: dict[str, Any]) -> bool:
    return row.get("r2_class") == "negative"


def rate(num: int, den: int) -> float | None:
    return num / den if den else None


def decision_metric(row: dict[str, Any], *fields: str) -> float | None:
    return numeric(row.get("decision"), *fields)


def concentration_extreme(row: dict[str, Any]) -> bool:
    decision = row["decision"]
    rc = reason_code(row)
    return (
        "EXTREME_TOP3" in rc
        or (decision_metric(row, "top3_volume_pct", "max_top3_volume_pct") or 0.0) >= 0.99
        or (decision_metric(row, "early_top3_buy_volume_pct_3s") or 0.0) >= 0.99
    )


def toxicity_reasons(row: dict[str, Any]) -> list[str]:
    decision = row["decision"]
    reasons: list[str] = []
    buy_ratio = decision_metric(row, "buy_ratio", "fixed_size_buy_ratio")
    sell_buy = decision_metric(row, "sell_buy_ratio")
    sell_share = decision_metric(row, "sell_share")
    dev_volume = decision_metric(row, "dev_volume_ratio")
    dev_tx = decision_metric(row, "dev_tx_ratio")
    flip = decision_metric(row, "flip_ratio_10s")
    flipper = decision_metric(row, "flipper_presence_ratio")
    same_ms = decision_metric(row, "same_ms_tx_ratio", "max_same_ms_tx_ratio")
    unique = decision_metric(row, "ab_unique_signers_window")
    volume = decision_metric(row, "total_volume_sol")

    if buy_ratio is not None and buy_ratio < 0.50:
        reasons.append("low_buy_ratio")
    if sell_buy is not None and sell_buy >= 1.20:
        reasons.append("high_sell_buy_ratio")
    if sell_share is not None and sell_share >= 0.50:
        reasons.append("high_sell_share")
    if bool_value(decision, "dev_has_sold") is True:
        reasons.append("dev_has_sold")
    if dev_volume is not None and dev_volume >= 0.50:
        reasons.append("high_dev_volume_ratio")
    if dev_tx is not None and dev_tx >= 0.50:
        reasons.append("high_dev_tx_ratio")
    if flip is not None and flip >= 0.40:
        reasons.append("high_flip_ratio_10s")
    if flipper is not None and flipper >= 0.50:
        reasons.append("high_flipper_presence")
    if same_ms is not None and same_ms >= 0.85:
        reasons.append("same_ms_cluster")
    if unique is not None and unique < 2:
        reasons.append("too_few_unique_signers")
    if volume is not None and volume < 0.10:
        reasons.append("low_total_volume")
    if funding_status(decision) in {"clean"}:
        pass
    elif funding_status(decision) not in {"missing", "unavailable", "funding_lane_unavailable"}:
        reasons.append(f"funding_status:{funding_status(decision)}")
    return sorted(set(reasons))


def demand_reasons(row: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    buy_ratio = decision_metric(row, "buy_ratio", "fixed_size_buy_ratio")
    early_buy_dom = decision_metric(row, "early_slot_volume_dominance_buy")
    volume = decision_metric(row, "total_volume_sol")
    buyers = decision_metric(row, "ab_unique_signers_window")
    buy_count = decision_metric(row, "buy_count")
    if buy_ratio is not None and buy_ratio >= 0.55:
        reasons.append("buy_ratio_ge_0_55")
    if early_buy_dom is not None and early_buy_dom >= 0.25:
        reasons.append("early_buy_volume_dominance")
    if volume is not None and volume >= 0.25:
        reasons.append("volume_ge_0_25_sol")
    if buyers is not None and buyers >= 2:
        reasons.append("unique_signers_ge_2")
    if buy_count is not None and buy_count >= 2:
        reasons.append("buy_count_ge_2")
    return reasons


def concentration_segment(row: dict[str, Any]) -> str:
    if not concentration_extreme(row):
        return "not_extreme_concentration"
    toxic = toxicity_reasons(row)
    demand = demand_reasons(row)
    if toxic:
        return "toxic_concentration"
    if len(demand) >= 2:
        return "concentrated_early_demand"
    return "concentration_ambiguous"


def core_axis(row: dict[str, Any]) -> str:
    decision = row["decision"]
    values = []
    for field in ("core1_passed", "core2_passed", "core3_passed", "core_pass"):
        value = bool_value(decision, field)
        values.append(f"{field}={value}")
    return "|".join(values)


def row_summary(row: dict[str, Any]) -> dict[str, Any]:
    decision = row["decision"]
    return {
        "candidate_id": row.get("candidate_id"),
        "pool_id": row.get("pool_id"),
        "base_mint": row.get("base_mint"),
        "decision_bucket": row.get("decision_bucket"),
        "verdict_type": row.get("verdict_type"),
        "reason_code": reason_code(row),
        "r2_class": row.get("r2_class"),
        "concentration_segment": concentration_segment(row),
        "toxicity_reasons": ";".join(toxicity_reasons(row)),
        "demand_reasons": ";".join(demand_reasons(row)),
        "top3_volume_pct": decision_metric(row, "top3_volume_pct", "max_top3_volume_pct"),
        "hhi": decision_metric(row, "hhi", "max_hhi"),
        "early_top3_buy_volume_pct_3s": decision_metric(row, "early_top3_buy_volume_pct_3s"),
        "early_slot_volume_dominance_buy": decision_metric(row, "early_slot_volume_dominance_buy"),
        "buy_ratio": decision_metric(row, "buy_ratio", "fixed_size_buy_ratio"),
        "sell_buy_ratio": decision_metric(row, "sell_buy_ratio"),
        "sell_share": decision_metric(row, "sell_share"),
        "dev_has_sold": bool_value(decision, "dev_has_sold"),
        "dev_volume_ratio": decision_metric(row, "dev_volume_ratio"),
        "dev_tx_ratio": decision_metric(row, "dev_tx_ratio"),
        "flip_ratio_10s": decision_metric(row, "flip_ratio_10s"),
        "flipper_presence_ratio": decision_metric(row, "flipper_presence_ratio"),
        "total_volume_sol": decision_metric(row, "total_volume_sol"),
        "ab_unique_signers_window": decision_metric(row, "ab_unique_signers_window"),
        "max_favorable_pnl_pct": numeric(row.get("r2"), "max_favorable_pnl_pct"),
        "max_adverse_pnl_pct": numeric(row.get("r2"), "max_adverse_pnl_pct"),
    }


def aggregate(rows: list[dict[str, Any]], group_key: Callable[[dict[str, Any]], str]) -> list[dict[str, Any]]:
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        groups[group_key(row)].append(row)
    out: list[dict[str, Any]] = []
    for key, group in sorted(groups.items()):
        resolved = [row for row in group if is_resolved(row)]
        positives = sum(1 for row in resolved if is_positive(row))
        negatives = sum(1 for row in resolved if is_negative(row))
        out.append(
            {
                "group": key,
                "total_rows": len(group),
                "resolved_rows": len(resolved),
                "r2_positive_rows": positives,
                "r2_negative_rows": negatives,
                "positive_rate_resolved": rate(positives, len(resolved)),
                "negative_rate_resolved": rate(negatives, len(resolved)),
                "unresolved_rows": len(group) - len(resolved),
            }
        )
    return out


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if fieldnames is None:
        fieldnames = sorted({key for row in rows for key in row})
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def join_rows(args: argparse.Namespace) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    root = Path(args.root)
    ds_dir = dataset_dir(root, args.selector_scope)
    candidate_path = ds_dir / "candidate_universe_v1.jsonl"
    r2_path = ds_dir / "r2_market_paths_v1.jsonl"
    candidates, indexes = gk_r2.load_candidates(candidate_path)
    r2_by_candidate = gk_r2.load_index(r2_path)
    decision_paths = gk_r2.decision_paths(root, args.runtime_scope, args.decision_plane)
    rows: list[dict[str, Any]] = []
    join_counts: Counter[str] = Counter()
    unmatched = 0
    for path in decision_paths:
        for decision in gk_r2.read_jsonl(path):
            candidate, method = gk_r2.join_candidate(decision, indexes, int(args.nearest_tolerance_ms))
            join_counts[method] += 1
            if candidate is None:
                unmatched += 1
                continue
            candidate_id = common.str_or_none(candidate.get("candidate_id"))
            r2 = r2_by_candidate.get(candidate_id or "")
            rows.append(
                {
                    "candidate_id": candidate_id,
                    "pool_id": gk_r2.row_pool(candidate) or gk_r2.row_pool(decision),
                    "base_mint": gk_r2.row_mint(candidate) or gk_r2.row_mint(decision),
                    "decision_bucket": gk_r2.decision_bucket(decision),
                    "verdict_type": gk_r2.raw_verdict(decision),
                    "r2_class": gk_r2.normalize_r2_class(r2),
                    "decision": decision,
                    "candidate": candidate,
                    "r2": r2,
                }
            )
    manifest = {
        "candidate_universe_rows": len(candidates),
        "decision_rows_read": sum(1 for path in decision_paths for _ in gk_r2.read_jsonl(path)),
        "decision_rows_joined": len(rows),
        "unmatched_decision_rows": unmatched,
        "join_method_counts": dict(join_counts),
        "r2_rows": len(r2_by_candidate),
    }
    return rows, manifest


def variant_current_buy(row: dict[str, Any]) -> bool:
    return row["decision_bucket"] == "BUY"


def variant_top3_clean_demand(row: dict[str, Any]) -> bool:
    return variant_current_buy(row) or (
        reason_code(row) == "HARD_FAIL_EXTREME_TOP3"
        and concentration_segment(row) == "concentrated_early_demand"
    )


def variant_core_iwim_clean_demand(row: dict[str, Any]) -> bool:
    return variant_current_buy(row) or (
        row["decision_bucket"] in {"REJECT_CORE_FAIL", "REJECT_IWIM_LOW_CONF"}
        and concentration_segment(row) in {"concentrated_early_demand", "not_extreme_concentration"}
        and not toxicity_reasons(row)
        and len(demand_reasons(row)) >= 2
    )


def variant_current_buy_toxicity_filter(row: dict[str, Any]) -> bool:
    return variant_current_buy(row) and not toxicity_reasons(row)


def variant_hybrid(row: dict[str, Any]) -> bool:
    return (
        variant_current_buy_toxicity_filter(row)
        or variant_top3_clean_demand(row)
        or variant_core_iwim_clean_demand(row)
    )


def evaluate_variant(
    rows: list[dict[str, Any]],
    variant_id: str,
    selector: Callable[[dict[str, Any]], bool],
    base_rate: float | None,
    current_precision: float | None,
) -> dict[str, Any]:
    selected = [row for row in rows if selector(row)]
    resolved = [row for row in selected if is_resolved(row)]
    positives = sum(1 for row in resolved if is_positive(row))
    negatives = sum(1 for row in resolved if is_negative(row))
    all_resolved = [row for row in rows if is_resolved(row)]
    missed_positive = sum(1 for row in all_resolved if is_positive(row) and not selector(row))
    nonbuy_resolved = sum(1 for row in all_resolved if not selector(row))
    nonbuy_positive = missed_positive
    precision = rate(positives, len(resolved))
    return {
        "variant_id": variant_id,
        "would_buy_rows": len(selected),
        "would_buy_resolved_rows": len(resolved),
        "would_buy_positive_rows": positives,
        "would_buy_negative_rows": negatives,
        "would_buy_precision": precision,
        "accept_rate_all_rows": rate(len(selected), len(rows)),
        "accept_rate_resolved_rows": rate(len(resolved), len(all_resolved)),
        "missed_positive_rows": missed_positive,
        "false_reject_rate_resolved": rate(nonbuy_positive, nonbuy_resolved),
        "delta_vs_base_rate_pp": None if precision is None or base_rate is None else precision - base_rate,
        "delta_vs_current_buy_precision_pp": None
        if precision is None or current_precision is None
        else precision - current_precision,
        "runtime_change_allowed": False,
        "requires_fresh_validation": variant_id != "current_gatekeeper_buy",
    }


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    def pct(value: Any) -> str:
        if value is None:
            return "n/a"
        return f"{float(value) * 100:.2f}%"

    lines = [
        "# Gatekeeper R2 Policy Autopsy",
        "",
        f"Status: `{report['status']}`",
        f"Business decision: `{report['business_decision']}`",
        f"Analysis conclusion: `{report['analysis_conclusion']}`",
        f"Selector scope: `{report['selector_scope']}`",
        f"Runtime scope: `{report['runtime_scope']}`",
        f"Decision plane: `{report['decision_plane']}`",
        "",
        "## Key Metrics",
        "",
    ]
    for key, value in report["global_metrics"].items():
        lines.append(f"- {key}: {value}")
    lines.extend(["", "## Verdicts", ""])
    for status in report["policy_autopsy_statuses"]:
        lines.append(f"- `{status}`")
    lines.extend(["", "## Concentration Split", ""])
    for row in report["top3_concentration_summary"]:
        lines.append(
            "- "
            f"{row['group']}: resolved={row['resolved_rows']}, "
            f"positive={row['r2_positive_rows']}, negative={row['r2_negative_rows']}, "
            f"positive_rate={pct(row['positive_rate_resolved'])}"
        )
    lines.extend(["", "## Counterfactual Variants", ""])
    for row in report["counterfactual_variant_summary"]:
        lines.append(
            "- "
            f"{row['variant_id']}: precision={pct(row['would_buy_precision'])}, "
            f"resolved={row['would_buy_resolved_rows']}, "
            f"missed_positive={row['missed_positive_rows']}, "
            f"delta_vs_base={pct(row['delta_vs_base_rate_pp'])}, "
            f"runtime_change_allowed={row['runtime_change_allowed']}"
        )
    lines.extend(["", "## Recommendation", ""])
    for item in report["recommended_actions"]:
        lines.append(f"- {item}")
    lines.extend(
        [
            "",
            "## Methodology",
            "",
            "This report is offline-only. It does not change Gatekeeper, runtime, execution, send path, or thresholds. "
            "Counterfactual variants are replay diagnostics on the same checkpoint and require fresh validation before any policy work.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    outputs = default_outputs(root, args.selector_scope, args)
    rows, join_manifest = join_rows(args)
    resolved = [row for row in rows if is_resolved(row)]
    positives = sum(1 for row in resolved if is_positive(row))
    negatives = sum(1 for row in resolved if is_negative(row))
    base_rate = rate(positives, len(resolved))
    buy_rows = [row for row in rows if row["decision_bucket"] == "BUY"]
    buy_resolved = [row for row in buy_rows if is_resolved(row)]
    buy_positive = sum(1 for row in buy_resolved if is_positive(row))
    buy_precision = rate(buy_positive, len(buy_resolved))

    top3_rows = [
        row
        for row in rows
        if row["decision_bucket"] == "REJECT_HARD_FAIL" and reason_code(row) == "HARD_FAIL_EXTREME_TOP3"
    ]
    top3_breakdown = []
    for item in aggregate(top3_rows, concentration_segment):
        item["segment_kind"] = "hard_fail_extreme_top3"
        top3_breakdown.append(item)

    core_rows = [row for row in rows if row["decision_bucket"] == "REJECT_CORE_FAIL"]
    core_breakdown = []
    for item in aggregate(core_rows, core_axis):
        item["segment_kind"] = "core_fail_axis"
        core_breakdown.append(item)

    iwim_rows = [row for row in rows if row["decision_bucket"] == "REJECT_IWIM_LOW_CONF"]
    iwim_breakdown = []
    for item in aggregate(iwim_rows, lambda row: f"{concentration_segment(row)}|tox={','.join(toxicity_reasons(row)) or 'none'}"):
        item["segment_kind"] = "iwim_low_conf"
        iwim_breakdown.append(item)

    false_buy_rows = [row for row in buy_rows if is_negative(row)]
    false_buy_breakdown = []
    for item in aggregate(false_buy_rows, lambda row: f"{concentration_segment(row)}|tox={','.join(toxicity_reasons(row)) or 'none'}"):
        item["segment_kind"] = "false_buy"
        false_buy_breakdown.append(item)

    variants = [
        evaluate_variant(rows, "current_gatekeeper_buy", variant_current_buy, base_rate, buy_precision),
        evaluate_variant(rows, "relax_extreme_top3_for_clean_early_demand", variant_top3_clean_demand, base_rate, buy_precision),
        evaluate_variant(rows, "relax_core_iwim_for_clean_demand", variant_core_iwim_clean_demand, base_rate, buy_precision),
        evaluate_variant(rows, "current_buy_with_toxicity_filter", variant_current_buy_toxicity_filter, base_rate, buy_precision),
        evaluate_variant(rows, "hybrid_clean_demand_plus_toxicity_filter", variant_hybrid, base_rate, buy_precision),
    ]

    def positive_rate_for(rows_: list[dict[str, Any]]) -> float | None:
        res = [row for row in rows_ if is_resolved(row)]
        return rate(sum(1 for row in res if is_positive(row)), len(res))

    statuses: list[str] = []
    lift = float(args.min_anti_signal_lift_pp)
    top3_rate = positive_rate_for(top3_rows)
    core_rate = positive_rate_for(core_rows)
    iwim_rate = positive_rate_for(iwim_rows)
    if top3_rate is not None and base_rate is not None and top3_rate >= base_rate + lift:
        statuses.append("POLICY_AUTOPSY_HARD_FAIL_ANTI_SIGNAL_DETECTED")
    if core_rate is not None and base_rate is not None and core_rate >= base_rate + lift:
        statuses.append("POLICY_AUTOPSY_CORE_FAIL_ANTI_SIGNAL_DETECTED")
    if iwim_rate is not None and base_rate is not None and abs(iwim_rate - base_rate) <= lift:
        statuses.append("POLICY_AUTOPSY_IWIM_NOT_CALIBRATED")
    if buy_precision is not None and base_rate is not None and buy_precision <= base_rate:
        statuses.append("POLICY_AUTOPSY_BUY_NO_EDGE")
    statuses.append("POLICY_AUTOPSY_NEEDS_FRESH_VALIDATION")

    best_variant = max(
        variants,
        key=lambda row: (
            -1.0 if row["would_buy_precision"] is None else float(row["would_buy_precision"]),
            int(row["would_buy_resolved_rows"]),
        ),
    )
    recommended_actions = [
        "Do not run runtime, burn-in, Gatekeeper tuning, execution, or send-path changes from this checkpoint.",
        "Treat HARD_FAIL_EXTREME_TOP3 and REJECT_CORE_FAIL as policy anti-signal suspects, not as proven safe hard rejects.",
        "Do not globally relax concentration gates: toxic concentration and concentrated early demand must remain separated.",
        "Use counterfactual variants only as offline diagnostics; any policy candidate requires fresh validation on a later frozen scope.",
        "Prefer policy redesign around toxicity evidence and new buyer/funding signal families over threshold tweaking.",
    ]
    analysis_conclusion = "POLICY_AUTOPSY_NO_RUNTIME_GO_POLICY_REDESIGN_REQUIRED"

    global_metrics = {
        "resolved_rows": len(resolved),
        "r2_positive_rows": positives,
        "r2_negative_rows": negatives,
        "base_positive_rate": base_rate,
        "buy_resolved_rows": len(buy_resolved),
        "buy_positive_rows": buy_positive,
        "buy_negative_rows": sum(1 for row in buy_resolved if is_negative(row)),
        "buy_precision": buy_precision,
        "hard_fail_extreme_top3_resolved_rows": len([row for row in top3_rows if is_resolved(row)]),
        "hard_fail_extreme_top3_positive_rate": top3_rate,
        "core_fail_positive_rate": core_rate,
        "iwim_low_conf_positive_rate": iwim_rate,
    }
    report = {
        "artifact": ARTIFACT,
        "status": "PASS",
        "business_decision": "DO_NOT_CHANGE_RUNTIME_USE_OFFLINE_REPLAY_ONLY",
        "selector_scope": args.selector_scope,
        "runtime_scope": args.runtime_scope,
        "decision_plane": args.decision_plane,
        "join_manifest": join_manifest,
        "global_metrics": global_metrics,
        "policy_autopsy_statuses": statuses,
        "analysis_conclusion": analysis_conclusion,
        "recommended_actions": recommended_actions,
        "best_counterfactual_variant": best_variant,
        "top3_concentration_summary": top3_breakdown,
        "counterfactual_variant_summary": variants,
        "non_claims": {
            "runtime_changed": False,
            "gatekeeper_changed": False,
            "execution_changed": False,
            "send_path_changed": False,
            "thresholds_tuned": False,
            "production_promotion_allowed": False,
        },
        "outputs": {key: str(value) for key, value in outputs.items()},
    }

    common.write_json(outputs["json"], report)
    write_markdown(outputs["md"], report)
    write_csv(outputs["hard_fail_extreme_top3"], top3_breakdown)
    write_csv(outputs["core_fail_axis"], core_breakdown)
    write_csv(outputs["iwim_low_conf"], iwim_breakdown)
    write_csv(outputs["false_buy"], false_buy_breakdown)
    write_csv(outputs["counterfactual_variants"], variants)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
