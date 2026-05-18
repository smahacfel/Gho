#!/usr/bin/env python3
"""
Audit P3.7 truth-layer evidence availability.

This report is intentionally narrower than a feature prototype. It checks
whether current R10/R11/R13 artifacts contain the post-decision price/lifecycle
evidence needed to promote label-v2 rows to good_clean/good_executable. It also
separates decision-time vectors from outcome truth so they cannot be confused
with MFE/MAE labels.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable


POST_DECISION_PATH_FIELDS = ("price_path_samples", "lifecycle_price_samples")
NUMERIC_OUTCOME_FIELDS = (
    "mfe_pct_10s",
    "mfe_pct_30s",
    "mfe_pct_60s",
    "mae_pct_10s",
    "mae_pct_30s",
    "mae_pct_60s",
    "time_to_mfe_ms",
    "time_to_mae_ms",
)


def iter_jsonl(path: Path | None) -> Iterable[dict[str, Any]]:
    if path is None or not path.exists():
        return
    with path.open(encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            raw = line.strip()
            if not raw:
                continue
            obj = json.loads(raw)
            if isinstance(obj, dict):
                yield obj


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def rate(count: int, total: int) -> float:
    return round(count / total, 6) if total else 0.0


def execution_feasible_rows(execution: Counter[str]) -> int:
    return execution.get("execution_feasible_clean", 0) + execution.get("execution_feasible_degraded", 0)


def execution_evidence_source_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts = Counter(str(row.get("execution_evidence_source") or "unknown") for row in rows)
    return counter_dict(counts)


def required_next_step(blockers: list[str]) -> str:
    blocker_set = set(blockers)
    if blocker_set and blocker_set <= {"no_good_executable_rows", "no_execution_proof_for_market_good_rows"}:
        return "resolve_execution_feasibility_before_feature_prototype"
    return "obtain_or_derive_post_decision_price_path_or_lifecycle_evidence"


def has_nonempty_list(row: dict[str, Any], field: str) -> bool:
    value = row.get(field)
    return isinstance(value, list) and len(value) > 0


def has_numeric(row: dict[str, Any], field: str) -> bool:
    return isinstance(row.get(field), (int, float))


def materialized_checkpoint_trajectory(row: dict[str, Any]) -> list[Any]:
    snapshot = row.get("v3_materialized_feature_snapshot")
    if not isinstance(snapshot, dict):
        return []
    checkpoint = snapshot.get("checkpoint_features")
    if not isinstance(checkpoint, dict):
        return []
    trajectory = checkpoint.get("price_trajectory")
    return trajectory if isinstance(trajectory, list) else []


def event_kind(row: dict[str, Any]) -> str:
    kind = row.get("kind")
    if isinstance(kind, dict):
        kind_type = kind.get("type")
        payload = kind.get("payload")
        source = payload.get("source") if isinstance(payload, dict) else None
        return "/".join(str(part) for part in (kind_type, source) if part)
    for field in ("event_type", "type", "record_type"):
        value = row.get(field)
        if isinstance(value, str) and value:
            return value
    return "unknown"


def sample_event_schema(events_dir: Path | None, *, max_files: int = 5, max_rows_per_file: int = 20) -> dict[str, Any]:
    if events_dir is None:
        return {
            "events_dir": None,
            "exists": False,
            "file_count": 0,
            "sampled_rows": 0,
            "sampled_kind_counts": {},
            "price_path_like_rows": 0,
            "classification": "not_configured",
        }
    if not events_dir.exists():
        return {
            "events_dir": str(events_dir),
            "exists": False,
            "file_count": 0,
            "sampled_rows": 0,
            "sampled_kind_counts": {},
            "price_path_like_rows": 0,
            "classification": "missing",
        }
    files = sorted(events_dir.glob("*.jsonl"))
    kind_counts: Counter[str] = Counter()
    sampled = 0
    price_path_like = 0
    for path in files[:max_files]:
        for idx, row in enumerate(iter_jsonl(path)):
            if idx >= max_rows_per_file:
                break
            sampled += 1
            kind_counts[event_kind(row)] += 1
            if any(has_nonempty_list(row, field) for field in POST_DECISION_PATH_FIELDS):
                price_path_like += 1
    classification = "sampled_candidate_events_only"
    if price_path_like:
        classification = "sample_contains_price_path_like_rows"
    return {
        "events_dir": str(events_dir),
        "exists": True,
        "file_count": len(files),
        "sampled_rows": sampled,
        "sampled_kind_counts": counter_dict(kind_counts),
        "price_path_like_rows": price_path_like,
        "classification": classification,
    }


def summarize_run(
    *,
    name: str,
    decisions_path: Path,
    threshold_hits_path: Path,
    labels_v2_path: Path,
    feasibility_path: Path,
    events_dir: Path | None,
) -> dict[str, Any]:
    decisions = list(iter_jsonl(decisions_path))
    threshold_hits = list(iter_jsonl(threshold_hits_path))
    labels = list(iter_jsonl(labels_v2_path))
    feasibility = list(iter_jsonl(feasibility_path))

    market = Counter(str(row.get("market_outcome_class") or "unknown") for row in labels)
    decision_quality = Counter(str(row.get("decision_quality_class") or "unknown") for row in feasibility)
    execution = Counter(str(row.get("execution_quality_class") or "unknown") for row in feasibility)

    threshold_post_path = sum(
        1
        for row in threshold_hits
        if any(has_nonempty_list(row, field) for field in POST_DECISION_PATH_FIELDS)
    )
    threshold_summary_rows = sum(
        1
        for row in threshold_hits
        if has_numeric(row, "threshold_window_max_return_pct")
        or has_numeric(row, "threshold_window_min_return_pct")
    )
    labels_with_numeric_outcome = sum(
        1 for row in labels if any(has_numeric(row, field) for field in NUMERIC_OUTCOME_FIELDS)
    )
    label_price_path_rows = sum(
        1
        for row in labels
        if row.get("price_path_source") not in {None, "", "none", "unknown"}
        and any(has_numeric(row, field) for field in NUMERIC_OUTCOME_FIELDS)
    )
    post_decision_path_rows = max(threshold_post_path, label_price_path_rows)
    decision_vector_rows = sum(
        1
        for row in decisions
        if has_nonempty_list(row, "vectors_prices") and has_nonempty_list(row, "vectors_ts_offsets_ms")
    )
    checkpoint_trajectory_rows = sum(1 for row in decisions if materialized_checkpoint_trajectory(row))
    dispatch_expected = sum(1 for row in feasibility if row.get("dispatch_expected") is True)
    shadow_observed = sum(1 for row in feasibility if row.get("shadow_dispatch_observed") is True)
    market_good = market.get("good_clean", 0) + market.get("good_dirty", 0)

    rows = len(labels)
    blockers: list[str] = []
    if market.get("good_clean", 0) == 0:
        blockers.append("no_good_clean_rows")
    if decision_quality.get("good_executable", 0) == 0:
        blockers.append("no_good_executable_rows")
    if market_good > 0 and execution_feasible_rows(execution) == 0:
        blockers.append("no_execution_proof_for_market_good_rows")
    if post_decision_path_rows == 0:
        blockers.append("no_post_decision_price_path_rows")
    if labels_with_numeric_outcome == 0:
        blockers.append("no_label_v2_mfe_mae_rows")

    return {
        "name": name,
        "paths": {
            "decisions": str(decisions_path),
            "threshold_hits": str(threshold_hits_path),
            "labels_v2": str(labels_v2_path),
            "feasibility": str(feasibility_path),
        },
        "row_counts": {
            "decision_rows": len(decisions),
            "threshold_hit_rows": len(threshold_hits),
            "label_v2_rows": rows,
            "feasibility_rows": len(feasibility),
        },
        "market_outcome_class_counts": counter_dict(market),
        "execution_quality_class_counts": counter_dict(execution),
        "decision_quality_class_counts": counter_dict(decision_quality),
        "outcome_truth_evidence": {
            "post_decision_price_path_rows": post_decision_path_rows,
            "post_decision_price_path_share": rate(post_decision_path_rows, rows),
            "threshold_post_decision_price_path_rows": threshold_post_path,
            "label_v2_price_path_rows": label_price_path_rows,
            "threshold_summary_rows": threshold_summary_rows,
            "threshold_summary_share": rate(threshold_summary_rows, len(threshold_hits)),
            "label_v2_mfe_mae_rows": labels_with_numeric_outcome,
            "label_v2_mfe_mae_share": rate(labels_with_numeric_outcome, rows),
            "threshold_summary_is_not_price_path": True,
        },
        "decision_time_inputs": {
            "decision_vector_rows": decision_vector_rows,
            "decision_vector_share": rate(decision_vector_rows, len(decisions)),
            "checkpoint_price_trajectory_rows": checkpoint_trajectory_rows,
            "checkpoint_price_trajectory_share": rate(checkpoint_trajectory_rows, len(decisions)),
            "not_outcome_truth_source": True,
        },
        "execution_evidence": {
            "dispatch_expected_rows": dispatch_expected,
            "shadow_dispatch_observed_rows": shadow_observed,
            "execution_feasible_rows": execution_feasible_rows(execution),
            "execution_evidence_source_counts": execution_evidence_source_counts(feasibility),
            "dispatch_observed_without_expected_rows": sum(
                1
                for row in feasibility
                if row.get("shadow_dispatch_observed") is True
                and row.get("dispatch_expected") is not True
            ),
        },
        "event_dataset_schema_sample": sample_event_schema(events_dir),
        "status": "blocked" if blockers else "evidence_ready_for_temporal_target",
        "blockers": blockers,
    }


def aggregate(name: str, runs: list[dict[str, Any]]) -> dict[str, Any]:
    def sum_path(path: list[str]) -> int:
        total = 0
        for run in runs:
            value: Any = run
            for key in path:
                value = value[key]
            total += int(value)
        return total

    label_rows = sum_path(["row_counts", "label_v2_rows"])
    threshold_rows = sum_path(["row_counts", "threshold_hit_rows"])
    decision_rows = sum_path(["row_counts", "decision_rows"])
    post_path = sum_path(["outcome_truth_evidence", "post_decision_price_path_rows"])
    numeric_rows = sum_path(["outcome_truth_evidence", "label_v2_mfe_mae_rows"])
    decision_vectors = sum_path(["decision_time_inputs", "decision_vector_rows"])
    checkpoint_vectors = sum_path(["decision_time_inputs", "checkpoint_price_trajectory_rows"])
    good_clean = sum(run["market_outcome_class_counts"].get("good_clean", 0) for run in runs)
    market_good = sum(
        run["market_outcome_class_counts"].get("good_clean", 0)
        + run["market_outcome_class_counts"].get("good_dirty", 0)
        for run in runs
    )
    good_executable = sum(run["decision_quality_class_counts"].get("good_executable", 0) for run in runs)
    execution_feasible = sum(run["execution_evidence"]["execution_feasible_rows"] for run in runs)
    blockers: list[str] = []
    if good_clean == 0:
        blockers.append("no_good_clean_rows")
    if good_executable == 0:
        blockers.append("no_good_executable_rows")
    if market_good > 0 and execution_feasible == 0:
        blockers.append("no_execution_proof_for_market_good_rows")
    if post_path == 0:
        blockers.append("no_post_decision_price_path_rows")
    if numeric_rows == 0:
        blockers.append("no_label_v2_mfe_mae_rows")
    return {
        "name": name,
        "row_counts": {
            "decision_rows": decision_rows,
            "threshold_hit_rows": threshold_rows,
            "label_v2_rows": label_rows,
        },
        "outcome_truth_evidence": {
            "post_decision_price_path_rows": post_path,
            "post_decision_price_path_share": rate(post_path, threshold_rows),
            "label_v2_mfe_mae_rows": numeric_rows,
            "label_v2_mfe_mae_share": rate(numeric_rows, label_rows),
        },
        "execution_evidence": {
            "execution_feasible_rows": execution_feasible,
        },
        "decision_time_inputs": {
            "decision_vector_rows": decision_vectors,
            "decision_vector_share": rate(decision_vectors, decision_rows),
            "checkpoint_price_trajectory_rows": checkpoint_vectors,
            "checkpoint_price_trajectory_share": rate(checkpoint_vectors, decision_rows),
            "not_outcome_truth_source": True,
        },
        "status": "blocked" if blockers else "evidence_ready_for_temporal_target",
        "blockers": blockers,
    }


def parse_run_spec(spec: str) -> tuple[str, Path, Path, Path, Path, Path | None]:
    parts = spec.split(":")
    if len(parts) not in {5, 6}:
        raise argparse.ArgumentTypeError(
            "--run must be name:decisions:threshold_hits:labels_v2:feasibility[:events_dir]"
        )
    name, decisions, threshold_hits, labels_v2, feasibility = parts[:5]
    events_dir = Path(parts[5]) if len(parts) == 6 and parts[5] else None
    return name, Path(decisions), Path(threshold_hits), Path(labels_v2), Path(feasibility), events_dir


def build_report(run_specs: list[tuple[str, Path, Path, Path, Path, Path | None]]) -> dict[str, Any]:
    runs = [
        summarize_run(
            name=name,
            decisions_path=decisions,
            threshold_hits_path=threshold_hits,
            labels_v2_path=labels_v2,
            feasibility_path=feasibility,
            events_dir=events_dir,
        )
        for name, decisions, threshold_hits, labels_v2, feasibility, events_dir in run_specs
    ]
    by_name = {run["name"].lower(): run for run in runs}
    if "r11" not in by_name or "r13" not in by_name:
        raise ValueError("P3.7 evidence availability requires R11 and R13")
    recent = aggregate("recent_r11_r13", [by_name["r11"], by_name["r13"]])
    combined = aggregate("combined_all_secondary", runs)
    blockers = sorted(set(recent["blockers"]) | set(combined["blockers"]))
    return {
        "status": "ok",
        "p3_7_evidence_status": "blocked" if blockers else "evidence_ready_for_temporal_target",
        "scope": {
            "phase": "P3.7 Phase A evidence availability audit",
            "no_p2": True,
            "no_live": True,
            "no_threshold_tuning": True,
            "no_feature_claims": True,
            "feature_prototype_candidate_work_allowed": False if blockers else True,
            "decision_time_vectors_are_not_outcome_truth": True,
        },
        "runs": runs,
        "recent_r11_r13": recent,
        "combined_all_secondary": combined,
        "gate": {
            "status": "blocked" if blockers else "ready_for_phase_b_truth_target",
            "blockers": blockers,
            "required_next_step": required_next_step(blockers)
            if blockers
            else "feature_prototype_may_start_with_temporal_split_controls",
        },
    }


def render_markdown(report: dict[str, Any]) -> str:
    blocked = report["gate"]["status"] == "blocked"
    combined = report.get("combined_all_secondary", {})
    combined_truth = combined.get("outcome_truth_evidence", {})
    combined_good_clean = combined_truth.get("post_decision_price_path_rows", 0) > 0
    combined_market_good = any(
        run["market_outcome_class_counts"].get("good_clean", 0)
        + run["market_outcome_class_counts"].get("good_dirty", 0)
        for run in report["runs"]
    )
    execution_blocked = "no_good_executable_rows" in report["gate"]["blockers"]
    strongest_for = (
        "Najmocniejsze evidence za V3: mamy Chainstack post-decision price path, niezerowe `good_clean` w R10/R11/R13 oraz stabilne label-v2/feasibility artefakty do dalszej diagnostyki."
        if combined_good_clean
        else "Najmocniejsze evidence za V3: mamy pelny replay, stabilne label-v2/feasibility artefakty i decision-time vectors, ktore moga posluzyc do przyszlej diagnostyki feature families."
    )
    unresolved = (
        "Nierozstrzygniete niepewnosci: czy historyczne market-good rows byly realnie egzekwowalne przez Ghost, czy tylko wygladaja dobrze na post-decision price path."
        if combined_market_good and execution_blocked
        else "Nierozstrzygniete niepewnosci: czy post-decision price path da sie odzyskac z RPC/threshold fetchera, czy wymaga nowego artefaktu labelera/lifecycle."
    )
    lines = [
        "# Raport P3.7 Evidence Availability R10/R11/R13",
        "",
        "Status: **EVIDENCE TARGET BLOCKED / NO FEATURE PROTOTYPE**"
        if blocked
        else "Status: **EVIDENCE TARGET READY**",
        "",
        "## Executive summary",
        "",
        "Ten raport sprawdza, czy obecne artefakty P3.7 maja evidence potrzebne do `good_clean` i `good_executable`. Nie jest to feature prototype, nie dowodzi edge i nie autoryzuje P2/live/tuningu.",
        "",
        f"- Gate status: `{report['gate']['status']}`",
        f"- Blockers: `{report['gate']['blockers']}`",
        f"- Required next step: `{report['gate']['required_next_step']}`",
        "",
        "## Evidence By Run",
        "",
        "| Run | Rows | Post-decision path | MFE/MAE rows | Decision vectors | Checkpoint trajectory | Good clean | Good executable | Status |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
    ]
    for run in report["runs"]:
        rows = run["row_counts"]["label_v2_rows"]
        truth = run["outcome_truth_evidence"]
        vectors = run["decision_time_inputs"]
        good_clean = run["market_outcome_class_counts"].get("good_clean", 0)
        good_exec = run["decision_quality_class_counts"].get("good_executable", 0)
        lines.append(
            f"| {run['name']} | {rows} | {truth['post_decision_price_path_rows']} | {truth['label_v2_mfe_mae_rows']} | "
            f"{vectors['decision_vector_rows']} | {vectors['checkpoint_price_trajectory_rows']} | {good_clean} | {good_exec} | `{run['status']}` |"
        )
    lines.extend(
        [
            "",
            "## Event Dataset Schema Sample",
            "",
            "| Run | Event files | Sampled rows | Classification | Price-path-like rows |",
            "| --- | ---: | ---: | --- | ---: |",
        ]
    )
    for run in report["runs"]:
        sample = run["event_dataset_schema_sample"]
        lines.append(
            f"| {run['name']} | {sample['file_count']} | {sample['sampled_rows']} | `{sample['classification']}` | {sample['price_path_like_rows']} |"
        )
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- Decision logs maja decision-time vectors i checkpoint price trajectory. To moze byc future input do feature prototype, ale nie jest outcome truth dla MFE/MAE po decyzji.",
            "- Threshold summaries maja `threshold_window_max_return_pct` / `threshold_window_min_return_pct`, ale to nie jest pelna sciezka price/lifecycle. Nie wolno z tego promowac v1 `+40` do `good_clean`.",
            "- Post-decision price path jest rozwiazany dla R10/R11/R13." if combined_good_clean else "- Obecne R10/R11/R13 nie maja post-decision price path rows w formacie wymaganym przez Outcome Label v2.",
            "- Aktualny blocker to `no_execution_proof_for_market_good_rows` / `no_good_executable_rows`, a nie brak market-good price path." if combined_market_good and execution_blocked else "- Brak `good_clean` i `good_executable` oznacza, ze Phase B candidate feature work pozostaje zablokowane.",
            "",
            "## Evidence Checkpoint",
            "",
            strongest_for,
            "",
            "Najmocniejsze evidence przeciw V3 jako selector: obecne artefakty nie dowodza ani jednego clean executable good target, wiec nie ma celu BUY-quality do walidacji candidate.",
            "",
            unresolved,
            "",
            "Mozliwe zrodla self-deception: potraktowanie decision-time vectors jako outcome path, potraktowanie threshold summary jako MFE/MAE path, przejscie do feature mining bez `good_clean` targetu.",
            "",
            "## Next Step",
            "",
            "Nie przechodzic do P3.7 Phase B. Nastepny ruch to P3.7.6 Execution Feasibility Resolution: rozstrzygnac, czy market-good rows maja realny shadow entry/lifecycle/simulation proof, czy pozostaja `good_not_executable`.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run",
        action="append",
        required=True,
        type=parse_run_spec,
        help="name:decisions:threshold_hits:labels_v2:feasibility[:events_dir]",
    )
    parser.add_argument("--output-json", type=Path, required=True)
    parser.add_argument("--output-md", type=Path)
    parser.add_argument("--json", action="store_true", help="print compact status JSON")
    args = parser.parse_args()

    report = build_report(args.run)
    write_json(args.output_json, report)
    if args.output_md:
        args.output_md.parent.mkdir(parents=True, exist_ok=True)
        args.output_md.write_text(render_markdown(report), encoding="utf-8")
    if args.json:
        print(
            json.dumps(
                {
                    "status": report["status"],
                    "p3_7_evidence_status": report["p3_7_evidence_status"],
                    "blockers": report["gate"]["blockers"],
                },
                sort_keys=True,
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
