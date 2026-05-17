#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import v3_outcome_quality_report
import v3_shadow_report


REPO_ROOT = Path(__file__).resolve().parents[1]
BLOCKING_STRICT = {"REJECT", "PENDING", "TIMEOUT"}
BLOCKING_TERMINAL_ONLY = {"REJECT", "TIMEOUT"}
CANDIDATE_VARIANT = "p36_candidate_organic_relaxed"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="P3.6 R10+R11 V3 shadow-only calibration report."
    )
    parser.add_argument(
        "--run",
        action="append",
        required=True,
        help="Run spec: name:<rollout_config>:<outcome_labels_jsonl>",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def resolve_path(path: str) -> Path:
    value = Path(path)
    return value if value.is_absolute() else (REPO_ROOT / value).resolve()


def parse_run_spec(spec: str) -> tuple[str, Path, Path]:
    parts = spec.split(":", 2)
    if len(parts) != 3 or not all(parts):
        raise ValueError(f"--run must use name:<config>:<labels>, got: {spec}")
    return parts[0], resolve_path(parts[1]), resolve_path(parts[2])


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def matrix_dict(matrix: dict[str, Counter[str]]) -> dict[str, dict[str, int]]:
    return {key: counter_dict(counter) for key, counter in sorted(matrix.items())}


def safe_ratio(numerator: int, denominator: int) -> float | None:
    return round(numerator / denominator, 6) if denominator else None


def numeric_stats(values: Iterable[Any]) -> dict[str, Any] | None:
    nums = [float(value) for value in values if isinstance(value, (int, float))]
    if not nums:
        return None
    return {
        "n": len(nums),
        "min": round(min(nums), 6),
        "median": round(statistics.median(nums), 6),
        "max": round(max(nums), 6),
    }


def label_for_quality_row(row: dict[str, Any]) -> str:
    return str(row.get("outcome_label") or "unknown")


def is_blocking_verdict(verdict: Any, *, terminal_only: bool = False) -> bool:
    normalized = str(verdict or "").upper()
    if normalized == "BUY_CANDIDATE":
        return False
    return normalized in (BLOCKING_TERMINAL_ONLY if terminal_only else BLOCKING_STRICT)


def load_run(name: str, config_path: Path, labels_path: Path) -> dict[str, Any]:
    decisions_log = v3_outcome_quality_report.resolve_primary_log(config_path, None)
    decision_rows, bad_rows = v3_shadow_report.load_jsonl(decisions_log)
    v3_rows = [row for row in decision_rows if v3_shadow_report.has_v3_fields(row)]
    label_rows = list(v3_outcome_quality_report.iter_jsonl(labels_path))
    label_index = v3_outcome_quality_report.index_by_join_key(label_rows)
    quality_rows = v3_outcome_quality_report.build_quality_rows(v3_rows, label_index, {}, 0.0)
    for source, quality in zip(v3_rows, quality_rows):
        quality["ab_record_id"] = source.get("ab_record_id")
    return {
        "name": name,
        "config_path": str(config_path),
        "decisions_log": str(decisions_log),
        "labels_path": str(labels_path),
        "decision_rows": len(decision_rows),
        "bad_decision_rows": bad_rows,
        "v3_rows": v3_rows,
        "quality_rows": quality_rows,
        "labels_loaded": len(label_rows),
    }


def headline(rows: list[dict[str, Any]]) -> dict[str, Any]:
    labels = Counter(label_for_quality_row(row) for row in rows)
    known_rows = len(rows) - labels.get("unknown", 0)
    avoided_bad = sum(
        1
        for row in rows
        if label_for_quality_row(row) == "bad_entry" and is_blocking_verdict(row.get("v3_verdict"))
    )
    blocked_good = sum(
        1
        for row in rows
        if label_for_quality_row(row) == "good_entry" and is_blocking_verdict(row.get("v3_verdict"))
    )
    denominator = avoided_bad + blocked_good
    total = len(rows)
    return {
        "known_rows": known_rows,
        "bad_entry": labels.get("bad_entry", 0),
        "good_entry": labels.get("good_entry", 0),
        "neutral_entry": labels.get("neutral_entry", 0),
        "unknown": labels.get("unknown", 0),
        "avoided_bad": avoided_bad,
        "blocked_good": blocked_good,
        "protective_ratio": safe_ratio(avoided_bad, blocked_good),
        "protective_precision": safe_ratio(avoided_bad, denominator),
        "neutral_share": round(labels.get("neutral_entry", 0) / total, 6) if total else 0.0,
        "unknown_share": round(labels.get("unknown", 0) / total, 6) if total else 0.0,
    }


def row_by_ab_record_id(rows: Iterable[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for row in rows:
        ab_id = row.get("ab_record_id")
        if isinstance(ab_id, str) and ab_id:
            indexed.setdefault(ab_id, row)
    return indexed


def evidence_requirements(row: dict[str, Any]) -> dict[str, bool]:
    payload = row.get("v3_policy_config_payload")
    if not isinstance(payload, dict):
        return {}
    requirements = payload.get("evidence_requirements")
    return requirements if isinstance(requirements, dict) else {}


def evidence_statuses(row: dict[str, Any]) -> dict[str, Any]:
    for field in ("v3_shadow_evidence_status", "v3_evidence_status"):
        value = row.get(field)
        if isinstance(value, dict):
            return value
    snapshot = row.get("v3_materialized_feature_snapshot")
    if isinstance(snapshot, dict) and isinstance(snapshot.get("evidence_status"), dict):
        return snapshot["evidence_status"]
    return {}


def reasons_from_status(status: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for key in ("degraded_reasons", "unavailable_reasons"):
        raw = status.get(key)
        if isinstance(raw, list):
            values.extend(str(item) for item in raw)
    return values


def pending_wait_evidence_decomposition(
    v3_rows: list[dict[str, Any]],
    quality_by_ab: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    groups: dict[str, Counter[str]] = defaultdict(Counter)
    reasons: dict[str, Counter[str]] = defaultdict(Counter)
    outcome_split: dict[str, Counter[str]] = defaultdict(Counter)
    strict_effect = Counter()
    terminal_only_effect = Counter()
    rows = [
        row
        for row in v3_rows
        if str(row.get("v3_shadow_reason_code") or "") == "PENDING_V3_WAIT_EVIDENCE"
    ]
    for row in rows:
        ab_id = str(row.get("ab_record_id") or "")
        quality = quality_by_ab.get(ab_id, {})
        label = label_for_quality_row(quality)
        strict_effect["block" if is_blocking_verdict(row.get("v3_shadow_verdict")) else "entry"] += 1
        if is_blocking_verdict(row.get("v3_shadow_verdict"), terminal_only=True):
            terminal_only_effect["terminal_block"] += 1
        elif str(row.get("v3_shadow_verdict") or "").upper() == "PENDING":
            terminal_only_effect["pending_separate"] += 1
        else:
            terminal_only_effect["entry"] += 1

        requirements = evidence_requirements(row)
        for group, status in evidence_statuses(row).items():
            if requirements and requirements.get(group) is not True:
                continue
            if not isinstance(status, dict):
                continue
            state = str(status.get("status") or "missing").lower()
            if state == "clean":
                continue
            groups[group][state] += 1
            outcome_split[group][label] += 1
            for reason in reasons_from_status(status):
                reasons[group][reason] += 1

    return {
        "rows": len(rows),
        "required_non_clean_groups": matrix_dict(groups),
        "required_non_clean_reasons": matrix_dict(reasons),
        "outcome_split": matrix_dict(outcome_split),
        "strict_effect": counter_dict(strict_effect),
        "terminal_only_effect": counter_dict(terminal_only_effect),
    }


def profile_thresholds(row: dict[str, Any]) -> dict[str, Any]:
    payload = row.get("v3_policy_config_payload")
    if not isinstance(payload, dict):
        return {}
    profiles = payload.get("profiles")
    if not isinstance(profiles, dict):
        return {}
    profile_name = policy_profile_name(row, payload)
    profile = profiles.get(profile_name)
    return profile if isinstance(profile, dict) else {}


def policy_profile_name(row: dict[str, Any], payload: dict[str, Any]) -> str:
    notes = row.get("v3_shadow_notes")
    deadline_elapsed = bool(notes.get("deadline_elapsed")) if isinstance(notes, dict) else False
    if deadline_elapsed:
        return "extended"
    snapshot = row.get("v3_materialized_feature_snapshot")
    session = snapshot.get("session_metadata") if isinstance(snapshot, dict) else None
    observation_duration_ms = 0
    if isinstance(session, dict):
        raw = session.get("observation_duration_ms")
        if isinstance(raw, int):
            observation_duration_ms = raw
    early_window_ms = payload.get("early_window_ms")
    if isinstance(early_window_ms, int) and observation_duration_ms < early_window_ms:
        return "early"
    return "normal"


def f64_from_bits(value: Any) -> float | None:
    if not isinstance(value, str):
        return None
    try:
        return _f64_from_u64_bits(int(value, 16))
    except ValueError:
        return None


def _f64_from_u64_bits(bits: int) -> float:
    import struct

    return struct.unpack(">d", bits.to_bytes(8, "big"))[0]


def manip_payload(row: dict[str, Any]) -> dict[str, Any]:
    for field in ("v3_shadow_manipulation_contradictions", "v3_manipulation_contradictions"):
        value = row.get(field)
        if isinstance(value, dict):
            return value
    snapshot = row.get("v3_materialized_feature_snapshot")
    if isinstance(snapshot, dict) and isinstance(snapshot.get("manipulation_contradictions"), dict):
        return snapshot["manipulation_contradictions"]
    return {}


def organic_payload(row: dict[str, Any]) -> dict[str, Any]:
    for field in ("v3_shadow_organic_broadening", "v3_organic_broadening"):
        value = row.get(field)
        if isinstance(value, dict):
            return value
    snapshot = row.get("v3_materialized_feature_snapshot")
    if isinstance(snapshot, dict) and isinstance(snapshot.get("organic_broadening"), dict):
        return snapshot["organic_broadening"]
    return {}


def tx_intel_payload(row: dict[str, Any]) -> dict[str, Any]:
    snapshot = row.get("v3_materialized_feature_snapshot")
    if isinstance(snapshot, dict) and isinstance(snapshot.get("tx_intel_features"), dict):
        return snapshot["tx_intel_features"]
    return {}


def numeric(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        return float(value)
    return None


def manip_subtriggers(row: dict[str, Any]) -> list[str]:
    risk = manip_payload(row)
    profile = profile_thresholds(row)
    same_ms_limit = f64_from_bits(profile.get("hard_fail_same_ms_tx_ratio_bits"))
    top3_limit = f64_from_bits(profile.get("hard_fail_top3_volume_pct_bits"))
    hhi_limit = f64_from_bits(profile.get("hard_fail_hhi_bits"))
    dev_limit = f64_from_bits(profile.get("max_dev_volume_ratio_bits"))
    signer_velocity_limit = f64_from_bits(profile.get("max_signer_cross_pool_velocity_bits"))

    triggers: list[str] = []
    dev_volume = numeric(risk.get("dev_volume_ratio"))
    if risk.get("high_dev_concentration") is True or (
        dev_volume is not None and dev_limit is not None and dev_volume > dev_limit
    ):
        triggers.append("dev_volume_ratio")
    top3 = numeric(risk.get("top3_volume_pct"))
    if risk.get("high_top3_volume_pct") is True or (
        top3 is not None and top3_limit is not None and top3 > top3_limit
    ):
        triggers.append("top3_volume_pct")
    hhi = numeric(risk.get("hhi"))
    if risk.get("high_hhi") is True or (
        hhi is not None and hhi_limit is not None and hhi > hhi_limit
    ):
        triggers.append("hhi")
    same_ms = numeric(risk.get("same_ms_tx_ratio"))
    bundle = numeric(risk.get("bundle_suspicion_ratio"))
    if (
        risk.get("high_same_ms_tx_ratio") is True
        or risk.get("high_bundle_suspicion_ratio") is True
        or risk.get("timing_bundle_concentration") is True
        or (same_ms is not None and same_ms_limit is not None and same_ms > same_ms_limit)
        or (bundle is not None and same_ms_limit is not None and bundle > same_ms_limit)
    ):
        triggers.append("same_ms_bundle")
    signer_velocity = numeric(risk.get("signer_cross_pool_velocity"))
    if risk.get("high_signer_concentration") is True or (
        signer_velocity is not None
        and signer_velocity_limit is not None
        and signer_velocity > signer_velocity_limit
    ):
        triggers.append("signer_concentration")
    return sorted(set(triggers))


def manipulation_decomposition(
    v3_rows: list[dict[str, Any]],
    quality_by_ab: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    by_trigger: dict[str, Counter[str]] = defaultdict(Counter)
    combinations: Counter[str] = Counter()
    rows = [
        row
        for row in v3_rows
        if str(row.get("v3_shadow_reason_code") or "")
        == "REJECT_V3_MANIPULATION_CONTRADICTION"
    ]
    for row in rows:
        ab_id = str(row.get("ab_record_id") or "")
        label = label_for_quality_row(quality_by_ab.get(ab_id, {}))
        triggers = manip_subtriggers(row)
        if not triggers:
            triggers = ["unclassified"]
        for trigger in triggers:
            by_trigger[trigger][label] += 1
        combinations["+".join(triggers)] += 1
    return {
        "rows": len(rows),
        "subtrigger_outcome_split": matrix_dict(by_trigger),
        "trigger_combinations": counter_dict(combinations),
    }


def organic_failure_reasons(row: dict[str, Any]) -> list[str]:
    organic = organic_payload(row)
    tx_intel = tx_intel_payload(row)
    profile = profile_thresholds(row)
    failures: list[str] = []

    if organic.get("sequence_available") is not True:
        failures.append("sequence_unavailable")

    total_tx_count = numeric(organic.get("total_tx_count")) or 0.0
    min_tx_count = numeric(profile.get("min_tx_count")) or 0.0
    if total_tx_count < min_tx_count:
        failures.append("total_tx_count_below_min")

    total_unique_signers = numeric(organic.get("total_unique_signers")) or 0.0
    min_unique_signers = numeric(profile.get("min_unique_signers")) or 0.0
    if total_unique_signers < min_unique_signers:
        failures.append("total_unique_signers_below_min")

    buy_count = numeric(tx_intel.get("buy_count")) or 0.0
    min_buy_count = numeric(profile.get("min_buy_count")) or 0.0
    if buy_count < min_buy_count:
        failures.append("buy_count_below_min")

    min_buy_ratio = f64_from_bits(profile.get("min_buy_ratio_bits"))
    buy_ratio_min = numeric(organic.get("buy_ratio_min"))
    if (
        buy_ratio_min is not None
        and min_buy_ratio is not None
        and buy_ratio_min < min_buy_ratio
    ):
        failures.append("buy_ratio_min_below_min")

    max_buy_ratio = f64_from_bits(profile.get("max_buy_ratio_bits"))
    buy_ratio_max = numeric(organic.get("buy_ratio_max"))
    if (
        buy_ratio_max is not None
        and max_buy_ratio is not None
        and buy_ratio_max > max_buy_ratio
    ):
        failures.append("buy_ratio_max_above_max")

    if numeric(organic.get("t1_vs_t0_unique_signer_delta")) is not None:
        if numeric(organic.get("t1_vs_t0_unique_signer_delta")) < 0:
            failures.append("t1_unique_signer_delta_negative")
    if numeric(organic.get("t2_vs_t1_unique_signer_delta")) is not None:
        if numeric(organic.get("t2_vs_t1_unique_signer_delta")) < 0:
            failures.append("t2_unique_signer_delta_negative")

    min_tx_growth = f64_from_bits(profile.get("organic_min_tx_count_growth_ratio_bits"))
    tx_growth = numeric(organic.get("tx_count_growth_ratio"))
    if tx_growth is not None and min_tx_growth is not None and tx_growth < min_tx_growth:
        failures.append("tx_count_growth_ratio_below_min")

    min_signer_growth = f64_from_bits(
        profile.get("organic_min_unique_signer_growth_ratio_bits")
    )
    signer_growth = numeric(organic.get("unique_signer_growth_ratio"))
    if (
        signer_growth is not None
        and min_signer_growth is not None
        and signer_growth < min_signer_growth
    ):
        failures.append("unique_signer_growth_ratio_below_min")

    max_hhi = f64_from_bits(profile.get("max_hhi_bits"))
    max_segment_hhi = numeric(organic.get("max_segment_hhi"))
    if max_segment_hhi is not None and max_hhi is not None and max_segment_hhi > max_hhi:
        failures.append("max_segment_hhi_above_max")

    return sorted(set(failures)) or ["organic_passes"]


def organic_decomposition(
    v3_rows: list[dict[str, Any]],
    quality_by_ab: dict[str, dict[str, Any]],
    variant: dict[str, Any] | None = None,
) -> dict[str, Any]:
    rows_by_ab = row_by_ab_record_id(v3_rows)
    if variant is None:
        target_ids = {
            str(row.get("ab_record_id") or "")
            for row in v3_rows
            if str(row.get("v3_shadow_reason_code") or "")
            == "REJECT_V3_LOW_ORGANIC_BROADENING"
        }
        source = "baseline"
    else:
        deltas = variant.get("row_deltas")
        if not isinstance(deltas, list):
            return {"status": "missing_row_deltas"}
        target_ids = {
            str(delta.get("ab_record_id") or "")
            for delta in deltas
            if str(delta.get("variant_reason") or "")
            == "REJECT_V3_LOW_ORGANIC_BROADENING"
        }
        source = f"variant:{CANDIDATE_VARIANT}"

    failure_counts: Counter[str] = Counter()
    failure_outcomes: dict[str, Counter[str]] = defaultdict(Counter)
    combinations: Counter[str] = Counter()
    label_counts: Counter[str] = Counter()
    for ab_id in sorted(target_ids):
        if not ab_id:
            continue
        row = rows_by_ab.get(ab_id)
        if row is None:
            continue
        label = label_for_quality_row(quality_by_ab.get(ab_id, {}))
        label_counts[label] += 1
        failures = organic_failure_reasons(row)
        combinations["+".join(failures)] += 1
        for failure in failures:
            failure_counts[failure] += 1
            failure_outcomes[failure][label] += 1

    return {
        "status": "ok",
        "source": source,
        "rows": sum(label_counts.values()),
        "label_counts": counter_dict(label_counts),
        "failure_counts": counter_dict(failure_counts),
        "failure_outcome_split": matrix_dict(failure_outcomes),
        "failure_combinations": counter_dict(combinations),
    }


def candidate_buy_analysis(
    v3_rows: list[dict[str, Any]],
    quality_by_ab: dict[str, dict[str, Any]],
    variant: dict[str, Any],
    variant_name: str,
) -> dict[str, Any]:
    deltas = variant.get("row_deltas")
    if not isinstance(deltas, list):
        return {"status": "missing_row_deltas"}

    rows_by_ab = row_by_ab_record_id(v3_rows)
    label_counts: Counter[str] = Counter()
    organic_failures: dict[str, Counter[str]] = defaultdict(Counter)
    manip_triggers: dict[str, Counter[str]] = defaultdict(Counter)
    feature_values: dict[str, dict[str, list[Any]]] = defaultdict(lambda: defaultdict(list))
    candidate_rows = 0

    feature_extractors = {
        "total_tx": lambda organic, tx, manip: organic.get("total_tx_count"),
        "unique_signers": lambda organic, tx, manip: organic.get("total_unique_signers"),
        "buy_count": lambda organic, tx, manip: tx.get("buy_count"),
        "buy_ratio_min": lambda organic, tx, manip: organic.get("buy_ratio_min"),
        "buy_ratio_mean": lambda organic, tx, manip: organic.get("buy_ratio_mean"),
        "buy_ratio_max": lambda organic, tx, manip: organic.get("buy_ratio_max"),
        "tx_growth": lambda organic, tx, manip: organic.get("tx_count_growth_ratio"),
        "signer_growth": lambda organic, tx, manip: organic.get("unique_signer_growth_ratio"),
        "max_segment_hhi": lambda organic, tx, manip: organic.get("max_segment_hhi"),
        "t1_delta": lambda organic, tx, manip: organic.get("t1_vs_t0_unique_signer_delta"),
        "t2_delta": lambda organic, tx, manip: organic.get("t2_vs_t1_unique_signer_delta"),
        "same_ms_tx_ratio": lambda organic, tx, manip: manip.get("same_ms_tx_ratio"),
        "bundle_suspicion_ratio": lambda organic, tx, manip: manip.get("bundle_suspicion_ratio"),
        "dev_volume_ratio": lambda organic, tx, manip: manip.get("dev_volume_ratio"),
        "top3_volume_pct": lambda organic, tx, manip: manip.get("top3_volume_pct"),
        "manipulation_hhi": lambda organic, tx, manip: manip.get("hhi"),
    }

    for delta in deltas:
        if not isinstance(delta, dict) or delta.get("variant_verdict") != "BUY_CANDIDATE":
            continue
        ab_id = str(delta.get("ab_record_id") or "")
        row = rows_by_ab.get(ab_id)
        if row is None:
            continue
        label = label_for_quality_row(quality_by_ab.get(ab_id, {}))
        label_counts[label] += 1
        candidate_rows += 1
        for failure in organic_failure_reasons(row):
            organic_failures[label][failure] += 1
        for trigger in manip_subtriggers(row):
            manip_triggers[label][trigger] += 1
        organic = organic_payload(row)
        tx = tx_intel_payload(row)
        manip = manip_payload(row)
        for feature, extractor in feature_extractors.items():
            feature_values[label][feature].append(extractor(organic, tx, manip))

    by_label = {
        label: {
            feature: stats
            for feature, values in sorted(features.items())
            if (stats := numeric_stats(values)) is not None
        }
        for label, features in sorted(feature_values.items())
    }

    return {
        "status": "ok",
        "variant": variant_name,
        "rows": candidate_rows,
        "sample_size_warning": candidate_rows < 50,
        "label_counts": counter_dict(label_counts),
        "organic_failure_split": matrix_dict(organic_failures),
        "manip_trigger_split": matrix_dict(manip_triggers),
        "feature_summary_by_label": by_label,
    }


def run_full_replay_ablation(path: Path) -> dict[str, Any]:
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "ghost-launcher",
        "--bin",
        "v3_replay",
        "--",
        "--input",
        str(path),
        "--ablation-json",
        "--strict",
    ]
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        return {
            "status": "failed",
            "exit_code": completed.returncode,
            "stdout_tail": completed.stdout[-2000:],
            "stderr_tail": completed.stderr[-2000:],
            "variants": {},
        }
    return json.loads(completed.stdout)


def variant_quality(
    variant: dict[str, Any],
    quality_by_ab: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    deltas = variant.get("row_deltas")
    if not isinstance(deltas, list):
        return {"status": "missing_row_deltas"}

    baseline_blocked_bad = baseline_blocked_good = 0
    variant_blocked_bad = variant_blocked_good = 0
    bad_unblocked = good_unblocked = neutral_unblocked = unknown_unblocked = 0
    reason_matrix: dict[str, Counter[str]] = defaultdict(Counter)
    verdict_matrix: dict[str, Counter[str]] = defaultdict(Counter)
    stage_matrix: dict[str, Counter[str]] = defaultdict(Counter)

    for delta in deltas:
        if not isinstance(delta, dict):
            continue
        label = label_for_quality_row(quality_by_ab.get(str(delta.get("ab_record_id") or ""), {}))
        baseline_block = is_blocking_verdict(delta.get("baseline_verdict"))
        variant_block = is_blocking_verdict(delta.get("variant_verdict"))
        if label == "bad_entry" and baseline_block:
            baseline_blocked_bad += 1
        if label == "good_entry" and baseline_block:
            baseline_blocked_good += 1
        if label == "bad_entry" and variant_block:
            variant_blocked_bad += 1
        if label == "good_entry" and variant_block:
            variant_blocked_good += 1
        if baseline_block and not variant_block:
            if label == "bad_entry":
                bad_unblocked += 1
            elif label == "good_entry":
                good_unblocked += 1
            elif label == "neutral_entry":
                neutral_unblocked += 1
            else:
                unknown_unblocked += 1
        reason_matrix[str(delta.get("baseline_reason") or "missing")][
            str(delta.get("variant_reason") or "missing")
        ] += 1
        verdict_matrix[str(delta.get("baseline_verdict") or "missing")][
            str(delta.get("variant_verdict") or "missing")
        ] += 1
        stage_matrix[str(delta.get("baseline_stage") or "missing")][
            str(delta.get("variant_stage") or "missing")
        ] += 1

    return {
        "status": "ok",
        "rows": len(deltas),
        "baseline_blocked_bad": baseline_blocked_bad,
        "baseline_blocked_good": baseline_blocked_good,
        "variant_blocked_bad": variant_blocked_bad,
        "variant_blocked_good": variant_blocked_good,
        "baseline_protective_ratio": safe_ratio(baseline_blocked_bad, baseline_blocked_good),
        "variant_protective_ratio": safe_ratio(variant_blocked_bad, variant_blocked_good),
        "baseline_protective_precision": safe_ratio(
            baseline_blocked_bad, baseline_blocked_bad + baseline_blocked_good
        ),
        "variant_protective_precision": safe_ratio(
            variant_blocked_bad, variant_blocked_bad + variant_blocked_good
        ),
        "bad_unblocked": bad_unblocked,
        "good_unblocked": good_unblocked,
        "net_good_recovered": good_unblocked - bad_unblocked,
        "safety_cost": bad_unblocked,
        "neutral_unblocked": neutral_unblocked,
        "unknown_unblocked": unknown_unblocked,
        "transition_matrix": {
            "baseline_reason_to_variant_reason": matrix_dict(reason_matrix),
            "baseline_verdict_to_variant_verdict": matrix_dict(verdict_matrix),
            "baseline_stage_to_variant_stage": matrix_dict(stage_matrix),
        },
    }


def combined_variant_quality(runs: list[dict[str, Any]]) -> dict[str, Any]:
    variants: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for run in runs:
        ablation = run["ablation"]
        for name, variant in ablation.get("variants", {}).items():
            variants[name].append(variant_quality(variant, run["quality_by_ab"]))

    result: dict[str, Any] = {}
    for name, parts in sorted(variants.items()):
        ok_parts = [part for part in parts if part.get("status") == "ok"]
        if len(ok_parts) != len(parts):
            result[name] = {"status": "missing_row_deltas", "runs": parts}
            continue
        totals = Counter()
        combined_transition: dict[str, dict[str, Counter[str]]] = {
            "baseline_reason_to_variant_reason": defaultdict(Counter),
            "baseline_verdict_to_variant_verdict": defaultdict(Counter),
            "baseline_stage_to_variant_stage": defaultdict(Counter),
        }
        for part in ok_parts:
            for key, value in part.items():
                if isinstance(value, int):
                    totals[key] += value
            transition = part.get("transition_matrix", {})
            if isinstance(transition, dict):
                for matrix_name, matrix in transition.items():
                    if matrix_name not in combined_transition or not isinstance(matrix, dict):
                        continue
                    for source, targets in matrix.items():
                        if isinstance(targets, dict):
                            combined_transition[matrix_name][source].update(targets)
        result[name] = {
            "status": "ok",
            **dict(totals),
            "baseline_protective_ratio": safe_ratio(
                totals["baseline_blocked_bad"], totals["baseline_blocked_good"]
            ),
            "variant_protective_ratio": safe_ratio(
                totals["variant_blocked_bad"], totals["variant_blocked_good"]
            ),
            "baseline_protective_precision": safe_ratio(
                totals["baseline_blocked_bad"],
                totals["baseline_blocked_bad"] + totals["baseline_blocked_good"],
            ),
            "variant_protective_precision": safe_ratio(
                totals["variant_blocked_bad"],
                totals["variant_blocked_bad"] + totals["variant_blocked_good"],
            ),
            "transition_matrix": {
                name: matrix_dict(matrix) for name, matrix in combined_transition.items()
            },
            "runs": parts,
        }
    return result


def r12_gate(headline_report: dict[str, Any], runs: list[dict[str, Any]], variants: dict[str, Any]) -> dict[str, Any]:
    blockers: list[str] = []
    if any(run["ablation"].get("status") != "ok" or run["ablation"].get("replay_status") != "full_replay_ok" for run in runs):
        blockers.append("baseline_full_replay_not_ok_for_every_run")
    candidate = variants.get(CANDIDATE_VARIANT, {})
    candidate_ok = candidate.get("status") == "ok"
    if not candidate_ok:
        blockers.append("candidate_missing_full_row_deltas")
    if candidate_ok:
        candidate_ratio = candidate.get("variant_protective_ratio")
        if candidate_ratio is None or candidate_ratio < 1.30:
            blockers.append("candidate_protective_ratio_below_1_30")
        if candidate.get("variant_blocked_good", headline_report.get("blocked_good", 0)) >= headline_report.get("blocked_good", 0):
            blockers.append("blocked_good_not_reduced")
        if candidate.get("bad_unblocked", 0) > candidate.get("good_unblocked", 0) * 0.50:
            blockers.append("bad_unblocked_exceeds_half_good_unblocked")
        if candidate.get("unknown_unblocked", 0) > max(1, candidate.get("good_unblocked", 0)):
            blockers.append("unknown_unblocked_dominates")
    return {
        "candidate": "V3-P36-ORGANIC-RELAXED",
        "candidate_variant": CANDIDATE_VARIANT,
        "candidate_metrics": {
            "variant_protective_ratio": candidate.get("variant_protective_ratio"),
            "variant_protective_precision": candidate.get("variant_protective_precision"),
            "variant_blocked_bad": candidate.get("variant_blocked_bad"),
            "variant_blocked_good": candidate.get("variant_blocked_good"),
            "good_unblocked": candidate.get("good_unblocked"),
            "bad_unblocked": candidate.get("bad_unblocked"),
            "unknown_unblocked": candidate.get("unknown_unblocked"),
        },
        "r12_gate_status": "pass" if not blockers else "blocked",
        "blocked_gates": blockers,
        "no_active_policy_change": True,
        "no_p2_promotion": True,
        "diagnostic_only_variants": [
            "fsc_not_required",
            "no_pending_wait_evidence_for_noncritical_degraded",
            "no_manipulation_contradiction",
            "manip_split_dev_top3_hhi",
            "p36_evidence_soft_manip_split",
            "p36_candidate_no_organic_hhi",
            "p36_candidate_no_organic_growth",
            "p36_candidate_no_buy_ratio_min",
            "relaxed_sample_gate",
        ],
    }


def build_report(run_specs: list[str]) -> dict[str, Any]:
    runs: list[dict[str, Any]] = []
    combined_quality_rows: list[dict[str, Any]] = []
    combined_v3_rows: list[dict[str, Any]] = []
    for spec in run_specs:
        name, config_path, labels_path = parse_run_spec(spec)
        run = load_run(name, config_path, labels_path)
        quality_by_ab = row_by_ab_record_id(run["quality_rows"])
        run["quality_by_ab"] = quality_by_ab
        run["headline"] = headline(run["quality_rows"])
        run["evidence_decomposition"] = pending_wait_evidence_decomposition(
            run["v3_rows"], quality_by_ab
        )
        run["manipulation_decomposition"] = manipulation_decomposition(
            run["v3_rows"], quality_by_ab
        )
        run["ablation"] = run_full_replay_ablation(Path(run["decisions_log"]))
        run["organic_decomposition"] = organic_decomposition(
            run["v3_rows"],
            quality_by_ab,
            run["ablation"].get("variants", {}).get(CANDIDATE_VARIANT),
        )
        run["candidate_buy_analysis"] = candidate_buy_analysis(
            run["v3_rows"],
            quality_by_ab,
            run["ablation"].get("variants", {}).get(CANDIDATE_VARIANT, {}),
            CANDIDATE_VARIANT,
        )
        combined_quality_rows.extend(run["quality_rows"])
        combined_v3_rows.extend(run["v3_rows"])
        runs.append(run)

    combined_quality_by_ab = row_by_ab_record_id(combined_quality_rows)
    headline_report = headline(combined_quality_rows)
    variants = combined_variant_quality(runs)
    combined_candidate_variant = {"row_deltas": []}
    for run in runs:
        candidate = run["ablation"].get("variants", {}).get(CANDIDATE_VARIANT, {})
        deltas = candidate.get("row_deltas")
        if isinstance(deltas, list):
            combined_candidate_variant["row_deltas"].extend(deltas)
    public_runs = []
    for run in runs:
        public = {key: value for key, value in run.items() if key not in {"v3_rows", "quality_rows", "quality_by_ab"}}
        public_runs.append(public)
    return {
        "status": "ok" if combined_v3_rows else "no_v3_rows",
        "p3_6_scope": {
            "shadow_only": True,
            "no_active_v2_v25_change": True,
            "no_iwim_change": True,
            "no_live_sender_change": True,
            "no_p2_promotion": True,
            "historical_r10_r11_hashes_immutable": True,
        },
        "headline": headline_report,
        "evidence_decomposition": pending_wait_evidence_decomposition(
            combined_v3_rows, combined_quality_by_ab
        ),
        "manipulation_decomposition": manipulation_decomposition(
            combined_v3_rows, combined_quality_by_ab
        ),
        "organic_decomposition": organic_decomposition(
            combined_v3_rows, combined_quality_by_ab, combined_candidate_variant
        ),
        "candidate_buy_analysis": candidate_buy_analysis(
            combined_v3_rows,
            combined_quality_by_ab,
            combined_candidate_variant,
            CANDIDATE_VARIANT,
        ),
        "variant_quality": variants,
        "r12_gate": r12_gate(headline_report, runs, variants),
        "runs": public_runs,
    }


def print_text(report: dict[str, Any]) -> None:
    headline_report = report["headline"]
    gate = report["r12_gate"]
    print(f"status={report['status']}")
    print(f"known_rows={headline_report['known_rows']}")
    print(f"avoided_bad={headline_report['avoided_bad']}")
    print(f"blocked_good={headline_report['blocked_good']}")
    print(f"protective_ratio={headline_report['protective_ratio']}")
    print(f"r12_gate_status={gate['r12_gate_status']}")
    print(f"blocked_gates={gate['blocked_gates']}")


def main() -> None:
    args = parse_args()
    report = build_report(args.run)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_text(report)


if __name__ == "__main__":
    main()
