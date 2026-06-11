#!/usr/bin/env python3
"""Offline frozen BUY quality candidate probe.

This report freezes one exploratory BUY-quality hypothesis:

    buyer_hhi <= 0.0431464614924829 AND buy_count >= 67

It evaluates the hypothesis on frozen selector training views only.  It does
not change runtime, Gatekeeper, execution, send path, configs, or thresholds.
R23/R24 are treated as discovery/confirmation evidence for a future fresh
validation candidate, not as permission to promote a runtime policy.
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "gatekeeper_buy_quality_candidate_v1"
MD_ARTIFACT = "GATEKEEPER_BUY_QUALITY_CANDIDATE.md"
MATRIX_CSV = "gatekeeper_buy_quality_candidate_matrix_v1.csv"
SELECTED_ROWS_CSV = "gatekeeper_buy_quality_candidate_selected_rows_v1.csv"
DEFAULT_TRAIN_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r23-score-tail-v1-r1-label-maturation-earlyhit-v1"
DEFAULT_VALIDATION_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r24-gk-edge-fresh-validation-check10-earlyhit-v1"

CANDIDATE_ID = "buyer_hhi_low_buy_count_high_v1"
BUYER_HHI_MAX = 0.0431464614924829
BUY_COUNT_MIN = 67


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", default=DEFAULT_TRAIN_SCOPE)
    parser.add_argument("--validation-scope", default=DEFAULT_VALIDATION_SCOPE)
    parser.add_argument("--min-lift-vs-base-pp", type=float, default=0.10)
    parser.add_argument("--min-train-resolved-rows", type=int, default=75)
    parser.add_argument("--min-validation-resolved-rows", type=int, default=50)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--matrix-output", default=None)
    parser.add_argument("--selected-rows-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def dataset_dir(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope


def report_dir(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope


def default_outputs(root: Path, validation_scope: str, args: argparse.Namespace) -> dict[str, Path]:
    out_dir = report_dir(root, validation_scope)
    return {
        "json": Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json",
        "md": Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT,
        "matrix_csv": Path(args.matrix_output) if args.matrix_output else out_dir / MATRIX_CSV,
        "selected_rows_csv": Path(args.selected_rows_output)
        if args.selected_rows_output
        else out_dir / SELECTED_ROWS_CSV,
    }


def float_value(row: dict[str, Any], *fields: str) -> float | None:
    for field in fields:
        value = common.float_or_none(row.get(field))
        if value is not None:
            return value
    return None


def int_value(row: dict[str, Any], *fields: str) -> int | None:
    for field in fields:
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def is_buy(row: dict[str, Any]) -> bool:
    verdict = str(row.get("gatekeeper_verdict") or row.get("gatekeeper_legacy_verdict_context") or "").upper()
    return row.get("decision_verdict_buy") is True or verdict == "BUY"


def is_resolved(row: dict[str, Any]) -> bool:
    return row.get("r2_label") in {"positive", "negative"}


def is_positive(row: dict[str, Any]) -> bool:
    return row.get("r2_label") == "positive"


def rate(num: int, den: int) -> float | None:
    return num / den if den else None


def load_training_view(root: Path, scope: str) -> list[dict[str, Any]]:
    path = dataset_dir(root, scope) / "selector_training_view_v1.jsonl"
    return list(common.iter_json_objects(path))


def candidate_selects(row: dict[str, Any]) -> bool:
    buyer_hhi = float_value(row, "buyer_hhi", "gk_hhi")
    buy_count = int_value(row, "buy_count", "evidence_buy_count", "gk_buy_count")
    return (
        is_buy(row)
        and buyer_hhi is not None
        and buy_count is not None
        and buyer_hhi <= BUYER_HHI_MAX
        and buy_count >= BUY_COUNT_MIN
    )


def summarize(run: str, scope: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    resolved = [row for row in rows if is_resolved(row)]
    positives = sum(1 for row in resolved if is_positive(row))
    buy_rows = [row for row in rows if is_buy(row)]
    buy_resolved = [row for row in buy_rows if is_resolved(row)]
    buy_positive = sum(1 for row in buy_resolved if is_positive(row))
    selected = [row for row in rows if candidate_selects(row)]
    selected_resolved = [row for row in selected if is_resolved(row)]
    selected_positive = sum(1 for row in selected_resolved if is_positive(row))
    selected_negative = len(selected_resolved) - selected_positive
    base_rate = rate(positives, len(resolved))
    buy_precision = rate(buy_positive, len(buy_resolved))
    selected_precision = rate(selected_positive, len(selected_resolved))
    return {
        "run": run,
        "scope": scope,
        "rows": len(rows),
        "resolved_rows": len(resolved),
        "base_positive_rows": positives,
        "base_negative_rows": len(resolved) - positives,
        "base_positive_rate": base_rate,
        "current_buy_rows": len(buy_rows),
        "current_buy_resolved_rows": len(buy_resolved),
        "current_buy_positive_rows": buy_positive,
        "current_buy_negative_rows": len(buy_resolved) - buy_positive,
        "current_buy_precision": buy_precision,
        "selected_rows": len(selected),
        "selected_resolved_rows": len(selected_resolved),
        "selected_positive_rows": selected_positive,
        "selected_negative_rows": selected_negative,
        "selected_precision": selected_precision,
        "selected_lift_vs_base_pp": None if selected_precision is None or base_rate is None else selected_precision - base_rate,
        "selected_lift_vs_current_buy_pp": None
        if selected_precision is None or buy_precision is None
        else selected_precision - buy_precision,
    }


def selected_row_examples(run: str, scope: str, rows: list[dict[str, Any]], limit: int = 200) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in [row for row in rows if candidate_selects(row)][:limit]:
        out.append(
            {
                "run": run,
                "scope": scope,
                "candidate_id": row.get("candidate_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "r2_label": row.get("r2_label"),
                "r2_status": row.get("r2_status"),
                "buyer_hhi": float_value(row, "buyer_hhi", "gk_hhi"),
                "buy_count": int_value(row, "buy_count", "evidence_buy_count", "gk_buy_count"),
                "unique_buyers": int_value(row, "unique_buyers", "evidence_unique_buyers"),
                "evidence_total_volume_sol": float_value(row, "evidence_total_volume_sol", "total_volume_sol"),
                "sell_share": float_value(row, "sell_share", "evidence_sell_share"),
                "decision_reason": row.get("decision_reason"),
            }
        )
    return out


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = sorted({key for row in rows for key in row})
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def verdict_for(train: dict[str, Any], validation: dict[str, Any], args: argparse.Namespace) -> tuple[str, str, list[str]]:
    fail_reasons: list[str] = []
    train_lift = train.get("selected_lift_vs_base_pp")
    validation_lift = validation.get("selected_lift_vs_base_pp")
    if train["selected_resolved_rows"] < int(args.min_train_resolved_rows):
        fail_reasons.append("train_selected_resolved_below_min")
    if validation["selected_resolved_rows"] < int(args.min_validation_resolved_rows):
        fail_reasons.append("validation_selected_resolved_below_min")
    if train_lift is None or train_lift < float(args.min_lift_vs_base_pp):
        fail_reasons.append("train_lift_below_min")
    if validation_lift is None or validation_lift < float(args.min_lift_vs_base_pp):
        fail_reasons.append("validation_lift_below_min")
    if not fail_reasons:
        return (
            "BUY_QUALITY_CANDIDATE_READY_FOR_FRESH_VALIDATION",
            "FREEZE_CANDIDATE_FOR_FRESH_VALIDATION_DO_NOT_CHANGE_RUNTIME",
            fail_reasons,
        )
    if "validation_selected_resolved_below_min" in fail_reasons and all(
        reason not in fail_reasons for reason in ("train_lift_below_min", "validation_lift_below_min")
    ):
        return (
            "BUY_QUALITY_CANDIDATE_PROMISING_LOW_VALIDATION_COVERAGE",
            "FREEZE_CANDIDATE_FOR_FRESH_VALIDATION_DO_NOT_CHANGE_RUNTIME",
            fail_reasons,
        )
    return (
        "BUY_QUALITY_CANDIDATE_NOT_CONFIRMED",
        "DO_NOT_CHANGE_RUNTIME_BUY_EDGE_NOT_CONFIRMED",
        fail_reasons,
    )


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    def pct(value: Any) -> str:
        if value is None:
            return "n/a"
        return f"{float(value) * 100:.2f}%"

    train = report["matrix"][0]
    validation = report["matrix"][1]
    lines = [
        "# Gatekeeper BUY Quality Candidate",
        "",
        f"Status: `{report['status']}`",
        f"Business decision: `{report['business_decision']}`",
        f"Candidate: `{report['candidate']['candidate_id']}`",
        "",
        "## Frozen Contract",
        "",
        f"- buyer_hhi <= `{report['candidate']['buyer_hhi_max']}`",
        f"- buy_count >= `{report['candidate']['buy_count_min']}`",
        "- input population: current Gatekeeper BUY rows only",
        "",
        "## Results",
        "",
        (
            "- train: "
            f"selected={train['selected_positive_rows']}/{train['selected_resolved_rows']} "
            f"precision={pct(train['selected_precision'])}, "
            f"base={pct(train['base_positive_rate'])}, "
            f"lift={pct(train['selected_lift_vs_base_pp'])}"
        ),
        (
            "- validation: "
            f"selected={validation['selected_positive_rows']}/{validation['selected_resolved_rows']} "
            f"precision={pct(validation['selected_precision'])}, "
            f"base={pct(validation['base_positive_rate'])}, "
            f"lift={pct(validation['selected_lift_vs_base_pp'])}"
        ),
        "",
        "## Claim Boundaries",
        "",
    ]
    for key, value in report["claim_boundaries"].items():
        lines.append(f"- {key}: `{value}`")
    lines.extend(["", "Fail reasons:", ""])
    for reason in report["fail_reasons"]:
        lines.append(f"- `{reason}`")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    outputs = default_outputs(root, args.validation_scope, args)
    train_rows = load_training_view(root, args.train_scope)
    validation_rows = load_training_view(root, args.validation_scope)
    matrix = [
        summarize("train", args.train_scope, train_rows),
        summarize("validation", args.validation_scope, validation_rows),
    ]
    status, business_decision, fail_reasons = verdict_for(matrix[0], matrix[1], args)
    selected_rows = selected_row_examples("train", args.train_scope, train_rows) + selected_row_examples(
        "validation",
        args.validation_scope,
        validation_rows,
    )
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": business_decision,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "candidate": {
            "candidate_id": CANDIDATE_ID,
            "candidate_status": "frozen_exploratory_hypothesis",
            "buyer_hhi_max": BUYER_HHI_MAX,
            "buy_count_min": BUY_COUNT_MIN,
            "input_population": "current_gatekeeper_buy_rows",
            "decision_time_safe": True,
            "runtime_feasible": True,
            "threshold_origin": "post_r23_r24_exploratory_screen",
        },
        "acceptance": {
            "min_lift_vs_base_pp": args.min_lift_vs_base_pp,
            "min_train_resolved_rows": args.min_train_resolved_rows,
            "min_validation_resolved_rows": args.min_validation_resolved_rows,
        },
        "fail_reasons": fail_reasons,
        "matrix": matrix,
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "changes_runtime": False,
            "changes_gatekeeper": False,
            "changes_execution": False,
            "changes_send_path": False,
            "thresholds_tuned": False,
            "production_promotion_allowed": False,
            "r23_r24_are_final_holdout": False,
            "requires_fresh_validation": True,
        },
        "outputs": {key: str(value) for key, value in outputs.items()},
    }
    common.write_json(outputs["json"], report)
    write_csv(outputs["matrix_csv"], matrix)
    write_csv(outputs["selected_rows_csv"], selected_rows)
    write_markdown(outputs["md"], report)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
