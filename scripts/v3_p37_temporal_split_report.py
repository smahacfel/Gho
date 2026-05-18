#!/usr/bin/env python3
"""
Build P3.7 temporal split baseline report.

This report consumes already-generated P3.7 label v2 and execution feasibility
join artifacts. It is not a feature prototype and does not recommend thresholds.
Combined R10/R11/R13 is secondary; R11 and R13 standalone are required views.
"""

from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path
from statistics import median
from typing import Any, Iterable


NUMERIC_FIELDS = [
    "mfe_pct_10s",
    "mfe_pct_30s",
    "mfe_pct_60s",
    "mae_pct_10s",
    "mae_pct_30s",
    "mae_pct_60s",
    "time_to_mfe_ms",
    "time_to_mae_ms",
]


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    with path.open(encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            obj = json.loads(raw)
            if isinstance(obj, dict):
                yield obj


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def rate(count: int, total: int) -> float:
    return round(count / total, 6) if total else 0.0


def numeric(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def percentile(values: list[float], pct: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    if len(ordered) == 1:
        return round(ordered[0], 6)
    pos = (len(ordered) - 1) * pct
    lo = math.floor(pos)
    hi = math.ceil(pos)
    if lo == hi:
        return round(ordered[lo], 6)
    frac = pos - lo
    return round(ordered[lo] * (1.0 - frac) + ordered[hi] * frac, 6)


def summarize_values(rows: list[dict[str, Any]], field: str) -> dict[str, Any]:
    values = [value for row in rows if (value := numeric(row.get(field))) is not None]
    return {
        "available": len(values),
        "available_share": rate(len(values), len(rows)),
        "median": round(median(values), 6) if values else None,
        "p75": percentile(values, 0.75),
    }


def read_run(name: str, label_path: Path, feasibility_path: Path) -> dict[str, Any]:
    labels = list(iter_jsonl(label_path))
    feasibility = list(iter_jsonl(feasibility_path))
    return build_run_metrics(name, labels, feasibility, str(label_path), str(feasibility_path))


def build_run_metrics(
    name: str,
    labels: list[dict[str, Any]],
    feasibility: list[dict[str, Any]],
    label_path: str,
    feasibility_path: str,
) -> dict[str, Any]:
    total = len(labels)
    market = Counter(str(row.get("market_outcome_class") or "unknown") for row in labels)
    quality = Counter(str(row.get("label_quality") or "unknown") for row in labels)
    price_path = Counter(str(row.get("price_path_source") or "unknown") for row in labels)
    execution = Counter(str(row.get("execution_quality_class") or "unknown") for row in feasibility)
    decision_quality = Counter(str(row.get("decision_quality_class") or "unknown") for row in feasibility)
    dispatch_expected = sum(1 for row in feasibility if row.get("dispatch_expected") is True)
    shadow_observed = sum(1 for row in feasibility if row.get("shadow_dispatch_observed") is True)

    return {
        "name": name,
        "label_path": label_path,
        "feasibility_path": feasibility_path,
        "rows": total,
        "feasibility_rows": len(feasibility),
        "market_outcome_class_counts": counter_dict(market),
        "label_quality_counts": counter_dict(quality),
        "price_path_source_counts": counter_dict(price_path),
        "execution_quality_class_counts": counter_dict(execution),
        "decision_quality_class_counts": counter_dict(decision_quality),
        "rates": {
            "good_clean": rate(market.get("good_clean", 0), total),
            "good_dirty": rate(market.get("good_dirty", 0), total),
            "bad_clean": rate(market.get("bad_clean", 0), total),
            "neutral_clean": rate(market.get("neutral_clean", 0), total),
            "unknown": rate(market.get("unknown", 0), total),
            "execution_feasible": rate(
                execution.get("execution_feasible_clean", 0)
                + execution.get("execution_feasible_degraded", 0),
                len(feasibility),
            ),
            "good_executable": rate(decision_quality.get("good_executable", 0), len(feasibility)),
            "good_not_executable": rate(
                decision_quality.get("good_not_executable", 0), len(feasibility)
            ),
            "price_path_available": rate(total - price_path.get("none", 0), total),
        },
        "dispatch": {
            "dispatch_expected_rows": dispatch_expected,
            "shadow_dispatch_observed_rows": shadow_observed,
            "dispatch_observed_without_expected_rows": sum(
                1
                for row in feasibility
                if row.get("shadow_dispatch_observed") is True
                and row.get("dispatch_expected") is not True
            ),
            "dispatch_expected_without_observed_rows": sum(
                1
                for row in feasibility
                if row.get("dispatch_expected") is True
                and row.get("shadow_dispatch_observed") is not True
            ),
        },
        "numeric_summaries": {
            field: summarize_values(labels, field) for field in NUMERIC_FIELDS
        },
    }


def aggregate_runs(name: str, runs: list[dict[str, Any]]) -> dict[str, Any]:
    labels: list[dict[str, Any]] = []
    feasibility: list[dict[str, Any]] = []
    for run in runs:
        labels.extend(list(iter_jsonl(Path(run["label_path"]))))
        feasibility.extend(list(iter_jsonl(Path(run["feasibility_path"]))))
    return build_run_metrics(name, labels, feasibility, "aggregate", "aggregate")


def rate_diff_ci(run_a: dict[str, Any], run_b: dict[str, Any], metric: str) -> dict[str, Any]:
    n_a = int(run_a["rows"])
    n_b = int(run_b["rows"])
    p_a = float(run_a["rates"].get(metric, 0.0))
    p_b = float(run_b["rates"].get(metric, 0.0))
    diff = p_b - p_a
    se = math.sqrt((p_a * (1.0 - p_a) / n_a) + (p_b * (1.0 - p_b) / n_b)) if n_a and n_b else 0.0
    low = diff - 1.96 * se
    high = diff + 1.96 * se
    return {
        "r11_rate": round(p_a, 6),
        "r13_rate": round(p_b, 6),
        "r13_minus_r11": round(diff, 6),
        "ci95_low": round(low, 6),
        "ci95_high": round(high, 6),
        "ci95_crosses_zero": low <= 0.0 <= high,
    }


def temporal_gate(r11: dict[str, Any], r13: dict[str, Any], combined: dict[str, Any]) -> dict[str, Any]:
    blockers: list[str] = []
    if r11["market_outcome_class_counts"].get("good_clean", 0) == 0:
        blockers.append("r11_has_no_good_clean_rows")
    if r13["market_outcome_class_counts"].get("good_clean", 0) == 0:
        blockers.append("r13_has_no_good_clean_rows")
    if r11["decision_quality_class_counts"].get("good_executable", 0) == 0:
        blockers.append("r11_has_no_good_executable_rows")
    if r13["decision_quality_class_counts"].get("good_executable", 0) == 0:
        blockers.append("r13_has_no_good_executable_rows")
    if combined["rates"].get("price_path_available", 0.0) == 0.0:
        blockers.append("mfe_mae_unavailable_all_runs")
    if combined["market_outcome_class_counts"].get("good_clean", 0) > 0 and (
        r11["market_outcome_class_counts"].get("good_clean", 0) == 0
        or r13["market_outcome_class_counts"].get("good_clean", 0) == 0
    ):
        blockers.append("effect_exists_only_in_combined_or_one_split")
    return {
        "status": "blocked" if blockers else "ready_for_feature_prototype",
        "blockers": blockers,
        "do_not_train_on_r13_then_validate_on_r13": True,
        "direction_stability_required": True,
        "combined_only_evidence_insufficient": True,
    }


def build_report(run_specs: list[tuple[str, Path, Path]]) -> dict[str, Any]:
    runs = [read_run(name, label_path, feasibility_path) for name, label_path, feasibility_path in run_specs]
    by_name = {run["name"].lower(): run for run in runs}
    if "r11" not in by_name or "r13" not in by_name:
        raise ValueError("P3.7 temporal split requires R11 and R13 views")
    r11 = by_name["r11"]
    r13 = by_name["r13"]
    recent = aggregate_runs("recent_r11_r13", [r11, r13])
    combined = aggregate_runs("combined_all_secondary", runs)
    drift = {
        metric: rate_diff_ci(r11, r13, metric)
        for metric in [
            "good_clean",
            "good_dirty",
            "bad_clean",
            "good_executable",
            "execution_feasible",
            "price_path_available",
        ]
    }
    gate = temporal_gate(r11, r13, combined)
    return {
        "status": "ok",
        "p3_7_5_status": gate["status"],
        "scope": {
            "phase": "P3.7.5 Temporal Split Baseline",
            "no_p2": True,
            "no_live": True,
            "no_threshold_tuning": True,
            "no_feature_claims": True,
            "combined_is_secondary": True,
        },
        "runs": runs,
        "recent_r11_r13": recent,
        "combined_all_secondary": combined,
        "r11_vs_r13_drift": drift,
        "temporal_gate": gate,
    }


def render_markdown(report: dict[str, Any]) -> str:
    combined = report["combined_all_secondary"]
    price_path_available = combined["rates"].get("price_path_available", 0.0) > 0.0
    good_clean_available = combined["market_outcome_class_counts"].get("good_clean", 0) > 0
    good_executable_available = combined["decision_quality_class_counts"].get("good_executable", 0) > 0
    if price_path_available and good_clean_available and not good_executable_available:
        gate_interpretation = (
            "- Obecny truth-layer blokuje feature prototype, bo price path i `good_clean` sa juz dostepne, "
            "ale `good_executable=0` i brakuje execution proof dla market-good rows."
        )
        strongest_for = (
            "Najmocniejsze evidence za V3: Chainstack price path daje niezerowe `good_clean` w R11/R13 "
            "oraz stabilny, audytowalny temporal split dla market-quality targetu."
        )
        strongest_against = (
            "Najmocniejsze evidence przeciw V3 jako selector: `good_clean` nie jest `good_executable`; "
            "nie ma ani jednego BUY-quality targetu z realnym shadow entry/lifecycle/simulation proof."
        )
        unresolved = (
            "Nierozstrzygniete niepewnosci: czy historyczne market-good rows byly realnie egzekwowalne, "
            "czy tylko wygladaja dobrze na post-decision price path; R13 ma pojedynczy dispatch fail-closed."
        )
        next_step = (
            "Nie przechodzic do Phase B feature prototype jako candidate work. Nastepny sensowny krok to "
            "P3.7.6 Execution Feasibility Resolution: rozstrzygnac `good_clean` vs `good_executable` "
            "na podstawie shadow entry/lifecycle/simulation evidence."
        )
    else:
        gate_interpretation = (
            "- Obecny truth-layer blokuje feature prototype, bo `good_clean=0`, `good_executable=0` "
            "i MFE/MAE sa niedostepne na wszystkich wymaganych widokach."
        )
        strongest_for = (
            "Najmocniejsze evidence za V3: replay/outcome pipeline zachowuje stabilna, audytowalna "
            "semantyke across R11/R13 i nie promuje grubego labela v1 do `good_clean`."
        )
        strongest_against = (
            "Najmocniejsze evidence przeciw V3 jako selector: nie ma ani jednego `good_clean` ani "
            "`good_executable` w R11/R13, wiec nie istnieje temporalnie walidowalny BUY-quality target."
        )
        unresolved = (
            "Nierozstrzygniete niepewnosci: czy brak price path/lifecycle jest tylko brakiem artefaktu, "
            "czy realnie oznacza niewykonalnosc; czy pelniejszy outcome v2 po danych sciezkowych zmieni rozklad."
        )
        next_step = (
            "Nie przechodzic do Phase B feature prototype jako candidate work. Nastepny sensowny krok w "
            "Phase A to uzupelnienie price path/lifecycle evidence albo jawne udokumentowanie, ze obecne "
            "artefakty nie pozwalaja zbudowac `good_clean` / `good_executable` targetu dla P3.7."
        )
    lines = [
        "# Raport P3.7 Temporal Split Baseline R10/R11/R13",
        "",
        "Status: **TEMPORAL SPLIT COMPLETE / FEATURE PROTOTYPE BLOCKED**"
        if report["p3_7_5_status"] == "blocked"
        else "Status: **TEMPORAL SPLIT COMPLETE**",
        "",
        "## Executive summary",
        "",
        "P3.7.5 raportuje R11 standalone, R13 standalone, recent-only R11/R13 oraz combined all jako widok pomocniczy. Ten raport nie dowodzi edge i nie autoryzuje P2, live, threshold tuning ani feature prototype.",
        "",
        f"- Temporal gate status: `{report['p3_7_5_status']}`",
        f"- Blockers: `{report['temporal_gate']['blockers']}`",
        "- `do_not_train_on_R13_then_validate_on_R13`: `true`",
        "",
        "## Required Views",
        "",
        "| View | Rows | Good clean | Good dirty | Bad clean | Good executable | Price path available | Dispatch expected | Shadow observed |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    views = report["runs"] + [report["recent_r11_r13"], report["combined_all_secondary"]]
    for run in views:
        market = run["market_outcome_class_counts"]
        decision = run["decision_quality_class_counts"]
        dispatch = run["dispatch"]
        lines.append(
            f"| {run['name']} | {run['rows']} | {market.get('good_clean', 0)} | "
            f"{market.get('good_dirty', 0)} | {market.get('bad_clean', 0)} | "
            f"{decision.get('good_executable', 0)} | {run['rates']['price_path_available']:.6f} | "
            f"{dispatch['dispatch_expected_rows']} | {dispatch['shadow_dispatch_observed_rows']} |"
        )
    lines.extend(["", "## R11 vs R13 Drift", ""])
    lines.append("| Metric | R11 rate | R13 rate | R13-R11 | CI95 low | CI95 high | Crosses zero |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | --- |")
    for metric, values in report["r11_vs_r13_drift"].items():
        lines.append(
            f"| `{metric}` | {values['r11_rate']:.6f} | {values['r13_rate']:.6f} | "
            f"{values['r13_minus_r11']:.6f} | {values['ci95_low']:.6f} | "
            f"{values['ci95_high']:.6f} | {values['ci95_crosses_zero']} |"
        )
    lines.extend(["", "## Numeric Path Availability", ""])
    lines.append("| View | MFE 10s n | MAE 10s n | Time to MFE n | Time to MAE n |")
    lines.append("| --- | ---: | ---: | ---: | ---: |")
    for run in views:
        nums = run["numeric_summaries"]
        lines.append(
            f"| {run['name']} | {nums['mfe_pct_10s']['available']} | "
            f"{nums['mae_pct_10s']['available']} | {nums['time_to_mfe_ms']['available']} | "
            f"{nums['time_to_mae_ms']['available']} |"
        )
    lines.extend(
        [
            "",
            "## Governance Interpretation",
            "",
            "- Combined all jest secondary; nie wolno wybrac candidate na podstawie samego combined.",
            "- Candidate fails if direction differs between R11 and R13.",
            "- Candidate fails if effect exists only in combined.",
            "- Candidate fails if R13 standalone does not support it.",
            "- Candidate fails if confidence interval crosses zero in either required split.",
            gate_interpretation,
            "",
            "## Evidence Checkpoint",
            "",
            strongest_for,
            "",
            strongest_against,
            "",
            unresolved,
            "",
            "Mozliwe zrodla self-deception: uznanie v1 `+40` za clean good, traktowanie combined jako walidacji, ignorowanie CI crossing zero, ignorowanie braku dispatch proof, oraz przejscie do feature mining bez executable target.",
            "",
            "## Next step",
            "",
            next_step,
        ]
    )
    return "\n".join(lines) + "\n"


def parse_run_spec(spec: str) -> tuple[str, Path, Path]:
    parts = spec.split(":", 2)
    if len(parts) != 3:
        raise ValueError("--run must use name:<label_v2_jsonl>:<feasibility_join_jsonl>")
    return parts[0], Path(parts[1]), Path(parts[2])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run",
        action="append",
        required=True,
        help="Run spec: name:<label_v2_jsonl>:<feasibility_join_jsonl>",
    )
    parser.add_argument("--output", required=True, type=Path, help="output JSON report")
    parser.add_argument("--md-output", required=True, type=Path, help="output Markdown report")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    report = build_report([parse_run_spec(spec) for spec in args.run])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.md_output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    args.md_output.write_text(render_markdown(report), encoding="utf-8")
    print(json.dumps({"status": report["status"], "p3_7_5_status": report["p3_7_5_status"]}, sort_keys=True))


if __name__ == "__main__":
    main()
