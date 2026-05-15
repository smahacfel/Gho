#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import v3_shadow_report


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"
TARGET_REASON = "REJECT_V3_MANIPULATION_CONTRADICTION"
NUMERIC_FIELDS = (
    "contradiction_score",
    "bundle_suspicion_ratio",
    "dev_volume_ratio",
    "same_ms_tx_ratio",
    "top3_volume_pct",
    "hhi",
    "fee_topology_diversity_index",
    "spend_fraction_divergence",
    "max_tx_per_signer",
    "signer_cross_pool_velocity",
)
BOOLEAN_FIELDS = (
    "timing_bundle_concentration",
    "early_top3_concentration",
    "fixed_size_or_ramping_pattern",
    "high_buy_pressure_with_high_top3",
    "momentum_without_broadening",
    "volume_spike_without_new_signers",
    "dev_has_sold",
    "sybil_evidence_degraded",
    "high_bundle_suspicion_ratio",
    "high_dev_concentration",
    "high_same_ms_tx_ratio",
    "high_top3_volume_pct",
    "high_hhi",
    "high_signer_concentration",
)
EVIDENCE_GROUPS = (
    "manipulation_contradiction",
    "manipulation",
    "sybil",
    "fsc",
    "organic_broadening",
    "pdd_sequence",
    "tx_segments",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="P3.1 targeted offline audit for REJECT_V3_MANIPULATION_CONTRADICTION."
    )
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decisions-log", type=Path)
    parser.add_argument(
        "--compare-decisions-log",
        action="append",
        default=[],
        type=Path,
        help="Historical V3 JSONL to compare against the primary P1 baseline.",
    )
    parser.add_argument("--sample-limit", type=int, default=3)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def resolve_primary_log(config_path: Path, decisions_log: Path | None) -> Path:
    resolved = v3_shadow_report.resolve_decisions_log(config_path, decisions_log)
    if decisions_log is not None and not resolved.exists():
        raise FileNotFoundError(f"explicit decisions log not found: {resolved}")
    return resolved


def resolve_compare_log(compare_path: Path) -> Path:
    resolved = compare_path if compare_path.is_absolute() else (REPO_ROOT / compare_path).resolve()
    if not resolved.exists():
        raise FileNotFoundError(f"compare decisions log not found: {resolved}")
    return resolved


def load_rows(path: Path) -> tuple[list[dict[str, Any]], int]:
    return v3_shadow_report.load_jsonl(path)


def has_v3(row: dict[str, Any]) -> bool:
    return v3_shadow_report.has_v3_fields(row)


def target_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [row for row in rows if row.get("v3_shadow_reason_code") == TARGET_REASON]


def manipulation_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = row.get("v3_shadow_manipulation_contradictions") or row.get(
        "v3_manipulation_contradictions"
    )
    return value if isinstance(value, dict) else {}


def evidence_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = row.get("v3_shadow_evidence_status") or row.get("v3_evidence_status")
    return value if isinstance(value, dict) else {}


def actionability_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = row.get("v3_actionability")
    return value if isinstance(value, dict) else {}


def active_reason(row: dict[str, Any]) -> str:
    return str(row.get("reason_code") or row.get("verdict_type") or "missing")


def profile_name(row: dict[str, Any]) -> str:
    actionability = actionability_payload(row)
    profile = actionability.get("profile")
    return str(profile or "normal")


def risk_profile(row: dict[str, Any]) -> dict[str, Any]:
    thresholds = row.get("v3_stage_thresholds")
    if not isinstance(thresholds, dict):
        return {}
    legacy_risk = thresholds.get("risk")
    if isinstance(legacy_risk, dict):
        return legacy_risk
    profiles = thresholds.get("profiles")
    if not isinstance(profiles, dict):
        return {}
    profile = profiles.get(profile_name(row)) or profiles.get("normal")
    if not isinstance(profile, dict):
        return {}
    risk = profile.get("risk")
    return risk if isinstance(risk, dict) else {}


def gt(payload: dict[str, Any], profile: dict[str, Any], field: str, threshold: str) -> bool:
    value = payload.get(field)
    limit = profile.get(threshold)
    return isinstance(value, (int, float)) and isinstance(limit, (int, float)) and value > limit


def hard_risk_triggers(row: dict[str, Any]) -> list[str]:
    payload = manipulation_payload(row)
    profile = risk_profile(row)
    if not payload:
        return ["manipulation_payload_missing"]
    if not profile:
        return ["thresholds_missing"]
    triggers: list[str] = []

    if profile.get("reject_on_dev_sell") and payload.get("dev_has_sold"):
        triggers.append("dev_sell")
    for field in (
        "high_same_ms_tx_ratio",
        "high_bundle_suspicion_ratio",
        "high_top3_volume_pct",
        "high_hhi",
        "high_signer_concentration",
        "high_dev_concentration",
    ):
        if payload.get(field) is True:
            triggers.append(f"{field}_flag")

    threshold_pairs = (
        ("same_ms_tx_ratio", "hard_fail_same_ms_tx_ratio", "same_ms_tx_ratio_threshold"),
        (
            "bundle_suspicion_ratio",
            "hard_fail_same_ms_tx_ratio",
            "bundle_suspicion_ratio_gt_hard_fail_same_ms_tx_ratio",
        ),
        ("top3_volume_pct", "hard_fail_top3_volume_pct", "top3_volume_pct_threshold"),
        ("hhi", "hard_fail_hhi", "hhi_threshold"),
        ("max_tx_per_signer", "max_tx_per_signer", "max_tx_per_signer_threshold"),
        ("dev_volume_ratio", "max_dev_volume_ratio", "dev_volume_ratio_threshold"),
        (
            "signer_cross_pool_velocity",
            "max_signer_cross_pool_velocity",
            "signer_cross_pool_velocity_threshold",
        ),
        (
            "funding_source_concentration",
            "max_funding_source_concentration",
            "funding_source_concentration_threshold",
        ),
    )
    for field, threshold, label in threshold_pairs:
        if gt(payload, profile, field, threshold):
            triggers.append(label)
    return triggers


def numeric_summary(rows: list[dict[str, Any]], field: str) -> dict[str, Any]:
    values: list[float] = []
    missing = 0
    for row in rows:
        value = manipulation_payload(row).get(field)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            values.append(float(value))
        else:
            missing += 1
    if not values:
        return {"count": 0, "missing": missing}
    values.sort()
    p90_index = min(len(values) - 1, math.ceil(0.9 * (len(values) - 1)))
    return {
        "count": len(values),
        "missing": missing,
        "min": round(values[0], 6),
        "p50": round(values[len(values) // 2], 6),
        "p90": round(values[p90_index], 6),
        "max": round(values[-1], 6),
        "avg": round(sum(values) / len(values), 6),
    }


def score_bucket(value: Any) -> str:
    return v3_shadow_report.confidence_bucket(value)


def component_breakdown(rows: list[dict[str, Any]]) -> dict[str, Any]:
    risk_statuses: Counter[str] = Counter()
    opportunity_statuses: Counter[str] = Counter()
    cap_reasons: Counter[str] = Counter()
    risk_penalty_buckets: Counter[str] = Counter()
    raw_confidence_buckets: Counter[str] = Counter()
    final_confidence_buckets: Counter[str] = Counter()
    opportunity_score_buckets: Counter[str] = Counter()

    for row in rows:
        scores = row.get("v3_component_scores")
        if not isinstance(scores, dict):
            scores = {}
        risk = scores.get("risk") if isinstance(scores.get("risk"), dict) else {}
        opportunity = scores.get("opportunity") if isinstance(scores.get("opportunity"), dict) else {}
        confidence = scores.get("confidence") if isinstance(scores.get("confidence"), dict) else {}
        risk_statuses[str(risk.get("status") or row.get("v3_shadow_risk_status") or "missing")] += 1
        opportunity_statuses[
            str(opportunity.get("status") or row.get("v3_shadow_opportunity_status") or "missing")
        ] += 1
        for reason in confidence.get("cap_reasons") or row.get("v3_shadow_confidence_cap_reasons") or []:
            cap_reasons[str(reason)] += 1
        risk_penalty_buckets[score_bucket(risk.get("penalty", row.get("v3_shadow_risk_penalty")))] += 1
        raw_confidence_buckets[
            score_bucket(confidence.get("raw", row.get("v3_shadow_confidence_raw")))
        ] += 1
        final_confidence_buckets[
            score_bucket(confidence.get("final", row.get("v3_shadow_confidence_final")))
        ] += 1
        opportunity_score_buckets[
            score_bucket(opportunity.get("score", row.get("v3_shadow_opportunity_score")))
        ] += 1

    return {
        "risk_statuses": counter_dict(risk_statuses),
        "opportunity_statuses": counter_dict(opportunity_statuses),
        "confidence_cap_reasons": counter_dict(cap_reasons),
        "risk_penalty_buckets": counter_dict(risk_penalty_buckets),
        "raw_confidence_buckets": counter_dict(raw_confidence_buckets),
        "final_confidence_buckets": counter_dict(final_confidence_buckets),
        "opportunity_score_buckets": counter_dict(opportunity_score_buckets),
    }


def samples_by_trigger_combo(rows: list[dict[str, Any]], sample_limit: int) -> dict[str, list[dict[str, Any]]]:
    samples: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        triggers = hard_risk_triggers(row)
        combo = "+".join(triggers) if triggers else "none"
        if len(samples[combo]) >= sample_limit:
            continue
        payload = manipulation_payload(row)
        evidence = evidence_payload(row)
        actionability = actionability_payload(row).get("groups", {})
        manipulation_action = (
            actionability.get("manipulation_contradiction", {})
            if isinstance(actionability, dict)
            else {}
        )
        samples[combo].append(
            {
                "pool_id": row.get("pool_id"),
                "ab_record_id": row.get("ab_record_id"),
                "active_reason_code": active_reason(row),
                "v3_reason_code": row.get("v3_shadow_reason_code"),
                "hard_risk_triggers": triggers,
                "manipulation_reasons": payload.get("reasons") or [],
                "manipulation_status": payload.get("status"),
                "manipulation_contradiction_evidence_status": (
                    evidence.get("manipulation_contradiction", {}).get("status")
                    if isinstance(evidence.get("manipulation_contradiction"), dict)
                    else None
                ),
                "manipulation_contradiction_actionability": manipulation_action.get("actionability"),
                "dev_volume_ratio": payload.get("dev_volume_ratio"),
                "top3_volume_pct": payload.get("top3_volume_pct"),
                "hhi": payload.get("hhi"),
                "bundle_suspicion_ratio": payload.get("bundle_suspicion_ratio"),
                "same_ms_tx_ratio": payload.get("same_ms_tx_ratio"),
            }
        )
    return dict(sorted(samples.items()))


def dataset_summary(
    name: str,
    path: Path,
    rows: list[dict[str, Any]],
    bad_rows: int,
    sample_limit: int,
) -> dict[str, Any]:
    v3_rows = [row for row in rows if has_v3(row)]
    target = target_rows(v3_rows)
    target_count = len(target)
    evidence_statuses: dict[str, Counter[str]] = {group: Counter() for group in EVIDENCE_GROUPS}
    evidence_degraded_reasons: Counter[str] = Counter()
    actionability: dict[str, Counter[str]] = {"manipulation_contradiction": Counter(), "risk_stage": Counter()}
    reason_lists: Counter[str] = Counter()
    boolean_flags: Counter[str] = Counter()
    hard_triggers: Counter[str] = Counter()
    hard_trigger_combos: Counter[str] = Counter()

    for row in target:
        payload = manipulation_payload(row)
        for reason in payload.get("reasons") or []:
            reason_lists[str(reason)] += 1
        for field in BOOLEAN_FIELDS:
            if payload.get(field) is True:
                boolean_flags[field] += 1
        triggers = hard_risk_triggers(row)
        hard_triggers.update(triggers or ["none"])
        hard_trigger_combos.update(["+".join(triggers) if triggers else "none"])

        evidence = evidence_payload(row)
        for group in EVIDENCE_GROUPS:
            item = evidence.get(group)
            if isinstance(item, dict):
                status = str(item.get("status") or "missing")
                evidence_statuses[group][status] += 1
                for degraded_reason in item.get("degraded_reasons") or item.get("unavailable_reasons") or []:
                    evidence_degraded_reasons[f"{group}:{degraded_reason}"] += 1
            else:
                evidence_statuses[group]["missing"] += 1

        action = actionability_payload(row)
        groups = action.get("groups") if isinstance(action.get("groups"), dict) else {}
        manipulation_action = groups.get("manipulation_contradiction", {}) if isinstance(groups, dict) else {}
        actionability["manipulation_contradiction"][
            str(manipulation_action.get("actionability") or "missing")
        ] += 1
        stages = action.get("stages") if isinstance(action.get("stages"), dict) else {}
        actionability["risk_stage"][str(stages.get("risk") or "missing")] += 1

    clean_count = evidence_statuses["manipulation_contradiction"].get("clean", 0)
    degraded_count = evidence_statuses["manipulation_contradiction"].get("degraded", 0)
    return {
        "name": name,
        "path": str(path),
        "rows": len(rows),
        "bad_rows": bad_rows,
        "v3_rows": len(v3_rows),
        "target_reason": TARGET_REASON,
        "target_rows": target_count,
        "target_share_of_v3_rows": round(target_count / len(v3_rows), 6) if v3_rows else 0.0,
        "decision_planes": counter_dict(Counter(str(row.get("decision_plane") or "missing") for row in target)),
        "missing_ab_record_id": sum(1 for row in target if not row.get("ab_record_id")),
        "policy_hashes": counter_dict(Counter(str(row.get("v3_policy_config_hash") or "missing") for row in target)),
        "active_reason_codes": counter_dict(Counter(active_reason(row) for row in target)),
        "v3_stages": counter_dict(Counter(str(row.get("v3_shadow_stage") or "missing") for row in target)),
        "manipulation_statuses": counter_dict(
            Counter(str(manipulation_payload(row).get("status") or "missing") for row in target)
        ),
        "manipulation_contradiction_clean_vs_degraded": {
            "clean": clean_count,
            "degraded": degraded_count,
            "other": target_count - clean_count - degraded_count,
        },
        "evidence_statuses": {
            group: counter_dict(counter) for group, counter in sorted(evidence_statuses.items())
        },
        "evidence_degraded_reasons": counter_dict(evidence_degraded_reasons),
        "actionability": {key: counter_dict(value) for key, value in sorted(actionability.items())},
        "manipulation_reasons": counter_dict(reason_lists),
        "boolean_flags_true": counter_dict(boolean_flags),
        "hard_risk_triggers": counter_dict(hard_triggers),
        "hard_risk_trigger_combos": counter_dict(hard_trigger_combos),
        "numeric_summaries": {field: numeric_summary(target, field) for field in NUMERIC_FIELDS},
        "component_breakdown": component_breakdown(target),
        "samples_by_hard_risk_trigger_combo": samples_by_trigger_combo(target, sample_limit),
    }


def cross_dataset_summary(datasets: list[dict[str, Any]]) -> dict[str, Any]:
    def dominant(counter_dict_value: dict[str, int]) -> tuple[str, int]:
        if not counter_dict_value:
            return ("missing", 0)
        return max(counter_dict_value.items(), key=lambda item: (item[1], item[0]))

    return {
        "target_rows_by_dataset": {item["name"]: item["target_rows"] for item in datasets},
        "target_share_by_dataset": {
            item["name"]: item["target_share_of_v3_rows"] for item in datasets
        },
        "dominant_active_reason_by_dataset": {
            item["name"]: dominant(item["active_reason_codes"])
            for item in datasets
        },
        "dominant_hard_trigger_by_dataset": {
            item["name"]: dominant(item["hard_risk_triggers"])
            for item in datasets
        },
    }


def certification(primary: dict[str, Any]) -> dict[str, Any]:
    target_rows_count = primary["target_rows"]
    hard_triggered = target_rows_count - primary["hard_risk_triggers"].get("none", 0)
    degraded = primary["manipulation_contradiction_clean_vs_degraded"]["degraded"]
    insufficient: list[str] = ["hash_only_no_counterfactual_replay"]
    if degraded:
        insufficient.append("dominant_bucket_has_degraded_manipulation_contradiction_evidence")
    if target_rows_count and hard_triggered / target_rows_count >= 0.9:
        interpretation = "hard_risk_signal_present_but_needs_replay"
    else:
        interpretation = "likely_overbroad_or_underexplained"
        insufficient.append("hard_risk_trigger_coverage_low")
    return {
        "p3_1_status": "keep_blocked_needs_full_replay",
        "promotion_ready": False,
        "no_p2_promotion": True,
        "interpretation": interpretation,
        "target_rows": target_rows_count,
        "hard_risk_trigger_coverage": round(hard_triggered / target_rows_count, 6)
        if target_rows_count
        else 0.0,
        "degraded_manipulation_contradiction_evidence_rows": degraded,
        "insufficient_evidence_gates": sorted(set(insufficient)),
        "recommended_next": [
            "targeted_manual_audit_of_dev_volume_top3_hhi_trigger_examples",
            "full_replay_payload_design_before_any_counterfactual_ablation",
            "multi_run_stability_collection_on_same_v3_policy_config_hash",
        ],
    }


def build_report(
    config_path: Path,
    decisions_log: Path | None = None,
    compare_logs: list[Path] | None = None,
    sample_limit: int = 3,
) -> dict[str, Any]:
    primary_path = resolve_primary_log(config_path, decisions_log)
    rows, bad_rows = load_rows(primary_path)
    datasets = [dataset_summary("primary", primary_path, rows, bad_rows, sample_limit)]
    for index, compare_path in enumerate(compare_logs or [], start=1):
        resolved = resolve_compare_log(compare_path)
        compare_rows, compare_bad = load_rows(resolved)
        datasets.append(dataset_summary(f"compare_{index}", resolved, compare_rows, compare_bad, sample_limit))
    return {
        "status": "ok" if datasets[0]["target_rows"] else "no_target_rows",
        "target_reason": TARGET_REASON,
        "inputs": {
            "config_path": str(config_path),
            "decisions_log": str(primary_path),
            "compare_decisions_logs": [str(path) for path in (compare_logs or [])],
        },
        "datasets": datasets,
        "cross_dataset": cross_dataset_summary(datasets),
        "certification": certification(datasets[0]),
        "runtime_contract": {
            "active_policy_changed": False,
            "promotion_activated": False,
            "decision_plane_v3_shadow_created": False,
        },
    }


def print_text(report: dict[str, Any]) -> None:
    primary = report["datasets"][0]
    cert = report["certification"]
    print(f"status={report['status']}")
    print(f"target_reason={report['target_reason']}")
    print(f"primary_target_rows={primary['target_rows']}")
    print(f"primary_target_share={primary['target_share_of_v3_rows']}")
    print(f"hard_risk_trigger_coverage={cert['hard_risk_trigger_coverage']}")
    print(
        "degraded_manipulation_contradiction_evidence_rows="
        f"{cert['degraded_manipulation_contradiction_evidence_rows']}"
    )
    print(f"p3_1_status={cert['p3_1_status']}")
    print(f"promotion_ready={cert['promotion_ready']}")


def main() -> None:
    args = parse_args()
    report = build_report(args.config, args.decisions_log, args.compare_decisions_log, args.sample_limit)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_text(report)


if __name__ == "__main__":
    main()
