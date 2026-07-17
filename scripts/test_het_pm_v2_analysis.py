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
        "schema_version": 2,
        "comparison_id": "comparison-1",
        "policy_id": "hierarchical_executable_trajectory_pm_v2",
        "policy_version": 2,
        "policy_config_hash": "config-hash-1",
        "v1_policy_id": "position_manager_lite_exit_policy_v1",
        "v1_policy_version": 1,
        "v1_policy_config_hash": "v1-config-hash-1",
        "time_stop_v2_config_hash": "time-stop-config-hash-1",
        "run_id": "run-1",
        "writer_instance_id": "writer-1",
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
            "exit_apply_status": "not_applied",
            "terminal_commit_status": "not_required",
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


def fixture_for(index: int, *, writer: str = "writer-1", run: str = "run-1") -> dict:
    row = fixture()
    row["comparison_id"] = f"comparison-{index}"
    row["snapshot_id"] = f"snapshot-{index}"
    row["v1_authority_receipt"]["snapshot_id"] = f"snapshot-{index}"
    row["writer_instance_id"] = writer
    row["run_id"] = run
    return row


def writer_health_fixture() -> dict:
    return {
        "schema_version": 2,
        "artifact_type": "het_pm_v2_writer_health",
        "writer_instance_id": "writer-1",
        "run_id": "run-1",
        "mixed_run_ids": False,
        "shutdown_complete": True,
        "policy_id": "hierarchical_executable_trajectory_pm_v2",
        "policy_version": 2,
        "policy_config_hash": "config-hash-1",
        "sidecar_path": "het_pm_v2_observations_v1.jsonl",
        "started_at_ms": 1_000,
        "snapshot_generated_at_ms": 20_000,
        "revision": 3,
        "comparison_attempts": 1,
        "comparison_ready_for_enqueue": 1,
        "core_validation_skips": 0,
        "final_validation_skips": 0,
        "serialization_skips": 0,
        "payload_oversized_skips": 0,
        "enqueue_attempts": 1,
        "enqueued": 1,
        "queue_full_drops": 0,
        "queue_closed_drops": 0,
        "writes_succeeded": 1,
        "writes_failed": 0,
        "cancelled_before_write": 0,
        "terminal_timeouts": 0,
        "terminal_outcome_unknown": 0,
    }


def writer_health_for(
    *,
    writer: str = "writer-1",
    run: str = "run-1",
    comparison_attempts: int = 1,
    ready: int = 1,
    writes: int = 1,
) -> dict:
    health = writer_health_fixture()
    health["writer_instance_id"] = writer
    health["run_id"] = run
    health["comparison_attempts"] = comparison_attempts
    health["comparison_ready_for_enqueue"] = ready
    health["enqueue_attempts"] = ready
    health["enqueued"] = ready
    health["writes_succeeded"] = writes
    return health


class AnalysisContractTest(unittest.TestCase):
    def test_identical_input_produces_identical_report(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            health_path = Path(temp_dir) / "writer-health.json"
            path.write_text(json.dumps(fixture(), sort_keys=True) + "\n", encoding="utf-8")
            health_record = writer_health_fixture()
            health_record["sidecar_path"] = str(path)
            health_path.write_text(
                json.dumps(health_record, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            records, inputs = MODULE.load_records([path])
            health_records, health_inputs = MODULE.load_writer_health([health_path])
            first = MODULE.canonical_json(
                MODULE.analyze(records, inputs, 0.0005, health_records, health_inputs)
            )
            second = MODULE.canonical_json(
                MODULE.analyze(records, inputs, 0.0005, health_records, health_inputs)
            )
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
            self.assertEqual(
                report["coverage"]["writer_health"]["writer_health_evidence_status"],
                "complete",
            )
            self.assertEqual(
                report["coverage"]["writer_health"]["capture_ratio"], 1.0
            )
            self.assertTrue(
                report["coverage"]["writer_health"][
                    "promotion_evidence_available"
                ]
            )

    def test_missing_writer_health_marks_coverage_unknown(self) -> None:
        report = MODULE.analyze([fixture()], [], 0.0005)

        health = report["coverage"]["writer_health"]
        self.assertEqual(health["writer_health_evidence_status"], "missing")
        self.assertTrue(health["coverage_unknown"])
        self.assertFalse(health["promotion_evidence_available"])

    def test_writer_health_exposes_dropped_denominator(self) -> None:
        health_record = writer_health_fixture()
        health_record.update(
            {
                "comparison_attempts": 3,
                "comparison_ready_for_enqueue": 3,
                "enqueue_attempts": 3,
                "enqueued": 1,
                "queue_full_drops": 2,
            }
        )

        report = MODULE.analyze([fixture()], [], 0.0005, [health_record], [])

        health = report["coverage"]["writer_health"]
        self.assertFalse(health["coverage_unknown"])
        self.assertEqual(
            health["writer_health_evidence_status"],
            "complete_with_observation_loss",
        )
        self.assertAlmostEqual(health["capture_ratio"], 1.0 / 3.0)
        self.assertAlmostEqual(health["drop_ratio"], 2.0 / 3.0)
        self.assertFalse(health["promotion_evidence_available"])

    def test_core_validation_skip_is_included_in_producer_denominator(self) -> None:
        health_record = writer_health_for(comparison_attempts=10, ready=9, writes=9)
        health_record["core_validation_skips"] = 1

        report = MODULE.analyze(
            [fixture_for(index) for index in range(1, 10)],
            [],
            0.0005,
            [health_record],
            [],
        )

        health = report["coverage"]["writer_health"]
        self.assertFalse(health["coverage_unknown"])
        self.assertEqual(
            health["writer_health_evidence_status"],
            "complete_with_observation_loss",
        )
        self.assertEqual(health["comparison_attempts"], 10)
        self.assertEqual(health["core_validation_skip_count"], 1)
        self.assertAlmostEqual(health["producer_validity_ratio"], 0.9)
        self.assertAlmostEqual(health["end_to_end_capture_ratio"], 0.9)
        self.assertFalse(health["promotion_evidence_available"])

    def test_final_validation_skip_is_included_in_producer_denominator(self) -> None:
        health_record = writer_health_for(comparison_attempts=4, ready=3, writes=3)
        health_record["final_validation_skips"] = 1

        report = MODULE.analyze(
            [fixture_for(index) for index in range(1, 4)],
            [],
            0.0005,
            [health_record],
            [],
        )

        health = report["coverage"]["writer_health"]
        self.assertEqual(
            health["writer_health_evidence_status"],
            "complete_with_observation_loss",
        )
        self.assertEqual(health["final_validation_skip_count"], 1)
        self.assertAlmostEqual(health["capture_ratio"], 0.75)
        self.assertFalse(health["promotion_evidence_available"])

    def test_serialization_or_oversized_skip_prevents_complete_health_status(self) -> None:
        for field, report_field in (
            ("serialization_skips", "serialization_skip_count"),
            ("payload_oversized_skips", "payload_oversized_skip_count"),
        ):
            with self.subTest(field=field):
                health_record = writer_health_for(comparison_attempts=2, ready=1, writes=1)
                health_record[field] = 1

                report = MODULE.analyze(
                    [fixture_for(1)],
                    [],
                    0.0005,
                    [health_record],
                    [],
                )

                health = report["coverage"]["writer_health"]
                self.assertEqual(
                    health["writer_health_evidence_status"],
                    "complete_with_observation_loss",
                )
                self.assertEqual(health[report_field], 1)
                self.assertFalse(health["promotion_evidence_available"])

    def test_end_to_end_capture_ratio_uses_all_comparison_attempts(self) -> None:
        health_record = writer_health_for(comparison_attempts=5, ready=4, writes=3)
        health_record["core_validation_skips"] = 1
        health_record["enqueued"] = 4
        health_record["queue_full_drops"] = 0
        health_record["writes_failed"] = 1

        report = MODULE.analyze(
            [fixture_for(index) for index in range(1, 4)],
            [],
            0.0005,
            [health_record],
            [],
        )

        health = report["coverage"]["writer_health"]
        self.assertAlmostEqual(health["writer_capture_ratio"], 3.0 / 4.0)
        self.assertAlmostEqual(health["end_to_end_capture_ratio"], 3.0 / 5.0)
        self.assertAlmostEqual(health["capture_ratio"], 3.0 / 5.0)
        self.assertEqual(
            health["writer_health_evidence_status"],
            "complete_with_observation_loss",
        )

    def test_writer_health_cannot_compensate_missing_rows_between_runs(self) -> None:
        rows = [
            fixture_for(1, writer="writer-a", run="run-a"),
            fixture_for(2, writer="writer-b", run="run-b"),
        ]
        health_a = writer_health_for(
            writer="writer-a", run="run-a", comparison_attempts=0, ready=0, writes=0
        )
        health_b = writer_health_for(
            writer="writer-b", run="run-b", comparison_attempts=2, ready=2, writes=2
        )

        report = MODULE.analyze(rows, [], 0.0005, [health_a, health_b], [])

        health = report["coverage"]["writer_health"]
        self.assertTrue(health["coverage_unknown"])
        self.assertFalse(health["sidecar_record_count_match"])
        self.assertEqual(
            health["writer_health_evidence_status"],
            "incomplete_or_inconsistent",
        )
        self.assertFalse(health["promotion_evidence_available"])

    def test_writer_health_cannot_compensate_missing_rows_between_instances(self) -> None:
        rows = [fixture_for(1, writer="writer-a"), fixture_for(2, writer="writer-b")]
        health_a = writer_health_for(writer="writer-a", comparison_attempts=0, ready=0, writes=0)
        health_b = writer_health_for(writer="writer-b", comparison_attempts=2, ready=2, writes=2)

        report = MODULE.analyze(rows, [], 0.0005, [health_a, health_b], [])

        health = report["coverage"]["writer_health"]
        self.assertTrue(health["coverage_unknown"])
        self.assertFalse(health["sidecar_record_count_match"])
        self.assertFalse(health["promotion_evidence_available"])

    def test_health_artifact_for_another_sidecar_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            sidecar_path = Path(temp_dir) / "het_pm_v2_observations_v1.jsonl"
            other_path = Path(temp_dir) / "other_het_pm_v2_observations_v1.jsonl"
            sidecar_path.write_text(json.dumps(fixture()) + "\n", encoding="utf-8")
            other_path.write_text("", encoding="utf-8")
            health = writer_health_fixture()
            health["sidecar_path"] = str(other_path)

            records, inputs = MODULE.load_records([sidecar_path])
            report = MODULE.analyze(records, inputs, 0.0005, [health], [])

            writer_health = report["coverage"]["writer_health"]
            self.assertTrue(writer_health["coverage_unknown"])
            self.assertFalse(writer_health["sidecar_file_identity_match"])
            self.assertFalse(writer_health["promotion_evidence_available"])

    def test_every_comparison_row_requires_exact_writer_instance_match(self) -> None:
        row = fixture()
        health = writer_health_fixture()
        health["writer_instance_id"] = "writer-2"

        report = MODULE.analyze([row], [], 0.0005, [health], [])

        writer_health = report["coverage"]["writer_health"]
        self.assertTrue(writer_health["coverage_unknown"])
        self.assertFalse(writer_health["per_writer_identity_match"])
        self.assertFalse(writer_health["promotion_evidence_available"])

    def test_unclean_writer_health_cannot_support_promotion(self) -> None:
        health_record = writer_health_fixture()
        health_record["shutdown_complete"] = False

        report = MODULE.analyze([fixture()], [], 0.0005, [health_record], [])

        health = report["coverage"]["writer_health"]
        self.assertTrue(health["coverage_unknown"])
        self.assertFalse(health["all_writers_shutdown_cleanly"])
        self.assertFalse(health["promotion_evidence_available"])

    def test_invalid_writer_health_counters_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "writer-health.json"
            row = writer_health_fixture()
            row["writes_succeeded"] = 2
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")

            with self.assertRaisesRegex(
                MODULE.ContractError, "completion counters exceed enqueued"
            ):
                MODULE.load_writer_health([path])

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
            row["schema_version"] = 999
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaises(MODULE.ContractError):
                MODULE.load_records([path])

    def test_mixed_policy_config_hash_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            first = fixture()
            second = fixture()
            second["comparison_id"] = "comparison-2"
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
            second["comparison_id"] = "comparison-2"
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
            second["comparison_id"] = "comparison-2"
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
            row["v1_final"] = "ExitApplied"
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ContractError, "disagrees with V1 receipt"):
                MODULE.load_records([path])

    def test_exit_applied_with_terminal_commit_pending_is_valid(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["terminal_tick"] = True
            row["v1_final"] = "ExitApplied"
            row["v1_authority_receipt"].update(
                {
                    "outcome": "exit_applied",
                    "exit_apply_status": "applied",
                    "terminal_commit_status": "pending",
                    "action_id": "action-1",
                }
            )
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")

            records, _ = MODULE.load_records([path])

            self.assertEqual(len(records), 1)
            self.assertEqual(
                records[0]["v1_authority_receipt"]["terminal_commit_status"],
                "pending",
            )

    def test_exit_applied_cannot_claim_terminal_not_required(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            row = fixture()
            row["terminal_tick"] = True
            row["v1_final"] = "ExitApplied"
            row["v1_authority_receipt"].update(
                {
                    "outcome": "exit_applied",
                    "exit_apply_status": "applied",
                    "terminal_commit_status": "not_required",
                    "action_id": "action-1",
                }
            )
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")

            with self.assertRaisesRegex(MODULE.ContractError, "axes disagree"):
                MODULE.load_records([path])

    def test_duplicate_comparison_id_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "input.jsonl"
            first = fixture()
            second = fixture()
            second["snapshot_id"] = "snapshot-2"
            second["v1_authority_receipt"]["snapshot_id"] = "snapshot-2"
            path.write_text(
                json.dumps(first) + "\n" + json.dumps(second) + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(MODULE.ContractError, "duplicate comparison_id"):
                MODULE.load_records([path])


if __name__ == "__main__":
    unittest.main()
