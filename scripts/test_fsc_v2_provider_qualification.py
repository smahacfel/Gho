#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import build_fsc_v2_provider_qualification as fscq


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class FscV2ProviderQualificationTests(unittest.TestCase):
    def test_builds_required_artifacts_without_promoting_provider(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            create = root / "input" / "create.jsonl"
            trade = root / "input" / "trade.jsonl"
            transfers = root / "input" / "transfers.jsonl"
            decisions = root / "input" / "decisions.jsonl"
            audit = root / "input" / "audit.jsonl"

            write_jsonl(
                create,
                [
                    {
                        "topic": "prod.rpc.solana.pumpfun.create",
                        "payload_json": {
                            "mint": "mint1",
                            "bonding_curve": "curve1",
                            "creator": "creator1",
                            "signature": "sig-create",
                            "slot": "10",
                            "tx_index": "1",
                            "block_time": "100",
                        },
                        "recv_ts_ms": "100030",
                    }
                ],
            )
            write_jsonl(
                trade,
                [
                    {
                        "topic": "prod.rpc.solana.pumpfun.trade",
                        "signature": "sig-trade",
                        "slot": "11",
                        "tx_index": "2",
                        "recv_ts_ms": "101000",
                    }
                ],
            )
            write_jsonl(
                transfers,
                [
                    {
                        "topic": "prod.rpc.solana.system.transfers",
                        "payload_json": {
                            "signature": "sig-transfer",
                            "slot": "9",
                            "tx_index": "0",
                            "instruction_index": "3",
                            "from_wallet": "source1",
                            "to_wallet": "buyer1",
                            "amount": "20000000",
                            "token_address": "solana",
                        },
                        "recv_ts_ms": "99000",
                    },
                    {
                        "topic": "prod.rpc.solana.system.transfers",
                        "payload_json": {
                            "signature": "sig-wsol",
                            "slot": "9",
                            "tx_index": "1",
                            "instruction_index": "4",
                            "from_wallet": "source2",
                            "to_wallet": "buyer2",
                            "amount": "20000000",
                            "token_address": "So11111111111111111111111111111111111111112",
                        },
                        "recv_ts_ms": "99500",
                    },
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "mint1:curve1:100000",
                        "base_mint": "mint1",
                        "pool_id": "curve1",
                        "decision_ts_ms": 105_000,
                        "funding_source_v2": {
                            "snapshot_mode": "decision_time",
                            "status": "clean",
                            "hhi_norm_count": 1.0,
                            "hhi_norm_sol_weighted_excess": 1.0,
                            "total_buyers": 2,
                            "known_buyers": 2,
                            "known_non_neutral_buyers": 2,
                            "unknown_count": 0,
                            "neutral_count": 0,
                            "low_confidence_count": 0,
                            "same_slot_unorderable_count": 0,
                            "known_coverage": 1.0,
                            "non_neutral_known_coverage": 1.0,
                            "top_funder": "source1",
                            "top_funder_count": 2,
                            "top_funder_buy_sol": 1.2,
                            "ttl_seconds": 300,
                            "min_abs_attribution_lamports": 10_000_000,
                            "index_warm": True,
                            "gap_suspected": False,
                            "provider": "NLN",
                            "source_topics": ["prod.rpc.solana.system.transfers"],
                        },
                    }
                ],
            )
            write_jsonl(
                audit,
                [
                    {
                        "topic": "raw_yellowstone_audit",
                        "signature": "sig-transfer",
                        "tx_index": "0",
                        "instruction_index": "3",
                        "from_wallet": "source1",
                        "to_wallet": "buyer1",
                        "amount": "20000000",
                        "recv_ts_ms": "99010",
                    }
                ],
            )

            manifest = fscq.build_artifacts(
                fscq.build_parser().parse_args(
                    [
                        "--scope",
                        "unit",
                        "--root",
                        str(root),
                        "--nln-create",
                        str(create),
                        "--nln-trade",
                        str(trade),
                        "--nln-transfer",
                        str(transfers),
                        "--decision-log",
                        str(decisions),
                        "--audit-event",
                        str(audit),
                        "--min-benchmark-hours",
                        "0",
                    ]
                )
            )

            dataset_dir = root / "datasets" / "selector" / "unit"
            report_dir = root / "reports" / "selector" / "unit"
            raw_dir = root / "logs" / "nln" / "unit"

            self.assertEqual(
                manifest["runtime_impact"],
                "offline_artifact_builder_only; no Gatekeeper, execution, or runtime config changes",
            )
            self.assertTrue((raw_dir / "pumpfun_create_raw_v1.jsonl").exists())
            self.assertTrue((raw_dir / "pumpfun_trade_raw_v1.jsonl").exists())
            self.assertTrue((raw_dir / "system_transfers_raw_v1.jsonl").exists())

            births = read_jsonl(dataset_dir / "nln_candidate_birth_v1.jsonl")
            funding = read_jsonl(dataset_dir / "funding_events_v1.jsonl")
            fsc_rows = read_jsonl(dataset_dir / "fsc_snapshots_v2.jsonl")
            coverage = read_json(report_dir / "fsc_coverage_v2.json")
            benchmark = read_json(report_dir / "nln_provider_benchmark_v1.json")

        self.assertEqual(births[0]["candidate_birth_status"], "ok")
        self.assertEqual(births[0]["slot"], 10)
        self.assertEqual(births[0]["tx_index"], 1)
        self.assertEqual(births[0]["birth_ts_ms"], 100_000)
        self.assertEqual(births[0]["quote_mint_source"], "verified_nln_pumpfun_create_topic")
        self.assertEqual(len(funding), 1)
        self.assertEqual(funding[0]["asset"], "native_sol")
        self.assertEqual(funding[0]["slot"], 9)
        self.assertEqual(funding[0]["tx_index"], 0)
        self.assertEqual(funding[0]["instruction_index"], 3)
        self.assertEqual(funding[0]["amount_lamports"], 20_000_000)
        self.assertEqual(fsc_rows[0]["fsc_count"], 1.0)
        self.assertEqual(coverage["status"], "PASS")
        self.assertEqual(benchmark["status"], "PASS")
        self.assertEqual(benchmark["shared_event_keys"], 1)
        self.assertEqual(benchmark["incomplete_nln_event_key_count"], 0)
        self.assertEqual(benchmark["incomplete_audit_event_key_count"], 0)

    def test_default_benchmark_fails_closed_without_audit_and_duration(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            create = root / "create.jsonl"
            write_jsonl(create, [{"signature": "sig-create", "slot": 1, "recv_ts_ms": 1000}])

            manifest = fscq.build_artifacts(
                fscq.build_parser().parse_args(
                    ["--scope", "unit", "--root", str(root), "--nln-create", str(create)]
                )
            )

            benchmark = read_json(
                root / "reports" / "selector" / "unit" / "nln_provider_benchmark_v1.json"
            )

        self.assertEqual(manifest["status"], "NO-GO")
        self.assertEqual(benchmark["status"], "NO-GO")
        self.assertIn("audit_rows_missing", benchmark["fail_reasons"])
        self.assertIn("benchmark_duration_below_minimum", benchmark["fail_reasons"])

    def test_provider_benchmark_fails_closed_on_incomplete_transfer_key(self) -> None:
        self.assertIsNone(fscq.event_key({"signature": "sig"}))

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            transfers = root / "transfers.jsonl"
            audit = root / "audit.jsonl"
            write_jsonl(
                transfers,
                [
                    {
                        "topic": "prod.rpc.solana.system.transfers",
                        "signature": "sig-transfer",
                        "recv_ts_ms": 1000,
                    }
                ],
            )
            write_jsonl(
                audit,
                [
                    {
                        "topic": "raw_yellowstone_audit",
                        "signature": "sig-transfer",
                        "tx_index": 0,
                        "instruction_index": 3,
                        "from_wallet": "source1",
                        "to_wallet": "buyer1",
                        "amount": 20_000_000,
                        "recv_ts_ms": 990,
                    }
                ],
            )

            manifest = fscq.build_artifacts(
                fscq.build_parser().parse_args(
                    [
                        "--scope",
                        "unit",
                        "--root",
                        str(root),
                        "--nln-transfer",
                        str(transfers),
                        "--audit-event",
                        str(audit),
                        "--min-benchmark-hours",
                        "0",
                    ]
                )
            )
            benchmark = read_json(
                root / "reports" / "selector" / "unit" / "nln_provider_benchmark_v1.json"
            )

        self.assertEqual(manifest["status"], "NO-GO")
        self.assertEqual(benchmark["status"], "NO-GO")
        self.assertIn("incomplete_nln_event_key", benchmark["fail_reasons"])
        self.assertEqual(benchmark["incomplete_nln_event_key_count"], 1)
        self.assertEqual(benchmark["shared_event_keys"], 0)
        self.assertIn(
            "tx_index",
            benchmark["samples"]["incomplete_nln_event_keys"][0]["missing_fields"],
        )


if __name__ == "__main__":
    raise SystemExit(unittest.main())
