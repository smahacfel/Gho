#!/usr/bin/env python3
"""Design and probe new selector signal families after P4C NO-GO.

This is an offline-only research handoff. It does not build a runtime score,
change Gatekeeper, change execution, change send path, or tune thresholds. The
script asks whether buyer/wallet-quality and funding/cabal feature families are
decision-time-safe, currently materialized, and worth implementing minimally.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_r2only_baseline_report as baseline
import build_selector_r2only_model_candidate as p3g
import selector_pipeline_common as common


ARTIFACT = "new_signal_family_design_v1"
SPEC_ARTIFACT = "P4D_NEW_SIGNAL_FAMILY_SPEC.md"
MATRIX_ARTIFACT = "new_signal_family_feature_matrix_v1.csv"
BUYER_PROBE_ARTIFACT = "buyer_quality_offline_probe_v1.json"
FUNDING_PROBE_ARTIFACT = "funding_graph_offline_probe_v1.json"
COORDINATION_GLOB = "logs/rollout/{runtime_scope}/decisions/**/coordination_risk_evidence.jsonl"
TOP_K = (10, 25, 50)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--train-scope", required=True)
    parser.add_argument("--validation-scope", required=True)
    parser.add_argument("--train-runtime-scope", default=None)
    parser.add_argument("--validation-runtime-scope", default=None)
    parser.add_argument("--min-probe-availability", type=float, default=0.50)
    parser.add_argument("--min-useful-lift-pp", type=float, default=0.10)
    parser.add_argument("--output", default=None)
    parser.add_argument("--spec-output", default=None)
    parser.add_argument("--matrix-output", default=None)
    parser.add_argument("--buyer-probe-output", default=None)
    parser.add_argument("--funding-probe-output", default=None)
    parser.add_argument("--json", action="store_true")
    return parser


def training_view_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"


def phase3_manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "phase3_r2only_manifest_v1.json"


def report_dir(root: Path, validation_scope: str) -> Path:
    return root / "reports" / "selector" / validation_scope


def read_json_object(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
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


def run_quality(root: Path, scope: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    manifest_path = phase3_manifest_path(root, scope)
    manifest = read_json_object(manifest_path)
    positives = sum(1 for row in rows if label_positive(row))
    return {
        "scope": scope,
        "denominator_rows": len(rows),
        "positive_rows": positives,
        "negative_rows": len(rows) - positives,
        "base_positive_rate": positives / len(rows) if rows else None,
        "leakage_audit_status": manifest.get("leakage_audit_status"),
        "leakage_clean": manifest.get("leakage_audit_status") == "PASS",
    }


def feature_specs() -> list[dict[str, Any]]:
    buyer_common = {
        "family": "buyer_wallet_quality",
        "decision_time_safe": True,
        "window_ms": "pre_decision_or_historical_before_cutoff",
        "expected_cost": "medium",
        "expected_latency": "low_if_incremental_index_exists",
        "missing_policy": "unknown_not_zero; score invalid/degraded according to requiredness",
    }
    funding_common = {
        "family": "funding_cabal",
        "decision_time_safe": True,
        "window_ms": "funding_lookback_5m_15m_60m_before_decision_cutoff",
        "expected_cost": "medium_high",
        "expected_latency": "low_if_rolling_funding_index_is_warm",
        "missing_policy": "unknown_not_clean; funding lane unavailable must invalidate cabal score",
    }
    return [
        {
            **buyer_common,
            "feature": "early_buyer_wallet_age_proxy",
            "source_fields": ["buyer_first_seen_slot", "buyer_first_seen_ts_ms"],
            "current_proxy_fields": [],
            "offline_current_view_status": "not_materialized_in_current_view",
        },
        {
            **buyer_common,
            "feature": "early_buyer_prior_success_count",
            "source_fields": ["buyer_historical_r2_positive_count_before_cutoff"],
            "current_proxy_fields": ["gk_signer_cross_pool_velocity"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "early_buyer_prior_fail_count",
            "source_fields": ["buyer_historical_r2_negative_count_before_cutoff"],
            "current_proxy_fields": ["gk_signer_cross_pool_velocity"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "early_buyer_repeat_participation",
            "source_fields": ["buyer_pool_participation_count_before_cutoff"],
            "current_proxy_fields": ["gk_signer_cross_pool_velocity"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "early_buyer_sell_after_buy_rate",
            "source_fields": ["buyer_sell_after_buy_rate_before_cutoff"],
            "current_proxy_fields": ["gk_sell_buy_ratio", "sell_share"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "early_buyer_fresh_wallet_share",
            "source_fields": ["fresh_wallet_count", "buyer_sample_count"],
            "current_proxy_fields": ["gk_fsc_unknown_buyer_rate"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "early_buyer_unique_funding_sources",
            "source_fields": ["funding_source_v2.known_funders"],
            "current_proxy_fields": ["gk_fsc_known_source_rate", "gk_fsc_buyer_sample_count"],
            "offline_current_view_status": "proxy_only_requires_funding_lane",
        },
        {
            **buyer_common,
            "feature": "smart_experienced_buyer_share",
            "source_fields": ["buyer_quality_tier_counts_before_cutoff"],
            "current_proxy_fields": ["gk_signer_cross_pool_velocity"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **buyer_common,
            "feature": "first_N_buyers_quality_score",
            "source_fields": ["first_n_buyer_quality_vector"],
            "current_proxy_fields": ["gk_signer_cross_pool_velocity", "gk_fsc_unknown_buyer_rate"],
            "offline_current_view_status": "proxy_only",
        },
        {
            **funding_common,
            "feature": "common_funding_source_count",
            "source_fields": ["funding_source_v2.top_funder_count"],
            "current_proxy_fields": ["gk_fsc_known_source_count"],
            "offline_current_view_status": "requires_funding_lane",
        },
        {
            **funding_common,
            "feature": "top_funding_source_share",
            "source_fields": ["funding_source_v2.top_funder_count", "funding_source_v2.total_buyers"],
            "current_proxy_fields": ["gk_fsc_known_source_rate"],
            "offline_current_view_status": "requires_funding_lane",
        },
        {
            **funding_common,
            "feature": "buyers_funded_within_5m",
            "source_fields": ["funding_transfers_5m"],
            "current_proxy_fields": [],
            "offline_current_view_status": "not_materialized_in_current_view",
        },
        {
            **funding_common,
            "feature": "buyers_funded_within_15m",
            "source_fields": ["funding_transfers_15m"],
            "current_proxy_fields": [],
            "offline_current_view_status": "not_materialized_in_current_view",
        },
        {
            **funding_common,
            "feature": "buyers_funded_within_60m",
            "source_fields": ["funding_transfers_60m"],
            "current_proxy_fields": [],
            "offline_current_view_status": "not_materialized_in_current_view",
        },
        {
            **funding_common,
            "feature": "same_funder_cluster_size",
            "source_fields": ["funding_source_v2.same_funder_cluster_size"],
            "current_proxy_fields": [],
            "offline_current_view_status": "requires_funding_lane",
        },
        {
            **funding_common,
            "feature": "sol_transfer_proximity_to_launch",
            "source_fields": ["funding_transfer_delta_ms_to_launch"],
            "current_proxy_fields": [],
            "offline_current_view_status": "not_materialized_in_current_view",
        },
        {
            **funding_common,
            "feature": "funding_graph_hhi",
            "source_fields": ["funding_source_v2.funder_hhi"],
            "current_proxy_fields": [],
            "offline_current_view_status": "requires_funding_lane",
        },
        {
            **funding_common,
            "feature": "dev_to_buyer_funding_link",
            "source_fields": ["dev_to_buyer_funding_link"],
            "current_proxy_fields": ["gk_dev_buyer_infrastructure_affinity"],
            "offline_current_view_status": "proxy_only_requires_funding_lane",
        },
        {
            **funding_common,
            "feature": "cabal_cluster_score",
            "source_fields": ["funding_graph_cluster_score"],
            "current_proxy_fields": ["gk_hhi", "gk_top3_volume_pct", "gk_dev_buyer_infrastructure_affinity"],
            "offline_current_view_status": "proxy_only",
        },
    ]


def availability(rows: list[dict[str, Any]], fields: list[str]) -> dict[str, Any]:
    if not rows:
        return {"rows": 0, "available_rows": 0, "availability_rate": None, "fields": fields}
    available = 0
    per_field: dict[str, int] = {field: 0 for field in fields}
    for row in rows:
        row_has_any = False
        for field in fields:
            value = p3g.feature_value(row, field)
            if value is None:
                value = row.get(field)
            if value not in (None, ""):
                per_field[field] += 1
                row_has_any = True
        if row_has_any:
            available += 1
    return {
        "rows": len(rows),
        "available_rows": available,
        "availability_rate": available / len(rows),
        "per_field_available_rows": per_field,
        "fields": fields,
    }


def topk_metric(rows: list[dict[str, Any]], scored: list[tuple[dict[str, Any], float]], k: int) -> dict[str, Any]:
    selected = [row for row, _score in scored[: min(k, len(scored))]]
    positives = sum(1 for row in rows if label_positive(row))
    selected_positive = sum(1 for row in selected if label_positive(row))
    precision = selected_positive / len(selected) if selected else None
    base_rate = positives / len(rows) if rows else None
    return {
        "selected_count": len(selected),
        "positive_count": selected_positive,
        "negative_count": len(selected) - selected_positive,
        "precision": precision,
        "base_positive_rate": base_rate,
        "lift_vs_base_rate_pp": (
            precision - base_rate if isinstance(precision, float) and isinstance(base_rate, float) else None
        ),
    }


def probe_numeric_feature(rows: list[dict[str, Any]], field: str, direction: str) -> dict[str, Any]:
    scored: list[tuple[dict[str, Any], float]] = []
    for row in rows:
        value = num(row, field)
        if value is None:
            continue
        score = value if direction == "high_good" else -value
        scored.append((row, score))
    scored.sort(
        key=lambda item: (
            -item[1],
            common.int_or_none(item[0].get("birth_ts_ms")) or common.int_or_none(item[0].get("decision_ts_ms")) or 0,
            row_key(item[0]),
        )
    )
    return {
        "field": field,
        "direction": direction,
        "available_rows": len(scored),
        "availability_rate": len(scored) / len(rows) if rows else None,
        "topk": {f"top{k}": topk_metric(rows, scored, k) for k in TOP_K},
    }


def buyer_probe(rows_by_run: dict[str, list[dict[str, Any]]]) -> dict[str, Any]:
    feature_directions = {
        "gk_signer_cross_pool_velocity": "low_good",
        "gk_fsc_unknown_buyer_rate": "low_good",
        "gk_sell_buy_ratio": "low_good",
        "sell_share": "low_good",
        "gk_dev_buyer_infrastructure_affinity": "low_good",
        "gk_fsc_known_source_rate": "high_good",
        "gk_buyer_pre_balance_cv": "low_good",
    }
    runs: dict[str, Any] = {}
    for run, rows in rows_by_run.items():
        probes = [probe_numeric_feature(rows, field, direction) for field, direction in feature_directions.items()]
        available = [probe for probe in probes if probe["available_rows"] > 0]
        useful = [
            probe
            for probe in available
            if isinstance(probe["topk"]["top25"].get("lift_vs_base_rate_pp"), float)
            and probe["topk"]["top25"]["lift_vs_base_rate_pp"] > 0.10
        ]
        runs[run] = {
            "rows": len(rows),
            "proxy_feature_count": len(feature_directions),
            "available_proxy_feature_count": len(available),
            "useful_proxy_feature_count_top25_lift_gt_10pp": len(useful),
            "features": probes,
        }
    return {
        "artifact": BUYER_PROBE_ARTIFACT.removesuffix(".json"),
        "family": "buyer_wallet_quality",
        "probe_kind": "current_view_proxy_probe",
        "runs": runs,
        "limitations": [
            "wallet age and prior success/fail counts are not directly materialized",
            "proxy fields cannot prove buyer quality without buyer identity history",
            "decision-time safety requires historical index keyed by buyer before current pool cutoff",
        ],
    }


def coordination_paths(root: Path, runtime_scope: str | None) -> list[Path]:
    if not runtime_scope:
        return []
    pattern = COORDINATION_GLOB.format(runtime_scope=runtime_scope)
    return sorted(root.glob(pattern))


def nested_get(row: dict[str, Any], path: list[str]) -> Any:
    value: Any = row
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def parse_funding_rows(paths: list[Path]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for path in paths:
        for row in common.iter_json_objects(path):
            funding = (
                nested_get(row, ["metric_breakdowns", "funding_source_v2", "breakdown"])
                or nested_get(row, ["sybil_resistance", "funding_source_v2"])
                or nested_get(row, ["funding_source_v2"])
            )
            if not isinstance(funding, dict):
                funding = {}
            out.append(
                {
                    "candidate_id": row.get("candidate_id"),
                    "funding_visibility": row.get("funding_visibility")
                    or nested_get(row, ["features", "funding_visibility"]),
                    "funding_status": funding.get("status") or funding.get("evidence_status"),
                    "known_buyers": common.int_or_none(funding.get("known_buyers")),
                    "unknown_count": common.int_or_none(funding.get("unknown_count")),
                    "total_buyers": common.int_or_none(funding.get("total_buyers")),
                    "known_coverage": common.float_or_none(funding.get("known_coverage")),
                    "top_funder_count": common.int_or_none(funding.get("top_funder_count")),
                    "top_funder_buy_sol": common.float_or_none(funding.get("top_funder_buy_sol")),
                    "capture_ready": funding.get("capture_ready"),
                    "excluded_reason": funding.get("excluded_reason"),
                    "provider": funding.get("provider"),
                }
            )
    return out


def funding_probe(root: Path, rows_by_run: dict[str, list[dict[str, Any]]], runtime_scopes: dict[str, str | None]) -> dict[str, Any]:
    runs: dict[str, Any] = {}
    for run, rows in rows_by_run.items():
        paths = coordination_paths(root, runtime_scopes.get(run))
        funding_rows = parse_funding_rows(paths)
        status_counts = Counter(str(row.get("funding_status") or row.get("funding_visibility") or "missing") for row in funding_rows)
        known_coverage_values = [row["known_coverage"] for row in funding_rows if row.get("known_coverage") is not None]
        ready_rows = sum(1 for row in funding_rows if row.get("capture_ready") is True or row.get("funding_status") in {"clean", "degraded"})
        unavailable_rows = sum(
            1
            for row in funding_rows
            if row.get("funding_status") == "unavailable"
            or row.get("funding_visibility") == "unavailable"
            or row.get("excluded_reason")
        )
        runs[run] = {
            "training_view_rows": len(rows),
            "coordination_evidence_files": [str(path) for path in paths],
            "coordination_evidence_rows": len(funding_rows),
            "funding_status_counts": common.counter_dict(status_counts),
            "capture_ready_rows": ready_rows,
            "unavailable_or_excluded_rows": unavailable_rows,
            "known_coverage_p50": percentile(known_coverage_values, 0.50),
            "known_coverage_p95": percentile(known_coverage_values, 0.95),
            "current_probe_status": (
                "funding_lane_not_available_for_scoring"
                if not funding_rows or unavailable_rows >= max(1, int(len(funding_rows) * 0.80))
                else "funding_lane_partially_available"
            ),
        }
    return {
        "artifact": FUNDING_PROBE_ARTIFACT.removesuffix(".json"),
        "family": "funding_cabal",
        "probe_kind": "coordination_risk_funding_lane_probe",
        "runs": runs,
        "limitations": [
            "funding graph cannot be evaluated as a scoring feature when funding_source_v2 is unavailable",
            "existing FSC rates in training view are proxy diagnostics, not same-funder graph evidence",
            "implementation requires decision-time rolling funding index and buyer identity join",
        ],
    }


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, int((len(ordered) - 1) * pct)))
    return ordered[index]


def matrix_rows(
    specs: list[dict[str, Any]],
    rows_by_run: dict[str, list[dict[str, Any]]],
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for spec in specs:
        row: dict[str, Any] = {
            "feature": spec["feature"],
            "family": spec["family"],
            "decision_time_safe": spec["decision_time_safe"],
            "source_fields": ";".join(spec["source_fields"]),
            "current_proxy_fields": ";".join(spec["current_proxy_fields"]),
            "window_ms": spec["window_ms"],
            "missing_policy": spec["missing_policy"],
            "expected_cost": spec["expected_cost"],
            "expected_latency": spec["expected_latency"],
            "offline_current_view_status": spec["offline_current_view_status"],
        }
        for run, rows in rows_by_run.items():
            fields = list(spec["current_proxy_fields"])
            available = availability(rows, fields) if fields else {"available_rows": 0, "availability_rate": 0.0}
            row[f"{run}_proxy_available_rows"] = available["available_rows"]
            row[f"{run}_proxy_availability_rate"] = available["availability_rate"]
        out.append(row)
    return out


def classify_recommendation(buyer: dict[str, Any], funding: dict[str, Any]) -> str:
    buyer_available = all(
        payload["available_proxy_feature_count"] >= 3
        for payload in buyer["runs"].values()
    )
    buyer_useful = any(
        payload["useful_proxy_feature_count_top25_lift_gt_10pp"] > 0
        for payload in buyer["runs"].values()
    )
    funding_unavailable = all(
        payload["current_probe_status"] == "funding_lane_not_available_for_scoring"
        for payload in funding["runs"].values()
    )
    if buyer_available and buyer_useful and funding_unavailable:
        return "IMPLEMENT_BOTH_MINIMAL"
    if buyer_available and buyer_useful:
        return "IMPLEMENT_BUYER_QUALITY"
    if funding_unavailable:
        return "IMPLEMENT_FUNDING_GRAPH"
    return "NO_ACTIONABLE_NEW_SIGNAL"


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
        "# P4D New Signal Family Spec",
        "",
        f"Status: {report['status']}",
        f"Recommendation: {report['recommendation']}",
        f"Train scope: `{report['train_scope']}`",
        f"Validation scope: `{report['validation_scope']}`",
        "",
        "## Scope Boundaries",
        "",
        "- offline only",
        "- no runtime changes",
        "- no Gatekeeper changes",
        "- no execution or send-path changes",
        "- no threshold tuning and no burn-in",
        "",
        "## P4C Closure",
        "",
        f"P4C closure status: {report['p4c_closure']['status']}",
        f"Reason: {', '.join(report['p4c_closure']['reasons'])}",
        "",
        "## Feature Families",
        "",
        "| feature | family | current status | proxy fields |",
        "|---|---|---|---|",
    ]
    for item in report["feature_matrix"]:
        lines.append(
            "| {feature} | {family} | {status} | {proxy} |".format(
                feature=item["feature"],
                family=item["family"],
                status=item["offline_current_view_status"],
                proxy=item["current_proxy_fields"] or "none",
            )
        )
    lines.extend(
        [
            "",
            "## Probe Summary",
            "",
            f"Buyer/wallet quality: {report['family_recommendations']['buyer_wallet_quality']}",
            f"Funding/cabal: {report['family_recommendations']['funding_cabal']}",
            "",
            "## Recommendation",
            "",
            report["recommendation"],
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    train_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.train_scope))))
    validation_rows = denominator_rows(list(common.iter_json_objects(training_view_path(root, args.validation_scope))))
    rows_by_run = {"train": train_rows, "validation": validation_rows}
    runtime_scopes = {
        "train": args.train_runtime_scope,
        "validation": args.validation_runtime_scope,
    }
    specs = feature_specs()
    feature_matrix = matrix_rows(specs, rows_by_run)
    buyer = buyer_probe(rows_by_run)
    funding = funding_probe(root, rows_by_run, runtime_scopes)
    recommendation = classify_recommendation(buyer, funding)
    family_recommendations = {
        "buyer_wallet_quality": (
            "implement_minimal_buyer_quality_history"
            if recommendation in {"IMPLEMENT_BUYER_QUALITY", "IMPLEMENT_BOTH_MINIMAL"}
            else "defer_until_feature_sources_exist"
        ),
        "funding_cabal": (
            "implement_minimal_funding_graph_capture"
            if recommendation in {"IMPLEMENT_FUNDING_GRAPH", "IMPLEMENT_BOTH_MINIMAL"}
            else "defer"
        ),
    }
    if recommendation == "IMPLEMENT_BOTH_MINIMAL":
        status = "P4D_NEW_SIGNAL_FAMILY_SPEC_READY"
    elif recommendation in {"IMPLEMENT_BUYER_QUALITY", "IMPLEMENT_FUNDING_GRAPH"}:
        status = "P4D_PARTIAL_NEW_SIGNAL_FAMILY_SPEC_READY"
    else:
        status = "P4D_NO_ACTIONABLE_NEW_SIGNAL"

    out_dir = report_dir(root, args.validation_scope)
    output = Path(args.output) if args.output else out_dir / f"{ARTIFACT}.json"
    spec_output = Path(args.spec_output) if args.spec_output else out_dir / SPEC_ARTIFACT
    matrix_output = Path(args.matrix_output) if args.matrix_output else out_dir / MATRIX_ARTIFACT
    buyer_output = Path(args.buyer_probe_output) if args.buyer_probe_output else out_dir / BUYER_PROBE_ARTIFACT
    funding_output = Path(args.funding_probe_output) if args.funding_probe_output else out_dir / FUNDING_PROBE_ARTIFACT

    report = {
        "artifact": ARTIFACT,
        "status": status,
        "recommendation": recommendation,
        "train_scope": args.train_scope,
        "validation_scope": args.validation_scope,
        "p4c_closure": {
            "status": "P4C_NO_GO_SIMPLE_EVIDENCE_GATED_CANDIDATES",
            "reasons": [
                "no stable candidate from current feature families",
                "do not tune thresholds or build another simple score grid",
                "move to buyer/wallet quality and funding/cabal signal design",
            ],
        },
        "run_quality": {
            "train": run_quality(root, args.train_scope, train_rows),
            "validation": run_quality(root, args.validation_scope, validation_rows),
        },
        "claim_boundaries": {
            "offline_only": True,
            "diagnostic_only": True,
            "builds_runtime_score": False,
            "changes_runtime": False,
            "changes_gatekeeper": False,
            "changes_execution": False,
            "changes_send_path": False,
            "tunes_thresholds": False,
            "production_promotion_allowed": False,
        },
        "feature_matrix": feature_matrix,
        "buyer_quality_probe": buyer,
        "funding_graph_probe": funding,
        "family_recommendations": family_recommendations,
        "outputs": {
            ARTIFACT: str(output),
            SPEC_ARTIFACT: str(spec_output),
            MATRIX_ARTIFACT: str(matrix_output),
            BUYER_PROBE_ARTIFACT: str(buyer_output),
            FUNDING_PROBE_ARTIFACT: str(funding_output),
        },
    }
    write_json(output, report)
    write_json(buyer_output, buyer)
    write_json(funding_output, funding)
    write_csv(matrix_output, feature_matrix)
    write_markdown(spec_output, report)
    return report


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
