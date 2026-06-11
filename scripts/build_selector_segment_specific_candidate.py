#!/usr/bin/env python3
"""Build P4E segment-specific selector candidate report.

P4E is offline-only. It evaluates a small fixed set of segment-specific
candidates after P4D found segment-level lift. It does not change runtime,
Gatekeeper, execution, send path, or production thresholds.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any

import audit_selector_shadow_score_parity as parity
import audit_selector_shadow_score_topk_drift as topk_drift
import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "segment_specific_candidate_v1"
MD_ARTIFACT = "SEGMENT_SPECIFIC_CANDIDATE.md"
GRID_ARTIFACT = "segment_candidate_grid_v1.csv"
PROMOTABILITY_ARTIFACT = "segment_promotability_matrix_v1.csv"
FALSE_POSITIVE_ARTIFACT = "segment_false_positive_review_v1.csv"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
TOP_K = (10, 25, 50)
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-top25-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-top50-lift-pp", type=float, default=0.05)
    parser.add_argument("--min-candidate-rows", type=int, default=100)
    parser.add_argument("--min-topk-evidence-sufficient-rate", type=float, default=0.80)
    parser.add_argument("--max-toxic-false-positive-rate", type=float, default=0.50)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--grid-output", default=None)
    parser.add_argument("--promotability-output", default=None)
    parser.add_argument("--false-positive-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def phase3_manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "phase3_r2only_manifest_v1.json"


def leakage_audit_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "leakage_audit_v1.json"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def row_key(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def num(row: dict[str, Any], field: str) -> float | None:
    value = p3g.feature_value(row, field)
    if value is None or isinstance(value, bool):
        value = row.get(field)
    return common.float_or_none(value)


def bool_value(row: dict[str, Any], field: str) -> bool | None:
    value = row.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    return None


def actor_count(row: dict[str, Any]) -> float | None:
    buyers = num(row, "evidence_unique_buyers")
    if buyers is not None:
        return buyers
    return num(row, "evidence_unique_signers")


def base_score(row: dict[str, Any], specs: list[dict[str, Any]]) -> float | None:
    score, _availability = topk_drift.score_training_row(row, specs)
    return score


def score_map(rows: list[dict[str, Any]], specs: list[dict[str, Any]]) -> dict[str, float]:
    out: dict[str, float] = {}
    for row in rows:
        key = row_key(row)
        if not key:
            continue
        score = base_score(row, specs)
        if score is not None:
            out[key] = score
    return out


def ordered(rows: list[dict[str, Any]], scores: dict[str, float]) -> list[dict[str, Any]]:
    return sorted(
        [row for row in rows if row_key(row) in scores],
        key=lambda row: (
            -scores[row_key(row)],
            common.int_or_none(row.get("birth_ts_ms")) or common.int_or_none(row.get("decision_ts_ms")) or 0,
            row_key(row),
        ),
    )


def selected_metric(rows: list[dict[str, Any]], selected: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    precision = selected_positive / len(selected) if selected else None
    base_rate = positives / len(rows) if rows else None
    return {
        "denominator_rows": len(rows),
        "selected_count": len(selected),
        "positive_count": selected_positive,
        "negative_count": len(selected) - selected_positive,
        "precision": precision,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": (
            precision - base_rate if isinstance(precision, float) and isinstance(base_rate, float) else None
        ),
        "accept_rate": len(selected) / len(rows) if rows else None,
        "ev_proxy_pct": (
            precision * TARGET_NET_PCT - (1.0 - precision) * STOP_NET_PCT
            if isinstance(precision, float)
            else None
        ),
    }


def topk_metrics(rows: list[dict[str, Any]], scores: dict[str, float]) -> dict[str, Any]:
    ranked = ordered(rows, scores)
    return {f"top{k}": selected_metric(rows, ranked[: min(k, len(ranked))]) for k in TOP_K}


def quantile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered_values = sorted(values)
    index = int((len(ordered_values) - 1) * pct)
    return ordered_values[index]


def unique_actor_bucket(row: dict[str, Any], all_rows: list[dict[str, Any]]) -> str:
    value = actor_count(row)
    if value is None:
        return "missing"
    values = [current for item in all_rows if (current := actor_count(item)) is not None]
    q1 = quantile(values, 0.25)
    q2 = quantile(values, 0.50)
    q3 = quantile(values, 0.75)
    if q1 is None or q2 is None or q3 is None:
        return "known"
    if value <= q1:
        return "q1"
    if value <= q2:
        return "q2"
    if value <= q3:
        return "q3"
    return "q4"


def market_cap_bucket(row: dict[str, Any]) -> str:
    value = num(row, "gk_current_market_cap_sol")
    if value is None:
        return "missing"
    if value < 30.0:
        return "low"
    if value < 100.0:
        return "mid"
    return "high"


def evidence_sufficient(row: dict[str, Any]) -> bool:
    return row.get("evidence_sufficiency_status") == "sufficient"


def score_eligible(row: dict[str, Any]) -> bool:
    return row.get("score_eligibility_status") != "score_invalid_insufficient_market_evidence"


def concentration_extreme(row: dict[str, Any]) -> bool:
    hhi = num(row, "gk_hhi")
    top3 = num(row, "gk_top3_volume_pct")
    return (hhi is not None and hhi >= 0.90) or (top3 is not None and top3 >= 0.95)


def sell_pressure_extreme(row: dict[str, Any]) -> bool:
    sell_share = num(row, "sell_share")
    if sell_share is None:
        sell_share = num(row, "evidence_sell_share")
    sell_buy = num(row, "gk_sell_buy_ratio")
    return (sell_share is not None and sell_share >= 0.55) or (sell_buy is not None and sell_buy >= 2.0)


def dev_toxic(row: dict[str, Any]) -> bool:
    dev_tx = num(row, "gk_dev_tx_ratio")
    dev_vol = num(row, "gk_dev_volume_ratio")
    return (
        bool_value(row, "gk_dev_has_sold") is True
        or (dev_tx is not None and dev_tx >= 0.80)
        or (dev_vol is not None and dev_vol >= 0.80)
    )


def market_evidence_ok(row: dict[str, Any]) -> bool:
    tx_count = num(row, "evidence_tx_count")
    buy_count = num(row, "evidence_buy_count")
    actors = actor_count(row)
    volume = num(row, "evidence_total_volume_sol")
    return (
        tx_count is not None
        and tx_count >= 3
        and buy_count is not None
        and buy_count >= 2
        and actors is not None
        and actors >= 2
        and volume is not None
    )


def anti_junk(row: dict[str, Any]) -> bool:
    return evidence_sufficient(row) and score_eligible(row) and market_evidence_ok(row) and not dev_toxic(row)


def candidate_defs() -> list[dict[str, Any]]:
    return [
        {
            "candidate_id": "low_market_cap_evidence_sufficient_antijunk",
            "kind": "promotable_probe",
            "description": "market_cap=low, evidence sufficient, anti-junk, rank by current score",
        },
        {
            "candidate_id": "unique_actors_q3_evidence_sufficient",
            "kind": "promotable_probe",
            "description": "unique_actors=q3, evidence sufficient, no extreme concentration/sell pressure",
        },
        {
            "candidate_id": "current_regime_unique_actors_q1_q2_score_probe",
            "kind": "current_regime_probe",
            "description": "diagnostic current-regime probe: unique_actors=q1/q2 with sufficient evidence, rank by current score",
        },
        {
            "candidate_id": "current_regime_unique_actors_q1_q2_q3_score_probe",
            "kind": "current_regime_probe",
            "description": "diagnostic current-regime probe: unique_actors=q1/q2/q3 with sufficient evidence, rank by current score",
        },
        {
            "candidate_id": "low_market_cap_unique_actors_q3_intersection",
            "kind": "promotable_probe",
            "description": "market_cap=low and unique_actors=q3 intersection with anti-junk",
        },
        {
            "candidate_id": "risky_positive_segments_diagnostic_only",
            "kind": "diagnostic_only",
            "description": "diagnostic union of insufficient evidence, q1 activity, and high dev-volume segments",
        },
    ]


def risky_segment(row: dict[str, Any], rows: list[dict[str, Any]]) -> bool:
    return (
        row.get("evidence_sufficiency_status") == "insufficient"
        or activity_bucket(row, rows, "evidence_tx_count") == "q1"
        or activity_bucket(row, rows, "evidence_buy_count") == "q1"
        or market_segment(row, "dev_volume_ratio") == "high"
    )


def activity_bucket(row: dict[str, Any], rows: list[dict[str, Any]], field: str) -> str:
    value = num(row, field)
    if value is None:
        return "missing"
    values = [current for item in rows if (current := num(item, field)) is not None]
    q1 = quantile(values, 0.25)
    q2 = quantile(values, 0.50)
    q3 = quantile(values, 0.75)
    if q1 is None or q2 is None or q3 is None:
        return "known"
    if value <= q1:
        return "q1"
    if value <= q2:
        return "q2"
    if value <= q3:
        return "q3"
    return "q4"


def market_segment(row: dict[str, Any], name: str) -> str:
    if name == "dev_volume_ratio":
        value = num(row, "gk_dev_volume_ratio")
        if value is None:
            return "missing"
        if value < 0.05:
            return "low"
        if value < 0.50:
            return "mid"
        return "high"
    raise ValueError(name)


def row_matches_candidate(row: dict[str, Any], rows: list[dict[str, Any]], candidate_id: str) -> bool:
    unique_bucket = unique_actor_bucket(row, rows)
    if candidate_id == "low_market_cap_evidence_sufficient_antijunk":
        return market_cap_bucket(row) == "low" and anti_junk(row)
    if candidate_id == "unique_actors_q3_evidence_sufficient":
        return (
            unique_bucket == "q3"
            and evidence_sufficient(row)
            and score_eligible(row)
            and market_evidence_ok(row)
            and not concentration_extreme(row)
            and not sell_pressure_extreme(row)
            and not dev_toxic(row)
        )
    if candidate_id == "current_regime_unique_actors_q1_q2_score_probe":
        return unique_bucket in {"q1", "q2"}
    if candidate_id == "current_regime_unique_actors_q1_q2_q3_score_probe":
        return unique_bucket in {"q1", "q2", "q3"}
    if candidate_id == "low_market_cap_unique_actors_q3_intersection":
        return market_cap_bucket(row) == "low" and unique_bucket == "q3" and anti_junk(row)
    if candidate_id == "risky_positive_segments_diagnostic_only":
        return risky_segment(row, rows)
    raise ValueError(candidate_id)


def candidate_scores(
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    candidate_id: str,
) -> dict[str, float]:
    rows_by_id = {row_key(row): row for row in rows if row_key(row)}
    return {
        key: score
        for key, score in scores.items()
        if (row := rows_by_id.get(key)) is not None
        and row_matches_candidate(row, rows, candidate_id)
    }


def risk_flags(row: dict[str, Any]) -> list[str]:
    flags = []
    if row.get("score_eligibility_status") == "score_invalid_insufficient_market_evidence":
        flags.append("insufficient_evidence")
    if not market_evidence_ok(row):
        flags.append("low_tx_buy_or_actor_evidence")
    if dev_toxic(row):
        flags.append("high_dev_or_dev_sold")
    if sell_pressure_extreme(row):
        flags.append("high_sell_pressure")
    if concentration_extreme(row):
        flags.append("extreme_concentration")
    if num(row, "gk_hhi") is None or num(row, "gk_top3_volume_pct") is None:
        flags.append("concentration_missing")
    return flags


def false_positive_review(
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    candidate_id: str,
    run: str,
) -> list[dict[str, Any]]:
    ranked = ordered(rows, scores)
    out = []
    for topk in (25, 50):
        for rank, row in enumerate(ranked[: min(topk, len(ranked))], start=1):
            if label_positive(row):
                continue
            flags = risk_flags(row)
            out.append(
                {
                    "candidate_id": candidate_id,
                    "run": run,
                    "topk": topk,
                    "rank": rank,
                    "row_candidate_id": row_key(row),
                    "base_mint": row.get("base_mint"),
                    "score": scores.get(row_key(row)),
                    "risk_flags": ";".join(flags),
                    "toxic_false_positive": bool(flags),
                    "evidence_sufficiency_status": row.get("evidence_sufficiency_status"),
                    "score_eligibility_status": row.get("score_eligibility_status"),
                    "evidence_tx_count": num(row, "evidence_tx_count"),
                    "evidence_buy_count": num(row, "evidence_buy_count"),
                    "evidence_unique_buyers": num(row, "evidence_unique_buyers"),
                    "gk_dev_volume_ratio": num(row, "gk_dev_volume_ratio"),
                    "sell_share": num(row, "sell_share"),
                }
            )
    return out


def candidate_payload(
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    candidate_id: str,
) -> dict[str, Any]:
    ranked = ordered(rows, scores)
    top25 = ranked[: min(25, len(ranked))]
    top50 = ranked[: min(50, len(ranked))]
    top25_sufficient = sum(1 for row in top25 if evidence_sufficient(row))
    fp_top50 = [row for row in top50 if not label_positive(row)]
    fp_toxic = sum(1 for row in fp_top50 if risk_flags(row))
    risk_counts: Counter[str] = Counter()
    for row in fp_top50:
        for flag in risk_flags(row):
            risk_counts[flag] += 1
    return {
        "eligible_rows": len(scores),
        "eligible_rate": len(scores) / len(rows) if rows else None,
        "topk": topk_metrics(rows, scores),
        "top25_evidence_sufficient_rate": top25_sufficient / len(top25) if top25 else None,
        "top50_false_positive_count": len(fp_top50),
        "top50_toxic_false_positive_count": fp_toxic,
        "top50_toxic_false_positive_rate": fp_toxic / len(fp_top50) if fp_top50 else 0.0,
        "false_positive_risk_counts": common.counter_dict(risk_counts),
        "segment_kind": candidate_kind(candidate_id),
    }


def candidate_kind(candidate_id: str) -> str:
    for item in candidate_defs():
        if item["candidate_id"] == candidate_id:
            return str(item["kind"])
    return "unknown"


def classify_promotability(
    candidate_id: str,
    train: dict[str, Any],
    validation: dict[str, Any],
    args: argparse.Namespace,
) -> dict[str, Any]:
    fail_reasons: list[str] = []
    if candidate_kind(candidate_id) == "current_regime_probe":
        train_top50 = train["topk"]["top50"].get("lift_vs_base_rate_pp")
        validation_top50 = validation["topk"]["top50"].get("lift_vs_base_rate_pp")
        if (
            isinstance(train_top50, float)
            and train_top50 > 0.0
            and isinstance(validation_top50, float)
            and validation_top50 > 0.0
        ):
            top25_sufficient = validation.get("top25_evidence_sufficient_rate")
            train_top25_sufficient = train.get("top25_evidence_sufficient_rate")
            if (
                not isinstance(top25_sufficient, float)
                or top25_sufficient < args.min_topk_evidence_sufficient_rate
                or not isinstance(train_top25_sufficient, float)
                or train_top25_sufficient < args.min_topk_evidence_sufficient_rate
            ):
                return {
                    "promotability_status": "REQUIRES_LABEL_REVIEW",
                    "label_review_required": True,
                    "fail_reasons": ["current_regime_probe_uses_insufficient_evidence_segment"],
                }
            return {
                "promotability_status": "FRESH_VALIDATION_REQUIRED",
                "label_review_required": False,
                "fail_reasons": ["current_regime_probe_not_promotable_without_fresh_run"],
            }
        return {
            "promotability_status": "NO_STABLE_EDGE",
            "label_review_required": False,
            "fail_reasons": ["current_regime_probe_not_directionally_positive"],
        }
    if candidate_kind(candidate_id) == "diagnostic_only":
        if validation["topk"]["top25"].get("lift_vs_base_rate_pp", 0.0) > 0.0:
            return {
                "promotability_status": "REQUIRES_LABEL_REVIEW",
                "label_review_required": True,
                "fail_reasons": ["risky_segment_has_r2_lift"],
            }
        return {
            "promotability_status": "DIAGNOSTIC_ONLY",
            "label_review_required": True,
            "fail_reasons": ["risky_segment_not_promotable"],
        }
    train_top25 = train["topk"]["top25"].get("lift_vs_base_rate_pp")
    validation_top25 = validation["topk"]["top25"].get("lift_vs_base_rate_pp")
    validation_top50 = validation["topk"]["top50"].get("lift_vs_base_rate_pp")
    if not isinstance(train_top25, float) or train_top25 <= args.min_top25_lift_pp:
        fail_reasons.append("train_top25_lift_below_promotable_threshold")
    if not isinstance(validation_top25, float) or validation_top25 <= args.min_top25_lift_pp:
        fail_reasons.append("validation_top25_lift_below_promotable_threshold")
    if not isinstance(validation_top50, float) or validation_top50 < args.min_top50_lift_pp:
        fail_reasons.append("validation_top50_lift_below_promotable_threshold")
    if validation["eligible_rows"] < args.min_candidate_rows:
        fail_reasons.append("validation_segment_rows_below_minimum")
    top25_sufficient = validation.get("top25_evidence_sufficient_rate")
    if not isinstance(top25_sufficient, float) or top25_sufficient < args.min_topk_evidence_sufficient_rate:
        fail_reasons.append("top25_evidence_sufficient_rate_too_low")
    train_top25_sufficient = train.get("top25_evidence_sufficient_rate")
    if not isinstance(train_top25_sufficient, float) or train_top25_sufficient < args.min_topk_evidence_sufficient_rate:
        fail_reasons.append("train_top25_evidence_sufficient_rate_too_low")
    toxic_fp_rate = validation.get("top50_toxic_false_positive_rate")
    if isinstance(toxic_fp_rate, float) and toxic_fp_rate > args.max_toxic_false_positive_rate:
        fail_reasons.append("toxic_false_positive_rate_too_high")
    train_toxic_fp_rate = train.get("top50_toxic_false_positive_rate")
    if isinstance(train_toxic_fp_rate, float) and train_toxic_fp_rate > args.max_toxic_false_positive_rate:
        fail_reasons.append("train_toxic_false_positive_rate_too_high")
    if fail_reasons:
        return {
            "promotability_status": "NO_STABLE_EDGE",
            "label_review_required": False,
            "fail_reasons": fail_reasons,
        }
    return {
        "promotability_status": "PROMOTABLE_CANDIDATE",
        "label_review_required": False,
        "fail_reasons": [],
    }


def run_quality(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    manifest_path = phase3_manifest_path(root, scope)
    manifest = read_json_object(manifest_path) if manifest_path.exists() else {}
    leakage_status = manifest.get("leakage_audit_status")
    leakage_path = leakage_audit_path(root, scope)
    if leakage_status is None and leakage_path.exists():
        leakage_status = read_json_object(leakage_path).get("status")
    positives = sum(1 for row in rows if label_positive(row))
    return {
        "scope": scope,
        "denominator_rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": positives / len(rows) if rows else None,
        "leakage_audit_status": leakage_status,
        "leakage_clean": leakage_status == "PASS",
    }


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
        "# Segment-Specific Candidate",
        "",
        f"Status: {report['status']}",
        f"Business decision: {report['business_decision']}",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Promotability",
        "",
        "| candidate | status | validation top25 | validation top50 | validation rows |",
        "|---|---|---:|---:|---:|",
    ]
    for candidate_id, payload in report["candidates"].items():
        validation = payload["validation"]
        lines.append(
            "| {candidate} | {status} | {top25} | {top50} | {rows} |".format(
                candidate=candidate_id,
                status=payload["promotability"]["promotability_status"],
                top25=(
                    f"{validation['topk']['top25']['precision']:.4f}"
                    if isinstance(validation["topk"]["top25"].get("precision"), float)
                    else "n/a"
                ),
                top50=(
                    f"{validation['topk']['top50']['precision']:.4f}"
                    if isinstance(validation["topk"]["top50"].get("precision"), float)
                    else "n/a"
                ),
                rows=validation["eligible_rows"],
            )
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    specs, _thresholds = parity.parse_runtime_spec(root / args.rust_source)
    train_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.train_scope))))
    validation_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.validation_scope))))
    train_scores_all = score_map(train_rows, specs)
    validation_scores_all = score_map(validation_rows, specs)

    candidates: dict[str, Any] = {}
    grid_rows: list[dict[str, Any]] = []
    promotability_rows: list[dict[str, Any]] = []
    false_positive_rows: list[dict[str, Any]] = []
    promotable_ids: list[str] = []
    fresh_validation_ids: list[str] = []
    label_review_required = False

    for definition in candidate_defs():
        candidate_id = str(definition["candidate_id"])
        train_scores = candidate_scores(train_rows, train_scores_all, candidate_id)
        validation_scores = candidate_scores(validation_rows, validation_scores_all, candidate_id)
        train_payload = candidate_payload(train_rows, train_scores, candidate_id)
        validation_payload = candidate_payload(validation_rows, validation_scores, candidate_id)
        promotability = classify_promotability(candidate_id, train_payload, validation_payload, args)
        if promotability["promotability_status"] == "PROMOTABLE_CANDIDATE":
            promotable_ids.append(candidate_id)
        if promotability["promotability_status"] == "FRESH_VALIDATION_REQUIRED":
            fresh_validation_ids.append(candidate_id)
        if promotability.get("label_review_required"):
            label_review_required = True
        candidates[candidate_id] = {
            "description": definition["description"],
            "kind": definition["kind"],
            "train": train_payload,
            "validation": validation_payload,
            "promotability": promotability,
        }
        for run_name, payload in (("train", train_payload), ("validation", validation_payload)):
            for topk_name, topk_payload in payload["topk"].items():
                grid_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "run": run_name,
                        "topk": topk_name,
                        **topk_payload,
                    }
                )
        promotability_rows.append(
            {
                "candidate_id": candidate_id,
                "candidate_kind": definition["kind"],
                "promotability_status": promotability["promotability_status"],
                "label_review_required": promotability["label_review_required"],
                "fail_reasons": ";".join(promotability["fail_reasons"]),
                "validation_eligible_rows": validation_payload["eligible_rows"],
                "validation_top25_lift_pp": validation_payload["topk"]["top25"].get("lift_vs_base_rate_pp"),
                "validation_top50_lift_pp": validation_payload["topk"]["top50"].get("lift_vs_base_rate_pp"),
                "validation_top50_toxic_false_positive_rate": validation_payload["top50_toxic_false_positive_rate"],
            }
        )
        false_positive_rows.extend(false_positive_review(train_rows, train_scores, candidate_id, "train"))
        false_positive_rows.extend(false_positive_review(validation_rows, validation_scores, candidate_id, "validation"))

    train_quality = run_quality(root, args.train_scope, train_rows)
    validation_quality = run_quality(root, args.validation_scope, validation_rows)
    if promotable_ids:
        status = "P4E_PROMOTABLE_CANDIDATE_FOUND"
        business_decision = "FREEZE_SEGMENT_CANDIDATE_CONTRACT_OFFLINE"
    elif fresh_validation_ids:
        status = "P4E_CURRENT_REGIME_CANDIDATE_REQUIRES_FRESH_VALIDATION"
        business_decision = "DO_NOT_RUN_RUNTIME_FREEZE_DIAGNOSTIC_PROBE"
    elif label_review_required:
        status = "P4E_REQUIRES_LABEL_REVIEW"
        business_decision = "REVIEW_R2_LABEL_OR_STRATEGY_CONTRACT"
    else:
        status = "P4E_NO_PROMOTABLE_SEGMENT_CANDIDATE"
        business_decision = "DO_NOT_RUN_RUNTIME"
    if not train_quality["leakage_clean"] or not validation_quality["leakage_clean"]:
        status = "P4E_REQUIRES_LABEL_REVIEW"
        business_decision = "DO_NOT_RUN_RUNTIME"

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    md_output = Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT
    grid_output = Path(args.grid_output) if args.grid_output else out_dir / GRID_ARTIFACT
    promotability_output = (
        Path(args.promotability_output) if args.promotability_output else out_dir / PROMOTABILITY_ARTIFACT
    )
    false_positive_output = (
        Path(args.false_positive_output) if args.false_positive_output else out_dir / FALSE_POSITIVE_ARTIFACT
    )
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": business_decision,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "promotable_candidate_ids": promotable_ids,
        "fresh_validation_candidate_ids": fresh_validation_ids,
        "label_review_required": label_review_required,
        "run_quality": {"train": train_quality, "validation": validation_quality},
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "builds_runtime_score": False,
            "changes_runtime": False,
            "changes_gatekeeper": False,
            "changes_execution": False,
            "changes_send_path": False,
            "production_promotion_allowed": False,
        },
        "acceptance": {
            "min_top25_lift_pp": args.min_top25_lift_pp,
            "min_top50_lift_pp": args.min_top50_lift_pp,
            "min_candidate_rows": args.min_candidate_rows,
            "min_topk_evidence_sufficient_rate": args.min_topk_evidence_sufficient_rate,
            "max_toxic_false_positive_rate": args.max_toxic_false_positive_rate,
        },
        "candidates": candidates,
        "outputs": {
            ARTIFACT: str(output),
            MD_ARTIFACT: str(md_output),
            GRID_ARTIFACT: str(grid_output),
            PROMOTABILITY_ARTIFACT: str(promotability_output),
            FALSE_POSITIVE_ARTIFACT: str(false_positive_output),
        },
    }
    write_json(output, report)
    write_markdown(md_output, report)
    write_csv(grid_output, grid_rows)
    write_csv(promotability_output, promotability_rows)
    write_csv(false_positive_output, false_positive_rows)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
