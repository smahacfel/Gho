#!/usr/bin/env python3

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fsc_attribution_lookup_audit as audit


class FscAttributionLookupAuditTest(unittest.TestCase):
    def test_audit_joins_lookup_wallet_to_funding_windows(self) -> None:
        lookup_rows = [
            audit.LookupRow(
                decision_id="decision-1",
                lookup_wallet="buyer-a",
                decision_ts_ms=1_000_000,
                miss_reason=None,
                lookup_result="hit",
            ),
            audit.LookupRow(
                decision_id="decision-2",
                lookup_wallet="buyer-b",
                decision_ts_ms=1_000_000,
                miss_reason="NO_INBOUND_TRANSFER_OBSERVED",
                lookup_result="miss",
            ),
            audit.LookupRow(
                decision_id="decision-3",
                lookup_wallet="buyer-c",
                decision_ts_ms=1_000_000,
                buy_event_ts_ms=900_000,
                miss_reason="NO_INBOUND_TRANSFER_OBSERVED",
                lookup_result="miss",
            ),
        ]
        funding_events = [
            audit.FundingEvent(
                recipient_wallet="buyer-a",
                source_wallet="source-a",
                lamports=12_000_000,
                ts_ms=1_000_000 - 4 * 60 * 1000,
                signature="fund-a",
                slot=10,
            ),
            audit.FundingEvent(
                recipient_wallet="buyer-b",
                source_wallet="source-b",
                lamports=12_000_000,
                ts_ms=1_000_000 - 45 * 60 * 1000,
                signature="fund-b",
                slot=11,
            ),
            audit.FundingEvent(
                recipient_wallet="buyer-c",
                source_wallet="source-c",
                lamports=12_000_000,
                ts_ms=950_000,
                signature="fund-c",
                slot=12,
            ),
        ]

        rows = audit.audit_lookup_rows(lookup_rows, funding_events)

        self.assertEqual(rows[0]["decision_id"], "decision-1")
        self.assertTrue(rows[0]["found_5m"])
        self.assertTrue(rows[0]["found_60m"])
        self.assertEqual(rows[0]["latest_funding_age_ms"], 240_000)
        self.assertEqual(rows[0]["funding_amount_lamports"], 12_000_000)
        self.assertEqual(rows[0]["source_wallet"], "source-a")
        self.assertEqual(rows[0]["diagnosed_bottleneck"], "ATTRIBUTION_HIT")

        self.assertFalse(rows[1]["found_5m"])
        self.assertTrue(rows[1]["found_60m"])
        self.assertEqual(rows[1]["diagnosed_bottleneck"], "LOOKBACK_WINDOW_TOO_SHORT")

        self.assertEqual(rows[2]["decision_id"], "decision-3")
        self.assertEqual(rows[2]["buy_event_ts_ms"], 900_000)
        self.assertFalse(rows[2]["found_60m"])
        self.assertEqual(
            rows[2]["diagnosed_bottleneck"],
            "DIRECT_FUNDING_NOT_OBSERVED_60M",
        )

    def test_writes_required_csv_columns(self) -> None:
        rows = [
            {
                "decision_id": "decision-1",
                "lookup_wallet": "buyer-a",
                "decision_ts_ms": 1_000_000,
                "buy_event_ts_ms": 999_000,
                "found_5m": True,
                "found_15m": True,
                "found_30m": True,
                "found_60m": True,
                "latest_funding_age_ms": 1_000,
                "funding_amount_lamports": 1_000_000,
                "source_wallet": "source-a",
                "miss_reason": "",
                "diagnosed_bottleneck": "ATTRIBUTION_HIT",
            }
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "audit.csv"
            audit.write_csv(path, rows)
            header = path.read_text(encoding="utf-8").splitlines()[0]

        self.assertEqual(header.split(","), audit.CSV_COLUMNS)

    def test_extracts_lookup_rows_from_decision_diagnostics(self) -> None:
        decisions = [
            {
                "ab_record_id": "decision-1",
                "observation_end_ts_ms": 1_000_000,
                "funding_source_diagnostics": {
                    "lookup_diagnostics": [
                        {
                            "selected_lookup_wallet": "buyer-a",
                            "buy_event_ts_ms": 999_000,
                            "lookup_result": "miss",
                            "diagnostic_miss_reason": "NO_INBOUND_TRANSFER_OBSERVED",
                        }
                    ]
                },
            }
        ]

        rows = audit.load_lookup_rows([], decisions, [])

        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0].decision_id, "decision-1")
        self.assertEqual(rows[0].lookup_wallet, "buyer-a")
        self.assertEqual(rows[0].decision_ts_ms, 1_000_000)
        self.assertEqual(rows[0].buy_event_ts_ms, 999_000)
        self.assertEqual(rows[0].miss_reason, "NO_INBOUND_TRANSFER_OBSERVED")

    def test_streams_only_lookup_wallet_funding_events(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "funding.jsonl"
            path.write_text(
                "\n".join(
                    [
                        '{"recipient_wallet":"buyer-a","source_wallet":"source-a","lamports":1,"ts_ms":100}',
                        '{"recipient_wallet":"buyer-b","source_wallet":"source-b","lamports":2,"ts_ms":200}',
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            events, rows_scanned, events_parsed = (
                audit.stream_funding_events_for_lookup_wallets([path], ["buyer-a"])
            )

        self.assertEqual(rows_scanned, 2)
        self.assertEqual(events_parsed, 2)
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0].recipient_wallet, "buyer-a")


if __name__ == "__main__":
    unittest.main()
