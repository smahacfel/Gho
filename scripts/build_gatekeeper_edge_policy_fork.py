#!/usr/bin/env python3
"""Build an offline Gatekeeper edge policy fork report.

This report materializes a shadow-only/counterfactual policy fork from the
policy-redesign candidate families. It does not change runtime Gatekeeper
policy, execution, send path, configs, or thresholds. The fork is an R2
opportunity probe: rows that would be allowed by the fork remain explicitly
classified as not execution-safe until fresh validation and execution-safety
review are complete.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path
from typing import Any

import analyze_gatekeeper_r2_policy_autopsy as autopsy
import build_gatekeeper_policy_redesign_candidates as redesign
import selector_pipeline_common as common


ARTIFACT = "gatekeeper_edge_policy_fork_v1"
MD_ARTIFACT = "GATEKEEPER_EDGE_POLICY_FORK.md"
SUMMARY_CSV = "gatekeeper_edge_policy_fork_summary_v1.csv"
ROWS_CSV = "gatekeeper_edge_policy_fork_rows_v1.csv"
DEFAULT_SELECTOR_SCOPE = redesign.DEFAULT_SELECTOR_SCOPE
DEFAULT_RUNTIME_SCOPE = redesign.DEFAULT_RUNTIME_SCOPE
DEFAULT_POLICY_CANDIDATES = (
    "tail_pressure_reversal_candidate",
    "top3_pressure_salvage_candidate",
)
OPPORTUNITY_PRIORITY = (
    "tail_pressure_reversal_candidate",
    "top3_pressure_salvage_candidate",
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--selector-scope", default=DEFAULT_SELECTOR_SCOPE)
    parser.add_argument("--runtime-scope", default=DEFAULT_RUNTIME_SCOPE)
    parser.add_argument("--decision-plane", default="legacy_live", choices=("legacy_live", "v25_shadow"))
    parser.add_argument("--input-source", default="decision_logs", choices=("decision_logs", "training_view"))
    parser.add_argument("--nearest-tolerance-ms", type=int, default=120_000)
    parser.add_argument("--edge-k", type=int, default=1000)
    parser.add_argument("--candidate-id", action="append", default=None)
    parser.add_argument("--min-opportunity-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-opportunity-resolved-rows", type=int, default=75)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--summary-output", default=None)
    parser.add_argument("--rows-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def report_dir(root: Path, selector_scope: str) -> Path:
    return root / "reports" / "selector" / selector_scope


def default_outputs(root: Path, selector_scope: str, args: argparse.Namespace) -> dict[str, Path]:
    out_dir = report_dir(root, selector_scope)
    return {
        "json": Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json",
        "md": Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT,
        "summary_csv": Path(args.summary_output) if args.summary_output else out_dir / SUMMARY_CSV,
        "rows_csv": Path(args.rows_output) if args.rows_output else out_dir / ROWS_CSV,
    }


def candidate_defs_by_id() -> dict[str, dict[str, Any]]:
    return {str(candidate["candidate_id"]): candidate for candidate in redesign.candidate_defs()}


def requested_candidate_ids(args: argparse.Namespace) -> list[str]:
    ids = args.candidate_id or list(DEFAULT_POLICY_CANDIDATES)
    known = candidate_defs_by_id()
    unknown = [candidate_id for candidate_id in ids if candidate_id not in known]
    if unknown:
        raise SystemExit(f"unknown candidate_id(s): {', '.join(sorted(unknown))}")
    return list(dict.fromkeys(ids))


def row_key(row: dict[str, Any]) -> str:
    return str(row.get("candidate_id") or "")


def current_buy(row: dict[str, Any]) -> bool:
    return row.get("decision_bucket") == "BUY"


def selected_opportunities(
    rows: list[dict[str, Any]], candidate_ids: list[str], edge_k: int
) -> dict[str, list[dict[str, Any]]]:
    known = candidate_defs_by_id()
    selected: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for candidate_id in candidate_ids:
        candidate = known[candidate_id]
        ranked = redesign.rank_rows(rows, candidate["score_fn"])
        for rank, (score, row) in enumerate(ranked[: min(edge_k, len(ranked))], start=1):
            selected[row_key(row)].append(
                {
                    "candidate_id": candidate_id,
                    "rank": rank,
                    "score": score,
                    "risk_class": candidate["risk_class"],
                }
            )
    return selected


def choose_opportunity(hits: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not hits:
        return None
    by_id = {str(hit["candidate_id"]): hit for hit in hits}
    for candidate_id in OPPORTUNITY_PRIORITY:
        if candidate_id in by_id:
            return by_id[candidate_id]
    return sorted(hits, key=lambda hit: int(hit["rank"]))[0]


def opportunity_reason(candidate_id: str) -> str:
    if candidate_id == "tail_pressure_reversal_candidate":
        return "TAIL_PRESSURE_REVERSAL_R2_OPPORTUNITY"
    if candidate_id == "top3_pressure_salvage_candidate":
        return "TOP3_PRESSURE_SALVAGE_R2_OPPORTUNITY"
    return f"{candidate_id.upper()}_R2_OPPORTUNITY"


def fork_classification(row: dict[str, Any], hits: list[dict[str, Any]]) -> dict[str, Any]:
    segment = autopsy.concentration_segment(row)
    toxic_reasons = autopsy.toxicity_reasons(row)
    demand_reasons = autopsy.demand_reasons(row)
    opportunity = choose_opportunity(hits)
    if current_buy(row):
        verdict = "CURRENT_BUY_UNCHANGED"
        reason = "CURRENT_GATEKEEPER_BUY"
        candidate_id = None
        score = None
        rank = None
        risk_class = "CURRENT_POLICY"
    elif opportunity is not None:
        verdict = "WOULD_ALLOW_R2_OPPORTUNITY_NOT_EXECUTION_SAFE"
        candidate_id = str(opportunity["candidate_id"])
        reason = opportunity_reason(candidate_id)
        score = opportunity["score"]
        rank = opportunity["rank"]
        risk_class = opportunity["risk_class"]
    elif segment == "toxic_concentration":
        verdict = "WOULD_KEEP_REJECT_TOXIC_CONCENTRATION"
        reason = "TOXIC_CONCENTRATION_REMAINS_HARD_REJECT"
        candidate_id = None
        score = None
        rank = None
        risk_class = "EXECUTION_SAFETY_REJECT"
    elif segment == "concentrated_early_demand":
        verdict = "WOULD_KEEP_REJECT_UNSELECTED_CONCENTRATED_DEMAND"
        reason = "CONCENTRATED_DEMAND_NOT_IN_POLICY_FORK_TOPK"
        candidate_id = None
        score = None
        rank = None
        risk_class = "UNSELECTED_R2_OPPORTUNITY"
    else:
        verdict = "WOULD_KEEP_CURRENT_NON_BUY"
        reason = "OUTSIDE_POLICY_FORK"
        candidate_id = None
        score = None
        rank = None
        risk_class = "CURRENT_POLICY"
    return {
        "policy_fork_verdict": verdict,
        "policy_fork_reason": reason,
        "policy_fork_candidate_id": candidate_id,
        "policy_fork_score": score,
        "policy_fork_rank": rank,
        "policy_fork_risk_class": risk_class,
        "concentration_segment": segment,
        "toxicity_reasons": ";".join(toxic_reasons),
        "demand_reasons": ";".join(demand_reasons),
        "opportunity_hit_count": len(hits),
        "opportunity_hit_ids": ";".join(str(hit["candidate_id"]) for hit in hits),
    }


def materialize_rows(
    rows: list[dict[str, Any]], selected: dict[str, list[dict[str, Any]]]
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in rows:
        fork = fork_classification(row, selected.get(row_key(row), []))
        decision = row.get("decision") or {}
        out.append(
            {
                "candidate_id": row.get("candidate_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "decision_ts_ms": (row.get("candidate") or {}).get("decision_ts_ms")
                or decision.get("decision_ts_ms"),
                "current_decision_bucket": row.get("decision_bucket"),
                "current_verdict_type": row.get("verdict_type"),
                "current_reason_code": autopsy.reason_code(row),
                "r2_class": row.get("r2_class"),
                "r2_label": (row.get("r2") or {}).get("r2_label"),
                "policy_fork_verdict": fork["policy_fork_verdict"],
                "policy_fork_reason": fork["policy_fork_reason"],
                "policy_fork_candidate_id": fork["policy_fork_candidate_id"],
                "policy_fork_score": fork["policy_fork_score"],
                "policy_fork_rank": fork["policy_fork_rank"],
                "policy_fork_risk_class": fork["policy_fork_risk_class"],
                "concentration_segment": fork["concentration_segment"],
                "toxicity_reasons": fork["toxicity_reasons"],
                "demand_reasons": fork["demand_reasons"],
                "opportunity_hit_count": fork["opportunity_hit_count"],
                "opportunity_hit_ids": fork["opportunity_hit_ids"],
                "shadow_only": True,
                "changes_gatekeeper_decision": False,
                "changes_execution": False,
                "production_promotion_allowed": False,
            }
        )
    return out


def rate(num: int, den: int) -> float | None:
    return num / den if den else None


def summarize_rows(
    *, group: str, rows: list[dict[str, Any]], base_rate: float | None
) -> dict[str, Any]:
    resolved = [row for row in rows if row.get("r2_class") in {"positive", "negative"}]
    positives = sum(1 for row in resolved if row.get("r2_class") == "positive")
    negatives = sum(1 for row in resolved if row.get("r2_class") == "negative")
    precision = rate(positives, len(resolved))
    return {
        "group": group,
        "total_rows": len(rows),
        "resolved_rows": len(resolved),
        "r2_positive_rows": positives,
        "r2_negative_rows": negatives,
        "precision": precision,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": None if precision is None or base_rate is None else precision - base_rate,
        "label_coverage": rate(len(resolved), len(rows)),
    }


def summary_rows(fork_rows: list[dict[str, Any]], base_rate: float | None) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    rows.append(summarize_rows(group="all_rows", rows=fork_rows, base_rate=base_rate))
    rows.append(
        summarize_rows(
            group="current_gatekeeper_buy",
            rows=[row for row in fork_rows if row["current_decision_bucket"] == "BUY"],
            base_rate=base_rate,
        )
    )
    rows.append(
        summarize_rows(
            group="policy_fork_would_allow",
            rows=[
                row
                for row in fork_rows
                if row["policy_fork_verdict"] == "WOULD_ALLOW_R2_OPPORTUNITY_NOT_EXECUTION_SAFE"
            ],
            base_rate=base_rate,
        )
    )
    rows.append(
        summarize_rows(
            group="policy_fork_keep_toxic_concentration",
            rows=[row for row in fork_rows if row["policy_fork_verdict"] == "WOULD_KEEP_REJECT_TOXIC_CONCENTRATION"],
            base_rate=base_rate,
        )
    )
    by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in fork_rows:
        candidate_id = row.get("policy_fork_candidate_id")
        if candidate_id:
            by_candidate[str(candidate_id)].append(row)
    for candidate_id, group_rows in sorted(by_candidate.items()):
        rows.append(
            summarize_rows(
                group=f"policy_fork_would_allow:{candidate_id}",
                rows=group_rows,
                base_rate=base_rate,
            )
        )
    return rows


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
        "# Gatekeeper Edge Policy Fork",
        "",
        f"Status: `{report['status']}`",
        f"Business decision: `{report['business_decision']}`",
        f"Selector scope: `{report['selector_scope']}`",
        f"Runtime scope: `{report['runtime_scope']}`",
        f"Input source: `{report['input_source']}`",
        f"Decision plane: `{report['decision_plane']}`",
        "",
        "## Global Metrics",
        "",
    ]
    for key, value in report["global_metrics"].items():
        lines.append(f"- {key}: {value}")
    lines.extend(["", "## Policy Fork Summary", ""])
    for row in report["summary_rows"]:
        lines.append(
            "- "
            f"{row['group']}: rows={row['total_rows']}, "
            f"resolved={row['resolved_rows']}, "
            f"precision={pct(row['precision'])}, "
            f"lift={pct(row['lift_vs_base_rate_pp'])}, "
            f"label_coverage={pct(row['label_coverage'])}"
        )
    lines.extend(["", "## Statuses", ""])
    for status in report["policy_fork_statuses"]:
        lines.append(f"- `{status}`")
    lines.extend(["", "## Non-Claims", ""])
    lines.append(
        "This is a shadow-only/offline policy fork. It does not change Gatekeeper, runtime, execution, send path, configs, or thresholds."
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    outputs = default_outputs(root, args.selector_scope, args)
    candidate_ids = requested_candidate_ids(args)
    rows, join_manifest = redesign.load_rows(args)
    selected = selected_opportunities(rows, candidate_ids, int(args.edge_k))
    fork_rows = materialize_rows(rows, selected)
    resolved = [row for row in fork_rows if row["r2_class"] in {"positive", "negative"}]
    positives = sum(1 for row in resolved if row["r2_class"] == "positive")
    negatives = sum(1 for row in resolved if row["r2_class"] == "negative")
    base_rate = rate(positives, len(resolved))
    summaries = summary_rows(fork_rows, base_rate)
    allow_summary = next(row for row in summaries if row["group"] == "policy_fork_would_allow")
    allow_lift = allow_summary.get("lift_vs_base_rate_pp")
    allow_precision = allow_summary.get("precision")
    allow_resolved = int(allow_summary.get("resolved_rows") or 0)
    statuses: list[str] = ["GK_EDGE_POLICY_FORK_OFFLINE_ONLY"]
    if (
        allow_precision is not None
        and allow_lift is not None
        and float(allow_lift) >= float(args.min_opportunity_lift_pp)
        and allow_resolved >= int(args.min_opportunity_resolved_rows)
    ):
        statuses.append("GK_EDGE_POLICY_FORK_R2_OPPORTUNITY_CONFIRMED_OFFLINE")
    else:
        statuses.append("GK_EDGE_POLICY_FORK_NO_STABLE_R2_OPPORTUNITY")
    if (allow_summary.get("label_coverage") or 0.0) < 0.20:
        statuses.append("GK_EDGE_POLICY_FORK_LABEL_COVERAGE_WARNING")
    if int(join_manifest.get("unmatched_decision_rows") or 0) > 0:
        statuses.append("GK_EDGE_POLICY_FORK_JOIN_SCOPE_MISMATCH_WARNING")
    statuses.extend(
        [
            "GK_EDGE_POLICY_FORK_R2_OPPORTUNITY_NOT_EXECUTION_SAFE",
            "GK_EDGE_POLICY_FORK_REQUIRES_FRESH_VALIDATION",
            "GK_EDGE_POLICY_FORK_NO_RUNTIME_GO",
        ]
    )
    business_decision = (
        "OFFLINE_POLICY_FORK_EDGE_FOUND_REQUIRES_SHADOW_VALIDATION"
        if "GK_EDGE_POLICY_FORK_R2_OPPORTUNITY_CONFIRMED_OFFLINE" in statuses
        else "OFFLINE_POLICY_FORK_NO_GO"
    )
    report = {
        "artifact": ARTIFACT,
        "status": "PASS",
        "business_decision": business_decision,
        "selector_scope": args.selector_scope,
        "runtime_scope": args.runtime_scope,
        "decision_plane": args.decision_plane,
        "input_source": args.input_source,
        "edge_k": int(args.edge_k),
        "candidate_ids": candidate_ids,
        "join_manifest": join_manifest,
        "global_metrics": {
            "decision_rows": len(fork_rows),
            "resolved_rows": len(resolved),
            "r2_positive_rows": positives,
            "r2_negative_rows": negatives,
            "base_positive_rate": base_rate,
            "policy_fork_would_allow_rows": allow_summary["total_rows"],
            "policy_fork_would_allow_resolved_rows": allow_resolved,
            "policy_fork_would_allow_precision": allow_precision,
            "policy_fork_would_allow_lift_vs_base_rate_pp": allow_lift,
            "policy_fork_would_allow_label_coverage": allow_summary.get("label_coverage"),
        },
        "policy_fork_statuses": statuses,
        "summary_rows": summaries,
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
    write_csv(outputs["summary_csv"], summaries)
    write_csv(outputs["rows_csv"], fork_rows)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
