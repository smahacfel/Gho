#!/usr/bin/env python3
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import check_selector_lifecycle_canary as canary
import start_selector_lifecycle_run as launcher
import guard_restore_shadow_lifecycle as restore_guard


class SelectorLifecycleRunGuardTests(unittest.TestCase):
    def test_event_canary_requires_feature_events_and_diag(self) -> None:
        status, errors = canary.validate_event_canary(
            {
                "NewPoolDetected": 1,
                "Candidate": 1,
                "PoolTransaction": 1,
            },
            diag_delta=3,
            bad_event_json_delta=0,
        )

        self.assertEqual(canary.PASS_STATUS, status)
        self.assertEqual([], errors)

    def test_event_canary_fails_without_diag(self) -> None:
        status, errors = canary.validate_event_canary(
            {
                "NewPoolDetected": 1,
                "Candidate": 1,
                "PoolTransaction": 1,
            },
            diag_delta=0,
            bad_event_json_delta=0,
        )

        self.assertEqual(canary.FAIL_EVENT_CANARY, status)
        self.assertIn("DIAG_ACCOUNT_UPDATE_RELAY_delta <= 0", errors)

    def test_event_kind_ignores_non_scalar_type_field(self) -> None:
        kind = canary.detect_event_kind(
            {
                "type": {"huge": "not-a-kind"},
                "payload": {"event_type": "PoolTransaction"},
            }
        )

        self.assertEqual("PoolTransaction", kind)

    def test_lifecycle_canary_passes_full_lifecycle_delta(self) -> None:
        rows = [
            {
                "record_type": "shadow_dispatch",
                "dispatch_status": "closed",
                "simulation_outcome": "closed",
                "selected_route_kind": "legacy_buy",
                "execution_feasibility_status": "executable",
            },
            {
                "record_type": "exit_filled",
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "final_pnl_pct": 12.5,
            },
            {
                "record_type": "position_closed",
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "final_pnl_pct": 12.5,
                "close_reason": "Target",
            },
        ]
        summary = canary.summarize_lifecycle_delta(rows)
        status, errors = canary.validate_lifecycle_canary(
            {
                "shadow_buys_delta": 1,
                "shadow_entries_delta": 1,
                "shadow_lifecycle_delta": 3,
            },
            summary,
        )

        self.assertEqual(canary.PASS_STATUS, status)
        self.assertEqual([], errors)

    def test_lifecycle_canary_fails_account_not_found_delta(self) -> None:
        rows = [
            {
                "record_type": "shadow_dispatch",
                "dispatch_status": "failed",
                "simulation_error_message": "AccountNotFound",
            }
        ]
        summary = canary.summarize_lifecycle_delta(rows)
        status, errors = canary.validate_lifecycle_canary(
            {
                "shadow_buys_delta": 1,
                "shadow_entries_delta": 1,
                "shadow_lifecycle_delta": 1,
            },
            summary,
        )

        self.assertEqual(canary.FAIL_LIFECYCLE_PROOF, status)
        self.assertIn("AccountNotFound_delta > 0", errors)

    def test_lifecycle_canary_fails_account_not_found_from_full_delta_markers(self) -> None:
        rows = [
            {
                "record_type": "shadow_dispatch",
                "dispatch_status": "closed",
                "simulation_outcome": "closed",
                "selected_route_kind": "legacy_buy",
                "execution_feasibility_status": "executable",
            },
            {
                "record_type": "exit_filled",
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "final_pnl_pct": 1.0,
            },
            {
                "record_type": "position_closed",
                "truth_status": "resolved",
                "truth_source": "canonical_account_state_snapshot",
                "final_pnl_pct": 1.0,
                "close_reason": "TimeStop",
            },
        ]
        summary = canary.summarize_lifecycle_delta(rows)
        status, errors = canary.validate_lifecycle_canary(
            {
                "shadow_buys_delta": 1,
                "shadow_entries_delta": 1,
                "shadow_lifecycle_delta": 3,
            },
            summary,
            {"AccountNotFound": 1},
        )

        self.assertEqual(canary.FAIL_LIFECYCLE_PROOF, status)
        self.assertIn("AccountNotFound_delta > 0", errors)

    def test_scope_contract_requires_artifact_paths_to_match_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "r8.toml"
            config_path.write_text(
                'scope = "shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag"\n'
                '[logging]\n'
                'level = "info"\n'
                '[execution]\n'
                'execution_mode = "shadow"\n'
                'entry_mode = "shadow_only"\n',
                encoding="utf-8",
            )
            artifact_paths = restore_guard.ArtifactPaths(
                shadow_buys=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag-buys.jsonl"),
                shadow_entries=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/shadow_entries.jsonl"),
                shadow_lifecycle=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/shadow_lifecycle.jsonl"),
                system_log=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/system.log"),
                oracle_log=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/oracle.log"),
            )

            status, errors = launcher.validate_scope_contract(
                scope="shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag",
                config_path=config_path,
                config={
                    "logging": {"level": "info"},
                    "trigger": {"entry_mode": "shadow_only"},
                    "execution": {"execution_mode": "shadow"},
                },
                artifact_paths=artifact_paths,
            )

        self.assertEqual(launcher.PASS_STATUS, status)
        self.assertEqual([], errors)

    def test_scope_contract_blocks_old_scope_residue(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "r8.toml"
            config_path.write_text(
                'scope = "shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag"\n',
                encoding="utf-8",
            )
            artifact_paths = restore_guard.ArtifactPaths(
                shadow_buys=Path("/tmp/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag-buys.jsonl"),
                shadow_entries=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/shadow_entries.jsonl"),
                shadow_lifecycle=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/shadow_lifecycle.jsonl"),
                system_log=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/system.log"),
                oracle_log=Path("/tmp/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag/oracle.log"),
            )

            status, errors = launcher.validate_scope_contract(
                scope="shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag",
                config_path=config_path,
                config={
                    "logging": {"level": "info"},
                    "trigger": {"entry_mode": "shadow_only"},
                    "execution": {"execution_mode": "shadow"},
                },
                artifact_paths=artifact_paths,
            )

        self.assertEqual(launcher.FAIL_CONFIG_CONTRACT, status)
        self.assertTrue(any("shadow_buys" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
