#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gatekeeper_metric_repair_acceptance import analyze_paths, analyze_rows


def row(**overrides):
    base = {
        "pool_amm_id": "pool-1",
        "fee_topology_diversity_index": 0.8,
        "min_fee_topology_diversity_index": 0.7,
        "demand_elasticity_score": None,
        "signer_cross_pool_velocity": None,
        "funding_source_concentration": None,
        "funding_source_v2": {"status": "clean"},
        "sybil_metric_degraded_reasons": [],
        "sybil_soft_flags": "",
        "sybil_interference_patterns": [],
        "shadow_fsc_v2_policy_signal": False,
    }
    base.update(overrides)
    return base


class GatekeeperMetricRepairAcceptanceTests(unittest.TestCase):
    def test_clean_rows_pass(self):
        report = analyze_rows(
            [
                (
                    None,
                    1,
                    row(
                        signer_cross_pool_velocity=0.2,
                        funding_source_concentration=0.3,
                        funding_source_v2={"status": "clean"},
                    ),
                )
            ]
        )

        self.assertTrue(report["pass"])
        self.assertEqual(report["violation_count"], 0)

    def test_flags_pr12_regressions(self):
        rows = [
            row(
                pool_amm_id="fsc-degraded",
                funding_source_concentration=0.9,
                funding_source_v2={"status": "degraded"},
            ),
            row(
                pool_amm_id="des-zero",
                demand_elasticity_score=0.0,
                sybil_metric_degraded_reasons=["DES_NO_COMPARABLE_PAIRS"],
            ),
            row(
                pool_amm_id="dbia-solo",
                sybil_soft_flags="high_dbia",
                sybil_interference_patterns=["HIGH_DBIA_LOW_FTDI"],
            ),
            row(
                pool_amm_id="cpv-coverage",
                signer_cross_pool_velocity=0.75,
                sybil_metric_degraded_reasons=["CPV_COVERAGE_WINDOW_UNAVAILABLE"],
            ),
        ]

        report = analyze_rows((None, idx, value) for idx, value in enumerate(rows, start=1))

        self.assertFalse(report["pass"])
        checks = {violation["check"] for violation in report["violations"]}
        self.assertIn("fsc_degraded_v2_actionable", checks)
        self.assertIn("des_no_comparable_pairs_zero_score", checks)
        self.assertIn("dbia_solo_high_ftdi_structural_penalty", checks)
        self.assertIn("cpv_without_coverage_window", checks)

    def test_nested_buy_log_jsonl_is_supported(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "buy-log.jsonl"
            path.write_text(json.dumps({"buy_log": row(pool_amm_id="nested")}) + "\n")

            report = analyze_paths([path])

        self.assertTrue(report["pass"])
        self.assertEqual(report["rows_checked"], 1)


if __name__ == "__main__":
    unittest.main()
