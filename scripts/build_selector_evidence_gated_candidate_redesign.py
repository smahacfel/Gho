#!/usr/bin/env python3
"""Build P4C evidence-gated selector candidate redesign report.

P4C is offline-only. It evaluates at most three explicit candidates built on
P4B evidence sufficiency fields. It does not change Gatekeeper, runtime score,
execution, send path, or production thresholds.
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


ARTIFACT = "evidence_gated_candidate_redesign_v1"
MD_ARTIFACT = "EVIDENCE_GATED_CANDIDATE_REDESIGN.md"
GRID_ARTIFACT = "evidence_gated_candidate_grid_v1.csv"
FALSE_POSITIVE_ARTIFACT = "evidence_gated_candidate_false_positives_v1.csv"
REJECTED_POSITIVE_ARTIFACT = "evidence_gated_candidate_rejected_positives_v1.csv"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
TARGET_NET_PCT = 40.0
STOP_NET_PCT = 40.0
TOP_K = (10, 25, 50)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--rust-source", default=DEFAULT_RUST_SOURCE)
    parser.add_argument("--min-top25-lift-pp", type=float, default=0.10)
    parser.add_argument("--min-top50-lift-pp", type=float, default=0.0)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--grid-output", default=None)
    parser.add_argument("--false-positive-output", default=None)
    parser.add_argument("--rejected-positive-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def phase3_manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "phase3_r2only_manifest_v1.json"


def denominator_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if baseline.r2only_denominator(row)]


def row_key(row: dict[str, Any]) -> str:
    return common.str_or_none(row.get("candidate_id")) or ""


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def num(row: dict[str, Any], field: str) -> float | None:
    value = p3g.feature_value(row, field)
    if value is None:
        value = common.float_or_none(row.get(field))
    return value


def evidence_actor_count(row: dict[str, Any]) -> float | None:
    buyers = num(row, "evidence_unique_buyers")
    if buyers is not None:
        return buyers
    return num(row, "evidence_unique_signers")


def bool_feature(row: dict[str, Any], field: str) -> bool | None:
    value = row.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    return None


def base_score(row: dict[str, Any], specs: list[dict[str, Any]]) -> float | None:
    score, _availability = topk_drift.score_training_row(row, specs)
    return score


def hard_reject_reasons(row: dict[str, Any], *, strict: bool) -> list[str]:
    reasons: list[str] = []
    tx_count = num(row, "evidence_tx_count")
    buy_count = num(row, "evidence_buy_count")
    actor_count = evidence_actor_count(row)
    volume = num(row, "evidence_total_volume_sol")
    if row.get("score_eligibility_status") == "score_invalid_insufficient_market_evidence":
        reasons.extend(str(item) for item in row.get("score_eligibility_reasons") or [])
    if row.get("evidence_sufficiency_status") not in {"sufficient", "partial"}:
        reasons.append(f"evidence_status:{row.get('evidence_sufficiency_status')}")
    if strict and row.get("evidence_sufficiency_status") != "sufficient":
        reasons.append("strict_requires_sufficient_evidence")
    if tx_count is None or tx_count < (5 if strict else 3):
        reasons.append("insufficient_tx_count")
    if buy_count is None or buy_count < (3 if strict else 2):
        reasons.append("insufficient_buy_count")
    if actor_count is None or actor_count < (3 if strict else 2):
        reasons.append("insufficient_unique_actors")
    if volume is None or volume < (0.25 if strict else 0.10):
        reasons.append("insufficient_volume")
    if num(row, "gk_top3_volume_pct") is not None and (num(row, "gk_top3_volume_pct") or 0.0) >= 0.995:
        reasons.append("extreme_top3_concentration")
    if num(row, "gk_hhi") is not None and (num(row, "gk_hhi") or 0.0) >= 0.95:
        reasons.append("extreme_hhi_concentration")
    dev_tx = num(row, "gk_dev_tx_ratio")
    dev_volume = num(row, "gk_dev_volume_ratio")
    if dev_tx is not None and dev_tx >= 0.95:
        reasons.append("dev_only_tx_ratio")
    if dev_volume is not None and dev_volume >= 0.95:
        reasons.append("dev_only_volume_ratio")
    if bool_feature(row, "gk_dev_has_sold") is True and strict:
        reasons.append("dev_has_sold")
    return sorted(set(reasons))


def candidate_defs() -> list[dict[str, Any]]:
    return [
        {
            "candidate_id": "strict_precision_candidate",
            "description": "sufficient evidence only, strict no-market/dev/concentration hard rejects, current combined rank",
            "mode": "strict",
        },
        {
            "candidate_id": "balanced_candidate",
            "description": "sufficient or explicit partial evidence, moderate hard rejects, risk-penalized combined rank",
            "mode": "balanced",
        },
        {
            "candidate_id": "conservative_hard_reject_candidate",
            "description": "strict hard-reject screen with flow/GK core blended rank",
            "mode": "conservative",
        },
    ]


def score_candidate(row: dict[str, Any], specs: list[dict[str, Any]], candidate: dict[str, Any]) -> tuple[float | None, list[str]]:
    mode = str(candidate["mode"])
    reasons = hard_reject_reasons(row, strict=mode in {"strict", "conservative"})
    if reasons:
        return None, reasons
    score = base_score(row, specs)
    if score is None:
        return None, ["score_unavailable"]
    if mode == "balanced":
        penalty = 0.0
        hhi = num(row, "gk_hhi")
        top3 = num(row, "gk_top3_volume_pct")
        sell_share = num(row, "evidence_sell_share")
        dev_volume = num(row, "gk_dev_volume_ratio")
        if hhi is not None and hhi > 0.80:
            penalty += 0.06
        if top3 is not None and top3 > 0.90:
            penalty += 0.06
        if sell_share is not None and sell_share > 0.50:
            penalty += 0.04
        if dev_volume is not None and dev_volume > 0.50:
            penalty += 0.06
        score = max(0.0, score - penalty)
    elif mode == "conservative":
        flow_terms = [
            num(row, "net_quote_in_15s"),
            num(row, "net_quote_in_30s"),
            num(row, "trade_rate"),
            num(row, "unique_buyers"),
        ]
        gk_terms = [
            num(row, "gk_bonding_progress_pct"),
            num(row, "gk_current_market_cap_sol"),
            num(row, "gk_price_change_ratio"),
        ]
        present = [value for value in flow_terms + gk_terms if value is not None]
        if not present:
            return None, ["score_unavailable_no_rank_features"]
        # Keep the score bounded and deterministic; this is a comparator, not a trained model.
        score = (score * 0.60) + min(0.40, len(present) / 7.0 * 0.40)
    return score, []


def candidate_scores(
    rows: list[dict[str, Any]],
    specs: list[dict[str, Any]],
    candidate: dict[str, Any],
) -> tuple[dict[str, float], Counter[str], dict[str, list[str]]]:
    scores: dict[str, float] = {}
    reason_by_id: dict[str, list[str]] = {}
    reason_counts: Counter[str] = Counter()
    for row in rows:
        key = row_key(row)
        if not key:
            continue
        score, reasons = score_candidate(row, specs, candidate)
        if reasons:
            reason_by_id[key] = reasons
            for reason in reasons:
                reason_counts[reason] += 1
            continue
        if score is not None:
            scores[key] = score
    return scores, reason_counts, reason_by_id


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
        "recall": selected_positive / positives if positives else None,
    }


def topk_metrics(rows: list[dict[str, Any]], scores: dict[str, float]) -> dict[str, Any]:
    ranked = ordered(rows, scores)
    return {
        f"top{k}": selected_metric(rows, ranked[: min(k, len(ranked))])
        for k in TOP_K
    }


def score_deciles(rows: list[dict[str, Any]], scores: dict[str, float]) -> list[dict[str, Any]]:
    ranked = ordered(rows, scores)
    if not ranked:
        return []
    out = []
    for idx in range(10):
        start = int(len(ranked) * idx / 10)
        end = int(len(ranked) * (idx + 1) / 10)
        bucket = ranked[start:end]
        metric = selected_metric(rows, bucket)
        out.append({"decile": idx + 1, **metric})
    return out


def false_positive_rows(rows: list[dict[str, Any]], scores: dict[str, float], candidate_id: str, run: str) -> list[dict[str, Any]]:
    ranked = ordered(rows, scores)
    out = []
    for topk in TOP_K:
        for rank, row in enumerate(ranked[: min(topk, len(ranked))], start=1):
            if label_positive(row):
                continue
            out.append(
                {
                    "candidate_id": candidate_id,
                    "run": run,
                    "topk": topk,
                    "rank": rank,
                    "row_candidate_id": row.get("candidate_id"),
                    "base_mint": row.get("base_mint"),
                    "score": scores.get(row_key(row)),
                    "r2_label": row.get("r2_label"),
                    "evidence_sufficiency_status": row.get("evidence_sufficiency_status"),
                    "score_eligibility_status": row.get("score_eligibility_status"),
                    "gk_hhi": num(row, "gk_hhi"),
                    "gk_top3_volume_pct": num(row, "gk_top3_volume_pct"),
                    "gk_dev_volume_ratio": num(row, "gk_dev_volume_ratio"),
                    "evidence_tx_count": num(row, "evidence_tx_count"),
                    "evidence_buy_count": num(row, "evidence_buy_count"),
                    "evidence_unique_buyers": num(row, "evidence_unique_buyers"),
                }
            )
    return out


def rejected_positive_rows(
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    reasons: dict[str, list[str]],
    candidate_id: str,
    run: str,
) -> list[dict[str, Any]]:
    out = []
    for row in rows:
        key = row_key(row)
        if not label_positive(row) or key in scores:
            continue
        out.append(
            {
                "candidate_id": candidate_id,
                "run": run,
                "row_candidate_id": key,
                "base_mint": row.get("base_mint"),
                "reject_reasons": ";".join(reasons.get(key) or ["not_scored"]),
                "evidence_sufficiency_status": row.get("evidence_sufficiency_status"),
                "score_eligibility_status": row.get("score_eligibility_status"),
            }
        )
    return out


def candidate_payload(
    rows: list[dict[str, Any]],
    scores: dict[str, float],
    reason_counts: Counter[str],
) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    invalid = sum(
        1
        for row in rows
        if row.get("score_eligibility_status") == "score_invalid_insufficient_market_evidence"
    )
    missing_reason_counts: Counter[str] = Counter()
    for row in rows:
        for reason in row.get("score_eligibility_reasons") or []:
            missing_reason_counts[str(reason)] += 1
    return {
        "denominator_rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": positives / len(rows) if rows else None,
        "eligible_rows": len(scores),
        "eligible_rate": len(scores) / len(rows) if rows else None,
        "invalid_insufficient_market_evidence": invalid,
        "hard_reject_reason_counts": common.counter_dict(reason_counts),
        "missing_evidence_reason_counts": common.counter_dict(missing_reason_counts),
        "topk": topk_metrics(rows, scores),
        "score_deciles": score_deciles(rows, scores),
    }


def candidate_stability(train: dict[str, Any], validation: dict[str, Any], args: argparse.Namespace) -> dict[str, Any]:
    fail_reasons: list[str] = []
    train_top25 = train["topk"]["top25"]
    validation_top25 = validation["topk"]["top25"]
    validation_top50 = validation["topk"]["top50"]
    train_lift = train_top25.get("lift_vs_base_rate_pp")
    validation_top25_lift = validation_top25.get("lift_vs_base_rate_pp")
    validation_top50_lift = validation_top50.get("lift_vs_base_rate_pp")
    if not isinstance(validation_top25_lift, float) or validation_top25_lift < args.min_top25_lift_pp:
        fail_reasons.append("validation_top25_lift_below_threshold")
    if not isinstance(validation_top50_lift, float) or validation_top50_lift <= args.min_top50_lift_pp:
        fail_reasons.append("validation_top50_lift_not_positive")
    if not isinstance(train_lift, float) or train_lift <= 0.0:
        fail_reasons.append("train_top25_direction_not_positive")
    if not isinstance(validation_top25.get("precision"), float) or validation_top25["selected_count"] < 10:
        fail_reasons.append("validation_top25_too_few_selected_or_unresolved")
    return {
        "status": "STABLE" if not fail_reasons else "UNSTABLE",
        "fail_reasons": fail_reasons,
    }


def read_json_object(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def run_quality(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    positives = sum(1 for row in rows if label_positive(row))
    base_rate = positives / len(rows) if rows else None
    manifest = read_json_object(phase3_manifest_path(root, scope)) if phase3_manifest_path(root, scope).exists() else {}
    leakage_status = manifest.get("leakage_audit_status")
    return {
        "scope": scope,
        "denominator_rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": base_rate,
        "leakage_audit_status": leakage_status,
        "leakage_clean": leakage_status == "PASS",
        "phase3_manifest": str(phase3_manifest_path(root, scope)),
        "phase3_fail_reasons": manifest.get("fail_reasons") or manifest.get("phase3_fail_reasons") or [],
        "label_definition_review_required": (
            base_rate is None
            or len(rows) == 0
            or positives == 0
            or positives == len(rows)
            or base_rate < 0.05
            or base_rate > 0.80
        ),
    }


def final_status(
    stable_ids: list[str],
    train_quality: dict[str, Any],
    validation_quality: dict[str, Any],
    candidates: dict[str, Any],
) -> tuple[str, str, list[str]]:
    fail_reasons: list[str] = []
    if not train_quality["leakage_clean"] or not validation_quality["leakage_clean"]:
        fail_reasons.append("leakage_audit_not_clean")
    if train_quality["label_definition_review_required"] or validation_quality["label_definition_review_required"]:
        fail_reasons.append("label_distribution_requires_review")
    if len(candidates) > 3:
        fail_reasons.append("candidate_count_exceeds_three")
    if fail_reasons:
        if "label_distribution_requires_review" in fail_reasons:
            return "P4C_LABEL_DEFINITION_REVIEW_REQUIRED", "DO_NOT_RUN_RUNTIME", fail_reasons
        return "P4C_NO_STABLE_CANDIDATE_FOUND", "DO_NOT_RUN_RUNTIME", fail_reasons
    if stable_ids:
        return "P4C_STABLE_CANDIDATE_FOUND", "FRESH_VALIDATION_REQUIRED_BEFORE_RUNTIME", []

    validation_lifts = []
    for payload in candidates.values():
        top25_lift = payload["validation"]["topk"]["top25"].get("lift_vs_base_rate_pp")
        top50_lift = payload["validation"]["topk"]["top50"].get("lift_vs_base_rate_pp")
        validation_lifts.extend(value for value in (top25_lift, top50_lift) if isinstance(value, float))
    if validation_lifts and max(validation_lifts) <= 0.0:
        return (
            "P4C_NEEDS_NEW_FEATURE_FAMILY",
            "DO_NOT_RUN_RUNTIME",
            ["no_candidate_lifted_validation_above_base_rate"],
        )
    return (
        "P4C_NO_STABLE_CANDIDATE_FOUND",
        "DO_NOT_RUN_RUNTIME",
        ["no_evidence_gated_candidate_met_crossrun_stability_gate"],
    )


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
        "# Evidence-Gated Candidate Redesign",
        "",
        f"Status: {report['status']}",
        f"Business decision: {report['business_decision']}",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Candidate Summary",
        "",
        "| candidate | stability | r19 top25 | r21 top25 | r21 top50 | r21 eligible |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for candidate_id, payload in report["candidates"].items():
        train_top25 = payload["train"]["topk"]["top25"].get("precision")
        validation_top25 = payload["validation"]["topk"]["top25"].get("precision")
        validation_top50 = payload["validation"]["topk"]["top50"].get("precision")
        lines.append(
            "| {candidate} | {stability} | {train_top25} | {validation_top25} | {validation_top50} | {eligible} |".format(
                candidate=candidate_id,
                stability=payload["stability"]["status"],
                train_top25=f"{train_top25:.4f}" if isinstance(train_top25, float) else "n/a",
                validation_top25=f"{validation_top25:.4f}" if isinstance(validation_top25, float) else "n/a",
                validation_top50=f"{validation_top50:.4f}" if isinstance(validation_top50, float) else "n/a",
                eligible=payload["validation"]["eligible_rows"],
            )
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    specs, _thresholds = parity.parse_runtime_spec(root / args.rust_source)
    train_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.train_scope))))
    validation_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.validation_scope))))
    train_quality = run_quality(root, args.train_scope, train_rows)
    validation_quality = run_quality(root, args.validation_scope, validation_rows)

    candidates: dict[str, Any] = {}
    grid_rows: list[dict[str, Any]] = []
    false_positive_out: list[dict[str, Any]] = []
    rejected_positive_out: list[dict[str, Any]] = []
    stable_ids: list[str] = []
    defs = candidate_defs()
    for definition in defs:
        candidate_id = str(definition["candidate_id"])
        payload: dict[str, Any] = {
            "description": definition["description"],
            "mode": definition["mode"],
        }
        per_run: dict[str, dict[str, Any]] = {}
        for run_name, rows in (("train", train_rows), ("validation", validation_rows)):
            scores, reason_counts, reasons_by_id = candidate_scores(rows, specs, definition)
            current = candidate_payload(rows, scores, reason_counts)
            per_run[run_name] = current
            payload[run_name] = current
            false_positive_out.extend(false_positive_rows(rows, scores, candidate_id, run_name))
            rejected_positive_out.extend(rejected_positive_rows(rows, scores, reasons_by_id, candidate_id, run_name))
            for topk_name, topk_payload in current["topk"].items():
                grid_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "run": run_name,
                        "topk": topk_name,
                        "candidate_count": len(defs),
                        **topk_payload,
                    }
                )
        stability = candidate_stability(per_run["train"], per_run["validation"], args)
        payload["stability"] = stability
        if stability["status"] == "STABLE":
            stable_ids.append(candidate_id)
        candidates[candidate_id] = payload

    status, business_decision, global_fail_reasons = final_status(
        stable_ids,
        train_quality,
        validation_quality,
        candidates,
    )

    report = {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": business_decision,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "candidate_count": len(defs),
        "stable_candidate_ids": stable_ids,
        "run_quality": {
            "train": train_quality,
            "validation": validation_quality,
        },
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "trained_model": False,
            "changed_gatekeeper": False,
            "changed_runtime_score": False,
            "changed_execution": False,
            "changed_send_path": False,
            "production_promotion_allowed": False,
        },
        "acceptance": {
            "min_top25_lift_pp": args.min_top25_lift_pp,
            "min_top50_lift_pp": args.min_top50_lift_pp,
            "candidate_count_max": 3,
            "candidate_count_ok": len(defs) <= 3,
            "fail_reasons": global_fail_reasons,
        },
        "inputs": {
            "train_training_view": str(training_view_path(root, args.train_scope)),
            "validation_training_view": str(training_view_path(root, args.validation_scope)),
            "rust_source": str(root / args.rust_source),
        },
        "candidates": candidates,
        "outputs": {},
    }

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    md_output = Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT
    grid_output = Path(args.grid_output) if args.grid_output else out_dir / GRID_ARTIFACT
    false_positive_output = (
        Path(args.false_positive_output)
        if args.false_positive_output
        else out_dir / FALSE_POSITIVE_ARTIFACT
    )
    rejected_positive_output = (
        Path(args.rejected_positive_output)
        if args.rejected_positive_output
        else out_dir / REJECTED_POSITIVE_ARTIFACT
    )
    report["outputs"] = {
        ARTIFACT: str(output),
        MD_ARTIFACT: str(md_output),
        GRID_ARTIFACT: str(grid_output),
        FALSE_POSITIVE_ARTIFACT: str(false_positive_output),
        REJECTED_POSITIVE_ARTIFACT: str(rejected_positive_output),
    }
    write_json(output, report)
    write_markdown(md_output, report)
    write_csv(grid_output, grid_rows)
    write_csv(false_positive_output, false_positive_out)
    write_csv(rejected_positive_output, rejected_positive_out)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
