#!/usr/bin/env python3
"""Shadow V2 L2-E Gatekeeper coverage / denominator / starvation audit.

This script is offline-only metrology. It checks whether a Shadow V2 L2
research validation scope has a known event-level candidate denominator,
typed Gatekeeper reject reasons, and enough Gatekeeper checkpoint reach to
avoid declaring research readiness from a starved sample.
"""

from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


SCHEMA_VERSION = 1
ARTIFACT = "shadow_v2_l2_e_gatekeeper_coverage_denominator_audit"
DECISION_FILE_NAME = "gatekeeper_v2_decisions.jsonl"

VERDICT_DENOMINATOR_UNKNOWN = "BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN"
VERDICT_THRESHOLD_STARVATION = "BLOCKED_GATEKEEPER_THRESHOLD_STARVATION"
VERDICT_UNKNOWN_REASONS = "BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS"
VERDICT_COVERAGE_KNOWN = "GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN"

UNKNOWN_REASON_TOKENS = {
    "",
    "UNKNOWN",
    "GENERIC",
    "REJECT",
    "REJECT_OTHER",
    "UNKNOWN_REJECT",
    "UNSPECIFIED",
    "NONE",
    "NULL",
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--candidate-universe",
        type=Path,
        default=Path("datasets/selector/shadow-v2-l2-e/candidate_universe_v1.jsonl"),
        help="event-level candidate_universe_v1 JSONL denominator",
    )
    parser.add_argument(
        "--candidate-manifest",
        type=Path,
        default=None,
        help="optional candidate_universe_manifest_v1 JSON",
    )
    parser.add_argument(
        "--decision-jsonl",
        type=Path,
        action="append",
        default=[],
        help="Gatekeeper decision JSONL. May be passed multiple times.",
    )
    parser.add_argument(
        "--decision-root",
        type=Path,
        action="append",
        default=[],
        help="directory scanned recursively for gatekeeper_v2_decisions.jsonl",
    )
    parser.add_argument(
        "--summary-csv",
        type=Path,
        action="append",
        default=[],
        help="optional Shadow V2 summary CSV with research-candidate counters",
    )
    parser.add_argument("--output-csv", type=Path, default=None)
    parser.add_argument("--top-n", type=int, default=10)
    parser.add_argument("--pretty", action="store_true")
    return parser


def read_json(path: Path | None) -> dict[str, Any]:
    if path is None or not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as fh:
        payload = json.load(fh)
    return payload if isinstance(payload, dict) else {}


def read_summary_csv(paths: list[Path]) -> dict[str, str]:
    metrics: dict[str, str] = {}
    for path in paths:
        if not path.exists():
            continue
        with path.open("r", encoding="utf-8", newline="") as fh:
            reader = csv.DictReader(fh)
            if not reader.fieldnames or "metric" not in reader.fieldnames or "value" not in reader.fieldnames:
                continue
            for row in reader:
                metric = str(row.get("metric") or "").strip()
                if not metric or metric in metrics:
                    continue
                metrics[metric] = str(row.get("value") or "").strip()
    return metrics


def int_metric(metrics: dict[str, str], *names: str) -> int | None:
    for name in names:
        raw = metrics.get(name)
        if raw in (None, ""):
            continue
        try:
            return int(float(raw))
        except ValueError:
            continue
    return None


def candidate_key(row: dict[str, Any]) -> tuple[str | None, str | None]:
    base_mint = common.str_or_none(row.get("base_mint")) or common.str_or_none(row.get("mint"))
    pool_id = common.str_or_none(row.get("pool_id")) or common.str_or_none(row.get("bonding_curve"))
    return base_mint, pool_id


def build_candidate_indexes(rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_candidate_id: dict[str, dict[str, Any]] = {}
    by_exact: dict[str, dict[str, Any]] = {}
    by_mint_pool: dict[tuple[str, str], dict[str, Any]] = {}
    ambiguous_mint_pool: set[tuple[str, str]] = set()

    for row in rows:
        candidate_id = common.str_or_none(row.get("candidate_id"))
        if candidate_id and candidate_id not in by_candidate_id:
            by_candidate_id[candidate_id] = row
            by_exact[candidate_id] = row
        for field in ("join_key", "decision_context_join_key"):
            value = common.str_or_none(row.get(field))
            if value and value not in by_exact:
                by_exact[value] = row
        base_mint, pool_id = candidate_key(row)
        if base_mint and pool_id:
            key = (base_mint, pool_id)
            if key in by_mint_pool:
                ambiguous_mint_pool.add(key)
            else:
                by_mint_pool[key] = row

    for key in ambiguous_mint_pool:
        by_mint_pool.pop(key, None)

    return {
        "by_candidate_id": by_candidate_id,
        "by_exact": by_exact,
        "by_mint_pool": by_mint_pool,
        "ambiguous_mint_pool_count": len(ambiguous_mint_pool),
    }


def match_candidate(decision: dict[str, Any], indexes: dict[str, Any]) -> tuple[dict[str, Any] | None, str]:
    candidate_id = common.str_or_none(decision.get("candidate_id"))
    if candidate_id and candidate_id in indexes["by_candidate_id"]:
        return indexes["by_candidate_id"][candidate_id], "candidate_id"

    for field in ("join_key", "decision_context_join_key"):
        value = common.str_or_none(decision.get(field))
        if value and value in indexes["by_exact"]:
            return indexes["by_exact"][value], field

    base_mint, pool_id = candidate_key(decision)
    if base_mint and pool_id:
        candidate = indexes["by_mint_pool"].get((base_mint, pool_id))
        if candidate is not None:
            return candidate, "base_mint_pool_id"

    return None, "unmatched"


def decision_paths(args: argparse.Namespace) -> list[Path]:
    paths = [path for path in args.decision_jsonl if path.exists()]
    for root in args.decision_root:
        if root.exists():
            paths.extend(sorted(root.rglob(DECISION_FILE_NAME)))
    seen: set[str] = set()
    out: list[Path] = []
    for path in paths:
        key = str(path.resolve())
        if key not in seen:
            seen.add(key)
            out.append(path)
    return out


def is_buy(decision: dict[str, Any]) -> bool:
    if decision.get("decision_verdict_buy") is True:
        return True
    for field in ("verdict_type", "gatekeeper_verdict", "legacy_live_verdict_type"):
        value = common.str_or_none(decision.get(field))
        if value and value.upper() in {"BUY", "EARLY_BUY"}:
            return True
    return False


def raw_verdict(decision: dict[str, Any]) -> str:
    for field in ("verdict_type", "gatekeeper_verdict", "legacy_live_verdict_type"):
        value = common.str_or_none(decision.get(field))
        if value:
            return value.upper()
    return "BUY" if is_buy(decision) else "UNKNOWN"


def reason_key(decision: dict[str, Any]) -> str:
    for field in (
        "reason_code",
        "gatekeeper_first_kill_reason",
        "decision_reason",
        "hard_fail_reason",
        "hard_reject_reason",
        "terminal_reason",
        "verdict_reason",
    ):
        value = common.str_or_none(decision.get(field))
        if value:
            return value
    return ""


def decision_bucket(decision: dict[str, Any]) -> str:
    if is_buy(decision):
        return "BUY"
    verdict = raw_verdict(decision)
    reason = reason_key(decision).upper()
    combined = f"{verdict} {reason}"
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
    if "HARD_FAIL" in combined or verdict.startswith("HARD_FAIL"):
        return "REJECT_HARD_FAIL"
    if "REJECT" in combined:
        return "REJECT_OTHER"
    return "UNKNOWN"


def reason_is_unknown_or_generic(decision: dict[str, Any], bucket: str) -> bool:
    if bucket == "BUY":
        return False
    reason = reason_key(decision).strip().upper()
    if reason in UNKNOWN_REASON_TOKENS:
        return True
    return bucket == "UNKNOWN"


def status_is_eligible(row: dict[str, Any]) -> bool:
    status = common.str_or_none(row.get("candidate_universe_status"))
    if status == "ok":
        return True
    if status is None:
        return row.get("cohort_in_scope") is True and row.get("stream_completeness_ok") is True
    return False


def denominator_failures(
    candidate_path: Path,
    candidates: list[dict[str, Any]],
    eligible_count: int,
    manifest: dict[str, Any],
) -> list[str]:
    failures: list[str] = []
    if not candidate_path.exists():
        failures.append("candidate_universe_file_missing")
    if not candidates:
        failures.append("candidate_universe_empty")
    if eligible_count == 0:
        failures.append("eligible_denominator_zero")
    if manifest:
        status = str(manifest.get("status") or "").upper()
        invariant = str(manifest.get("denominator_invariant_status") or "").upper()
        decision_created = common.int_or_none(manifest.get("decision_logs_created_denominator_rows")) or 0
        decision_only = common.int_or_none(manifest.get("candidate_ids_from_decision_only")) or 0
        if status and status not in {"OK", "PASS"}:
            failures.append(f"candidate_manifest_status_{status}")
        if invariant and invariant != "PASS":
            failures.append(f"denominator_invariant_status_{invariant}")
        if decision_created != 0:
            failures.append("decision_logs_created_denominator_rows_nonzero")
        if decision_only != 0:
            failures.append("candidate_ids_from_decision_only_nonzero")
    return failures


def threshold_starvation_verdict(
    *,
    denominator_known: bool,
    checkpoint_reach_count: int,
    gatekeeper_decision_count: int,
    gatekeeper_buy_count: int,
    gatekeeper_reject_count: int,
    gatekeeper_timeout_count: int,
) -> str:
    if not denominator_known:
        return "NOT_EVALUATED_DENOMINATOR_UNKNOWN"
    if gatekeeper_decision_count == 0 or checkpoint_reach_count == 0:
        return VERDICT_THRESHOLD_STARVATION
    if gatekeeper_buy_count == 0 and gatekeeper_reject_count + gatekeeper_timeout_count == gatekeeper_decision_count:
        return VERDICT_THRESHOLD_STARVATION
    return "NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED"


def metric_rows(report: dict[str, Any]) -> list[dict[str, str]]:
    def row(metric: str, value: Any, notes: str = "") -> dict[str, str]:
        if isinstance(value, (dict, list)):
            rendered = json.dumps(value, sort_keys=True, separators=(",", ":"))
        elif value is None:
            rendered = ""
        else:
            rendered = str(value)
        return {"metric": metric, "value": rendered, "notes": notes}

    return [
        row("final_verdict", report["final_verdict"], "L2-E audit verdict only; not L2 research approval"),
        row("candidate_universe_count", report["candidate_universe_count"], "event-level denominator rows"),
        row("eligible_denominator_count", report["eligible_denominator_count"], "candidate_universe_status=ok rows"),
        row("candidate_universe_status_counts", report["candidate_universe_status_counts"]),
        row("denominator_contract_failures", report["denominator_contract_failures"]),
        row("gatekeeper_decision_path_count", report["gatekeeper_decision_path_count"]),
        row("gatekeeper_decision_count", report["gatekeeper_decision_count"]),
        row("gatekeeper_decision_joined_to_candidate_count", report["gatekeeper_decision_joined_to_candidate_count"]),
        row("gatekeeper_decision_unmatched_count", report["gatekeeper_decision_unmatched_count"]),
        row("gatekeeper_buy_count", report["gatekeeper_buy_count"]),
        row("gatekeeper_reject_count", report["gatekeeper_reject_count"]),
        row("gatekeeper_timeout_count", report["gatekeeper_timeout_count"]),
        row("gatekeeper_reject_reason_top_n", report["gatekeeper_reject_reason_top_n"], "typed reject reason distribution"),
        row("checkpoint_reach_count", report["checkpoint_reach_count"], "unique eligible candidates with joined Gatekeeper decision row"),
        row("entry_research_candidate_count", report["entry_research_candidate_count"]),
        row("exit_research_candidate_count", report["exit_research_candidate_count"]),
        row("research_candidate_roundtrip_count", report["research_candidate_roundtrip_count"], "explicit metric only; not inferred from entry/exit separately"),
        row("complete_executable_roundtrip_positions", report["complete_executable_roundtrip_positions"]),
        row("threshold_starvation_verdict", report["threshold_starvation_verdict"]),
        row("unknown_reason_count", report["unknown_reason_count"], "missing or generic non-BUY reason rows"),
        row("unknown_reason_samples", report["unknown_reason_samples"]),
        row("runtime_decision_behavior_changes", False, "audit-only"),
        row("runtime_evidence_schema_changes", False, "no Shadow V2 runtime schema change"),
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
        writer = csv.DictWriter(fh, fieldnames=["metric", "value", "notes"])
        writer.writeheader()
        writer.writerows(rows)


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    candidates = list(common.iter_json_objects(args.candidate_universe))
    manifest = read_json(args.candidate_manifest)
    status_counts = Counter(str(row.get("candidate_universe_status") or "missing") for row in candidates)
    eligible_candidates = [row for row in candidates if status_is_eligible(row)]
    eligible_candidate_ids = {
        common.str_or_none(row.get("candidate_id")) for row in eligible_candidates if common.str_or_none(row.get("candidate_id"))
    }
    indexes = build_candidate_indexes(candidates)

    paths = decision_paths(args)
    decision_count = 0
    joined_count = 0
    unmatched_count = 0
    gatekeeper_buy_count = 0
    gatekeeper_reject_count = 0
    gatekeeper_timeout_count = 0
    reason_counts: Counter[str] = Counter()
    bucket_counts: Counter[str] = Counter()
    join_method_counts: Counter[str] = Counter()
    checkpoint_reach_candidate_ids: set[str] = set()
    unknown_reason_samples: list[dict[str, Any]] = []
    unknown_reason_count = 0

    for path in paths:
        for decision in common.iter_json_objects(path):
            decision_count += 1
            bucket = decision_bucket(decision)
            bucket_counts[bucket] += 1
            if bucket == "BUY":
                gatekeeper_buy_count += 1
            elif bucket.startswith("TIMEOUT"):
                gatekeeper_timeout_count += 1
                reason_counts[reason_key(decision) or bucket] += 1
            else:
                gatekeeper_reject_count += 1
                reason_counts[reason_key(decision) or bucket] += 1

            if reason_is_unknown_or_generic(decision, bucket):
                unknown_reason_count += 1
                if len(unknown_reason_samples) < 10:
                    unknown_reason_samples.append(
                        {
                            "candidate_id": decision.get("candidate_id"),
                            "pool_id": decision.get("pool_id"),
                            "base_mint": decision.get("base_mint") or decision.get("mint"),
                            "verdict_type": raw_verdict(decision),
                            "bucket": bucket,
                        }
                    )

            candidate, method = match_candidate(decision, indexes)
            join_method_counts[method] += 1
            if candidate is None:
                unmatched_count += 1
                continue
            joined_count += 1
            candidate_id = common.str_or_none(candidate.get("candidate_id"))
            if candidate_id and (not eligible_candidate_ids or candidate_id in eligible_candidate_ids):
                checkpoint_reach_candidate_ids.add(candidate_id)

    summary_metrics = read_summary_csv(args.summary_csv)
    entry_research_candidate_count = int_metric(
        summary_metrics,
        "entry_research_candidate_count",
        "entry_execution_label_grade_RESEARCH_CANDIDATE_count",
        "execution_label_grade_RESEARCH_CANDIDATE_count",
    )
    exit_research_candidate_count = int_metric(
        summary_metrics,
        "exit_research_candidate_count",
        "exit_execution_label_grade_RESEARCH_CANDIDATE_count",
    )
    research_candidate_roundtrip_count = int_metric(summary_metrics, "research_candidate_roundtrip_count")
    complete_executable_roundtrip_positions = int_metric(
        summary_metrics, "complete_executable_roundtrip_positions"
    )

    denominator_contract_failures = denominator_failures(
        args.candidate_universe,
        candidates,
        len(eligible_candidates),
        manifest,
    )
    denominator_known = not denominator_contract_failures
    starvation = threshold_starvation_verdict(
        denominator_known=denominator_known,
        checkpoint_reach_count=len(checkpoint_reach_candidate_ids),
        gatekeeper_decision_count=decision_count,
        gatekeeper_buy_count=gatekeeper_buy_count,
        gatekeeper_reject_count=gatekeeper_reject_count,
        gatekeeper_timeout_count=gatekeeper_timeout_count,
    )

    if not denominator_known:
        final_verdict = VERDICT_DENOMINATOR_UNKNOWN
        next_stage = "repair_candidate_universe_denominator_before_l2_f"
    elif unknown_reason_count > 0:
        final_verdict = VERDICT_UNKNOWN_REASONS
        next_stage = "repair_gatekeeper_reason_taxonomy_before_l2_f"
    elif starvation == VERDICT_THRESHOLD_STARVATION:
        final_verdict = VERDICT_THRESHOLD_STARVATION
        next_stage = "separate_policy_or_observation_starvation_review_no_threshold_tuning_here"
    else:
        final_verdict = VERDICT_COVERAGE_KNOWN
        next_stage = "L2-F_DEDICATED_RESEARCH_VALIDATION_RUN_AFTER_L2_D_PASS"

    report = {
        "artifact": ARTIFACT,
        "schema_version": SCHEMA_VERSION,
        "final_verdict": final_verdict,
        "candidate_universe_path": str(args.candidate_universe),
        "candidate_manifest_path": str(args.candidate_manifest) if args.candidate_manifest else None,
        "candidate_universe_count": len(candidates),
        "eligible_denominator_count": len(eligible_candidates),
        "candidate_universe_status_counts": common.counter_dict(status_counts),
        "denominator_contract_failures": denominator_contract_failures,
        "candidate_manifest_status": manifest.get("status") if manifest else None,
        "candidate_manifest_denominator_invariant_status": manifest.get("denominator_invariant_status") if manifest else None,
        "candidate_manifest_decision_logs_created_denominator_rows": manifest.get("decision_logs_created_denominator_rows") if manifest else None,
        "gatekeeper_decision_paths": [str(path) for path in paths],
        "gatekeeper_decision_path_count": len(paths),
        "gatekeeper_decision_count": decision_count,
        "gatekeeper_decision_joined_to_candidate_count": joined_count,
        "gatekeeper_decision_unmatched_count": unmatched_count,
        "gatekeeper_join_method_counts": common.counter_dict(join_method_counts),
        "gatekeeper_decision_bucket_counts": common.counter_dict(bucket_counts),
        "gatekeeper_buy_count": gatekeeper_buy_count,
        "gatekeeper_reject_count": gatekeeper_reject_count,
        "gatekeeper_timeout_count": gatekeeper_timeout_count,
        "gatekeeper_reject_reason_top_n": [
            {"reason": reason, "count": count}
            for reason, count in reason_counts.most_common(max(0, args.top_n))
        ],
        "checkpoint_reach_count": len(checkpoint_reach_candidate_ids),
        "checkpoint_reach_definition": "unique eligible candidate_id with a joined gatekeeper_v2_decisions row",
        "entry_research_candidate_count": entry_research_candidate_count,
        "exit_research_candidate_count": exit_research_candidate_count,
        "research_candidate_roundtrip_count": research_candidate_roundtrip_count,
        "complete_executable_roundtrip_positions": complete_executable_roundtrip_positions,
        "threshold_starvation_verdict": starvation,
        "unknown_reason_count": unknown_reason_count,
        "unknown_reason_samples": unknown_reason_samples,
        "summary_csv_paths": [str(path) for path in args.summary_csv],
        "runtime_decision_behavior_changes": False,
        "runtime_evidence_schema_changes": False,
        "approval_flags": {
            "runtime_approval": False,
            "research_grade": False,
            "live_equivalence": False,
            "strategy_research_unblocked": False,
            "shadow_close_only": False,
            "active_close": False,
        },
        "non_goals": [
            "no_threshold_tuning",
            "no_gatekeeper_policy_change",
            "no_buy_reject_change",
            "no_selector_runtime_change",
            "no_tx_jito_live_path_change",
            "no_l2_research_grade_grant",
        ],
        "recommended_next_stage": next_stage,
    }

    if args.output_csv:
        write_summary_csv(args.output_csv, metric_rows(report))

    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
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
