#!/usr/bin/env python3
"""Build offline Gatekeeper policy redesign candidate report.

This is a diagnostic-only R2 opportunity analysis. It evaluates a fixed,
small set of semantic policy candidates against Gatekeeper decision logs joined
to R2 labels. It does not change runtime, Gatekeeper, execution, send path, or
production thresholds.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from pathlib import Path
from typing import Any, Callable

import analyze_gatekeeper_r2_policy_autopsy as autopsy


ARTIFACT = "gatekeeper_policy_redesign_candidates_v1"
MD_ARTIFACT = "GATEKEEPER_POLICY_REDESIGN_CANDIDATES.md"
METRICS_CSV = "gatekeeper_policy_candidate_metrics_v1.csv"
TOPK_CSV = "gatekeeper_policy_candidate_topk_v1.csv"
DEFAULT_SELECTOR_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r23-score-tail-v1-r1-cutoff-check2"
DEFAULT_RUNTIME_SCOPE = "shadow-burnin-v3-score-tail-v1-r1-cutoff-check2-snapshot"
TOP_K = (8, 25, 50, 100, 200, 500, 1000)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--selector-scope", default=DEFAULT_SELECTOR_SCOPE)
    parser.add_argument("--runtime-scope", default=DEFAULT_RUNTIME_SCOPE)
    parser.add_argument("--decision-plane", default="legacy_live", choices=("legacy_live", "v25_shadow"))
    parser.add_argument("--nearest-tolerance-ms", type=int, default=120_000)
    parser.add_argument("--min-edge-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-edge-resolved-rows", type=int, default=75)
    parser.add_argument("--edge-k", type=int, default=1000)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--metrics-output", default=None)
    parser.add_argument("--topk-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def report_dir(root: Path, selector_scope: str) -> Path:
    return root / "reports" / "selector" / selector_scope


def default_outputs(root: Path, selector_scope: str, args: argparse.Namespace) -> dict[str, Path]:
    out_dir = report_dir(root, selector_scope)
    return {
        "json": Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json",
        "md": Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT,
        "metrics_csv": Path(args.metrics_output) if args.metrics_output else out_dir / METRICS_CSV,
        "topk_csv": Path(args.topk_output) if args.topk_output else out_dir / TOPK_CSV,
    }


def metric(row: dict[str, Any], *fields: str) -> float | None:
    value = autopsy.decision_metric(row, *fields)
    if value is None or not math.isfinite(value):
        return None
    return value


def finite_or_zero(value: float | None) -> float:
    return value if value is not None and math.isfinite(value) else 0.0


def score_tail_pressure_reversal(row: dict[str, Any]) -> float | None:
    price_change = metric(row, "price_change_ratio")
    if price_change is None:
        return None
    return (
        (-price_change) * 0.35
        + finite_or_zero(metric(row, "sell_buy_ratio")) * 0.20
        + finite_or_zero(metric(row, "sell_share")) * 0.15
        + finite_or_zero(metric(row, "top3_volume_pct", "max_top3_volume_pct")) * 0.15
        + finite_or_zero(metric(row, "hhi", "max_hhi")) * 0.10
        + min(finite_or_zero(metric(row, "total_volume_sol", "max_total_volume_sol")), 5.0) / 5.0 * 0.05
        - finite_or_zero(metric(row, "buy_ratio", "fixed_size_buy_ratio")) * 0.05
    )


def score_tas_volume_momentum(row: dict[str, Any]) -> float | None:
    values = [
        metric(row, "tas_volume_score"),
        metric(row, "tas_momentum_score"),
        metric(row, "tas_buy_ratio_score"),
        metric(row, "soft_score"),
    ]
    present = [value for value in values if value is not None]
    if len(present) < 2:
        return None
    return sum(present) / len(present)


def score_top3_pressure_salvage(row: dict[str, Any]) -> float | None:
    price_change = metric(row, "price_change_ratio") or 0.0
    return (
        finite_or_zero(metric(row, "top3_volume_pct", "max_top3_volume_pct")) * 0.35
        + finite_or_zero(metric(row, "early_top3_buy_volume_pct_3s")) * 0.15
        + finite_or_zero(metric(row, "early_slot_volume_dominance_buy")) * 0.10
        + finite_or_zero(metric(row, "sell_buy_ratio")) * 0.25
        + (-price_change) * 0.15
    )


def candidate_defs() -> list[dict[str, Any]]:
    return [
        {
            "candidate_id": "top3_pressure_salvage_candidate",
            "score_fn": score_top3_pressure_salvage,
            "risk_class": "R2_OPPORTUNITY_NOT_EXECUTION_SAFE",
            "description": "Ranks high top3/HHI pressure plus sell pressure and low early price change.",
        },
        {
            "candidate_id": "tail_pressure_reversal_candidate",
            "score_fn": score_tail_pressure_reversal,
            "risk_class": "R2_OPPORTUNITY_NOT_EXECUTION_SAFE",
            "description": "Ranks low price-change tail/reversal candidates with sell pressure and concentrated flow.",
        },
        {
            "candidate_id": "tas_volume_momentum_candidate",
            "score_fn": score_tas_volume_momentum,
            "risk_class": "DIAGNOSTIC_POLICY_SUPPORT",
            "description": "Ranks TAS volume/momentum/buy-ratio support without changing TAS runtime policy.",
        },
    ]


def rank_rows(
    rows: list[dict[str, Any]], score_fn: Callable[[dict[str, Any]], float | None]
) -> list[tuple[float, dict[str, Any]]]:
    scored: list[tuple[float, dict[str, Any]]] = []
    for row in rows:
        score = score_fn(row)
        if score is not None and math.isfinite(score):
            scored.append((score, row))
    return sorted(
        scored,
        key=lambda item: (
            -item[0],
            autopsy.common.int_or_none(item[1].get("candidate", {}).get("decision_ts_ms")) or 0,
            str(item[1].get("candidate_id") or ""),
        ),
    )


def selected_metric(
    *,
    candidate_id: str,
    selection_mode: str,
    k: int,
    selected: list[dict[str, Any]],
    base_rate: float | None,
) -> dict[str, Any]:
    resolved = [row for row in selected if autopsy.is_resolved(row)]
    positives = sum(1 for row in resolved if autopsy.is_positive(row))
    negatives = sum(1 for row in resolved if autopsy.is_negative(row))
    precision = autopsy.rate(positives, len(resolved))
    return {
        "candidate_id": candidate_id,
        "selection_mode": selection_mode,
        "top_k": k,
        "selected_rows": len(selected),
        "resolved_rows": len(resolved),
        "r2_positive_rows": positives,
        "r2_negative_rows": negatives,
        "precision": precision,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": (
            None if precision is None or base_rate is None else precision - base_rate
        ),
        "label_coverage": autopsy.rate(len(resolved), len(selected)),
    }


def topk_metrics(
    *,
    candidate_id: str,
    ranked_all: list[tuple[float, dict[str, Any]]],
    ranked_resolved: list[tuple[float, dict[str, Any]]],
    base_rate: float | None,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for k in TOP_K:
        rows.append(
            selected_metric(
                candidate_id=candidate_id,
                selection_mode="all_rows_ranked",
                k=k,
                selected=[row for _score, row in ranked_all[: min(k, len(ranked_all))]],
                base_rate=base_rate,
            )
        )
        rows.append(
            selected_metric(
                candidate_id=candidate_id,
                selection_mode="resolved_only_reference",
                k=k,
                selected=[row for _score, row in ranked_resolved[: min(k, len(ranked_resolved))]],
                base_rate=base_rate,
            )
        )
    return rows


def candidate_metric_summary(
    candidate: dict[str, Any],
    ranked_all: list[tuple[float, dict[str, Any]]],
    ranked_resolved: list[tuple[float, dict[str, Any]]],
    topk_rows: list[dict[str, Any]],
    args: argparse.Namespace,
) -> dict[str, Any]:
    edge_rows = [
        row
        for row in topk_rows
        if row["candidate_id"] == candidate["candidate_id"]
        and row["selection_mode"] == "all_rows_ranked"
        and int(row["top_k"]) == int(args.edge_k)
    ]
    edge_row = edge_rows[0] if edge_rows else {}
    precision = edge_row.get("precision")
    lift = edge_row.get("lift_vs_base_rate_pp")
    resolved = int(edge_row.get("resolved_rows") or 0)
    edge_pass = (
        precision is not None
        and lift is not None
        and float(lift) >= float(args.min_edge_lift_pp)
        and resolved >= int(args.min_edge_resolved_rows)
    )
    return {
        "candidate_id": candidate["candidate_id"],
        "description": candidate["description"],
        "risk_class": candidate["risk_class"],
        "scorable_rows": len(ranked_all),
        "scorable_resolved_rows": len(ranked_resolved),
        "edge_k": int(args.edge_k),
        "edge_k_resolved_rows": resolved,
        "edge_k_precision": precision,
        "edge_k_lift_vs_base_rate_pp": lift,
        "edge_k_label_coverage": edge_row.get("label_coverage"),
        "edge_pass": edge_pass,
        "runtime_change_allowed": False,
        "requires_fresh_validation": True,
    }


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = sorted({key for row in rows for key in row})
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    def pct(value: Any) -> str:
        if value is None:
            return "n/a"
        return f"{float(value) * 100:.2f}%"

    lines = [
        "# Gatekeeper Policy Redesign Candidates",
        "",
        f"Status: `{report['status']}`",
        f"Business decision: `{report['business_decision']}`",
        f"Selector scope: `{report['selector_scope']}`",
        f"Runtime scope: `{report['runtime_scope']}`",
        f"Decision plane: `{report['decision_plane']}`",
        "",
        "## Global Metrics",
        "",
    ]
    for key, value in report["global_metrics"].items():
        lines.append(f"- {key}: {value}")
    lines.extend(["", "## Candidate Summary", ""])
    for row in report["candidate_summaries"]:
        lines.append(
            "- "
            f"{row['candidate_id']}: edge_k={row['edge_k']}, "
            f"precision={pct(row['edge_k_precision'])}, "
            f"lift={pct(row['edge_k_lift_vs_base_rate_pp'])}, "
            f"resolved={row['edge_k_resolved_rows']}, "
            f"label_coverage={pct(row['edge_k_label_coverage'])}, "
            f"edge_pass={row['edge_pass']}, "
            f"risk_class={row['risk_class']}"
        )
    lines.extend(["", "## Statuses", ""])
    for status in report["policy_redesign_statuses"]:
        lines.append(f"- `{status}`")
    lines.extend(["", "## Recommendation", ""])
    for item in report["recommended_actions"]:
        lines.append(f"- {item}")
    lines.extend(
        [
            "",
            "## Non-Claims",
            "",
            "This report is offline-only. It does not change runtime, Gatekeeper policy, execution, send path, or production thresholds. "
            "Candidates are R2 opportunity probes and require fresh frozen validation before any policy work.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    outputs = default_outputs(root, args.selector_scope, args)
    rows, join_manifest = autopsy.join_rows(args)
    resolved = [row for row in rows if autopsy.is_resolved(row)]
    positives = sum(1 for row in resolved if autopsy.is_positive(row))
    negatives = sum(1 for row in resolved if autopsy.is_negative(row))
    base_rate = autopsy.rate(positives, len(resolved))

    topk_rows: list[dict[str, Any]] = []
    summaries: list[dict[str, Any]] = []
    for candidate in candidate_defs():
        ranked_all = rank_rows(rows, candidate["score_fn"])
        ranked_resolved = [(score, row) for score, row in ranked_all if autopsy.is_resolved(row)]
        candidate_topk = topk_metrics(
            candidate_id=str(candidate["candidate_id"]),
            ranked_all=ranked_all,
            ranked_resolved=ranked_resolved,
            base_rate=base_rate,
        )
        topk_rows.extend(candidate_topk)
        summaries.append(candidate_metric_summary(candidate, ranked_all, ranked_resolved, candidate_topk, args))

    edge_candidates = [row for row in summaries if row["edge_pass"]]
    statuses: list[str] = []
    if edge_candidates:
        statuses.append("POLICY_REDESIGN_EDGE_FOUND_OFFLINE_R2_OPPORTUNITY")
    else:
        statuses.append("POLICY_REDESIGN_NO_STABLE_EDGE_FOUND")
    if any((row.get("edge_k_label_coverage") or 0.0) < 0.20 for row in summaries):
        statuses.append("POLICY_REDESIGN_LABEL_COVERAGE_WARNING")
    statuses.extend(
        [
            "POLICY_REDESIGN_REQUIRES_FRESH_VALIDATION",
            "POLICY_REDESIGN_NO_RUNTIME_GO",
        ]
    )

    best = max(
        summaries,
        key=lambda row: (
            -1.0 if row["edge_k_precision"] is None else float(row["edge_k_precision"]),
            int(row["edge_k_resolved_rows"]),
        ),
    ) if summaries else None
    recommended_actions = [
        "Do not change runtime, Gatekeeper, execution, send path, or thresholds from this offline report.",
        "Treat edge candidates as R2 opportunity probes, not execution-safe policy.",
        "Validate the best candidate on a later frozen scope before any Gatekeeper policy PR.",
        "If fresh validation fails, move to buyer/funding signal family work instead of threshold tuning.",
    ]
    report = {
        "artifact": ARTIFACT,
        "status": "PASS",
        "business_decision": "OFFLINE_EDGE_PROBE_ONLY_DO_NOT_CHANGE_RUNTIME",
        "selector_scope": args.selector_scope,
        "runtime_scope": args.runtime_scope,
        "decision_plane": args.decision_plane,
        "join_manifest": join_manifest,
        "global_metrics": {
            "decision_rows": len(rows),
            "resolved_rows": len(resolved),
            "r2_positive_rows": positives,
            "r2_negative_rows": negatives,
            "base_positive_rate": base_rate,
            "edge_k": int(args.edge_k),
            "min_edge_lift_pp": float(args.min_edge_lift_pp),
            "min_edge_resolved_rows": int(args.min_edge_resolved_rows),
        },
        "policy_redesign_statuses": statuses,
        "best_candidate": best,
        "candidate_summaries": summaries,
        "recommended_actions": recommended_actions,
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
    autopsy.common.write_json(outputs["json"], report)
    write_markdown(outputs["md"], report)
    write_csv(outputs["metrics_csv"], summaries)
    write_csv(outputs["topk_csv"], topk_rows)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
