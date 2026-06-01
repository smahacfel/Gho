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
    def test_legitimate_zero_hhi_is_not_fake_zero(self) -> None:
        legitimate_distinct_sources = {
            "fsc_count": 0.0,
            "fsc_status": "degraded",
            "fsc_excluded_reason": "low_coverage",
            "fsc_total_buyers": 7,
            "fsc_unknown_count": 5,
            "fsc_known_non_neutral_buyers": 2,
            "raw_fsc_v2": {
                "source_counts": [
                    {"source": {"wallet": "source1"}, "count": 1},
                    {"source": {"wallet": "source2"}, "count": 1},
                ]
            },
        }
        self.assertFalse(fscq.fake_zero_fsc_row(legitimate_distinct_sources))

        unavailable_zero = {
            "fsc_count": 0.0,
            "fsc_status": "unavailable",
            "fsc_excluded_reason": "index_cold",
            "fsc_total_buyers": 3,
            "fsc_unknown_count": 3,
            "fsc_known_non_neutral_buyers": 0,
            "raw_fsc_v2": {"source_counts": []},
        }
        self.assertTrue(fscq.fake_zero_fsc_row(unavailable_zero))

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
                        "mint": "mint1",
                        "user": "buyer1",
                        "ix_name": "buy",
                        "sol_amount": "50000000",
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
                        "provider": "Alchemy",
                        "source_kind": "archive_rpc",
                        "audit_mode": "sampled_block_audit",
                        "slot": 9,
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
                        "--min-audit-slots",
                        "0",
                        "--min-audit-transfer-events",
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
            unknown_reason = read_json(report_dir / "fsc_unknown_reason_v2.json")
            parameter_grid = read_json(report_dir / "fsc_parameter_grid_v1.json")
            join_sanity = read_json(report_dir / "nln_native_fsc_join_sanity_v1.json")
            benchmark = read_json(report_dir / "nln_provider_benchmark_v1.json")
            topic_liveness = read_json(report_dir / "nln_topic_liveness_v1.json")
            canary = read_json(report_dir / "fsc_capture_canary_v1.json")

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
        self.assertEqual(unknown_reason["status"], "PASS")
        self.assertEqual(unknown_reason["unknown_buyer_count"], 0)
        self.assertEqual(parameter_grid["status"], "PASS")
        self.assertEqual(parameter_grid["baseline_variant"]["known_buyers"], 1)
        self.assertEqual(join_sanity["status"], "PASS")
        self.assertEqual(join_sanity["nln_native_summary"]["known_buyers"], 1)
        self.assertEqual(benchmark["status"], "PASS")
        self.assertEqual(benchmark["shared_event_keys"], 1)
        self.assertEqual(benchmark["audit_sampling_mode"], "sampled_block_audit")
        self.assertEqual(topic_liveness["status"], "PASS")
        self.assertEqual(topic_liveness["create_rows"], 1)
        self.assertEqual(topic_liveness["trade_status"], "PASS")
        self.assertEqual(topic_liveness["transfer_status"], "PASS")
        self.assertEqual(canary["status"], "PASS")
        self.assertEqual(manifest["status"], "PASS_FOR_PHASE1_EVIDENCE")
        self.assertEqual(manifest["capture_evidence_status"], "PASS")
        self.assertEqual(manifest["provider_policy_qualification"], "NOT_CLAIMED")
        self.assertEqual(manifest["phase1_dataset_unblock"], "PASS")
        self.assertEqual(benchmark["incomplete_nln_event_key_count"], 0)
        self.assertEqual(benchmark["incomplete_audit_event_key_count"], 0)

    def test_missing_external_audit_is_not_capture_blocker(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            trade = root / "trade.jsonl"
            transfers = root / "transfers.jsonl"
            decisions = root / "decisions.jsonl"
            write_jsonl(
                trade,
                [
                    {
                        "topic": "prod.rpc.solana.pumpfun.trade",
                        "signature": "sig-trade",
                        "mint": "mint1",
                        "user": "buyer1",
                        "ix_name": "buy",
                        "sol_amount": 50_000_000,
                        "slot": 2,
                        "tx_index": 0,
                        "recv_ts_ms": 2000,
                    }
                ],
            )
            write_jsonl(
                transfers,
                [
                    {
                        "topic": "prod.rpc.solana.system.transfers",
                        "signature": "sig-transfer",
                        "slot": 1,
                        "tx_index": 0,
                        "instruction_index": 0,
                        "from_wallet": "source1",
                        "to_wallet": "buyer1",
                        "amount": 20_000_000,
                        "token_address": "solana",
                        "recv_ts_ms": 1000,
                    }
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "candidate1",
                        "funding_source_v2": {
                            "snapshot_mode": "decision_time",
                            "status": "unavailable",
                            "excluded_reason": "insufficient_non_neutral_support",
                            "hhi_norm_count": None,
                            "total_buyers": 1,
                            "known_buyers": 0,
                            "known_non_neutral_buyers": 0,
                            "unknown_count": 1,
                            "neutral_count": 0,
                            "low_confidence_count": 0,
                            "same_slot_unorderable_count": 0,
                            "known_coverage": 0.0,
                            "non_neutral_known_coverage": 0.0,
                        },
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
                        "--nln-trade",
                        str(trade),
                        "--nln-transfer",
                        str(transfers),
                        "--decision-log",
                        str(decisions),
                    ]
                )
            )

            benchmark = read_json(
                root / "reports" / "selector" / "unit" / "nln_provider_benchmark_v1.json"
            )
            canary = read_json(root / "reports" / "selector" / "unit" / "fsc_capture_canary_v1.json")
            unknown_reason = read_json(
                root / "reports" / "selector" / "unit" / "fsc_unknown_reason_v2.json"
            )

        self.assertEqual(manifest["status"], "PASS_FOR_PHASE1_EVIDENCE")
        self.assertEqual(manifest["provider_independent_benchmark"], "NOT_AVAILABLE")
        self.assertEqual(manifest["phase1_dataset_unblock"], "PASS")
        self.assertEqual(benchmark["status"], "NOT_AVAILABLE")
        self.assertEqual(benchmark["fail_reasons"], [])
        self.assertFalse(benchmark["blocking"])
        self.assertEqual(canary["status"], "PASS")
        self.assertEqual(unknown_reason["top_reason"], "FSC_INSUFFICIENT_KNOWN_SOURCES")

    def test_provider_benchmark_classifies_incomplete_transfer_key(self) -> None:
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
                        "provider": "Alchemy",
                        "source_kind": "archive_rpc",
                        "audit_mode": "sampled_block_audit",
                        "slot": 1,
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
                        "--min-audit-slots",
                        "0",
                        "--min-audit-transfer-events",
                        "0",
                    ]
                )
            )
            benchmark = read_json(
                root / "reports" / "selector" / "unit" / "nln_provider_benchmark_v1.json"
            )

        self.assertEqual(manifest["status"], "NO-GO")
        self.assertEqual(benchmark["status"], "NO-GO")
        self.assertNotIn("incomplete_nln_event_key", benchmark["fail_reasons"])
        self.assertIn("no_keyable_nln_transfer_rows", benchmark["fail_reasons"])
        self.assertEqual(benchmark["incomplete_nln_event_key_count"], 1)
        self.assertEqual(
            benchmark["incomplete_nln_event_key_classification"],
            "excluded_from_transfer_event_key_benchmark",
        )
        self.assertEqual(benchmark["shared_event_keys"], 0)
        self.assertIn(
            "tx_index",
            benchmark["samples"]["incomplete_nln_event_keys"][0]["missing_fields"],
        )


if __name__ == "__main__":
    raise SystemExit(unittest.main())
