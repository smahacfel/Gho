#!/usr/bin/env python3
"""Build a Phase 3 R2-only baseline precision draft report without tuning."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import random
from collections import Counter
from pathlib import Path
from typing import Any, Callable

import build_selector_training_view as training
import selector_pipeline_common as common


FEATURE_COLUMNS = (
    "curve_progress_pct",
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
    "creator_sold_early_flag",
    "quote_mint_is_sol",
)
SCORE_FIELDS = (
    "gatekeeper_v25_score",
    "gatekeeper_v3_score",
    "v25_shadow_confidence",
    "v3_shadow_confidence",
)
EXCLUDED_EXECUTION_STATUSES = {
    "execution_not_realized",
    "not_dispatched",
    "not_executable_route",
}


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object in {path}")
    return payload


def file_provenance(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {"path": None, "exists": False}
    payload: dict[str, Any] = {"path": str(path), "exists": path.exists()}
    if not path.exists() or not path.is_file():
        return payload
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    payload.update({"size_bytes": path.stat().st_size, "sha256": digest.hexdigest()})
    return payload


def phase3_report_dir(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope


def phase3_dataset_dir(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope


def r2only_denominator(row: dict[str, Any]) -> bool:
    execution_status = common.str_or_none(row.get("execution_feasibility_status"))
    return bool(
        training.r2_training_denominator(row)
        and execution_status not in EXCLUDED_EXECUTION_STATUSES
    )


def selector_accept(row: dict[str, Any]) -> bool:
    context = row.get("selector_accept_context")
    if isinstance(context, dict):
        return context.get("decision_verdict_buy") is True
    return row.get("decision_verdict_buy") is True


def label_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def metric_block(
    rows: list[dict[str, Any]],
    select: Callable[[dict[str, Any]], bool],
) -> dict[str, Any]:
    selected = [row for row in rows if select(row)]
    tp = sum(1 for row in selected if label_positive(row))
    fp = sum(1 for row in selected if not label_positive(row))
    fn = sum(1 for row in rows if not select(row) and label_positive(row))
    tn = sum(1 for row in rows if not select(row) and not label_positive(row))
    positives = tp + fn
    negatives = fp + tn
    return {
        "rows": len(rows),
        "positive_rows": positives,
        "negative_rows": negatives,
        "selected_count": len(selected),
        "tp_r2": tp,
        "fp_r2": fp,
        "fn_r2": fn,
        "tn_r2": tn,
        "precision_r2": tp / (tp + fp) if (tp + fp) else None,
        "recall_r2": tp / positives if positives else None,
        "accept_rate": len(selected) / len(rows) if rows else None,
        "coverage": len(selected) / len(rows) if rows else None,
        "positive_rate": positives / len(rows) if rows else None,
        "confusion_matrix_r2": {
            "selected_positive_tp": tp,
            "selected_negative_fp": fp,
            "rejected_positive_fn": fn,
            "rejected_negative_tn": tn,
        },
    }


def bootstrap_precision_ci(
    rows: list[dict[str, Any]],
    *,
    select: Callable[[dict[str, Any]], bool],
    samples: int,
    seed: int,
) -> dict[str, Any]:
    selected = [row for row in rows if select(row)]
    if not selected:
        return {
            "samples": samples,
            "seed": seed,
            "selected_count": 0,
            "precision_mean": None,
            "precision_p025": None,
            "precision_p975": None,
        }
    rng = random.Random(seed)
    precisions: list[float] = []
    for _ in range(samples):
        draw = [selected[rng.randrange(len(selected))] for _ in range(len(selected))]
        tp = sum(1 for row in draw if label_positive(row))
        precisions.append(tp / len(draw))
    precisions.sort()
    p025_idx = int(0.025 * (len(precisions) - 1))
    p975_idx = int(0.975 * (len(precisions) - 1))
    return {
        "samples": samples,
        "seed": seed,
        "selected_count": len(selected),
        "precision_mean": sum(precisions) / len(precisions),
        "precision_p025": precisions[p025_idx],
        "precision_p975": precisions[p975_idx],
    }


def score_value(row: dict[str, Any]) -> tuple[float | None, str | None]:
    for field in SCORE_FIELDS:
        value = common.float_or_none(row.get(field))
        if value is not None:
            return value, field
    return None, None


def precision_at_top_k(rows: list[dict[str, Any]], top_k_values: list[int]) -> list[dict[str, Any]]:
    scored = []
    score_field_counts: Counter[str] = Counter()
    for row in rows:
        score, field = score_value(row)
        if score is None or field is None:
            continue
        score_field_counts[field] += 1
        scored.append((score, common.int_or_none(row.get("birth_ts_ms")) or 0, str(row.get("candidate_id")), row))
    scored.sort(key=lambda item: (-item[0], item[1], item[2]))
    reports = []
    for top_k in top_k_values:
        selected_rows = [item[3] for item in scored[: min(top_k, len(scored))]]
        metrics = metric_block(selected_rows, lambda _row: True) if selected_rows else metric_block([], lambda _row: True)
        reports.append(
            {
                "k": top_k,
                "available_scored_rows": len(scored),
                "score_field_counts": common.counter_dict(score_field_counts),
                "selected_count": metrics["selected_count"],
                "tp_r2": metrics["tp_r2"],
                "fp_r2": metrics["fp_r2"],
                "precision_r2": metrics["precision_r2"],
                "positive_rate": metrics["positive_rate"],
            }
        )
    return reports


def split_counts(rows: list[dict[str, Any]]) -> dict[str, dict[str, int]]:
    by_split: dict[str, Counter[str]] = {}
    for row in rows:
        split = str(row.get("split") or "unknown")
        label = str(row.get("r2_label") or "unknown")
        by_split.setdefault(split, Counter())[label] += 1
    return {split: common.counter_dict(counter) for split, counter in sorted(by_split.items())}


def feature_availability(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    summary: dict[str, dict[str, Any]] = {}
    for feature in FEATURE_COLUMNS:
        present = 0
        numeric = 0
        for row in rows:
            value = row.get(feature)
            if value is None:
                continue
            present += 1
            if isinstance(value, bool) or common.float_or_none(value) is not None:
                numeric += 1
        summary[feature] = {
            "present_rows": present,
            "numeric_or_bool_rows": numeric,
            "present_rate": present / len(rows) if rows else None,
        }
    return summary


def write_bucket_csv(path: Path, report: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    split_report = report.get("selector_accept_context", {}).get("by_split", {})
    if isinstance(split_report, dict):
        for split, metrics in sorted(split_report.items()):
            if isinstance(metrics, dict):
                rows.append(
                    {
                        "bucket": split,
                        "rows": metrics.get("rows"),
                        "positive_rows": metrics.get("positive_rows"),
                        "negative_rows": metrics.get("negative_rows"),
                        "selected_count": metrics.get("selected_count"),
                        "tp_r2": metrics.get("tp_r2"),
                        "fp_r2": metrics.get("fp_r2"),
                        "precision_r2": metrics.get("precision_r2"),
                        "accept_rate": metrics.get("accept_rate"),
                    }
                )
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(
            fh,
            fieldnames=[
                "bucket",
                "rows",
                "positive_rows",
                "negative_rows",
                "selected_count",
                "tp_r2",
                "fp_r2",
                "precision_r2",
                "accept_rate",
            ],
        )
        writer.writeheader()
        writer.writerows(rows)


def require_phase3_manifest(manifest: dict[str, Any]) -> list[str]:
    fail_reasons: list[str] = []
    if manifest.get("status") != "PASS_R2_ONLY_DRAFT":
        fail_reasons.append("phase3_r2only_manifest_not_pass")
    if manifest.get("dataset_kind") != "r2_only":
        fail_reasons.append("dataset_kind_not_r2_only")
    if manifest.get("market_recall_claim_allowed") is not False:
        fail_reasons.append("market_recall_claim_not_disabled")
    if manifest.get("production_promotion_allowed") is not False:
        fail_reasons.append("production_promotion_not_disabled")
    if manifest.get("leakage_audit_status") != "PASS" and manifest.get("leakage_audit", {}).get("status") != "PASS":
        fail_reasons.append("leakage_audit_not_pass")
    return fail_reasons


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    report_dir = phase3_report_dir(args.root, args.scope)
    dataset_dir = phase3_dataset_dir(args.root, args.scope)
    training_view = args.training_view or dataset_dir / "selector_training_view_v1.jsonl"
    phase3_manifest_path = args.phase3_manifest or report_dir / "phase3_r2only_manifest_v1.json"
    output = args.output or report_dir / "selector_r2only_baseline_report_v1.json"
    bucket_output = args.by_bucket_output or report_dir / "selector_r2only_baseline_by_bucket_v1.csv"
    phase3_manifest = read_json(phase3_manifest_path)
    rows = list(common.iter_json_objects(training_view))
    denominator_rows = [row for row in rows if r2only_denominator(row)]
    holdout_rows = [row for row in denominator_rows if row.get("split") == "holdout"]

    fail_reasons = require_phase3_manifest(phase3_manifest)
    if len(denominator_rows) < args.min_resolved_rows:
        fail_reasons.append("insufficient_r2_training_denominator_rows")
    if len(holdout_rows) < args.min_holdout_resolved_rows:
        fail_reasons.append("insufficient_holdout_resolved_rows")

    selector_by_split = {
        split: metric_block(
            [row for row in denominator_rows if row.get("split") == split],
            selector_accept,
        )
        for split in ("train", "validation", "holdout")
    }
    selector_accept_context = {
        "all": metric_block(denominator_rows, selector_accept),
        "by_split": selector_by_split,
        "precision_at_accept": metric_block(holdout_rows, selector_accept),
        "bootstrap_ci_holdout_precision_at_accept": bootstrap_precision_ci(
            holdout_rows,
            select=selector_accept,
            samples=args.bootstrap_samples,
            seed=args.bootstrap_seed,
        ),
    }
    status = "P3B_PASS_R2_ONLY_BASELINE_DRAFT" if not fail_reasons else "NO-GO"
    report = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_baseline_report_v1",
        "phase": "phase3",
        "dataset_kind": "r2_only",
        "status": status,
        "fail_reasons": fail_reasons,
        "scope": args.scope,
        "input_provenance": {
            "training_view": file_provenance(training_view),
            "phase3_r2only_manifest_v1": file_provenance(phase3_manifest_path),
        },
        "claim_boundaries": {
            "r2_only_baseline_draft": status == "P3B_PASS_R2_ONLY_BASELINE_DRAFT",
            "r1_lifecycle_claim": False,
            "realized_pnl_claim": False,
            "execution_success_claim": False,
            "market_recall_claim": False,
            "production_promotion_claim": False,
            "gatekeeper_tuning_started": False,
        },
        "universe_source_class": "ghost_observed_birth_universe",
        "universe_completeness_claim": "system_observed_not_archive_complete",
        "precision_claim_scope": "observed_birth_universe_only",
        "market_recall_claim_allowed": False,
        "production_promotion_allowed": False,
        "gatekeeper_tuning_started": False,
        "baseline_built": status == "P3B_PASS_R2_ONLY_BASELINE_DRAFT",
        "training_rows": len(rows),
        "resolved_denominator_count": len(denominator_rows),
        "r2_training_denominator_rows": len(denominator_rows),
        "positive_rows": sum(1 for row in denominator_rows if row.get("r2_label") == "positive"),
        "negative_rows": sum(1 for row in denominator_rows if row.get("r2_label") == "negative"),
        "split_counts": split_counts(denominator_rows),
        "selector_accept_context": selector_accept_context,
        "precision_at_top_k": precision_at_top_k(denominator_rows, args.top_k),
        "feature_availability_summary": feature_availability(denominator_rows),
        "exclusions": {
            "horizon_unmatured": sum(1 for row in rows if row.get("r2_status") == "horizon_unmatured"),
            "missing_path": sum(1 for row in rows if row.get("r2_status") == "missing_path"),
            "stream_incomplete": sum(1 for row in rows if row.get("r2_status") == "stream_incomplete"),
            "execution_status_excluded": common.counter_dict(
                Counter(
                    str(row.get("execution_feasibility_status"))
                    for row in rows
                    if common.str_or_none(row.get("execution_feasibility_status"))
                    in EXCLUDED_EXECUTION_STATUSES
                )
            ),
        },
        "gates": {
            "min_resolved_rows": args.min_resolved_rows,
            "min_holdout_resolved_rows": args.min_holdout_resolved_rows,
            "leakage_audit": "PASS",
            "production_promotion_allowed": False,
        },
        "caveats": [
            "R2-only baseline draft.",
            "No R1 lifecycle claim.",
            "No realized PnL claim.",
            "No execution success claim.",
            "No market recall claim.",
            "No production promotion.",
            "No Gatekeeper tuning.",
        ],
    }
    common.write_json(output, report)
    write_bucket_csv(bucket_output, report)
    report["outputs"] = {
        "selector_r2only_baseline_report_v1": file_provenance(output),
        "selector_r2only_baseline_by_bucket_v1": file_provenance(bucket_output),
    }
    common.write_json(output, report)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--training-view", type=Path)
    parser.add_argument("--phase3-manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--by-bucket-output", type=Path)
    parser.add_argument("--min-resolved-rows", type=int, default=500)
    parser.add_argument("--min-holdout-resolved-rows", type=int, default=50)
    parser.add_argument("--top-k", type=int, nargs="+", default=[10, 25, 50, 100])
    parser.add_argument("--bootstrap-samples", type=int, default=1000)
    parser.add_argument("--bootstrap-seed", type=int, default=1337)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    return build_report(args)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = run(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == "P3B_PASS_R2_ONLY_BASELINE_DRAFT" else 2


if __name__ == "__main__":
    raise SystemExit(main())
