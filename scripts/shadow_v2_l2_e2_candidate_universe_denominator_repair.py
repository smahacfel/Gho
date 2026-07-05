#!/usr/bin/env python3
"""Shadow V2 L2-E2 candidate universe denominator repair.

This is an offline-only orchestration layer. It produces the event-level
candidate_universe_v1 denominator from candidate observations, attaches
Gatekeeper decision JSONL only as context, and then runs the L2-E denominator
coverage audit against the produced artifacts.
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
from typing import Any

import build_selector_candidate_universe as candidate_builder
import shadow_v2_gatekeeper_coverage_denominator_audit as l2e_audit


SCHEMA_VERSION = 1
ARTIFACT = "shadow_v2_l2_e2_candidate_universe_denominator_repair"

VERDICT_READY = "CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E"
VERDICT_SOURCE_MISSING = "BLOCKED_CANDIDATE_UNIVERSE_SOURCE_MISSING"
VERDICT_DECISION_JOIN_MISSING = "BLOCKED_GATEKEEPER_DECISION_JOIN_MISSING"
VERDICT_UNKNOWN_REASONS = "BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS"
VERDICT_THRESHOLD_STARVATION = "BLOCKED_GATEKEEPER_THRESHOLD_STARVATION"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--scope", default="shadow-v2-l2-e2")
    parser.add_argument("--events", type=Path, action="append", default=[], help="event-level JSONL")
    parser.add_argument(
        "--events-root",
        type=Path,
        action="append",
        default=[],
        help="directory containing event-level JSONL observations",
    )
    parser.add_argument(
        "--events-glob",
        default="*.jsonl",
        help="glob used for each --events-root; default is non-recursive *.jsonl",
    )
    parser.add_argument(
        "--decision-jsonl",
        type=Path,
        action="append",
        default=[],
        help="Gatekeeper decision JSONL used as context only",
    )
    parser.add_argument(
        "--decision-root",
        type=Path,
        action="append",
        default=[],
        help="directory scanned recursively for gatekeeper_v2_decisions.jsonl",
    )
    parser.add_argument("--summary-csv", type=Path, action="append", default=[])
    parser.add_argument("--candidate-universe-output", type=Path, default=None)
    parser.add_argument("--candidate-manifest-output", type=Path, default=None)
    parser.add_argument("--output-csv", type=Path, default=None)
    parser.add_argument("--output-json", type=Path, default=None)
    parser.add_argument("--window-start-ms", type=int)
    parser.add_argument("--window-end-ms", type=int)
    parser.add_argument("--top-n", type=int, default=10)
    parser.add_argument("--pretty", action="store_true")
    return parser


def dedupe_paths(paths: list[Path]) -> list[Path]:
    seen: set[str] = set()
    out: list[Path] = []
    for path in paths:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        out.append(path)
    return out


def collect_event_paths(args: argparse.Namespace) -> list[Path]:
    paths = [path for path in args.events if path.exists()]
    for root in args.events_root:
        if not root.exists():
            continue
        paths.extend(sorted(path for path in root.glob(args.events_glob) if path.is_file()))
    return dedupe_paths(paths)


def collect_decision_paths(args: argparse.Namespace) -> list[Path]:
    paths = [path for path in args.decision_jsonl if path.exists()]
    for root in args.decision_root:
        if not root.exists():
            continue
        paths.extend(sorted(root.rglob(l2e_audit.DECISION_FILE_NAME)))
    return dedupe_paths(paths)


def default_candidate_output(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "candidate_universe_v1.jsonl"


def default_candidate_manifest(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / "candidate_universe_manifest_v1.json"


def default_summary_csv(root: Path) -> Path:
    return root / "reports" / "selector" / "shadow_v2_l2_e2_candidate_universe_denominator_summary.csv"


def l2e_args(
    *,
    candidate_universe: Path,
    candidate_manifest: Path,
    decision_paths: list[Path],
    decision_roots: list[Path],
    summary_csv: list[Path],
    top_n: int,
) -> argparse.Namespace:
    return argparse.Namespace(
        candidate_universe=candidate_universe,
        candidate_manifest=candidate_manifest,
        decision_jsonl=decision_paths,
        decision_root=decision_roots,
        summary_csv=summary_csv,
        output_csv=None,
        top_n=top_n,
        pretty=False,
    )


def l2e2_verdict(candidate_manifest: dict[str, Any], l2e_report: dict[str, Any]) -> str:
    manifest_status = str(candidate_manifest.get("status") or "").upper()
    candidate_count = int(l2e_report.get("candidate_universe_count") or 0)
    eligible_count = int(l2e_report.get("eligible_denominator_count") or 0)
    if manifest_status not in {"OK", "PASS"} or candidate_count == 0 or eligible_count == 0:
        return VERDICT_SOURCE_MISSING

    decision_count = int(l2e_report.get("gatekeeper_decision_count") or 0)
    joined_count = int(l2e_report.get("gatekeeper_decision_joined_to_candidate_count") or 0)
    checkpoint_reach_count = int(l2e_report.get("checkpoint_reach_count") or 0)
    if decision_count == 0 or joined_count == 0 or checkpoint_reach_count == 0:
        return VERDICT_DECISION_JOIN_MISSING

    l2e_verdict = str(l2e_report.get("final_verdict") or "")
    if l2e_verdict == l2e_audit.VERDICT_UNKNOWN_REASONS:
        return VERDICT_UNKNOWN_REASONS
    if l2e_verdict == l2e_audit.VERDICT_THRESHOLD_STARVATION:
        return VERDICT_THRESHOLD_STARVATION
    if l2e_verdict == l2e_audit.VERDICT_COVERAGE_KNOWN:
        return VERDICT_READY
    return VERDICT_SOURCE_MISSING


def next_stage(verdict: str) -> str:
    if verdict == VERDICT_READY:
        return "return_to_L2_E_gate_then_L2_F_only_after_temporal_density_and_research_gates"
    if verdict == VERDICT_SOURCE_MISSING:
        return "restore_or_produce_event_level_candidate_observation_artifacts"
    if verdict == VERDICT_DECISION_JOIN_MISSING:
        return "provide_gatekeeper_v2_decisions_jsonl_or_explicit_decision_root_scope"
    if verdict == VERDICT_UNKNOWN_REASONS:
        return "repair_gatekeeper_reason_taxonomy_before_l2_f"
    if verdict == VERDICT_THRESHOLD_STARVATION:
        return "separate_policy_or_observation_starvation_review_no_threshold_tuning_here"
    return "manual_review_required"


def metric_rows(report: dict[str, Any]) -> list[dict[str, str]]:
    def render(value: Any) -> str:
        if value is None:
            return ""
        if isinstance(value, (dict, list)):
            return json.dumps(value, sort_keys=True, separators=(",", ":"))
        return str(value)

    def row(metric: str, value: Any, notes: str = "") -> dict[str, str]:
        return {"metric": metric, "value": render(value), "notes": notes}

    l2e = report["l2_e_audit"]
    manifest = report["candidate_universe_manifest"]
    return [
        row("final_verdict", report["final_verdict"], "L2-E2 verdict only; not L2 approval"),
        row("l2_e_verdict", l2e.get("final_verdict"), "underlying L2-E audit verdict"),
        row("candidate_universe_v1", report["candidate_universe_path"]),
        row("candidate_universe_manifest_v1", report["candidate_manifest_path"]),
        row("gatekeeper_decision_path_count", l2e.get("gatekeeper_decision_path_count")),
        row("gatekeeper_decision_paths", l2e.get("gatekeeper_decision_paths")),
        row("event_source_path_count", len(report["event_source_paths"])),
        row("candidate_universe_count", l2e.get("candidate_universe_count")),
        row("eligible_denominator_count", l2e.get("eligible_denominator_count")),
        row("denominator_invariant_status", manifest.get("denominator_invariant_status")),
        row("decision_logs_created_denominator_rows", manifest.get("decision_logs_created_denominator_rows")),
        row("candidate_ids_from_decision_only", manifest.get("candidate_ids_from_decision_only")),
        row("decision_only_rows_skipped", manifest.get("decision_only_rows_skipped")),
        row("decision_context_rows_joined", manifest.get("decision_context_rows_joined")),
        row("gatekeeper_decision_count", l2e.get("gatekeeper_decision_count")),
        row("gatekeeper_decision_joined_to_candidate_count", l2e.get("gatekeeper_decision_joined_to_candidate_count")),
        row("gatekeeper_decision_unmatched_count", l2e.get("gatekeeper_decision_unmatched_count")),
        row("checkpoint_reach_count", l2e.get("checkpoint_reach_count")),
        row("gatekeeper_buy_count", l2e.get("gatekeeper_buy_count")),
        row("gatekeeper_reject_count", l2e.get("gatekeeper_reject_count")),
        row("gatekeeper_timeout_count", l2e.get("gatekeeper_timeout_count")),
        row("unknown_reason_count", l2e.get("unknown_reason_count")),
        row("threshold_starvation_verdict", l2e.get("threshold_starvation_verdict")),
        row("runtime_decision_behavior_changes", False, "offline-only"),
        row("runtime_evidence_schema_changes", False, "no runtime schema change"),
        row("runtime_approval", False, "not granted"),
        row("research_grade", False, "not granted"),
        row("live_equivalence", False, "not granted"),
        row("strategy_research_unblocked", False, "not granted"),
        row("shadow_close_only", False, "not enabled"),
        row("active_close", False, "not enabled"),
        row("recommended_next_stage", report["recommended_next_stage"]),
    ]


def write_summary_csv(path: Path, rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=["metric", "value", "notes"], lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def run(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root
    candidate_universe = args.candidate_universe_output or default_candidate_output(root, args.scope)
    candidate_manifest = args.candidate_manifest_output or default_candidate_manifest(root, args.scope)
    output_csv = args.output_csv or default_summary_csv(root)
    event_paths = collect_event_paths(args)
    decision_paths = collect_decision_paths(args)

    candidate_manifest_payload = candidate_builder.run(
        argparse.Namespace(
            events=event_paths,
            decisions=decision_paths,
            output=candidate_universe,
            manifest_output=candidate_manifest,
            allow_degraded_events=False,
            allow_decision_universe=False,
            allow_incomplete_universe=False,
            window_start_ms=args.window_start_ms,
            window_end_ms=args.window_end_ms,
            json=False,
        )
    )
    l2e_report = l2e_audit.build_report(
        l2e_args(
            candidate_universe=candidate_universe,
            candidate_manifest=candidate_manifest,
            decision_paths=decision_paths,
            decision_roots=args.decision_root,
            summary_csv=args.summary_csv,
            top_n=args.top_n,
        )
    )
    verdict = l2e2_verdict(candidate_manifest_payload, l2e_report)
    report = {
        "artifact": ARTIFACT,
        "schema_version": SCHEMA_VERSION,
        "final_verdict": verdict,
        "candidate_universe_path": str(candidate_universe),
        "candidate_manifest_path": str(candidate_manifest),
        "event_source_paths": [str(path) for path in event_paths],
        "decision_jsonl_paths": [str(path) for path in decision_paths],
        "decision_root_scope": [str(path) for path in args.decision_root],
        "candidate_universe_manifest": candidate_manifest_payload,
        "l2_e_audit": l2e_report,
        "denominator_contract": {
            "candidate_universe_source": "event_level_candidate_observations_only",
            "decision_logs": "context_only_not_denominator",
            "decision_logs_created_denominator_rows_required": 0,
            "candidate_ids_from_decision_only_required": 0,
        },
        "non_goals": [
            "no_threshold_tuning",
            "no_gatekeeper_policy_change",
            "no_buy_reject_change",
            "no_selector_runtime_change",
            "no_tx_jito_live_path_change",
            "no_l2_f_research_validation_run",
            "no_l2_research_grade_grant",
        ],
        "approval_flags": {
            "runtime_approval": False,
            "research_grade": False,
            "live_equivalence": False,
            "strategy_research_unblocked": False,
            "shadow_close_only": False,
            "active_close": False,
        },
        "recommended_next_stage": next_stage(verdict),
    }
    write_summary_csv(output_csv, metric_rows(report))
    report["summary_csv_path"] = str(output_csv)
    if args.output_json:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        args.output_json.write_text(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = run(args)
    print(
        json.dumps(
            report,
            ensure_ascii=False,
            indent=2 if args.pretty else None,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
