#!/usr/bin/env python3
"""Regression tests for exact-child Pump Research capture supervision."""

from __future__ import annotations

import importlib.util
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("pump_research_capture_supervisor.py")
SPEC = importlib.util.spec_from_file_location("pump_research_capture_supervisor", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
supervisor = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = supervisor
SPEC.loader.exec_module(supervisor)


class PumpResearchCaptureSupervisorTests(unittest.TestCase):
    def run_child(
        self,
        code: str,
        *,
        duration_seconds: float = 0.08,
        drain_timeout_seconds: float = 1.0,
        disk_floor_bytes: int = 0,
        free_bytes: int = 1_000_000,
        disk_poll_seconds: float = 0.01,
    ) -> supervisor.SupervisedProcessOutcome:
        root = Path(tempfile.mkdtemp(prefix="pump-research-supervisor-test-"))
        return supervisor.supervise_process(
            [sys.executable, "-c", code],
            log_path=root / "child.log",
            child_environment=dict(),
            duration_seconds=duration_seconds,
            drain_timeout_seconds=drain_timeout_seconds,
            disk_path=root,
            disk_floor_bytes=disk_floor_bytes,
            disk_poll_seconds=disk_poll_seconds,
            free_bytes_reader=lambda _path: free_bytes,
        )

    def test_duration_sigint_preserves_clean_exact_child_exit_status(self) -> None:
        outcome = self.run_child(
            "import signal,time; "
            "signal.signal(signal.SIGINT, lambda *_: raise_system_exit()); "
            "raise_system_exit=lambda: exit(0); "
            "time.sleep(30)"
        )
        self.assertEqual(outcome.shutdown_reason, "duration_elapsed")
        self.assertEqual(outcome.shutdown_signal, signal.SIGINT)
        self.assertFalse(outcome.forced_kill)
        self.assertTrue(os.WIFEXITED(outcome.raw_wait_status))
        self.assertEqual(os.WEXITSTATUS(outcome.raw_wait_status), 0)
        self.assertEqual(outcome.returncode, 0)
        self.assertEqual(outcome.process_exit_status, 0)
        self.assertIsNone(outcome.process_termination_signal)
        self.assertEqual(outcome.wait_call_count, 1)

    def test_early_nonzero_exit_is_preserved_without_imputed_shutdown(self) -> None:
        outcome = self.run_child("raise SystemExit(7)", duration_seconds=5.0)
        self.assertIsNone(outcome.shutdown_reason)
        self.assertIsNone(outcome.shutdown_signal)
        self.assertTrue(os.WIFEXITED(outcome.raw_wait_status))
        self.assertEqual(os.WEXITSTATUS(outcome.raw_wait_status), 7)
        self.assertEqual(outcome.returncode, 7)
        self.assertEqual(outcome.process_exit_status, 7)
        self.assertEqual(outcome.wait_call_count, 1)

    def test_drain_timeout_sigkills_the_exact_child_and_preserves_signal(self) -> None:
        outcome = self.run_child(
            "import signal,time; signal.signal(signal.SIGINT, signal.SIG_IGN); time.sleep(30)",
            drain_timeout_seconds=0.05,
        )
        self.assertTrue(outcome.forced_kill)
        self.assertTrue(os.WIFSIGNALED(outcome.raw_wait_status))
        self.assertEqual(os.WTERMSIG(outcome.raw_wait_status), signal.SIGKILL)
        self.assertEqual(outcome.returncode, -signal.SIGKILL)
        self.assertIsNone(outcome.process_exit_status)
        self.assertEqual(outcome.process_termination_signal, signal.SIGKILL)
        self.assertEqual(outcome.wait_call_count, 1)

    def test_disk_floor_sends_sigint_without_process_name_lookup(self) -> None:
        outcome = self.run_child(
            "import signal,time; "
            "signal.signal(signal.SIGINT, lambda *_: raise_system_exit()); "
            "raise_system_exit=lambda: exit(0); "
            "time.sleep(30)",
            duration_seconds=5.0,
            disk_floor_bytes=2,
            free_bytes=1,
            disk_poll_seconds=0.05,
        )
        self.assertEqual(outcome.shutdown_reason, "disk_floor")
        self.assertEqual(outcome.returncode, 0)
        self.assertEqual(outcome.wait_call_count, 1)

    def test_config_credentials_are_deduplicated_and_never_fall_back_to_legacy(self) -> None:
        self.assertEqual(
            supervisor.config_credentials(
                {
                    "grpc_auth_token_env": "GHOST_PUMP_RESEARCH_TOKEN",
                    "rpc_auth_token_env": "GHOST_PUMP_RESEARCH_TOKEN",
                }
            ),
            ("GHOST_PUMP_RESEARCH_TOKEN",),
        )
        self.assertEqual(supervisor.config_credentials({}), ())
        with self.assertRaisesRegex(RuntimeError, "valid environment name"):
            supervisor.config_credentials({"grpc_auth_token_env": "BAD=NAME"})
        with self.assertRaisesRegex(RuntimeError, "legacy credential"):
            supervisor.capture_child_environment(
                {"GHOST_SEER_GRPC_X_TOKEN": "synthetic-value"},
                ("GHOST_SEER_GRPC_X_TOKEN",),
            )

    def test_after_spawn_runs_before_the_supervisor_waits_for_shutdown(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="pump-research-supervisor-spawn-test-"))
        observed: list[int] = []
        outcome = supervisor.supervise_process(
            [sys.executable, "-c", "raise SystemExit(0)"],
            log_path=root / "child.log",
            child_environment=dict(),
            duration_seconds=5.0,
            drain_timeout_seconds=1.0,
            disk_path=root,
            disk_floor_bytes=0,
            disk_poll_seconds=0.01,
            free_bytes_reader=lambda _path: 1_000_000,
            after_spawn=observed.append,
        )
        self.assertEqual(observed, [outcome.child_pid])

    def test_public_cli_persists_raw_wait_status_and_no_post_capture_phase(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="pump-research-supervisor-cli-test-"))
        output = root / "raw-output"
        output.mkdir()
        environment_evidence = root / "child-environment-evidence.json"
        fake_binary = root / "fake-pump-research-tape"
        fake_binary.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, signal, time\n"
            "from pathlib import Path\n"
            "signal.signal(signal.SIGINT, lambda *_: raise_system_exit())\n"
            "raise_system_exit = lambda: exit(0)\n"
            f"run = Path({str(output / 'pump-research-test-run')!r})\n"
            "raw = run / 'raw'\n"
            "raw.mkdir(parents=True)\n"
            "(raw / 'run_completion_receipt.json').write_text(\n"
            "    json.dumps({'run_id': run.name, 'status': 'Complete', "
            "'clean_shutdown': True}) + '\\n', encoding='utf-8')\n"
            f"Path({str(environment_evidence)!r}).write_text(json.dumps({{\n"
            "    'dedicated_present': bool(os.environ.get('TEST_PUMP_CAPTURE_TOKEN')),\n"
            "    'legacy_grpc_present': bool(os.environ.get('GHOST_SEER_GRPC_X_TOKEN')),\n"
            "    'legacy_rpc_present': bool(os.environ.get('GHOST_RPC_AUTH_TOKEN')),\n"
            "}) + '\\n', encoding='utf-8')\n"
            "time.sleep(30)\n",
            encoding="utf-8",
        )
        fake_binary.chmod(0o700)
        config = root / "capture.toml"
        config.write_text(
            f'output_dir = "{output}"\n'
            'grpc_auth_token_env = "TEST_PUMP_CAPTURE_TOKEN"\n',
            encoding="utf-8",
        )
        receipt = root / "preflight.json"
        receipt.write_text("{}\n", encoding="utf-8")
        operator_dir = root / "operator" / "run-1"
        process_environment = dict(os.environ)
        process_environment.update(
            {
                "TEST_PUMP_CAPTURE_TOKEN": "synthetic-dedicated-value",
                "GHOST_SEER_GRPC_X_TOKEN": "synthetic-legacy-grpc-value",
                "GHOST_RPC_AUTH_TOKEN": "synthetic-legacy-rpc-value",
            }
        )

        completed = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--binary",
                str(fake_binary),
                "--config",
                str(config),
                "--provenance-receipt",
                str(receipt),
                "--operator-dir",
                str(operator_dir),
                "--duration-seconds",
                "0.15",
                "--drain-timeout-seconds",
                "1",
                "--start-free-min-bytes",
                "0",
                "--disk-floor-bytes",
                "0",
                "--disk-poll-seconds",
                "0.05",
            ],
            check=False,
            capture_output=True,
            env=process_environment,
            text=True,
            timeout=5,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        execution = json.loads(
            (operator_dir / "operator_execution_receipt_v1.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(execution["raw_wait_status"], 0)
        self.assertEqual(execution["process_exit_status"], 0)
        self.assertIsNone(execution["process_termination_signal"])
        self.assertEqual(execution["wait_call_count"], 1)
        self.assertFalse(execution["post_capture_pipeline_started"])
        self.assertTrue(execution["operator_capture_success"])
        self.assertIsNone(execution["operator_failure_code"])
        self.assertEqual(execution["new_run_ids"], ["pump-research-test-run"])
        self.assertEqual(execution["validated_run_id"], "pump-research-test-run")
        self.assertEqual(execution["completion_status"], "Complete")
        self.assertTrue(execution["completion_clean_shutdown"])
        self.assertEqual(execution["partial_path_count"], 0)
        self.assertTrue(execution["completion_receipt_sha256"])
        self.assertTrue(execution["credentials_unset_in_supervisor"])
        child_evidence = json.loads(environment_evidence.read_text(encoding="utf-8"))
        self.assertTrue(child_evidence["dedicated_present"])
        self.assertFalse(child_evidence["legacy_grpc_present"])
        self.assertFalse(child_evidence["legacy_rpc_present"])

    def test_public_cli_rejects_zero_run_even_when_child_exits_zero(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="pump-research-supervisor-zero-run-test-"))
        output = root / "raw-output"
        output.mkdir()
        fake_binary = root / "fake-pump-research-tape"
        fake_binary.write_text(
            "#!/usr/bin/env python3\n"
            "import signal, time\n"
            "signal.signal(signal.SIGINT, lambda *_: raise_system_exit())\n"
            "raise_system_exit = lambda: exit(0)\n"
            "time.sleep(30)\n",
            encoding="utf-8",
        )
        fake_binary.chmod(0o700)
        config = root / "capture.toml"
        config.write_text(f'output_dir = "{output}"\n', encoding="utf-8")
        receipt = root / "preflight.json"
        receipt.write_text("{}\n", encoding="utf-8")
        operator_dir = root / "operator" / "run-1"

        completed = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--binary",
                str(fake_binary),
                "--config",
                str(config),
                "--provenance-receipt",
                str(receipt),
                "--operator-dir",
                str(operator_dir),
                "--duration-seconds",
                "0.15",
                "--drain-timeout-seconds",
                "1",
                "--start-free-min-bytes",
                "0",
                "--disk-floor-bytes",
                "0",
                "--disk-poll-seconds",
                "0.05",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(completed.returncode, 1, completed.stderr)
        execution = json.loads(
            (operator_dir / "operator_execution_receipt_v1.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(execution["raw_wait_status"], 0)
        self.assertEqual(execution["process_exit_status"], 0)
        self.assertFalse(execution["operator_capture_success"])
        self.assertEqual(execution["operator_failure_code"], "NEW_RUN_COUNT_NOT_ONE")
        self.assertEqual(execution["new_run_ids"], [])

    def test_output_lock_excludes_different_operator_roots_before_second_popen(
        self,
    ) -> None:
        root = Path(tempfile.mkdtemp(prefix="pump-research-supervisor-lock-test-"))
        output = root / "raw-output"
        output.mkdir()
        spawn_marker = root / "capture-child-spawns.txt"
        fake_binary = root / "fake-pump-research-tape"
        fake_binary.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, signal, time\n"
            "from pathlib import Path\n"
            "def stop(*_args):\n"
            "    raise SystemExit(0)\n"
            "signal.signal(signal.SIGINT, stop)\n"
            f"output = Path({str(output)!r})\n"
            "run = output / f'pump-research-lock-{os.getpid()}'\n"
            "raw = run / 'raw'\n"
            "raw.mkdir(parents=True)\n"
            "(raw / 'run_completion_receipt.json').write_text(\n"
            "    json.dumps({'run_id': run.name, 'status': 'Complete', "
            "'clean_shutdown': True}) + '\\n', encoding='utf-8')\n"
            f"with Path({str(spawn_marker)!r}).open('a', encoding='utf-8') as marker:\n"
            "    marker.write(f'{os.getpid()}\\n')\n"
            "    marker.flush()\n"
            "time.sleep(30)\n",
            encoding="utf-8",
        )
        fake_binary.chmod(0o700)
        config = root / "capture.toml"
        config.write_text(f'output_dir = "{output}"\n', encoding="utf-8")
        receipt = root / "preflight.json"
        receipt.write_text("{}\n", encoding="utf-8")
        first_operator_dir = root / "operator-a" / "run-1"
        second_operator_dir = root / "operator-b-alternative" / "run-2"

        def supervisor_command(operator_dir: Path) -> list[str]:
            return [
                sys.executable,
                str(SCRIPT),
                "--binary",
                str(fake_binary),
                "--config",
                str(config),
                "--provenance-receipt",
                str(receipt),
                "--operator-dir",
                str(operator_dir),
                "--duration-seconds",
                "0.8",
                "--drain-timeout-seconds",
                "1",
                "--start-free-min-bytes",
                "0",
                "--disk-floor-bytes",
                "0",
                "--disk-poll-seconds",
                "0.05",
            ]

        first = subprocess.Popen(
            supervisor_command(first_operator_dir),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        marker_deadline = time.monotonic() + 3.0
        while not spawn_marker.exists() and time.monotonic() < marker_deadline:
            time.sleep(0.01)
        self.assertTrue(spawn_marker.exists(), "first capture child did not reach Popen")

        second = subprocess.run(
            supervisor_command(second_operator_dir),
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        self.assertEqual(second.returncode, 1)
        self.assertIn("capture lock is already held", second.stderr)
        self.assertFalse(second_operator_dir.exists())

        first_stdout, first_stderr = first.communicate(timeout=5)
        self.assertEqual(first.returncode, 0, first_stderr or first_stdout)
        self.assertEqual(len(spawn_marker.read_text(encoding="utf-8").splitlines()), 1)
        execution = json.loads(
            (first_operator_dir / "operator_execution_receipt_v1.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertTrue(execution["operator_capture_success"])
        self.assertEqual(
            execution["capture_lock_scope"], "canonical_output_directory_v1"
        )
        lock_path = output / ".pump-research-capture.lock"
        self.assertEqual(Path(execution["capture_lock_path"]), lock_path)
        self.assertTrue(lock_path.is_file())
        self.assertEqual(lock_path.stat().st_mode & 0o777, 0o600)
        self.assertFalse((first_operator_dir.parent / ".capture.lock").exists())
        self.assertFalse((second_operator_dir.parent / ".capture.lock").exists())

    def test_postcondition_rejects_multiple_runs_bad_receipt_and_partial_path(self) -> None:
        root = Path(tempfile.mkdtemp(prefix="pump-research-postcondition-test-"))
        output = root / "raw-output"
        output.mkdir()
        for run_id in ("pump-research-a", "pump-research-b"):
            (output / run_id).mkdir()
        multiple = supervisor.capture_postcondition(output, set(), 0)
        self.assertFalse(multiple.operator_capture_success)
        self.assertEqual(multiple.operator_failure_code, "NEW_RUN_COUNT_NOT_ONE")

        for run_id in multiple.new_run_ids:
            (output / run_id).rmdir()
        run = output / "pump-research-one"
        raw = run / "raw"
        raw.mkdir(parents=True)
        completion = raw / "run_completion_receipt.json"
        missing = supervisor.capture_postcondition(output, set(), 0)
        self.assertEqual(
            missing.operator_failure_code, "COMPLETION_RECEIPT_MISSING"
        )

        completion.write_text("not-json\n", encoding="utf-8")
        invalid = supervisor.capture_postcondition(output, set(), 0)
        self.assertEqual(
            invalid.operator_failure_code, "COMPLETION_RECEIPT_INVALID"
        )

        completion.write_text(
            json.dumps(
                {
                    "run_id": "pump-research-wrong",
                    "status": "Complete",
                    "clean_shutdown": True,
                }
            )
            + "\n",
            encoding="utf-8",
        )
        mismatched = supervisor.capture_postcondition(output, set(), 0)
        self.assertEqual(
            mismatched.operator_failure_code, "COMPLETION_RUN_ID_MISMATCH"
        )

        completion.write_text(
            json.dumps(
                {
                    "run_id": run.name,
                    "status": "Incomplete",
                    "clean_shutdown": True,
                }
            )
            + "\n",
            encoding="utf-8",
        )
        incomplete = supervisor.capture_postcondition(output, set(), 0)
        self.assertEqual(
            incomplete.operator_failure_code, "COMPLETION_STATUS_NOT_COMPLETE"
        )

        completion.write_text(
            json.dumps(
                {"run_id": run.name, "status": "Complete", "clean_shutdown": False}
            )
            + "\n",
            encoding="utf-8",
        )
        unclean = supervisor.capture_postcondition(output, set(), 0)
        self.assertEqual(unclean.operator_failure_code, "CLEAN_SHUTDOWN_FALSE")

        completion.write_text(
            json.dumps(
                {"run_id": run.name, "status": "Complete", "clean_shutdown": True}
            )
            + "\n",
            encoding="utf-8",
        )
        (raw / "segment_00000.bin.partial").write_bytes(b"incomplete")
        partial = supervisor.capture_postcondition(output, set(), 0)
        self.assertFalse(partial.operator_capture_success)
        self.assertEqual(partial.operator_failure_code, "PARTIAL_PATH_PRESENT")
        self.assertEqual(partial.partial_path_count, 1)


if __name__ == "__main__":
    unittest.main()
