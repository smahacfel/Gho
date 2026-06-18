#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import analiza_porownawcza as ap


def make_segment_records(n: int, *, offset: int = 0) -> list[dict]:
    records = []
    for i in range(n):
        x = i + offset
        records.append({
            "ab_record_id": f"rec-{x}",
            "buyer_hhi": 0.01 + x * 0.001,
            "buy_count": 50 + x,
            "unique_buyers": 10 + (x % 11),
            "unique_signers_evaluated": 12 + (x % 13),
            "sell_share": 0.05 + (x % 7) * 0.01,
        })
    return records


class SegmentLabTests(unittest.TestCase):
    def test_condition_eval_supported_ops(self) -> None:
        record = {"buyer_hhi": 0.02, "name": "HARD_FAIL_RISK", "kind": "alpha"}

        self.assertTrue(ap.condition_passes(record, {"field": "buyer_hhi", "op": ">=", "value": 0.01}))
        self.assertTrue(ap.condition_passes(record, {"field": "buyer_hhi", "op": "<=", "value": 0.05}))
        self.assertFalse(ap.condition_passes(record, {"field": "missing_num", "op": "<=", "value": 1}))
        self.assertTrue(ap.condition_passes(record, {"field": "name", "op": "startswith", "value": "HARD"}))
        self.assertTrue(ap.condition_passes(record, {"field": "name", "op": "not_startswith", "value": "SOFT"}))
        self.assertTrue(ap.condition_passes(record, {"field": "kind", "op": "in", "value": ["alpha", "beta"]}))
        self.assertTrue(ap.condition_passes(record, {"field": "kind", "op": "not_in", "value": ["gamma"]}))
        self.assertTrue(ap.condition_passes(record, {"field": "name", "op": "exists"}))
        self.assertTrue(ap.condition_passes(record, {"field": "absent", "op": "missing"}))

    def test_eval_segment_counts_ab_precision(self) -> None:
        rec_a = [
            {"buyer_hhi": 0.01, "buy_count": 100},
            {"buyer_hhi": 0.02, "buy_count": 80},
            {"buyer_hhi": 0.20, "buy_count": 5},
        ]
        rec_b = [
            {"buyer_hhi": 0.01, "buy_count": 90},
            {"buyer_hhi": 0.30, "buy_count": 10},
            {"buyer_hhi": 0.40, "buy_count": 5},
        ]
        conditions = [
            {"field": "buyer_hhi", "op": "<=", "value": 0.05},
            {"field": "buy_count", "op": ">=", "value": 67},
        ]

        result = ap.eval_segment(rec_a, rec_b, conditions, "buyer_hhi_low_buy_count_high")

        self.assertEqual(result["selected_A"], 2)
        self.assertEqual(result["selected_B"], 1)
        self.assertEqual(result["selected_total"], 3)
        self.assertAlmostEqual(result["precision_ab"], 2 / 3)

    def test_missing_is_not_zero_for_numeric_conditions(self) -> None:
        record = {"buy_count": 100}

        evaluation = ap.condition_eval(record, {"field": "buyer_hhi", "op": "<=", "value": 0.043})

        self.assertFalse(evaluation["passed"])
        self.assertEqual(evaluation["reason"], "missing")

    def test_leakage_fields_are_excluded_from_segment_features(self) -> None:
        excluded = [
            "gatekeeper_verdict_type",
            "decision_verdict_buy",
            "outcome",
            "post_buy_return",
            "created_ts_ms",
            "wall_timestamp",
            "pool_id",
        ]

        for field in excluded:
            with self.subTest(field=field):
                self.assertTrue(ap.is_excluded_segment_field(field))

    def test_field_filter_rejects_low_quality_dynamic_fields(self) -> None:
        rec_a = [{"flag": i % 2, "rare": i} for i in range(25)]
        rec_b = [{"flag": i % 2} for i in range(25)]

        ok_flag, flag_status = ap.segment_field_status(rec_a, rec_b, "flag", min_valid_values=10)
        ok_rare, rare_status = ap.segment_field_status(rec_a, rec_b, "rare", min_valid_values=10)

        self.assertFalse(ok_flag)
        self.assertEqual(flag_status["reason"], "bool_like")
        self.assertFalse(ok_rare)
        self.assertEqual(rare_status["reason"], "insufficient_side_values")

    def test_quartile_segments_generate_four_directions(self) -> None:
        rec_a = make_segment_records(25, offset=0)
        rec_b = make_segment_records(25, offset=25)

        segments, skipped = ap.generate_single_feature_quartile_segments(rec_a, rec_b, ["buyer_hhi"])
        ids = {segment["segment_id"] for segment in segments}

        self.assertFalse(skipped)
        self.assertIn("buyer_hhi_lte_q25", ids)
        self.assertIn("buyer_hhi_lte_q50", ids)
        self.assertIn("buyer_hhi_gte_q50", ids)
        self.assertIn("buyer_hhi_gte_q75", ids)

    def test_semantic_pair_segments_use_whitelist_without_triples(self) -> None:
        rec_a = make_segment_records(25, offset=0)
        rec_b = make_segment_records(25, offset=25)

        segments, _ = ap.generate_semantic_pair_segments(rec_a, rec_b)
        ids = {segment["segment_id"] for segment in segments}

        self.assertTrue(any("buyer_hhi_lte_q25_AND_buy_count_gte_q50" in item for item in ids))
        self.assertTrue(all(len(segment["conditions"]) == 2 for segment in segments))

    def test_ablation_reports_precision_delta_for_two_condition_segment(self) -> None:
        old_min_selected = ap.AB_SEGMENT_MIN_SELECTED
        ap.AB_SEGMENT_MIN_SELECTED = 1
        try:
            rec_a = [
                {"buyer_hhi": 0.01, "buy_count": 100},
                {"buyer_hhi": 0.02, "buy_count": 80},
                {"buyer_hhi": 0.20, "buy_count": 5},
            ]
            rec_b = [
                {"buyer_hhi": 0.01, "buy_count": 90},
                {"buyer_hhi": 0.30, "buy_count": 10},
                {"buyer_hhi": 0.40, "buy_count": 5},
            ]
            segment = {
                "segment_id": "buyer_hhi_low_AND_buy_count_high",
                "conditions": [
                    {"field": "buyer_hhi", "op": "<=", "value": 0.05},
                    {"field": "buy_count", "op": ">=", "value": 67},
                ],
            }

            ablation = ap.eval_ablation(rec_a, rec_b, segment)

            self.assertIsNotNone(ablation)
            self.assertEqual(len(ablation["ablations"]), 2)
            for item in ablation["ablations"]:
                self.assertIn("delta_precision_pp", item)
                self.assertIn("delta_selected_total", item)
        finally:
            ap.AB_SEGMENT_MIN_SELECTED = old_min_selected

    def test_false_positive_sample_uses_only_matching_b_records(self) -> None:
        rec_b = [
            {"ab_record_id": "b1", "buyer_hhi": 0.01, "buy_count": 90, "gatekeeper_verdict_type": "HARD_FAIL_X"},
            {"ab_record_id": "b2", "buyer_hhi": 0.30, "buy_count": 90},
        ]
        conditions = [
            {"field": "buyer_hhi", "op": "<=", "value": 0.05},
            {"field": "buy_count", "op": ">=", "value": 67},
        ]

        samples = ap.false_positive_sample(rec_b, conditions, limit=20)

        self.assertEqual(len(samples), 1)
        self.assertEqual(samples[0]["identity"]["ab_record_id"], "b1")
        self.assertIn("HARD_FAIL_RISK", samples[0]["tags"])

    def test_no_ready_for_bot_vocabulary_in_segment_lab_disclaimer(self) -> None:
        text = ap.SEGMENT_DISCOVERY_DISCLAIMER

        self.assertNotIn("READY_FOR_BOT", text)
        self.assertIn("NO DEPLOY", text)


if __name__ == "__main__":
    unittest.main()
