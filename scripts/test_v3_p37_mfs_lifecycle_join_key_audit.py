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

    def test_probe_transport_without_entry_is_classified_as_missing_token_quantity(self) -> None:
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
            decision_base = {
                "candidate_id": "pool_mint_1000",
                "pool_id": "pool",
                "base_mint": "mint",
                "v3_replay_payload_schema_version": 1,
                "v3_policy_config_hash": "policy-hash",
            }
            decisions = [
                {
                    **decision_base,
                    "ab_record_id": "source-ab-1",
                    "v3_feature_snapshot_hash": "feature-hash-1",
                },
                {
                    **decision_base,
                    "ab_record_id": "source-ab-2",
                    "v3_feature_snapshot_hash": "feature-hash-2",
                },
            ]
            probe_base = {
                "candidate_id": "pool_mint_1000",
                "pool_id": "pool",
                "base_mint": "mint",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "v3_policy_config_hash": "policy-hash",
            }
            materialized = {
                **probe_base,
                "ab_record_id": "source-ab-1",
                "source_ab_record_id": "source-ab-1",
                "probe_id": "probe-id-1",
                "v3_feature_snapshot_hash": "feature-hash-1",
                "buy_variant": "legacy_buy",
                "token_param_role": "token_amount",
                "entry_token_amount_raw": 123,
                "min_tokens_out": 100,
                "execution_outcome": "counterfactual_shadow_probe_simulated",
            }
            transport_only = {
                **probe_base,
                "ab_record_id": "source-ab-2",
                "source_ab_record_id": "source-ab-2",
                "probe_id": "probe-id-2",
                "v3_feature_snapshot_hash": "feature-hash-2",
                "buy_variant": "routed_exact_sol_in",
                "token_param_role": "min_tokens_out",
                "entry_token_amount_raw": None,
                "min_tokens_out": 1,
                "execution_outcome": "counterfactual_shadow_probe_simulated",
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                decisions,
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_selected.jsonl",
                [materialized, transport_only],
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_transport.jsonl",
                [materialized, transport_only],
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_entries.jsonl", [materialized])

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(report["probe_readiness"]["status"], "ready_for_probe_transport_entry_join")
        self.assertEqual(materialization["status_counts"]["entry_materialized"], 1)
        self.assertEqual(materialization["status_counts"]["transport_only_missing_token_quantity"], 1)
        self.assertEqual(
            materialization["reason_counts"]["routed_exact_sol_in_entry_token_amount_raw_null"],
            1,
        )

    def test_probe_transport_simulation_error_without_entry_is_classified(self) -> None:
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
            probe_error = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-id",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "error_class": "simulation_mismatch",
                "simulation_error_category": "simulation_slippage_or_price_mismatch",
                "simulation_error_custom_code": 6002,
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_selected.jsonl", [probe_error])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_transport.jsonl", [probe_error])

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(report["probe_readiness"]["status"], "not_ready")
        self.assertEqual(materialization["status_counts"]["simulation_error"], 1)
        self.assertEqual(
            materialization["reason_counts"]["simulation_slippage_or_price_mismatch:custom_6002"],
            1,
        )
        self.assertEqual(
            materialization["simulation_error_custom_code_counts"]["custom_6002"],
            1,
        )

    def test_probe_transport_creator_vault_and_amount_guard_counts_are_reported(self) -> None:
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
            creator_vault_error = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-creator",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "error_class": "simulation_mismatch",
                "simulation_error_category": "simulation_account_layout_mismatch",
                "simulation_error_custom_code": 2006,
                "simulation_error_account_role": "creator_vault",
                "simulation_error_actual_account_pubkey": "actual-vault",
                "simulation_error_expected_account_pubkey": "expected-vault",
                "creator_vault_authority_status": "creator_vault_source_not_authoritative",
                "creator_vault_actual_pubkey": "actual-vault",
                "creator_vault_expected_pubkey": "expected-vault",
                "creator_vault_mismatch_reason": "actual_expected_mismatch",
                "creator_identity_source": "account_overrides.creator_pubkey",
                "creator_identity_authoritative": False,
            }
            amount_error = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-amount",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "error_class": "simulation_mismatch",
                "simulation_error_category": "simulation_slippage_or_price_mismatch",
                "simulation_error_custom_code": 6002,
                "amount_guard_status": "amount_required_exceeds_probe_amount",
                "amount_provided_lamports_if_available": 7_000_000,
                "amount_required_lamports_if_available": 7_739_140,
                "amount_shortfall_lamports_if_available": 739_140,
            }
            creator_vault_skip = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-skip-creator",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "event_type": "probe_skipped",
                "probe_skip_reason": "creator_vault_source_not_authoritative",
                "precheck_failure_reason": (
                    "creator_vault_source_not_authoritative:"
                    "pumpfun_legacy_extended_buy_accounts:"
                    "detected_pool.creator:creator"
                ),
                "creator_vault_authority_status": "creator_vault_source_not_authoritative",
                "creator_vault_mismatch_reason": "creator_identity_source_not_authoritative",
                "creator_identity_source": "detected_pool.creator",
                "creator_identity_authoritative": False,
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_selected.jsonl", [decision])
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_skipped.jsonl",
                [creator_vault_skip],
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_transport.jsonl",
                [creator_vault_error, amount_error],
            )

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(
            materialization["creator_vault_authority_status_counts"][
                "creator_vault_source_not_authoritative"
            ],
            1,
        )
        self.assertEqual(
            materialization["creator_vault_mismatch_reason_counts"][
                "actual_expected_mismatch"
            ],
            1,
        )
        self.assertEqual(
            materialization["creator_identity_source_counts"][
                "account_overrides.creator_pubkey"
            ],
            1,
        )
        self.assertEqual(
            materialization["amount_guard_status_counts"][
                "amount_required_exceeds_probe_amount"
            ],
            1,
        )
        self.assertEqual(
            materialization["simulation_error_custom_code_counts"]["custom_2006"],
            1,
        )
        self.assertEqual(
            materialization["simulation_error_custom_code_counts"]["custom_6002"],
            1,
        )
        self.assertEqual(
            materialization["skip_reason_counts"]["creator_vault_source_not_authoritative"],
            1,
        )
        self.assertEqual(
            materialization["skip_creator_vault_authority_status_counts"][
                "creator_vault_source_not_authoritative"
            ],
            1,
        )
        self.assertEqual(
            materialization["skip_creator_vault_mismatch_reason_counts"][
                "creator_identity_source_not_authoritative"
            ],
            1,
        )
        self.assertEqual(
            materialization["skip_creator_identity_source_counts"]["detected_pool.creator"],
            1,
        )

    def test_probe_transport_simulation_error_with_entry_is_not_clean_materialized(self) -> None:
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
            probe_error = {
                **decision,
                "source_ab_record_id": "source-ab",
                "probe_id": "probe-id",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "error_class": "simulation_mismatch",
                "simulation_error_kind": "simulation_error",
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_selected.jsonl", [probe_error])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_transport.jsonl", [probe_error])
            write_jsonl(root / "logs/shadow_run/r15-probe/probe_entries.jsonl", [probe_error])

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(report["probe_readiness"]["status"], "ready_for_probe_transport_entry_join")
        self.assertEqual(materialization["status_counts"]["simulation_error"], 1)
        self.assertNotIn("entry_materialized", materialization["status_counts"])
        self.assertEqual(materialization["reason_counts"]["simulation_mismatch"], 1)

    def test_probe_account_not_found_attribution_and_entry_eligibility_are_reported(self) -> None:
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
            decision_base = {
                "candidate_id": "pool_mint_1000",
                "pool_id": "pool",
                "base_mint": "mint",
                "v3_replay_payload_schema_version": 1,
                "v3_policy_config_hash": "policy-hash",
            }
            decisions = [
                {**decision_base, "ab_record_id": f"source-ab-{idx}", "v3_feature_snapshot_hash": f"feature-hash-{idx}"}
                for idx in range(1, 5)
            ]
            probe_base = {
                "candidate_id": "pool_mint_1000",
                "pool_id": "pool",
                "base_mint": "mint",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "v3_policy_config_hash": "policy-hash",
            }
            attributed = {
                **probe_base,
                "ab_record_id": "source-ab-1",
                "source_ab_record_id": "source-ab-1",
                "probe_id": "probe-attributed",
                "v3_feature_snapshot_hash": "feature-hash-1",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_attributed",
                "simulation_error_account_pubkey": "missing-pubkey",
                "simulation_error_account_role": "creator_vault",
                "simulation_error_account_source": "route_builder",
                "account_set_match": True,
                "precheck_account_set_hash": "precheck-hash",
                "simulation_account_set_hash": "simulation-hash",
            }
            multi_candidate = {
                **probe_base,
                "ab_record_id": "source-ab-2",
                "source_ab_record_id": "source-ab-2",
                "probe_id": "probe-multi",
                "v3_feature_snapshot_hash": "feature-hash-2",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_multi_candidate",
                "simulation_error_account_candidates": [
                    {"pubkey": "a", "role": "bonding_curve", "source": "route_builder"},
                    {"pubkey": "b", "role": "creator_vault", "source": "route_builder"},
                ],
                "account_set_match": False,
                "account_set_mismatch_reason": "simulation_required_accounts_missing_from_precheck",
            }
            unattributed = {
                **probe_base,
                "ab_record_id": "source-ab-3",
                "source_ab_record_id": "source-ab-3",
                "probe_id": "probe-unattributed",
                "v3_feature_snapshot_hash": "feature-hash-3",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_unattributed",
                "account_set_match": False,
            }
            successful = {
                **probe_base,
                "ab_record_id": "source-ab-4",
                "source_ab_record_id": "source-ab-4",
                "probe_id": "probe-success",
                "v3_feature_snapshot_hash": "feature-hash-4",
                "execution_outcome": "counterfactual_shadow_probe_simulated",
                "probe_entry_materialization_status": "entry_materialized",
                "probe_lifecycle_eligibility_status": "lifecycle_eligible",
                "account_set_match": True,
            }
            write_jsonl(
                root / "logs/rollout/r15-probe/decisions/gatekeeper_v2_decisions.jsonl",
                decisions,
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_selected.jsonl",
                [attributed, multi_candidate, unattributed, successful],
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_transport.jsonl",
                [attributed, multi_candidate, unattributed, successful],
            )
            write_jsonl(
                root / "logs/shadow_run/r15-probe/probe_entries.jsonl",
                [
                    {**attributed, "probe_entry_materialization_status": "simulation_error", "probe_lifecycle_eligibility_status": "not_lifecycle_eligible"},
                    {**successful, "probe_entry_materialization_status": "entry_materialized", "probe_lifecycle_eligibility_status": "lifecycle_eligible"},
                ],
            )

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(materialization["account_not_found_rows"], 3)
        self.assertEqual(materialization["account_not_found_attributed_rows"], 1)
        self.assertEqual(materialization["account_not_found_multi_candidate_rows"], 1)
        self.assertEqual(materialization["account_not_found_unattributed_rows"], 1)
        self.assertEqual(materialization["precheck_simulation_account_set_mismatch_rows"], 2)
        self.assertEqual(materialization["unexplained_account_set_mismatch_rows"], 1)
        self.assertEqual(materialization["simulation_error_entry_rows"], 1)
        self.assertEqual(materialization["successful_probe_entry_rows"], 1)
        self.assertEqual(materialization["lifecycle_eligible_entry_rows"], 1)
        self.assertEqual(
            materialization["simulation_error_account_role_counts"]["creator_vault"],
            1,
        )
        self.assertEqual(
            materialization["simulation_error_account_source_counts"]["route_builder"],
            1,
        )
        self.assertEqual(report["probe_readiness"]["status"], "not_ready")
        self.assertIn(
            "unattributed_account_not_found_blocks_collection",
            report["probe_readiness"]["reasons"],
        )
        self.assertIn(
            "unexplained_precheck_simulation_account_set_mismatch",
            report["probe_readiness"]["reasons"],
        )

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
