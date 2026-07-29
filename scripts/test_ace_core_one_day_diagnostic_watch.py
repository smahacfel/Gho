#!/usr/bin/env python3
"""Focused contract tests for the ACE IPC diagnostic watcher."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_core_one_day_diagnostic_watch.py")
SPEC = importlib.util.spec_from_file_location("ace_core_one_day_diagnostic_watch", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
WATCHER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = WATCHER
SPEC.loader.exec_module(WATCHER)


class AceCoreOneDayDiagnosticWatchTests(unittest.TestCase):
    def test_empty_launcher_stdout_times_out_without_a_saturation_marker(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            launcher_stdout = root / "launcher.stdout.log"
            launcher_stdout.write_text("oracle log marker elsewhere is ignored\n", encoding="utf-8")

            result = WATCHER.watch(
                launcher_stdout_log=launcher_stdout,
                launcher_pid=999_999,
                timeout_seconds=0.01,
                poll_seconds=0.001,
                metrics_url=None,
                metrics_output=None,
                status_path=None,
                dry_run=True,
            )

            self.assertEqual(result.reason, "timeout")
            self.assertFalse(result.marker_seen)
            self.assertIsNone(result.stopped_pid)

    def test_launcher_stdout_saturation_marker_stops_the_diagnostic_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            launcher_stdout = root / "launcher.stdout.log"
            launcher_stdout.write_text("2026-07-29T00:00:00Z IPC_EGRESS_SATURATED\n", encoding="utf-8")

            result = WATCHER.watch(
                launcher_stdout_log=launcher_stdout,
                launcher_pid=999_999,
                timeout_seconds=1.0,
                poll_seconds=0.001,
                metrics_url=None,
                metrics_output=None,
                status_path=None,
                dry_run=True,
            )

            self.assertEqual(result.reason, "ipc_egress_saturated")
            self.assertTrue(result.marker_seen)
            self.assertIsNone(result.stopped_pid)


if __name__ == "__main__":
    unittest.main()
