#!/usr/bin/env python3
"""Evaluate the R22 XGB-derived rule profile as an offline shadow policy.

The evaluator is diagnostic-only. It materializes counterfactual fields such as
would_reject_by_xgb_rule_profile and xgb_rule_reasons against frozen selector
artifacts. It does not change Gatekeeper, execution, send path, or runtime
thresholds.
"""

from __future__ import annotations

import argparse
import json
import math
import tomllib
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "xgb_rule_profile_shadow_eval_v1"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
DEFAULT_PROFILE = "configs/selector/xgb_rule_profile_r22_v1.toml"
RULE_TO_FEATURE = {
    "min_buy_ratio_min": "buy_ratio_min",
    "max_flipper_presence_ratio": "flipper_presence_ratio",
    "max_flip_ratio_10s": "flip_ratio_10s",
    "max_early_slot_volume_dominance_buy": "early_slot_volume_dominance_buy",
    "max_hhi_delta_t2_t0": "hhi_delta_t2_t0",
    "min_dev_paperhand_latency_ms": "dev_paperhand_latency_ms",
}
MIN_RULE_FEATURE_COVERAGE = 0.80


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", required=True, help="Frozen selector scope.")
    parser.add_argument("--runtime-scope", required=True)
    parser.add_argument("--rule-profile", default=DEFAULT_PROFILE)
    parser.add_argument("--training-view", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--markdown-output", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def selector_dataset_dir(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope


def selector_report_dir(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope


def default_output(root: Path, scope: str) -> Path:
    return selector_report_dir(root, scope) / f"{ARTIFACT}.json"


def default_markdown(root: Path, scope: str) -> Path:
    return selector_report_dir(root, scope) / "XGB_RULE_PROFILE_SHADOW_EVAL.md"


def training_view_path(root: Path, scope: str) -> Path:
    return selector_dataset_dir(root, scope) / "selector_training_view_v1.jsonl"


def decision_paths(root: Path, runtime_scope: str) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / runtime_scope / "decisions" / runtime_scope
    if not decisions_root.exists():
        return []
    return sorted(decisions_root.rglob(DECISION_FILE))


def load_profile(path: Path) -> dict[str, Any]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    profile = data.get("profile")
    rules = data.get("rules")
    if not isinstance(profile, dict) or not isinstance(rules, dict):
        raise ValueError(f"{path} must contain [profile] and [rules]")
    for key in RULE_TO_FEATURE:
        if key not in rules:
            raise ValueError(f"{path} missing rule {key}")
    return {"profile": profile, "rules": rules}


def runtime_feature_rows(root: Path, runtime_scope: str) -> dict[str, dict[str, Any]]:
    by_candidate: dict[str, dict[str, Any]] = {}
    for path in decision_paths(root, runtime_scope):
        for row in common.iter_json_objects(path):
            candidate_id = common.str_or_none(row.get("candidate_id"))
            if not candidate_id:
                continue
            current = by_candidate.setdefault(candidate_id, {})
            for feature in RULE_TO_FEATURE.values():
                value = common.float_or_none(row.get(feature))
                if value is not None:
                    current[feature] = value
    return by_candidate


def row_label(row: dict[str, Any]) -> str | None:
    label = common.str_or_none(row.get("r2_label"))
    if label in {"positive", "negative"}:
        return label
    status = common.str_or_none(row.get("r2_status"))
    if status in {"positive", "negative"}:
        return status
    if row.get("label_resolved") is True and isinstance(label, str):
        normalized = label.lower()
        if normalized in {"1", "true", "win"}:
            return "positive"
        if normalized in {"0", "false", "loss"}:
            return "negative"
    return None


def feature_value(row: dict[str, Any], runtime_features: dict[str, Any], feature: str) -> float | None:
    value = common.float_or_none(row.get(feature))
    if value is not None:
        return value
    return common.float_or_none(runtime_features.get(feature))


def apply_rules(
    row: dict[str, Any],
    runtime_features: dict[str, Any],
    rules: dict[str, Any],
) -> tuple[bool, list[str], dict[str, float | None]]:
    reasons: list[str] = []
    values: dict[str, float | None] = {}
    for rule, feature in RULE_TO_FEATURE.items():
        value = feature_value(row, runtime_features, feature)
        threshold = common.float_or_none(rules.get(rule))
        values[feature] = value
        if threshold is None:
            reasons.append(f"{rule}:threshold_missing")
            continue
        if value is None:
            reasons.append(f"{feature}:missing")
            continue
        if rule.startswith("min_") and value < threshold:
            reasons.append(f"{feature}:below_min:{value:.6g}<{threshold:.6g}")
        elif rule.startswith("max_") and value > threshold:
            reasons.append(f"{feature}:above_max:{value:.6g}>{threshold:.6g}")
    return (len(reasons) == 0, reasons, values)


def pct(numerator: int, denominator: int) -> float | None:
    if denominator <= 0:
        return None
    return numerator / denominator


def finite(value: float | None) -> bool:
    return isinstance(value, (int, float)) and math.isfinite(float(value))


def load_context_summary(root: Path, scope: str) -> dict[str, Any]:
    dataset_dir = selector_dataset_dir(root, scope)
    summary: dict[str, Any] = {}
    for artifact, status_field in (
        ("buyer_quality_context_v1.jsonl", "bq_context_status"),
        ("funding_graph_context_v1.jsonl", "fg_status"),
    ):
        path = dataset_dir / artifact
        rows = common.read_jsonl(path)
        counts = Counter(str(row.get(status_field) or "missing") for row in rows)
        summary[artifact] = {
            "path": str(path),
            "rows": len(rows),
            "status_counts": common.counter_dict(counts),
        }
    return summary


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    profile_path = Path(args.rule_profile)
    if not profile_path.is_absolute():
        profile_path = root / profile_path
    profile = load_profile(profile_path)
    rules = profile["rules"]
    training_path = args.training_view or training_view_path(root, args.scope)
    rows = common.read_jsonl(training_path)
    runtime_by_candidate = runtime_feature_rows(root, args.runtime_scope)

    resolved = 0
    positives = 0
    negatives = 0
    would_pass = 0
    would_reject = 0
    pass_positive = 0
    pass_negative = 0
    reject_positive = 0
    reject_negative = 0
    unresolved_rows = 0
    missing_feature_counts: Counter[str] = Counter()
    rule_reject_counts: Counter[str] = Counter()
    feature_present_counts: Counter[str] = Counter()
    evaluated_rows: list[dict[str, Any]] = []

    for row in rows:
        label = row_label(row)
        if label is None:
            unresolved_rows += 1
            continue
        resolved += 1
        positives += int(label == "positive")
        negatives += int(label == "negative")
        candidate_id = common.str_or_none(row.get("candidate_id")) or ""
        runtime_features = runtime_by_candidate.get(candidate_id, {})
        passes, reasons, values = apply_rules(row, runtime_features, rules)
        for feature, value in values.items():
            if value is None:
                missing_feature_counts[feature] += 1
            else:
                feature_present_counts[feature] += 1
        for reason in reasons:
            rule_reject_counts[reason.split(":", 1)[0]] += 1
        would_pass += int(passes)
        would_reject += int(not passes)
        if passes and label == "positive":
            pass_positive += 1
        elif passes and label == "negative":
            pass_negative += 1
        elif not passes and label == "positive":
            reject_positive += 1
        elif not passes and label == "negative":
            reject_negative += 1
        evaluated_rows.append(
            {
                "candidate_id": candidate_id,
                "r2_label": label,
                "would_pass_rules": passes,
                "would_reject_by_xgb_rule_profile": not passes,
                "xgb_rule_reasons": reasons,
                "xgb_rule_values": values,
            }
        )

    base_rate = pct(positives, resolved)
    pass_precision = pct(pass_positive, would_pass)
    pass_recall = pct(pass_positive, positives)
    pass_accept_rate = pct(would_pass, resolved)
    false_accepted_negatives = pass_negative
    false_rejected_positives = reject_positive
    feature_coverage = {
        feature: pct(feature_present_counts[feature], resolved) if resolved else 0.0
        for feature in RULE_TO_FEATURE.values()
    }
    low_feature_coverage = [
        feature for feature, rate in feature_coverage.items() if (rate or 0.0) < MIN_RULE_FEATURE_COVERAGE
    ]

    fail_reasons: list[str] = []
    if resolved < 1000:
        fail_reasons.append(f"r2_resolved_rows_below_1000:{resolved}")
    if would_pass < 50:
        fail_reasons.append(f"would_pass_count_below_50:{would_pass}")
    if low_feature_coverage:
        fail_reasons.append("rule_feature_coverage_below_80pct:" + ",".join(low_feature_coverage))
    if not finite(base_rate) or not finite(pass_precision):
        fail_reasons.append("precision_or_base_rate_unavailable")
    elif float(pass_precision) < float(base_rate) + 0.10:
        fail_reasons.append(
            f"would_pass_precision_lift_below_10pp:{float(pass_precision) - float(base_rate):.6f}"
        )

    if resolved < 1000 or low_feature_coverage:
        verdict = "R22_CAPTURE_INSUFFICIENT_FIX_DATA_LANE_ONLY"
    elif not finite(base_rate) or not finite(pass_precision):
        verdict = "R22_LABEL_DEFINITION_REVIEW_REQUIRED"
    elif not fail_reasons:
        verdict = "R22_XGB_RULE_PROFILE_CONFIRMED_BUILD_OFFLINE_CANDIDATE"
    else:
        verdict = "R22_XGB_RULE_PROFILE_NOT_CONFIRMED_STOP_SELECTOR_PATH"

    report = {
        "schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "scope": args.scope,
        "runtime_scope": args.runtime_scope,
        "rule_profile": str(profile_path),
        "profile": profile["profile"],
        "rules": rules,
        "training_view": str(training_path),
        "r2_resolved_rows": resolved,
        "r2_positive_rows": positives,
        "r2_negative_rows": negatives,
        "r2_unresolved_rows_skipped": unresolved_rows,
        "base_positive_rate": base_rate,
        "would_pass_count": would_pass,
        "would_reject_count": would_reject,
        "would_pass_precision": pass_precision,
        "would_pass_recall": pass_recall,
        "would_pass_accept_rate": pass_accept_rate,
        "false_accepted_negatives": false_accepted_negatives,
        "false_rejected_positives": false_rejected_positives,
        "per_rule_reject_contribution": common.counter_dict(rule_reject_counts),
        "rule_feature_coverage": feature_coverage,
        "missing_feature_counts": common.counter_dict(missing_feature_counts),
        "buyer_funding_coverage_summary": load_context_summary(root, args.scope),
        "evaluated_row_samples": evaluated_rows[:100],
        "verdict": verdict,
        "fail_reasons": fail_reasons,
        "non_claims": {
            "changes_gatekeeper_decision": False,
            "changes_execution": False,
            "production_promotion_allowed": False,
            "runtime_sidecar_required": False,
        },
    }
    return report


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    lines = [
        "# XGB Rule Profile Shadow Eval",
        "",
        f"Verdict: `{report['verdict']}`",
        "",
        "## Counts",
        "",
        f"- r2_resolved_rows: {report['r2_resolved_rows']}",
        f"- base_positive_rate: {report['base_positive_rate']}",
        f"- would_pass_count: {report['would_pass_count']}",
        f"- would_reject_count: {report['would_reject_count']}",
        f"- would_pass_precision: {report['would_pass_precision']}",
        f"- would_pass_recall: {report['would_pass_recall']}",
        f"- would_pass_accept_rate: {report['would_pass_accept_rate']}",
        f"- false_accepted_negatives: {report['false_accepted_negatives']}",
        f"- false_rejected_positives: {report['false_rejected_positives']}",
        "",
        "## Fail Reasons",
        "",
    ]
    if report["fail_reasons"]:
        lines.extend(f"- {reason}" for reason in report["fail_reasons"])
    else:
        lines.append("- none")
    lines.extend(["", "## Non-Claims", ""])
    for key, value in report["non_claims"].items():
        lines.append(f"- {key}: {str(value).lower()}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    root = Path(args.root)
    output = args.output or default_output(root, args.scope)
    markdown = args.markdown_output or default_markdown(root, args.scope)
    common.write_json(output, report)
    write_markdown(markdown, report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    else:
        print(f"{report['verdict']} {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
