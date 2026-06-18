#!/usr/bin/env python3
"""Evaluate frozen selector candidates with TARGET/STOP/TIMEOUT business labels.

This audit is offline-only.  It recomputes first-barrier labels from
``r2_market_paths_v1.jsonl`` samples and does not use the legacy
positive/negative R2 label as the business outcome.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


BUSINESS_LABELS = {"TARGET", "STOP", "TIMEOUT"}
UNRESOLVED_LABELS = {
    "AMBIGUOUS_BARRIER_ORDER",
    "HORIZON_UNMATURED",
    "MISSING_PATH",
    "STREAM_INCOMPLETE",
    "NONCANONICAL_SOURCE",
    "NO_SAMPLES",
}
FIELD_ALIASES = {
    "bonding_progress_pct": "gk_bonding_progress_pct",
    "buy_ratio": "gk_buy_ratio",
    "current_market_cap_sol": "gk_current_market_cap_sol",
    "dev_has_sold": "gk_dev_has_sold",
    "dev_tx_ratio": "gk_dev_tx_ratio",
    "dev_volume_ratio": "gk_dev_volume_ratio",
    "hhi": "gk_hhi",
    "max_single_tx_price_impact_pct_observed": "gk_max_single_tx_price_impact_pct_observed",
    "price_change_ratio": "gk_price_change_ratio",
    "sell_buy_ratio": "gk_sell_buy_ratio",
    "sol_buy_ratio": "gk_sol_buy_ratio",
    "top3_volume_pct": "gk_top3_volume_pct",
    "unique_signers_evaluated": "gk_unique_signers_evaluated",
}


def finite_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        fv = float(value)
        return fv if math.isfinite(fv) else None
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None
        try:
            fv = float(text)
        except ValueError:
            return None
        return fv if math.isfinite(fv) else None
    return None


def condition_eval(record: dict[str, Any], condition: dict[str, Any]) -> dict[str, Any]:
    field = condition.get("field")
    op = condition.get("op")
    expected = condition.get("value")
    exists = isinstance(record, dict) and isinstance(field, str) and field in record
    actual_field = field
    if not exists and isinstance(field, str):
        alias = FIELD_ALIASES.get(field)
        if alias and alias in record:
            exists = True
            actual_field = alias
    actual = record.get(actual_field) if exists else None
    result = {
        "passed": False,
        "field": field,
        "actual_field": actual_field,
        "field_alias_applied": actual_field != field,
        "op": op,
        "expected": expected,
        "actual": actual,
        "reason": "fail",
    }
    missing = actual is None or (isinstance(actual, float) and not math.isfinite(actual))

    if op == "exists":
        result["passed"] = exists and not missing
        result["reason"] = "pass" if result["passed"] else "missing"
        return result
    if op == "missing":
        result["passed"] = (not exists) or missing
        result["reason"] = "pass" if result["passed"] else "fail"
        return result
    if not exists or missing:
        result["reason"] = "missing"
        return result

    if op in {">=", ">", "<=", "<"}:
        actual_num = finite_float(actual)
        expected_num = finite_float(expected)
        if actual_num is None or expected_num is None:
            result["reason"] = "type_error"
            return result
        if op == ">=":
            passed = actual_num >= expected_num
        elif op == ">":
            passed = actual_num > expected_num
        elif op == "<=":
            passed = actual_num <= expected_num
        else:
            passed = actual_num < expected_num
        result["passed"] = passed
        result["reason"] = "pass" if passed else "fail"
        return result

    if op in {"==", "!="}:
        passed = actual == expected
        result["passed"] = passed if op == "==" else not passed
        result["reason"] = "pass" if result["passed"] else "fail"
        return result

    if op in {"in", "not_in"}:
        values = expected if isinstance(expected, (list, tuple, set)) else [expected]
        passed = actual in values
        result["passed"] = passed if op == "in" else not passed
        result["reason"] = "pass" if result["passed"] else "fail"
        return result

    if op in {"startswith", "not_startswith"}:
        passed = str(actual).startswith(str(expected))
        result["passed"] = passed if op == "startswith" else not passed
        result["reason"] = "pass" if result["passed"] else "fail"
        return result

    result["reason"] = "unknown_op"
    return result


def record_matches(record: dict[str, Any], conditions: list[dict[str, Any]]) -> bool:
    return all(condition_eval(record, condition).get("passed") is True for condition in conditions)


def barrier_key(sample: dict[str, Any], *, decision_ts_ms: int | None) -> int | None:
    ts_ms = common.int_or_none(sample.get("ts_ms") or sample.get("timestamp_ms"))
    if ts_ms is not None:
        return ts_ms
    offset = common.int_or_none(sample.get("offset_ms"))
    if offset is None:
        return None
    if decision_ts_ms is not None:
        return decision_ts_ms + offset
    return offset


def normalize_samples(row: dict[str, Any], *, horizon_ms: int) -> list[dict[str, Any]]:
    raw = row.get("samples")
    if not isinstance(raw, list):
        return []
    samples: list[dict[str, Any]] = []
    for item in raw:
        if not isinstance(item, dict):
            continue
        offset = common.int_or_none(item.get("offset_ms"))
        if offset is not None and (offset < 0 or offset > horizon_ms):
            continue
        ret = finite_float(item.get("return_pct"))
        if ret is None:
            continue
        samples.append(dict(item))
    samples.sort(
        key=lambda sample: (
            barrier_key(sample, decision_ts_ms=common.int_or_none(row.get("decision_ts_ms"))) is None,
            barrier_key(sample, decision_ts_ms=common.int_or_none(row.get("decision_ts_ms"))) or 0,
        )
    )
    return samples


def first_barrier_label(
    row: dict[str, Any],
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    if row.get("path_coverage_ok") is not True:
        status = str(row.get("r2_status") or "")
        if status == "noncanonical_source":
            label = "NONCANONICAL_SOURCE"
        elif status == "stream_incomplete":
            label = "STREAM_INCOMPLETE"
        else:
            label = "MISSING_PATH"
        return {
            "business_label": label,
            "business_label_resolved": False,
            "business_excluded_reason": label.lower(),
            "target_hit_ts_ms": None,
            "stop_hit_ts_ms": None,
        }

    decision_ts_ms = common.int_or_none(row.get("decision_ts_ms"))
    samples = normalize_samples(row, horizon_ms=horizon_ms)
    if not samples:
        return {
            "business_label": "NO_SAMPLES",
            "business_label_resolved": False,
            "business_excluded_reason": "no_return_samples",
            "target_hit_ts_ms": None,
            "stop_hit_ts_ms": None,
        }

    target_keys: list[int] = []
    stop_keys: list[int] = []
    for sample in samples:
        key = barrier_key(sample, decision_ts_ms=decision_ts_ms)
        if key is None:
            continue
        ret = finite_float(sample.get("return_pct"))
        if ret is None:
            continue
        if ret >= target_net_pct:
            target_keys.append(key)
        if ret <= -abs(stop_net_pct):
            stop_keys.append(key)

    first_target = min(target_keys) if target_keys else None
    first_stop = min(stop_keys) if stop_keys else None
    if first_target is not None and first_stop is not None:
        if first_target == first_stop:
            return {
                "business_label": "AMBIGUOUS_BARRIER_ORDER",
                "business_label_resolved": False,
                "business_excluded_reason": "ambiguous_barrier_order",
                "target_hit_ts_ms": first_target,
                "stop_hit_ts_ms": first_stop,
            }
        if first_target < first_stop:
            return {
                "business_label": "TARGET",
                "business_label_resolved": True,
                "business_excluded_reason": None,
                "target_hit_ts_ms": first_target,
                "stop_hit_ts_ms": first_stop,
            }
        return {
            "business_label": "STOP",
            "business_label_resolved": True,
            "business_excluded_reason": None,
            "target_hit_ts_ms": first_target,
            "stop_hit_ts_ms": first_stop,
        }
    if first_target is not None:
        return {
            "business_label": "TARGET",
            "business_label_resolved": True,
            "business_excluded_reason": None,
            "target_hit_ts_ms": first_target,
            "stop_hit_ts_ms": None,
        }
    if first_stop is not None:
        return {
            "business_label": "STOP",
            "business_label_resolved": True,
            "business_excluded_reason": None,
            "target_hit_ts_ms": None,
            "stop_hit_ts_ms": first_stop,
        }

    horizon_matured = common.bool_or_none(row.get("horizon_matured"))
    if horizon_matured is None:
        offsets = [common.int_or_none(sample.get("offset_ms")) for sample in samples]
        offsets = [offset for offset in offsets if offset is not None]
        horizon_matured = bool(offsets) and max(offsets) >= horizon_ms
    if horizon_matured:
        return {
            "business_label": "TIMEOUT",
            "business_label_resolved": True,
            "business_excluded_reason": None,
            "target_hit_ts_ms": None,
            "stop_hit_ts_ms": None,
        }
    return {
        "business_label": "HORIZON_UNMATURED",
        "business_label_resolved": False,
        "business_excluded_reason": "horizon_unmatured",
        "target_hit_ts_ms": None,
        "stop_hit_ts_ms": None,
    }


def load_business_labels(
    r2_market_paths: Path,
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> tuple[dict[str, dict[str, Any]], dict[str, Any]]:
    labels: dict[str, dict[str, Any]] = {}
    counts: Counter[str] = Counter()
    rows_read = 0
    duplicate_candidate_ids = 0
    for row in common.iter_json_objects(r2_market_paths):
        rows_read += 1
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if not candidate_id:
            continue
        if candidate_id in labels:
            duplicate_candidate_ids += 1
            continue
        label = first_barrier_label(
            row,
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
        )
        labels[candidate_id] = label
        counts[str(label.get("business_label") or "UNKNOWN")] += 1
    resolved = sum(counts[label] for label in BUSINESS_LABELS)
    return labels, {
        "r2_market_path_rows_read": rows_read,
        "business_label_counts": common.counter_dict(counts),
        "business_resolved_rows": resolved,
        "duplicate_candidate_ids": duplicate_candidate_ids,
    }


def load_candidates(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return []
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    if isinstance(payload, dict) and isinstance(payload.get("shortlist"), list):
        return [
            {
                "candidate_id": str(item.get("candidate_id")),
                "conditions": item.get("conditions") if isinstance(item.get("conditions"), list) else [],
                "source": "shortlist",
                "primary_variant": item.get("primary_variant") or item.get("type") or "business_target_rate",
            }
            for item in payload["shortlist"]
            if isinstance(item, dict) and item.get("candidate_id")
        ]
    if isinstance(payload, dict) and isinstance(payload.get("ranking"), list):
        rows = []
        for item in payload["ranking"]:
            if not isinstance(item, dict):
                continue
            candidate = item.get("candidate") if isinstance(item.get("candidate"), dict) else item
            raw_conditions = candidate.get("conditions")
            if raw_conditions is None and candidate.get("conditions_json"):
                raw_conditions = json.loads(candidate["conditions_json"])
            rows.append(
                {
                    "candidate_id": str(candidate.get("candidate_id")),
                    "conditions": raw_conditions if isinstance(raw_conditions, list) else [],
                    "source": "ranking",
                    "primary_variant": candidate.get("primary_variant") or "business_target_rate",
                }
            )
        return [row for row in rows if row.get("candidate_id")]
    if isinstance(payload, list):
        return [
            {
                "candidate_id": str(item.get("candidate_id")),
                "conditions": item.get("conditions") if isinstance(item.get("conditions"), list) else [],
                "source": "list",
                "primary_variant": item.get("primary_variant") or "business_target_rate",
            }
            for item in payload
            if isinstance(item, dict) and item.get("candidate_id")
        ]
    raise ValueError(f"unsupported candidate shortlist shape: {path}")


def condition_field_coverage(rows: list[dict[str, Any]], conditions: list[dict[str, Any]]) -> dict[str, Any]:
    fields = [condition.get("field") for condition in conditions if isinstance(condition.get("field"), str)]
    if not fields or not rows:
        return {
            "condition_fields": fields,
            "field_coverage": {},
            "field_aliases_applied": {},
            "min_feature_coverage": None,
            "missing_rate": None,
        }
    coverage: dict[str, float] = {}
    aliases: dict[str, str] = {}
    for field in fields:
        actual_field = field
        if not any(field in row for row in rows):
            alias = FIELD_ALIASES.get(field)
            if alias and any(alias in row for row in rows):
                actual_field = alias
                aliases[field] = alias
        present = sum(
            1
            for row in rows
            if finite_float(row.get(actual_field)) is not None or row.get(actual_field) not in (None, "")
        )
        coverage[field] = present / len(rows)
    min_cov = min(coverage.values()) if coverage else None
    return {
        "condition_fields": fields,
        "field_coverage": coverage,
        "field_aliases_applied": aliases,
        "min_feature_coverage": min_cov,
        "missing_rate": (1.0 - min_cov) if min_cov is not None else None,
    }


def profile_from_rows(
    rows: list[dict[str, Any]],
    *,
    base_counts: Counter[str],
    base_total: int,
) -> dict[str, Any]:
    counts = Counter(str(row.get("_business_label")) for row in rows)
    selected_total = sum(counts[label] for label in BUSINESS_LABELS)
    target = counts["TARGET"]
    stop = counts["STOP"]
    timeout = counts["TIMEOUT"]
    target_rate = target / selected_total if selected_total else None
    stop_rate = stop / selected_total if selected_total else None
    timeout_rate = timeout / selected_total if selected_total else None
    base_target = base_counts["TARGET"] / base_total if base_total else None
    return {
        "selected_total": selected_total,
        "TARGET_count": target,
        "STOP_count": stop,
        "TIMEOUT_count": timeout,
        "AMBIGUOUS_count": counts["AMBIGUOUS_BARRIER_ORDER"],
        "TARGET_rate": target_rate,
        "STOP_rate": stop_rate,
        "TIMEOUT_rate": timeout_rate,
        "base_TARGET_rate": base_target,
        "TARGET_lift_pp": ((target_rate - base_target) * 100.0) if target_rate is not None and base_target is not None else None,
    }


def view_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    counts = Counter(str(row.get("_business_label")) for row in rows)

    def one(a: int, b: int) -> dict[str, Any]:
        total = a + b
        return {"A": a, "B": b, "total": total, "precision_A": (a / total) if total else None}

    return {
        "target_vs_not_target": one(counts["TARGET"], counts["STOP"] + counts["TIMEOUT"]),
        "target_vs_stop": one(counts["TARGET"], counts["STOP"]),
        "target_vs_timeout": one(counts["TARGET"], counts["TIMEOUT"]),
        "stop_vs_non_stop": one(counts["STOP"], counts["TARGET"] + counts["TIMEOUT"]),
    }


def business_verdict(profile: dict[str, Any], args: argparse.Namespace) -> tuple[str, list[str]]:
    selected = int(profile.get("selected_total") or 0)
    target_rate = profile.get("TARGET_rate")
    stop_rate = profile.get("STOP_rate")
    timeout_rate = profile.get("TIMEOUT_rate")
    lift = profile.get("TARGET_lift_pp")
    fail_reasons: list[str] = []
    if selected == 0:
        return "INCONCLUSIVE_EMPTY_SELECTION", ["empty_selection"]
    if selected < args.underpowered_min_selected:
        return "INCONCLUSIVE_INSUFFICIENT_SELECTED", ["insufficient_selected"]
    if target_rate is None or lift is None or stop_rate is None or timeout_rate is None:
        return "INCONCLUSIVE_MISSING_METRICS", ["missing_business_metrics"]
    if selected < args.min_selected:
        if target_rate >= args.primary_target_rate and lift > 0:
            return "PROMISING_UNDERPOWERED", ["selected_total_below_primary_pass"]
        return "INCONCLUSIVE_INSUFFICIENT_SELECTED", ["selected_total_below_primary_pass"]
    if (
        target_rate >= args.strong_target_rate
        and lift >= args.min_lift_pp
        and stop_rate <= args.strong_max_stop_rate
        and timeout_rate <= args.strong_max_timeout_rate
    ):
        return "STRONG_PASS", []
    if (
        target_rate >= args.primary_target_rate
        and lift >= args.min_lift_pp
        and stop_rate <= args.max_stop_rate
        and timeout_rate <= args.max_timeout_rate
    ):
        return "PRIMARY_PASS", []
    if target_rate <= (profile.get("base_TARGET_rate") or 0.0):
        fail_reasons.append("target_rate_not_above_base")
    if lift < args.fail_min_lift_pp:
        fail_reasons.append("target_lift_below_fail_floor")
    if stop_rate > args.max_stop_rate:
        fail_reasons.append("stop_rate_above_primary_limit")
    if timeout_rate > args.max_timeout_rate:
        fail_reasons.append("timeout_rate_above_primary_limit")
    return "FAIL" if fail_reasons else "INCONCLUSIVE_NEAR_THRESHOLD", fail_reasons


def evaluate_scope(
    *,
    root: Path,
    scope: str,
    candidates: list[dict[str, Any]],
    args: argparse.Namespace,
) -> dict[str, Any]:
    dataset_dir = root / "datasets" / "selector" / scope
    training_view = dataset_dir / "selector_training_view_v1.jsonl"
    r2_market_paths = dataset_dir / "r2_market_paths_v1.jsonl"
    if not training_view.exists() or not r2_market_paths.exists():
        return {
            "scope": scope,
            "status": "SKIP_MISSING_INPUTS",
            "training_view_exists": training_view.exists(),
            "r2_market_paths_exists": r2_market_paths.exists(),
            "candidates": [],
        }

    labels, label_manifest = load_business_labels(
        r2_market_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
    )
    rows: list[dict[str, Any]] = []
    unmatched_training_rows = 0
    for row in common.iter_json_objects(training_view):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        label = labels.get(candidate_id or "")
        if not label:
            unmatched_training_rows += 1
            continue
        enriched = dict(row)
        enriched["_business_label"] = label["business_label"]
        enriched["_business_label_resolved"] = label["business_label_resolved"]
        enriched["_business_excluded_reason"] = label["business_excluded_reason"]
        rows.append(enriched)

    resolved_rows = [row for row in rows if row.get("_business_label") in BUSINESS_LABELS]
    base_counts = Counter(str(row.get("_business_label")) for row in resolved_rows)
    base_total = sum(base_counts[label] for label in BUSINESS_LABELS)
    candidate_reports = []
    for candidate in candidates:
        conditions = candidate.get("conditions") if isinstance(candidate.get("conditions"), list) else []
        selected = [row for row in resolved_rows if record_matches(row, conditions)]
        profile = profile_from_rows(selected, base_counts=base_counts, base_total=base_total)
        coverage = condition_field_coverage(resolved_rows, conditions)
        verdict, fail_reasons = business_verdict(profile, args)
        candidate_reports.append(
            {
                "candidate_id": candidate.get("candidate_id"),
                "conditions": conditions,
                "candidate_source": candidate.get("source"),
                "primary_variant": candidate.get("primary_variant"),
                "business_verdict": verdict,
                "fail_reasons": fail_reasons,
                "profile": profile,
                "views": view_metrics(selected),
                "feature_coverage": coverage,
                "expected_sim_pnl_before_fees": None,
                "expected_sim_pnl_after_fees": None,
                "expected_sim_pnl_available": False,
            }
        )

    return {
        "scope": scope,
        "status": "PASS",
        "training_view_rows_joined": len(rows),
        "unmatched_training_rows": unmatched_training_rows,
        "target_net_pct": args.target_net_pct,
        "stop_net_pct": args.stop_net_pct,
        "horizon_ms": args.horizon_ms,
        "business_label_manifest": label_manifest,
        "base_profile": profile_from_rows(resolved_rows, base_counts=base_counts, base_total=base_total),
        "views": view_metrics(resolved_rows),
        "candidates": candidate_reports,
    }


def aggregate_candidates(scope_reports: list[dict[str, Any]], candidates: list[dict[str, Any]], args: argparse.Namespace) -> list[dict[str, Any]]:
    aggregated: list[dict[str, Any]] = []
    for candidate in candidates:
        candidate_id = candidate.get("candidate_id")
        totals = Counter()
        base_totals = Counter()
        per_scope = []
        for scope_report in scope_reports:
            if scope_report.get("status") != "PASS":
                continue
            base_manifest = scope_report.get("business_label_manifest", {}).get("business_label_counts", {})
            for label in BUSINESS_LABELS:
                base_totals[label] += int(base_manifest.get(label) or 0)
            item = next((row for row in scope_report.get("candidates", []) if row.get("candidate_id") == candidate_id), None)
            if not item:
                continue
            profile = item.get("profile", {})
            totals["TARGET"] += int(profile.get("TARGET_count") or 0)
            totals["STOP"] += int(profile.get("STOP_count") or 0)
            totals["TIMEOUT"] += int(profile.get("TIMEOUT_count") or 0)
            per_scope.append({"scope": scope_report.get("scope"), **profile, "business_verdict": item.get("business_verdict")})
        selected_total = sum(totals[label] for label in BUSINESS_LABELS)
        base_total = sum(base_totals[label] for label in BUSINESS_LABELS)
        target_rate = totals["TARGET"] / selected_total if selected_total else None
        stop_rate = totals["STOP"] / selected_total if selected_total else None
        timeout_rate = totals["TIMEOUT"] / selected_total if selected_total else None
        base_target_rate = base_totals["TARGET"] / base_total if base_total else None
        profile = {
            "selected_total": selected_total,
            "TARGET_count": totals["TARGET"],
            "STOP_count": totals["STOP"],
            "TIMEOUT_count": totals["TIMEOUT"],
            "TARGET_rate": target_rate,
            "STOP_rate": stop_rate,
            "TIMEOUT_rate": timeout_rate,
            "base_TARGET_rate": base_target_rate,
            "TARGET_lift_pp": ((target_rate - base_target_rate) * 100.0) if target_rate is not None and base_target_rate is not None else None,
        }
        verdict, fail_reasons = business_verdict(profile, args)
        aggregated.append(
            {
                "candidate_id": candidate_id,
                "conditions": candidate.get("conditions"),
                "business_verdict": verdict,
                "fail_reasons": fail_reasons,
                "profile": profile,
                "per_scope": per_scope,
            }
        )
    aggregated.sort(
        key=lambda row: (
            row.get("business_verdict") not in {"STRONG_PASS", "PRIMARY_PASS", "PROMISING_UNDERPOWERED"},
            -(row.get("profile", {}).get("TARGET_rate") or -1.0),
            -(row.get("profile", {}).get("selected_total") or 0),
        )
    )
    return aggregated


def write_csv(path: Path, aggregate: list[dict[str, Any]], scope_reports: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fields = [
        "level",
        "scope",
        "candidate_id",
        "business_verdict",
        "selected_total",
        "TARGET_count",
        "STOP_count",
        "TIMEOUT_count",
        "AMBIGUOUS_count",
        "TARGET_rate",
        "STOP_rate",
        "TIMEOUT_rate",
        "base_TARGET_rate",
        "TARGET_lift_pp",
    ]
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fields)
        writer.writeheader()
        for item in aggregate:
            profile = item.get("profile", {})
            writer.writerow({"level": "aggregate", "scope": "ALL", "candidate_id": item.get("candidate_id"), "business_verdict": item.get("business_verdict"), **profile})
        for scope_report in scope_reports:
            for item in scope_report.get("candidates", []):
                profile = item.get("profile", {})
                writer.writerow({"level": "scope", "scope": scope_report.get("scope"), "candidate_id": item.get("candidate_id"), "business_verdict": item.get("business_verdict"), **profile})


def fmt_pct(value: Any) -> str:
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        return "N/A"
    return f"{float(value) * 100:.2f}%"


def fmt_pp(value: Any) -> str:
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        return "N/A"
    return f"{float(value):+.2f} pp"


def write_md(path: Path, report: dict[str, Any]) -> None:
    lines = [
        "# Selector Business Target Rate Audit",
        "",
        "Offline-only audit. Runtime, Gatekeeper, execution and send path are unchanged.",
        "",
        "Primary metric: `TARGET_RATE_SELECTED = TARGET / (TARGET + STOP + TIMEOUT)`.",
        "",
        f"Label contract: TARGET `+{report['target_net_pct']}%`, STOP `-{report['stop_net_pct']}%`, horizon `{report['horizon_ms']} ms`.",
        "",
        "TIMEOUT is counted as NOT_TARGET for BUY precision and reported separately.",
        "",
        "## Aggregate Candidates",
        "",
        "| candidate_id | verdict | selected | TARGET | STOP | TIMEOUT | TARGET_rate | STOP_rate | TIMEOUT_rate | base_TARGET_rate | lift |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for item in report.get("aggregate_candidates", []):
        profile = item.get("profile", {})
        lines.append(
            "| {candidate_id} | {verdict} | {selected} | {target} | {stop} | {timeout} | {target_rate} | {stop_rate} | {timeout_rate} | {base_rate} | {lift} |".format(
                candidate_id=item.get("candidate_id"),
                verdict=item.get("business_verdict"),
                selected=profile.get("selected_total"),
                target=profile.get("TARGET_count"),
                stop=profile.get("STOP_count"),
                timeout=profile.get("TIMEOUT_count"),
                target_rate=fmt_pct(profile.get("TARGET_rate")),
                stop_rate=fmt_pct(profile.get("STOP_rate")),
                timeout_rate=fmt_pct(profile.get("TIMEOUT_rate")),
                base_rate=fmt_pct(profile.get("base_TARGET_rate")),
                lift=fmt_pp(profile.get("TARGET_lift_pp")),
            )
        )
    lines.extend(["", "## Scope Base Profiles", ""])
    for scope in report.get("scopes", []):
        base = scope.get("base_profile", {})
        lines.append(
            f"- `{scope.get('scope')}`: status `{scope.get('status')}`, "
            f"TARGET={base.get('TARGET_count')}, STOP={base.get('STOP_count')}, TIMEOUT={base.get('TIMEOUT_count')}, "
            f"base_TARGET_rate={fmt_pct(base.get('TARGET_rate'))}"
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    candidates = load_candidates(args.candidate_shortlist)
    if not candidates:
        raise ValueError("no candidates loaded")
    scopes = args.scope or []
    scope_reports = [
        evaluate_scope(root=args.root, scope=scope, candidates=candidates, args=args)
        for scope in scopes
    ]
    aggregate = aggregate_candidates(scope_reports, candidates, args)
    return {
        "artifact": "selector_business_target_rate_audit_v1",
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "PASS",
        "target_net_pct": args.target_net_pct,
        "stop_net_pct": args.stop_net_pct,
        "horizon_ms": args.horizon_ms,
        "primary_metric": "TARGET_RATE_SELECTED = TARGET / (TARGET + STOP + TIMEOUT)",
        "timeout_policy": "TIMEOUT is NOT_TARGET for BUY precision and is reported separately.",
        "ambiguous_policy": "AMBIGUOUS_BARRIER_ORDER is excluded from main metrics and reported separately.",
        "runtime_changes": False,
        "gatekeeper_changes": False,
        "execution_changes": False,
        "send_path_changes": False,
        "candidate_count": len(candidates),
        "scopes": scope_reports,
        "aggregate_candidates": aggregate,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--scope", action="append", default=[])
    parser.add_argument("--candidate-shortlist", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--target-net-pct", type=float, default=25.0)
    parser.add_argument("--stop-net-pct", type=float, default=25.0)
    parser.add_argument("--horizon-ms", type=int, default=60_000)
    parser.add_argument("--min-selected", type=int, default=100)
    parser.add_argument("--underpowered-min-selected", type=int, default=30)
    parser.add_argument("--primary-target-rate", type=float, default=0.65)
    parser.add_argument("--strong-target-rate", type=float, default=0.70)
    parser.add_argument("--min-lift-pp", type=float, default=15.0)
    parser.add_argument("--fail-min-lift-pp", type=float, default=5.0)
    parser.add_argument("--max-stop-rate", type=float, default=0.20)
    parser.add_argument("--max-timeout-rate", type=float, default=0.25)
    parser.add_argument("--strong-max-stop-rate", type=float, default=0.15)
    parser.add_argument("--strong-max-timeout-rate", type=float, default=0.20)
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.output_dir:
        output_dir = args.output_dir
        output_dir.mkdir(parents=True, exist_ok=True)
        common.write_json(output_dir / "selector_business_target_rate_v1.json", report)
        write_csv(output_dir / "selector_business_target_rate_candidates_v1.csv", report["aggregate_candidates"], report["scopes"])
        write_md(output_dir / "SELECTOR_BUSINESS_TARGET_RATE.md", report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
