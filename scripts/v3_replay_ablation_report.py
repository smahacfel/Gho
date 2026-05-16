#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import v3_shadow_report


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"
LABEL_SUCCESS_STATUSES = {"confirmed", "landed", "success", "executed"}
UNKNOWN_OUTCOMES = {"missing", "submitted", "no_dispatch", "unknown", "no_execution"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="P3 replay/ablation/calibration gate for V3 shadow sidecar evidence."
    )
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decisions-log", type=Path)
    parser.add_argument(
        "--compare-decisions-log",
        action="append",
        default=[],
        type=Path,
        help="Additional historical V3 JSONL to compare against the primary run.",
    )
    parser.add_argument(
        "--shadow-lifecycle",
        type=Path,
        help="Reserved for a future lifecycle merge; currently fails closed if provided.",
    )
    parser.add_argument(
        "--events-dir",
        type=Path,
        help="Reserved for a future event replay merge; currently fails closed if provided.",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def load_rows(path: Path) -> tuple[list[dict[str, Any]], int]:
    return v3_shadow_report.load_jsonl(path)


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


def validate_unimplemented_inputs(shadow_lifecycle: Path | None, events_dir: Path | None) -> None:
    if shadow_lifecycle is not None:
        raise NotImplementedError(
            "--shadow-lifecycle is accepted only after lifecycle merge is implemented"
        )
    if events_dir is not None:
        raise NotImplementedError("--events-dir is accepted only after event replay merge is implemented")


def has_v3(row: dict[str, Any]) -> bool:
    return v3_shadow_report.has_v3_fields(row)


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def nested_counter_dict(counters: dict[str, Counter[str]]) -> dict[str, dict[str, int]]:
    return {key: counter_dict(counter) for key, counter in sorted(counters.items())}


def active_verdict(row: dict[str, Any]) -> str:
    return v3_shadow_report.active_verdict(row)


def confidence_bucket(value: Any) -> str:
    return v3_shadow_report.confidence_bucket(value)


def outcome_status(row: dict[str, Any]) -> str:
    raw = row.get("shadow_execution_outcome") or row.get("execution_status")
    if raw is None:
        return "missing"
    return str(raw).strip().lower() or "missing"


def is_success_outcome(row: dict[str, Any]) -> bool:
    return outcome_status(row) in LABEL_SUCCESS_STATUSES


def is_known_outcome(row: dict[str, Any]) -> bool:
    status = outcome_status(row)
    return status not in UNKNOWN_OUTCOMES and status != "missing"


def degraded_evidence_ratio(rows: list[dict[str, Any]]) -> float:
    if not rows:
        return 0.0
    degraded = 0
    for row in rows:
        evidence = row.get("v3_shadow_evidence_status") or row.get("v3_evidence_status")
        if not isinstance(evidence, dict):
            degraded += 1
            continue
        if any(
            isinstance(value, dict)
            and str(value.get("status", "")).lower() in {"degraded", "unavailable"}
            for value in evidence.values()
        ):
            degraded += 1
    return round(degraded / len(rows), 6)


def calibration_buckets(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[confidence_bucket(row.get("v3_shadow_confidence"))].append(row)

    result: dict[str, dict[str, Any]] = {}
    for bucket in ("0", "0_to_0_25", "0_25_to_0_50", "0_50_to_0_75", "0_75_to_1_00", "missing"):
        bucket_rows = grouped.get(bucket, [])
        known = sum(1 for row in bucket_rows if is_known_outcome(row))
        unknown = len(bucket_rows) - known
        result[bucket] = {
            "count": len(bucket_rows),
            "active_verdict_distribution": counter_dict(Counter(active_verdict(row) for row in bucket_rows)),
            "v3_verdict_distribution": counter_dict(
                Counter(str(row.get("v3_shadow_verdict") or "missing") for row in bucket_rows)
            ),
            "outcome_label_coverage": round(known / len(bucket_rows), 6) if bucket_rows else 0.0,
            "unknown_outcome_ratio": round(unknown / len(bucket_rows), 6) if bucket_rows else 0.0,
            "degraded_evidence_ratio": degraded_evidence_ratio(bucket_rows),
        }
    return result


def reason_group(row: dict[str, Any]) -> str:
    reason = str(row.get("v3_shadow_reason_code") or "")
    cap_reasons = [str(value) for value in row.get("v3_shadow_confidence_cap_reasons", []) or []]
    joined = " ".join([reason, *cap_reasons]).lower()
    if "manipulation" in joined:
        return "manipulation_contradiction"
    if "organic" in joined or "sample" in joined or "broadening" in joined:
        return "organic_broadening"
    if "sybil" in joined or "fsc" in joined or "cpv" in joined:
        return "sybil_fsc_cpv_caps"
    if "alpha" in joined:
        return "alpha_cap"
    if "execution" in joined:
        return "execution_cap"
    if "evidence" in joined:
        return "evidence_wait"
    return "other"


def ablation_proxy(rows: list[dict[str, Any]]) -> dict[str, Any]:
    groups = Counter(reason_group(row) for row in rows)
    v3_verdicts = Counter(str(row.get("v3_shadow_verdict") or "missing") for row in rows)
    variants: dict[str, dict[str, Any]] = {
        "full_v3": {
            "verdict_distribution": counter_dict(v3_verdicts),
            "changed_rows_proxy": 0,
            "note": "Observed V3 verdicts; no counterfactual recomputation.",
        }
    }
    mapping = {
        "no_organic_broadening": "organic_broadening",
        "no_manipulation_contradiction": "manipulation_contradiction",
        "no_sybil_fsc_cpv_caps": "sybil_fsc_cpv_caps",
        "no_alpha_cap": "alpha_cap",
        "no_execution_cap": "execution_cap",
    }
    for variant, group in mapping.items():
        impacted = groups.get(group, 0)
        variants[variant] = {
            "changed_rows_proxy": impacted,
            "affected_reason_group": group,
            "mode": "reason_group_proxy",
            "note": "Proxy mode cannot recompute V3; full replay counterfactual mode requires replay-stable payload rows.",
        }
    return {
        "mode": "reason_group_proxy",
        "reason_group_counts": counter_dict(groups),
        "variants": variants,
    }


def full_replay_ablation(path: Path) -> dict[str, Any]:
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
            "mode": "full_replay_counterfactual_failed",
            "engine": "ghost-launcher::v3_replay --ablation-json --strict",
            "exit_code": completed.returncode,
            "stdout_tail": completed.stdout[-2000:],
            "stderr_tail": completed.stderr[-2000:],
            "variants": {},
        }
    try:
        report = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        return {
            "mode": "full_replay_counterfactual_failed",
            "engine": "ghost-launcher::v3_replay --ablation-json --strict",
            "exit_code": completed.returncode,
            "error": f"invalid json from rust ablation engine: {exc}",
            "stdout_tail": completed.stdout[-2000:],
            "stderr_tail": completed.stderr[-2000:],
            "variants": {},
        }
    return {
        "mode": "full_replay_counterfactual",
        "engine": "ghost-launcher::v3_replay --ablation-json --strict",
        "replay_status": report.get("replay_status"),
        "baseline_status_counts": report.get("baseline_status_counts", {}),
        "variants": report.get("variants", {}),
    }


def replay_parity(rows: list[dict[str, Any]], bad_rows: int) -> dict[str, Any]:
    v3_rows = [row for row in rows if has_v3(row)]
    rows_by_ab_id: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in v3_rows:
        ab_id = str(row.get("ab_record_id") or "")
        if ab_id:
            rows_by_ab_id[ab_id].append(row)

    conflicts: dict[str, dict[str, Any]] = {}
    for ab_id, grouped in rows_by_ab_id.items():
        signatures = {
            (
                row.get("v3_policy_config_hash"),
                row.get("v3_feature_snapshot_hash"),
                row.get("v3_shadow_verdict"),
                row.get("v3_shadow_reason_code"),
            )
            for row in grouped
        }
        if len(signatures) > 1:
            conflicts[ab_id] = {
                "rows": len(grouped),
                "signatures": [list(signature) for signature in sorted(signatures)],
            }

    policy_missing = sum(1 for row in v3_rows if not row.get("v3_policy_config_hash"))
    snapshot_missing = sum(1 for row in v3_rows if not row.get("v3_feature_snapshot_hash"))
    full_payload_rows = sum(1 for row in v3_rows if v3_shadow_report.has_full_replay_payload(row))
    if not v3_rows:
        status = "no_v3_rows"
    elif full_payload_rows == len(v3_rows):
        status = "full"
    elif snapshot_missing < len(v3_rows):
        status = "hash_only"
    else:
        status = "missing_snapshot_hash"

    note = (
        "full means JSONL carries replay payload rows; strict counterfactual ablation still requires the Rust replay engine."
        if status == "full"
        else "hash_only means P3 can compare stable snapshot hashes but cannot recompute counterfactual V3."
    )

    return {
        "status": status,
        "rows": len(v3_rows),
        "bad_rows": bad_rows,
        "policy_hash_missing": policy_missing,
        "snapshot_hash_missing": snapshot_missing,
        "full_snapshot_payload_rows": full_payload_rows,
        "duplicate_ab_record_conflicts": conflicts,
        "duplicate_ab_record_conflict_count": len(conflicts),
        "note": note,
    }


def dataset_summary(name: str, path: Path, rows: list[dict[str, Any]], bad_rows: int) -> dict[str, Any]:
    v3_rows = [row for row in rows if has_v3(row)]
    return {
        "name": name,
        "path": str(path),
        "rows": len(rows),
        "bad_rows": bad_rows,
        "v3_rows": len(v3_rows),
        "policy_hashes": counter_dict(Counter(str(row.get("v3_policy_config_hash") or "missing") for row in v3_rows)),
        "v3_reason_codes": counter_dict(Counter(str(row.get("v3_shadow_reason_code") or "missing") for row in v3_rows)),
        "v3_verdicts": counter_dict(Counter(str(row.get("v3_shadow_verdict") or "missing") for row in v3_rows)),
        "active_vs_v3": nested_counter_dict(active_vs_v3(v3_rows)),
    }


def active_vs_v3(rows: list[dict[str, Any]]) -> dict[str, Counter[str]]:
    matrix: dict[str, Counter[str]] = defaultdict(Counter)
    for row in rows:
        matrix[active_verdict(row)][str(row.get("v3_shadow_verdict") or "missing")] += 1
    return matrix


def stability_matrix(datasets: list[dict[str, Any]]) -> dict[str, Any]:
    reason_sets = {
        item["name"]: set(item["v3_reason_codes"].keys()) - {"missing"}
        for item in datasets
        if item["v3_rows"] > 0
    }
    policy_sets = {
        item["name"]: set(item["policy_hashes"].keys()) - {"missing"}
        for item in datasets
        if item["v3_rows"] > 0
    }
    pairwise: dict[str, dict[str, Any]] = {}
    names = list(reason_sets)
    for index, left in enumerate(names):
        for right in names[index + 1 :]:
            left_set = reason_sets[left]
            right_set = reason_sets[right]
            union = left_set | right_set
            pairwise[f"{left}__vs__{right}"] = {
                "shared_reason_codes": sorted(left_set & right_set),
                "left_only_reason_codes": sorted(left_set - right_set),
                "right_only_reason_codes": sorted(right_set - left_set),
                "reason_jaccard": round(len(left_set & right_set) / len(union), 6) if union else 1.0,
            }
    return {
        "policy_hash_sets": {name: sorted(values) for name, values in sorted(policy_sets.items())},
        "reason_code_sets": {name: sorted(values) for name, values in sorted(reason_sets.items())},
        "pairwise_reason_stability": pairwise,
    }


def certification(
    rows: list[dict[str, Any]],
    replay: dict[str, Any],
    calibration: dict[str, dict[str, Any]],
    ablation: dict[str, Any],
) -> dict[str, Any]:
    v3_rows = [row for row in rows if has_v3(row)]
    blockers: list[str] = []
    insufficient: list[str] = []
    if not v3_rows:
        blockers.append("no_v3_rows")
    if replay["status"] != "full":
        insufficient.append(f"replay_{replay['status']}")
    elif ablation.get("mode") != "full_replay_counterfactual":
        blockers.append("full_replay_ablation_unavailable")
    if replay["policy_hash_missing"] or replay["snapshot_hash_missing"]:
        blockers.append("missing_v3_hashes")
    label_counts = [bucket["count"] for bucket in calibration.values()]
    known_coverage = sum(
        bucket["count"] * bucket["outcome_label_coverage"] for bucket in calibration.values()
    )
    total = sum(label_counts)
    label_coverage = round(known_coverage / total, 6) if total else 0.0
    if label_coverage < 0.5:
        insufficient.append("low_outcome_label_coverage")
    manipulation_count = ablation.get("reason_group_counts", {}).get(
        "manipulation_contradiction",
        Counter(reason_group(row) for row in v3_rows).get("manipulation_contradiction", 0),
    )
    if v3_rows and manipulation_count / len(v3_rows) >= 0.5:
        insufficient.append("dominant_manipulation_contradiction_requires_more_evidence")

    if blockers:
        status = "fail"
    elif insufficient:
        status = "insufficient_data"
    else:
        status = "pass"

    return {
        "p3_status": status,
        "promotion_ready_gates": [],
        "blocked_gates": sorted(blockers),
        "insufficient_evidence_gates": sorted(set(insufficient)),
        "no_p2_promotion": True,
        "label_coverage": label_coverage,
        "note": "P3 may recommend ADR candidates later, but this report never activates promotion.",
    }


def build_report(
    config_path: Path,
    decisions_log: Path | None = None,
    compare_logs: list[Path] | None = None,
    shadow_lifecycle: Path | None = None,
    events_dir: Path | None = None,
) -> dict[str, Any]:
    validate_unimplemented_inputs(shadow_lifecycle, events_dir)
    primary_path = resolve_primary_log(config_path, decisions_log)
    primary_rows, primary_bad = load_rows(primary_path)
    v3_rows = [row for row in primary_rows if has_v3(row)]
    replay = replay_parity(primary_rows, primary_bad)
    calibration = calibration_buckets(v3_rows)
    if replay["status"] == "full":
        ablation = full_replay_ablation(primary_path)
    else:
        ablation = ablation_proxy(v3_rows)

    datasets = [dataset_summary("primary", primary_path, primary_rows, primary_bad)]
    for index, compare_path in enumerate(compare_logs or [], start=1):
        resolved = resolve_compare_log(compare_path)
        rows, bad = load_rows(resolved)
        datasets.append(dataset_summary(f"compare_{index}", resolved, rows, bad))

    cert = certification(primary_rows, replay, calibration, ablation)
    return {
        "status": "ok" if v3_rows else "no_v3_rows",
        "inputs": {
            "config_path": str(config_path),
            "decisions_log": str(primary_path),
            "compare_decisions_logs": [str(path) for path in (compare_logs or [])],
            "shadow_lifecycle": str(shadow_lifecycle) if shadow_lifecycle else None,
            "events_dir": str(events_dir) if events_dir else None,
        },
        "replay": replay,
        "calibration": {
            "confidence_buckets": calibration,
            "note": "Outcome label ratios are offline diagnostics and are never runtime features.",
        },
        "ablation": ablation,
        "datasets": datasets,
        "stability": stability_matrix(datasets),
        "certification": cert,
        "runtime_contract": {
            "active_policy_changed": False,
            "promotion_activated": False,
            "decision_plane_v3_shadow_created": False,
        },
    }


def print_text(report: dict[str, Any]) -> None:
    cert = report["certification"]
    replay = report["replay"]
    print(f"status={report['status']}")
    print(f"p3_status={cert['p3_status']}")
    print(f"replay_status={replay['status']}")
    print(f"v3_rows={replay['rows']}")
    print(f"promotion_ready_gates={cert['promotion_ready_gates']}")
    print(f"blocked_gates={cert['blocked_gates']}")
    print(f"insufficient_evidence_gates={cert['insufficient_evidence_gates']}")


def main() -> None:
    args = parse_args()
    report = build_report(
        args.config,
        args.decisions_log,
        args.compare_decisions_log,
        args.shadow_lifecycle,
        args.events_dir,
    )
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_text(report)


if __name__ == "__main__":
    main()
