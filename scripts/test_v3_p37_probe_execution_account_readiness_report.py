import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from v3_p37_probe_execution_account_readiness_report import (
    classify_missing_account,
    parse_missing_required_account,
    recursive_contains_key,
    recursive_contains_value,
)


class ProbeExecutionAccountReadinessReportTests(unittest.TestCase):
    def test_parse_missing_required_account(self):
        self.assertEqual(
            parse_missing_required_account(
                "missing_required_account:bonding_curve_v2:abc"
            ),
            ("bonding_curve_v2", "abc"),
        )
        self.assertEqual(
            parse_missing_required_account(
                "execution_account_not_ready:creator_vault:def"
            ),
            ("creator_vault", "def"),
        )
        self.assertEqual(parse_missing_required_account(None), (None, None))
        self.assertEqual(parse_missing_required_account("other"), (None, None))

    def test_recursive_helpers(self):
        payload = {"a": [{"creator_vault": "x"}, {"b": "target"}]}
        self.assertTrue(recursive_contains_key(payload, "creator_vault"))
        self.assertTrue(recursive_contains_value(payload, "target"))
        self.assertFalse(recursive_contains_key(payload, "bonding_curve_v2"))

    def test_builder_derived_missing_account_classifies_as_rpc_missing(self):
        decision_row = {
            "v3_materialized_feature_snapshot": {
                "account_features": {"update_count": 3},
                "curve_readiness": {"curve_data_known": True},
            }
        }
        classification, reasons, basis = classify_missing_account(
            "bonding_curve_v2",
            "missing_pubkey",
            decision_row,
            {"diag_account_update_occurrences": 0},
        )
        self.assertEqual(classification, "override_present_but_account_missing_on_rpc")
        self.assertIn("not_materialized_in_v3_mfs:bonding_curve_v2", reasons)
        self.assertIn("precheck/RPC", basis)

    def test_execution_account_not_ready_reason_classifies_as_not_ready(self):
        classification, reasons, basis = classify_missing_account(
            "creator_vault",
            "missing_pubkey",
            {"v3_materialized_feature_snapshot": {"account_features": {"update_count": 1}}},
            {"diag_account_update_occurrences": 0},
            "execution_account_not_ready:creator_vault:missing_pubkey",
        )
        self.assertEqual(classification, "execution_account_not_ready")
        self.assertIn("not_materialized_in_v3_mfs:creator_vault", reasons)
        self.assertIn("unavailable before probe dispatch", basis)

    def test_route_identity_precheck_reasons_are_classified(self):
        classification, reasons, basis = classify_missing_account(
            None,
            None,
            {},
            {"diag_account_update_occurrences": 0},
            "missing_execution_route_identity",
        )

        self.assertEqual(classification, "missing_execution_route_identity")
        self.assertEqual(reasons, ["missing_execution_route_identity"])
        self.assertIn("buy route identity", basis)

    def test_routed_associated_curve_precheck_reason_is_classified(self):
        classification, reasons, basis = classify_missing_account(
            None,
            None,
            {},
            {"diag_account_update_occurrences": 0},
            "missing_routed_associated_bonding_curve",
        )

        self.assertEqual(classification, "missing_routed_associated_bonding_curve")
        self.assertEqual(reasons, ["missing_routed_associated_bonding_curve"])
        self.assertIn("associated bonding curve", basis)


if __name__ == "__main__":
    unittest.main()
