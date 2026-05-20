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
                "decision_ts_ms": 1000,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
                "decision_plane": "legacy_live",
                "rollout_namespace": "r14-j2b-harness",
            }
            write_jsonl(
                root / "logs/rollout/r14/decisions/gatekeeper_v2_decisions.jsonl",
                [
                    {
                        **common,
                        "v3_replay_payload": {"schema_version": 1},
                    }
                ],
            )
            write_jsonl(root / "logs/shadow_run/r14/buys.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_entries.jsonl", [common])
            write_jsonl(root / "logs/shadow_run/r14/shadow_lifecycle.jsonl", [common])

            report = audit.build_report(config)

        self.assertEqual(report["readiness"]["status"], "ready_for_lifecycle_feature_join")
        self.assertEqual(report["readiness"]["join_key_acceptance"], "pass")
        self.assertEqual(report["readiness"]["join_quality"], "exact_ab_record_id")
        self.assertEqual(report["readiness"]["v3_payload_rows"], 1)
        self.assertEqual(report["cross_artifact_intersections"]["candidate_id"]["common_values"], 1)
        self.assertEqual(report["join_key_coverage"]["full_chain_ab_record_id_coverage"], 1.0)
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

    def test_probe_transport_entry_join_can_pass_without_lifecycle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r15-probe.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r15-probe/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r15-probe/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r15-probe/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r15-probe/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r15-probe/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r15-probe/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            decision = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "source-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            probe_common = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-id",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_selected.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_transport.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_entries.jsonl", [probe_common])

            report = audit.build_report(config)

        self.assertEqual(report["probe_readiness"]["status"], "ready_for_probe_transport_entry_join")
        self.assertEqual(report["probe_readiness"]["join_key_acceptance"], "pass")
        self.assertEqual(report["probe_readiness"]["join_quality"], "exact_probe_id_and_ab_record_id")
        self.assertEqual(report["probe_readiness"]["decision_join_acceptance"], "pass")
        self.assertEqual(report["probe_readiness"]["required_exact_decision_v3_join_coverage"], 1.0)
        self.assertEqual(report["probe_join_key_coverage"]["probe_transport_rows"], 1)
        self.assertEqual(report["probe_join_key_coverage"]["probe_entry_rows"], 1)
        self.assertEqual(report["probe_join_key_coverage"]["probe_lifecycle_rows"], 0)
        self.assertEqual(report["probe_join_key_coverage"]["probe_chain_ab_record_id_coverage"], 1.0)
        self.assertEqual(report["probe_join_key_coverage"]["probe_chain_probe_id_coverage"], 1.0)
        self.assertEqual(
            report["probe_decision_join"]["artifacts"]["probe_transport"]["exact_decision_v3_join"],
            1,
        )
        self.assertEqual(
            report["artifacts"]["probe_transport"][0]["value_counts"]["probe_bucket"],
            {"v3_pending_wait_sample": 1},
        )
        self.assertEqual(
            report["artifacts"]["probe_transport"][0]["value_counts"]["probe_amount_source"],
            {"fixed_lamports": 1},
        )
        self.assertEqual(report["readiness"]["status"], "not_ready")

    def test_probe_transport_entry_without_decision_v3_join_is_not_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r15-probe.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r15-probe/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r15-probe/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r15-probe/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r15-probe/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r15-probe/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r15-probe/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [
                    {
                        "candidate_id": "pool_mint_1000",
                        "ab_record_id": "different-ab",
                        "pool_id": "pool",
                        "base_mint": "mint",
                        "v3_replay_payload_schema_version": 1,
                        "v3_feature_snapshot_hash": "feature-hash",
                        "v3_policy_config_hash": "policy-hash",
                    }
                ],
            )
            probe_common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "source-ab",
                "source_ab_record_id": "source-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "probe_id": "probe-id",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_selected.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_transport.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_entries.jsonl", [probe_common])

            report = audit.build_report(config)

        self.assertEqual(report["probe_readiness"]["status"], "not_ready")
        self.assertEqual(report["probe_readiness"]["join_key_acceptance"], "fail")
        self.assertEqual(report["probe_readiness"]["decision_join_acceptance"], "fail")
        self.assertIn(
            "probe_rows_missing_exact_decision_v3_join",
            report["probe_readiness"]["reasons"],
        )
        self.assertEqual(
            report["probe_decision_join"]["required_exact_decision_v3_join_coverage"],
            0.0,
        )
        reasons = report["probe_decision_join"]["artifacts"]["probe_transport"]["mismatch_reasons"]
        self.assertEqual(reasons, {"decision_row_not_found": 1})

    def test_concatenated_probe_jsonl_is_parsed_and_mismatch_is_explained(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r15-probe.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r15-probe/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r15-probe/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r15-probe/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r15-probe/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r15-probe/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r15-probe/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            decision = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "source-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_plane": "legacy_live",
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "decision-feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            probe_common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "source-ab",
                "source_ab_record_id": "source-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "source_decision_plane": "legacy_live",
                "probe_id": "probe-id",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "v3_feature_snapshot_hash": "probe-feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            # Simulate the previous concurrent append corruption class: two JSON
            # objects on one physical line must still be counted as two rows.
            selected_path = root / "logs/shadow_run/r15-probe/probe_selected.jsonl"
            selected_path.parent.mkdir(parents=True, exist_ok=True)
            selected_path.write_text(
                json.dumps(probe_common) + json.dumps({**probe_common, "probe_id": "probe-id-2"}) + "\n",
                encoding="utf-8",
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_transport.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_entries.jsonl", [probe_common])

            report = audit.build_report(config)

        self.assertEqual(report["probe_join_key_coverage"]["probe_selection_rows"], 2)
        self.assertEqual(report["probe_readiness"]["status"], "not_ready")
        transport = report["probe_decision_join"]["artifacts"]["probe_transport"]
        self.assertEqual(transport["feature_hash_mismatch"], 1)
        self.assertEqual(transport["mismatch_reasons"], {"feature_hash_mismatch": 1})


if __name__ == "__main__":
    unittest.main()
