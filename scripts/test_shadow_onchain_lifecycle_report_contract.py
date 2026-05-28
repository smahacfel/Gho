#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import shadow_onchain_lifecycle_report as report


REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_PATH = (
    REPO_ROOT / "tests" / "fixtures" / "shadow_lifecycle" / "raportneu_sample.json"
)
REQUIRED_ROW_FIELDS = {
    "candidate_id",
    "close_reason",
    "entry_execution_ts_ms",
    "close_ts_ms",
    "position_duration_ms",
    "entry_price_logged",
    "effective_exit_price_sol",
    "final_pnl_pct",
    "fills",
}
REQUIRED_FILL_FIELDS = {
    "fill_index",
    "target_sample_slot",
    "shadow_exit_vs_onchain_executable_pct",
    "shadow_exit_vs_onchain_spot_pct",
}


def load_fixture() -> list[dict[str, object]]:
    payload = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))
    assert isinstance(payload, list)
    return payload


def full_lifecycle_row(*, exit_fills: list[dict[str, object]] | None = None) -> dict[str, object]:
    return {
        "schema_version": 1,
        "analysis_status": "ok",
        "candidate_id": "mint_pool_1700000000000",
        "position_id": "pool:mint:1700000000100",
        "mint_id": "mint",
        "pool_id": "pool",
        "close_reason": "Target",
        "truth_status": "resolved",
        "truth_source": "canonical_account_state_snapshot",
        "sample_price_state": "Valid",
        "timing": {
            "curve_t0_event_ts_ms": 1699999999000,
            "entry_execution_ts_ms": 1700000000100,
            "close_ts_ms": 1700000000900,
            "position_duration_ms": 800,
        },
        "shadow": {
            "entry_price_logged": 0.00000007,
            "effective_exit_price_sol": 0.00000012,
            "final_pnl_pct": 60.0,
        },
        "onchain": {
            "entry": {
                "match_slot": 12345,
            },
        },
        "exit_fills": exit_fills
        if exit_fills is not None
        else [
            {
                "fill_index": 1,
                "target_sample_slot": 12346,
                "shadow_exit_vs_onchain_executable_pct": -0.001,
                "shadow_exit_vs_onchain_spot_pct": -1.0,
            }
        ],
    }


class ShadowOnchainLifecycleReportContractTests(unittest.TestCase):
    def test_raportneu_fixture_contract_fields(self) -> None:
        rows = load_fixture()

        self.assertGreaterEqual(len(rows), 3)
        self.assertEqual(
            {"Target", "TimeStop", "StopLoss"},
            {str(row.get("close_reason")) for row in rows},
        )
        for row in rows:
            self.assertTrue(REQUIRED_ROW_FIELDS.issubset(row), row)
            self.assertIsInstance(row["fills"], list)
            self.assertGreaterEqual(len(row["fills"]), 1)
            for fill in row["fills"]:
                self.assertIsInstance(fill, dict)
                self.assertTrue(REQUIRED_FILL_FIELDS.issubset(fill), fill)

    def test_project_outcome_summary_row_matches_fixture_shape(self) -> None:
        expected = load_fixture()[0]
        full_row = full_lifecycle_row()
        full_row["candidate_id"] = expected["candidate_id"]
        full_row["close_reason"] = expected["close_reason"]
        full_row["timing"] = {
            "curve_t0_event_ts_ms": expected["curve_t0_event_ts_ms"],
            "entry_execution_ts_ms": expected["entry_execution_ts_ms"],
            "close_ts_ms": expected["close_ts_ms"],
            "position_duration_ms": expected["position_duration_ms"],
        }
        full_row["shadow"] = {
            "entry_price_logged": expected["entry_price_logged"],
            "effective_exit_price_sol": expected["effective_exit_price_sol"],
            "final_pnl_pct": expected["final_pnl_pct"],
        }
        full_row["onchain"] = {"entry": {"match_slot": expected["match_slot"]}}
        full_row["exit_fills"] = [
            {
                "fill_index": fill["fill_index"],
                "target_sample_slot": fill["target_sample_slot"],
                "shadow_exit_vs_onchain_executable_pct": fill[
                    "shadow_exit_vs_onchain_executable_pct"
                ],
                "shadow_exit_vs_onchain_spot_pct": fill[
                    "shadow_exit_vs_onchain_spot_pct"
                ],
            }
            for fill in expected["fills"]
        ]

        self.assertEqual(expected, report.project_outcome_summary_row(full_row))

    def test_project_outcome_summary_preserves_multi_fill_contract(self) -> None:
        row = full_lifecycle_row(
            exit_fills=[
                {
                    "fill_index": 1,
                    "target_sample_slot": 222,
                    "shadow_exit_vs_onchain_executable_pct": -0.01,
                    "shadow_exit_vs_onchain_spot_pct": -1.0,
                },
                {
                    "fill_index": 2,
                    "target_sample_slot": 333,
                    "shadow_exit_vs_onchain_executable_pct": -0.02,
                    "shadow_exit_vs_onchain_spot_pct": -1.1,
                },
            ]
        )

        compact = report.project_outcome_summary_row(row)

        self.assertEqual(2, len(compact["fills"]))
        self.assertEqual(222, compact["fills"][0]["target_sample_slot"])
        self.assertEqual(333, compact["fills"][1]["target_sample_slot"])

    def test_optional_summary_write_does_not_change_jsonl_output(self) -> None:
        rows = [full_lifecycle_row()]

        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            without_flag_jsonl = tmp / "without.jsonl"
            with_flag_jsonl = tmp / "with.jsonl"
            compact_json = tmp / "raportneu.json"

            report.write_jsonl(without_flag_jsonl, rows)
            report.write_jsonl(with_flag_jsonl, rows)
            report.write_json(compact_json, report.project_outcome_summary_rows(rows))

            self.assertFalse((tmp / "without_raportneu.json").exists())
            self.assertEqual(
                without_flag_jsonl.read_text(encoding="utf-8"),
                with_flag_jsonl.read_text(encoding="utf-8"),
            )
            compact = json.loads(compact_json.read_text(encoding="utf-8"))
            self.assertEqual([report.project_outcome_summary_row(rows[0])], compact)


if __name__ == "__main__":
    unittest.main()
