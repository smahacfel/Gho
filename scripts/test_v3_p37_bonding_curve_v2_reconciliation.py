#!/usr/bin/env python3
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import v3_p37_bonding_curve_v2_reconciliation as recon


class BondingCurveV2ReconciliationTest(unittest.TestCase):
    def test_diag_exact_pubkey_classification(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            log_path = Path(tmp) / "tmux_launcher.log"
            log_path.write_text(
                "2026-05-22T00:00:01.000Z INFO DIAG_ACCOUNT_UPDATE_RELAY "
                "base_mint=mint1 bonding_curve=bcv2 slot=7 sol_reserves=1\n",
                encoding="utf-8",
            )
            diag_index = recon.build_diag_index([log_path])
            case = {
                "base_mint": "mint1",
                "builder_bonding_curve_v2_pubkey": "bcv2",
                "decision_ts_ms": 1779408002000,
            }
            diag = recon.analyze_diag_for_case(case, diag_index, [log_path])
            classification, reasons = recon.classify_case(
                case,
                diag,
                {
                    "mfs_present": True,
                    "mfs_contains_bonding_curve_v2_key": True,
                    "mfs_contains_builder_bcv2_pubkey": True,
                },
                {},
            )
            self.assertTrue(diag["diag_seen_exact_pubkey"])
            self.assertEqual(classification, "diag_seen_exact_pubkey")
            self.assertNotIn("builder_bcv2_pubkey_not_seen_in_diag", reasons)

    def test_other_curve_for_same_mint_classifies_builder_pubkey_not_seen(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            log_path = Path(tmp) / "tmux_launcher.log"
            log_path.write_text(
                "2026-05-22T00:00:01.000Z INFO DIAG_ACCOUNT_UPDATE_RELAY "
                "base_mint=mint1 bonding_curve=legacy_curve slot=7 sol_reserves=1\n",
                encoding="utf-8",
            )
            diag_index = recon.build_diag_index([log_path])
            case = {
                "base_mint": "mint1",
                "builder_bonding_curve_v2_pubkey": "bcv2",
                "decision_ts_ms": 1779408002000,
            }
            diag = recon.analyze_diag_for_case(case, diag_index, [log_path])
            classification, reasons = recon.classify_case(
                case,
                diag,
                {
                    "mfs_present": True,
                    "mfs_contains_bonding_curve_v2_key": False,
                    "mfs_contains_builder_bcv2_pubkey": False,
                },
                {},
            )
            self.assertFalse(diag["diag_seen_exact_pubkey"])
            self.assertTrue(diag["diag_seen_other_curve_pubkey_for_mint"])
            self.assertEqual(classification, "builder_pubkey_not_seen_in_diag")
            self.assertIn("builder_bcv2_pubkey_not_seen_in_diag", reasons)
            self.assertIn("diag_seen_other_curve_pubkey_for_same_mint", reasons)

    def test_collect_cases_deduplicates_active_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            row = {
                "ab_record_id": "ab1",
                "base_mint": "mint1",
                "pool_amm_id": "pool1",
                "precheck_failure_reason": "execution_account_not_ready:bonding_curve_v2:bcv2",
                "simulation_error_account_role": "bonding_curve_v2",
                "simulation_error_account_pubkey": "bcv2",
            }
            for name in ("buys.jsonl", "shadow_entries.jsonl", "shadow_lifecycle.jsonl"):
                (root / name).write_text(json.dumps(row) + "\n", encoding="utf-8")
            (root / "probe_skips.jsonl").write_text("", encoding="utf-8")
            cases = recon.collect_cases(
                {
                    "buys": root / "buys.jsonl",
                    "entries": root / "shadow_entries.jsonl",
                    "lifecycle": root / "shadow_lifecycle.jsonl",
                    "probe_skips": root / "probe_skips.jsonl",
                }
            )
            self.assertEqual(len(cases), 1)
            self.assertEqual(cases[0]["plane"], "active_shadow")
            self.assertEqual(cases[0]["artifact_row_count"], 3)
            self.assertEqual(
                sorted(cases[0]["artifact_sources"]),
                ["buys", "entry", "lifecycle"],
            )


if __name__ == "__main__":
    unittest.main()
