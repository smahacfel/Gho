#!/usr/bin/env python3
from __future__ import annotations

import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import check_selector_lifecycle_canary as canary
import start_selector_lifecycle_run as launcher
import guard_restore_shadow_lifecycle as restore_guard


class SelectorLifecycleRunGuardTests(unittest.TestCase):
    def test_release_build_uses_locked_command_and_records_reproducibility_contract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            launcher_path = root / "target" / "release" / "ghost-launcher"
            launcher_path.parent.mkdir(parents=True)
            launcher_path.write_bytes(b"binary")
            output_dir = root / "reports" / "run"

            def run_command(command: list[str], **_: object) -> dict[str, object]:
                if command == list(launcher.RELEASE_CLEAN_COMMAND):
                    launcher_path.unlink()
                    label = "clean"
                else:
                    launcher_path.write_bytes(b"rebuilt-binary")
                    label = "build"
                return {
                    "command": command,
                    "exit_code": 0,
                    "log_path": str(output_dir / f"{label}.log"),
                }

            with (
                mock.patch.object(launcher, "git_worktree_is_clean", side_effect=[True, True]),
                mock.patch.object(launcher, "tracked_file_sha256", side_effect=["1" * 64, "2" * 64, "3" * 64]),
                mock.patch.object(launcher, "sha256_command_stdout", side_effect=["4" * 64, "5" * 64, "6" * 64]),
                mock.patch.object(
                    launcher,
                    "run_command",
                    side_effect=run_command,
                ) as run_build,
            ):
                report = launcher.run_release_build_before_start(
                    root, output_dir, launcher_path
                )

        self.assertEqual(launcher.PASS_STATUS, report["status"])
        self.assertEqual(list(launcher.RELEASE_BUILD_COMMAND), report["command"])
        self.assertTrue(report["worktree_clean_before_build"])
        self.assertTrue(report["worktree_clean_after_build"])
        self.assertTrue(report["binary_absent_after_clean"])
        self.assertEqual(2, run_build.call_count)
        self.assertEqual(
            list(launcher.RELEASE_CLEAN_COMMAND),
            run_build.call_args_list[0].args[0],
        )
        self.assertEqual(
            launcher.canonical_release_build_env(root)["CARGO_ENCODED_RUSTFLAGS"],
            run_build.call_args_list[0].kwargs["env"]["CARGO_ENCODED_RUSTFLAGS"],
        )
        self.assertEqual(
            list(launcher.RELEASE_BUILD_COMMAND),
            run_build.call_args_list[1].args[0],
        )
        self.assertEqual(
            launcher.canonical_release_build_env(root)["CARGO_ENCODED_RUSTFLAGS"],
            run_build.call_args_list[1].kwargs["env"]["CARGO_ENCODED_RUSTFLAGS"],
        )
        self.assertEqual(
            launcher.RELEASE_RUSTFLAGS_CONTRACT,
            report["rustflags_contract"],
        )

    def test_dirty_runtime_worktree_cannot_pass_release_build_freshness(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            launcher_path = root / "target" / "release" / "ghost-launcher"
            launcher_path.parent.mkdir(parents=True)
            launcher_path.write_bytes(b"binary")
            with (
                mock.patch.object(launcher, "git_worktree_is_clean", side_effect=[False, False]),
                mock.patch.object(launcher, "tracked_file_sha256", return_value="1" * 64),
                mock.patch.object(launcher, "sha256_command_stdout", return_value="2" * 64),
                mock.patch.object(
                    launcher,
                    "run_command",
                    side_effect=[
                        {
                            "command": list(launcher.RELEASE_CLEAN_COMMAND),
                            "exit_code": 0,
                            "log_path": str(root / "clean.log"),
                        },
                        {
                            "command": list(launcher.RELEASE_BUILD_COMMAND),
                            "exit_code": 0,
                            "log_path": str(root / "build.log"),
                        },
                    ],
                ),
            ):
                report = launcher.run_release_build_before_start(
                    root, root / "reports", launcher_path
                )

        self.assertEqual(launcher.INCONCLUSIVE_ENV_OR_CONFIG, report["status"])
        self.assertFalse(report["worktree_clean_before_build"])

    def test_release_build_rejects_clean_that_leaves_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            launcher_path = root / "target" / "release" / "ghost-launcher"
            launcher_path.parent.mkdir(parents=True)
            launcher_path.write_bytes(b"stale-binary")
            with (
                mock.patch.object(
                    launcher, "git_worktree_is_clean", side_effect=[True, True]
                ),
                mock.patch.object(
                    launcher, "tracked_file_sha256", return_value="1" * 64
                ),
                mock.patch.object(
                    launcher, "sha256_command_stdout", return_value="2" * 64
                ),
                mock.patch.object(
                    launcher,
                    "run_command",
                    side_effect=[
                        {
                            "command": list(launcher.RELEASE_CLEAN_COMMAND),
                            "exit_code": 0,
                            "log_path": str(root / "clean.log"),
                        },
                        {
                            "command": list(launcher.RELEASE_BUILD_COMMAND),
                            "exit_code": 0,
                            "log_path": str(root / "build.log"),
                        },
                    ],
                ),
            ):
                report = launcher.run_release_build_before_start(
                    root, root / "reports", launcher_path
                )

        self.assertEqual(launcher.INCONCLUSIVE_ENV_OR_CONFIG, report["status"])
        self.assertFalse(report["binary_absent_after_clean"])

    def test_preflight_uses_the_exact_guarded_release_binary(self) -> None:
        command = launcher.build_preflight_command(
            Path("/tmp/target/release/ghost-launcher"),
            Path("/tmp/config.toml"),
        )

        self.assertEqual(
            command,
            [
                "/tmp/target/release/ghost-launcher",
                "--config",
                "/tmp/config.toml",
                "--preflight",
            ],
        )
        self.assertNotIn("cargo", command)

    def test_runtime_timeout_requests_controlled_launcher_shutdown(self) -> None:
        self.assertEqual(
            "timeout --signal=INT --kill-after=120s 5400s ",
            launcher.build_runtime_timeout_prefix(5400),
        )
        self.assertEqual("", launcher.build_runtime_timeout_prefix(None))

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

    def test_tmux_start_sources_env_and_aliases_nln_api_key_without_secret_literal(self) -> None:
        captured = {}

        def fake_run(command, **kwargs):
            captured["command"] = command
            return mock.Mock(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(launcher.subprocess, "run", fake_run):
            root = Path(tmp)
            result = launcher.start_tmux_session(
                root=root,
                session="selector_dataset_r12_simcov_evidence",
                launcher=root / "target/release/ghost-launcher",
                config_path=root / "configs/rollout/r12.toml",
                runtime_log=root / "reports/runtime.log",
                runtime_timeout_seconds=5400,
            )

        self.assertEqual(0, result["exit_code"])
        tmux_payload = captured["command"][-1]
        self.assertIn("if [ -f ./.env ]; then . ./.env; fi", tmux_payload)
        self.assertNotIn("[ -f ./.env ] && . ./.env", tmux_payload)
        self.assertIn('export NLN_API_KEY="$GHOST_SEER_GRPC_X_TOKEN"', tmux_payload)
        self.assertIn(
            "timeout --signal=INT --kill-after=120s 5400s",
            tmux_payload,
        )
        self.assertIn("RUST_BACKTRACE=1", tmux_payload)
        self.assertNotIn("sk_live_", tmux_payload)

    def test_launcher_zero_buy_lifecycle_allowance_has_distinct_pass_claim(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            report = {
                "run_state": launcher.RUN_STATE_EVENT_ONLY,
                "scope": "r37",
                "config": "/tmp/r37.toml",
                "tmux_session": "r37",
                "allow_zero_buy_lifecycle_proof": True,
                "runtime_binary": "/tmp/ghost-launcher",
                "build_release_before_start": False,
                "build_freshness_status": "NOT_REQUESTED",
                "git_head_at_build": None,
                "git_head_at_launch": "abc",
                "binary_mtime_utc": None,
                "storage": {"status": launcher.PASS_STATUS},
                "config_contract": {"status": launcher.PASS_STATUS},
                "scope_contract": {"status": launcher.PASS_STATUS},
                "static_guard": {"status": launcher.PASS_STATUS},
                "preflight": {"status": launcher.PASS_STATUS},
                "event_canary": {"status": launcher.PASS_STATUS},
                "lifecycle_canary": {"status": "SKIPPED_ZERO_BUY_LIFECYCLE_ALLOWED"},
                "errors": [],
                "artifacts": {},
            }

            with redirect_stdout(StringIO()):
                exit_code = launcher.finish(report, output_dir, launcher.PASS_STATUS)

            self.assertEqual(0, exit_code)
            self.assertEqual(
                "SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED",
                report["claim"],
            )
            markdown = (output_dir / "RUN_LIFECYCLE_LAUNCHER_REPORT.md").read_text(encoding="utf-8")
            self.assertIn("PASS means event-ingest proof only", markdown)
            self.assertNotIn("SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF", report["claim"])

    def test_launcher_parser_accepts_zero_buy_lifecycle_allowance(self) -> None:
        parser = launcher.build_parser()
        args = parser.parse_args(
            [
                "--scope",
                "r37",
                "--launch-cohort-id",
                "cohort-r37",
                "--config",
                "configs/rollout/r37.toml",
                "--run-role",
                "validation",
                "--tmux-session",
                "r37",
                "--allow-zero-buy-lifecycle-proof",
            ]
        )

        self.assertTrue(args.allow_zero_buy_lifecycle_proof)


if __name__ == "__main__":
    unittest.main()
