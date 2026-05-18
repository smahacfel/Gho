#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_price_path_fetcher as fetcher


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def decision(row_id: str = "pool:1000:2000:REJECT") -> dict:
    return {
        "ab_record_id": row_id,
        "join_key": f"join-{row_id}",
        "pool_id": "pool-a",
        "base_mint": "mint-a",
        "first_seen_ts_ms": 1000,
        "observation_end_ts_ms": 2000,
        "decision_verdict_buy": False,
        "verdict_type": "REJECT",
    }


def threshold(
    row_id: str = "pool:1000:2000:REJECT",
    *,
    usable: bool = True,
    entry_price: float = 1.0,
) -> dict:
    return {
        "ab_record_id": row_id,
        "join_key": f"join-{row_id}",
        "pool_id": "pool-a",
        "base_mint": "mint-a",
        "threshold_status": "ok",
        "threshold_verdict": "OK",
        "hypothetical_entry_price_sol": entry_price,
        "hypothetical_entry_target_ts_ms": 2358,
        "hypothetical_entry_match_delta_ms": 0 if usable else 6000,
        "analysis_entry_match_quality_usable": usable,
        "threshold_window_max_return_pct": 45.0,
        "threshold_window_min_return_pct": -10.0,
    }


class P37PricePathFetcherTests(unittest.TestCase):
    def test_schema_skeleton_emits_unavailable_without_inference(self) -> None:
        row = fetcher.base_row(
            decision(),
            threshold(),
            target_pct=40.0,
            stop_pct=40.0,
            window_s=60.0,
        )

        self.assertEqual(row["price_path_schema_version"], 1)
        self.assertEqual(row["entry_price"], 1.0)
        self.assertEqual(row["entry_ts_ms"], 2358)
        self.assertEqual(row["entry_match_confidence"], "usable_causal_match")
        self.assertEqual(row["path_status"], "unavailable")
        self.assertEqual(row["path_source"], "unavailable")
        self.assertEqual(row["samples"], [])
        self.assertIsNone(row["mfe_pct_10s"])
        self.assertTrue(row["threshold_summary_is_not_price_path"])

    def test_invalid_entry_fails_closed(self) -> None:
        row = fetcher.base_row(
            decision(),
            threshold(usable=False),
            target_pct=40.0,
            stop_pct=40.0,
            window_s=60.0,
        )

        self.assertEqual(row["path_status"], "entry_invalid")
        self.assertEqual(row["entry_match_confidence"], "unusable_match")
        self.assertEqual(row["unknown_reason"], "entry_match_not_usable")

    def test_build_rows_joins_thresholds_and_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decisions = root / "decisions.jsonl"
            thresholds = root / "thresholds.jsonl"
            write_jsonl(decisions, [decision("a"), decision("b")])
            write_jsonl(thresholds, [threshold("a"), threshold("b", usable=False)])

            rows, counts = fetcher.build_rows(
                decisions,
                thresholds,
                target_pct=40.0,
                stop_pct=40.0,
                window_s=60.0,
            )

        self.assertEqual(len(rows), 2)
        self.assertEqual(counts["threshold_matched"], 2)
        self.assertEqual(counts["unavailable"], 1)
        self.assertEqual(counts["entry_invalid"], 1)

    def test_limit_bounds_processed_decisions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decisions = root / "decisions.jsonl"
            thresholds = root / "thresholds.jsonl"
            write_jsonl(decisions, [decision("a"), decision("b")])
            write_jsonl(thresholds, [threshold("a"), threshold("b")])

            rows, counts = fetcher.build_rows(
                decisions,
                thresholds,
                target_pct=40.0,
                stop_pct=40.0,
                window_s=60.0,
                limit=1,
            )

        self.assertEqual(len(rows), 1)
        self.assertEqual(counts["decisions"], 1)

    def test_resume_skips_output_and_checkpoint_identities(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decisions = root / "decisions.jsonl"
            thresholds = root / "thresholds.jsonl"
            output = root / "path.jsonl"
            checkpoint = root / "path.checkpoint.jsonl"
            write_jsonl(decisions, [decision("a"), decision("b")])
            write_jsonl(thresholds, [threshold("a"), threshold("b")])
            existing = fetcher.base_row(
                decision("a"),
                threshold("a"),
                target_pct=40.0,
                stop_pct=40.0,
                window_s=60.0,
            )
            write_jsonl(output, [existing])

            summary = fetcher.run(
                fetcher.build_parser().parse_args(
                    [
                        "--decisions",
                        str(decisions),
                        "--threshold-hits",
                        str(thresholds),
                        "--output",
                        str(output),
                    "--checkpoint",
                    str(checkpoint),
                    "--schema-only",
                    "--resume",
                ]
            )
            )

            rows = read_jsonl(output)
            checkpoint_rows = read_jsonl(checkpoint)

        self.assertEqual(summary["counts"]["skipped_existing"], 1)
        self.assertEqual(summary["counts"]["written_candidates"], 1)
        self.assertEqual(len(rows), 2)
        self.assertEqual(len(checkpoint_rows), 1)
        self.assertEqual(rows[-1]["ab_record_id"], "b")

    def test_cli_outputs_json_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decisions = root / "decisions.jsonl"
            thresholds = root / "thresholds.jsonl"
            output = root / "path.jsonl"
            write_jsonl(decisions, [decision("a")])
            write_jsonl(thresholds, [threshold("a")])

            exit_code = fetcher.main(
                [
                    "--decisions",
                    str(decisions),
                    "--threshold-hits",
                    str(thresholds),
                    "--output",
                    str(output),
                    "--schema-only",
                    "--json",
                ]
            )
            rows = read_jsonl(output)

        self.assertEqual(exit_code, 0)
        self.assertEqual(len(rows), 1)

    def test_rpc_collector_builds_price_path_samples(self) -> None:
        class FakeClient:
            def call(self, method, params):
                if method == "getSignaturesForAddress":
                    return {
                        "result": [
                            {"signature": "s2", "blockTime": 10, "slot": 20, "err": None},
                            {"signature": "s1", "blockTime": 3, "slot": 10, "err": None},
                        ]
                    }
                if method == "getTransaction":
                    sig = params[0]
                    delta_sol = 1_500_000 if sig == "s2" else 1_000_000
                    return {
                        "result": {
                            "slot": 20 if sig == "s2" else 10,
                            "transaction": {"message": {"accountKeys": [{"pubkey": "pool-a"}]}},
                            "meta": {
                                "preBalances": [1_000_000_000],
                                "postBalances": [1_000_000_000 + delta_sol],
                                "preTokenBalances": [
                                    {
                                        "mint": "mint-a",
                                        "owner": "pool-a",
                                        "uiTokenAmount": {"amount": "2000000"},
                                    }
                                ],
                                "postTokenBalances": [
                                    {
                                        "mint": "mint-a",
                                        "owner": "pool-a",
                                        "uiTokenAmount": {"amount": "1000000"},
                                    }
                                ],
                            },
                        }
                    }
                raise AssertionError(method)

        collector = object.__new__(fetcher.RpcPathCollector)
        collector.client = FakeClient()
        collector.max_pages = 3
        collector.target_pct = 40.0
        collector.diag_timelines = {}
        row = fetcher.base_row(
            decision(),
            threshold(entry_price=0.001),
            target_pct=40.0,
            stop_pct=40.0,
            window_s=60.0,
        )

        out = collector.collect(row)

        self.assertEqual(out["path_status"], "ok")
        self.assertEqual(out["path_source"], "rpc_pool_signatures")
        self.assertEqual(out["sample_count"], 2)
        self.assertEqual([sample["signature"] for sample in out["samples"]], ["s1", "s2"])
        self.assertAlmostEqual(out["samples"][1]["return_pct"], 50.0)
        self.assertAlmostEqual(out["mfe_pct_10s"], 50.0)
        self.assertEqual(out["time_to_mfe_ms"], 7642)

    def test_rpc_collector_fails_closed_on_rpc_error(self) -> None:
        class FailingClient:
            def call(self, method, params):
                raise RuntimeError("boom")

        collector = object.__new__(fetcher.RpcPathCollector)
        collector.client = FailingClient()
        collector.max_pages = 3
        collector.target_pct = 40.0
        collector.diag_timelines = {}
        row = fetcher.base_row(
            decision(),
            threshold(),
            target_pct=40.0,
            stop_pct=40.0,
            window_s=60.0,
        )

        out = collector.collect(row)

        self.assertEqual(out["path_status"], "rpc_error")
        self.assertEqual(out["sample_count"], 0)
        self.assertIn("boom", out["unknown_reason"])

    def test_diag_samples_preempt_rpc_when_available(self) -> None:
        class FailingClient:
            def call(self, method, params):
                raise AssertionError("RPC should not be called when DIAG samples exist")

        collector = object.__new__(fetcher.RpcPathCollector)
        collector.client = FailingClient()
        collector.max_pages = 3
        collector.target_pct = 40.0
        collector.diag_timelines = {
            "mint-a": [
                {
                    "timestamp_ms": 3000,
                    "slot": 10,
                    "sol_reserves_lamports": 1_000_000,
                    "token_reserves_raw": 1_000_000,
                },
                {
                    "timestamp_ms": 10000,
                    "slot": 20,
                    "sol_reserves_lamports": 1_500_000,
                    "token_reserves_raw": 1_000_000,
                },
            ]
        }
        row = fetcher.base_row(
            decision(),
            threshold(entry_price=0.001),
            target_pct=40.0,
            stop_pct=40.0,
            window_s=60.0,
        )

        out = collector.collect(row)

        self.assertEqual(out["path_status"], "ok")
        self.assertEqual(out["path_source"], "diag_account_update")
        self.assertEqual(out["sample_count"], 2)
        self.assertAlmostEqual(out["samples"][1]["return_pct"], 50.0)
        self.assertEqual(out["collector_status"], "diag_collected")


if __name__ == "__main__":
    unittest.main()
