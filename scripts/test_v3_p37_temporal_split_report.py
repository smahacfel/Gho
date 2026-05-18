#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_temporal_split_report as report


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def label(row_id: str, market: str, *, mfe: float | None = None) -> dict:
    return {
        "ab_record_id": row_id,
        "market_outcome_class": market,
        "label_quality": "clean_price_path" if mfe is not None else "dirty_threshold_summary",
        "price_path_source": "price_path_samples" if mfe is not None else "none",
        "mfe_pct_10s": mfe,
        "mae_pct_10s": -5.0 if mfe is not None else None,
    }


def feasibility(row_id: str, execution: str, decision_quality: str) -> dict:
    return {
        "ab_record_id": row_id,
        "market_outcome_class": "good_clean" if decision_quality == "good_executable" else "unknown",
        "execution_quality_class": execution,
        "decision_quality_class": decision_quality,
        "dispatch_expected": execution != "no_dispatch_expected",
        "shadow_dispatch_observed": execution.startswith("execution_feasible"),
    }


class P37TemporalSplitReportTests(unittest.TestCase):
    def test_requires_r11_and_r13(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            labels = tmp / "r10_labels.jsonl"
            feas = tmp / "r10_feas.jsonl"
            write_jsonl(labels, [label("a", "unknown")])
            write_jsonl(feas, [feasibility("a", "no_dispatch_expected", "unknown")])
            with self.assertRaises(ValueError):
                report.build_report([("r10", labels, feas)])

    def test_blocks_when_no_good_clean_or_executable(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            specs = []
            for name in ("r11", "r13"):
                labels = tmp / f"{name}_labels.jsonl"
                feas = tmp / f"{name}_feas.jsonl"
                write_jsonl(labels, [label("a", "good_dirty"), label("b", "bad_clean")])
                write_jsonl(
                    feas,
                    [
                        feasibility("a", "no_dispatch_expected", "good_not_executable"),
                        feasibility("b", "no_dispatch_expected", "bad_avoidable"),
                    ],
                )
                specs.append((name, labels, feas))

            built = report.build_report(specs)

        self.assertEqual(built["p3_7_5_status"], "blocked")
        self.assertIn("r11_has_no_good_clean_rows", built["temporal_gate"]["blockers"])
        self.assertIn("r13_has_no_good_executable_rows", built["temporal_gate"]["blockers"])
        self.assertIn("mfe_mae_unavailable_all_runs", built["temporal_gate"]["blockers"])

    def test_ready_when_splits_have_clean_executable_support(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            specs = []
            for name in ("r11", "r13"):
                labels = tmp / f"{name}_labels.jsonl"
                feas = tmp / f"{name}_feas.jsonl"
                write_jsonl(labels, [label("a", "good_clean", mfe=45.0)])
                write_jsonl(
                    feas,
                    [
                        feasibility(
                            "a",
                            "execution_feasible_clean",
                            "good_executable",
                        )
                    ],
                )
                specs.append((name, labels, feas))

            built = report.build_report(specs)

        self.assertEqual(built["p3_7_5_status"], "ready_for_feature_prototype")
        self.assertEqual(built["temporal_gate"]["blockers"], [])
        self.assertEqual(built["r11_vs_r13_drift"]["good_clean"]["r13_minus_r11"], 0.0)

    def test_markdown_contains_no_combined_only_warning(self) -> None:
        built = {
            "p3_7_5_status": "blocked",
            "temporal_gate": {"blockers": ["x"]},
            "runs": [],
            "recent_r11_r13": {
                "name": "recent_r11_r13",
                "rows": 0,
                "market_outcome_class_counts": {},
                "decision_quality_class_counts": {},
                "dispatch": {"dispatch_expected_rows": 0, "shadow_dispatch_observed_rows": 0},
                "rates": {"price_path_available": 0.0},
                "numeric_summaries": {field: {"available": 0} for field in report.NUMERIC_FIELDS},
            },
            "combined_all_secondary": {
                "name": "combined_all_secondary",
                "rows": 0,
                "market_outcome_class_counts": {},
                "decision_quality_class_counts": {},
                "dispatch": {"dispatch_expected_rows": 0, "shadow_dispatch_observed_rows": 0},
                "rates": {"price_path_available": 0.0},
                "numeric_summaries": {field: {"available": 0} for field in report.NUMERIC_FIELDS},
            },
            "r11_vs_r13_drift": {},
        }
        text = report.render_markdown(built)
        self.assertIn("Combined all jest secondary", text)
        self.assertIn("nie autoryzuje P2", text)


if __name__ == "__main__":
    unittest.main()
