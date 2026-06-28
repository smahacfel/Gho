#!/usr/bin/env python3
"""Build the TSV2 A3 fixed-cell two-scope validation summary.

This script is offline/read-only for input logs and reports. It consumes the
already generated A2 CSV summaries for R49 and R50 and writes only A3 research
artifacts. It does not run A2, tune masks, or introduce new thresholds.
"""

from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Mapping, Tuple


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"

REPORT_DIR = Path("reports/selector")
PLAN_PATH = Path("PLANS/AUDYT/RAPORT_TSV2_A3_TWO_SCOPE_VALIDATION_20260628.md")
ADR_PATH = Path("docs/ADR/ADR_8D_TSV2_A3_TWO_SCOPE_VALIDATION_20260628.md")
SUMMARY_CSV = REPORT_DIR / "tsv2_a3_two_scope_validation_summary.csv"
INTERSECTION_CSV = REPORT_DIR / "tsv2_a3_two_scope_fixed_cell_intersection.csv"

Key = Tuple[str, int, int, int]

NAMED_CELLS: Mapping[str, Key] = {
    "canonical_fixed": ("M0_ALL", 6000, -6000, 120000),
    "r49_selected_tested_on_r50": ("M4_CONFIRM_2_WINDOWS", 10000, -6000, 120000),
    "r50_selected_tested_on_r49": ("M7_CLASS_RESTRICTED", 10000, -6000, 60000),
}


def _read_csv_by_key(path: Path) -> Dict[Key, Dict[str, str]]:
    rows: Dict[Key, Dict[str, str]] = {}
    with path.open(newline="") as f:
        reader = csv.DictReader(f)
        for row in reader:
            key = (
                row["mask_name"],
                int(row["target_bps"]),
                int(row["stop_bps"]),
                int(row["max_hold_ms"]),
            )
            rows[key] = row
    return rows


def _bool(value: str) -> bool:
    return str(value).strip().lower() == "true"


def _float(row: Mapping[str, str], field: str) -> float:
    value = row.get(field, "")
    if value == "":
        return 0.0
    return float(value)


def _int(row: Mapping[str, str], field: str) -> int:
    value = row.get(field, "")
    if value == "":
        return 0
    return int(float(value))


def _cell_passes(row: Mapping[str, str]) -> bool:
    return (
        _float(row, "cost100_delta_sum_bps") > 0.0
        and _float(row, "cost100_delta_avg_bps") > 0.0
        and _float(row, "cost100_delta_median_bps") >= 0.0
        and _float(row, "cost100_exit_action_precision") >= 0.70
        and _float(row, "cost100_exit_action_precision_wilson95_lower") >= 0.65
        and _bool(row.get("cost100_aggregate_target_cut_damage_guard_pass", "false"))
        and _bool(row.get("cost100_segment_target_cut_damage_guard_pass", "false"))
        and _bool(row.get("cost100_target_cut_count_guard_pass", "false"))
    )


def _cell_label(key: Key) -> str:
    return f"{key[0]} / {key[1]} / {key[2]} / {key[3]}"


def _metric(row: Mapping[str, str], field: str) -> str:
    return row.get(field, "")


@dataclass(frozen=True)
class CellComparison:
    key: Key
    pass_r49: bool
    pass_r50: bool
    passing_both: bool
    absolute_profitable_both_cost100_200: bool
    r49: Mapping[str, str]
    r50: Mapping[str, str]


def _build_comparisons(
    r49_summary: Mapping[Key, Mapping[str, str]],
    r50_summary: Mapping[Key, Mapping[str, str]],
    r49_cost: Mapping[Key, Mapping[str, str]],
    r50_cost: Mapping[Key, Mapping[str, str]],
) -> List[CellComparison]:
    comparisons: List[CellComparison] = []
    for key in sorted(set(r49_summary) & set(r50_summary)):
        pass_r49 = _cell_passes(r49_summary[key])
        pass_r50 = _cell_passes(r50_summary[key])
        r49_cost_row = r49_cost.get(key, {})
        r50_cost_row = r50_cost.get(key, {})
        absolute_profitable = (
            _float(r49_cost_row, "absolute_tsv2_pnl_cost100") > 0.0
            and _float(r49_cost_row, "absolute_tsv2_pnl_cost200") > 0.0
            and _float(r50_cost_row, "absolute_tsv2_pnl_cost100") > 0.0
            and _float(r50_cost_row, "absolute_tsv2_pnl_cost200") > 0.0
        )
        comparisons.append(
            CellComparison(
                key=key,
                pass_r49=pass_r49,
                pass_r50=pass_r50,
                passing_both=pass_r49 and pass_r50,
                absolute_profitable_both_cost100_200=absolute_profitable,
                r49=r49_summary[key],
                r50=r50_summary[key],
            )
        )
    return comparisons


def _write_intersection(comparisons: Iterable[CellComparison]) -> None:
    fields = [
        "mask_name",
        "target_bps",
        "stop_bps",
        "max_hold_ms",
        "pass_r49",
        "pass_r50",
        "passing_both",
        "absolute_profitable_both_cost100_200",
        "r49_delta_sum_bps",
        "r49_delta_avg_bps",
        "r49_delta_median_bps",
        "r49_action_precision",
        "r49_wilson_lower",
        "r49_target_cut_damage_ratio",
        "r49_aggregate_target_cut_guard",
        "r49_segment_target_cut_guard",
        "r49_target_cut_count_guard",
        "r49_public_row_verdict",
        "r50_delta_sum_bps",
        "r50_delta_avg_bps",
        "r50_delta_median_bps",
        "r50_action_precision",
        "r50_wilson_lower",
        "r50_target_cut_damage_ratio",
        "r50_aggregate_target_cut_guard",
        "r50_segment_target_cut_guard",
        "r50_target_cut_count_guard",
        "r50_public_row_verdict",
    ]
    INTERSECTION_CSV.parent.mkdir(parents=True, exist_ok=True)
    with INTERSECTION_CSV.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for item in comparisons:
            key = item.key
            writer.writerow(
                {
                    "mask_name": key[0],
                    "target_bps": key[1],
                    "stop_bps": key[2],
                    "max_hold_ms": key[3],
                    "pass_r49": item.pass_r49,
                    "pass_r50": item.pass_r50,
                    "passing_both": item.passing_both,
                    "absolute_profitable_both_cost100_200": item.absolute_profitable_both_cost100_200,
                    "r49_delta_sum_bps": _metric(item.r49, "cost100_delta_sum_bps"),
                    "r49_delta_avg_bps": _metric(item.r49, "cost100_delta_avg_bps"),
                    "r49_delta_median_bps": _metric(item.r49, "cost100_delta_median_bps"),
                    "r49_action_precision": _metric(item.r49, "cost100_exit_action_precision"),
                    "r49_wilson_lower": _metric(item.r49, "cost100_exit_action_precision_wilson95_lower"),
                    "r49_target_cut_damage_ratio": _metric(item.r49, "cost100_target_cut_damage_ratio"),
                    "r49_aggregate_target_cut_guard": _metric(item.r49, "cost100_aggregate_target_cut_damage_guard_pass"),
                    "r49_segment_target_cut_guard": _metric(item.r49, "cost100_segment_target_cut_damage_guard_pass"),
                    "r49_target_cut_count_guard": _metric(item.r49, "cost100_target_cut_count_guard_pass"),
                    "r49_public_row_verdict": _metric(item.r49, "cost100_public_row_verdict"),
                    "r50_delta_sum_bps": _metric(item.r50, "cost100_delta_sum_bps"),
                    "r50_delta_avg_bps": _metric(item.r50, "cost100_delta_avg_bps"),
                    "r50_delta_median_bps": _metric(item.r50, "cost100_delta_median_bps"),
                    "r50_action_precision": _metric(item.r50, "cost100_exit_action_precision"),
                    "r50_wilson_lower": _metric(item.r50, "cost100_exit_action_precision_wilson95_lower"),
                    "r50_target_cut_damage_ratio": _metric(item.r50, "cost100_target_cut_damage_ratio"),
                    "r50_aggregate_target_cut_guard": _metric(item.r50, "cost100_aggregate_target_cut_damage_guard_pass"),
                    "r50_segment_target_cut_guard": _metric(item.r50, "cost100_segment_target_cut_damage_guard_pass"),
                    "r50_target_cut_count_guard": _metric(item.r50, "cost100_target_cut_count_guard_pass"),
                    "r50_public_row_verdict": _metric(item.r50, "cost100_public_row_verdict"),
                }
            )


def _summary_rows(summary: Mapping[str, str]) -> List[Dict[str, str]]:
    return [{"metric": key, "value": str(value)} for key, value in summary.items()]


def _write_summary_csv(summary: Mapping[str, str]) -> None:
    with SUMMARY_CSV.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=["metric", "value"])
        writer.writeheader()
        writer.writerows(_summary_rows(summary))


def _named_cell_rows(comparisons_by_key: Mapping[Key, CellComparison]) -> List[str]:
    lines = [
        "| cell | fixed tuple | pass R49 | pass R50 | passing both | R49 precision | R49 Wilson | R49 target-cut ratio | R50 precision | R50 Wilson | R50 target-cut ratio |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for name, key in NAMED_CELLS.items():
        item = comparisons_by_key[key]
        lines.append(
            "| "
            + " | ".join(
                [
                    name,
                    f"`{_cell_label(key)}`",
                    str(item.pass_r49),
                    str(item.pass_r50),
                    str(item.passing_both),
                    _metric(item.r49, "cost100_exit_action_precision"),
                    _metric(item.r49, "cost100_exit_action_precision_wilson95_lower"),
                    _metric(item.r49, "cost100_target_cut_damage_ratio"),
                    _metric(item.r50, "cost100_exit_action_precision"),
                    _metric(item.r50, "cost100_exit_action_precision_wilson95_lower"),
                    _metric(item.r50, "cost100_target_cut_damage_ratio"),
                ]
            )
            + " |"
        )
    return lines


def _passing_cell_rows(passing: Iterable[CellComparison]) -> List[str]:
    rows = list(passing)
    lines = [
        "| fixed tuple | R49 delta sum | R49 precision | R49 target-cut ratio | R50 delta sum | R50 precision | R50 target-cut ratio | absolute profitable both cost100/200 |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    if not rows:
        lines.append("| none |  |  |  |  |  |  |  |")
        return lines
    for item in rows:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{_cell_label(item.key)}`",
                    _metric(item.r49, "cost100_delta_sum_bps"),
                    _metric(item.r49, "cost100_exit_action_precision"),
                    _metric(item.r49, "cost100_target_cut_damage_ratio"),
                    _metric(item.r50, "cost100_delta_sum_bps"),
                    _metric(item.r50, "cost100_exit_action_precision"),
                    _metric(item.r50, "cost100_target_cut_damage_ratio"),
                    str(item.absolute_profitable_both_cost100_200),
                ]
            )
            + " |"
        )
    return lines


def _write_markdown(
    summary: Mapping[str, str],
    comparisons_by_key: Mapping[Key, CellComparison],
    passing_cells: List[CellComparison],
) -> None:
    PLAN_PATH.parent.mkdir(parents=True, exist_ok=True)
    named_rows = "\n".join(_named_cell_rows(comparisons_by_key))
    passing_rows = "\n".join(_passing_cell_rows(passing_cells))
    content = f"""# PR-TSV2-A3: Two-scope fixed-cell validation summary

Date: `2026-06-28`

Status: `{summary['final_verdict']}`

## Scope

- R49: `{R49_SCOPE}`
- R50: `{R50_SCOPE}`

This is an offline fixed-cell intersection report. It does not introduce new masks, new thresholds, R48/R49/R50 retuning, runtime close behavior, `shadow_close_only`, Gatekeeper changes, selector runtime changes, `alpha_31100`, XGBoost, or TX/Jito/live path changes.

## Question

Does the same fixed combination `(mask, target_bps, stop_bps, max_hold_ms)` pass the A2 cost100 fixed-cell gate in both R49 and R50?

## Fixed-Cell Gate

A fixed cell passes a scope only if all conditions hold:

- `cost100_delta_sum_bps > 0`
- `cost100_delta_avg_bps > 0`
- `cost100_delta_median_bps >= 0`
- `cost100_exit_action_precision >= 0.70`
- `cost100_exit_action_precision_wilson95_lower >= 0.65`
- `cost100_aggregate_target_cut_damage_guard_pass = true`
- `cost100_segment_target_cut_damage_guard_pass = true`
- `cost100_target_cut_count_guard_pass = true`

## Required Cells

{named_rows}

## Intersection Result

- `fixed_cell_passing_both_count = {summary['fixed_cell_passing_both_count']}`
- `canonical_cell_passing_both = {summary['canonical_cell_passing_both']}`
- `R49_selected_cell_validated_by_R50 = {summary['R49_selected_cell_validated_by_R50']}`
- `R50_selected_cell_validated_by_R49 = {summary['R50_selected_cell_validated_by_R49']}`
- `absolute_profitability_proven = {summary['absolute_profitability_proven']}`
- `runtime_approval = false`
- `shadow_close_only_approval = false`

## Passing Fixed Cells

{passing_rows}

## Interpretation

R50 confirms that selective TSV2 masks can reduce damage versus a losing baseline, but selected rows differ between R49 and R50. A3 therefore checks only fixed-cell intersection and does not promote any runtime behavior.

No fixed cell passed both scopes. Therefore R49+R50 do not establish a stable fixed TSV2 close-policy candidate.

## Final Verdict

`{summary['final_verdict']}`

No runtime approval.
No `shadow_close_only` approval.

Next step: close active TSV2 exit direction for runtime and keep TSV2 as diagnostic/logging-only. R51 predeclared validation would require an explicitly accepted fixed-cell hypothesis, which A3 did not find.
"""
    PLAN_PATH.write_text(content)

    adr = f"""# ADR-8D: TSV2 A3 two-scope fixed-cell validation

Status: {summary['final_verdict']}
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Autor/Agent: Codex
Zakres: PR-TSV2-A3 fixed-cell two-scope validation
Poziom ryzyka: MEDIUM

Powiazane scope:
- R49: `{R49_SCOPE}`
- R50: `{R50_SCOPE}`

## 1. Decyzja

A3 compares already generated A2 CSV reports from R49 and R50. It does not add masks, thresholds, runtime behavior, active close, `shadow_close_only`, selector runtime policy, Gatekeeper policy, `alpha_31100`, XGBoost, or TX/Jito/live path changes.

## 2. Fixed cells

{named_rows}

## 3. Result

- `fixed_cell_passing_both_count = {summary['fixed_cell_passing_both_count']}`
- `canonical_cell_passing_both = {summary['canonical_cell_passing_both']}`
- `R49_selected_cell_validated_by_R50 = {summary['R49_selected_cell_validated_by_R50']}`
- `R50_selected_cell_validated_by_R49 = {summary['R50_selected_cell_validated_by_R49']}`
- `absolute_profitability_proven = {summary['absolute_profitability_proven']}`
- `runtime_approval = false`
- `shadow_close_only_approval = false`

## 4. Passing fixed cells

{passing_rows}

## 5. Consequences

Final verdict: `{summary['final_verdict']}`

This is not runtime approval and not `shadow_close_only` approval. A3 did not find a fixed-cell hypothesis to carry into R51. The active TSV2 exit direction remains rejected for runtime; TSV2 can remain diagnostic/logging-only.
"""
    ADR_PATH.write_text(adr)


def main() -> None:
    r49_summary = _read_csv_by_key(REPORT_DIR / R49_SCOPE / "time_stop_v2_mask_summary_a2.csv")
    r50_summary = _read_csv_by_key(REPORT_DIR / R50_SCOPE / "time_stop_v2_mask_summary_a2.csv")
    r49_cost = _read_csv_by_key(REPORT_DIR / R49_SCOPE / "time_stop_v2_mask_cost_sensitivity_a2.csv")
    r50_cost = _read_csv_by_key(REPORT_DIR / R50_SCOPE / "time_stop_v2_mask_cost_sensitivity_a2.csv")

    comparisons = _build_comparisons(r49_summary, r50_summary, r49_cost, r50_cost)
    comparisons_by_key = {item.key: item for item in comparisons}
    missing_named = [name for name, key in NAMED_CELLS.items() if key not in comparisons_by_key]
    if missing_named:
        raise SystemExit(f"missing required named cells: {missing_named}")

    passing_cells = [item for item in comparisons if item.passing_both]
    canonical = comparisons_by_key[NAMED_CELLS["canonical_fixed"]]
    r49_selected = comparisons_by_key[NAMED_CELLS["r49_selected_tested_on_r50"]]
    r50_selected = comparisons_by_key[NAMED_CELLS["r50_selected_tested_on_r49"]]
    absolute_profitability_proven = any(item.absolute_profitable_both_cost100_200 for item in passing_cells)

    if not passing_cells:
        final_verdict = "NO_TWO_SCOPE_FIXED_POLICY / INCONCLUSIVE_RESEARCH / NO_RUNTIME"
    elif absolute_profitability_proven:
        final_verdict = "PROMISING_OFFLINE_ONLY / NEED_R51_PREDECLARED_VALIDATION / NO_RUNTIME"
    else:
        final_verdict = "PROMISING_EXIT_DAMAGE_REDUCTION_FIXED_CELL / NEED_R51_PREDECLARED_VALIDATION / NO_RUNTIME"

    summary = {
        "r49_scope": R49_SCOPE,
        "r50_scope": R50_SCOPE,
        "fixed_cell_passing_both_count": str(len(passing_cells)),
        "canonical_cell_passing_both": str(canonical.passing_both).lower(),
        "R49_selected_cell_validated_by_R50": str(r49_selected.pass_r50).lower(),
        "R49_selected_cell_passing_both": str(r49_selected.passing_both).lower(),
        "R50_selected_cell_validated_by_R49": str(r50_selected.pass_r49).lower(),
        "R50_selected_cell_passing_both": str(r50_selected.passing_both).lower(),
        "absolute_profitability_proven": str(absolute_profitability_proven).lower(),
        "runtime_approval": "false",
        "shadow_close_only_approval": "false",
        "final_verdict": final_verdict,
        "intersection_csv": str(INTERSECTION_CSV),
        "report_md": str(PLAN_PATH),
        "adr_md": str(ADR_PATH),
    }

    _write_intersection(comparisons)
    _write_summary_csv(summary)
    _write_markdown(summary, comparisons_by_key, passing_cells)
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
