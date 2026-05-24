import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_e1_pumpfun_route_support_matrix as e1


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def build_report(rows: list[dict]) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_jsonl(root / "buys.jsonl", rows)
        return e1.build_report(root, None)


class PumpfunRouteSupportMatrixTests(unittest.TestCase):
    def test_primary_ready_route_scopes_next_runtime(self) -> None:
        report = build_report([
            {
                "ab_record_id": "ab1",
                "primary_route_kind": "routed_exact_sol_in",
                "primary_route_ready": True,
                "selected_route_kind": "routed_exact_sol_in",
                "route_resolution_status": "primary_route_ready",
            }
        ])

        self.assertEqual(report["final_decision"], "GO_R18_EXECUTABLE_ROUTE_SCOPED_RUN")
        self.assertEqual(
            report["summary"]["simulation_support_by_route"],
            {"routed_exact_sol_in": "executable"},
        )

    def test_legacy_fallback_core_curve_gap_recommends_e2_route_support(self) -> None:
        report = build_report([
            {
                "ab_record_id": "ab1",
                "primary_route_kind": "routed_exact_sol_in",
                "primary_route_ready": False,
                "primary_route_not_ready_reason": "bonding_curve_v2_observed_meta_missing_on_rpc",
                "fallback_route_kind": "legacy_buy",
                "fallback_route_attempted": True,
                "fallback_route_ready": False,
                "fallback_route_not_ready_reason": "fallback_route_missing_legacy_buy_curve",
                "fallback_failure_class": "fallback_missing_core_curve_account",
                "fallback_missing_roles": ["bonding_curve"],
                "fallback_account_sources": ["legacy_buy_curve"],
                "observed_bcv2_source_buy_variant": "legacy_buy",
                "observed_bcv2_provenance_status": "route_compatible",
                "observed_bcv2_source_program_id": "pump",
                "observed_bcv2_source_discriminator": "66063d1201daebea",
                "observed_bcv2_instruction_account_position": 16,
                "observed_bcv2_message_account_index": 16,
                "observed_bcv2_loaded_address_source": "resolved_transaction_account_keys",
                "bonding_curve_v2_rpc_load_ready": False,
                "bonding_curve_v2_rpc_load_status": "missing_on_rpc_precheck",
                "route_resolution_status": "no_executable_route_account_set",
            }
        ])

        self.assertEqual(report["final_decision"], "GO_E2_IMPLEMENT_TOP_ROUTE_SUPPORT")
        self.assertEqual(
            report["summary"]["recommended_next_route_to_implement"],
            "legacy_buy_executable_account_set_materialization",
        )
        self.assertIn("legacy_buy", report["summary"]["route_classes_excluded_from_l2"])

    def test_manifest_account_role_map_is_extracted(self) -> None:
        report = build_report([
            {
                "ab_record_id": "ab1",
                "primary_route_kind": "routed_exact_sol_in",
                "primary_route_ready": False,
                "simulation_account_manifest": [
                    {
                        "pubkey": "payer",
                        "role": "payer_pubkey",
                        "source": "payer",
                        "instruction_index": 2,
                        "account_index": 0,
                        "route_kind": "routed_exact_sol_in",
                        "buy_variant": "routed_exact_sol_in",
                    },
                    {
                        "pubkey": "bcv2",
                        "role": "bonding_curve_v2",
                        "source": "observed_tx_account_meta",
                        "instruction_index": 3,
                        "account_index": 16,
                        "route_kind": "routed_exact_sol_in",
                        "buy_variant": "routed_exact_sol_in",
                    },
                ],
            }
        ])

        route = report["routes"]["routed_exact_sol_in"]
        self.assertEqual(route["account_index_to_role_map_coverage"], 2)
        self.assertEqual(
            route["account_index_to_role_map"]["16"],
            {"bonding_curve_v2:observed_tx_account_meta": 1},
        )

    def test_only_routed_bcv2_missing_blocks_policy_calibration(self) -> None:
        report = build_report([
            {
                "ab_record_id": "ab1",
                "primary_route_kind": "routed_exact_sol_in",
                "primary_route_ready": False,
                "primary_route_not_ready_reason": "bonding_curve_v2_identity_authoritative_but_not_load_ready",
                "route_resolution_status": "no_executable_route_account_set",
            }
        ])

        self.assertEqual(report["final_decision"], "BLOCK_POLICY_CALIBRATION_ROUTE_SUPPORT_REQUIRED")
        self.assertEqual(
            report["summary"]["recommended_next_route_to_implement"],
            "no_implementable_route_found",
        )

    def test_no_route_evidence_is_audit_gap(self) -> None:
        report = build_report([{"ab_record_id": "ab1", "pool_id": "pool"}])

        self.assertEqual(report["final_decision"], "BLOCK_E1_AUDIT_GAP")
        self.assertEqual(report["summary"]["recommended_next_route_to_implement"], "unknown")


if __name__ == "__main__":
    unittest.main()
