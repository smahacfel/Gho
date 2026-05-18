#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_lifecycle_join_report as join_report


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def decision(ab_id: str, *, buy: bool) -> dict:
    return {
        "ab_record_id": ab_id,
        "join_key": f"{ab_id}:pool:mint:1000",
        "pool_id": f"{ab_id}_pool",
        "base_mint": f"{ab_id}_mint",
        "decision_verdict_buy": buy,
        "verdict_type": "BUY" if buy else "REJECT_CORE_FAIL",
        "ab_t_end_event_ts_ms": 2_000,
    }


def label(ab_id: str, market: str) -> dict:
    return {
        "ab_record_id": ab_id,
        "join_key": f"{ab_id}:pool:mint:1000",
        "pool_id": f"{ab_id}_pool",
        "base_mint": f"{ab_id}_mint",
        "market_outcome_class": market,
        "label_v2_schema_version": 1,
    }


def run_report(
    *,
    decisions: list[dict],
    labels: list[dict],
    entries: list[dict] | None = None,
    lifecycle: list[dict] | None = None,
) -> tuple[dict, list[dict]]:
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        decisions_path = tmp / "decisions.jsonl"
        labels_path = tmp / "labels_v2.jsonl"
        entries_path = tmp / "entries.jsonl"
        lifecycle_path = tmp / "lifecycle.jsonl"
        joined_path = tmp / "joined.jsonl"
        write_jsonl(decisions_path, decisions)
        write_jsonl(labels_path, labels)
        if entries is not None:
            write_jsonl(entries_path, entries)
        if lifecycle is not None:
            write_jsonl(lifecycle_path, lifecycle)
        summary = join_report.build_report(
            decisions_path,
            labels_path,
            entries_path if entries is not None else None,
            lifecycle_path if lifecycle is not None else None,
            joined_path,
        )
        rows = [json.loads(line) for line in joined_path.read_text().splitlines()]
    return summary, rows


class P37LifecycleJoinReportTests(unittest.TestCase):
    def test_reject_without_dispatch_is_no_dispatch_expected_not_failure(self) -> None:
        summary, rows = run_report(
            decisions=[decision("a", buy=False)],
            labels=[label("a", "good_dirty")],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "no_dispatch_expected")
        self.assertEqual(rows[0]["decision_quality_class"], "good_not_executable")
        self.assertEqual(summary["dispatch_expected_rows"], 0)
        self.assertEqual(summary["shadow_dispatch_observed_rows"], 0)
        self.assertEqual(summary["execution_quality_class_counts"], {"no_dispatch_expected": 1})

    def test_buy_without_shadow_artifacts_is_execution_unknown(self) -> None:
        summary, rows = run_report(
            decisions=[decision("a", buy=True)],
            labels=[label("a", "good_clean")],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "execution_unknown")
        self.assertEqual(rows[0]["no_dispatch_reason"], "dispatch_expected_but_no_shadow_artifacts")
        self.assertTrue(rows[0]["unknown_execution_status"])
        self.assertEqual(summary["dispatch_expected_rows"], 1)
        self.assertEqual(summary["dispatch_expected_without_observed_rows"], 1)

    def test_position_closed_is_execution_feasible_clean(self) -> None:
        entry = {
            "candidate_id": "candidate-a",
            "ab_record_id": "a",
            "execution_outcome": "success",
            "timestamp_ms": 2_050,
        }
        lifecycle = {
            "candidate_id": "candidate-a",
            "ab_record_id": "a",
            "record_type": "position_closed",
            "dispatch_status": "success",
            "simulation_outcome": "success",
            "timestamp_ms": 2_075,
        }
        _, rows = run_report(
            decisions=[decision("a", buy=True)],
            labels=[label("a", "good_clean")],
            entries=[entry],
            lifecycle=[lifecycle],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "execution_feasible_clean")
        self.assertEqual(rows[0]["decision_quality_class"], "good_executable")
        self.assertEqual(rows[0]["decision_to_entry_materialization_ms"], 50)
        self.assertEqual(rows[0]["decision_to_sim_ms"], 75)

    def test_failed_shadow_dispatch_is_infeasible(self) -> None:
        lifecycle = {
            "candidate_id": "candidate-a",
            "ab_record_id": "a",
            "record_type": "shadow_dispatch",
            "dispatch_status": "failed",
            "simulation_outcome": "failed",
            "error_class": "data_problem",
            "timestamp_ms": 2_075,
        }
        _, rows = run_report(
            decisions=[decision("a", buy=True)],
            labels=[label("a", "good_clean")],
            lifecycle=[lifecycle],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "execution_infeasible")
        self.assertEqual(rows[0]["simulation_error_class"], "data_problem")
        self.assertEqual(rows[0]["decision_quality_class"], "good_not_executable")

    def test_missing_decision_row_is_explicit_unknown(self) -> None:
        summary, rows = run_report(decisions=[], labels=[label("a", "neutral_clean")])

        self.assertEqual(summary["unmatched_label_rows"], 1)
        self.assertEqual(rows[0]["execution_quality_class"], "execution_unknown")
        self.assertEqual(rows[0]["no_dispatch_reason"], "missing_decision_row")
        self.assertEqual(rows[0]["decision_quality_class"], "neutral")

    def test_unmatched_shadow_artifacts_are_reported(self) -> None:
        summary, rows = run_report(
            decisions=[decision("a", buy=False)],
            labels=[label("a", "neutral_clean")],
            entries=[{"candidate_id": "orphan", "execution_outcome": "success"}],
            lifecycle=[{"candidate_id": "orphan", "record_type": "shadow_dispatch"}],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "no_dispatch_expected")
        self.assertEqual(summary["unmatched_shadow_entry_rows"], 1)
        self.assertEqual(summary["unmatched_shadow_lifecycle_rows"], 1)

    def test_observed_shadow_without_expected_dispatch_is_counted(self) -> None:
        entry = {
            "ab_record_id": "a",
            "candidate_id": "candidate-a",
            "execution_outcome": "success",
        }
        summary, rows = run_report(
            decisions=[decision("a", buy=False)],
            labels=[label("a", "neutral_clean")],
            entries=[entry],
        )

        self.assertEqual(rows[0]["execution_quality_class"], "no_dispatch_expected")
        self.assertTrue(rows[0]["shadow_dispatch_observed"])
        self.assertEqual(summary["shadow_dispatch_observed_rows"], 1)
        self.assertEqual(summary["dispatch_observed_without_expected_rows"], 1)


if __name__ == "__main__":
    unittest.main()
