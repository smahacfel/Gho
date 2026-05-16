#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_outcome_quality_report


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def decision(
    *,
    pool_id: str,
    active_buy: bool,
    v3_verdict: str,
    v3_reason: str = "REJECT_V3_MANIPULATION_CONTRADICTION",
    candidate_id: str | None = None,
) -> dict:
    row = {
        "pool_id": pool_id,
        "base_mint": f"{pool_id}_mint",
        "join_key": f"{pool_id}:mint:1000",
        "first_seen_ts_ms": 1000,
        "decision_verdict_buy": active_buy,
        "verdict_type": "BUY" if active_buy else "REJECT_CORE_FAIL",
        "reason_code": "BUY" if active_buy else "REJECT_CORE_FAIL",
        "v3_shadow_schema_version": 1,
        "v3_shadow_verdict": v3_verdict,
        "v3_shadow_reason_code": v3_reason,
    }
    if candidate_id is not None:
        row["candidate_id"] = candidate_id
    return row


def label(pool_id: str, *, good: bool) -> dict:
    return {
        "pool_id": pool_id,
        "base_mint": f"{pool_id}_mint",
        "join_key": f"{pool_id}:mint:1000",
        "first_seen_ts_ms": 1000,
        "label_valid": True,
        "hit_40_before_stop": good,
        "rug_or_early_death": not good,
    }


class V3OutcomeQualityReportTests(unittest.TestCase):
    def test_quality_summary_classifies_v3_help_and_harm_from_labels(self) -> None:
        decisions = [
            decision(pool_id="bad_blocked", active_buy=True, v3_verdict="REJECT"),
            decision(pool_id="good_blocked", active_buy=True, v3_verdict="REJECT"),
            decision(pool_id="good_selected", active_buy=True, v3_verdict="BUY"),
            decision(pool_id="bad_selected", active_buy=True, v3_verdict="BUY"),
        ]
        labels = [
            label("bad_blocked", good=False),
            label("good_blocked", good=True),
            label("good_selected", good=True),
            label("bad_selected", good=False),
        ]
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            decisions_path = tmp / "decisions.jsonl"
            labels_path = tmp / "labels.jsonl"
            config_path = tmp / "config.toml"
            write_jsonl(decisions_path, decisions)
            write_jsonl(labels_path, labels)
            config_path.write_text("", encoding="utf-8")

            report = v3_outcome_quality_report.build_report(
                config_path,
                decisions_path,
                labels_path,
                None,
            )

        quality = report["quality"]
        self.assertEqual(quality["p3_5_status"], "outcome_quality_ready")
        self.assertEqual(quality["outcome_label_coverage"], 1.0)
        self.assertEqual(quality["sponsor_summary"]["avoided_bad_entries"], 1)
        self.assertEqual(quality["sponsor_summary"]["blocked_good_entries"], 1)
        self.assertEqual(quality["sponsor_summary"]["selected_good_entries"], 1)
        self.assertEqual(quality["sponsor_summary"]["selected_bad_entries"], 1)

    def test_missing_labels_are_insufficient_not_success(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            decisions_path = tmp / "decisions.jsonl"
            config_path = tmp / "config.toml"
            write_jsonl(
                decisions_path,
                [decision(pool_id="unknown", active_buy=False, v3_verdict="PENDING")],
            )
            config_path.write_text("", encoding="utf-8")

            report = v3_outcome_quality_report.build_report(
                config_path,
                decisions_path,
                None,
                None,
            )

        self.assertEqual(report["quality"]["p3_5_status"], "insufficient_outcome_data")
        self.assertEqual(report["quality"]["outcome_label_coverage"], 0.0)
        self.assertEqual(report["quality"]["sponsor_summary"]["inconclusive"], 1)

    def test_lifecycle_can_supply_outcome_when_labels_are_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            decisions_path = tmp / "decisions.jsonl"
            lifecycle_path = tmp / "shadow_lifecycle.jsonl"
            config_path = tmp / "config.toml"
            write_jsonl(
                decisions_path,
                [
                    decision(
                        pool_id="lifecycle_good",
                        active_buy=True,
                        v3_verdict="BUY",
                        candidate_id="candidate-a",
                    )
                ],
            )
            write_jsonl(
                lifecycle_path,
                [
                    {
                        "record_type": "position_closed",
                        "candidate_id": "candidate-a",
                        "entry_value_sol": 1.0,
                        "exit_value_sol": 1.125,
                        "net_pnl_sol": 0.01,
                        "close_reason": "take_profit",
                    }
                ],
            )
            config_path.write_text("", encoding="utf-8")

            report = v3_outcome_quality_report.build_report(
                config_path,
                decisions_path,
                None,
                lifecycle_path,
            )

        self.assertEqual(report["quality"]["outcome_label_coverage"], 1.0)
        self.assertEqual(report["quality"]["sponsor_summary"]["selected_good_entries"], 1)
        self.assertEqual(report["sample_rows"][0]["label_source"], "shadow_lifecycle")
        self.assertEqual(report["sample_rows"][0]["lifecycle_final_pnl_pct"], 12.5)


if __name__ == "__main__":
    unittest.main()
