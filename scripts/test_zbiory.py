#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import zbiory


class ZbioryScriptTests(unittest.TestCase):
    def test_merges_matching_records_and_splits_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp = Path(tmp_dir)
            (tmp / "shadow_lifecycle.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"mint_id": "mint-a", "final_pnl_pct": 31, "lane": "shadow"}),
                        json.dumps({"mint_id": "mint-mid", "final_pnl_pct": 5, "lane": "shadow"}),
                        json.dumps({"mint_id": "mint-dup", "final_pnl_pct": 11, "lane": "shadow"}),
                        json.dumps({"mint_id": "mint-no-pnl", "lane": "shadow"}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            (tmp / "probe_shadow_lifecycle.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"mint_id": "mint-b", "final_pnl_pct": -35, "lane": "probe"}),
                        json.dumps({"mint_id": "mint-dup", "final_pnl_pct": 99, "lane": "probe"}),
                        json.dumps({"mint_id": "mint-unmatched", "final_pnl_pct": 44, "lane": "probe"}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            (tmp / "gatekeeper_v2_decisions.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps({"base_mint": "mint-a", "decision_id": "dec-a"}),
                        json.dumps({"base_mint": "mint-b", "decision_id": "dec-b"}),
                        json.dumps({"base_mint": "mint-mid", "decision_id": "dec-mid"}),
                        json.dumps({"base_mint": "mint-dup", "decision_id": "dec-dup"}),
                        json.dumps({"base_mint": "mint-other", "decision_id": "dec-other"}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            exit_code = zbiory.main(["30", "-30", "--directory", str(tmp)])

            self.assertEqual(exit_code, 0)

            zbior_a = [json.loads(line) for line in (tmp / "zbior_A.jsonl").read_text(encoding="utf-8").splitlines()]
            zbior_b = [json.loads(line) for line in (tmp / "zbior_B.jsonl").read_text(encoding="utf-8").splitlines()]
            zbior_n = [json.loads(line) for line in (tmp / "zbior_N.jsonl").read_text(encoding="utf-8").splitlines()]

            self.assertEqual({record["mint_id"] for record in zbior_a}, {"mint-a"})
            self.assertEqual({record["base_mint"] for record in zbior_b}, {"mint-b"})
            self.assertEqual({record["mint_id"] for record in zbior_n}, {"mint-mid", "mint-dup"})

            dup_record = next(record for record in zbior_n if record["mint_id"] == "mint-dup")
            self.assertEqual(dup_record["final_pnl_pct"], 11)
            self.assertEqual(dup_record["lane"], "shadow")
            self.assertEqual(dup_record["decision_id"], "dec-dup")
            self.assertEqual(dup_record["_lifecycle_source_file"], "shadow_lifecycle.jsonl")
            self.assertEqual(dup_record["_decision_source_file"], "gatekeeper_v2_decisions.jsonl")

            all_output_mints = {
                record["mint_id"]
                for record in zbior_a + zbior_b + zbior_n
            }
            self.assertNotIn("mint-unmatched", all_output_mints)
            self.assertNotIn("mint-no-pnl", all_output_mints)


if __name__ == "__main__":
    unittest.main()
