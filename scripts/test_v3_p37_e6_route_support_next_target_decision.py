import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_e6_route_support_next_target_decision as e6


E5A_BCV2 = "## Verdict\n\n`BUILDER_LEGACY_LAYOUT_USES_BCV2`\n"
E5B_CLOSED = "## Verdict\n\n`LEGACY_BUY_UNSUPPORTED_REMOVED_FROM_FALLBACK`\n"


def route(
    *,
    observed_count: int,
    account_count: int,
    map_coverage: int,
    roles: dict[str, int] | None = None,
    ready_true: int = 0,
    ready_false: int = 0,
    shadow_status: str = "not_executable_no_executable_route_account_set",
    builder_status: str = "supported_by_current_builder",
) -> dict:
    return {
        "observed_count": observed_count,
        "account_count_values": {str(account_count): 1},
        "account_index_to_role_map_coverage": map_coverage,
        "account_role_counts": roles or {},
        "builder_support_status": builder_status,
        "prepared_request_support_status": "prepared_request_manifest_available",
        "rpc_load_ready_true": ready_true,
        "rpc_load_ready_false": ready_false,
        "rpc_load_readiness_rate": (
            ready_true / (ready_true + ready_false)
            if ready_true + ready_false
            else None
        ),
        "shadow_simulation_support_status": shadow_status,
        "primary_failure_class": "none",
    }


class RouteSupportNextTargetDecisionTests(unittest.TestCase):
    def test_e5b_closes_legacy_and_blocks_current_e1_universe(self) -> None:
        report = e6.build_report(
            {
                "final_decision": "GO_E2_IMPLEMENT_TOP_ROUTE_SUPPORT",
                "recommended_next_path": "implement_legacy_buy_executable_account_set_materialization",
                "routes": {
                    "legacy_buy": route(
                        observed_count=5,
                        account_count=21,
                        map_coverage=1,
                        roles={"bonding_curve_v2": 6},
                        ready_false=10,
                    ),
                    "routed_exact_sol_in": route(
                        observed_count=5,
                        account_count=21,
                        map_coverage=14,
                        roles={"bonding_curve_v2": 2},
                        ready_false=7,
                    ),
                },
            },
            E5A_BCV2,
            E5B_CLOSED,
        )

        self.assertEqual(report["final_decision"], "BLOCK_ROUTE_SUPPORT_ABI_DISCOVERY_REQUIRED")
        self.assertEqual(report["recommended_next_route_to_implement"], "no_implementable_route_found")
        self.assertEqual(report["summary"]["closed_route_classes"], ["legacy_buy"])
        self.assertEqual(report["summary"]["supported_executable_route_classes"], [])
        self.assertEqual(
            report["routes"]["legacy_buy"]["route_closure_status"],
            "closed_unsupported_builder_layout_requires_bcv2",
        )

    def test_clean_complete_route_selects_e7_target(self) -> None:
        report = e6.build_report(
            {
                "routes": {
                    "clean_buy": route(
                        observed_count=9,
                        account_count=4,
                        map_coverage=4,
                        ready_true=2,
                        shadow_status="not_executable_no_executable_route_account_set",
                    )
                }
            },
            "",
            "",
        )

        self.assertEqual(report["final_decision"], "GO_E7_IMPLEMENT_NEXT_ROUTE_CLASS")
        self.assertEqual(report["recommended_next_route_to_implement"], "clean_buy")

    def test_existing_executable_route_scopes_execution_universe(self) -> None:
        report = e6.build_report(
            {
                "routes": {
                    "supported_buy": route(
                        observed_count=3,
                        account_count=5,
                        map_coverage=5,
                        ready_true=3,
                        shadow_status="executable",
                    )
                }
            },
            "",
            "",
        )

        self.assertEqual(report["final_decision"], "GO_SCOPE_RESTRICT_TO_SUPPORTED_ROUTE_CLASSES")
        self.assertEqual(report["summary"]["supported_executable_route_classes"], ["supported_buy"])

    def test_no_routes_blocks_for_abi_discovery(self) -> None:
        report = e6.build_report({"routes": {}}, "", "")

        self.assertEqual(report["final_decision"], "BLOCK_ROUTE_SUPPORT_ABI_DISCOVERY_REQUIRED")
        self.assertEqual(report["recommended_next_path"], "route_artifact_gap")


if __name__ == "__main__":
    unittest.main()
