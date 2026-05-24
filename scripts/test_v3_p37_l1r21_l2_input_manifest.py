import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l1r20_l2_executable_subset_preflight as l1r20
import v3_p37_l1r21_l2_input_manifest as l1r21


def preflight_report(**overrides: int) -> dict:
    denominator = overrides.get("buy_quality_denominator_rows", 85)
    dirty_good = overrides.get("buy_quality_dirty_good", 4)
    return {
        "preflight_status": "pass",
        "final_decision": "GO_L2_EXECUTABLE_SUBSET_LOCKED",
        "blockers": [],
        "requested_l2_namespaces": list(l1r20.DEFAULT_ALLOWED_L2_NAMESPACES),
        "missing_requested_l2_namespaces": [],
        "blocked_requested_l2_namespaces": [],
        "disallowed_requested_l2_namespaces": [],
        "unusable_requested_l2_namespaces": [],
        "input_totals": {
            "total_rows": 4322,
            "executable_eligible_rows": overrides.get("executable_eligible_rows", 87),
            "buy_quality_denominator_rows": denominator,
            "buy_quality_dirty_good": dirty_good,
            "dirty_good_rate": dirty_good / denominator if denominator else None,
        },
        "excluded_totals": {
            "excluded_non_executable_rows": overrides.get("excluded_non_executable_rows", 3956),
            "excluded_unsupported_route_rows": overrides.get("excluded_unsupported_route_rows", 11),
        },
    }


class P37L1R21L2InputManifestTests(unittest.TestCase):
    def test_contract_passes_for_locked_expected_counts(self) -> None:
        failures = l1r21.validate_contract(preflight_report(), l1r21.default_expected_contract())

        self.assertEqual(failures, [])

    def test_denominator_mismatch_fails_contract(self) -> None:
        failures = l1r21.validate_contract(
            preflight_report(buy_quality_denominator_rows=84),
            l1r21.default_expected_contract(),
        )

        self.assertIn("buy_quality_denominator_rows_mismatch", failures)

    def test_unknown_namespace_fails_contract(self) -> None:
        report = preflight_report()
        report["requested_l2_namespaces"] = ["unexpected-run"]

        failures = l1r21.validate_contract(report, l1r21.default_expected_contract())

        self.assertIn("allowed_l2_run_set_mismatch", failures)

    def test_file_manifest_records_hash_and_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "labels.jsonl"
            path.write_text('{"a": 1}\n{"a": 2}\n', encoding="utf-8")

            manifest = l1r21.file_manifest(path, role="lifecycle_label_file")

        self.assertTrue(manifest["exists"])
        self.assertEqual(manifest["jsonl_rows"], 2)
        self.assertRegex(manifest["sha256"], r"^[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
