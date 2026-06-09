#!/usr/bin/env python3
"""Build P4A offline selector model redesign report.

P4A is diagnostic/offline only. It does not train a production model, change
Gatekeeper, change runtime score emission, change thresholds in runtime, or
start a burn-in. It explains why the current candidate failed cross-run and
tests simple evidence-sufficiency gates before ranking.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any

import audit_selector_shadow_score_parity as parity
import audit_selector_shadow_score_topk_drift as topk_drift
import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "model_redesign_v1"
MD_ARTIFACT = "MODEL_REDESIGN_DECISION.md"
FAILURE_MATRIX_ARTIFACT = "model_feature_failure_matrix_v1.csv"
CANDIDATE_GRID_ARTIFACT = "model_candidate_grid_v1.csv"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0
TOP_K = (10, 25, 50)

FLOW_FEATURES = [
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
]
GK_CORE_FEATURES = [
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
]
CONCENTRATION_FEATURES = ["gk_hhi", "gk_top3_volume_pct"]
DEV_RISK_FEATURES = [
    "gk_dev_has_sold",
    "gk_dev_volume_ratio",
    "gk_dev_sold_within_3s",
    "gk_dev_sold_within_5s",
]
CORE_REQUIRED_FEATURES = [
    "gk_bonding_progress_pct",
    "gk_current_market_cap_sol",
    "gk_price_change_ratio",
]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--candidate", default="combined:simple_feature_score_v1")
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-stable-top25-hit-rate", type=float, default=0.60)
    parser.add_argument("--min-stable-top50-lift-vs-base", type=float, default=0.05)
    parser.add_argument("--high-score-k", type=int, default=50)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--failure-matrix-output", default=None)
    parser.add_argument("--candidate-grid-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def row_key(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def numeric_feature(row: dict[str, Any], feature: str) -> float | None:
    value = p3g.feature_value(row, feature)
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        out = float(value)
        return out if math.isfinite(out) else None
    return None


def numeric_any(row: dict[str, Any], names: list[str]) -> float | None:
    for name in names:
        value = numeric_feature(row, name)
        if value is not None:
            return value
    return None


def score_row(row: dict[str, Any], specs: list[dict[str, Any]]) -> float | None:
    if not specs:
        return None
    score, _availability = topk_drift.score_training_row(row, specs)
    return score


def spec_subset(specs: list[dict[str, Any]], mode: str) -> list[dict[str, Any]]:
    if mode == "combined_full":
        return specs
    if mode == "gk_core_only":
        allowed = set(GK_CORE_FEATURES)
        return [spec for spec in specs if str(spec["name"]) in allowed]
    if mode == "flow_only":
        allowed = set(FLOW_FEATURES)
        return [spec for spec in specs if str(spec["name"]) in allowed]
    if mode == "penalized_missing":
        return specs
    raise ValueError(f"unknown score mode {mode}")


def score_map(rows: list[dict[str, Any]], specs: list[dict[str, Any]]) -> dict[str, float]:
    scores: dict[str, float] = {}
    for row in rows:
        key = row_key(row)
        if not key:
            continue
        score = score_row(row, specs)
        if score is not None:
            scores[key] = score
    return scores


def missing_features(row: dict[str, Any], features: list[str]) -> list[str]:
    return [feature for feature in features if numeric_feature(row, feature) is None]


def evidence_values(row: dict[str, Any]) -> dict[str, float | None]:
    return {
        "tx_count": numeric_any(
            row,
            ["evidence_tx_count", "tx_count", "total_tx_count", "total_tx_evaluated", "gk_total_tx_evaluated"],
        ),
        "unique_buyers": numeric_any(row, ["evidence_unique_buyers", "unique_buyers", "gk_unique_buyers"]),
        "unique_signers": numeric_any(row, ["evidence_unique_signers", "gk_unique_signers_evaluated"]),
        "buy_count": numeric_any(row, ["evidence_buy_count", "buy_count", "gk_buy_count"]),
        "total_volume_sol": numeric_any(row, ["evidence_total_volume_sol", "total_volume_sol", "gk_total_volume_sol"]),
    }


def evidence_status(row: dict[str, Any], gate: dict[str, Any]) -> tuple[str, list[str], dict[str, float | None]]:
    values = evidence_values(row)
    existing_status = common.str_or_none(row.get("score_eligibility_status"))
    existing_reasons = row.get("score_eligibility_reasons")
    if existing_status == "score_invalid_insufficient_market_evidence":
        return (
            existing_status,
            list(existing_reasons) if isinstance(existing_reasons, list) else ["score_eligibility_invalid"],
            values,
        )
    missing: list[str] = []
    for field in ("tx_count", "buy_count", "total_volume_sol"):
        threshold = gate.get(f"min_{field}")
        if threshold is None:
            continue
        value = values.get(field)
        if value is None:
            missing.append(f"{field}_missing")
        elif value < float(threshold):
            missing.append(f"{field}_below_min")
    unique_threshold = gate.get("min_unique_buyers")
    if unique_threshold is not None:
        unique_value = values.get("unique_buyers")
        signer_value = values.get("unique_signers")
        actor_value = unique_value if unique_value is not None else signer_value
        if actor_value is None:
            missing.append("unique_actor_count_missing")
        elif actor_value < float(unique_threshold):
            missing.append("unique_actor_count_below_min")
    core_missing = missing_features(row, CORE_REQUIRED_FEATURES)
    if core_missing:
        missing.extend(f"{feature}_missing" for feature in core_missing)
    if missing:
        return "score_invalid_insufficient_market_evidence", missing, values
    return "score_eligible", [], values


def penalize_missing(score: float, row: dict[str, Any]) -> float:
    penalty = 0.0
    if missing_features(row, CONCENTRATION_FEATURES):
        penalty += 0.08
    if missing_features(row, CORE_REQUIRED_FEATURES):
        penalty += 0.50
    if missing_features(row, FLOW_FEATURES):
        penalty += min(0.20, 0.03 * len(missing_features(row, FLOW_FEATURES)))
    return max(0.0, score - penalty)


def candidate_defs() -> list[dict[str, Any]]:
    return [
        {
            "candidate_id": "current_combined_score_v1",
            "score_mode": "combined_full",
            "eligibility_gate": {},
        },
        {
            "candidate_id": "eligibility_tx3_buy2_buyer2_plus_combined",
            "score_mode": "combined_full",
            "eligibility_gate": {"min_tx_count": 3, "min_buy_count": 2, "min_unique_buyers": 2},
        },
        {
            "candidate_id": "eligibility_tx5_buy3_buyer3_plus_combined",
            "score_mode": "combined_full",
            "eligibility_gate": {"min_tx_count": 5, "min_buy_count": 3, "min_unique_buyers": 3},
        },
        {
            "candidate_id": "eligibility_tx3_buy2_buyer2_plus_gk_core",
            "score_mode": "gk_core_only",
            "eligibility_gate": {"min_tx_count": 3, "min_buy_count": 2, "min_unique_buyers": 2},
        },
        {
            "candidate_id": "eligibility_tx3_buy2_buyer2_plus_flow",
            "score_mode": "flow_only",
            "eligibility_gate": {"min_tx_count": 3, "min_buy_count": 2, "min_unique_buyers": 2},
        },
        {
            "candidate_id": "eligibility_tx3_buy2_buyer2_plus_penalized_missing",
            "score_mode": "penalized_missing",
            "eligibility_gate": {"min_tx_count": 3, "min_buy_count": 2, "min_unique_buyers": 2},
        },
    ]


def candidate_scores(
    rows: list[dict[str, Any]],
    specs: list[dict[str, Any]],
    candidate: dict[str, Any],
) -> tuple[dict[str, float], Counter[str]]:
    subset = spec_subset(specs, str(candidate["score_mode"]))
    gate = candidate.get("eligibility_gate") or {}
    scores: dict[str, float] = {}
    status_counts: Counter[str] = Counter()
    for row in rows:
        key = row_key(row)
        if not key:
            continue
        status, _reasons, _values = evidence_status(row, gate)
        status_counts[status] += 1
        if status not in {"score_eligible", "score_degraded_partial_evidence"}:
            continue
        score = score_row(row, subset)
        if score is None:
            continue
        if candidate["score_mode"] == "penalized_missing":
            score = penalize_missing(score, row)
        scores[key] = score
    return scores, status_counts


def ordered(rows: list[dict[str, Any]], scores: dict[str, float]) -> list[dict[str, Any]]:
    selected = [row for row in rows if row_key(row) in scores]
    return sorted(
        selected,
        key=lambda row: (
            -scores[row_key(row)],
            common.int_or_none(row.get("birth_ts_ms")) or 0,
            row_key(row),
        ),
    )


def metric(rows: list[dict[str, Any]], selected: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    positive_count = sum(1 for row in selected if label_positive(row))
    precision = positive_count / len(selected) if selected else None
    base_rate = positives / len(rows) if rows else None
    return {
        "denominator_rows": len(rows),
        "selected_count": len(selected),
        "positive_count": positive_count,
        "negative_count": len(selected) - positive_count,
        "precision": precision,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": (
            precision - base_rate if isinstance(precision, float) and isinstance(base_rate, float) else None
        ),
        "ev_proxy_pct": (
            precision * TARGET_NET_PCT - (1.0 - precision) * STOP_NET_PCT
            if isinstance(precision, float)
            else None
        ),
        "accept_rate": len(selected) / len(rows) if rows else None,
        "recall": positive_count / positives if positives else None,
    }


def topk_metrics(rows: list[dict[str, Any]], scores: dict[str, float]) -> dict[str, Any]:
    ranked = ordered(rows, scores)
    return {
        f"top{k}": metric(rows, ranked[: min(k, len(ranked))])
        for k in TOP_K
    }


def pushed_features(row: dict[str, Any], specs: list[dict[str, Any]]) -> list[str]:
    out: list[str] = []
    for spec in specs:
        feature = str(spec["name"])
        value = numeric_feature(row, feature)
        if value is None:
            continue
        normalized = p3g.normalized_feature(row, feature, {feature: spec})
        direction = float(spec.get("direction") or 1.0)
        if normalized >= 0.75:
            out.append(f"high_{feature}" if direction >= 0 else f"low_{feature}")
    return out[:16]


def autopsy_rows(rows: list[dict[str, Any]], scores: dict[str, float], specs: list[dict[str, Any]], k: int) -> list[dict[str, Any]]:
    ranked = ordered(rows, scores)[:k]
    out: list[dict[str, Any]] = []
    for rank, row in enumerate(ranked, start=1):
        gate = {"min_tx_count": 3, "min_buy_count": 2, "min_unique_buyers": 2}
        status, reasons, values = evidence_status(row, gate)
        out.append(
            {
                "rank": rank,
                "candidate_id": row.get("candidate_id"),
                "base_mint": row.get("base_mint"),
                "pool_id": row.get("pool_id"),
                "score": scores.get(row_key(row)),
                "r2_label": row.get("r2_label"),
                "is_positive": label_positive(row),
                "evidence_status_min_tx3_buy2_buyer2": status,
                "evidence_blocking_reasons": ";".join(reasons),
                "tx_count": values["tx_count"],
                "unique_buyers": values["unique_buyers"],
                "buy_count": values["buy_count"],
                "total_volume_sol": values["total_volume_sol"],
                "sell_share": numeric_feature(row, "sell_share"),
                "net_quote_in_15s": numeric_feature(row, "net_quote_in_15s"),
                "net_quote_in_30s": numeric_feature(row, "net_quote_in_30s"),
                "gk_bonding_progress_pct": numeric_feature(row, "gk_bonding_progress_pct"),
                "gk_current_market_cap_sol": numeric_feature(row, "gk_current_market_cap_sol"),
                "gk_price_change_ratio": numeric_feature(row, "gk_price_change_ratio"),
                "gk_hhi": numeric_feature(row, "gk_hhi"),
                "gk_top3_volume_pct": numeric_feature(row, "gk_top3_volume_pct"),
                "features_pushing_score_up": ";".join(pushed_features(row, specs)),
                "missing_features": ";".join(missing_features(row, FLOW_FEATURES + GK_CORE_FEATURES + CONCENTRATION_FEATURES)),
            }
        )
    return out


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = sorted({key for row in rows for key in row})
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field) for field in fieldnames})


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    lines = [
        "# Model Redesign Decision",
        "",
        f"Status: {report['status']}",
        f"Business decision: {report['business_decision']}",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Decision",
        "",
        f"- fail_reasons: {', '.join(report['acceptance']['fail_reasons']) or 'none'}",
        f"- current_candidate_status: {report['current_candidate_status']}",
        "",
        "## Candidate Grid",
        "",
        "| candidate | train top25 | validation top25 | validation top50 | validation eligible |",
        "|---|---:|---:|---:|---:|",
    ]
    for candidate_id, payload in report["candidate_grid"].items():
        train_top25 = payload["train"]["topk"]["top25"].get("precision")
        validation_top25 = payload["validation"]["topk"]["top25"].get("precision")
        validation_top50 = payload["validation"]["topk"]["top50"].get("precision")
        eligible = payload["validation"].get("eligible_rows")
        lines.append(
            "| {candidate} | {train_top25} | {validation_top25} | {validation_top50} | {eligible} |".format(
                candidate=candidate_id,
                train_top25=f"{train_top25:.4f}" if isinstance(train_top25, float) else "n/a",
                validation_top25=f"{validation_top25:.4f}" if isinstance(validation_top25, float) else "n/a",
                validation_top50=f"{validation_top50:.4f}" if isinstance(validation_top50, float) else "n/a",
                eligible=eligible,
            )
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    specs, _thresholds = parity.parse_runtime_spec(root / args.rust_source)
    train_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.train_scope))))
    validation_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.validation_scope))))

    current_scores_train = score_map(train_rows, specs)
    current_scores_validation = score_map(validation_rows, specs)
    failure_rows = autopsy_rows(validation_rows, current_scores_validation, specs, args.high_score_k)
    high_score_failed_rows = [row for row in failure_rows if not row["is_positive"]]
    high_score_insufficient_evidence_rows = [
        row
        for row in high_score_failed_rows
        if row["evidence_status_min_tx3_buy2_buyer2"] == "score_invalid_insufficient_market_evidence"
    ]

    grid: dict[str, Any] = {}
    grid_csv_rows: list[dict[str, Any]] = []
    stable_candidates: list[str] = []
    for candidate in candidate_defs():
        candidate_id = str(candidate["candidate_id"])
        candidate_payload: dict[str, Any] = {}
        for run_name, rows in (("train", train_rows), ("validation", validation_rows)):
            scores, status_counts = candidate_scores(rows, specs, candidate)
            topk = topk_metrics(rows, scores)
            payload = {
                "score_mode": candidate["score_mode"],
                "eligibility_gate": candidate["eligibility_gate"],
                "eligible_rows": len(scores),
                "eligibility_status_counts": dict(status_counts),
                "topk": topk,
            }
            candidate_payload[run_name] = payload
            for topk_label, metric_payload in topk.items():
                grid_csv_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "run": run_name,
                        "score_mode": candidate["score_mode"],
                        "topk": topk_label,
                        "eligible_rows": len(scores),
                        **metric_payload,
                    }
                )
        validation_top25 = candidate_payload["validation"]["topk"]["top25"].get("precision")
        validation_top50 = candidate_payload["validation"]["topk"]["top50"].get("precision")
        validation_base = candidate_payload["validation"]["topk"]["top50"].get("base_positive_rate")
        train_top25 = candidate_payload["train"]["topk"]["top25"].get("precision")
        if (
            isinstance(train_top25, float)
            and isinstance(validation_top25, float)
            and isinstance(validation_top50, float)
            and isinstance(validation_base, float)
            and train_top25 >= args.min_stable_top25_hit_rate
            and validation_top25 >= args.min_stable_top25_hit_rate
            and validation_top50 - validation_base >= args.min_stable_top50_lift_vs_base
        ):
            stable_candidates.append(candidate_id)
        grid[candidate_id] = candidate_payload

    fail_reasons: list[str] = []
    if not stable_candidates:
        fail_reasons.append("no_crossrun_stable_simple_candidate")
    if high_score_insufficient_evidence_rows:
        fail_reasons.append("high_score_failed_rows_include_insufficient_market_evidence")

    if stable_candidates:
        status = "P4A_NEW_CANDIDATE_FOUND_NEEDS_FRESH_VALIDATION"
        business_decision = "FRESH_VALIDATION_REQUIRED_BEFORE_RUNTIME"
    elif high_score_insufficient_evidence_rows:
        status = "P4A_NEEDS_FEATURE_FIX"
        business_decision = "DO_NOT_RUN_RUNTIME_FIX_SCORE_ELIGIBILITY_OFFLINE"
    else:
        status = "P4A_NO_STABLE_CANDIDATE_FOUND"
        business_decision = "DO_NOT_RUN_RUNTIME"

    report = {
        "artifact": ARTIFACT,
        "status": status,
        "candidate": args.candidate,
        "current_candidate_status": "REJECTED_FOR_FORWARD_SHADOW",
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "business_decision": business_decision,
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "trained_model": False,
            "changed_gatekeeper": False,
            "changed_runtime_score": False,
            "changed_execution": False,
            "production_promotion_allowed": False,
        },
        "acceptance": {
            "min_stable_top25_hit_rate": args.min_stable_top25_hit_rate,
            "min_stable_top50_lift_vs_base": args.min_stable_top50_lift_vs_base,
            "fail_reasons": fail_reasons,
        },
        "summary": {
            "train_rows": len(train_rows),
            "validation_rows": len(validation_rows),
            "validation_current_top50_failed_rows": len(high_score_failed_rows),
            "validation_current_top50_insufficient_evidence_failed_rows": len(high_score_insufficient_evidence_rows),
            "stable_candidate_ids": stable_candidates,
        },
        "current_candidate_topk": {
            "train": topk_metrics(train_rows, current_scores_train),
            "validation": topk_metrics(validation_rows, current_scores_validation),
        },
        "candidate_grid": grid,
        "outputs": {},
    }

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    md_output = Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT
    failure_output = (
        Path(args.failure_matrix_output)
        if args.failure_matrix_output
        else out_dir / FAILURE_MATRIX_ARTIFACT
    )
    grid_output = (
        Path(args.candidate_grid_output)
        if args.candidate_grid_output
        else out_dir / CANDIDATE_GRID_ARTIFACT
    )
    report["outputs"] = {
        ARTIFACT: str(output),
        MD_ARTIFACT: str(md_output),
        FAILURE_MATRIX_ARTIFACT: str(failure_output),
        CANDIDATE_GRID_ARTIFACT: str(grid_output),
    }
    write_json(output, report)
    write_markdown(md_output, report)
    write_csv(failure_output, failure_rows)
    write_csv(grid_output, grid_csv_rows)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
