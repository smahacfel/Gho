import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_mfs_lifecycle_join_key_audit as audit


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")


class P37MfsLifecycleJoinKeyAuditTests(unittest.TestCase):
    def test_ready_when_ab_record_id_and_v3_payload_join_across_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r14.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r14/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r14/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r14/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r14/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab1",
                "pool_id": "pool",
                "base_mint": "mint",
            }
            write_jsonl(
                root / "logs/rollout/r14/decisions/gatekeeper_v2_decisions.jsonl",
                [
                    {
                        **common,
                        "v3_replay_payload": {"schema_version": 1},
                        "v3_feature_snapshot_hash": "hash",
                        "decision_plane": "v25_shadow",
                    }
                ],
            )
            write_jsonl(root / "logs/shadow_run/r14/buys.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_entries.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_lifecycle.jsonl", [common])

            report = audit.build_report(config)

        self.assertEqual(report["readiness"]["status"], "ready_for_lifecycle_feature_join")
        self.assertEqual(report["readiness"]["join_quality"], "exact_ab_record_id")
        self.assertEqual(report["readiness"]["v3_payload_rows"], 1)
        self.assertEqual(report["cross_artifact_intersections"]["candidate_id"]["common_values"], 1)
        self.assertEqual(report["join_key_coverage"]["shadow_entry_rows_with_ab_record_id"], 1)

    def test_candidate_id_only_join_is_degraded(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r14.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r14/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r14/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r14/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r14/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "pool_id": "pool",
                "base_mint": "mint",
            }
            write_jsonl(
                root / "logs/rollout/r14/decisions/gatekeeper_v2_decisions.jsonl",
                [
                    {
                        **common,
                        "v3_replay_payload_schema_version": 1,
                        "v3_feature_snapshot_hash": "hash",
                    }
                ],
            )
            write_jsonl(root / "logs/shadow_run/r14/buys.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_entries.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_lifecycle.jsonl", [common])

            report = audit.build_report(config)

        self.assertEqual(report["readiness"]["status"], "degraded")
        self.assertEqual(report["readiness"]["join_quality"], "exact_candidate_id")
        self.assertIn("no_common_ab_record_id_across_nonempty_artifacts", report["readiness"]["reasons"])

    def test_missing_lifecycle_is_not_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r14.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r14/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r14/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r14/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r14/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            row = {"candidate_id": "pool_mint_1000", "pool_id": "pool", "base_mint": "mint"}
            write_jsonl(
                root / "logs/rollout/r14/decisions/gatekeeper_v2_decisions.jsonl",
                [{**row, "v3_replay_payload_schema_version": 1}],
            )
            write_jsonl(root / "logs/shadow_run/r14/buys.jsonl", [row])
            write_jsonl(root / "logs/shadow_run/r14/shadow_entries.jsonl", [row])

            report = audit.build_report(config)

        self.assertEqual(report["readiness"]["status"], "not_ready")
        self.assertIn("missing_shadow_lifecycle_rows", report["readiness"]["reasons"])


if __name__ == "__main__":
    unittest.main()
