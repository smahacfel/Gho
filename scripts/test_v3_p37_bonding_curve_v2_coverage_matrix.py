#!/usr/bin/env python3
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import v3_p37_bonding_curve_v2_coverage_matrix as matrix


class BondingCurveV2CoverageMatrixTest(unittest.TestCase):
    def test_missing_on_rpc_routes_to_builder_repair(self) -> None:
        row = {
            "account_index": 16,
            "builder_bonding_curve_v2_pubkey": "bcv2",
            "diag": {
                "diag_seen_exact_pubkey": False,
                "diag_seen_other_curve_pubkey_for_mint": True,
            },
            "mfs": {
                "mfs_contains_bonding_curve_v2_key": False,
                "mfs_contains_builder_bcv2_pubkey": False,
            },
            "rpc_current": {"rpc_current_status": "missing"},
        }

        classification, reasons = matrix.classify_matrix_row(row)

        self.assertEqual(classification, "builder_bcv2_missing_on_rpc")
        self.assertIn("builder_bcv2_missing_on_rpc", reasons)
        self.assertEqual(
            matrix.route_decision(
                [{**row, "matrix_classification": classification}],
                rpc_checked=True,
            ),
            "route_builder_source_repair_or_route_fallback",
        )

    def test_rpc_present_but_not_materialized_routes_to_mfs_readiness(self) -> None:
        row = {
            "account_index": 16,
            "builder_bonding_curve_v2_pubkey": "bcv2",
            "diag": {
                "diag_seen_exact_pubkey": False,
                "diag_seen_other_curve_pubkey_for_mint": True,
            },
            "mfs": {
                "mfs_contains_bonding_curve_v2_key": False,
                "mfs_contains_builder_bcv2_pubkey": False,
            },
            "rpc_current": {
                "rpc_current_status": "present",
                "rpc_current_owner": "pumpfun",
                "rpc_current_data_len": 128,
            },
        }

        classification, reasons = matrix.classify_matrix_row(row)

        self.assertEqual(classification, "builder_bcv2_not_materialized_but_rpc_exists")
        self.assertIn("builder_bcv2_exists_on_rpc", reasons)
        self.assertEqual(
            matrix.route_decision(
                [{**row, "matrix_classification": classification}],
                rpc_checked=True,
            ),
            "rpc_readiness_source_and_mfs_materialization",
        )

    def test_tx_meta_account_16_mismatch_blocks_handoff(self) -> None:
        row = {
            "account_index": 16,
            "builder_bonding_curve_v2_pubkey": "builder-bcv2",
            "diag": {},
            "mfs": {},
            "rpc_current": {"rpc_current_status": "present"},
        }
        # Force the helper-visible tx meta mismatch without depending on a full
        # artifact row by monkeypatching the field after classification setup.
        original = matrix.tx_meta_account_16_pubkey
        try:
            matrix.tx_meta_account_16_pubkey = lambda _: "tx-meta-bcv2"  # type: ignore[assignment]
            classification, reasons = matrix.classify_matrix_row(row)
        finally:
            matrix.tx_meta_account_16_pubkey = original  # type: ignore[assignment]

        self.assertEqual(classification, "tx_meta_builder_bcv2_mismatch")
        self.assertIn("tx_meta_account_16_differs_from_builder_bcv2", reasons)


if __name__ == "__main__":
    unittest.main()
