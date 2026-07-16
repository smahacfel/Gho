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
        "policy_version": 2,
        "policy_config_hash": "config-hash-1",
        "v1_policy_id": "position_manager_lite_exit_policy_v1",
        "v1_policy_version": 1,
        "v1_policy_config_hash": "v1-config-hash-1",
        "time_stop_v2_config_hash": "time-stop-config-hash-1",
        "run_id": "run-1",
        "lane": "shadow",
        "position_id": "position-1",
        "position_epoch": 1,
        "state_revision": 7,
        "remaining_quantity_raw": 100,
        "snapshot_id": "snapshot-1",
        "observation_timestamp_ms": 16_000,
        "terminal_tick": False,
        "trajectory_sampling_mode": "latest_canonical_state_per_monitor_tick",
        "trajectory_measurement_grade": "online_non_lookahead_sampled_trajectory",
        "monitor_tick_ms": 500,
        "trajectory": {
            "return_1500ms_bps": 10,
            "return_5s_bps": 20,
            "return_15s_bps": 30,
            "peak_mark_price_sol": 1.2,
            "peak_sample_slot": 10,
            "peak_sample_timestamp_ms": 15_000,
            "drawdown_from_peak_bps": 100,
            "time_since_peak_ms": 1_000,
            "peak_giveback_velocity_bps_per_sec": 100,
            "newest_sample_slot": 11,
            "newest_sample_timestamp_ms": 16_000,
            "newest_sample_age_ms": 0,
            "distinct_slots_1500ms": 2,
            "state_update_delta_since_previous_sample": 1,
            "quality": "usable",
            "flags": 0,
        },
        "vitality": {
            "current_state": "alive",
            "consecutive_non_alive_windows": 0,
            "last_window_at_ms": 16_000,
            "last_alive_at_ms": 16_000,
            "latest_window_price_delta_bps": 100,
            "latest_window_state_update_delta": 1,
            "quality_fresh": True,
        },
        "route_status": "pump_curve_supported",
        "v2_winning_gate": "hold",
        "v2_suppressed_gates_mask": 0,
        "v1_prequote": "hold",
        "v1_crash_prequote": "Disabled",
        "v1_final": "Hold",
        "v1_authority_receipt": {
            "snapshot_id": "snapshot-1",
            "state_revision": 7,
            "remaining_quantity_raw": 100,
            "outcome": "hold",
            "action_id": None,
            "reason": None,
            "crash_quote_decision": None,
        },
        "v2_prequote": "Hold",
        "v2_final": "Hold",
        "v2_crash_quote_decision": None,
        "consumed_by_policy": False,
        "v1_shadow_authority": True,
        "v2_shadow_authority": False,
        "live_authority": False,
        "quote_keys": [],
        "quote_statuses": [],
        "quote_resolution_count": 0,
        "anchor_before": None,
        "anchor_applied": False,
        "anchor_request": None,
        "v2_economic_mutation": False,
        "v2_proposal_created": False,
        "v2_time_stop_mutation": False,
        "duplicate_action_observed": False,
        "route_build_authority_changed": False,
        "terminal_isolation_violation": False,
        "entry_value_quote_raw": 1_000_000_000,
        "entry_value_source": "persisted_entry_amount",
        "entry_value_authoritative_for_shadow": True,
        "current_executable_value_sol": None,
        "current_executable_gross_return_bps": None,
        "known_estimated_costs_sol": None,
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
            self.assertEqual(
                report["evidence_contract"]["v1_authority"]["policy_config_hash"],
                "v1-config-hash-1",
            )
            self.assertEqual(
                report["evidence_contract"]["time_stop_v2_source"]["config_hash"],
                "time-stop-config-hash-1",
            )
            self.assertEqual(inputs[0]["record_count"], 1)
            self.assertEqual(inputs[0]["v1_policy_config_hash"], "v1-config-hash-1")
            self.assertEqual(
                inputs[0]["time_stop_v2_config_hash"],
                "time-stop-config-hash-1",
            )
            self.assertEqual(report["quote_budget"]["hold_quote_count"], 0)
            self.assertIsNone(
                report["quote_budget"]["between_tick_cache_reuse_violation_count"]
            )
            self.assertFalse(report["promotion_gate_passed"])
            self.assertFalse(
                report["producer_asserted_integrity"]["promotion_evidence"]
            )
            self.assertEqual(
                report["independently_measured_integrity"]["status"],
                "not_evaluated_requires_lifecycle_reconciliation_artifact",
            )

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

    def test_mixed_policy_config_hash_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            first = fixture()
            second = fixture()
            second["snapshot_id"] = "snapshot-2"
            second["v1_authority_receipt"]["snapshot_id"] = "snapshot-2"
            second["policy_config_hash"] = "config-hash-2"
            path.write_text(
                json.dumps(first) + "\n" + json.dumps(second) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MODULE.ContractError, "mixed evidence contracts"):
                MODULE.load_records([path])

    def test_mixed_v1_policy_config_hash_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            first = fixture()
            second = fixture()
            second["snapshot_id"] = "snapshot-2"
            second["v1_authority_receipt"]["snapshot_id"] = "snapshot-2"
            second["v1_policy_config_hash"] = "v1-config-hash-2"
            path.write_text(
                json.dumps(first) + "\n" + json.dumps(second) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MODULE.ContractError, "mixed evidence contracts"):
                MODULE.load_records([path])

    def test_mixed_time_stop_v2_config_hash_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            first = fixture()
            second = fixture()
            second["snapshot_id"] = "snapshot-2"
            second["v1_authority_receipt"]["snapshot_id"] = "snapshot-2"
            second["time_stop_v2_config_hash"] = "time-stop-config-hash-2"
            path.write_text(
                json.dumps(first) + "\n" + json.dumps(second) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(MODULE.ContractError, "mixed evidence contracts"):
                MODULE.load_records([path])

    def test_wrong_lane_or_authority_is_rejected(self) -> None:
        for field, value in (("lane", "live"), ("v1_shadow_authority", False), ("v2_shadow_authority", True)):
            with self.subTest(field=field):
                with tempfile.TemporaryDirectory() as temp_dir:
                    path = Path(temp_dir) / "input.jsonl"
                    row = fixture()
                    row[field] = value
                    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
                    with self.assertRaises(MODULE.ContractError):
                        MODULE.load_records([path])

    def test_non_finite_json_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["current_executable_value_sol"] = float("nan")
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ContractError, "non-finite"):
                MODULE.load_records([path])

    def test_missing_strict_provenance_field_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            del row["trajectory_measurement_grade"]
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ContractError, "missing required field"):
                MODULE.load_records([path])

    def test_unknown_decision_enum_label_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["v2_final"] = "MagicExit"
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ContractError, "invalid v2_final enum label"):
                MODULE.load_records([path])

    def test_v1_final_must_match_actual_authority_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["v1_final"] = "TerminalApplied"
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ContractError, "disagrees with V1 receipt"):
                MODULE.load_records([path])


if __name__ == "__main__":
    unittest.main()
