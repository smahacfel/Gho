#!/usr/bin/env python3
"""Offline Gatekeeper BUY false-positive separation audit.

This audit evaluates a fixed, small set of semantic BUY veto probes on frozen
selector training views. It does not change runtime, Gatekeeper, execution,
send path, configs, or thresholds. A passing probe is only an offline diagnostic
candidate and still requires a separate policy design and fresh validation step.
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
from typing import Any, Callable

import analyze_gatekeeper_r2_policy_autopsy as autopsy
import selector_pipeline_common as common


ARTIFACT = "gatekeeper_buy_false_positive_separation_v1"
MD_ARTIFACT = "GATEKEEPER_BUY_FALSE_POSITIVE_SEPARATION.md"
VETO_MATRIX_CSV = "gatekeeper_buy_veto_probe_matrix_v1.csv"
FALSE_BUY_EXAMPLES_CSV = "gatekeeper_buy_false_positive_examples_v1.csv"
DEFAULT_TRAIN_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r23-score-tail-v1-r1-label-maturation-earlyhit-v1"
DEFAULT_VALIDATION_SCOPE = "selector-phase1-pumpfun-sol-v1-20260611-r24-gk-edge-fresh-validation-check10-earlyhit-v1"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", default=DEFAULT_TRAIN_SCOPE)
    parser.add_argument("--validation-scope", default=DEFAULT_VALIDATION_SCOPE)
    parser.add_argument("--min-lift-vs-base-pp", type=float, default=0.05)
    parser.add_argument("--min-kept-resolved-rows", type=int, default=50)
    parser.add_argument("--max-positive-loss-rate", type=float, default=0.35)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--veto-matrix-output", default=None)
    parser.add_argument("--false-buy-examples-output", default=None)
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
        "veto_matrix_csv": Path(args.veto_matrix_output) if args.veto_matrix_output else out_dir / VETO_MATRIX_CSV,
        "false_buy_examples_csv": Path(args.false_buy_examples_output)
        if args.false_buy_examples_output
        else out_dir / FALSE_BUY_EXAMPLES_CSV,
    }


def training_view_rows(root: Path, scope: str) -> list[dict[str, Any]]:
    args = autopsy.build_parser().parse_args(
        [
            "--root",
            str(root),
            "--selector-scope",
            scope,
            "--runtime-scope",
            "unused_for_training_view",
            "--input-source",
            "training_view",
        ]
    )
    rows, _manifest = autopsy.training_view_rows(args)
    return rows


def is_buy(row: dict[str, Any]) -> bool:
    return row.get("decision_bucket") == "BUY"


def resolved(row: dict[str, Any]) -> bool:
    return autopsy.is_resolved(row)


def positive(row: dict[str, Any]) -> bool:
    return autopsy.is_positive(row)


def negative(row: dict[str, Any]) -> bool:
    return autopsy.is_negative(row)


def rate(num: int, den: int) -> float | None:
    return num / den if den else None


def metric(row: dict[str, Any], *fields: str) -> float | None:
    return autopsy.decision_metric(row, *fields)


def has_reason(row: dict[str, Any], reason: str) -> bool:
    return reason in autopsy.toxicity_reasons(row)


def veto_dev_has_sold(row: dict[str, Any]) -> bool:
    return has_reason(row, "dev_has_sold")


def veto_high_sell_share(row: dict[str, Any]) -> bool:
    return has_reason(row, "high_sell_share")


def veto_high_sell_buy_ratio(row: dict[str, Any]) -> bool:
    return has_reason(row, "high_sell_buy_ratio")


def veto_high_flip_ratio(row: dict[str, Any]) -> bool:
    return has_reason(row, "high_flip_ratio_10s")


def veto_high_flipper_presence(row: dict[str, Any]) -> bool:
    return has_reason(row, "high_flipper_presence")


def veto_high_dev_volume(row: dict[str, Any]) -> bool:
    return has_reason(row, "high_dev_volume_ratio")


def veto_obvious_sell_pressure(row: dict[str, Any]) -> bool:
    return veto_high_sell_share(row) or veto_high_sell_buy_ratio(row)


def veto_dev_or_sell_pressure(row: dict[str, Any]) -> bool:
    return veto_dev_has_sold(row) or veto_obvious_sell_pressure(row)


def veto_any_current_toxicity(row: dict[str, Any]) -> bool:
    return bool(autopsy.toxicity_reasons(row))


VETO_PROBES: tuple[dict[str, Any], ...] = (
    {
        "veto_id": "veto_dev_has_sold",
        "description": "Reject current BUY when dev_has_sold is true.",
        "fn": veto_dev_has_sold,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_high_sell_share",
        "description": "Reject current BUY with sell_share >= 0.50.",
        "fn": veto_high_sell_share,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_high_sell_buy_ratio",
        "description": "Reject current BUY with sell_buy_ratio >= 1.20.",
        "fn": veto_high_sell_buy_ratio,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_high_flip_ratio_10s",
        "description": "Reject current BUY with flip_ratio_10s >= 0.40.",
        "fn": veto_high_flip_ratio,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_high_flipper_presence",
        "description": "Reject current BUY with flipper_presence_ratio >= 0.50.",
        "fn": veto_high_flipper_presence,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_high_dev_volume_ratio",
        "description": "Reject current BUY with dev_volume_ratio >= 0.50.",
        "fn": veto_high_dev_volume,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_obvious_sell_pressure",
        "description": "Reject current BUY with high sell_share or high sell_buy_ratio.",
        "fn": veto_obvious_sell_pressure,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_dev_or_sell_pressure",
        "description": "Reject current BUY with dev_has_sold or obvious sell pressure.",
        "fn": veto_dev_or_sell_pressure,
        "runtime_change_allowed": False,
    },
    {
        "veto_id": "veto_any_current_toxicity",
        "description": "Reject current BUY with any current autopsy toxicity reason.",
        "fn": veto_any_current_toxicity,
        "runtime_change_allowed": False,
    },
)


def count_outcomes(rows: list[dict[str, Any]]) -> dict[str, Any]:
    res = [row for row in rows if resolved(row)]
    positives = sum(1 for row in res if positive(row))
    negatives = sum(1 for row in res if negative(row))
    return {
        "rows": len(rows),
        "resolved_rows": len(res),
        "positive_rows": positives,
        "negative_rows": negatives,
        "precision": rate(positives, len(res)),
        "negative_rate": rate(negatives, len(res)),
    }


def evaluate_probe(
    scope: str,
    run_name: str,
    rows: list[dict[str, Any]],
    probe: dict[str, Any],
    min_lift_vs_base_pp: float,
    min_kept_resolved_rows: int,
    max_positive_loss_rate: float,
) -> dict[str, Any]:
    all_resolved = [row for row in rows if resolved(row)]
    base_positive = sum(1 for row in all_resolved if positive(row))
    buy_rows = [row for row in rows if is_buy(row)]
    buy_resolved = [row for row in buy_rows if resolved(row)]
    buy_positive = sum(1 for row in buy_resolved if positive(row))
    buy_negative = sum(1 for row in buy_resolved if negative(row))
    fn: Callable[[dict[str, Any]], bool] = probe["fn"]
    flagged = [row for row in buy_rows if fn(row)]
    kept = [row for row in buy_rows if not fn(row)]
    flagged_counts = count_outcomes(flagged)
    kept_counts = count_outcomes(kept)
    current_buy_precision = rate(buy_positive, len(buy_resolved))
    base_rate = rate(base_positive, len(all_resolved))
    kept_precision = kept_counts["precision"]
    flagged_positive = int(flagged_counts["positive_rows"])
    flagged_negative = int(flagged_counts["negative_rows"])
    removed_false_positive_rate = rate(flagged_negative, buy_negative)
    positive_loss_rate = rate(flagged_positive, buy_positive)
    lift_vs_current = None if kept_precision is None or current_buy_precision is None else kept_precision - current_buy_precision
    lift_vs_base = None if kept_precision is None or base_rate is None else kept_precision - base_rate
    pass_base = (
        kept_precision is not None
        and base_rate is not None
        and kept_counts["resolved_rows"] >= min_kept_resolved_rows
        and kept_precision >= base_rate + min_lift_vs_base_pp
        and (positive_loss_rate is None or positive_loss_rate <= max_positive_loss_rate)
    )
    improves_buy = kept_precision is not None and current_buy_precision is not None and kept_precision > current_buy_precision
    if pass_base:
        status = "VETO_BEATS_BASE_OFFLINE"
    elif improves_buy:
        status = "VETO_IMPROVES_CURRENT_BUY_ONLY"
    else:
        status = "VETO_NO_HELP"
    return {
        "scope": scope,
        "run": run_name,
        "veto_id": probe["veto_id"],
        "description": probe["description"],
        "status": status,
        "base_positive_rate": base_rate,
        "current_buy_precision": current_buy_precision,
        "current_buy_rows": len(buy_rows),
        "current_buy_resolved_rows": len(buy_resolved),
        "current_buy_positive_rows": buy_positive,
        "current_buy_negative_rows": buy_negative,
        "flagged_buy_rows": flagged_counts["rows"],
        "flagged_resolved_rows": flagged_counts["resolved_rows"],
        "flagged_positive_rows": flagged_counts["positive_rows"],
        "flagged_negative_rows": flagged_counts["negative_rows"],
        "flagged_negative_rate": flagged_counts["negative_rate"],
        "kept_buy_rows": kept_counts["rows"],
        "kept_resolved_rows": kept_counts["resolved_rows"],
        "kept_positive_rows": kept_counts["positive_rows"],
        "kept_negative_rows": kept_counts["negative_rows"],
        "kept_precision": kept_precision,
        "lift_vs_current_buy_precision_pp": lift_vs_current,
        "lift_vs_base_rate_pp": lift_vs_base,
        "removed_false_positive_rate": removed_false_positive_rate,
        "positive_loss_rate": positive_loss_rate,
        "runtime_change_allowed": False,
        "requires_policy_design": True,
        "requires_fresh_validation": True,
    }


def false_buy_examples(scope: str, run_name: str, rows: list[dict[str, Any]], limit: int = 100) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    false_buys = [row for row in rows if is_buy(row) and negative(row)]
    for row in false_buys[:limit]:
        out.append(
            {
                "scope": scope,
                "run": run_name,
                "candidate_id": row.get("candidate_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "toxicity_reasons": ";".join(autopsy.toxicity_reasons(row)),
                "sell_share": metric(row, "sell_share"),
                "sell_buy_ratio": metric(row, "sell_buy_ratio"),
                "dev_has_sold": autopsy.bool_value(row.get("decision"), "dev_has_sold"),
                "dev_volume_ratio": metric(row, "dev_volume_ratio"),
                "flip_ratio_10s": metric(row, "flip_ratio_10s"),
                "flipper_presence_ratio": metric(row, "flipper_presence_ratio"),
                "buy_ratio": metric(row, "buy_ratio", "fixed_size_buy_ratio"),
                "total_volume_sol": metric(row, "total_volume_sol"),
                "unique_signers": metric(row, "ab_unique_signers_window"),
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


def summarize_scope(rows: list[dict[str, Any]]) -> dict[str, Any]:
    all_resolved = [row for row in rows if resolved(row)]
    buy_rows = [row for row in rows if is_buy(row)]
    buy_resolved = [row for row in buy_rows if resolved(row)]
    return {
        "rows": len(rows),
        "resolved_rows": len(all_resolved),
        "base_positive_rows": sum(1 for row in all_resolved if positive(row)),
        "base_negative_rows": sum(1 for row in all_resolved if negative(row)),
        "base_positive_rate": rate(sum(1 for row in all_resolved if positive(row)), len(all_resolved)),
        "current_buy_rows": len(buy_rows),
        "current_buy_resolved_rows": len(buy_resolved),
        "current_buy_positive_rows": sum(1 for row in buy_resolved if positive(row)),
        "current_buy_negative_rows": sum(1 for row in buy_resolved if negative(row)),
        "current_buy_precision": rate(sum(1 for row in buy_resolved if positive(row)), len(buy_resolved)),
    }


def choose_best_probe(matrix: list[dict[str, Any]]) -> dict[str, Any] | None:
    validation_rows = [row for row in matrix if row["run"] == "validation"]
    if not validation_rows:
        return None
    return max(
        validation_rows,
        key=lambda row: (
            -1.0 if row["kept_precision"] is None else float(row["kept_precision"]),
            -1.0 if row["lift_vs_base_rate_pp"] is None else float(row["lift_vs_base_rate_pp"]),
            int(row["kept_resolved_rows"]),
        ),
    )


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    outputs = default_outputs(root, args.validation_scope, args)
    train_rows = training_view_rows(root, args.train_scope)
    validation_rows = training_view_rows(root, args.validation_scope)
    matrix: list[dict[str, Any]] = []
    for run_name, scope, rows in (
        ("train", args.train_scope, train_rows),
        ("validation", args.validation_scope, validation_rows),
    ):
        for probe in VETO_PROBES:
            matrix.append(
                evaluate_probe(
                    scope,
                    run_name,
                    rows,
                    probe,
                    float(args.min_lift_vs_base_pp),
                    int(args.min_kept_resolved_rows),
                    float(args.max_positive_loss_rate),
                )
            )
    statuses_by_probe: dict[str, dict[str, str]] = {}
    for row in matrix:
        statuses_by_probe.setdefault(str(row["veto_id"]), {})[str(row["run"])] = str(row["status"])
    stable_pass = [
        veto_id
        for veto_id, statuses in statuses_by_probe.items()
        if statuses.get("train") == "VETO_BEATS_BASE_OFFLINE"
        and statuses.get("validation") == "VETO_BEATS_BASE_OFFLINE"
    ]
    stable_improve = [
        veto_id
        for veto_id, statuses in statuses_by_probe.items()
        if statuses.get("train") in {"VETO_BEATS_BASE_OFFLINE", "VETO_IMPROVES_CURRENT_BUY_ONLY"}
        and statuses.get("validation") in {"VETO_BEATS_BASE_OFFLINE", "VETO_IMPROVES_CURRENT_BUY_ONLY"}
    ]
    if stable_pass:
        status = "BUY_FP_STABLE_VETO_FOUND_OFFLINE"
        business_decision = "DO_NOT_CHANGE_RUNTIME_REQUIRES_POLICY_DESIGN_AND_FRESH_VALIDATION"
    elif stable_improve:
        status = "BUY_FP_VETO_IMPROVES_BUT_NO_EDGE"
        business_decision = "DO_NOT_CHANGE_RUNTIME_BUY_EDGE_NOT_CONFIRMED"
    else:
        status = "BUY_FP_NO_STABLE_SEPARATOR_FOUND"
        business_decision = "DO_NOT_CHANGE_RUNTIME_BUY_EDGE_NOT_CONFIRMED"
    examples = false_buy_examples(args.train_scope, "train", train_rows) + false_buy_examples(
        args.validation_scope,
        "validation",
        validation_rows,
    )
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "business_decision": business_decision,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "train_summary": summarize_scope(train_rows),
        "validation_summary": summarize_scope(validation_rows),
        "stable_pass_veto_ids": stable_pass,
        "stable_improve_veto_ids": stable_improve,
        "best_validation_probe": choose_best_probe(matrix),
        "acceptance": {
            "min_lift_vs_base_pp": args.min_lift_vs_base_pp,
            "min_kept_resolved_rows": args.min_kept_resolved_rows,
            "max_positive_loss_rate": args.max_positive_loss_rate,
        },
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "changes_runtime": False,
            "changes_gatekeeper": False,
            "changes_execution": False,
            "changes_send_path": False,
            "thresholds_tuned": False,
            "production_promotion_allowed": False,
        },
        "outputs": {key: str(value) for key, value in outputs.items()},
    }
    common.write_json(outputs["json"], report)
    write_csv(outputs["veto_matrix_csv"], matrix)
    write_csv(outputs["false_buy_examples_csv"], examples)
    write_markdown(outputs["md"], report)
    return report


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    def pct(value: Any) -> str:
        if value is None:
            return "n/a"
        return f"{float(value) * 100:.2f}%"

    lines = [
        "# Gatekeeper BUY False Positive Separation",
        "",
        f"Status: `{report['status']}`",
        f"Business decision: `{report['business_decision']}`",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Scope Summaries",
        "",
        (
            "- train: "
            f"base={pct(report['train_summary']['base_positive_rate'])}, "
            f"buy_precision={pct(report['train_summary']['current_buy_precision'])}, "
            f"buy_resolved={report['train_summary']['current_buy_resolved_rows']}"
        ),
        (
            "- validation: "
            f"base={pct(report['validation_summary']['base_positive_rate'])}, "
            f"buy_precision={pct(report['validation_summary']['current_buy_precision'])}, "
            f"buy_resolved={report['validation_summary']['current_buy_resolved_rows']}"
        ),
        "",
        "## Stable Veto Results",
        "",
        f"- stable_pass_veto_ids: `{report['stable_pass_veto_ids']}`",
        f"- stable_improve_veto_ids: `{report['stable_improve_veto_ids']}`",
        "",
        "## Claim Boundaries",
        "",
    ]
    for key, value in report["claim_boundaries"].items():
        lines.append(f"- {key}: `{value}`")
    lines.extend(
        [
            "",
            "This report is offline-only and diagnostic-only. It does not change runtime, Gatekeeper, execution, send path, configs, or thresholds.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
