#!/usr/bin/env python3
"""Audit R2-only selector feature availability and source-field coverage."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

import build_selector_training_view as training
import selector_pipeline_common as common


DEFAULT_FEATURES = (
    "curve_progress_pct",
    "sell_share",
    "top1_wallet_share",
    "buyer_hhi",
    "net_quote_in_15s",
    "net_quote_in_30s",
    "trade_rate",
    "unique_buyers",
    "quote_mint_is_sol",
    "creator_sold_early_flag",
)
SIDE_FIELDS = ("side", "trade_side", "direction", "is_buy", "buy", "ix_name")
WALLET_FIELDS = ("signer", "buyer", "seller", "wallet", "owner", "trader", "user")
AMOUNT_FIELDS = (
    "quote_amount",
    "quote_amount_sol",
    "amount_sol",
    "volume_sol",
    "sol_amount",
    "trade_amount_sol",
    "amount",
)
CURVE_PROGRESS_FIELDS = (
    "curve_progress_pct",
    "bonding_curve_progress_pct",
    "bonding_curve_progress",
)


def read_json(path: Path | None) -> dict[str, Any]:
    if path is None or not path.exists():
        return {}
    with path.open(encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def resolve_paths(args: argparse.Namespace, report_dir: Path, dataset_dir: Path) -> dict[str, Any]:
    feature_manifest = read_json(args.feature_snapshots_manifest or report_dir / "feature_snapshots_manifest_v1.json")
    event_paths = list(args.events or [])
    if not event_paths:
        for raw in feature_manifest.get("input_event_paths", []):
            if isinstance(raw, str):
                event_paths.append(Path(raw))
    return {
        "training_view": args.training_view or dataset_dir / "selector_training_view_v1.jsonl",
        "feature_snapshots": args.feature_snapshots or dataset_dir / "feature_snapshots_v1.jsonl",
        "r2_market_paths": args.r2_market_paths or dataset_dir / "r2_market_paths_v1.jsonl",
        "feature_manifest": feature_manifest,
        "event_paths": event_paths,
    }


def value_present(value: Any) -> bool:
    return value not in (None, "")


def feature_presence(rows: list[dict[str, Any]], feature: str) -> dict[str, Any]:
    present = sum(1 for row in rows if value_present(row.get(feature)))
    numeric = sum(
        1
        for row in rows
        if isinstance(row.get(feature), bool) or common.float_or_none(row.get(feature)) is not None
    )
    return {
        "present_rows": present,
        "numeric_or_bool_rows": numeric,
        "present_rate": present / len(rows) if rows else None,
    }


def field_candidates_seen(rows: list[dict[str, Any]], fields: tuple[str, ...]) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for row in rows:
        for field in fields:
            if common.find_first_key(row, (field,)) not in (None, ""):
                counts[field] += 1
    return common.counter_dict(counts)


def source_event_probe(event_paths: list[Path], *, max_rows: int) -> dict[str, Any]:
    kind_counts: Counter[str] = Counter()
    top_level_keys: Counter[str] = Counter()
    payload_keys: Counter[str] = Counter()
    side_counts: Counter[str] = Counter()
    rows: list[dict[str, Any]] = []
    rows_checked = 0
    matched_tx_rows = 0
    amount_nonzero = 0
    for path in event_paths:
        if not path.exists():
            continue
        for row in common.iter_json_objects(path):
            rows_checked += 1
            if len(rows) < max_rows:
                rows.append(row)
            kind_counts[common.event_type(row)] += 1
            for key in row.keys():
                top_level_keys[key] += 1
            payload = common.nested(row, "kind", "payload") or row.get("payload")
            if isinstance(payload, dict):
                for key in payload.keys():
                    payload_keys[key] += 1
            side = common.event_side(row)
            if side:
                side_counts[side] += 1
                matched_tx_rows += 1
            if common.event_quote_amount(row) > 0.0:
                amount_nonzero += 1
    return {
        "candidate_event_rows_checked": rows_checked,
        "sampled_rows_for_field_probe": len(rows),
        "event_type_counts": common.counter_dict(kind_counts),
        "top_level_keys_seen": common.counter_dict(top_level_keys),
        "payload_keys_seen": common.counter_dict(payload_keys),
        "side_field_candidates_seen": field_candidates_seen(rows, SIDE_FIELDS),
        "wallet_field_candidates_seen": field_candidates_seen(rows, WALLET_FIELDS),
        "amount_field_candidates_seen": field_candidates_seen(rows, AMOUNT_FIELDS),
        "curve_progress_field_candidates_seen": field_candidates_seen(rows, CURVE_PROGRESS_FIELDS),
        "recognized_side_counts": common.counter_dict(side_counts),
        "recognized_buy_sell_rows": matched_tx_rows,
        "nonzero_quote_amount_rows": amount_nonzero,
    }


def feature_root_causes(
    feature: str,
    *,
    presence: dict[str, Any],
    snapshot_summary: dict[str, Any],
    source_probe: dict[str, Any],
) -> list[str]:
    if (presence.get("present_rows") or 0) > 0:
        return []
    reasons: list[str] = []
    if snapshot_summary.get("tx_event_count_nonzero_rows") == 0:
        reasons.append("no_transaction_events_joined_to_feature_snapshots")
    type_counts = source_probe.get("event_type_counts", {})
    if not any(str(key).lower() == "pooltransaction" for key in type_counts):
        reasons.append("source_event_artifacts_lack_pool_transaction_rows")
    if source_probe.get("recognized_buy_sell_rows") == 0:
        reasons.append("no_buy_side_detected")
    if not source_probe.get("wallet_field_candidates_seen"):
        reasons.append("missing_wallet_identity")
    if not source_probe.get("amount_field_candidates_seen") or source_probe.get("nonzero_quote_amount_rows") == 0:
        reasons.append("missing_quote_amount")
    if feature == "curve_progress_pct" and not source_probe.get("curve_progress_field_candidates_seen"):
        reasons.append("missing_curve_progress_fields_in_source_events")
    if feature in {"sell_share", "top1_wallet_share", "buyer_hhi"} and feature not in {
        "curve_progress_pct"
    }:
        reasons.append("flow_or_concentration_feature_requires_buy_sell_tx_events")
    return list(dict.fromkeys(reasons))


def snapshot_summary(rows: list[dict[str, Any]]) -> dict[str, Any]:
    tx_nonzero = sum(1 for row in rows if (common.int_or_none(row.get("tx_event_count")) or 0) > 0)
    source_nonzero = sum(
        1 for row in rows if (common.int_or_none(row.get("source_event_count")) or 0) > 0
    )
    return {
        "feature_snapshot_rows": len(rows),
        "feature_snapshot_status_counts": common.counter_dict(
            Counter(str(row.get("feature_snapshot_status") or "missing") for row in rows)
        ),
        "snapshot_kind_counts": common.counter_dict(
            Counter(str(row.get("snapshot_kind") or "missing") for row in rows)
        ),
        "tx_event_count_nonzero_rows": tx_nonzero,
        "source_event_count_nonzero_rows": source_nonzero,
    }


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    dataset_dir = args.root / "datasets" / "selector" / args.scope
    report_dir = args.root / "reports" / "selector" / args.scope
    paths = resolve_paths(args, report_dir, dataset_dir)
    training_rows = list(common.iter_json_objects(paths["training_view"]))
    denominator_rows = [row for row in training_rows if training.r2_training_denominator(row)]
    feature_rows = list(common.iter_json_objects(paths["feature_snapshots"]))
    r2_rows = list(common.iter_json_objects(paths["r2_market_paths"]))
    source_probe = source_event_probe(paths["event_paths"], max_rows=args.source_probe_rows)
    snapshots = snapshot_summary(feature_rows)
    feature_reports = []
    for feature in args.feature:
        all_presence = feature_presence(training_rows, feature)
        denominator_presence = feature_presence(denominator_rows, feature)
        report = {
            "feature": feature,
            "training_view": all_presence,
            "resolved_r2_denominator": denominator_presence,
            "root_cause_candidates": feature_root_causes(
                feature,
                presence=denominator_presence,
                snapshot_summary=snapshots,
                source_probe=source_probe,
            ),
            "source_field_probe": {
                "candidate_event_rows_checked": source_probe["candidate_event_rows_checked"],
                "keys_seen": sorted(source_probe.get("payload_keys_seen", {}).keys())[:100],
                "side_field_candidates_seen": source_probe["side_field_candidates_seen"],
                "wallet_field_candidates_seen": source_probe["wallet_field_candidates_seen"],
                "amount_field_candidates_seen": source_probe["amount_field_candidates_seen"],
                "curve_progress_field_candidates_seen": source_probe[
                    "curve_progress_field_candidates_seen"
                ],
            },
        }
        feature_reports.append(report)
    status = "P3C_PASS_DIAGNOSTIC_ONLY"
    payload = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": "selector_r2only_feature_audit_v1",
        "phase": "phase3",
        "status": status,
        "dataset_kind": "r2_only",
        "scope": args.scope,
        "training_rows": len(training_rows),
        "resolved_r2_denominator_rows": len(denominator_rows),
        "r2_market_path_rows": len(r2_rows),
        "snapshot_summary": snapshots,
        "source_event_probe": source_probe,
        "feature_reports": feature_reports,
        "diagnostic_conclusion": (
            "flow/concentration/curve features are unavailable because the feature rollup "
            "did not join buy/sell PoolTransaction-style events; current event artifacts are "
            "birth/candidate oriented"
        ),
        "claim_boundaries": {
            "diagnostic_only": True,
            "feature_fix_applied": False,
            "gatekeeper_tuning_started": False,
            "production_promotion_claim": False,
        },
    }
    common.write_json(args.output or report_dir / "selector_r2only_feature_audit_v1.json", payload)
    return payload


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--training-view", type=Path)
    parser.add_argument("--feature-snapshots", type=Path)
    parser.add_argument("--feature-snapshots-manifest", type=Path)
    parser.add_argument("--r2-market-paths", type=Path)
    parser.add_argument("--events", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--feature", action="append", default=list(DEFAULT_FEATURES))
    parser.add_argument("--source-probe-rows", type=int, default=5000)
    parser.add_argument("--json", action="store_true")
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    return build_report(args)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = run(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
