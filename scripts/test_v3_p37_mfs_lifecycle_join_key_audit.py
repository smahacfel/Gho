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

    def test_bcv2_exact_watch_coverage_counts_log_markers_and_account_state_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/x8a.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/x8a/decisions"

[logging]
file_path = "../../logs/rollout/x8a/system.log"

[p37_shadow_probe]
entry_log_path = "../../logs/shadow_run/x8a/probe_entries.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            (root / "logs/rollout/x8a").mkdir(parents=True)
            (root / "logs/rollout/x8a/system.log.2026-05-26").write_text(
                "\n".join(
                    [
                        "BCV2_EXACT_WATCH_REGISTERED pubkey=bcv2",
                        "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED profile=primary_global",
                        "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED profile=primary_global",
                        "BCV2_EXACT_WATCH_RESUBSCRIBE_SENT reason=bcv2_registry_notify",
                        "BCV2_RPC_HYDRATION_READY pubkey=bcv2 context_slot=10 owner=owner data_len=256",
                        "BCV2_RPC_HYDRATION_MISSING pubkey=bcv2 error_class=missing_on_rpc",
                        "BCV2_ACCOUNT_UPDATE_RECEIVED pubkey=bcv2 owner=owner data_len=256",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            write_jsonl(
                root / "logs/shadow_run/x8a/probe_entries.jsonl",
                [
                    {
                        "working_builder_parity_mode": "working_builder_parity",
                        "working_builder_bcv2_account_state_seen": True,
                        "working_builder_bcv2_account_state_owner": "owner",
                        "working_builder_bcv2_account_state_data_len": 256,
                    }
                ],
            )

            report = audit.build_report(config)

        coverage = report["bcv2_exact_watch_coverage"]
        self.assertEqual(coverage["bcv2_exact_watch_registered_rows"], 1)
        self.assertEqual(coverage["bcv2_exact_watch_in_subscribe_request_rows"], 1)
        self.assertEqual(coverage["bcv2_exact_watch_subscribe_dropped_rows"], 1)
        self.assertEqual(coverage["bcv2_resubscribe_sent_rows"], 1)
        self.assertEqual(coverage["bcv2_rpc_hydration_ready_rows"], 1)
        self.assertEqual(coverage["bcv2_rpc_hydration_missing_rows"], 1)
        self.assertEqual(coverage["bcv2_account_update_received_rows"], 1)
        self.assertEqual(coverage["bcv2_account_state_seen_rows"], 1)
        self.assertEqual(coverage["bcv2_account_state_owner_rows"], 1)
        self.assertEqual(coverage["bcv2_account_state_data_len_rows"], 1)

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

    def test_probe_account_not_found_narrowing_counts_are_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-r5.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-r5/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r16-r5/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r16-r5/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r16-r5/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r16-r5/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-r5/probe_lifecycle.jsonl"
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
                    "ab_record_id": f"source-ab-{idx}",
                    "v3_feature_snapshot_hash": f"feature-hash-{idx}",
                }
                for idx in range(1, 4)
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
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "simulation_error_kind": "AccountNotFound",
                "account_set_match": True,
            }
            exact = {
                **probe_base,
                "ab_record_id": "source-ab-1",
                "source_ab_record_id": "source-ab-1",
                "probe_id": "probe-exact",
                "v3_feature_snapshot_hash": "feature-hash-1",
                "simulation_error_category": "simulation_account_not_found_attributed",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_pubkey": "bc-v2",
                "simulation_error_account_narrowing_status": "exact_after_narrowing",
                "simulation_error_account_candidates_raw": [
                    {
                        "pubkey": "payer",
                        "role": "payer_pubkey",
                        "source": "payer",
                        "candidate_class": "ephemeral_payer_nonfatal",
                        "candidate_fatality": "non_fatal",
                        "candidate_exclusion_reason": "ephemeral_payer_not_rpc_required",
                    },
                    {
                        "pubkey": "ata",
                        "role": "user_ata",
                        "source": "user_ata",
                        "candidate_class": "idempotent_creatable_user_ata",
                        "candidate_fatality": "non_fatal",
                        "candidate_exclusion_reason": "idempotent_ata_create_attached",
                    },
                    {
                        "pubkey": "bc-v2",
                        "role": "bonding_curve_v2",
                        "source": "route_builder",
                        "candidate_class": "strict_execution_account",
                        "candidate_fatality": "fatal",
                    },
                ],
                "simulation_error_account_candidates_narrowed": [
                    {
                        "pubkey": "bc-v2",
                        "role": "bonding_curve_v2",
                        "source": "route_builder",
                        "candidate_class": "strict_execution_account",
                        "candidate_fatality": "fatal",
                    }
                ],
                "simulation_error_account_candidates_excluded": [
                    {
                        "pubkey": "payer",
                        "role": "payer_pubkey",
                        "source": "payer",
                        "candidate_class": "ephemeral_payer_nonfatal",
                        "candidate_fatality": "non_fatal",
                        "candidate_exclusion_reason": "ephemeral_payer_not_rpc_required",
                    },
                    {
                        "pubkey": "ata",
                        "role": "user_ata",
                        "source": "user_ata",
                        "candidate_class": "idempotent_creatable_user_ata",
                        "candidate_fatality": "non_fatal",
                        "candidate_exclusion_reason": "idempotent_ata_create_attached",
                    },
                ],
            }
            multi = {
                **probe_base,
                "ab_record_id": "source-ab-2",
                "source_ab_record_id": "source-ab-2",
                "probe_id": "probe-multi",
                "v3_feature_snapshot_hash": "feature-hash-2",
                "simulation_error_category": "simulation_account_not_found_multi_candidate_narrow",
                "simulation_error_account_narrowing_status": "multi_candidate_narrowed",
                "simulation_error_account_candidates_narrowed": [
                    {"pubkey": "bc-v2", "role": "bonding_curve_v2", "candidate_class": "strict_execution_account"},
                    {"pubkey": "uva", "role": "user_volume_accumulator", "candidate_class": "route_specific_required_account"},
                ],
            }
            all_nonfatal = {
                **probe_base,
                "ab_record_id": "source-ab-3",
                "source_ab_record_id": "source-ab-3",
                "probe_id": "probe-nonfatal",
                "v3_feature_snapshot_hash": "feature-hash-3",
                "simulation_error_category": "all_candidates_nonfatal_but_sim_failed",
                "simulation_error_account_narrowing_status": "all_candidates_nonfatal_but_sim_failed",
                "simulation_error_account_candidates_excluded": [
                    {
                        "pubkey": "uva",
                        "role": "user_volume_accumulator",
                        "candidate_class": "creatable_or_optional_route_pda",
                        "candidate_exclusion_reason": "route_user_volume_accumulator_not_precheck_required",
                    }
                ],
            }
            bonding_curve_v2_skip = {
                **probe_base,
                "ab_record_id": "source-ab-4",
                "source_ab_record_id": "source-ab-4",
                "probe_id": "probe-bcv2-skip",
                "v3_feature_snapshot_hash": "feature-hash-4",
                "event_type": "probe_skipped",
                "probe_skip_reason": "execution_account_not_ready",
                "precheck_failure_reason": "execution_account_not_ready:bonding_curve_v2:bc-v2",
                "execution_account_readiness_status": "not_ready",
                "execution_account_readiness_role": "bonding_curve_v2",
                "execution_account_readiness_pubkey": "bc-v2",
                "execution_account_readiness_reason": "execution_account_not_ready:bonding_curve_v2:bc-v2",
            }
            write_jsonl(
                root / "logs/rollout/r16-r5/decisions/gatekeeper_v2_decisions.jsonl",
                decisions,
            )
            rows = [exact, multi, all_nonfatal]
            write_jsonl(root / "logs/shadow_run/r16-r5/probe_selected.jsonl", rows)
            write_jsonl(
                root / "logs/shadow_run/r16-r5/probe_skipped.jsonl",
                [bonding_curve_v2_skip],
            )
            write_jsonl(root / "logs/shadow_run/r16-r5/probe_transport.jsonl", rows)
            write_jsonl(root / "logs/shadow_run/r16-r5/probe_entries.jsonl", rows)

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(materialization["account_not_found_rows"], 3)
        self.assertEqual(materialization["exact_after_narrowing_rows"], 1)
        self.assertEqual(materialization["multi_candidate_narrowed_rows"], 1)
        self.assertEqual(materialization["all_candidates_nonfatal_but_sim_failed_rows"], 1)
        self.assertEqual(materialization["simulation_required_account_not_in_precheck_rows"], 1)
        self.assertEqual(materialization["simulation_account_meta_missing_on_rpc_rows"], 1)
        self.assertEqual(
            materialization["bonding_curve_v2_account_not_found_after_simulation_rows"],
            1,
        )
        self.assertEqual(
            materialization["bonding_curve_v2_precheck_skipped_before_simulation_rows"],
            1,
        )
        self.assertEqual(
            materialization["skip_execution_account_readiness_role_counts"]["bonding_curve_v2"],
            1,
        )
        self.assertEqual(
            materialization["account_not_found_candidate_raw_counts"]["payer_pubkey"],
            1,
        )
        self.assertEqual(
            materialization["account_not_found_candidate_narrowed_counts"]["bonding_curve_v2"],
            2,
        )
        self.assertEqual(
            materialization["candidate_exclusion_reason_counts"][
                "ephemeral_payer_not_rpc_required"
            ],
            1,
        )
        self.assertEqual(
            materialization["candidate_exclusion_reason_counts"][
                "idempotent_ata_create_attached"
            ],
            1,
        )
        self.assertEqual(
            materialization["simulation_error_account_narrowing_status_counts"][
                "multi_candidate_narrowed"
            ],
            1,
        )
        self.assertIn(
            "multi_candidate_narrowed_requires_explicit_acceptance",
            report["probe_readiness"]["reasons"],
        )
        self.assertIn(
            "all_candidates_nonfatal_but_sim_failed_requires_rpc_visibility_review",
            report["probe_readiness"]["reasons"],
        )
        self.assertIn(
            "bonding_curve_v2_account_not_found_after_simulation",
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

    def test_active_shadow_account_not_found_attribution_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-buy",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            failure = {
                **common,
                "dispatch_status": "failed",
                "simulation_outcome": "failed",
                "err": "shadow RPC simulate failed: AccountNotFound",
                "active_shadow_precheck_status": "not_run_post_simulation_attribution",
                "active_shadow_lifecycle_eligibility_status": "not_lifecycle_eligible",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_attributed",
                "simulation_error_account_pubkey": "missing-account",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_candidates_narrowed": [
                    {
                        "pubkey": "missing-account",
                        "role": "bonding_curve_v2",
                        "source": "route_builder",
                        "required": True,
                    }
                ],
                "account_set_match": True,
            }
            write_jsonl(
                root / "logs/rollout/r16/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(root / "logs/shadow_run/r16/buys.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_entries.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_lifecycle.jsonl", [failure])

            report = audit.build_report(config)

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(active["active_shadow_dispatch_failure_rows"], 3)
        self.assertEqual(active["active_shadow_account_not_found_rows"], 3)
        self.assertEqual(active["active_shadow_account_not_found_attributed_rows"], 3)
        self.assertEqual(active["active_shadow_account_not_found_unattributed_rows"], 0)
        self.assertEqual(
            active["active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows"],
            3,
        )
        self.assertEqual(active["active_shadow_lifecycle_eligible_failure_rows"], 0)
        self.assertEqual(
            active["active_shadow_account_not_found_role_counts"],
            {"bonding_curve_v2": 3},
        )
        self.assertIn(
            "active_shadow_bonding_curve_v2_account_not_found_after_simulation",
            report["readiness"]["reasons"],
        )
        self.assertNotIn(
            "active_shadow_dispatch_failure_marked_lifecycle_eligible",
            report["readiness"]["reasons"],
        )

    def test_active_shadow_precheck_bonding_curve_v2_failure_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-buy",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
            }
            failure = {
                **common,
                "dispatch_status": "failed",
                "simulation_outcome": "failed",
                "err": "execution_account_not_ready:bonding_curve_v2:bc-v2",
                "active_shadow_precheck_status": "precheck_failed",
                "active_shadow_lifecycle_eligibility_status": "not_lifecycle_eligible",
                "precheck_failure_reason": "execution_account_not_ready:bonding_curve_v2:bc-v2",
                "simulation_error_category": "active_shadow_precheck_failed",
                "simulation_error_account_pubkey": "bc-v2",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_candidates_narrowed": [
                    {
                        "pubkey": "bc-v2",
                        "role": "bonding_curve_v2",
                        "source": "route_builder",
                        "required": True,
                    }
                ],
                "bonding_curve_v2_pubkey": "bc-v2",
                "bonding_curve_v2_source": "route_builder",
                "bonding_curve_v2_authority_status": "builder_only",
                "bonding_curve_v2_mismatch_reason": "builder_pubkey_not_materialized",
                "builder_required_curve_account_ready": False,
                "account_set_match": True,
            }
            write_jsonl(
                root / "logs/rollout/r16/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(root / "logs/shadow_run/r16/buys.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_entries.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_lifecycle.jsonl", [failure])

            report = audit.build_report(config)

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(active["active_shadow_precheck_failed_rows"], 3)
        self.assertEqual(active["active_shadow_runtime_simulation_error_rows"], 0)
        self.assertEqual(
            active["active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows"],
            3,
        )
        self.assertEqual(
            active["active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows"],
            0,
        )
        self.assertEqual(
            active["active_shadow_bonding_curve_v2_authority_status_counts"]["builder_only"],
            3,
        )
        self.assertEqual(
            active["active_shadow_bonding_curve_v2_mismatch_reason_counts"][
                "builder_pubkey_not_materialized"
            ],
            3,
        )
        self.assertEqual(
            active["active_shadow_builder_required_curve_account_ready_counts"]["false"],
            3,
        )
        self.assertEqual(active["active_shadow_builder_bcv2_derived_unverified_rows"], 3)
        self.assertEqual(active["active_shadow_route_excluded_bcv2_missing_rows"], 3)
        self.assertEqual(active["active_shadow_route_fallback_attempted_rows"], 0)
        self.assertEqual(active["active_shadow_route_fallback_success_rows"], 0)
        self.assertEqual(active["active_shadow_account_not_found_rows"], 0)
        self.assertEqual(active["active_shadow_lifecycle_eligible_failure_rows"], 0)
        self.assertIn(
            "active_shadow_bonding_curve_v2_source_not_authoritative",
            report["readiness"]["reasons"],
        )

    def test_bonding_curve_v2_source_authority_counts_are_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-source.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-source/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r16-source/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r16-source/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r16-source/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r16-source/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-source/probe_lifecycle.jsonl"
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
                "probe_id": "probe-bcv2-source",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "bonding_curve_v2_pubkey": "bc-v2",
                "bonding_curve_v2_source": "route_builder",
                "bonding_curve_v2_authority_status": "builder_only",
                "bonding_curve_v2_mismatch_reason": "builder_pubkey_not_materialized",
                "builder_required_curve_account_ready": False,
            }
            source_skip = {
                **probe_common,
                "event_type": "probe_skipped",
                "probe_skip_reason": "bonding_curve_v2_source_not_authoritative",
                "precheck_failure_reason": (
                    "bonding_curve_v2_source_not_authoritative:"
                    "builder_only:route_builder:bc-v2"
                ),
                "execution_account_readiness_status": "not_ready",
                "execution_account_readiness_role": "bonding_curve_v2",
                "execution_account_readiness_pubkey": "bc-v2",
            }
            write_jsonl(
                root / "logs/rollout/r16-source/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(root / "logs/shadow_run/r16-source/probe_selected.jsonl", [probe_common])
            write_jsonl(root / "logs/shadow_run/r16-source/probe_skipped.jsonl", [source_skip])
            write_jsonl(root / "logs/shadow_run/r16-source/probe_transport.jsonl", [probe_common])

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(
            materialization["bonding_curve_v2_authority_status_counts"]["builder_only"],
            1,
        )
        self.assertEqual(
            materialization["bonding_curve_v2_mismatch_reason_counts"][
                "builder_pubkey_not_materialized"
            ],
            1,
        )
        self.assertEqual(
            materialization["builder_required_curve_account_ready_counts"]["false"],
            1,
        )
        self.assertEqual(
            materialization["skip_bonding_curve_v2_authority_status_counts"]["builder_only"],
            1,
        )
        self.assertEqual(materialization["builder_bcv2_derived_unverified_rows"], 2)
        self.assertEqual(materialization["route_excluded_bcv2_missing_rows"], 2)
        self.assertEqual(materialization["route_fallback_attempted_rows"], 0)
        self.assertEqual(materialization["route_fallback_success_rows"], 0)
        self.assertIn(
            "bonding_curve_v2_source_not_authoritative",
            report["probe_readiness"]["reasons"],
        )
        self.assertIn(
            "bonding_curve_v2_source_not_authoritative_skip",
            report["probe_readiness"]["reasons"],
        )

    def test_l1r13_observed_tx_bonding_curve_v2_counts_as_authoritative(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-observed.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-observed/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r16-observed/probe_selected.jsonl"
transport_log_path = "../../logs/shadow_run/r16-observed/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r16-observed/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-observed/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            decision = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "observed-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            transport = {
                **decision,
                "source_ab_record_id": "observed-ab",
                "probe_id": "probe-observed-bcv2",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_dispatched",
                "bonding_curve_v2_pubkey": "observed-bc-v2",
                "bonding_curve_v2_source": "observed_tx_account_meta",
                "bonding_curve_v2_authority_status": "authoritative_observed_tx",
                "bonding_curve_v2_identity_authority_status": "authoritative_observed_tx",
                "bonding_curve_v2_rpc_load_status": "rpc_load_ready",
                "bonding_curve_v2_rpc_load_ready": True,
                "builder_required_curve_account_ready": True,
                "builder_required_curve_account_ready_reason": "load_ready:rpc_load_ready",
                "observed_bcv2_source_tx_signature": "sig",
                "observed_bcv2_source_slot": 42,
                "observed_bcv2_source_slot_index": 0,
                "observed_bcv2_source_instruction_index": 3,
                "observed_bcv2_source_program_id": "pump",
                "observed_bcv2_source_discriminator": "disc",
                "observed_bcv2_source_buy_variant": "routed_exact_sol_in",
                "observed_bcv2_instruction_account_position": 16,
                "observed_bcv2_message_account_index": 24,
                "observed_bcv2_resolved_pubkey": "observed-bc-v2",
                "observed_bcv2_loaded_address_source": "resolved_transaction_account_keys",
                "observed_bcv2_tx_success": True,
                "observed_bcv2_provenance_status": "route_compatible",
            }
            entry = {
                **transport,
                "probe_entry_materialization_status": "entry_materialized",
                "probe_lifecycle_eligibility_status": "lifecycle_eligible",
            }
            write_jsonl(
                root / "logs/rollout/r16-observed/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-observed/probe_selected.jsonl",
                [transport],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-observed/probe_transport.jsonl",
                [transport],
            )
            write_jsonl(root / "logs/shadow_run/r16-observed/probe_entries.jsonl", [entry])

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(
            materialization["builder_bcv2_authoritative_observed_tx_rows"],
            1,
        )
        self.assertEqual(materialization["builder_bcv2_derived_unverified_rows"], 0)
        self.assertEqual(materialization["route_excluded_bcv2_missing_rows"], 0)
        self.assertEqual(materialization["successful_probe_entry_rows"], 1)
        self.assertEqual(
            materialization["bonding_curve_v2_identity_authority_status_counts"][
                "authoritative_observed_tx"
            ],
            1,
        )
        self.assertEqual(
            materialization["bonding_curve_v2_rpc_load_ready_counts"]["true"],
            1,
        )
        self.assertEqual(
            materialization["builder_required_curve_account_ready_reason_counts"][
                "load_ready:rpc_load_ready"
            ],
            1,
        )
        self.assertEqual(materialization["observed_bcv2_route_compatible_rows"], 1)
        self.assertEqual(
            materialization["observed_bcv2_authoritative_without_route_compatible_rows"],
            0,
        )

    def test_l1r15_observed_tx_authoritative_requires_route_compatible_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-observed-bad-provenance.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-observed-bad-provenance/decisions"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r16-observed-bad-provenance/probe_selected.jsonl"
transport_log_path = "../../logs/shadow_run/r16-observed-bad-provenance/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r16-observed-bad-provenance/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-observed-bad-provenance/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            decision = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "observed-bad-ab",
                "pool_id": "pool",
                "base_mint": "mint",
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            transport = {
                **decision,
                "source_ab_record_id": "observed-bad-ab",
                "probe_id": "probe-observed-bad-bcv2",
                "dispatch_source": "counterfactual_shadow_probe",
                "collection_plane": "counterfactual_shadow_probe",
                "probe_plane": "p37_shadow_probe",
                "probe_bucket": "v3_pending_wait_sample",
                "probe_amount_source": "fixed_lamports",
                "execution_outcome": "counterfactual_shadow_probe_dispatched",
                "bonding_curve_v2_pubkey": "observed-bc-v2",
                "bonding_curve_v2_source": "observed_tx_account_meta",
                "bonding_curve_v2_authority_status": "authoritative_observed_tx",
                "bonding_curve_v2_identity_authority_status": "authoritative_observed_tx",
                "bonding_curve_v2_rpc_load_status": "rpc_load_ready",
                "bonding_curve_v2_rpc_load_ready": True,
                "builder_required_curve_account_ready": True,
                "builder_required_curve_account_ready_reason": "load_ready:rpc_load_ready",
                "observed_bcv2_instruction_account_position": 16,
                "observed_bcv2_message_account_index": 24,
                "observed_bcv2_resolved_pubkey": "observed-bc-v2",
                "observed_bcv2_provenance_status": "program_id_mismatch",
            }
            entry = {
                **transport,
                "probe_entry_materialization_status": "entry_materialized",
                "probe_lifecycle_eligibility_status": "lifecycle_eligible",
            }
            write_jsonl(
                root
                / "logs/rollout/r16-observed-bad-provenance/decisions/gatekeeper_v2_decisions.jsonl",
                [decision],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-observed-bad-provenance/probe_selected.jsonl",
                [transport],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-observed-bad-provenance/probe_transport.jsonl",
                [transport],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-observed-bad-provenance/probe_entries.jsonl",
                [entry],
            )

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(materialization["observed_bcv2_not_route_compatible_rows"], 1)
        self.assertEqual(
            materialization["observed_bcv2_authoritative_without_route_compatible_rows"],
            1,
        )
        self.assertIn(
            "observed_bcv2_authoritative_without_route_compatible",
            report["probe_readiness"]["reasons"],
        )

    def test_active_shadow_data_problem_entry_is_not_successful(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-buy",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            entry_failure = {
                **common,
                "execution_outcome": "shadow_data_problem",
                "active_shadow_lifecycle_eligibility_status": "not_lifecycle_eligible",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_attributed",
                "simulation_error_account_pubkey": "missing-account",
                "simulation_error_account_role": "bonding_curve_v2",
                "account_set_match": True,
            }
            write_jsonl(
                root / "logs/rollout/r16/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(root / "logs/shadow_run/r16/shadow_entries.jsonl", [entry_failure])

            report = audit.build_report(config)

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(active["active_shadow_entry_rows"], 1)
        self.assertEqual(active["active_shadow_dispatch_failure_rows"], 1)
        self.assertEqual(active["active_shadow_successful_entry_rows"], 0)
        self.assertEqual(active["active_shadow_lifecycle_eligible_rows"], 0)
        self.assertEqual(active["active_shadow_account_not_found_attributed_rows"], 1)

    def test_active_shadow_observed_tx_authoritative_requires_route_compatible_provenance(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-active-bad-provenance.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-active-bad-provenance/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16-active-bad-provenance/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16-active-bad-provenance/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-active-bad-provenance/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-buy",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            failure = {
                **common,
                "execution_outcome": "shadow_data_problem",
                "active_shadow_lifecycle_eligibility_status": "not_lifecycle_eligible",
                "bonding_curve_v2_pubkey": "observed-bc-v2",
                "bonding_curve_v2_source": "observed_tx_account_meta",
                "bonding_curve_v2_identity_authority_status": "authoritative_observed_tx",
                "bonding_curve_v2_rpc_load_status": "missing_on_rpc_precheck",
                "bonding_curve_v2_rpc_load_ready": False,
                "builder_required_curve_account_ready": False,
                "builder_required_curve_account_ready_reason": (
                    "bonding_curve_v2_observed_meta_not_route_compatible"
                ),
                "observed_bcv2_instruction_account_position": 16,
                "observed_bcv2_message_account_index": 24,
                "observed_bcv2_resolved_pubkey": "observed-bc-v2",
                "observed_bcv2_provenance_status": "program_id_mismatch",
            }
            write_jsonl(
                root
                / "logs/rollout/r16-active-bad-provenance/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-active-bad-provenance/shadow_entries.jsonl",
                [failure],
            )

            report = audit.build_report(config)

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(active["active_shadow_observed_bcv2_not_route_compatible_rows"], 1)
        self.assertEqual(
            active["active_shadow_observed_bcv2_authoritative_without_route_compatible_rows"],
            1,
        )
        self.assertIn(
            "active_shadow_observed_bcv2_authoritative_without_route_compatible",
            report["readiness"]["reasons"],
        )

    def test_l1r16_route_resolver_counters_are_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16-route-resolver.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16-route-resolver/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16-route-resolver/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16-route-resolver/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-route-resolver/shadow_lifecycle.jsonl"

[p37_shadow_probe]
selection_log_path = "../../logs/shadow_run/r16-route-resolver/probe_selected.jsonl"
skip_log_path = "../../logs/shadow_run/r16-route-resolver/probe_skipped.jsonl"
transport_log_path = "../../logs/shadow_run/r16-route-resolver/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/r16-route-resolver/probe_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16-route-resolver/probe_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-route",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
                "v3_feature_snapshot_hash": "feature-hash",
                "v3_policy_config_hash": "policy-hash",
            }
            route_fields = {
                "route_resolution_status": "no_executable_route_account_set",
                "selected_route_reason": "no_route_candidate_passed_simulation_load_readiness",
                "primary_route_kind": "routed_exact_sol_in",
                "primary_route_ready": False,
                "primary_route_not_ready_reason": "bonding_curve_v2_observed_meta_missing_on_rpc",
                "fallback_route_kind": "legacy_buy",
                "fallback_route_attempted": True,
                "fallback_route_ready": False,
                "fallback_route_not_ready_reason": (
                    "fallback_route_requires_same_bcv2_simulation_load_account"
                ),
                "fallback_missing_roles": ["bonding_curve_v2"],
                "fallback_missing_pubkeys": ["bc-v2"],
                "fallback_account_sources": ["primary_route_account_set"],
                "fallback_simulation_load_account_set": [
                    "bonding_curve_v2:bc-v2:observed_tx_account_meta"
                ],
                "fallback_creatable_account_set": [],
                "fallback_required_precheck_account_set": [
                    "bonding_curve_v2:bc-v2:observed_tx_account_meta"
                ],
                "fallback_failure_class": "fallback_builder_account_source_unverified",
                "no_executable_route_account_set_reason": (
                    "primary_route_bcv2_missing:bonding_curve_v2:bc-v2"
                ),
                "legacy_buy_account_set_status": "not_ready",
                "legacy_buy_curve_pubkey": "legacy-curve",
                "legacy_buy_curve_source": "materialized_feature_set",
                "legacy_buy_curve_authority_status": "authoritative_mfs",
                "legacy_buy_curve_rpc_load_status": "present",
                "legacy_buy_curve_rpc_load_ready": True,
                "legacy_buy_associated_bonding_curve_pubkey": "associated-curve",
                "legacy_buy_associated_bonding_curve_source": "route_builder",
                "legacy_buy_associated_bonding_curve_rpc_load_ready": True,
                "legacy_buy_required_roles": ["bonding_curve", "bonding_curve_v2"],
                "legacy_buy_missing_roles": ["bonding_curve_v2"],
                "legacy_buy_missing_pubkeys": ["bc-v2"],
                "legacy_buy_route_ready": False,
                "legacy_buy_route_not_ready_reason": "legacy_buy_missing_route_identity",
            }
            no_executable_reason = (
                "no_executable_route_account_set:"
                "primary_route_bcv2_missing:bonding_curve_v2:bc-v2"
            )
            probe_skip = {
                **common,
                **route_fields,
                "event_type": "probe_skipped",
                "probe_id": "probe-route",
                "probe_skip_reason": "no_executable_route_account_set",
                "precheck_failure_reason": no_executable_reason,
                "execution_account_readiness_role": "bonding_curve_v2",
                "execution_account_readiness_pubkey": "bc-v2",
                "execution_account_readiness_reason": no_executable_reason,
            }
            active_failure = {
                **common,
                **route_fields,
                "execution_outcome": "shadow_data_problem",
                "active_shadow_precheck_status": "precheck_failed",
                "active_shadow_lifecycle_eligibility_status": "not_lifecycle_eligible",
                "precheck_failure_reason": no_executable_reason,
                "simulation_error_category": "active_shadow_precheck_failed",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_pubkey": "bc-v2",
            }
            write_jsonl(
                root / "logs/rollout/r16-route-resolver/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-route-resolver/probe_skipped.jsonl",
                [probe_skip],
            )
            write_jsonl(
                root / "logs/shadow_run/r16-route-resolver/shadow_entries.jsonl",
                [active_failure],
            )

            report = audit.build_report(config)

        materialization = report["probe_entry_materialization"]
        self.assertEqual(
            materialization["route_resolution_status_counts"],
            {"no_executable_route_account_set": 1},
        )
        self.assertEqual(materialization["route_fallback_attempted_rows"], 1)
        self.assertEqual(materialization["route_fallback_success_rows"], 0)
        self.assertEqual(materialization["route_fallback_failed_rows"], 1)
        self.assertEqual(materialization["legacy_buy_route_attempted_rows"], 1)
        self.assertEqual(materialization["legacy_buy_route_ready_rows"], 0)
        self.assertEqual(materialization["legacy_buy_route_not_ready_rows"], 1)
        self.assertEqual(materialization["legacy_buy_missing_core_curve_account_rows"], 0)
        self.assertEqual(
            materialization["legacy_buy_missing_associated_bonding_curve_rows"],
            0,
        )
        self.assertEqual(materialization["legacy_buy_authoritative_curve_rows"], 1)
        self.assertEqual(materialization["legacy_buy_rpc_load_ready_rows"], 1)
        self.assertEqual(materialization["legacy_buy_successful_entry_rows"], 0)
        self.assertEqual(
            materialization["legacy_buy_curve_authority_status_counts"],
            {"authoritative_mfs": 1},
        )
        self.assertEqual(
            materialization["fallback_failure_class_counts"],
            {"fallback_builder_account_source_unverified": 1},
        )
        self.assertEqual(
            materialization["fallback_missing_role_counts"],
            {"bonding_curve_v2": 1},
        )
        self.assertEqual(
            materialization["fallback_account_source_counts"],
            {"primary_route_account_set": 1},
        )
        self.assertFalse(materialization["fallback_repairable"])
        self.assertEqual(
            materialization["recommended_next_path"],
            "route_class_exclusion_from_execution_label_universe",
        )
        self.assertEqual(materialization["executable_route_ready_rows"], 0)
        self.assertEqual(materialization["primary_route_bcv2_missing_rows"], 1)
        self.assertEqual(materialization["no_executable_route_account_set_rows"], 1)
        self.assertEqual(materialization["route_executable_rows"], 0)
        self.assertEqual(materialization["route_non_executable_rows"], 1)
        self.assertEqual(materialization["execution_feasibility_reject_rows"], 1)
        self.assertEqual(
            materialization["execution_feasibility_status_counts"],
            {"not_executable_route": 1},
        )
        self.assertEqual(
            materialization["execution_feasibility_reason_counts"],
            {"no_executable_route_account_set": 1},
        )
        self.assertEqual(
            materialization["lifecycle_label_eligibility_counts"],
            {"not_lifecycle_label_eligible": 1},
        )

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(
            active["active_shadow_route_resolution_status_counts"],
            {"no_executable_route_account_set": 1},
        )
        self.assertEqual(active["active_shadow_route_fallback_attempted_rows"], 1)
        self.assertEqual(active["active_shadow_route_fallback_success_rows"], 0)
        self.assertEqual(active["active_shadow_route_fallback_failed_rows"], 1)
        self.assertEqual(active["active_shadow_legacy_buy_route_attempted_rows"], 1)
        self.assertEqual(active["active_shadow_legacy_buy_route_ready_rows"], 0)
        self.assertEqual(active["active_shadow_legacy_buy_route_not_ready_rows"], 1)
        self.assertEqual(
            active["active_shadow_legacy_buy_missing_core_curve_account_rows"],
            0,
        )
        self.assertEqual(active["active_shadow_legacy_buy_authoritative_curve_rows"], 1)
        self.assertEqual(active["active_shadow_legacy_buy_rpc_load_ready_rows"], 1)
        self.assertEqual(active["active_shadow_legacy_buy_successful_entry_rows"], 0)
        self.assertEqual(
            active["active_shadow_fallback_failure_class_counts"],
            {"fallback_builder_account_source_unverified": 1},
        )
        self.assertEqual(
            active["active_shadow_fallback_missing_role_counts"],
            {"bonding_curve_v2": 1},
        )
        self.assertEqual(
            active["active_shadow_fallback_account_source_counts"],
            {"primary_route_account_set": 1},
        )
        self.assertFalse(active["active_shadow_fallback_repairable"])
        self.assertEqual(
            active["active_shadow_recommended_next_path"],
            "route_class_exclusion_from_execution_label_universe",
        )
        self.assertEqual(active["active_shadow_executable_route_ready_rows"], 0)
        self.assertEqual(active["active_shadow_primary_route_bcv2_missing_rows"], 1)
        self.assertEqual(active["active_shadow_no_executable_route_account_set_rows"], 1)
        self.assertEqual(active["active_shadow_route_executable_rows"], 0)
        self.assertEqual(active["active_shadow_route_non_executable_rows"], 1)
        self.assertEqual(active["active_shadow_execution_feasibility_reject_rows"], 1)
        self.assertEqual(active["active_buy_execution_infeasible_rows"], 1)
        self.assertEqual(
            active["active_shadow_execution_feasibility_status_counts"],
            {"not_executable_route": 1},
        )
        self.assertEqual(
            active["active_shadow_execution_feasibility_reason_counts"],
            {"no_executable_route_account_set": 1},
        )
        self.assertEqual(
            active["active_shadow_lifecycle_label_eligibility_counts"],
            {"not_lifecycle_label_eligible": 1},
        )

        feasibility = report["execution_feasibility"]
        self.assertEqual(feasibility["route_executable_rows"], 0)
        self.assertEqual(feasibility["route_non_executable_rows"], 2)
        self.assertEqual(feasibility["execution_feasibility_reject_rows"], 2)
        self.assertEqual(feasibility["successful_entry_rows"], 0)
        self.assertEqual(feasibility["lifecycle_eligible_rows"], 0)
        self.assertEqual(feasibility["active_buy_execution_infeasible_rows"], 1)

    def test_e3_legacy_buy_authority_readiness_counters_are_reported(self) -> None:
        rows = [
            {
                "selected_route_kind": "legacy_buy",
                "legacy_buy_curve_authority_status": "authoritative_cross_checked",
                "legacy_buy_curve_authority_readiness_status": "authoritative_and_load_ready",
                "legacy_buy_curve_rpc_load_status": "present_on_rpc_precheck",
                "legacy_buy_curve_rpc_load_ready": True,
                "legacy_buy_route_ready": True,
            },
            {
                "fallback_route_kind": "legacy_buy",
                "legacy_buy_curve_authority_status": "derived_unverified",
                "legacy_buy_curve_authority_readiness_status": (
                    "load_ready_but_authority_unverified"
                ),
                "legacy_buy_curve_rpc_load_status": "present_on_rpc_precheck",
                "legacy_buy_curve_rpc_load_ready": True,
                "legacy_buy_route_ready": False,
            },
            {
                "fallback_route_kind": "legacy_buy",
                "legacy_buy_curve_authority_status": "authoritative_account_state",
                "legacy_buy_curve_authority_readiness_status": (
                    "authoritative_but_not_load_checked"
                ),
                "legacy_buy_curve_rpc_load_status": "not_checked",
                "legacy_buy_curve_rpc_load_ready": False,
                "legacy_buy_route_ready": False,
            },
            {
                "fallback_route_kind": "legacy_buy",
                "legacy_buy_curve_authority_status": (
                    "derived_mismatch_authoritative_source"
                ),
                "legacy_buy_curve_authority_readiness_status": (
                    "derived_mismatch_authoritative_source"
                ),
                "legacy_buy_route_ready": False,
            },
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["legacy_buy_curve_authoritative_and_load_ready_rows"], 1)
        self.assertEqual(
            payload["legacy_buy_curve_load_ready_but_authority_unverified_rows"],
            1,
        )
        self.assertEqual(payload["legacy_buy_curve_authoritative_but_not_checked_rows"], 1)
        self.assertEqual(payload["legacy_buy_curve_derived_matches_account_state_rows"], 1)
        self.assertEqual(payload["legacy_buy_curve_derived_mismatch_account_state_rows"], 1)
        self.assertEqual(payload["legacy_buy_route_ready_after_reconciliation_rows"], 1)
        self.assertEqual(payload["legacy_buy_route_still_not_ready_after_reconciliation_rows"], 3)
        self.assertEqual(
            payload["legacy_buy_curve_authority_readiness_status_counts"],
            {
                "authoritative_and_load_ready": 1,
                "authoritative_but_not_load_checked": 1,
                "derived_mismatch_authoritative_source": 1,
                "load_ready_but_authority_unverified": 1,
            },
        )

    def test_e4_legacy_buy_account_set_separation_counters_are_reported(self) -> None:
        rows = [
            {
                "fallback_route_kind": "legacy_buy",
                "fallback_account_sources": ["primary_route_account_set"],
                "fallback_missing_roles": ["bonding_curve_v2"],
                "fallback_required_precheck_account_set": [
                    "bonding_curve_v2:bcv2:primary_route_account_set"
                ],
                "legacy_buy_route_ready": False,
            },
            {
                "fallback_route_kind": "legacy_buy",
                "fallback_creatable_account_set": [
                    "user_ata:ata:user_ata",
                    "user_volume_accumulator:uva:route_builder",
                ],
                "fallback_missing_roles": [],
                "payer_provenance": "ephemeral",
                "legacy_buy_route_ready": True,
            },
            {
                "fallback_route_kind": "legacy_buy",
                "fallback_missing_roles": ["associated_bonding_curve"],
                "legacy_buy_route_ready": False,
            },
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["legacy_buy_primary_bcv2_leak_rows"], 1)
        self.assertEqual(payload["legacy_buy_missing_creatable_user_ata_rows"], 0)
        self.assertEqual(
            payload["legacy_buy_missing_creatable_user_volume_accumulator_rows"],
            0,
        )
        self.assertEqual(payload["legacy_buy_missing_ephemeral_payer_rows"], 0)
        self.assertEqual(payload["legacy_buy_non_blocking_missing_creatable_rows"], 1)
        self.assertEqual(payload["legacy_buy_non_blocking_ephemeral_payer_rows"], 1)
        self.assertEqual(payload["legacy_buy_blocking_missing_required_rows"], 2)
        self.assertEqual(payload["legacy_buy_fallback_account_set_ready_rows"], 1)
        self.assertEqual(
            payload["legacy_buy_route_ready_after_account_set_separation_rows"],
            1,
        )

    def test_e4r_selected_fallback_handoff_counters_are_reported(self) -> None:
        rows = [
            {
                "selected_route_kind": "legacy_buy",
                "selected_route_source": "selected_fallback_route_execution_handoff",
                "selected_route_handoff_status": "selected_route_handoff_applied",
                "selected_route_precheck_hash": "legacy-precheck",
                "selected_route_simulation_hash": "legacy-simulation",
                "precheck_account_set_hash": "legacy-precheck",
                "simulation_account_set_hash": "legacy-simulation",
                "buy_variant": "legacy_buy",
                "fallback_route_kind": "legacy_buy",
            },
            {
                "selected_route_kind": "legacy_buy",
                "route_resolution_status": "fallback_route_ready",
                "fallback_route_ready": True,
                "fallback_route_kind": "legacy_buy",
                "buy_variant": "routed_exact_sol_in",
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "primary_route_bcv2_missing:bonding_curve_v2:bcv2"
                ),
            },
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["selected_fallback_route_ready_rows"], 2)
        self.assertEqual(payload["selected_fallback_route_handoff_applied_rows"], 1)
        self.assertEqual(payload["selected_fallback_route_handoff_mismatch_rows"], 0)
        self.assertEqual(payload["selected_fallback_route_handoff_not_applied_rows"], 1)
        self.assertEqual(
            payload["selected_fallback_route_blocked_by_primary_reason_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_primary_bcv2_terminal_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_request_variant_not_legacy_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows"],
            0,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_precheck_hash_mismatch_rows"],
            0,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_simulation_hash_mismatch_rows"],
            0,
        )
        self.assertEqual(
            payload["legacy_buy_selected_and_precheck_uses_legacy_account_set_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_and_simulation_uses_legacy_account_set_rows"],
            1,
        )

    def test_x2_working_builder_parity_counters_are_reported(self) -> None:
        rows = [
            {
                "working_builder_parity_mode": "working_builder_parity",
                "working_builder_request_built": True,
                "buy_variant": "routed_exact_sol_in",
                "working_builder_buy_variant": "routed_exact_sol_in",
                "working_builder_rpc_manifest_hash": "rpc-hash",
                "working_builder_sender_manifest_hash": "sender-hash",
                "working_builder_manifest_contains_bcv2": True,
                "working_builder_rpc_manifest_account_roles": [
                    "16:bonding_curve_v2:bcv2:observed_tx_account_meta:required=true"
                ],
                "working_builder_missing_required_accounts": [],
                "working_builder_bcv2_pubkey": "bcv2",
                "bonding_curve_v2_pubkey": "bcv2",
                "observed_bcv2_resolved_pubkey": "bcv2",
                "working_builder_bcv2_source_authority": "authoritative_observed_tx",
                "working_builder_bcv2_rpc_load_status": "rpc_load_ready",
                "working_builder_bcv2_rpc_load_ready": True,
                "working_builder_bcv2_seen_in_observed_tx": True,
                "working_builder_bcv2_seen_in_account_state": False,
                "working_builder_bcv2_seen_in_mfs": False,
                "working_builder_bcv2_seen_in_diag": False,
                "working_builder_bcv2_readiness_reason": "load_ready:rpc_load_ready",
                "working_builder_bcv2_precheck_pubkey": "bcv2",
                "working_builder_bcv2_builder_pubkey": "bcv2",
                "working_builder_bcv2_observed_pubkey": "bcv2",
                "working_builder_bcv2_pubkey_consistency_status": "builder_observed_precheck_match",
                "working_builder_bcv2_observed_slot": 100,
                "working_builder_bcv2_observed_tx_signature": "sig-1",
                "working_builder_bcv2_precheck_context_slot": 105,
                "working_builder_bcv2_precheck_commitment": "processed",
                "working_builder_bcv2_precheck_attempt_count": 1,
                "working_builder_bcv2_precheck_latency_ms": 7,
                "working_builder_bcv2_precheck_age_from_observed_slot": 5,
                "working_builder_bcv2_reconciliation_class": "rpc_ready_but_account_state_missing",
                "working_builder_bcv2_materialization_class": "rpc_fetch_ready",
                "working_builder_bcv2_subscription_requested": False,
                "working_builder_bcv2_account_update_received": False,
                "working_builder_bcv2_account_update_mapped": False,
                "working_builder_bcv2_rpc_fetch_ready": True,
                "working_builder_bcv2_rpc_fetch_missing": False,
                "working_builder_bcv2_rpc_fetch_owner": "pump-program",
                "working_builder_bcv2_rpc_fetch_data_len": 256,
                "working_builder_bcv2_account_state_materialized": False,
                "working_builder_bcv2_mfs_materialized": False,
                "working_builder_bcv2_diag_materialized": False,
                "working_builder_bcv2_account_state_lookup_performed": True,
                "working_builder_bcv2_account_state_seen": False,
                "working_builder_bcv2_mfs_seen_reason": "mfs_missing_bonding_curve_v2_identity",
                "working_builder_bcv2_diag_seen_reason": "diag_missing_bonding_curve_v2_identity",
                "working_builder_bcv2_local_coverage_class": "observed_only_no_account_state",
                "observed_bcv2_loaded_address_source": "static_message_key",
                "working_builder_creator_vault_pubkey": "creator-vault",
                "working_builder_creator_vault_source_authority": "authoritative_detected_pool_creator",
                "working_builder_creator_vault_rpc_load_status": "rpc_load_ready",
                "working_builder_creator_vault_rpc_load_ready": True,
                "working_builder_creator_vault_seen_in_account_state": False,
                "working_builder_creator_vault_seen_in_mfs": False,
                "working_builder_creator_vault_seen_in_observed_tx": False,
                "working_builder_creator_vault_readiness_reason": "load_ready:rpc_load_ready",
            },
            {
                "working_builder_parity_mode": "working_builder_parity",
                "working_builder_request_built": True,
                "buy_variant": "routed_exact_sol_in",
                "working_builder_buy_variant": "routed_exact_sol_in",
                "working_builder_rpc_manifest_hash": "rpc-hash-2",
                "working_builder_missing_required_accounts": [
                    "bonding_curve_v2:bcv2:observed_tx_account_meta"
                ],
                "working_builder_bcv2_pubkey": "bcv2-2",
                "bonding_curve_v2_pubkey": "bcv2-2",
                "observed_bcv2_resolved_pubkey": "bcv2-2",
                "working_builder_bcv2_source_authority": "authoritative_observed_tx",
                "working_builder_bcv2_rpc_load_status": "missing_on_rpc_precheck",
                "working_builder_bcv2_rpc_load_ready": False,
                "working_builder_bcv2_seen_in_observed_tx": True,
                "working_builder_bcv2_seen_in_account_state": False,
                "working_builder_bcv2_seen_in_mfs": False,
                "working_builder_bcv2_seen_in_diag": False,
                "working_builder_bcv2_readiness_reason": "bonding_curve_v2_observed_meta_missing_on_rpc",
                "working_builder_bcv2_precheck_pubkey": "bcv2-2",
                "working_builder_bcv2_builder_pubkey": "bcv2-2",
                "working_builder_bcv2_observed_pubkey": "bcv2-2",
                "working_builder_bcv2_pubkey_consistency_status": "builder_observed_precheck_match",
                "working_builder_bcv2_observed_slot": 200,
                "working_builder_bcv2_observed_tx_signature": "sig-2",
                "working_builder_bcv2_precheck_context_slot": 203,
                "working_builder_bcv2_precheck_commitment": "processed",
                "working_builder_bcv2_precheck_attempt_count": 1,
                "working_builder_bcv2_precheck_latency_ms": 11,
                "working_builder_bcv2_precheck_age_from_observed_slot": 3,
                "working_builder_bcv2_rpc_error_class": "account_missing",
                "working_builder_bcv2_reconciliation_class": "local_state_gap",
                "working_builder_bcv2_materialization_class": "rpc_fetch_missing",
                "working_builder_bcv2_subscription_requested": False,
                "working_builder_bcv2_account_update_received": False,
                "working_builder_bcv2_account_update_mapped": False,
                "working_builder_bcv2_rpc_fetch_ready": False,
                "working_builder_bcv2_rpc_fetch_missing": True,
                "working_builder_bcv2_account_state_materialized": False,
                "working_builder_bcv2_mfs_materialized": False,
                "working_builder_bcv2_diag_materialized": False,
                "working_builder_bcv2_account_state_lookup_performed": True,
                "working_builder_bcv2_account_state_seen": False,
                "working_builder_bcv2_mfs_seen_reason": "mfs_missing_bonding_curve_v2_identity",
                "working_builder_bcv2_diag_seen_reason": "diag_missing_bonding_curve_v2_identity",
                "working_builder_bcv2_local_coverage_class": "observed_only_no_account_state",
                "observed_bcv2_loaded_address_source": "resolved_transaction_account_keys",
                "working_builder_creator_vault_pubkey": "creator-vault-2",
                "working_builder_creator_vault_source_authority": "creator_vault_source_not_authoritative",
                "working_builder_creator_vault_rpc_load_status": "identity_only_rpc_unverified",
                "working_builder_creator_vault_rpc_load_ready": False,
                "working_builder_creator_vault_seen_in_account_state": False,
                "working_builder_creator_vault_seen_in_mfs": False,
                "working_builder_creator_vault_seen_in_observed_tx": False,
                "working_builder_creator_vault_readiness_reason": "creator_vault_source_not_authoritative",
            },
            {
                "fallback_route_kind": "legacy_buy",
                "fallback_route_attempted": True,
                "selected_route_handoff_status": "selected_route_handoff_mismatch",
            },
        ]

        payload = audit.working_builder_parity_payload(rows)
        active_payload = audit.working_builder_parity_payload(rows, "active_shadow_")

        self.assertEqual(payload["working_builder_parity_rows"], 2)
        self.assertEqual(payload["working_builder_request_built_rows"], 2)
        self.assertEqual(payload["working_builder_buy_variant_counts"], {"routed_exact_sol_in": 2})
        self.assertEqual(payload["probe_working_builder_variant_drift_rows"], 0)
        self.assertEqual(payload["probe_working_builder_legacy_variant_rows"], 0)
        self.assertEqual(payload["probe_working_builder_selected_legacy_handoff_rows"], 0)
        self.assertEqual(payload["probe_working_builder_stale_route_diagnostics_rows"], 0)
        self.assertEqual(payload["legacy_fallback_attempted_rows"], 1)
        self.assertEqual(payload["selected_route_handoff_mismatch_rows"], 1)
        self.assertEqual(payload["working_builder_manifest_missing_required_rows"], 1)
        self.assertEqual(payload["working_builder_manifest_ready_rows"], 1)
        self.assertEqual(payload["working_builder_manifest_contains_bcv2_rows"], 1)
        self.assertEqual(
            payload["working_builder_bcv2_source_authority_counts"],
            {"authoritative_observed_tx": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_rpc_load_status_counts"],
            {"missing_on_rpc_precheck": 1, "rpc_load_ready": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_reconciliation_class_counts"],
            {"local_state_gap": 1, "rpc_ready_but_account_state_missing": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_pubkey_consistency_status_counts"],
            {"builder_observed_precheck_match": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_precheck_commitment_counts"],
            {"processed": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_rpc_error_class_counts"],
            {"account_missing": 1, "missing": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_loaded_address_source_counts"],
            {
                "resolved_transaction_account_keys": 1,
                "static_message_key": 1,
            },
        )
        self.assertEqual(
            payload["working_builder_bcv2_precheck_age_bucket_counts"],
            {"3_8": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_local_coverage_class_counts"],
            {"observed_only_no_account_state": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_materialization_class_counts"],
            {"rpc_fetch_missing": 1, "rpc_fetch_ready": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_subscription_requested_counts"],
            {"false": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_account_update_received_counts"],
            {"false": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_account_update_mapped_counts"],
            {"false": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_account_state_lookup_performed_counts"],
            {"true": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_account_state_age_bucket_counts"],
            {"missing": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_mfs_seen_reason_counts"],
            {"mfs_missing_bonding_curve_v2_identity": 2},
        )
        self.assertEqual(
            payload["working_builder_bcv2_diag_seen_reason_counts"],
            {"diag_missing_bonding_curve_v2_identity": 2},
        )
        self.assertEqual(payload["working_builder_bcv2_precheck_pubkey_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_builder_pubkey_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_observed_pubkey_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_observed_slot_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_observed_tx_signature_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_precheck_context_slot_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_precheck_attempt_count_rows"], 2)
        self.assertEqual(payload["working_builder_bcv2_precheck_latency_rows"], 2)
        self.assertEqual(
            payload["working_builder_bcv2_precheck_age_from_observed_slot_rows"], 2
        )
        self.assertEqual(
            payload["working_builder_bcv2_loaded_address_source_missing_rows"], 0
        )
        self.assertEqual(
            payload["working_builder_bcv2_account_state_lookup_performed_rows"], 2
        )
        self.assertEqual(payload["working_builder_bcv2_account_state_seen_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_state_seen_slot_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_state_age_slots_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_state_owner_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_state_data_len_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_subscription_requested_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_update_received_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_account_update_mapped_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_rpc_fetch_ready_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_rpc_fetch_missing_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_rpc_fetch_owner_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_rpc_fetch_data_len_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_account_state_materialized_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_mfs_materialized_rows"], 0)
        self.assertEqual(payload["working_builder_bcv2_diag_materialized_rows"], 0)
        self.assertEqual(
            payload["working_builder_creator_vault_source_authority_counts"],
            {
                "authoritative_detected_pool_creator": 1,
                "creator_vault_source_not_authoritative": 1,
            },
        )
        self.assertEqual(
            payload["working_builder_creator_vault_rpc_load_status_counts"],
            {"identity_only_rpc_unverified": 1, "rpc_load_ready": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_authoritative_and_load_ready_rows"], 1
        )
        self.assertEqual(
            payload["working_builder_bcv2_authoritative_but_missing_on_rpc_rows"], 1
        )
        self.assertEqual(payload["working_builder_bcv2_pubkey_mismatch_rows"], 0)
        self.assertEqual(
            payload["working_builder_bcv2_observed_tx_missing_on_rpc_rows"], 1
        )
        self.assertEqual(payload["working_builder_bcv2_account_state_missing_rows"], 2)
        self.assertEqual(
            payload[
                "working_builder_creator_vault_authoritative_and_load_ready_rows"
            ],
            1,
        )
        self.assertEqual(
            payload[
                "working_builder_creator_vault_authoritative_but_missing_on_rpc_rows"
            ],
            0,
        )
        self.assertEqual(
            payload["working_builder_creator_vault_source_mismatch_rows"], 1
        )
        self.assertEqual(
            payload[
                "working_builder_manifest_ready_after_account_source_repair_rows"
            ],
            1,
        )
        self.assertEqual(
            payload[
                "working_builder_manifest_still_not_ready_after_account_source_repair_rows"
            ],
            1,
        )
        self.assertEqual(active_payload["active_shadow_working_builder_parity_rows"], 2)
        self.assertEqual(
            active_payload["active_shadow_working_builder_buy_variant_counts"],
            {"routed_exact_sol_in": 2},
        )
        self.assertEqual(
            active_payload["active_shadow_probe_working_builder_legacy_variant_rows"],
            0,
        )
        self.assertEqual(active_payload["active_shadow_legacy_fallback_attempted_rows"], 1)
        self.assertEqual(
            active_payload[
                "active_shadow_working_builder_bcv2_authoritative_but_missing_on_rpc_rows"
            ],
            1,
        )

    def test_audit_flags_probe_working_builder_legacy_variant_rows(self) -> None:
        payload = audit.working_builder_parity_payload(
            [
                {
                    "working_builder_parity_mode": "working_builder_parity",
                    "working_builder_request_built": True,
                    "buy_variant": "legacy_buy",
                    "working_builder_buy_variant": "legacy_buy",
                }
            ]
        )

        self.assertEqual(payload["probe_working_builder_legacy_variant_rows"], 1)
        self.assertEqual(payload["probe_working_builder_stale_route_diagnostics_rows"], 0)

    def test_audit_flags_probe_working_builder_selected_legacy_handoff_rows(self) -> None:
        payload = audit.working_builder_parity_payload(
            [
                {
                    "working_builder_parity_mode": "working_builder_parity",
                    "working_builder_request_built": True,
                    "buy_variant": "routed_exact_sol_in",
                    "working_builder_buy_variant": "routed_exact_sol_in",
                    "selected_route_kind": "legacy_buy",
                    "selected_route_source": "selected_fallback_route_execution_handoff",
                    "selected_route_handoff_status": "selected_route_handoff_mismatch",
                    "legacy_buy_curve_pubkey": "LegacyCurve11111111111111111111111111111111",
                }
            ]
        )

        self.assertEqual(payload["probe_working_builder_selected_legacy_handoff_rows"], 1)
        self.assertEqual(payload["probe_working_builder_stale_route_diagnostics_rows"], 1)

    def test_audit_flags_x5_account_source_readiness_blockers(self) -> None:
        payload = audit.working_builder_parity_payload(
            [
                {
                    "working_builder_parity_mode": "working_builder_parity",
                    "working_builder_request_built": True,
                    "working_builder_bcv2_pubkey": "bcv2-final",
                    "bonding_curve_v2_pubkey": "bcv2-stale",
                    "working_builder_bcv2_source_authority": "authoritative_observed_tx",
                    "working_builder_bcv2_rpc_load_status": "missing_on_rpc_precheck",
                    "working_builder_bcv2_rpc_load_ready": False,
                    "working_builder_bcv2_seen_in_observed_tx": True,
                    "working_builder_bcv2_seen_in_account_state": False,
                    "working_builder_creator_vault_source_authority": "authoritative_detected_pool_creator",
                    "working_builder_creator_vault_rpc_load_status": "missing_on_rpc_precheck",
                    "working_builder_creator_vault_rpc_load_ready": False,
                    "working_builder_missing_required_accounts": [
                        "bonding_curve_v2:bcv2-final:observed_tx_account_meta",
                        "creator_vault:creator-vault:route_builder",
                    ],
                }
            ]
        )

        self.assertEqual(payload["working_builder_bcv2_pubkey_mismatch_rows"], 1)
        self.assertEqual(
            payload["working_builder_bcv2_observed_tx_missing_on_rpc_rows"], 1
        )
        self.assertEqual(
            payload[
                "working_builder_creator_vault_authoritative_but_missing_on_rpc_rows"
            ],
            1,
        )
        self.assertEqual(
            payload[
                "working_builder_manifest_ready_after_account_source_repair_rows"
            ],
            0,
        )
        self.assertEqual(
            payload[
                "working_builder_manifest_still_not_ready_after_account_source_repair_rows"
            ],
            1,
        )

    def test_audit_flags_x6_bcv2_reconciliation_counters(self) -> None:
        payload = audit.working_builder_parity_payload(
            [
                {
                    "working_builder_parity_mode": "working_builder_parity",
                    "working_builder_request_built": True,
                    "working_builder_bcv2_pubkey": "bcv2-a",
                    "working_builder_bcv2_builder_pubkey": "bcv2-a",
                    "working_builder_bcv2_observed_pubkey": "bcv2-a",
                    "working_builder_bcv2_precheck_pubkey": "bcv2-a",
                    "working_builder_bcv2_pubkey_consistency_status": "builder_observed_precheck_match",
                    "working_builder_bcv2_source_authority": "authoritative_observed_tx",
                    "working_builder_bcv2_rpc_load_status": "missing_on_rpc_precheck",
                    "working_builder_bcv2_rpc_load_ready": False,
                    "working_builder_bcv2_seen_in_observed_tx": True,
                    "working_builder_bcv2_seen_in_account_state": False,
                    "working_builder_bcv2_observed_slot": 10,
                    "working_builder_bcv2_observed_tx_signature": "sig-a",
                    "working_builder_bcv2_precheck_context_slot": 11,
                    "working_builder_bcv2_precheck_commitment": "processed",
                    "working_builder_bcv2_precheck_attempt_count": 1,
                    "working_builder_bcv2_precheck_latency_ms": 9,
                    "working_builder_bcv2_precheck_age_from_observed_slot": 1,
                    "working_builder_bcv2_rpc_error_class": "account_missing",
                    "working_builder_bcv2_reconciliation_class": "commitment_or_timing_suspected",
                    "observed_bcv2_loaded_address_source": "resolved_transaction_account_keys",
                },
                {
                    "working_builder_parity_mode": "working_builder_parity",
                    "working_builder_request_built": True,
                    "working_builder_bcv2_pubkey": "bcv2-b",
                    "working_builder_bcv2_builder_pubkey": "bcv2-b",
                    "working_builder_bcv2_observed_pubkey": "bcv2-c",
                    "working_builder_bcv2_precheck_pubkey": "bcv2-b",
                    "working_builder_bcv2_pubkey_consistency_status": "observed_pubkey_mismatch",
                    "working_builder_bcv2_source_authority": "authoritative_observed_tx",
                    "working_builder_bcv2_rpc_load_status": "missing_on_rpc_precheck",
                    "working_builder_bcv2_rpc_load_ready": False,
                    "working_builder_bcv2_reconciliation_class": "pubkey_mismatch",
                },
            ]
        )

        self.assertEqual(
            payload["working_builder_bcv2_reconciliation_class_counts"],
            {"commitment_or_timing_suspected": 1, "pubkey_mismatch": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_pubkey_consistency_status_counts"],
            {
                "builder_observed_precheck_match": 1,
                "observed_pubkey_mismatch": 1,
            },
        )
        self.assertEqual(
            payload["working_builder_bcv2_precheck_age_bucket_counts"],
            {"1_2": 1, "missing": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_loaded_address_source_counts"],
            {"missing": 1, "resolved_transaction_account_keys": 1},
        )
        self.assertEqual(
            payload["working_builder_bcv2_loaded_address_source_missing_rows"], 1
        )
        self.assertEqual(payload["working_builder_bcv2_precheck_context_slot_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_precheck_attempt_count_rows"], 1)
        self.assertEqual(payload["working_builder_bcv2_precheck_latency_rows"], 1)

    def test_audit_blocks_pass_b_when_probe_variant_drift_present(self) -> None:
        report = {
            "probe_join_key_coverage": {
                "probe_selection_rows": 1,
                "probe_transport_rows": 1,
                "probe_entry_rows": 1,
                "probe_join_quality": "exact_probe_id_and_ab_record_id",
            },
            "probe_decision_join": {
                "decision_join_acceptance": "pass",
                "required_exact_decision_v3_join_coverage": 1.0,
            },
            "probe_artifact_intersections": {
                "ab_record_id": {"common_values": 1},
                "probe_id": {"common_values": 1},
            },
            "probe_entry_materialization": {
                "probe_working_builder_variant_drift_rows": 1,
                "probe_working_builder_legacy_variant_rows": 0,
                "probe_working_builder_selected_legacy_handoff_rows": 0,
                "probe_working_builder_stale_route_diagnostics_rows": 0,
            },
        }

        readiness = audit.probe_readiness(report)

        self.assertEqual(readiness["status"], "not_ready")
        self.assertIn(
            "probe_working_builder_variant_drift",
            readiness["reasons"],
        )

    def test_audit_allows_pass_b_only_when_legacy_rows_zero_and_remaining_blocker_is_account_source(
        self,
    ) -> None:
        report = {
            "probe_join_key_coverage": {
                "probe_selection_rows": 1,
                "probe_transport_rows": 1,
                "probe_entry_rows": 1,
                "probe_join_quality": "exact_probe_id_and_ab_record_id",
            },
            "probe_decision_join": {
                "decision_join_acceptance": "pass",
                "required_exact_decision_v3_join_coverage": 1.0,
            },
            "probe_artifact_intersections": {
                "ab_record_id": {"common_values": 1},
                "probe_id": {"common_values": 1},
            },
            "probe_entry_materialization": {
                "probe_working_builder_variant_drift_rows": 0,
                "probe_working_builder_legacy_variant_rows": 0,
                "probe_working_builder_selected_legacy_handoff_rows": 0,
                "probe_working_builder_stale_route_diagnostics_rows": 0,
                "working_builder_manifest_missing_required_rows": 1,
            },
        }

        readiness = audit.probe_readiness(report)

        self.assertEqual(readiness["status"], "ready_for_probe_transport_entry_join")
        self.assertNotIn(
            "probe_working_builder_legacy_variant",
            readiness["reasons"],
        )

    def test_e4r2_selected_fallback_handoff_violation_counters_are_reported(
        self,
    ) -> None:
        rows = [
            {
                "selected_route_kind": "legacy_buy",
                "route_resolution_status": "fallback_route_ready",
                "fallback_route_ready": True,
                "fallback_route_kind": "legacy_buy",
                "buy_variant": "routed_exact_sol_in",
                "selected_route_account_set_roles": [
                    "bonding_curve_v2:BCV2",
                    "bonding_curve:CURVE",
                ],
                "selected_route_handoff_status": "selected_route_handoff_applied",
                "selected_route_precheck_hash": "selected-precheck",
                "precheck_account_set_hash": "actual-precheck",
                "selected_route_simulation_hash": "selected-simulation",
                "simulation_account_set_hash": "actual-simulation",
            }
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(
            payload["legacy_buy_selected_but_request_variant_not_legacy_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_precheck_hash_mismatch_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_selected_but_simulation_hash_mismatch_rows"],
            1,
        )

    def test_e4r3_final_manifest_bcv2_and_no_executable_simulation_are_flagged(
        self,
    ) -> None:
        rows = [
            {
                "selected_route_kind": "legacy_buy",
                "selected_route_source": "selected_fallback_route_execution_handoff",
                "selected_route_handoff_status": "selected_route_handoff_mismatch",
                "selected_route_handoff_reason": (
                    "selected_legacy_buy_final_manifest_contains_primary_bcv2"
                ),
                "route_resolution_status": "no_executable_route_account_set",
                "fallback_route_kind": "legacy_buy",
                "buy_variant": "legacy_buy",
                "execution_outcome": "counterfactual_shadow_probe_simulation_error",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_source": "route_builder",
                "simulation_account_manifest": [
                    {
                        "role": "bonding_curve",
                        "pubkey": "CURVE",
                        "source": "materialized_feature_set",
                    },
                    {
                        "role": "bonding_curve_v2",
                        "pubkey": "BCV2",
                        "source": "route_builder",
                    },
                ],
            }
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["selected_legacy_handoff_claimed_rows"], 1)
        self.assertEqual(payload["selected_legacy_handoff_validated_rows"], 0)
        self.assertEqual(payload["selected_legacy_handoff_mismatch_rows"], 1)
        self.assertEqual(
            payload["selected_legacy_final_manifest_contains_bcv2_rows"],
            1,
        )
        self.assertEqual(
            payload[
                "selected_legacy_final_manifest_contains_primary_route_builder_rows"
            ],
            1,
        )
        self.assertEqual(payload["no_executable_route_but_simulated_rows"], 1)

    def test_e4r3_selected_route_handoff_mismatch_precheck_is_not_simulated(
        self,
    ) -> None:
        rows = [
            {
                "selected_route_kind": "legacy_buy",
                "selected_route_source": "selected_fallback_route_execution_handoff",
                "selected_route_handoff_status": "selected_route_handoff_mismatch",
                "selected_route_handoff_reason": (
                    "selected_legacy_buy_final_manifest_contains_primary_bcv2"
                ),
                "route_resolution_status": "no_executable_route_account_set",
                "fallback_route_kind": "legacy_buy",
                "buy_variant": "legacy_buy",
                "execution_outcome": "selected_route_handoff_mismatch",
                "precheck_failure_reason": (
                    "selected_route_handoff_mismatch:"
                    "selected_legacy_buy_final_manifest_contains_primary_bcv2"
                ),
                "simulation_account_manifest": [
                    {
                        "role": "bonding_curve_v2",
                        "pubkey": "BCV2",
                        "source": "route_builder",
                    }
                ],
            }
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["selected_legacy_handoff_claimed_rows"], 1)
        self.assertEqual(payload["selected_legacy_handoff_mismatch_rows"], 1)
        self.assertEqual(
            payload["selected_legacy_final_manifest_contains_bcv2_rows"],
            1,
        )
        self.assertEqual(payload["no_executable_route_but_simulated_rows"], 0)

    def test_e5b_legacy_buy_unsupported_builder_layout_counters_are_reported(
        self,
    ) -> None:
        rows = [
            {
                "fallback_route_kind": "legacy_buy",
                "fallback_route_attempted": False,
                "fallback_route_ready": False,
                "fallback_route_not_ready_reason": (
                    "unsupported_builder_layout_requires_bcv2"
                ),
                "fallback_failure_class": "fallback_unsupported_builder_layout",
                "route_resolution_status": "no_executable_route_account_set",
                "no_executable_route_account_set_reason": (
                    "unsupported_builder_layout_requires_bcv2:"
                    "bonding_curve_v2:BCV2"
                ),
                "legacy_buy_account_set_status": "ready",
                "legacy_buy_curve_authority_readiness_status": (
                    "authoritative_and_load_ready"
                ),
                "legacy_buy_route_ready": False,
                "legacy_buy_route_not_ready_reason": (
                    "legacy_buy_unsupported_builder_layout_requires_bcv2"
                ),
            }
        ]

        payload = audit.legacy_buy_route_payload(rows, [])

        self.assertEqual(payload["legacy_buy_route_attempted_rows"], 1)
        self.assertEqual(payload["legacy_buy_route_ready_rows"], 0)
        self.assertEqual(
            payload["legacy_buy_route_unsupported_builder_layout_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_excluded_from_execution_route_universe_rows"],
            1,
        )
        self.assertEqual(
            payload["legacy_buy_removed_from_fallback_candidates_rows"],
            1,
        )
        self.assertEqual(payload["legacy_buy_fallback_account_set_ready_rows"], 0)
        self.assertEqual(
            payload["legacy_buy_route_not_ready_reason_counts"],
            {"legacy_buy_unsupported_builder_layout_requires_bcv2": 1},
        )

        fallback = audit.fallback_decision_payload(rows)

        self.assertEqual(
            fallback["fallback_failure_class_counts"],
            {"fallback_unsupported_builder_layout": 1},
        )
        self.assertFalse(fallback["fallback_repairable"])
        self.assertEqual(
            fallback["recommended_next_path"],
            "route_class_exclusion_from_execution_label_universe",
        )

    def test_active_shadow_unattributed_account_not_found_blocks_readiness(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "configs/rollout/r16.toml"
            config.parent.mkdir(parents=True)
            config.write_text(
                """
[oracle]
decision_log_path = "../../logs/rollout/r16/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/r16/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/r16/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/r16/shadow_lifecycle.jsonl"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            common = {
                "candidate_id": "pool_mint_1000",
                "ab_record_id": "ab-buy",
                "pool_id": "pool",
                "base_mint": "mint",
                "decision_ts_ms": 1000,
                "v3_replay_payload_schema_version": 1,
            }
            failure = {
                **common,
                "dispatch_status": "failed",
                "simulation_outcome": "failed",
                "err": "shadow RPC simulate failed: AccountNotFound",
                "simulation_error_kind": "AccountNotFound",
                "simulation_error_category": "simulation_account_not_found_unattributed",
                "active_shadow_lifecycle_eligibility_status": "lifecycle_eligible",
            }
            write_jsonl(
                root / "logs/rollout/r16/decisions/gatekeeper_v2_decisions.jsonl",
                [common],
            )
            write_jsonl(root / "logs/shadow_run/r16/buys.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_entries.jsonl", [failure])
            write_jsonl(root / "logs/shadow_run/r16/shadow_lifecycle.jsonl", [failure])

            report = audit.build_report(config)

        active = report["active_shadow_dispatch_diagnostics"]
        self.assertEqual(active["active_shadow_account_not_found_unattributed_rows"], 3)
        self.assertEqual(active["active_shadow_lifecycle_eligible_failure_rows"], 3)
        self.assertIn(
            "active_shadow_unattributed_account_not_found",
            report["readiness"]["reasons"],
        )
        self.assertIn(
            "active_shadow_dispatch_failure_marked_lifecycle_eligible",
            report["readiness"]["reasons"],
        )


if __name__ == "__main__":
    unittest.main()
