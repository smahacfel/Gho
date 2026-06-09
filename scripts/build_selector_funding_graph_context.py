#!/usr/bin/env python3
"""Build minimal funding graph context for selector candidates.

P4E-B is offline-only. It materializes funding/cabal context status and coverage
from durable evidence. Unknown or unavailable funding evidence is never treated
as safe zero evidence.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "funding_graph_context_v1"
MANIFEST_ARTIFACT = "funding_graph_context_manifest_v1"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", required=True)
    parser.add_argument("--runtime-scope", required=True)
    parser.add_argument("--candidate-universe", type=Path)
    parser.add_argument("--coordination-glob", action="append")
    parser.add_argument("--funding-events", type=Path)
    parser.add_argument("--system-transfers", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def candidate_universe_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "candidate_universe_v1.jsonl"


def output_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / f"{ARTIFACT}.jsonl"


def manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"{MANIFEST_ARTIFACT}.json"


def coordination_paths(root: Path, runtime_scope: str, globs: list[str] | None) -> list[Path]:
    patterns = globs or [
        f"logs/rollout/{runtime_scope}/decisions/**/coordination_risk_evidence.jsonl"
    ]
    paths: list[Path] = []
    for pattern in patterns:
        paths.extend(root.glob(pattern))
    return sorted(set(paths))


def nested_get(value: Any, *keys: str) -> Any:
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def funding_payload(row: dict[str, Any]) -> dict[str, Any]:
    for path in (
        ("sybil_resistance", "funding_source_v2"),
        ("metric_breakdowns", "funding_source_v2", "breakdown"),
        ("funding_source_v2",),
    ):
        value = nested_get(row, *path)
        if isinstance(value, dict):
            return value
    return {}


def parse_coordination(paths: list[Path]) -> dict[str, list[dict[str, Any]]]:
    by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for path in paths:
        for row in common.iter_json_objects(path):
            candidate_id = common.str_or_none(row.get("candidate_id"))
            if not candidate_id:
                continue
            funding = funding_payload(row)
            by_candidate[candidate_id].append(
                {
                    "candidate_id": candidate_id,
                    "funding_visibility": row.get("funding_visibility")
                    or nested_get(row, "features", "funding_visibility"),
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
                    "raw": funding,
                }
            )
    return by_candidate


def parse_optional_funding_events(path: Path | None) -> dict[str, list[dict[str, Any]]]:
    if path is None or not path.exists():
        return {}
    by_candidate: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in common.iter_json_objects(path):
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id:
            by_candidate[candidate_id].append(row)
    return by_candidate


def selected_evidence(items: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not items:
        return None
    clean = [item for item in items if item.get("funding_status") in {"clean", "degraded"}]
    return clean[-1] if clean else items[-1]


def hhi_from_counts(counts: list[int]) -> float | None:
    total = sum(counts)
    if total <= 0:
        return None
    return sum((count / total) ** 2 for count in counts)


def funding_event_counts(events: list[dict[str, Any]]) -> dict[str, Any]:
    windows = {5: 0, 15: 0, 60: 0}
    dev_links = 0
    funder_counts: Counter[str] = Counter()
    for row in events:
        delta_ms = common.int_or_none(
            row.get("funding_delta_ms_to_launch")
            or row.get("delta_ms_to_launch")
            or row.get("transfer_delta_ms")
        )
        if delta_ms is not None:
            for minutes in windows:
                if 0 <= delta_ms <= minutes * 60_000:
                    windows[minutes] += 1
        if row.get("dev_to_buyer_link") is True:
            dev_links += 1
        source = common.str_or_none(row.get("funding_source")) or common.str_or_none(row.get("source_wallet"))
        if source:
            funder_counts[source] += 1
    return {
        "funded_within_5m": windows[5],
        "funded_within_15m": windows[15],
        "funded_within_60m": windows[60],
        "dev_links": dev_links,
        "funder_counts": funder_counts,
    }


def build_context_row(
    candidate: dict[str, Any],
    evidence_by_candidate: dict[str, list[dict[str, Any]]],
    events_by_candidate: dict[str, list[dict[str, Any]]],
) -> dict[str, Any]:
    candidate_id = common.str_or_none(candidate.get("candidate_id")) or ""
    evidence = selected_evidence(evidence_by_candidate.get(candidate_id, []))
    funding_events = funding_event_counts(events_by_candidate.get(candidate_id, []))
    reasons: list[str] = []
    if evidence is None:
        status = "missing_funding_lane"
        reasons.append("no_coordination_funding_evidence")
    else:
        raw_status = str(evidence.get("funding_status") or evidence.get("funding_visibility") or "unknown")
        if raw_status == "unavailable" or evidence.get("excluded_reason"):
            status = "unavailable"
            reasons.append(str(evidence.get("excluded_reason") or "funding_lane_unavailable"))
        elif evidence.get("known_coverage") is not None and float(evidence["known_coverage"]) < 0.50:
            status = "degraded_low_coverage"
            reasons.append("known_coverage_below_50pct")
        elif raw_status in {"clean", "degraded"}:
            status = raw_status
        else:
            status = "insufficient_history"
            reasons.append(f"funding_status:{raw_status}")
    total_buyers = common.int_or_none(evidence.get("total_buyers")) if evidence else None
    known_sources = common.int_or_none(evidence.get("known_buyers")) if evidence else None
    unknown_buyers = common.int_or_none(evidence.get("unknown_count")) if evidence else None
    top_count = common.int_or_none(evidence.get("top_funder_count")) if evidence else None
    known_rate = (
        known_sources / total_buyers
        if known_sources is not None and total_buyers not in (None, 0)
        else None
    )
    unknown_rate = (
        unknown_buyers / total_buyers
        if unknown_buyers is not None and total_buyers not in (None, 0)
        else None
    )
    top_share = top_count / total_buyers if top_count is not None and total_buyers not in (None, 0) else None
    event_funder_counts: Counter[str] = funding_events["funder_counts"]
    cluster_sizes = list(event_funder_counts.values())
    funding_hhi = hhi_from_counts(cluster_sizes)
    if funding_hhi is None and top_share is not None and status in {"clean", "degraded", "degraded_low_coverage"}:
        funding_hhi = top_share * top_share
    if status in {"unavailable", "missing_funding_lane", "insufficient_history"}:
        known_sources = None
        unknown_buyers = None
        known_rate = None
        unknown_rate = None
        top_share = None
        top_count = None
        funding_hhi = None
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "candidate_id": candidate_id,
        "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "pool_id": candidate.get("pool_id"),
        "fg_status": status,
        "fg_buyer_sample_count": total_buyers if status not in {"unavailable", "missing_funding_lane"} else None,
        "fg_known_source_count": known_sources,
        "fg_unknown_buyer_count": unknown_buyers,
        "fg_known_source_rate": known_rate,
        "fg_unknown_buyer_rate": unknown_rate,
        "fg_top_source_share": top_share,
        "fg_top_source_buyer_count": top_count,
        "fg_same_source_cluster_count": len(cluster_sizes) if cluster_sizes and status not in {"unavailable", "missing_funding_lane"} else None,
        "fg_same_source_cluster_max_size": max(cluster_sizes) if cluster_sizes and status not in {"unavailable", "missing_funding_lane"} else None,
        "fg_funding_hhi": funding_hhi,
        "fg_funded_within_5m_count": funding_events["funded_within_5m"] if funding_events["funded_within_5m"] else None,
        "fg_funded_within_15m_count": funding_events["funded_within_15m"] if funding_events["funded_within_15m"] else None,
        "fg_funded_within_60m_count": funding_events["funded_within_60m"] if funding_events["funded_within_60m"] else None,
        "fg_dev_to_buyer_link_count": funding_events["dev_links"] if funding_events["dev_links"] else None,
        "fg_context_reasons": reasons,
        "fg_unknown_is_safe": False,
    }


def build_context(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    candidates_path = args.candidate_universe or candidate_universe_path(root, args.scope)
    out = args.output or output_path(root, args.scope)
    manifest_out = args.manifest_output or manifest_path(root, args.scope)
    candidates = list(common.iter_json_objects(candidates_path))
    paths = coordination_paths(root, args.runtime_scope, args.coordination_glob)
    evidence_by_candidate = parse_coordination(paths)
    funding_events = parse_optional_funding_events(args.funding_events)
    system_transfers = parse_optional_funding_events(args.system_transfers)
    for candidate_id, rows in system_transfers.items():
        funding_events.setdefault(candidate_id, []).extend(rows)
    rows = [
        build_context_row(candidate, evidence_by_candidate, funding_events)
        for candidate in candidates
        if common.str_or_none(candidate.get("candidate_id"))
    ]
    common.write_jsonl(out, rows)
    status_counts = Counter(str(row.get("fg_status") or "unknown") for row in rows)
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": MANIFEST_ARTIFACT,
        "status": "PASS" if rows else "NO-GO",
        "fail_reasons": [] if rows else ["no_candidate_rows"],
        "scope": args.scope,
        "runtime_scope": args.runtime_scope,
        "offline_only": True,
        "changes_runtime": False,
        "changes_gatekeeper": False,
        "changes_execution": False,
        "changes_send_path": False,
        "candidate_rows": len(candidates),
        "rows_written": len(rows),
        "coordination_evidence_files": [str(path) for path in paths],
        "coordination_evidence_candidate_rows": sum(len(v) for v in evidence_by_candidate.values()),
        "status_counts": common.counter_dict(status_counts),
        "output": str(out),
    }
    common.write_json(manifest_out, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = build_context(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
