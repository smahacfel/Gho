import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from v3_p37_probe_account_reconciliation_report import (
    choose_next_fix,
    classify_reconciliation,
    mfs_route_account_summary,
)


class ProbeAccountReconciliationReportTests(unittest.TestCase):
    def test_missing_bonding_curve_seen_in_diag_before_request_build_routes_to_override_fix(self):
        classification, detail, next_fix = classify_reconciliation(
            {
                "precheck_failure_reason": "missing_bonding_curve",
                "diag_seen": True,
                "prepared_request_status": "not_built_pre_route_precheck",
                "rpc_precheck_status": "not_run_prepared_request_not_built",
                "mfs": {"mfs_expected_pubkey_present_as_value": False},
            }
        )

        self.assertEqual(classification, "mfs_has_account_but_overrides_missing")
        self.assertEqual(
            detail,
            "diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override",
        )
        self.assertEqual(next_fix, "route_override_propagation")

    def test_missing_execution_route_identity_classifies_as_route_mismatch(self):
        classification, detail, next_fix = classify_reconciliation(
            {
                "precheck_failure_reason": "missing_execution_route_identity",
                "diag_seen": False,
                "prepared_request_status": "not_built_pre_route_precheck",
                "rpc_precheck_status": "not_run_prepared_request_not_built",
                "mfs": {"mfs_expected_pubkey_present_as_value": False},
            }
        )

        self.assertEqual(classification, "route_mismatch")
        self.assertEqual(detail, "buy_variant_or_route_identity_missing_before_request_build")
        self.assertEqual(next_fix, "route_identity_propagation")

    def test_explicit_missing_required_account_seen_in_diag_classifies_as_rpc_visibility_gap(self):
        classification, detail, next_fix = classify_reconciliation(
            {
                "precheck_failure_reason": "missing_required_account:bonding_curve_v2:abc",
                "diag_seen": True,
                "prepared_request_status": "transport_recorded",
                "rpc_precheck_status": "rpc_processed_missing",
                "mfs": {"mfs_expected_pubkey_present_as_value": False},
            }
        )

        self.assertEqual(classification, "diag_seen_rpc_missing")
        self.assertEqual(
            detail,
            "local_diag_observed_account_but_rpc_processed_precheck_missing",
        )
        self.assertEqual(next_fix, "rpc_visibility_reconciliation")

    def test_mfs_summary_reports_expected_pubkey_presence_and_route_fields(self):
        summary = mfs_route_account_summary(
            {
                "v3_materialized_feature_snapshot": {
                    "route_kind": "legacy_buy",
                    "account_features": {
                        "update_count": 2,
                        "curve_finality": "processed",
                    },
                    "curve_readiness": {
                        "is_ready": True,
                        "curve_data_known": True,
                    },
                    "evidence_status": {
                        "account_state": {"status": "clean"},
                        "curve": {"status": "clean"},
                    },
                    "nested": {"bonding_curve": "curve-123"},
                }
            },
            "curve-123",
        )

        self.assertTrue(summary["mfs_present"])
        self.assertTrue(summary["mfs_expected_pubkey_present_as_value"])
        self.assertTrue(summary["mfs_has_bonding_curve_field"])
        self.assertTrue(summary["mfs_has_route_kind_field"])
        self.assertEqual(summary["mfs_account_features_update_count"], 2)
        self.assertTrue(summary["mfs_curve_readiness_is_ready"])
        self.assertEqual(summary["mfs_evidence_account_state_status"], "clean")

    def test_choose_next_fix_selects_dominant_path(self):
        self.assertEqual(
            choose_next_fix(
                [
                    {"recommended_fix_path": "route_identity_propagation"},
                    {"recommended_fix_path": "route_override_propagation"},
                    {"recommended_fix_path": "route_override_propagation"},
                ]
            ),
            "route_override_propagation",
        )

    def test_choose_next_fix_defaults_to_manual_investigation_for_empty_records(self):
        self.assertEqual(choose_next_fix([]), "manual_investigation")


if __name__ == "__main__":
    unittest.main()
