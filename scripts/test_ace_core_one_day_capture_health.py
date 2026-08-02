#!/usr/bin/env python3
"""Focused fail-closed tests for ACE capture-health evidence."""

from __future__ import annotations

import argparse
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_core_one_day_capture_health.py")
SPEC = importlib.util.spec_from_file_location("ace_capture_health", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
health = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(health)


class AceCaptureHealthTests(unittest.TestCase):
    def make_fixture(self) -> tuple[Path, Path, Path, Path, Path, Path]:
        root = Path(tempfile.mkdtemp(prefix="ace-capture-health-test-"))
        manifest_path = root / "manifest.json"
        receipt_path = root / "capture_health_v1.json"
        events_dir = root / "events"
        events_dir.mkdir()
        log_path = root / "launcher.log"
        manifest = {
            "run_id": "smoke-run",
            "health_evidence_path": str(receipt_path),
        }
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        manifest_sha256 = health.sha256_hex(manifest_path.read_bytes())
        self.write_snapshot(
            root / "start.json",
            manifest_sha256,
            phase="start",
            captured_at_unix_ms=1_000,
        )
        self.write_snapshot(
            root / "end.json",
            manifest_sha256,
            phase="end",
            captured_at_unix_ms=121_000,
        )
        event = {
            "kind": {
                "type": "NewPoolDetected",
                "payload": {},
            }
        }
        trade = {
            "kind": {
                "type": "PoolTransaction",
                "payload": {
                    "success": True,
                    "is_synthetic": False,
                    "slot": 1,
                    "tx_index": 0,
                    "outer_instruction_index": 0,
                    "inner_group_index": 0,
                    "event_ordinal": 0,
                    "virtual_sol_reserves": 1,
                    "virtual_token_reserves": 1,
                    "real_sol_reserves": 1,
                    "real_token_reserves": 1,
                    "signer_pre_balance_lamports": 2,
                    "signer_post_balance_lamports": 1,
                },
            }
        }
        (events_dir / "exec_smoke_0000.jsonl").write_text(
            json.dumps(event) + "\n" + json.dumps(trade) + "\n", encoding="utf-8"
        )
        log_path.write_text("Ghost Launcher shutdown complete\n", encoding="utf-8")
        return (
            root,
            manifest_path,
            receipt_path,
            events_dir,
            root / "start.json",
            root / "end.json",
        )

    def write_snapshot(
        self,
        path: Path,
        manifest_sha256: str,
        *,
        phase: str,
        captured_at_unix_ms: int,
        counters: dict[str, int] | None = None,
    ) -> None:
        payload = {
            "schema_version": health.METRICS_SNAPSHOT_SCHEMA_VERSION,
            "run_id": "smoke-run",
            "manifest_sha256": manifest_sha256,
            "phase": phase,
            "capture_kind": "smoke",
            "captured_at_unix_ms": captured_at_unix_ms,
            "counters": counters
            or {name: 0 for name in health.REQUIRED_COUNTERS},
            "raw_metrics_sha256": "raw-metrics-hash",
        }
        path.write_text(json.dumps(payload), encoding="utf-8")

    def finalize_args(
        self,
        manifest_path: Path,
        events_dir: Path,
        start_path: Path,
        end_path: Path,
        receipt_path: Path,
    ) -> argparse.Namespace:
        return argparse.Namespace(
            manifest=str(manifest_path),
            capture_kind="smoke",
            events_dir=str(events_dir),
            start_metrics=str(start_path),
            end_metrics=str(end_path),
            log=[str(manifest_path.parent / "launcher.log")],
            output=str(receipt_path),
        )

    def test_valid_manifest_bound_smoke_writes_receipt(self) -> None:
        root, manifest, receipt, events, start, end = self.make_fixture()
        result = health.finalize(self.finalize_args(manifest, events, start, end, receipt))
        self.assertEqual(result, 0)
        saved = json.loads(receipt.read_bytes())
        self.assertEqual(saved["schema_version"], health.HEALTH_SCHEMA_VERSION)
        self.assertEqual(saved["capture_kind"], "smoke")
        self.assertEqual(saved["duration_ms"], health.SMOKE_MIN_DURATION_MS)
        self.assertTrue(saved["start_snapshot_sha256"])
        self.assertTrue(saved["end_snapshot_sha256"])
        self.assertTrue(root.exists())

    def test_ten_minute_smoke_duration_is_the_upper_bound(self) -> None:
        duration_ms, failures = health.validate_capture_duration(
            "smoke", 1_000, 1_000 + health.SMOKE_MAX_DURATION_MS
        )
        self.assertEqual(duration_ms, health.SMOKE_MAX_DURATION_MS)
        self.assertEqual(failures, [])

        _, failures = health.validate_capture_duration(
            "smoke", 1_000, 1_001 + health.SMOKE_MAX_DURATION_MS
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("outside", failures[0])

    def test_thirty_minute_resilience_soak_has_its_own_duration_contract(self) -> None:
        duration_ms, failures = health.validate_capture_duration(
            "soak", 1_000, 1_000 + health.SOAK_MIN_DURATION_MS
        )
        self.assertEqual(duration_ms, health.SOAK_MIN_DURATION_MS)
        self.assertEqual(failures, [])

        _, failures = health.validate_capture_duration(
            "soak", 1_000, 1_000 + health.SOAK_MIN_DURATION_MS - 1
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("outside", failures[0])

    def test_ace_ev_v2_yield_qualification_has_its_own_enrollment_plus_drain_contract(self) -> None:
        duration_ms, failures = health.validate_capture_duration(
            "yield_qualification",
            1_000,
            1_000 + health.YIELD_QUALIFICATION_MIN_DURATION_MS,
        )
        self.assertEqual(duration_ms, health.YIELD_QUALIFICATION_MIN_DURATION_MS)
        self.assertEqual(failures, [])

        _, failures = health.validate_capture_duration(
            "yield_qualification",
            1_000,
            1_000 + health.YIELD_QUALIFICATION_MIN_DURATION_MS - 1,
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("yield qualification duration", failures[0])

    def test_invalid_snapshot_does_not_write_health_receipt(self) -> None:
        _, manifest, receipt, events, start, end = self.make_fixture()
        snapshot = json.loads(end.read_bytes())
        del snapshot["counters"]["pr1_runtime_bypass_attempt_total"]
        end.write_text(json.dumps(snapshot), encoding="utf-8")
        result = health.finalize(self.finalize_args(manifest, events, start, end, receipt))
        self.assertEqual(result, 2)
        self.assertFalse(receipt.exists())

    def test_empty_event_file_beside_valid_tape_does_not_fail_finalize(self) -> None:
        _, manifest, receipt, events, start, end = self.make_fixture()
        (events / "exec_launcher_empty_0000.jsonl").write_bytes(b"")

        result = health.finalize(self.finalize_args(manifest, events, start, end, receipt))

        self.assertEqual(result, 0)
        self.assertTrue(receipt.exists())

    def test_nonempty_event_file_without_final_newline_fails_without_receipt(self) -> None:
        _, manifest, receipt, events, start, end = self.make_fixture()
        tape = events / "exec_smoke_0000.jsonl"
        tape.write_bytes(tape.read_bytes()[:-1])

        result = health.finalize(self.finalize_args(manifest, events, start, end, receipt))

        self.assertEqual(result, 2)
        self.assertFalse(receipt.exists())

    def test_fee_authority_advisory_marker_does_not_invalidate_ace_logs(self) -> None:
        root, _, _, _, _, _ = self.make_fixture()
        log_path = root / "launcher.log"
        log_path.write_text(
            "Ghost Launcher shutdown complete\n"
            "RUG_SCALP_RUNTIME_FEE_AUTHORITY_CHANGED_ADVISORY\n",
            encoding="utf-8",
        )
        logs_ok, _, _, _, failures = health.validate_logs([log_path])
        self.assertTrue(logs_ok)
        self.assertEqual(failures, [])

    def test_segment_invalid_counter_fails_without_claiming_valid_capture(self) -> None:
        _, manifest, receipt, events, start, end = self.make_fixture()
        snapshot = json.loads(end.read_bytes())
        snapshot["counters"]["ace_capture_segment_invalid_total"] = 1
        end.write_text(json.dumps(snapshot), encoding="utf-8")

        result = health.finalize(self.finalize_args(manifest, events, start, end, receipt))

        self.assertEqual(result, 2)
        self.assertFalse(receipt.exists())

    def test_lifecycle_status_preserves_launcher_root_cause_when_metrics_are_unavailable(self) -> None:
        root, manifest, _, _, _, _ = self.make_fixture()
        status = root / "lifecycle_status.json"
        status.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "run_id": "smoke-run",
                    "manifest_sha256": health.sha256_hex(manifest.read_bytes()),
                    "launcher_returncode": 1,
                    "exit_reason": "semantic_runtime_failure",
                }
            ),
            encoding="utf-8",
        )

        failures = health.load_lifecycle_status(
            status,
            expected_run_id="smoke-run",
            expected_manifest_sha256=health.sha256_hex(manifest.read_bytes()),
        )

        self.assertTrue(any("semantic_runtime_failure" in item for item in failures))

    def test_verify_probe_requires_valid_capture_without_invalid_reasons(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="ace-probe-summary-test-"))
        summary_path = root / "summary_v1.json"
        summary_path.write_text(
            json.dumps({"capture_status": "VALID_CAPTURE", "capture_invalid_reasons": []}),
            encoding="utf-8",
        )
        self.assertEqual(health.verify_probe(argparse.Namespace(summary=str(summary_path))), 0)
        summary_path.write_text(
            json.dumps(
                {
                    "capture_status": "INVALID_CAPTURE",
                    "capture_invalid_reasons": ["missing_receipt"],
                }
            ),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(RuntimeError, "capture_status"):
            health.verify_probe(argparse.Namespace(summary=str(summary_path)))


if __name__ == "__main__":
    unittest.main()
