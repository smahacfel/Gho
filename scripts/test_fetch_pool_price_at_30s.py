#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
LABELER_PATH = (
    REPO_ROOT
    / "logs"
    / "decisions.json"
    / "rollout"
    / "shadow-burnin"
    / "decisions"
    / "fetch_pool_price_at_30s.py"
)


def load_labeler():
    spec = importlib.util.spec_from_file_location("fetch_pool_price_at_30s", LABELER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load labeler from {LABELER_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FetchPoolPriceAt30sTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.labeler = load_labeler()

    def test_vectors_entry_uses_last_sample_not_after_target(self):
        rec = {
            "vectors_ts_offsets_ms": [0, 8_000, 9_000],
            "vectors_prices": [1.0, 2.0, 3.0],
        }

        price, blocktime, stats = self.labeler.get_price_from_vectors(
            rec,
            t0_ts_ms=100_000,
            target_ts_ms=108_500,
            search_start_ts_ms=103_500,
            search_end_ts_ms=108_500,
        )

        self.assertEqual(price, 2.0)
        self.assertEqual(blocktime, 108)
        self.assertEqual(stats["matched_ts_ms"], 108_000)
        self.assertEqual(stats["match_delta_ms"], -500)
        self.assertEqual(stats["match_selection"], "last_lte_target")

    def test_vectors_entry_rejects_future_only_sample(self):
        rec = {
            "vectors_ts_offsets_ms": [9_000],
            "vectors_prices": [3.0],
        }

        price, blocktime, stats = self.labeler.get_price_from_vectors(
            rec,
            t0_ts_ms=100_000,
            target_ts_ms=108_500,
            search_start_ts_ms=103_500,
            search_end_ts_ms=109_500,
        )

        self.assertIsNone(price)
        self.assertIsNone(blocktime)
        self.assertEqual(stats["status"], "no_causal_vectors_in_window")

    def test_threshold_scan_starts_at_entry_target_not_entry_match(self):
        rec = {
            "vectors_ts_offsets_ms": [8_000, 8_500, 9_000],
            "vectors_prices": [1.0, 1.5, 1.6],
        }

        hit_price, stats = self.labeler.find_threshold_hit_in_vectors(
            rec,
            t0_ts_ms=100_000,
            entry_price_sol=1.0,
            entry_match_ts_ms=108_000,
            entry_target_ts_ms=108_700,
            monitor_window_s=90.0,
            monitor_window_deadline_s=222.0,
            take_profit_pct=40.0,
            stop_loss_pct=40.0,
        )

        self.assertEqual(hit_price, 1.6)
        self.assertEqual(stats["verdict"], "OK")
        self.assertEqual(stats["hit_after_entry_s"], 0.3)
        self.assertEqual(stats["window_start_ts_ms"], 108_700)

    def test_diag_lookup_does_not_fallback_to_future_update(self):
        timeline = {
            "timestamps_ms": [101_600],
            "updates": [{"timestamp_ms": 101_600}],
        }

        update = self.labeler.find_causal_diag_update(
            timeline,
            target_ts_ms=101_500,
            search_start_ts_ms=101_000,
            search_end_ts_ms=102_000,
        )

        self.assertIsNone(update)


if __name__ == "__main__":
    unittest.main()
