import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import v3_full_replay_report


class V3FullReplayReportTest(unittest.TestCase):
    def test_validator_command_uses_cargo_by_default(self):
        command = v3_full_replay_report.validator_command(Path("/tmp/decisions.jsonl"), False, None)
        self.assertEqual(command[:7], ["cargo", "run", "--quiet", "-p", "ghost-launcher", "--bin", "v3_replay"])
        self.assertIn("--input", command)
        self.assertIn("--json", command)
        self.assertNotIn("--strict", command)

    def test_validator_command_can_use_prebuilt_binary_and_strict(self):
        command = v3_full_replay_report.validator_command(
            Path("/tmp/decisions.jsonl"),
            True,
            Path("/tmp/v3_replay"),
        )
        self.assertEqual(command, ["/tmp/v3_replay", "--input", "/tmp/decisions.jsonl", "--json", "--strict"])

    def test_resolve_decisions_log_rejects_missing_explicit_path(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            missing = Path(temp_dir) / "missing.jsonl"
            with self.assertRaises(FileNotFoundError):
                v3_full_replay_report.resolve_decisions_log(Path("/tmp/config.toml"), missing)


if __name__ == "__main__":
    unittest.main()
