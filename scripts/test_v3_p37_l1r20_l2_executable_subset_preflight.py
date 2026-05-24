import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l1r20_l2_executable_subset_preflight as l1r20


ALLOWED_A = "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1"
ALLOWED_B = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1"
BLOCKED = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver"


def run(namespace: str, **values: int) -> dict:
    return {
        "namespace": namespace,
        "decision_rows_total": values.get("decision_rows_total", 10),
        "route_executable_rows": values.get("route_executable_rows", 0),
        "route_non_executable_rows": values.get("route_non_executable_rows", 0),
        "execution_feasibility_reject_rows": values.get("execution_feasibility_reject_rows", 0),
        "lifecycle_labeled_rows": values.get("lifecycle_labeled_rows", 0),
        "buy_quality_denominator_rows": values.get("buy_quality_denominator_rows", 0),
        "buy_quality_bad": values.get("buy_quality_bad", 0),
        "buy_quality_dirty_good": values.get("buy_quality_dirty_good", 0),
        "buy_quality_good": values.get("buy_quality_good", 0),
        "buy_quality_not_executable": values.get("buy_quality_not_executable", 0),
        "feature_join_executable_labeled_rows": values.get("feature_join_executable_labeled_rows", 0),
    }


def discovery(rows: list[dict]) -> dict:
    return {"final_decision": "GO_L2_EXECUTABLE_SUBSET", "runs": rows}


class P37L1R20L2ExecutableSubsetPreflightTests(unittest.TestCase):
    def test_default_allowed_subset_passes_and_reports_dirty_good_rate(self) -> None:
        report = l1r20.build_preflight_report_from_discovery(
            discovery(
                [
                    run(ALLOWED_A, route_executable_rows=44, lifecycle_labeled_rows=42, buy_quality_denominator_rows=42, buy_quality_bad=42),
                    run(ALLOWED_B, route_executable_rows=43, lifecycle_labeled_rows=43, buy_quality_denominator_rows=43, buy_quality_bad=39, buy_quality_dirty_good=4),
                    run(BLOCKED, route_non_executable_rows=63, execution_feasibility_reject_rows=11),
                ]
            )
        )

        self.assertEqual(report["preflight_status"], "pass")
        self.assertEqual(report["final_decision"], "GO_L2_EXECUTABLE_SUBSET_LOCKED")
        self.assertEqual(report["input_totals"]["buy_quality_denominator_rows"], 85)
        self.assertEqual(report["input_totals"]["buy_quality_dirty_good"], 4)
        self.assertAlmostEqual(report["input_totals"]["dirty_good_rate"], 4 / 85)
        self.assertEqual(report["excluded_totals"]["excluded_unsupported_route_rows"], 11)

    def test_hard_blocked_namespace_as_l2_input_fails_closed(self) -> None:
        report = l1r20.build_preflight_report_from_discovery(
            discovery(
                [
                    run(ALLOWED_A, route_executable_rows=1, lifecycle_labeled_rows=1, buy_quality_denominator_rows=1),
                    run(BLOCKED, route_non_executable_rows=63, execution_feasibility_reject_rows=11),
                ]
            ),
            l2_input_namespaces=[ALLOWED_A, BLOCKED],
        )

        self.assertEqual(report["preflight_status"], "fail")
        self.assertEqual(report["final_decision"], "BLOCK_L2_INPUT_UNIVERSE_CONTRACT")
        self.assertIn("requested_l2_namespace_is_hard_blocked", report["blockers"])
        self.assertEqual(report["blocked_requested_l2_namespaces"], [BLOCKED])

    def test_namespace_without_buy_quality_denominator_fails_closed(self) -> None:
        no_denominator = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2"
        report = l1r20.build_preflight_report_from_discovery(
            discovery([run(no_denominator, route_executable_rows=2)]),
            l2_input_namespaces=[no_denominator],
        )

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn("requested_l2_namespace_has_no_buy_quality_denominator", report["blockers"])
        self.assertIn("l2_buy_quality_denominator_empty", report["blockers"])

    def test_explicit_override_allows_blocked_namespace_but_marks_override(self) -> None:
        report = l1r20.build_preflight_report_from_discovery(
            discovery(
                [
                    run(BLOCKED, route_non_executable_rows=63, execution_feasibility_reject_rows=11, buy_quality_denominator_rows=1),
                ]
            ),
            l2_input_namespaces=[BLOCKED],
            allow_unsupported_override=True,
        )

        self.assertEqual(report["preflight_status"], "pass")
        self.assertTrue(report["override_used"])
        self.assertEqual(report["input_totals"]["buy_quality_denominator_rows"], 1)


if __name__ == "__main__":
    unittest.main()
