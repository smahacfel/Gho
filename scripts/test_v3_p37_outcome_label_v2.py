#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_outcome_label_v2 as label_v2


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def decision(pool_id: str = "pool") -> dict:
    return {
        "ab_record_id": f"{pool_id}:1000:2000:REJECT",
        "pool_id": pool_id,
        "base_mint": f"{pool_id}_mint",
        "join_key": f"{pool_id}:mint:1000",
        "first_seen_ts_ms": 1000,
        "decision_reason": "REJECT_TEST",
        "decision_verdict_buy": False,
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_reason_code": "REJECT_V3_TEST",
    }


def threshold(
    pool_id: str = "pool",
    *,
    status: str = "ok",
    verdict: str = "OK",
    max_return: float = 45.0,
    min_return: float = -10.0,
    entry_price: float | None = 1.0,
    usable: bool = True,
    entry_delta_ms: int = -100,
    samples: list[dict] | None = None,
) -> dict:
    row = {
        "ab_record_id": f"{pool_id}:1000:2000:REJECT",
        "pool_id": pool_id,
        "base_mint": f"{pool_id}_mint",
        "join_key": f"{pool_id}:mint:1000",
        "first_seen_ts_ms": 1000,
        "threshold_status": status,
        "threshold_verdict": verdict,
        "threshold_window_max_return_pct": max_return,
        "threshold_window_min_return_pct": min_return,
        "threshold_hit_after_entry_s": 7.5,
        "hypothetical_entry_price_sol": entry_price,
        "hypothetical_entry_target_ts_ms": 2000,
        "hypothetical_entry_match_delta_ms": entry_delta_ms,
        "hypothetical_entry_match_selection": "latest_blocktime_lte_target",
        "analysis_entry_match_quality_usable": usable,
        "threshold_monitor_window_s": 90.0,
        "threshold_monitor_window_deadline_s": 222.0,
    }
    if samples is not None:
        row["price_path_samples"] = samples
    return row


def price_path(
    pool_id: str = "pool",
    *,
    status: str = "ok",
    source: str = "rpc_pool_signatures",
    samples: list[dict] | None = None,
    unknown_reason: str | None = None,
) -> dict:
    return {
        "price_path_schema_version": 1,
        "ab_record_id": f"{pool_id}:1000:2000:REJECT",
        "pool_id": pool_id,
        "base_mint": f"{pool_id}_mint",
        "join_key": f"{pool_id}:mint:1000",
        "entry_ts_ms": 2000,
        "entry_price": 1.0,
        "path_status": status,
        "path_source": source,
        "samples": samples or [],
        "sample_count": len(samples or []),
        "unknown_reason": unknown_reason,
    }


class P37OutcomeLabelV2Tests(unittest.TestCase):
    def build_one(self, threshold_row: dict, price_path_row: dict | None = None) -> dict:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            decisions = tmp / "decisions.jsonl"
            thresholds = tmp / "thresholds.jsonl"
            price_paths = tmp / "price_paths.jsonl"
            output = tmp / "labels_v2.jsonl"
            write_jsonl(decisions, [decision()])
            write_jsonl(thresholds, [threshold_row])
            if price_path_row is not None:
                write_jsonl(price_paths, [price_path_row])
            summary = label_v2.build_labels(
                decisions,
                thresholds,
                output,
                target_pct=40.0,
                stop_pct=40.0,
                dirty_mae_pct=-40.0,
                price_path_samples_path=price_paths if price_path_row is not None else None,
            )
            rows = [json.loads(line) for line in output.read_text().splitlines()]
        self.assertEqual(summary["written"], 1)
        self.assertEqual(len(rows), 1)
        return rows[0]

    def test_plus40_without_price_path_is_good_dirty_not_good_clean(self) -> None:
        row = self.build_one(threshold())

        self.assertEqual(row["v1_outcome_class"], "good_entry")
        self.assertEqual(row["market_outcome_class"], "good_dirty")
        self.assertEqual(row["unknown_reason"], "missing_price_path_for_good_clean")
        self.assertEqual(row["price_path_source"], "none")
        self.assertIsNone(row["mfe_pct_10s"])

    def test_price_path_can_create_good_clean_when_mae_is_controlled(self) -> None:
        row = self.build_one(
            threshold(
                samples=[
                    {"ts_ms": 2000, "return_pct": 0.0},
                    {"ts_ms": 3000, "return_pct": -8.0},
                    {"ts_ms": 7000, "return_pct": 44.0},
                    {"ts_ms": 15_000, "return_pct": 30.0},
                ]
            )
        )

        self.assertEqual(row["market_outcome_class"], "good_clean")
        self.assertEqual(row["label_quality"], "clean_price_path")
        self.assertEqual(row["mfe_pct_10s"], 44.0)
        self.assertEqual(row["mae_pct_10s"], -8.0)
        self.assertEqual(row["drawdown_before_plus40"], -8.0)
        self.assertTrue(row["survived_10s"])

    def test_external_price_path_can_create_good_clean(self) -> None:
        row = self.build_one(
            threshold(samples=None),
            price_path(
                samples=[
                    {"ts_ms": 2000, "return_pct": 0.0, "price_sol": 1.0},
                    {"ts_ms": 3000, "return_pct": -8.0, "price_sol": 0.92},
                    {"ts_ms": 7000, "return_pct": 44.0, "price_sol": 1.44},
                ]
            ),
        )

        self.assertEqual(row["market_outcome_class"], "good_clean")
        self.assertEqual(row["label_quality"], "clean_price_path")
        self.assertEqual(row["price_path_source"], "rpc_pool_signatures")
        self.assertEqual(row["price_path_status"], "ok")
        self.assertEqual(row["mfe_pct_10s"], 44.0)
        self.assertEqual(row["mae_pct_10s"], -8.0)

    def test_unavailable_external_price_path_does_not_promote_good_clean(self) -> None:
        row = self.build_one(
            threshold(
                samples=[
                    {"ts_ms": 2000, "return_pct": 0.0},
                    {"ts_ms": 7000, "return_pct": 44.0},
                ]
            ),
            price_path(status="unavailable", samples=[], unknown_reason="no_post_entry_price_samples"),
        )

        self.assertEqual(row["market_outcome_class"], "good_dirty")
        self.assertEqual(row["label_quality"], "dirty_threshold_summary")
        self.assertEqual(row["price_path_source"], "none")
        self.assertEqual(row["price_path_status"], "unavailable")
        self.assertEqual(row["price_path_unknown_reason"], "no_post_entry_price_samples")
        self.assertEqual(row["unknown_reason"], "missing_price_path_for_good_clean")

    def test_invalid_entry_stays_unknown(self) -> None:
        row = self.build_one(threshold(entry_price=None))

        self.assertEqual(row["v1_outcome_class"], "unknown")
        self.assertEqual(row["market_outcome_class"], "unknown")
        self.assertEqual(row["unknown_reason"], "missing_or_invalid_entry_price")
        self.assertEqual(row["entry_price_confidence"], "missing")

    def test_bad_outcome_is_bad_clean_with_threshold_summary(self) -> None:
        row = self.build_one(threshold(verdict="NOK", max_return=8.0, min_return=-55.0))

        self.assertEqual(row["v1_outcome_class"], "bad_entry")
        self.assertEqual(row["market_outcome_class"], "bad_clean")
        self.assertEqual(row["hit_stop_40"], True)

    def test_reserved_lifecycle_args_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            with self.assertRaises(NotImplementedError):
                label_v2.validate_reserved_inputs(tmp / "lifecycle.jsonl", None)


if __name__ == "__main__":
    unittest.main()
