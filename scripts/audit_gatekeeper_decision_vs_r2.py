#!/usr/bin/env python3
"""Audit Gatekeeper terminal decisions against counterfactual R2 outcomes.

This report is offline-only.  It evaluates the full decision population
BUY/REJECT/TIMEOUT against selector R2 market paths.  Shadow lifecycle may be
used only as optional BUY enrichment and never as the denominator.
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "gatekeeper_decision_vs_r2_audit_v1"
MD_ARTIFACT = "GATEKEEPER_DECISION_VS_R2_AUDIT.md"
CONFUSION_CSV = "gatekeeper_decision_vs_r2_confusion_matrix_v1.csv"
FALSE_REJECTS_JSONL = "gatekeeper_false_reject_examples_v1.jsonl"
FALSE_BUYS_JSONL = "gatekeeper_false_buy_examples_v1.jsonl"
TIMEOUT_LOSS_JSONL = "gatekeeper_timeout_opportunity_loss_examples_v1.jsonl"
REASON_RATES_CSV = "gatekeeper_decision_reason_r2_rates_v1.csv"
XGB_METRICS_CSV = "gatekeeper_xgb_metric_by_decision_outcome_v1.csv"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
TOP_EXAMPLE_LIMIT = 100
XGB_METRICS = (
    "buy_ratio_min",
    "buy_ratio_mean",
    "flipper_presence_ratio",
    "flip_ratio_10s",
    "early_slot_volume_dominance_buy",
    "hhi_delta_t2_t0",
    "burst_ratio",
    "dev_paperhand_latency_ms",
    "tx_count_growth_ratio",
    "tas_available",
    "v25_confidence_available",
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-scope", required=True)
    parser.add_argument("--selector-scope", required=True)
    parser.add_argument("--decision-plane", default="legacy_live", choices=("legacy_live", "v25_shadow", "both"))
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--nearest-tolerance-ms", type=int, default=120_000)
    parser.add_argument("--output", default=None)
    parser.add_argument("--md-output", default=None)
    parser.add_argument("--confusion-output", default=None)
    parser.add_argument("--false-reject-output", default=None)
    parser.add_argument("--false-buy-output", default=None)
    parser.add_argument("--timeout-loss-output", default=None)
    parser.add_argument("--reason-rates-output", default=None)
    parser.add_argument("--xgb-metrics-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def dataset_dir(root: Path, selector_scope: str) -> Path:
    return root / "datasets" / "selector" / selector_scope


def report_dir(root: Path, selector_scope: str) -> Path:
    return root / "reports" / "selector" / selector_scope


def default_outputs(root: Path, selector_scope: str, args: argparse.Namespace) -> dict[str, Path]:
    out_dir = report_dir(root, selector_scope)
    return {
        "json": Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json",
        "md": Path(args.md_output) if args.md_output else out_dir / MD_ARTIFACT,
        "confusion_csv": Path(args.confusion_output) if args.confusion_output else out_dir / CONFUSION_CSV,
        "false_rejects_jsonl": Path(args.false_reject_output) if args.false_reject_output else out_dir / FALSE_REJECTS_JSONL,
        "false_buys_jsonl": Path(args.false_buy_output) if args.false_buy_output else out_dir / FALSE_BUYS_JSONL,
        "timeout_loss_jsonl": Path(args.timeout_loss_output) if args.timeout_loss_output else out_dir / TIMEOUT_LOSS_JSONL,
        "reason_rates_csv": Path(args.reason_rates_output) if args.reason_rates_output else out_dir / REASON_RATES_CSV,
        "xgb_metrics_csv": Path(args.xgb_metrics_output) if args.xgb_metrics_output else out_dir / XGB_METRICS_CSV,
    }


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def decision_paths(root: Path, runtime_scope: str, decision_plane: str) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / runtime_scope / "decisions" / runtime_scope
    if not decisions_root.exists():
        return []
    paths = sorted(decisions_root.rglob(DECISION_FILE))
    if decision_plane != "both":
        paths = [path for path in paths if f"/{decision_plane}/" in path.as_posix()]
    else:
        paths = [
            path
            for path in paths
            if "/legacy_live/" in path.as_posix() or "/v25_shadow/" in path.as_posix()
        ]
    return paths


def str_key(row: dict[str, Any], *fields: str) -> str | None:
    for field in fields:
        value = common.str_or_none(row.get(field))
        if value:
            return value
    return None


def row_pool(row: dict[str, Any]) -> str | None:
    return str_key(row, "pool_id", "bonding_curve")


def row_mint(row: dict[str, Any]) -> str | None:
    return str_key(row, "base_mint", "mint", "mint_id")


def row_decision_ts(row: dict[str, Any]) -> int | None:
    for field in ("decision_ts_ms", "observation_end_ts_ms", "first_seen_ts_ms", "birth_ts_ms"):
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def pool_mint_key(row: dict[str, Any]) -> tuple[str, str] | None:
    pool = row_pool(row)
    mint = row_mint(row)
    if not pool or not mint:
        return None
    return pool, mint


def exact_join_key(row: dict[str, Any]) -> str | None:
    return str_key(row, "candidate_id", "join_key", "decision_context_join_key")


def decision_context_key(row: dict[str, Any]) -> str | None:
    pool = row_pool(row)
    mint = row_mint(row)
    if not pool or not mint:
        return None
    return f"mint_pool:{mint}:{pool}"


def load_candidates(path: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows = read_jsonl(path)
    by_candidate_id: dict[str, dict[str, Any]] = {}
    by_exact: dict[str, dict[str, Any]] = {}
    by_pool_mint: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id and candidate_id not in by_candidate_id:
            by_candidate_id[candidate_id] = row
            by_exact[candidate_id] = row
        for key in (row.get("join_key"), row.get("decision_context_join_key"), decision_context_key(row)):
            key = common.str_or_none(key)
            if key and key not in by_exact:
                by_exact[key] = row
        pm_key = pool_mint_key(row)
        if pm_key:
            by_pool_mint[pm_key].append(row)
    return rows, {
        "by_candidate_id": by_candidate_id,
        "by_exact": by_exact,
        "by_pool_mint": by_pool_mint,
    }


def load_index(path: Path) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    if not path.exists():
        return out
    for row in read_jsonl(path):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id and candidate_id not in out:
            out[candidate_id] = row
    return out


def choose_nearest_candidate(
    decision: dict[str, Any],
    candidates: list[dict[str, Any]],
    tolerance_ms: int,
) -> dict[str, Any] | None:
    if not candidates:
        return None
    if len(candidates) == 1:
        return candidates[0]
    decision_ts = row_decision_ts(decision)
    if decision_ts is None:
        return None
    scored: list[tuple[int, dict[str, Any]]] = []
    for candidate in candidates:
        candidate_ts = row_decision_ts(candidate)
        if candidate_ts is None:
            continue
        scored.append((abs(candidate_ts - decision_ts), candidate))
    if not scored:
        return None
    scored.sort(key=lambda item: (item[0], common.str_or_none(item[1].get("candidate_id")) or ""))
    if scored[0][0] <= tolerance_ms:
        return scored[0][1]
    return None


def join_candidate(
    decision: dict[str, Any],
    indexes: dict[str, Any],
    tolerance_ms: int,
) -> tuple[dict[str, Any] | None, str]:
    candidate_id = common.str_or_none(decision.get("candidate_id"))
    if candidate_id and candidate_id in indexes["by_candidate_id"]:
        return indexes["by_candidate_id"][candidate_id], "candidate_id"
    for key in (decision.get("join_key"), decision_context_key(decision)):
        key = common.str_or_none(key)
        if key and key in indexes["by_exact"]:
            return indexes["by_exact"][key], "join_key"
    pm_key = pool_mint_key(decision)
    if pm_key:
        matches = indexes["by_pool_mint"].get(pm_key, [])
        if len(matches) == 1:
            return matches[0], "pool_id_base_mint"
        nearest = choose_nearest_candidate(decision, matches, tolerance_ms)
        if nearest is not None:
            return nearest, "base_mint_nearest_decision_ts"
    return None, "unmatched"


def normalize_r2_class(row: dict[str, Any] | None) -> str:
    if row is None:
        return "missing_path"
    label = row.get("r2_label")
    if label in {"positive", "negative"}:
        return str(label)
    status = str(row.get("r2_status") or row.get("r2_excluded_reason") or "unresolved")
    if status in {"horizon_unmatured", "missing_path", "stream_incomplete", "candidate_missing_decision_ts_ms"}:
        return status
    return "unresolved"


def is_buy(decision: dict[str, Any]) -> bool:
    plane = str(decision.get("decision_plane") or "")
    if decision.get("decision_verdict_buy") is True:
        return True
    fields = ["verdict_type", "gatekeeper_verdict"]
    if plane != "v25_shadow":
        fields.append("legacy_live_verdict_type")
    for field in fields:
        value = common.str_or_none(decision.get(field))
        if value and value.upper() == "BUY":
            return True
    return False


def raw_verdict(decision: dict[str, Any]) -> str:
    plane = str(decision.get("decision_plane") or "")
    fields = ["verdict_type", "gatekeeper_verdict"]
    if plane != "v25_shadow":
        fields.append("legacy_live_verdict_type")
    for field in fields:
        value = common.str_or_none(decision.get(field))
        if value:
            return value
    if is_buy(decision):
        return "BUY"
    return "UNKNOWN"


def decision_bucket(decision: dict[str, Any]) -> str:
    if is_buy(decision):
        return "BUY"
    verdict = raw_verdict(decision).upper()
    reason_code = str(decision.get("reason_code") or decision.get("gatekeeper_first_kill_reason") or "").upper()
    reason = str(decision.get("decision_reason") or decision.get("hard_fail_reason") or decision.get("hard_reject_reason") or "").upper()
    combined = " ".join([verdict, reason_code, reason])
    if "TIMEOUT_PHASE1_NO_DATA" in combined:
        return "TIMEOUT_PHASE1_NO_DATA"
    if "TIMEOUT_PHASE1_INSUFFICIENT" in combined:
        return "TIMEOUT_PHASE1_INSUFFICIENT"
    if "TIMEOUT" in combined:
        return "TIMEOUT_OTHER"
    if "PDD" in combined:
        if "WHALE" in combined:
            return "REJECT_PDD_WHALE"
        if "FLASH" in combined:
            return "REJECT_PDD_FLASH_CRASH"
        if "ENTRY" in combined or "DRIFT" in combined:
            return "REJECT_PDD_ENTRY_DRIFT"
        if "RAMP" in combined:
            return "REJECT_PDD_RAMPING"
        return "REJECT_PDD_OTHER"
    if "IWIM_LOW_CONF" in combined or "LOW_CONF" in combined:
        return "REJECT_IWIM_LOW_CONF"
    if "IWIM_UNKNOWN" in combined or "UNKNOWN_STRICT" in combined:
        return "REJECT_IWIM_UNKNOWN_STRICT"
    if "CORE" in combined:
        return "REJECT_CORE_FAIL"
    if "HARD_FAIL" in combined or "HARD_FAIL" in verdict or verdict.startswith("HARD_FAIL_"):
        return "REJECT_HARD_FAIL"
    if "REJECT" in combined:
        return "REJECT_OTHER"
    return "UNKNOWN"


def interpretation(bucket: str, r2_class: str) -> str:
    if bucket == "BUY" and r2_class == "positive":
        return "accepted opportunity"
    if bucket == "BUY" and r2_class == "negative":
        return "false buy / bought junk"
    if bucket != "BUY" and r2_class == "positive":
        return "false reject / missed opportunity"
    if bucket != "BUY" and r2_class == "negative":
        return "correct no-entry"
    return "unresolved counterfactual outcome"


def numeric(row: dict[str, Any] | None, field: str) -> float | None:
    if row is None:
        return None
    value = row.get(field)
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    return common.float_or_none(value)


def metric_value(decision: dict[str, Any], training: dict[str, Any] | None, metric: str) -> float | None:
    value = numeric(decision, metric)
    if value is not None:
        return value
    aliases = {
        "buy_ratio_min": ("min_buy_ratio", "buy_ratio"),
        "buy_ratio_mean": ("buy_ratio",),
        "tx_count_growth_ratio": ("tx_count_growth_ratio",),
        "tas_available": ("tas_available",),
        "v25_confidence_available": ("v25_confidence_available", "v25_shadow_confidence"),
    }
    for alias in aliases.get(metric, ()):
        value = numeric(decision, alias)
        if value is not None:
            return value
    value = numeric(training, metric)
    if value is not None:
        return value
    for alias in aliases.get(metric, ()):
        value = numeric(training, alias)
        if value is not None:
            return value
    return None


def outcome_class(bucket: str, r2_class: str) -> str | None:
    if r2_class not in {"positive", "negative"}:
        return None
    if bucket == "BUY":
        prefix = "BUY"
    elif bucket.startswith("TIMEOUT"):
        prefix = "TIMEOUT"
    else:
        prefix = "REJECT"
    return f"{prefix}_{r2_class}"


def concise_example(row: dict[str, Any]) -> dict[str, Any]:
    decision = row["decision"]
    r2 = row.get("r2") or {}
    lifecycle = row.get("lifecycle") or {}
    training = row.get("training") or {}
    out = {
        "candidate_id": row.get("candidate_id"),
        "pool_id": row.get("pool_id"),
        "base_mint": row.get("base_mint"),
        "decision_plane": row.get("decision_plane"),
        "decision_bucket": row.get("decision_bucket"),
        "verdict_type": row.get("verdict_type"),
        "decision_reason": decision.get("decision_reason"),
        "reason_code": decision.get("reason_code") or decision.get("gatekeeper_first_kill_reason"),
        "r2_label": row.get("r2_class"),
        "target_hit_time_ms": r2.get("target_hit_ts_ms"),
        "max_runup_pct": r2.get("max_favorable_pnl_pct"),
        "drawdown_before_target": r2.get("max_adverse_pnl_pct"),
        "final_pnl_pct": lifecycle.get("final_pnl_pct"),
        "close_reason": lifecycle.get("close_reason"),
        "truth_status": lifecycle.get("truth_status"),
        "simulation_outcome": lifecycle.get("execution_outcome") or decision.get("shadow_execution_outcome"),
    }
    for metric in XGB_METRICS:
        out[metric] = metric_value(decision, training, metric)
    return out


def rate(numerator: int, denominator: int) -> float | None:
    return numerator / denominator if denominator else None


def count_fields(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for row in rows:
        counts[normalize_r2_class(row)] += 1
    return dict(counts)


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    metrics = report["global_metrics"]
    lines = [
        "# Gatekeeper Decision vs R2 Audit",
        "",
        f"Status: `{report['status']}`",
        f"Runtime scope: `{report['runtime_scope']}`",
        f"Selector scope: `{report['selector_scope']}`",
        f"Decision plane: `{report['decision_plane']}`",
        "",
        "## Core Counts",
        "",
        f"- candidate_universe_rows: {report['join_manifest']['candidate_universe_rows']}",
        f"- decision_rows_loaded: {report['join_manifest']['decision_rows_loaded']}",
        f"- decision_rows_joined_to_candidate: {report['join_manifest']['decision_rows_joined_to_candidate']}",
        f"- decision_rows_joined_to_r2: {report['join_manifest']['decision_rows_joined_to_r2']}",
        f"- r2_resolved_rows: {report['join_manifest']['r2_resolved_rows']}",
        f"- lifecycle_used_as_denominator: {str(report['lifecycle_used_as_denominator']).lower()}",
        "",
        "## Global Metrics",
        "",
        f"- buy_precision_r2: {metrics.get('buy_precision_r2')}",
        f"- false_reject_rate_resolved: {metrics.get('false_reject_rate_resolved')}",
        f"- timeout_opportunity_loss_rate: {metrics.get('timeout_opportunity_loss_rate')}",
        f"- buy_positive_rows / buy_negative_rows: {metrics.get('buy_positive_rows')} / {metrics.get('buy_negative_rows')}",
        f"- nonbuy_positive_rows / nonbuy_negative_rows: {metrics.get('nonbuy_positive_rows')} / {metrics.get('nonbuy_negative_rows')}",
        "",
        "## Fail Reasons",
        "",
    ]
    if report["fail_reasons"]:
        lines.extend(f"- {reason}" for reason in report["fail_reasons"])
    else:
        lines.append("- none")
    lines.extend(
        [
            "",
            "## Methodology",
            "",
            "`shadow_lifecycle.jsonl` is BUY-only post-decision lifecycle evidence. It is never used as denominator. "
            "The denominator is the full Gatekeeper decision population joined to `candidate_universe_v1.jsonl` "
            "and `r2_market_paths_v1.jsonl`.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    ds_dir = dataset_dir(root, args.selector_scope)
    outputs = default_outputs(root, args.selector_scope, args)
    candidate_path = ds_dir / "candidate_universe_v1.jsonl"
    r2_path = ds_dir / "r2_market_paths_v1.jsonl"
    training_path = ds_dir / "selector_training_view_v1.jsonl"
    lifecycle_path = ds_dir / "accepted_lifecycle_v1.jsonl"

    candidates, candidate_indexes = load_candidates(candidate_path)
    r2_by_candidate = load_index(r2_path)
    training_by_candidate = load_index(training_path)
    lifecycle_by_candidate = load_index(lifecycle_path)

    paths = decision_paths(root, args.runtime_scope, args.decision_plane)
    decision_rows_read = 0
    joined_rows: list[dict[str, Any]] = []
    unmatched_decision_rows = 0
    duplicate_keys: Counter[tuple[Any, ...]] = Counter()
    join_method_counts: Counter[str] = Counter()

    for path in paths:
        for decision in read_jsonl(path):
            decision_rows_read += 1
            plane = str(decision.get("decision_plane") or ("v25_shadow" if "/v25_shadow/" in path.as_posix() else "legacy_live"))
            candidate, method = join_candidate(decision, candidate_indexes, int(args.nearest_tolerance_ms))
            join_method_counts[method] += 1
            if candidate is None:
                unmatched_decision_rows += 1
                continue
            candidate_id = common.str_or_none(candidate.get("candidate_id"))
            r2 = r2_by_candidate.get(candidate_id or "")
            training = training_by_candidate.get(candidate_id or "")
            lifecycle = lifecycle_by_candidate.get(candidate_id or "")
            bucket = decision_bucket(decision)
            r2_class = normalize_r2_class(r2)
            row = {
                "candidate_id": candidate_id,
                "pool_id": row_pool(candidate) or row_pool(decision),
                "base_mint": row_mint(candidate) or row_mint(decision),
                "decision_plane": plane,
                "decision_bucket": bucket,
                "verdict_type": raw_verdict(decision),
                "r2_class": r2_class,
                "join_method": method,
                "decision": decision,
                "candidate": candidate,
                "r2": r2,
                "training": training,
                "lifecycle": lifecycle,
            }
            joined_rows.append(row)
            duplicate_keys[(plane, candidate_id, raw_verdict(decision), row_decision_ts(decision))] += 1

    joined_candidate_ids = {row.get("candidate_id") for row in joined_rows if row.get("candidate_id")}
    unmatched_candidate_rows = len([row for row in candidates if row.get("candidate_id") not in joined_candidate_ids])
    duplicate_decision_rows = sum(count - 1 for count in duplicate_keys.values() if count > 1)
    decision_rows_joined_to_r2 = sum(1 for row in joined_rows if row.get("r2") is not None)

    matrix_counts: dict[tuple[str, str, str], Counter[str]] = defaultdict(Counter)
    reason_counts: dict[tuple[str, str, str, str], Counter[str]] = defaultdict(Counter)
    xgb_values: dict[tuple[str, str, str], list[float]] = defaultdict(list)
    for row in joined_rows:
        matrix_counts[(row["decision_plane"], row["decision_bucket"], row["verdict_type"])][row["r2_class"]] += 1
        reason = str(row["decision"].get("reason_code") or row["decision"].get("gatekeeper_first_kill_reason") or row["decision"].get("decision_reason") or "")
        reason_counts[(row["decision_plane"], row["decision_bucket"], row["verdict_type"], reason)][row["r2_class"]] += 1
        cls = outcome_class(row["decision_bucket"], row["r2_class"])
        if cls:
            for metric in XGB_METRICS:
                value = metric_value(row["decision"], row.get("training"), metric)
                if value is not None:
                    xgb_values[(row["decision_plane"], cls, metric)].append(value)

    confusion_rows: list[dict[str, Any]] = []
    for (plane, bucket, verdict), counts in sorted(matrix_counts.items()):
        positive = counts["positive"]
        negative = counts["negative"]
        unresolved = counts["unresolved"]
        horizon = counts["horizon_unmatured"]
        missing = counts["missing_path"]
        stream = counts["stream_incomplete"]
        candidate_missing_ts = counts["candidate_missing_decision_ts_ms"]
        total = sum(counts.values())
        resolved = positive + negative
        confusion_rows.append(
            {
                "decision_plane": plane,
                "decision_bucket": bucket,
                "verdict_type": verdict,
                "total_rows": total,
                "r2_positive_rows": positive,
                "r2_negative_rows": negative,
                "r2_unresolved_rows": unresolved,
                "r2_horizon_unmatured_rows": horizon,
                "r2_missing_path_rows": missing,
                "r2_stream_incomplete_rows": stream,
                "r2_candidate_missing_decision_ts_ms_rows": candidate_missing_ts,
                "resolved_rows": resolved,
                "positive_rate_resolved": rate(positive, resolved),
                "negative_rate_resolved": rate(negative, resolved),
                "unresolved_rate": rate(total - resolved, total),
                "interpretation": interpretation(bucket, "positive" if positive else ("negative" if negative else "unresolved")),
            }
        )

    reason_rows: list[dict[str, Any]] = []
    for (plane, bucket, verdict, reason), counts in sorted(reason_counts.items()):
        positive = counts["positive"]
        negative = counts["negative"]
        total = sum(counts.values())
        resolved = positive + negative
        reason_rows.append(
            {
                "decision_plane": plane,
                "decision_bucket": bucket,
                "verdict_type": verdict,
                "reason": reason,
                "total_rows": total,
                "resolved_rows": resolved,
                "r2_positive_rows": positive,
                "r2_negative_rows": negative,
                "r2_unresolved_rows": total - resolved,
                "positive_rate_resolved": rate(positive, resolved),
                "negative_rate_resolved": rate(negative, resolved),
            }
        )

    xgb_rows: list[dict[str, Any]] = []
    for (plane, cls, metric), values in sorted(xgb_values.items()):
        values_sorted = sorted(values)
        xgb_rows.append(
            {
                "decision_plane": plane,
                "decision_outcome_class": cls,
                "metric": metric,
                "count": len(values_sorted),
                "mean": sum(values_sorted) / len(values_sorted),
                "median": statistics.median(values_sorted),
                "min": values_sorted[0],
                "max": values_sorted[-1],
            }
        )

    buy_rows = [row for row in joined_rows if row["decision_bucket"] == "BUY"]
    nonbuy_rows = [row for row in joined_rows if row["decision_bucket"] != "BUY"]
    reject_rows = [row for row in joined_rows if row["decision_bucket"].startswith("REJECT")]
    timeout_rows = [row for row in joined_rows if row["decision_bucket"].startswith("TIMEOUT")]

    def r2_count(rows: list[dict[str, Any]], cls: str) -> int:
        return sum(1 for row in rows if row["r2_class"] == cls)

    buy_positive = r2_count(buy_rows, "positive")
    buy_negative = r2_count(buy_rows, "negative")
    nonbuy_positive = r2_count(nonbuy_rows, "positive")
    nonbuy_negative = r2_count(nonbuy_rows, "negative")
    reject_positive = r2_count(reject_rows, "positive")
    reject_negative = r2_count(reject_rows, "negative")
    timeout_positive = r2_count(timeout_rows, "positive")
    timeout_negative = r2_count(timeout_rows, "negative")

    def top_buckets(rows: list[dict[str, Any]], cls: str) -> list[dict[str, Any]]:
        counter: Counter[str] = Counter(row["decision_bucket"] for row in rows if row["r2_class"] == cls)
        return [{"decision_bucket": key, "rows": value} for key, value in counter.most_common(10)]

    global_metrics = {
        "resolved_decision_rows": buy_positive + buy_negative + nonbuy_positive + nonbuy_negative,
        "buy_resolved_rows": buy_positive + buy_negative,
        "buy_positive_rows": buy_positive,
        "buy_negative_rows": buy_negative,
        "buy_precision_r2": rate(buy_positive, buy_positive + buy_negative),
        "nonbuy_resolved_rows": nonbuy_positive + nonbuy_negative,
        "nonbuy_positive_rows": nonbuy_positive,
        "nonbuy_negative_rows": nonbuy_negative,
        "false_reject_rate_resolved": rate(nonbuy_positive, nonbuy_positive + nonbuy_negative),
        "reject_resolved_rows": reject_positive + reject_negative,
        "reject_positive_rows": reject_positive,
        "reject_negative_rows": reject_negative,
        "timeout_resolved_rows": timeout_positive + timeout_negative,
        "timeout_positive_rows": timeout_positive,
        "timeout_negative_rows": timeout_negative,
        "timeout_opportunity_loss_rate": rate(timeout_positive, timeout_positive + timeout_negative),
        "top_false_reject_decision_buckets": top_buckets(nonbuy_rows, "positive"),
        "top_false_buy_decision_buckets": top_buckets(buy_rows, "negative"),
        "top_timeout_opportunity_loss_buckets": top_buckets(timeout_rows, "positive"),
    }

    false_rejects = [concise_example(row) for row in joined_rows if row["decision_bucket"] != "BUY" and row["r2_class"] == "positive"]
    false_buys = [concise_example(row) for row in joined_rows if row["decision_bucket"] == "BUY" and row["r2_class"] == "negative"]
    timeout_loss = [concise_example(row) for row in joined_rows if row["decision_bucket"].startswith("TIMEOUT") and row["r2_class"] == "positive"]

    r2_rows = read_jsonl(r2_path) if r2_path.exists() else []
    r2_counts = count_fields(r2_rows)
    fail_reasons: list[str] = []
    if not candidates:
        fail_reasons.append("candidate_universe_empty")
    if len(r2_rows) != len(candidates):
        fail_reasons.append("r2_market_paths_rows_do_not_match_candidate_universe_rows")
    if not joined_rows:
        fail_reasons.append("decision_rows_joined_to_candidate_zero")
    if decision_rows_joined_to_r2 == 0:
        fail_reasons.append("decision_rows_joined_to_r2_zero")
    if buy_positive + buy_negative == 0:
        fail_reasons.append("buy_positive_negative_not_counted")
    if reject_positive + reject_negative == 0:
        fail_reasons.append("reject_positive_negative_not_counted")
    if timeout_positive + timeout_negative == 0:
        fail_reasons.append("timeout_positive_negative_not_counted")
    if not any(row["r2_class"] not in {"positive", "negative"} for row in joined_rows):
        fail_reasons.append("unresolved_not_present_or_not_separated")
    if (reject_positive + reject_negative == 0) or (timeout_positive + timeout_negative == 0):
        fail_reasons.append("NO-GO_INCOMPLETE_DECISION_JOIN")

    status = "PASS" if not fail_reasons else "FAIL"
    join_manifest = {
        "candidate_universe_rows": len(candidates),
        "decision_rows_read": decision_rows_read,
        "decision_rows_loaded": decision_rows_read,
        "decision_rows_joined_to_candidate": len(joined_rows),
        "decision_rows_joined_to_r2": decision_rows_joined_to_r2,
        "r2_market_path_rows": len(r2_rows),
        "r2_resolved_rows": r2_counts.get("positive", 0) + r2_counts.get("negative", 0),
        "join_method_counts": dict(join_method_counts),
        "unmatched_decision_rows": unmatched_decision_rows,
        "unmatched_candidate_rows": unmatched_candidate_rows,
        "duplicate_decision_rows": duplicate_decision_rows,
        "r2_label_counts": r2_counts,
        "training_view_loaded": training_path.exists(),
        "training_view_rows": len(training_by_candidate),
        "accepted_lifecycle_loaded": lifecycle_path.exists(),
        "accepted_lifecycle_rows": len(lifecycle_by_candidate),
    }
    report = {
        "artifact": ARTIFACT,
        "status": status,
        "runtime_scope": args.runtime_scope,
        "selector_scope": args.selector_scope,
        "decision_plane": args.decision_plane,
        "lifecycle_used_as_denominator": False,
        "join_manifest": join_manifest,
        "global_metrics": global_metrics,
        "fail_reasons": fail_reasons,
        "outputs": {key: str(value) for key, value in outputs.items()},
    }

    common.write_json(outputs["json"], report)
    write_markdown(outputs["md"], report)
    write_csv(
        outputs["confusion_csv"],
        confusion_rows,
        [
            "decision_plane",
            "decision_bucket",
            "verdict_type",
            "total_rows",
            "r2_positive_rows",
            "r2_negative_rows",
            "r2_unresolved_rows",
            "r2_horizon_unmatured_rows",
            "r2_missing_path_rows",
            "r2_stream_incomplete_rows",
            "r2_candidate_missing_decision_ts_ms_rows",
            "resolved_rows",
            "positive_rate_resolved",
            "negative_rate_resolved",
            "unresolved_rate",
            "interpretation",
        ],
    )
    write_csv(
        outputs["reason_rates_csv"],
        reason_rows,
        [
            "decision_plane",
            "decision_bucket",
            "verdict_type",
            "reason",
            "total_rows",
            "resolved_rows",
            "r2_positive_rows",
            "r2_negative_rows",
            "r2_unresolved_rows",
            "positive_rate_resolved",
            "negative_rate_resolved",
        ],
    )
    write_csv(
        outputs["xgb_metrics_csv"],
        xgb_rows,
        ["decision_plane", "decision_outcome_class", "metric", "count", "mean", "median", "min", "max"],
    )
    write_jsonl(outputs["false_rejects_jsonl"], false_rejects[:TOP_EXAMPLE_LIMIT])
    write_jsonl(outputs["false_buys_jsonl"], false_buys[:TOP_EXAMPLE_LIMIT])
    write_jsonl(outputs["timeout_loss_jsonl"], timeout_loss[:TOP_EXAMPLE_LIMIT])
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0 if report["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
