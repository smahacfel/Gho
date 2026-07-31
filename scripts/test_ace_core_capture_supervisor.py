#!/usr/bin/env python3
"""Focused tests for non-authoritative ACE capture supervision."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_core_capture_supervisor.py")
SPEC = importlib.util.spec_from_file_location("ace_capture_supervisor", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
supervisor = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = supervisor
SPEC.loader.exec_module(supervisor)


class AceCaptureSupervisorTests(unittest.TestCase):
    def make_state(self) -> supervisor.LifecycleState:
        return supervisor.LifecycleState(
            run_id="resilience-smoke",
            manifest_sha256="manifest-hash",
            capture_kind="smoke",
            launcher_pid=123,
        )

    def test_one_unavailable_metrics_scrape_never_requests_shutdown(self) -> None:
        state = self.make_state()
        state.metrics.failure(TimeoutError("timed out"))

        self.assertFalse(state.shutdown_requested)
        self.assertEqual(state.metrics.consecutive_failures, 1)
        self.assertEqual(state.metrics.last_error_class, "timeout")

    def test_multiple_metrics_failures_preserve_liveness_state_not_false_root_cause(self) -> None:
        state = self.make_state()
        for _ in range(3):
            state.metrics.failure(ConnectionResetError("connection reset"))

        self.assertFalse(state.shutdown_requested)
        self.assertEqual(state.metrics.total_failures, 3)
        self.assertEqual(state.metrics.last_error_class, "transport")

    def test_metrics_retries_back_off_without_acquiring_shutdown_authority(self) -> None:
        self.assertEqual(supervisor.metrics_retry_delay_s(1.0, 1), 1.0)
        self.assertEqual(supervisor.metrics_retry_delay_s(1.0, 2), 2.0)
        self.assertEqual(supervisor.metrics_retry_delay_s(1.0, 10), 30.0)

    def test_success_after_failures_restores_last_known_good_without_erasing_history(self) -> None:
        state = self.make_state()
        state.metrics.failure(TimeoutError("timeout"))
        state.metrics.success(b"metric 1\n")

        self.assertEqual(state.metrics.consecutive_failures, 0)
        self.assertEqual(state.metrics.total_failures, 1)
        self.assertIsNotNone(state.metrics.last_success_at_unix_ms)
        self.assertIsNotNone(state.metrics.last_success_metrics_sha256)

    def test_endpoint_missing_during_shutdown_is_classified_not_promoted_to_exit_reason(self) -> None:
        state = self.make_state()
        state.shutdown_requested = True
        state.metrics.failure(ConnectionRefusedError("connection refused"))
        state.endpoint_state = "unavailable_during_or_before_controlled_shutdown"

        payload = state.as_json()
        self.assertTrue(payload["shutdown_requested"])
        self.assertEqual(payload["endpoint_state"], "unavailable_during_or_before_controlled_shutdown")
        self.assertEqual(payload["metrics"]["last_error_class"], "transport")

    def test_launcher_exit_reason_is_persistable_without_a_final_scrape(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="ace-supervisor-test-"))
        log = root / "launcher.stdout.log"
        log.write_text("Ghost Launcher shutdown complete\n", encoding="utf-8")

        self.assertEqual(supervisor.derive_launcher_exit_reason([log], 0), "Ghost Launcher shutdown complete")
        self.assertEqual(json.loads(json.dumps(self.make_state().as_json()))["run_id"], "resilience-smoke")

    def test_supervisor_snapshot_is_manifest_bound_and_contains_all_health_counters(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="ace-supervisor-test-"))
        output = root / "start.prom.json"
        manifest = {"run_id": "resilience-smoke"}
        body = (
            b"pr1_runtime_bypass_attempt_total 0\n"
            b"pr1_runtime_candidate_admission_closed_total 0\n"
            b"pr1_runtime_primary_coverage_gap_total 0\n"
            b"ace_capture_segment_invalid_total 0\n"
        )

        supervisor.write_bound_metrics_snapshot(
            output,
            manifest=manifest,
            manifest_sha256="manifest-hash",
            capture_kind="smoke",
            phase="start",
            body=body,
        )

        snapshot = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(snapshot["run_id"], "resilience-smoke")
        self.assertEqual(snapshot["manifest_sha256"], "manifest-hash")
        self.assertEqual(snapshot["phase"], "start")
        self.assertEqual(snapshot["counters"], {
            "pr1_runtime_bypass_attempt_total": 0,
            "pr1_runtime_candidate_admission_closed_total": 0,
            "pr1_runtime_primary_coverage_gap_total": 0,
            "ace_capture_segment_invalid_total": 0,
        })

    def test_snapshot_missing_required_health_counter_is_not_silently_accepted(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="ace-supervisor-test-"))
        with self.assertRaisesRegex(RuntimeError, "does not expose required ACE health series"):
            supervisor.write_bound_metrics_snapshot(
                root / "end.prom.json",
                manifest={"run_id": "resilience-smoke"},
                manifest_sha256="manifest-hash",
                capture_kind="smoke",
                phase="end",
                body=b"pr1_runtime_bypass_attempt_total 0\n",
            )

    def test_last_known_good_end_snapshot_keeps_its_actual_observation_time(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="ace-supervisor-test-"))
        output = root / "end.prom.json"
        observed_at = 1_700_000_000_000
        body = (
            b"pr1_runtime_bypass_attempt_total 0\n"
            b"pr1_runtime_candidate_admission_closed_total 0\n"
            b"pr1_runtime_primary_coverage_gap_total 0\n"
            b"ace_capture_segment_invalid_total 0\n"
        )

        supervisor.write_bound_metrics_snapshot(
            output,
            manifest={"run_id": "resilience-smoke"},
            manifest_sha256="manifest-hash",
            capture_kind="smoke",
            phase="end",
            body=body,
            captured_at_unix_ms=observed_at,
            source="last_known_good",
        )

        snapshot = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(snapshot["source"], "last_known_good")
        self.assertEqual(snapshot["captured_at_unix_ms"], observed_at)


if __name__ == "__main__":
    unittest.main()
