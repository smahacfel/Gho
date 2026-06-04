#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import guard_restore_shadow_lifecycle as guard


class RestoreShadowLifecycleGuardTests(unittest.TestCase):
    def test_reporter_rows_written_and_resolved_truth_passes(self) -> None:
        rows = [
            {
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "close_reason": "TimeStop",
                "timing": {"gatekeeper_buy_context_found": True},
                "shadow": {"final_pnl_pct": -1.25},
                "exit_fills": [{"fill_index": 1}],
            }
        ]

        result = guard.validate_reporter_rows(
            rows,
            min_rows_written=1,
            require_resolved=True,
            reporter_stdout="close_truth_coverage=1/1",
        )

        self.assertEqual(guard.PASS_STATUS, result.status)
        self.assertEqual(1, result.rows_written)
        self.assertEqual("1/1", result.close_truth_coverage)
        self.assertEqual(1, result.truth_status_resolved_rows)
        self.assertEqual(1, result.final_pnl_pct_present_rows)
        self.assertEqual(1, result.exit_fills_total)

    def test_reporter_rows_written_zero_fails_no_rows(self) -> None:
        result = guard.validate_reporter_rows(
            [],
            min_rows_written=1,
            require_resolved=True,
            reporter_stdout="rows_written=0",
        )

        self.assertEqual(guard.FAIL_REPORTER_NO_ROWS, result.status)
        self.assertIn("rows_written=0", result.errors[0])

    def test_runtime_artifacts_fail_on_unsupported_legacy_marker(self) -> None:
        artifact_deltas = {
            "shadow_buys_delta": 1,
            "shadow_entries_delta": 1,
            "shadow_lifecycle_delta": 1,
            "diag_account_update_relay_delta": 1,
        }
        marker_counts = {
            "ResourceExhausted": 0,
            "relative URL without a base": 0,
            "Custom(6062)": 0,
            "custom program error: 0x17ae": 0,
            "0x17ae": 0,
            "unsupported_legacy_buy_layout_requires_bcv2": 1,
            "buy_remaining_account_count=2": 1,
            "DIAG_ACCOUNT_UPDATE_RELAY": 1,
        }
        lifecycle_matrix = {
            "legacy_buy_executable_rows": 1,
            "dispatch_attempted_rows": 1,
            "simulation_attempted_rows": 1,
            "unsupported_legacy_buy_layout_requires_bcv2_rows": 0,
        }

        status, errors = guard.validate_runtime_artifacts(
            artifact_deltas,
            marker_counts,
            lifecycle_matrix,
        )

        self.assertEqual(guard.FAIL_RUNTIME_ARTIFACTS, status)
        self.assertIn("unsupported_legacy_buy_layout_requires_bcv2 > 0", errors)

    def test_critical_file_changed_requires_guard(self) -> None:
        required, changed = guard.guard_required_for_changed_files(
            [
                "ghost-launcher/src/oracle_runtime.rs",
                "README.md",
            ]
        )

        self.assertTrue(required)
        self.assertEqual(["ghost-launcher/src/oracle_runtime.rs"], changed)

    def test_non_critical_file_changed_does_not_require_guard(self) -> None:
        required, changed = guard.guard_required_for_changed_files(
            [
                "README.md",
                "docs/notes/example.md",
            ]
        )

        self.assertFalse(required)
        self.assertEqual([], changed)

    def test_preflight_provider_or_env_failure_is_inconclusive(self) -> None:
        text = "trigger.rpc_url: jsonrpc getVersion failed: missing env GHOST_TRIGGER_RPC_URL"

        status = guard.classify_preflight_failure(text)

        self.assertEqual(guard.INCONCLUSIVE_ENV_OR_CONFIG, status)
        self.assertEqual(2, guard.exit_code_for_status(status))

    def test_simulated_failure_status_exits_one(self) -> None:
        self.assertEqual(1, guard.exit_code_for_status(guard.FAIL_RUNTIME_ARTIFACTS))
        self.assertEqual(1, guard.exit_code_for_status(guard.FAIL_REPORTER_NO_ROWS))


if __name__ == "__main__":
    unittest.main()
