#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
import random
import shutil
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable

import v3_p36_calibration_report as calibration


REPO_ROOT = Path(__file__).resolve().parents[1]
LEGACY_ANALYZER = REPO_ROOT / "logs/decisions.json/analiza_porownawcza.py"
CANDIDATE_VARIANT = calibration.CANDIDATE_VARIANT
EXCLUDED_BY_ADR_0130 = [
    "funding_source_concentration",
    "funding_source_diagnostics",
]

TX_INTEL_FIELDS = [
    "tx_count",
    "buy_count",
    "unique_signers",
    "buy_ratio",
    "hhi",
    "top3_volume_pct",
    "same_ms_tx_ratio",
    "bundle_suspicion_ratio",
    "dev_volume_ratio",
    "max_tx_per_signer",
    "total_volume_sol",
    "avg_tx_sol",
    "volume_cv",
]
ORGANIC_FIELDS = [
    "buy_ratio_min",
    "buy_ratio_mean",
    "buy_ratio_max",
    "tx_count_growth_ratio",
    "unique_signer_growth_ratio",
    "new_signer_ratio_t2",
    "hhi_delta_t2_t0",
    "max_segment_hhi",
    "t1_vs_t0_unique_signer_delta",
    "t2_vs_t1_unique_signer_delta",
]
MANIP_FIELDS = [
    "same_ms_tx_ratio",
    "bundle_suspicion_ratio",
    "top3_volume_pct",
    "hhi",
    "dev_volume_ratio",
    "contradiction_score",
    "timing_bundle_concentration",
    "high_buy_pressure_with_high_top3",
    "fixed_size_or_ramping_pattern",
    "early_top3_concentration",
]
TAS_FIELDS = [
    "overall_tas_score",
    "momentum_score",
    "hhi_score",
    "volume_score",
    "interval_score",
    "buy_ratio_score",
]
ALPHA_FIELDS = [
    "fixed_size_buy_ratio",
    "flipper_presence_ratio",
    "compute_unit_cluster_dominance",
    "static_fee_profile_ratio",
    "jito_tip_intensity",
    "early_top3_buy_volume_pct_3s",
]
NON_FSC_SYBIL_FIELDS = [
    "fee_topology_diversity_index",
    "signer_cross_pool_velocity",
    "spend_fraction_divergence",
    "demand_elasticity_score",
]


@dataclass
class JoinedRow:
    run_name: str
    source: dict[str, Any]
    quality: dict[str, Any]


@dataclass
class Comparison:
    name: str
    rows_a: list[JoinedRow]
    rows_b: list[JoinedRow]
    neutral_excluded: int
    unknown_excluded: int
    runs_included: list[str]
    description: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="P3.6 decision-time-safe feature separation audit."
    )
    parser.add_argument(
        "--run",
        action="append",
        required=True,
        help="Run spec: name:<rollout_config>:<outcome_labels_jsonl>",
    )
    parser.add_argument(
        "--comparison",
        action="append",
        default=[],
        help=(
            "Comparison name. Use 'all' for the mandatory P3.6 set. "
            "Supported: good_vs_bad, variant_unblocked, reason_reject_manipulation, "
            "reason_pending_wait_evidence, organic_failures, per_run_good_vs_bad."
        ),
    )
    parser.add_argument("--variant", default=CANDIDATE_VARIANT)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--markdown", action="store_true")
    return parser.parse_args()


def label_of(row: JoinedRow) -> str:
    return calibration.label_for_quality_row(row.quality)


def reason_of(row: JoinedRow) -> str:
    return str(row.source.get("v3_shadow_reason_code") or "")


def ab_id_of(row: JoinedRow) -> str:
    return str(row.source.get("ab_record_id") or row.quality.get("ab_record_id") or "")


def as_number(value: Any) -> float | None:
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    if isinstance(value, (int, float)):
        value = float(value)
        return value if math.isfinite(value) else None
    return None


def get_nested(mapping: dict[str, Any], *path: str) -> Any:
    value: Any = mapping
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def first_number(*values: Any) -> float | None:
    for value in values:
        converted = as_number(value)
        if converted is not None:
            return converted
    return None


def snapshot(row: dict[str, Any]) -> dict[str, Any]:
    value = row.get("v3_materialized_feature_snapshot")
    return value if isinstance(value, dict) else {}


def tx_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = snapshot(row).get("tx_intel_features")
    return value if isinstance(value, dict) else {}


def organic_payload(row: dict[str, Any]) -> dict[str, Any]:
    return calibration.organic_payload(row)


def manip_payload(row: dict[str, Any]) -> dict[str, Any]:
    return calibration.manip_payload(row)


def alpha_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = snapshot(row).get("alpha_fingerprint")
    return value if isinstance(value, dict) else {}


def sybil_payload(row: dict[str, Any]) -> dict[str, Any]:
    value = snapshot(row).get("sybil_resistance")
    return value if isinstance(value, dict) else {}


def flatten_features(joined: JoinedRow, *, collection: str) -> dict[str, Any]:
    row = joined.source
    tx = tx_payload(row)
    organic = organic_payload(row)
    manip = manip_payload(row)
    alpha = alpha_payload(row)
    sybil = sybil_payload(row)

    out: dict[str, Any] = {
        "origin": "v3_p36_feature_separation_audit",
        "comparison_collection": collection,
        "run_name": joined.run_name,
        "pool_id": row.get("pool_id"),
        "base_mint": row.get("base_mint"),
        "ab_record_id": ab_id_of(joined),
        "ab_window_complete": row.get("ab_window_complete", True),
        "ab_window_ms": row.get("ab_window_ms", 0),
        "ab_tx_count_window": row.get("ab_tx_count_window", 0),
        "ab_unique_signers_window": row.get("ab_unique_signers_window", 0),
        "outcome_label": label_of(joined),
        "v3_shadow_reason_code": reason_of(joined),
        "v3_shadow_verdict": row.get("v3_shadow_verdict"),
        "v3_shadow_stage": row.get("v3_shadow_stage"),
    }

    for field in TX_INTEL_FIELDS:
        value = first_number(row.get(field), tx.get(field))
        if value is not None:
            out[field] = value
            out[f"tx_intel_{field}"] = value
    for field in ORGANIC_FIELDS:
        value = first_number(row.get(field), organic.get(field))
        if value is not None:
            out[f"organic_{field}"] = value
            out.setdefault(field, value)
    for field in MANIP_FIELDS:
        value = first_number(row.get(field), manip.get(field))
        if value is not None:
            out[f"manip_{field}"] = value
            out.setdefault(field, value)
    tas_aliases = {
        "overall_tas_score": ["overall_tas_score", "tas_overall_score"],
        "momentum_score": ["momentum_score", "tas_momentum_score"],
        "hhi_score": ["hhi_score", "tas_hhi_score"],
        "volume_score": ["volume_score", "tas_volume_score"],
        "interval_score": ["interval_score", "tas_interval_score"],
        "buy_ratio_score": ["buy_ratio_score", "tas_buy_ratio_score"],
    }
    for field in TAS_FIELDS:
        value = first_number(*(row.get(alias) for alias in tas_aliases[field]))
        if value is not None:
            out[field] = value
            out[f"tas_{field}"] = value
    for field in ALPHA_FIELDS:
        value = first_number(row.get(field), alpha.get(field))
        if value is not None:
            out[field] = value
            out[f"alpha_{field}"] = value
    for field in NON_FSC_SYBIL_FIELDS:
        value = first_number(row.get(field), sybil.get(field), manip.get(field))
        if value is not None:
            out[field] = value
            out[f"sybil_{field}"] = value

    return out


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    count = 0
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
            count += 1
    return count


def load_runs(run_specs: list[str]) -> tuple[list[dict[str, Any]], list[JoinedRow]]:
    loaded: list[dict[str, Any]] = []
    joined: list[JoinedRow] = []
    seen: set[str] = set()
    for spec in run_specs:
        name, config_path, labels_path = calibration.parse_run_spec(spec)
        run = calibration.load_run(name, config_path, labels_path)
        run["quality_by_ab"] = calibration.row_by_ab_record_id(run["quality_rows"])
        run["ablation"] = calibration.run_full_replay_ablation(Path(run["decisions_log"]))
        rows_by_ab = calibration.row_by_ab_record_id(run["v3_rows"])
        for quality in run["quality_rows"]:
            ab_id = str(quality.get("ab_record_id") or "")
            if not ab_id or ab_id in seen:
                continue
            source = rows_by_ab.get(ab_id)
            if source is None:
                continue
            seen.add(ab_id)
            joined.append(JoinedRow(run["name"], source, quality))
        loaded.append(run)
    return loaded, joined


def label_partition(
    rows: list[JoinedRow],
    predicate: Callable[[JoinedRow], bool] | None = None,
) -> tuple[list[JoinedRow], list[JoinedRow], int, int]:
    rows_a: list[JoinedRow] = []
    rows_b: list[JoinedRow] = []
    neutral = 0
    unknown = 0
    for row in rows:
        if predicate is not None and not predicate(row):
            continue
        label = label_of(row)
        if label == "good_entry":
            rows_a.append(row)
        elif label == "bad_entry":
            rows_b.append(row)
        elif label == "neutral_entry":
            neutral += 1
        else:
            unknown += 1
    return rows_a, rows_b, neutral, unknown


def unblocked_ids(run: dict[str, Any], variant_name: str) -> tuple[set[str], Counter[str]]:
    variant = run.get("ablation", {}).get("variants", {}).get(variant_name, {})
    deltas = variant.get("row_deltas")
    if not isinstance(deltas, list):
        return set(), Counter({"missing_row_deltas": 1})
    ids: set[str] = set()
    labels: Counter[str] = Counter()
    quality_by_ab = run.get("quality_by_ab", {})
    for delta in deltas:
        if not isinstance(delta, dict):
            continue
        baseline_block = calibration.is_blocking_verdict(delta.get("baseline_verdict"))
        variant_block = calibration.is_blocking_verdict(delta.get("variant_verdict"))
        if baseline_block and not variant_block:
            ab_id = str(delta.get("ab_record_id") or "")
            if ab_id:
                ids.add(ab_id)
                labels[calibration.label_for_quality_row(quality_by_ab.get(ab_id, {}))] += 1
    return ids, labels


def build_comparisons(
    loaded_runs: list[dict[str, Any]],
    rows: list[JoinedRow],
    requested: list[str],
    variant_name: str,
) -> list[Comparison]:
    if not requested or "all" in requested:
        requested = [
            "good_vs_bad",
            "variant_unblocked",
            "reason_reject_manipulation",
            "reason_pending_wait_evidence",
            "organic_failures",
            "per_run_good_vs_bad",
        ]
    comparisons: list[Comparison] = []
    all_runs = sorted({row.run_name for row in rows})

    if "good_vs_bad" in requested:
        a, b, neutral, unknown = label_partition(rows)
        comparisons.append(
            Comparison(
                "good_vs_bad_all",
                a,
                b,
                neutral,
                unknown,
                all_runs,
                "good_entry vs bad_entry across all included runs",
            )
        )
        recent = [row for row in rows if row.run_name.lower() != "r10"]
        if recent and len({row.run_name for row in recent}) != len(all_runs):
            a, b, neutral, unknown = label_partition(recent)
            comparisons.append(
                Comparison(
                    "good_vs_bad_recent_no_r10",
                    a,
                    b,
                    neutral,
                    unknown,
                    sorted({row.run_name for row in recent}),
                    "good_entry vs bad_entry on recent runs excluding R10",
                )
            )

    if "variant_unblocked" in requested:
        ids: set[str] = set()
        for run in loaded_runs:
            run_ids, _ = unblocked_ids(run, variant_name)
            ids.update(run_ids)
        a, b, neutral, unknown = label_partition(rows, lambda row: ab_id_of(row) in ids)
        comparisons.append(
            Comparison(
                f"{variant_name}_good_unblocked_vs_bad_unblocked",
                a,
                b,
                neutral,
                unknown,
                all_runs,
                f"{variant_name}: good_unblocked vs bad_unblocked",
            )
        )

    if "reason_reject_manipulation" in requested:
        a, b, neutral, unknown = label_partition(
            rows,
            lambda row: reason_of(row) == "REJECT_V3_MANIPULATION_CONTRADICTION",
        )
        comparisons.append(
            Comparison(
                "reject_manipulation_contradiction_good_vs_bad",
                a,
                b,
                neutral,
                unknown,
                all_runs,
                "REJECT_V3_MANIPULATION_CONTRADICTION good vs bad",
            )
        )

    if "reason_pending_wait_evidence" in requested:
        a, b, neutral, unknown = label_partition(
            rows,
            lambda row: reason_of(row) == "PENDING_V3_WAIT_EVIDENCE",
        )
        comparisons.append(
            Comparison(
                "pending_wait_evidence_good_vs_bad",
                a,
                b,
                neutral,
                unknown,
                all_runs,
                "PENDING_V3_WAIT_EVIDENCE good vs bad",
            )
        )

    if "organic_failures" in requested:
        a, b, neutral, unknown = label_partition(
            rows,
            lambda row: calibration.organic_failure_reasons(row.source) != ["organic_passes"],
        )
        comparisons.append(
            Comparison(
                "organic_failure_groups_good_vs_bad",
                a,
                b,
                neutral,
                unknown,
                all_runs,
                "Rows with organic failure groups: good vs bad",
            )
        )

    if "per_run_good_vs_bad" in requested:
        for run_name in all_runs:
            run_rows = [row for row in rows if row.run_name == run_name]
            a, b, neutral, unknown = label_partition(run_rows)
            comparisons.append(
                Comparison(
                    f"good_vs_bad_{run_name}",
                    a,
                    b,
                    neutral,
                    unknown,
                    [run_name],
                    f"{run_name} standalone good_entry vs bad_entry",
                )
            )

    return comparisons


def numeric_values(rows: list[dict[str, Any]], feature: str) -> list[float]:
    values: list[float] = []
    for row in rows:
        value = as_number(row.get(feature))
        if value is not None:
            values.append(value)
    return values


def mean(values: list[float]) -> float | None:
    return sum(values) / len(values) if values else None


def median(values: list[float]) -> float | None:
    if not values:
        return None
    values = sorted(values)
    mid = len(values) // 2
    if len(values) % 2:
        return values[mid]
    return (values[mid - 1] + values[mid]) / 2.0


def stdev(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    avg = sum(values) / len(values)
    return math.sqrt(sum((value - avg) ** 2 for value in values) / (len(values) - 1))


def auc_rank(values_a: list[float], values_b: list[float]) -> float | None:
    if not values_a or not values_b:
        return None
    wins = ties = 0.0
    sorted_b = sorted(values_b)
    for value in values_a:
        less = sum(1 for other in sorted_b if other < value)
        equal = sum(1 for other in sorted_b if other == value)
        wins += less
        ties += equal
    return (wins + 0.5 * ties) / (len(values_a) * len(values_b))


def overlap(values_a: list[float], values_b: list[float], bins: int = 40) -> float | None:
    if not values_a or not values_b:
        return None
    lo = min(values_a + values_b)
    hi = max(values_a + values_b)
    if hi == lo:
        return 1.0
    width = (hi - lo) / bins
    hist_a = [0] * bins
    hist_b = [0] * bins
    for value in values_a:
        hist_a[min(int((value - lo) / width), bins - 1)] += 1
    for value in values_b:
        hist_b[min(int((value - lo) / width), bins - 1)] += 1
    return sum(min(a / len(values_a), b / len(values_b)) for a, b in zip(hist_a, hist_b))


def bootstrap_mean_delta(values_a: list[float], values_b: list[float]) -> dict[str, Any]:
    if len(values_a) < 20 or len(values_b) < 20:
        return {"status": "insufficient_sample"}
    rng = random.Random(42)
    deltas: list[float] = []
    for _ in range(250):
        sample_a = [rng.choice(values_a) for _ in values_a]
        sample_b = [rng.choice(values_b) for _ in values_b]
        deltas.append((mean(sample_a) or 0.0) - (mean(sample_b) or 0.0))
    deltas.sort()
    lo = deltas[int(len(deltas) * 0.025)]
    hi = deltas[min(len(deltas) - 1, int(len(deltas) * 0.975))]
    status = "directional" if (lo > 0 and hi > 0) or (lo < 0 and hi < 0) else "crosses_zero"
    return {"status": status, "ci95_low": round(lo, 6), "ci95_high": round(hi, 6)}


def feature_metrics(rows_a: list[dict[str, Any]], rows_b: list[dict[str, Any]]) -> dict[str, Any]:
    feature_names = sorted(
        {
            key
            for row in rows_a + rows_b
            for key, value in row.items()
            if as_number(value) is not None
            and key
            not in {
                "ab_window_complete",
            }
            and key not in EXCLUDED_BY_ADR_0130
        }
    )
    metrics: list[dict[str, Any]] = []
    for feature in feature_names:
        values_a = numeric_values(rows_a, feature)
        values_b = numeric_values(rows_b, feature)
        if len(values_a) < 5 or len(values_b) < 5:
            continue
        avg_a = mean(values_a)
        avg_b = mean(values_b)
        if avg_a is None or avg_b is None:
            continue
        pooled = math.sqrt((stdev(values_a) ** 2 + stdev(values_b) ** 2) / 2.0)
        mean_delta = avg_a - avg_b
        standardized_delta = mean_delta / pooled if pooled > 1e-12 else 0.0
        auc = auc_rank(values_a, values_b)
        ovl = overlap(values_a, values_b)
        metrics.append(
            {
                "feature": feature,
                "n_A": len(values_a),
                "n_B": len(values_b),
                "mean_A": round(avg_a, 6),
                "mean_B": round(avg_b, 6),
                "median_A": round(median(values_a) or 0.0, 6),
                "median_B": round(median(values_b) or 0.0, 6),
                "mean_delta_A_minus_B": round(mean_delta, 6),
                "standardized_delta": round(standardized_delta, 6),
                "auc_A_gt_B": round(auc, 6) if auc is not None else None,
                "auc_separation": round(abs((auc or 0.5) - 0.5), 6) if auc is not None else None,
                "overlap": round(ovl, 6) if ovl is not None else None,
                "bootstrap_ci": bootstrap_mean_delta(values_a, values_b),
            }
        )
    return {
        "top_feature_deltas": sorted(
            metrics, key=lambda item: abs(item["standardized_delta"]), reverse=True
        )[:20],
        "auc_ranking": sorted(
            metrics, key=lambda item: item["auc_separation"] or 0.0, reverse=True
        )[:20],
        "overlap": sorted(
            [item for item in metrics if item.get("overlap") is not None],
            key=lambda item: item["overlap"],
        )[:20],
    }


def stability_by_run(
    comparison: Comparison,
    top_features: list[str],
) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for run_name in comparison.runs_included:
        rows_a = [
            flatten_features(row, collection="A")
            for row in comparison.rows_a
            if row.run_name == run_name
        ]
        rows_b = [
            flatten_features(row, collection="B")
            for row in comparison.rows_b
            if row.run_name == run_name
        ]
        feature_dirs: dict[str, Any] = {}
        for feature in top_features:
            values_a = numeric_values(rows_a, feature)
            values_b = numeric_values(rows_b, feature)
            if len(values_a) < 5 or len(values_b) < 5:
                continue
            avg_a = mean(values_a) or 0.0
            avg_b = mean(values_b) or 0.0
            auc = auc_rank(values_a, values_b)
            feature_dirs[feature] = {
                "n_A": len(values_a),
                "n_B": len(values_b),
                "mean_delta_A_minus_B": round(avg_a - avg_b, 6),
                "auc_A_gt_B": round(auc, 6) if auc is not None else None,
                "direction": "A_higher" if avg_a > avg_b else "B_higher" if avg_b > avg_a else "flat",
            }
        result[run_name] = {
            "n_A": len(rows_a),
            "n_B": len(rows_b),
            "feature_directions": feature_dirs,
        }
    return result


def run_legacy_analyzer(path_a: Path, path_b: Path, output_dir: Path) -> dict[str, Any]:
    before = set(output_dir.glob("analiza_*.html"))
    env = os.environ.copy()
    env.setdefault("AB_MIN_TX", "0")
    env.setdefault("AB_MIN_VEC_LEN", "0")
    completed = subprocess.run(
        ["python3", str(LEGACY_ANALYZER), str(path_a), str(path_b)],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
        env=env,
    )
    stdout_path = output_dir / "analiza_stdout.txt"
    stdout_path.write_text(completed.stdout + completed.stderr, encoding="utf-8")
    after = set(output_dir.glob("analiza_*.html"))
    new_html = sorted(after - before)
    return {
        "status": "ok" if completed.returncode == 0 else "failed",
        "exit_code": completed.returncode,
        "stdout_path": str(stdout_path),
        "html_path": str(new_html[-1]) if new_html else None,
    }


def render_markdown(summary: dict[str, Any]) -> str:
    lines = [
        f"# {summary['comparison_name']}",
        "",
        f"- status: `{summary['status']}`",
        f"- runs: `{', '.join(summary['runs_included'])}`",
        f"- n_A: `{summary['n_A']}`",
        f"- n_B: `{summary['n_B']}`",
        f"- neutral_excluded: `{summary['neutral_excluded']}`",
        f"- unknown_excluded: `{summary['unknown_excluded']}`",
        f"- hypothesis_only: `{summary['hypothesis_only']}`",
        f"- threshold_recommendation_allowed: `{summary['threshold_recommendation_allowed']}`",
        "",
        "## Top Feature Deltas",
    ]
    for item in summary["top_feature_deltas"][:10]:
        lines.append(
            f"- `{item['feature']}`: delta={item['mean_delta_A_minus_B']}, "
            f"std_delta={item['standardized_delta']}, auc={item['auc_A_gt_B']}, "
            f"overlap={item['overlap']}"
        )
    lines.extend(["", "## AUC Ranking"])
    for item in summary["auc_ranking"][:10]:
        lines.append(
            f"- `{item['feature']}`: auc={item['auc_A_gt_B']}, "
            f"sep={item['auc_separation']}, n_A={item['n_A']}, n_B={item['n_B']}"
        )
    lines.extend(["", "## Governance"])
    lines.append("- FSC fields are excluded from feature ranking under ADR-0130.")
    lines.append("- Legacy analyzer threshold sections are appendix-only.")
    lines.append("- No runtime threshold recommendation is allowed from this audit.")
    lines.append("")
    return "\n".join(lines)


def process_comparison(
    comparison: Comparison,
    output_root: Path,
    *,
    markdown: bool,
) -> dict[str, Any]:
    output_dir = output_root / comparison.name
    output_dir.mkdir(parents=True, exist_ok=True)
    rows_a = [flatten_features(row, collection="A") for row in comparison.rows_a]
    rows_b = [flatten_features(row, collection="B") for row in comparison.rows_b]
    path_a = output_dir / "A_good_entry.jsonl"
    path_b = output_dir / "B_bad_entry.jsonl"
    if "unblocked" in comparison.name:
        path_a = output_dir / "A_good_unblocked.jsonl"
        path_b = output_dir / "B_bad_unblocked.jsonl"
    write_jsonl(path_a, rows_a)
    write_jsonl(path_b, rows_b)

    metrics = feature_metrics(rows_a, rows_b)
    top_features = [item["feature"] for item in metrics["auc_ranking"][:10]]
    sample_warning = len(rows_a) < 50 or len(rows_b) < 50
    summary = {
        "status": "hypothesis_only" if sample_warning else "ok",
        "comparison_name": comparison.name,
        "description": comparison.description,
        "runs_included": comparison.runs_included,
        "n_A": len(rows_a),
        "n_B": len(rows_b),
        "neutral_excluded": comparison.neutral_excluded,
        "unknown_excluded": comparison.unknown_excluded,
        "sample_size_warning": sample_warning,
        "hypothesis_only": sample_warning,
        "threshold_recommendation_allowed": False,
        "fsc_excluded_by_adr_0130": EXCLUDED_BY_ADR_0130,
        "top_feature_deltas": metrics["top_feature_deltas"],
        "auc_ranking": metrics["auc_ranking"],
        "overlap": metrics["overlap"],
        "bootstrap_ci_status": Counter(
            str((item.get("bootstrap_ci") or {}).get("status") or "missing")
            for item in metrics["top_feature_deltas"]
        ),
        "stability_by_run": stability_by_run(comparison, top_features),
        "legacy_analyzer": run_legacy_analyzer(path_a, path_b, output_dir),
        "input_files": {
            "A": str(path_a),
            "B": str(path_b),
        },
        "warnings": [
            "Legacy analyzer threshold/Youden/logistic/scoring sections are appendix-only.",
            "This audit is feature-separation evidence, not runtime tuning.",
        ],
    }
    summary["bootstrap_ci_status"] = dict(summary["bootstrap_ci_status"])

    summary_path = output_dir / "comparison_summary.json"
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
    if markdown:
        (output_dir / "comparison_summary.md").write_text(
            render_markdown(summary), encoding="utf-8"
        )
    return summary


def main() -> None:
    args = parse_args()
    output_root = calibration.resolve_path(args.output_dir)
    output_root.mkdir(parents=True, exist_ok=True)
    loaded_runs, rows = load_runs(args.run)
    comparisons = build_comparisons(loaded_runs, rows, args.comparison, args.variant)
    summaries = [
        process_comparison(comparison, output_root, markdown=args.markdown)
        for comparison in comparisons
    ]
    index = {
        "status": "ok" if summaries else "no_comparisons",
        "runs": [run["name"] for run in loaded_runs],
        "comparisons": [
            {
                "comparison_name": summary["comparison_name"],
                "status": summary["status"],
                "n_A": summary["n_A"],
                "n_B": summary["n_B"],
                "sample_size_warning": summary["sample_size_warning"],
                "summary_path": str(output_root / summary["comparison_name"] / "comparison_summary.json"),
            }
            for summary in summaries
        ],
        "threshold_recommendation_allowed": False,
        "no_active_policy_change": True,
        "no_p2_promotion": True,
        "fsc_excluded_by_adr_0130": EXCLUDED_BY_ADR_0130,
    }
    (output_root / "feature_separation_index.json").write_text(
        json.dumps(index, indent=2, sort_keys=True), encoding="utf-8"
    )
    if args.json:
        print(json.dumps(index, indent=2, sort_keys=True))
    else:
        print(f"status={index['status']}")
        for comparison in index["comparisons"]:
            print(
                f"{comparison['comparison_name']}: status={comparison['status']} "
                f"n_A={comparison['n_A']} n_B={comparison['n_B']}"
            )


if __name__ == "__main__":
    main()
