#!/usr/bin/env python3
"""Gate 0 only: read five-snapshot JSONL, compare every numeric metric per phase.
No model fitting, threshold optimization, live scoring or external dependencies.
"""
from __future__ import annotations
import argparse
import bisect
import json
import math
from collections import Counter
from pathlib import Path
from typing import Any

PHASES = (30_000, 90_000, 180_000, 300_000, 600_000)
LABEL_COLUMNS = {"tx_count", "buy_count", "sell_count", "alpha_sample", "successful_swap_count", "unique_successful_tx_count"}

def quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lo = int(index)
    hi = min(lo + 1, len(ordered) - 1)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (index - lo)

def numeric(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)

def effect_size(gems: list[float], controls: list[float]) -> float | None:
    """Cliff's delta, including ties; [-1,1], zero for identical distributions."""
    if not gems or not controls:
        return None
    controls = sorted(controls)
    total = sum(bisect.bisect_left(controls, x) - (len(controls) - bisect.bisect_right(controls, x)) for x in gems)
    return total / (len(gems) * len(controls))

def overlap(gems: list[float], controls: list[float]) -> float | None:
    if not gems or not controls:
        return None
    combined = gems + controls
    # Shared pooled quantile bins, never separate bins per outcome class.
    edges = sorted(set(quantile(combined, i / 12) for i in range(1, 12)))
    a = Counter(bisect.bisect_right(edges, x) for x in gems)
    b = Counter(bisect.bisect_right(edges, x) for x in controls)
    return sum(min(a[i] / len(gems), b[i] / len(controls)) for i in set(a) | set(b))

def describe(values: list[float]) -> dict[str, Any]:
    return {"n": len(values), "p10": quantile(values, .1), "p25": quantile(values, .25),
            "median": quantile(values, .5), "p75": quantile(values, .75), "p90": quantile(values, .9)}

def load(paths: list[Path]) -> tuple[dict, dict, dict, dict]:
    starts, ends, births, phases, labels = {}, {}, {}, {}, {}
    for path in paths:
        with path.open(encoding="utf-8") as source:
            for line_no, line in enumerate(source, 1):
                if not line.strip():
                    continue
                row = json.loads(line)
                run = row.get("run_id")
                if not isinstance(run, str) or not run:
                    raise ValueError(f"{path}:{line_no}: missing run_id")
                kind = row.get("kind")
                key = (run, row.get("mint"))
                if kind == "run_start":
                    target, key = starts, run
                    if row.get("phase_ages_ms") != list(PHASES) or row.get("schema_version") != 1:
                        raise ValueError("incompatible Gate 0 source schema")
                elif kind == "run_end":
                    target, key = ends, run
                elif kind == "transaction_outcome_conflict":
                    raise ValueError("transaction outcome conflict in source log; dataset is invalid")
                elif kind == "birth":
                    target = births
                elif kind == "terminal":
                    target = labels
                    if row.get("gem") is not None and type(row.get("gem")) is not bool:
                        raise ValueError("Gem label must be boolean or null")
                elif kind == "phase":
                    phase = row.get("phase")
                    if type(phase) is not int or not 1 <= phase <= 5 or row.get("age_ms") != PHASES[phase - 1]:
                        raise ValueError("invalid phase/cutoff")
                    target, key = phases, (*key, phase)
                else:
                    raise ValueError(f"unknown row kind: {kind!r}")
                if key in target:
                    raise ValueError(f"duplicate {kind}: {key}")
                target[key] = row
    if not starts or set(starts) != set(ends):
        raise ValueError("run is not finalized; wait for the last 600s observation")
    for run, end in ends.items():
        if end.get("reason") is not None:
            raise ValueError(
                f"run {run} is diagnostic/incomplete: root={end.get('reason')} "
                f"shutdown={end.get('shutdown_error')}"
            )
    if set(births) != set(labels):
        raise ValueError("birth/terminal population mismatch")
    if any(run not in starts or not mint for run, mint in births):
        raise ValueError("birth has no run or mint")
    prefixes: dict[tuple, list[int]] = {}
    for (run, mint, phase), row in phases.items():
        prefixes.setdefault((run, mint), []).append(phase)
        if (run, mint) not in births or (run, mint) not in labels:
            raise ValueError("phase has no matching birth/terminal")
        if row.get("cutoff_ms") != births[(run, mint)]["born_ms"] + PHASES[phase - 1]:
            raise ValueError("phase cutoff differs from birth + age")
        if not isinstance(row.get("snapshot", {}).get("metrics"), dict):
            raise ValueError("missing full metric snapshot")
    for key, label in labels.items():
        present = sorted(prefixes.get(key, []))
        if present != list(range(1, label["last_phase"] + 1)):
            raise ValueError("phase prefix is incomplete")
        if label["gem"] is True and (present != [1, 2, 3, 4, 5] or label["reason"] != "completed"):
            raise ValueError("Gem has no complete five-phase observation")
    return starts, births, phases, labels

def analyze(paths: list[Path], minimum: int = 20, max_overlap: float = .8, min_effect: float = .33) -> dict[str, Any]:
    starts, births, phases, labels = load(paths)
    results, candidates = [], []
    phase_counts = Counter(phase for _, _, phase in phases)
    enough = False
    for phase in range(1, 6):
        groups: dict[bool, list[dict]] = {True: [], False: []}
        for (run, mint, p), row in phases.items():
            label = labels[(run, mint)]["gem"]
            if p == phase and type(label) is bool:
                groups[label].append(row["snapshot"]["metrics"])
        names = sorted({name for rows in groups.values() for row in rows for name in row})
        for name in names:
            a = [float(row[name]) for row in groups[True] if numeric(row.get(name))]
            b = [float(row[name]) for row in groups[False] if numeric(row.get(name))]
            effect, ovl = effect_size(a, b), overlap(a, b)
            descriptive = phase == 5 or name in LABEL_COLUMNS
            # Phase V is the outcome horizon, not an early predictive test.
            eligible = not descriptive and len(a) >= minimum and len(b) >= minimum
            enough |= eligible
            candidate = eligible and abs(effect) >= min_effect and ovl <= max_overlap
            item = {"phase": phase, "age_ms": PHASES[phase - 1], "metric": name,
                    "gem": describe(a), "non_gem": describe(b), "cliffs_delta": effect, "ovl": ovl,
                    "missing_gem": len(groups[True]) - len(a), "missing_non_gem": len(groups[False]) - len(b),
                    "descriptive_only": descriptive, "candidate": candidate}
            results.append(item)
            if candidate:
                candidates.append(item)
    candidates.sort(key=lambda r: (-abs(r["cliffs_delta"]), r["ovl"], r["phase"], r["metric"]))
    verdict = "GO" if candidates else "STOP" if enough else "INSUFFICIENT_DATA"
    return {"gate": verdict, "scope": "exploratory_univariate_scan_not_validated_evidence",
            "settings": {"min_class": minimum, "max_ovl": max_overlap, "min_abs_cliffs_delta": min_effect},
            "runs": list(starts), "births": len(births), "phase_counts": dict(sorted(phase_counts.items())),
            "terminal_reasons": dict(Counter(row["reason"] for row in labels.values())),
            "gems": sum(row["gem"] is True for row in labels.values()),
            "label_unavailable": sum(row["gem"] is None for row in labels.values()),
            "candidates": candidates, "metrics": results,
            "interpretation": {"GO": "Visible candidates for further checking, not proof of a tradable model.",
                               "STOP": "No univariate separation at these settings in the evaluable part of this run.",
                               "INSUFFICIENT_DATA": "Too few observed Gem/control values; this is not evidence of no signal."}[verdict]}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--min-class", type=int, default=20)
    parser.add_argument("--max-ovl", type=float, default=.8)
    parser.add_argument("--min-effect", type=float, default=.33)
    args = parser.parse_args()
    if args.min_class < 2 or not 0 <= args.max_ovl <= 1 or not 0 <= args.min_effect <= 1:
        parser.error("invalid comparison settings")
    try:
        report = analyze(args.logs, args.min_class, args.max_ovl, args.min_effect)
        with args.output.open("x", encoding="utf-8") as out:
            json.dump(report, out, ensure_ascii=False, indent=2, allow_nan=False)
            out.write("\n")
    except (ValueError, KeyError, OSError, TypeError) as error:
        parser.exit(2, f"Gate 0 input/output error: {error}\n")
    print(f"{report['gate']}: births={report['births']} gems={report['gems']} unavailable={report['label_unavailable']}")
    print(f"phases={report['phase_counts']} terminal={report['terminal_reasons']}")
    for row in report["candidates"][:20]:
        print(f"phase={row['phase']} {row['metric']}: delta={row['cliffs_delta']:.3f} OVL={row['ovl']:.3f}")
    print(report["interpretation"])
if __name__ == "__main__":
    main()
