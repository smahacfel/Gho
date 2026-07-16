#!/usr/bin/env python3

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("het_pm_v2_analysis.py")
SPEC = importlib.util.spec_from_file_location("het_pm_v2_analysis", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def fixture() -> dict:
    return {
        "schema_version": 1,
        "policy_id": "hierarchical_executable_trajectory_pm_v2",
        "position_id": "position-1",
        "position_epoch": 1,
        "snapshot_id": "snapshot-1",
        "terminal_tick": False,
        "trajectory": {"quality": "usable", "flags": 0},
        "vitality": {"current_state": "alive"},
        "route_status": "pump_curve_supported",
        "v2_winning_gate": "hold",
        "v1_prequote": "hold",
        "v2_prequote": "Hold",
        "v2_final": "Hold",
        "quote_keys": [],
        "quote_statuses": [],
        "quote_resolution_count": 0,
        "anchor_before": None,
        "anchor_applied": False,
        "anchor_request": None,
        "v2_economic_mutation": False,
        "v2_proposal_created": False,
        "v2_time_stop_mutation": False,
        "entry_value_quote_raw": 1_000_000_000,
        "current_executable_value_sol": None,
        "current_executable_gross_return_bps": None,
    }


class AnalysisContractTest(unittest.TestCase):
    def test_identical_input_produces_identical_report(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            path.write_text(json.dumps(fixture(), sort_keys=True) + "\n", encoding="utf-8")
            records, inputs = MODULE.load_records([path])
            first = MODULE.canonical_json(MODULE.analyze(records, inputs, 0.0005))
            second = MODULE.canonical_json(MODULE.analyze(records, inputs, 0.0005))
            self.assertEqual(first, second)
            report = json.loads(first)
            self.assertEqual(report["position_count"], 1)
            self.assertEqual(report["quote_budget"]["hold_quote_count"], 0)
            self.assertIsNone(
                report["quote_budget"]["between_tick_cache_reuse_violation_count"]
            )
            self.assertFalse(report["promotion_gate_passed"])

    def test_v1_owned_quote_is_not_misattributed_to_v2_hold(self) -> None:
        row = fixture()
        row["v1_prequote"] = "quote_required:TakeProfit"
        row["quote_keys"] = ["same-tick-key"]
        row["quote_statuses"] = ["resolved"]
        row["quote_resolution_count"] = 1

        report = MODULE.analyze([row], [], 0.0005)

        self.assertEqual(report["quote_budget"]["hold_quote_count"], 0)

    def test_unsupported_schema_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["schema_version"] = 2
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaises(MODULE.ContractError):
                MODULE.load_records([path])


if __name__ == "__main__":
    unittest.main()
