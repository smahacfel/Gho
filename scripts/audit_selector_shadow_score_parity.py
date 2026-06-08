#!/usr/bin/env python3
"""Audit runtime selector shadow score parity against same-scope decisions.

P3L-B emits a runtime partial score sidecar.  This audit verifies that the
sidecar score matches the declared runtime formula when recomputed from the
same Gatekeeper decision rows.  It also reports mapped-only drift as diagnostic
context because the current runtime score intentionally marks flow rollups as
missing runtime mappings.
"""

from __future__ import annotations

import argparse
import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "selector_shadow_score_parity_audit_v1"
DECISION_FILE = "gatekeeper_v2_decisions.jsonl"
SCORE_FILE = "selector_shadow_score_v1.jsonl"
EXPECTED_SCORE_VERSION = "selector_shadow_score_combined_simple_v1"
EXPECTED_CANDIDATE_ID = "combined:simple_feature_score_v1"
DEFAULT_RUST_SOURCE = "ghost-brain/src/oracle/decision_logger.rs"
FLOAT_TOLERANCE = 1e-9


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-scope", required=True, help="Runtime rollout scope.")
    parser.add_argument("--root", default="/root/Gho", help="Repository/runtime root.")
    parser.add_argument("--decision-plane", default=None, help="Optional decision plane filter.")
    parser.add_argument(
        "--rust-source",
        default=DEFAULT_RUST_SOURCE,
        help="Rust source containing the frozen runtime score spec.",
    )
    parser.add_argument(
        "--tolerance",
        type=float,
        default=FLOAT_TOLERANCE,
        help="Absolute float tolerance for parity.",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Optional JSON output path. Defaults under reports/selector/<runtime-scope>.",
    )
    parser.add_argument("--json", action="store_true", help="Print JSON report.")
    return parser


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return list(common.iter_json_objects(path))


def default_output(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"{ARTIFACT}.json"


def decision_dirs(root: Path, scope: str, decision_plane: str | None) -> list[Path]:
    decisions_root = root / "logs" / "rollout" / scope / "decisions" / scope
    if not decisions_root.exists():
        return []
    paths = sorted(decisions_root.rglob(DECISION_FILE))
    if decision_plane:
        paths = [path for path in paths if f"/{decision_plane}/" in path.as_posix()]
    return [path.parent for path in paths]


def parse_runtime_spec(source_path: Path) -> tuple[list[dict[str, Any]], dict[str, float]]:
    text = source_path.read_text(encoding="utf-8")
    specs: list[dict[str, Any]] = []
    pattern = re.compile(
        r"SelectorShadowFeatureSpec\s*\{\s*"
        r'name:\s*"(?P<name>[^"]+)",\s*'
        r"min:\s*(?P<min>[-+0-9.eE]+),\s*"
        r"max:\s*(?P<max>[-+0-9.eE]+),\s*"
        r"direction:\s*(?P<direction>[-+0-9.eE]+),\s*"
        r"source:\s*SelectorShadowRuntimeFeatureSource::(?P<source>[A-Za-z]+),\s*"
        r"\}",
        re.MULTILINE,
    )
    for match in pattern.finditer(text):
        specs.append(
            {
                "name": match.group("name"),
                "min": float(match.group("min")),
                "max": float(match.group("max")),
                "direction": float(match.group("direction")),
                "source": match.group("source"),
            }
        )
    if not specs:
        raise ValueError(f"no SelectorShadowFeatureSpec entries parsed from {source_path}")

    thresholds: dict[str, float] = {}
    threshold_patterns = {
        "top10_equiv_pass": "SELECTOR_SHADOW_TOP10_EQUIV_THRESHOLD",
        "top25_equiv_pass": "SELECTOR_SHADOW_TOP25_EQUIV_THRESHOLD",
        "q99_pass": "SELECTOR_SHADOW_Q99_THRESHOLD",
        "q98_pass": "SELECTOR_SHADOW_Q98_THRESHOLD",
        "q975_pass": "SELECTOR_SHADOW_Q975_THRESHOLD",
        "target_precision_0_70_pass": "SELECTOR_SHADOW_TARGET_PRECISION_0_70_THRESHOLD",
    }
    for output_name, const_name in threshold_patterns.items():
        threshold_match = re.search(
            rf"const\s+{const_name}:\s*f64\s*=\s*([-+0-9.eE]+);",
            text,
        )
        if not threshold_match:
            raise ValueError(f"missing threshold constant {const_name} in {source_path}")
        thresholds[output_name] = float(threshold_match.group(1))
    return specs, thresholds


def numeric(value: Any) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        numeric_value = float(value)
        return numeric_value if math.isfinite(numeric_value) else None
    return None


def bool_value(value: Any) -> float | None:
    if value is True:
        return 1.0
    if value is False:
        return 0.0
    return None


def vector_event_count(row: dict[str, Any]) -> float | None:
    prices = row.get("vectors_prices")
    sol_amounts = row.get("vectors_sol_amounts")
    offsets = row.get("vectors_ts_offsets_ms")
    counts = [
        len(value)
        for value in (prices, sol_amounts, offsets)
        if isinstance(value, list) and value
    ]
    return float(max(counts)) if counts else None


def price_return(prices: list[Any]) -> float | None:
    if not prices:
        return None
    first = numeric(prices[0])
    last = numeric(prices[-1])
    if first is None or last is None or first == 0.0:
        return None
    return (last - first) / first


def price_drawdown(prices: list[Any]) -> float | None:
    peak: float | None = None
    max_drawdown = 0.0
    found = False
    for raw in prices:
        value = numeric(raw)
        if value is None:
            continue
        found = True
        peak = value if peak is None else max(peak, value)
        if peak and peak > 0.0:
            max_drawdown = max(max_drawdown, (peak - value) / peak)
    return max_drawdown if found else None


def intervals_from_offsets(offsets: list[Any]) -> list[float]:
    values = [numeric(value) for value in offsets]
    clean = [value for value in values if value is not None]
    if len(clean) < 2:
        return []
    return [max(0.0, right - left) for left, right in zip(clean, clean[1:])]


def median(values: list[float]) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2 == 0:
        return (ordered[mid - 1] + ordered[mid]) / 2.0
    return ordered[mid]


def value_for_feature(row: dict[str, Any], feature: str) -> float | None:
    if feature == "gk_curve_wait_elapsed_ms":
        return numeric(row.get("curve_wait_elapsed_ms"))
    if feature in {
        "gk_dev_has_sold",
        "gk_dev_sold_within_3s",
        "gk_dev_sold_within_5s",
    }:
        return bool_value(row.get(feature[3:]))
    if feature == "gk_iwim_confidence":
        return numeric(row.get("iwim_confidence"))
    if feature == "gk_iwim_sybil_score":
        return numeric(row.get("iwim_sybil_score"))
    if feature == "gk_iwim_organic_score":
        return numeric(row.get("iwim_organic_score"))
    if feature == "gk_fsc_buyer_sample_count":
        funding = row.get("funding_source_v2")
        if isinstance(funding, dict):
            return numeric(funding.get("total_buyers"))
        return None
    if feature == "gk_vector_event_count":
        return vector_event_count(row)
    if feature.startswith("gk_vector_price_"):
        prices = row.get("vectors_prices")
        if not isinstance(prices, list):
            return None
        clean = [numeric(value) for value in prices]
        clean = [value for value in clean if value is not None]
        if feature == "gk_vector_price_first":
            return clean[0] if clean else None
        if feature == "gk_vector_price_last":
            return clean[-1] if clean else None
        if feature == "gk_vector_price_return":
            return price_return(prices)
        if feature == "gk_vector_price_max":
            return max(clean) if clean else None
        if feature == "gk_vector_price_min":
            return min(clean) if clean else None
        if feature == "gk_vector_price_drawdown":
            return price_drawdown(prices)
    if feature == "gk_vector_sol_sum":
        values = row.get("vectors_sol_amounts")
        if isinstance(values, list):
            clean = [numeric(value) for value in values]
            clean = [value for value in clean if value is not None]
            return sum(clean) if clean else None
        return None
    if feature == "gk_vector_sol_max":
        values = row.get("vectors_sol_amounts")
        if isinstance(values, list):
            clean = [numeric(value) for value in values]
            clean = [value for value in clean if value is not None]
            return max(clean) if clean else None
        return None
    if feature.startswith("gk_vector_interval_"):
        offsets = row.get("vectors_ts_offsets_ms")
        if not isinstance(offsets, list):
            return None
        intervals = intervals_from_offsets(offsets)
        if feature == "gk_vector_interval_median":
            return median(intervals)
        if feature == "gk_vector_interval_min":
            return min(intervals) if intervals else None
        if feature == "gk_vector_interval_max":
            return max(intervals) if intervals else None
    if feature.startswith("gk_"):
        return numeric(row.get(feature[3:]))
    return None


def normalize(value: float | None, spec: dict[str, Any]) -> float:
    if value is None:
        return 0.0
    denom = float(spec["max"]) - float(spec["min"])
    if abs(denom) <= 2.220446049250313e-16:
        return 0.0
    normalized = (value - float(spec["min"])) / denom
    if float(spec["direction"]) < 0.0:
        normalized = 1.0 - normalized
    return max(0.0, min(1.0, normalized))


def recompute(row: dict[str, Any], specs: list[dict[str, Any]], thresholds: dict[str, float]) -> dict[str, Any]:
    normalized_values: list[float] = []
    mapped_values: list[float] = []
    mapped_feature_count = 0
    missing_runtime_mapping_count = 0
    missing: list[str] = []
    for spec in specs:
        value = None
        if spec["source"] == "Mapped":
            value = value_for_feature(row, spec["name"])
            if value is not None:
                mapped_feature_count += 1
                mapped_values.append(normalize(value, spec))
        else:
            missing_runtime_mapping_count += 1
        if value is None:
            missing.append(spec["name"])
        normalized_values.append(normalize(value, spec))

    score = sum(normalized_values) / len(specs)
    mapped_only_score = sum(mapped_values) / len(mapped_values) if mapped_values else None
    cutoff_verified = row.get("observation_end_ts_ms") is not None
    core_curve_market_available = (
        row.get("bonding_progress_pct") is not None
        and row.get("current_market_cap_sol") is not None
        and row.get("price_change_ratio") is not None
        and row.get("curve_data_known") is True
    )
    concentration_available = row.get("hhi") is not None and row.get("top3_volume_pct") is not None
    required_missing = sum(
        value_for_feature(row, feature) is None
        for feature in (
            "gk_bonding_progress_pct",
            "gk_current_market_cap_sol",
            "gk_price_change_ratio",
        )
    ) + int(row.get("curve_data_known") is not True)
    if not cutoff_verified:
        validity = "score_invalid_cutoff_unverified"
    elif required_missing > 0 or not core_curve_market_available:
        validity = "score_invalid_missing_core_curve_market"
    elif not concentration_available:
        validity = "score_degraded_missing_concentration"
    else:
        validity = "score_valid"

    return {
        "selector_shadow_score": score,
        "mapped_only_score": mapped_only_score,
        "score_validity_status": validity,
        "thresholds": {key: score >= value for key, value in thresholds.items()},
        "feature_availability": {
            "feature_mapping_status": (
                "complete_runtime_mapping"
                if missing_runtime_mapping_count == 0
                else "partial_runtime_mapping_missing_flow_features"
            ),
            "mapped_feature_count": mapped_feature_count,
            "missing_runtime_mapping_count": missing_runtime_mapping_count,
            "cutoff_verified": cutoff_verified,
            "core_curve_market_available": core_curve_market_available,
            "concentration_available": concentration_available,
            "gk_context_available": mapped_feature_count > 0 and cutoff_verified,
            "flow_available": False,
        },
        "feature_missing_count": len(specs) - mapped_feature_count,
        "required_feature_missing_count": required_missing,
        "missing_features": missing,
    }


def sidecar_candidate_id(decision: dict[str, Any]) -> str:
    return (
        decision.get("execution_candidate_id")
        or decision.get("join_key")
        or decision.get("pool_id")
        or ""
    )


def row_key(row: dict[str, Any]) -> tuple[str, str, str]:
    return (
        str(row.get("candidate_id") or sidecar_candidate_id(row)),
        str(row.get("pool_id") or ""),
        str(row.get("base_mint") or ""),
    )


def claim_boundary_violation(row: dict[str, Any]) -> bool:
    boundaries = row.get("claim_boundaries")
    if not isinstance(boundaries, dict):
        return True
    expected = {
        "diagnostic_only": True,
        "shadow_only": True,
        "production_promotion_allowed": False,
        "gatekeeper_tuning_started": False,
        "changes_gatekeeper_decision": False,
        "changes_execution": False,
        "send_path_changed": False,
    }
    return any(boundaries.get(key) is not value for key, value in expected.items())


def quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, math.ceil(q * len(ordered)) - 1))
    return ordered[index]


def audit_plane(decision_dir: Path, specs: list[dict[str, Any]], thresholds: dict[str, float], tolerance: float) -> dict[str, Any]:
    decisions = read_jsonl(decision_dir / DECISION_FILE)
    scores = read_jsonl(decision_dir / SCORE_FILE)
    matched = min(len(decisions), len(scores))
    score_diffs: list[float] = []
    mapped_only_diffs: list[float] = []
    score_mismatch_rows = 0
    missing_runtime_score_rows = 0
    threshold_mismatch_count = 0
    validity_mismatch_count = 0
    mapping_status_mismatch_count = 0
    key_mismatch_count = 0
    claim_boundary_violation_rows = 0
    status_counts: Counter[str] = Counter()
    mapping_counts: Counter[str] = Counter()
    mismatch_samples: list[dict[str, Any]] = []

    for idx, (decision, score) in enumerate(zip(decisions, scores), 1):
        expected = recompute(decision, specs, thresholds)
        if row_key(decision) != row_key(score):
            key_mismatch_count += 1
        if claim_boundary_violation(score):
            claim_boundary_violation_rows += 1
        emitted_score = score.get("selector_shadow_score")
        if not isinstance(emitted_score, (int, float)) or isinstance(emitted_score, bool):
            missing_runtime_score_rows += 1
        else:
            diff = abs(float(emitted_score) - float(expected["selector_shadow_score"]))
            score_diffs.append(diff)
            if diff > tolerance:
                score_mismatch_rows += 1
                if len(mismatch_samples) < 10:
                    mismatch_samples.append(
                        {
                            "row_index": idx,
                            "candidate_id": row_key(score)[0],
                            "runtime_score": emitted_score,
                            "expected_score": expected["selector_shadow_score"],
                            "diff": diff,
                        }
                    )
            mapped_only = expected["mapped_only_score"]
            if mapped_only is not None:
                mapped_only_diffs.append(abs(float(emitted_score) - float(mapped_only)))

        emitted_thresholds = score.get("thresholds") if isinstance(score.get("thresholds"), dict) else {}
        for key, expected_value in expected["thresholds"].items():
            if emitted_thresholds.get(key) is not expected_value:
                threshold_mismatch_count += 1

        emitted_validity = score.get("score_validity_status")
        status_counts[str(emitted_validity)] += 1
        if emitted_validity != expected["score_validity_status"]:
            validity_mismatch_count += 1

        emitted_availability = (
            score.get("feature_availability")
            if isinstance(score.get("feature_availability"), dict)
            else {}
        )
        mapping = emitted_availability.get("feature_mapping_status")
        mapping_counts[str(mapping)] += 1
        if mapping != expected["feature_availability"]["feature_mapping_status"]:
            mapping_status_mismatch_count += 1

    return {
        "decision_dir": str(decision_dir),
        "decision_rows": len(decisions),
        "score_rows": len(scores),
        "matched_rows": matched,
        "key_mismatch_count": key_mismatch_count,
        "missing_runtime_score_rows": missing_runtime_score_rows,
        "score_mismatch_rows": score_mismatch_rows,
        "score_abs_diff_p50": quantile(score_diffs, 0.50),
        "score_abs_diff_p95": quantile(score_diffs, 0.95),
        "score_abs_diff_max": max(score_diffs) if score_diffs else None,
        "mapped_only_drift_abs_p50": quantile(mapped_only_diffs, 0.50),
        "mapped_only_drift_abs_p95": quantile(mapped_only_diffs, 0.95),
        "mapped_only_drift_abs_max": max(mapped_only_diffs) if mapped_only_diffs else None,
        "threshold_pass_mismatch_count": threshold_mismatch_count,
        "validity_status_mismatch_count": validity_mismatch_count,
        "mapping_status_mismatch_count": mapping_status_mismatch_count,
        "claim_boundary_violation_rows": claim_boundary_violation_rows,
        "score_validity_status_counts": dict(status_counts),
        "feature_mapping_status_counts": dict(mapping_counts),
        "mismatch_samples": mismatch_samples,
    }


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    rust_source = root / args.rust_source
    specs, thresholds = parse_runtime_spec(rust_source)
    dirs = decision_dirs(root, args.runtime_scope, args.decision_plane)
    planes = [audit_plane(path, specs, thresholds, args.tolerance) for path in dirs]

    fail_reasons: list[str] = []
    if not dirs:
        fail_reasons.append("no_decision_dirs_found")

    totals = Counter()
    for plane in planes:
        for key in (
            "decision_rows",
            "score_rows",
            "matched_rows",
            "key_mismatch_count",
            "missing_runtime_score_rows",
            "score_mismatch_rows",
            "threshold_pass_mismatch_count",
            "validity_status_mismatch_count",
            "mapping_status_mismatch_count",
            "claim_boundary_violation_rows",
        ):
            totals[key] += int(plane[key])

    if totals["matched_rows"] <= 0:
        fail_reasons.append("matched_rows=0")
    if totals["key_mismatch_count"]:
        fail_reasons.append(f"key_mismatch_count={totals['key_mismatch_count']}")
    if totals["missing_runtime_score_rows"]:
        fail_reasons.append(f"missing_runtime_score_rows={totals['missing_runtime_score_rows']}")
    if totals["score_mismatch_rows"]:
        fail_reasons.append(f"score_mismatch_rows={totals['score_mismatch_rows']}")
    if totals["threshold_pass_mismatch_count"]:
        fail_reasons.append(
            f"threshold_pass_mismatch_count={totals['threshold_pass_mismatch_count']}"
        )
    if totals["validity_status_mismatch_count"]:
        fail_reasons.append(
            f"validity_status_mismatch_count={totals['validity_status_mismatch_count']}"
        )
    if totals["mapping_status_mismatch_count"]:
        fail_reasons.append(
            f"mapping_status_mismatch_count={totals['mapping_status_mismatch_count']}"
        )
    if totals["claim_boundary_violation_rows"]:
        fail_reasons.append(
            f"claim_boundary_violation_rows={totals['claim_boundary_violation_rows']}"
        )

    all_diffs = [
        plane["score_abs_diff_max"]
        for plane in planes
        if plane["score_abs_diff_max"] is not None
    ]
    missing_runtime_mapping_features = [
        spec["name"] for spec in specs if spec["source"] != "Mapped"
    ]
    mapped_only_drift = (
        {
            "status": "NO_RUNTIME_MAPPING_DRIFT_FULL_MAPPING_AVAILABLE",
            "meaning": "runtime score spec declares all frozen selector features as mapped; emitted runtime score is expected to match the recomputed runtime contract",
        }
        if not missing_runtime_mapping_features
        else {
            "status": "DRIFT_MEASURED_EXPECTED_MISSING_FLOW_FEATURES",
            "meaning": "runtime score keeps missing flow mappings as zero contribution in the frozen full denominator; mapped-only score is diagnostic drift, not the emitted contract",
        }
    )
    report = {
        "artifact": ARTIFACT,
        "status": "PASS" if not fail_reasons else "FAIL",
        "runtime_scope": args.runtime_scope,
        "decision_plane_filter": args.decision_plane,
        "rust_source": str(rust_source),
        "score_version": EXPECTED_SCORE_VERSION,
        "score_candidate_id": EXPECTED_CANDIDATE_ID,
        "runtime_formula_parity": {
            "status": "PASS" if not fail_reasons else "FAIL",
            "tolerance": args.tolerance,
            "matched_rows": totals["matched_rows"],
            "score_mismatch_rows": totals["score_mismatch_rows"],
            "threshold_pass_mismatch_count": totals["threshold_pass_mismatch_count"],
            "validity_status_mismatch_count": totals["validity_status_mismatch_count"],
            "mapping_status_mismatch_count": totals["mapping_status_mismatch_count"],
            "max_score_abs_diff": max(all_diffs) if all_diffs else None,
        },
        "mapped_only_drift": mapped_only_drift,
        "feature_spec": {
            "total_features": len(specs),
            "mapped_features": sum(1 for spec in specs if spec["source"] == "Mapped"),
            "missing_runtime_mapping_features": missing_runtime_mapping_features,
        },
        "decision_rows": totals["decision_rows"],
        "score_rows": totals["score_rows"],
        "matched_rows": totals["matched_rows"],
        "key_mismatch_count": totals["key_mismatch_count"],
        "missing_runtime_score_rows": totals["missing_runtime_score_rows"],
        "claim_boundary_violation_rows": totals["claim_boundary_violation_rows"],
        "planes": planes,
        "fail_reasons": fail_reasons,
    }
    output = Path(args.output) if args.output else default_output(root, args.runtime_scope)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    report["output"] = str(output)
    return report


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            f"{report['status']} matched={report['matched_rows']} "
            f"mismatches={report['runtime_formula_parity']['score_mismatch_rows']} "
            f"output={report['output']}"
        )
        for reason in report["fail_reasons"]:
            print(f"FAIL_REASON {reason}")
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
