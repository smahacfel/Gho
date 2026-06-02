#!/usr/bin/env python3

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import scripts.analyze_bcv2_hydration_gap as audit


class AnalyzeBcv2HydrationGapTests(unittest.TestCase):
    def write_log(self, lines: list[str]) -> Path:
        tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(tmpdir.cleanup)
        path = Path(tmpdir.name) / "system.log"
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return path

    def test_exact_watch_system_empty_bucket(self) -> None:
        path = self.write_log(
            [
                "2026-05-31T21:00:00Z INFO seer::binary_parser: BCV2_EXACT_WATCH_REGISTERED pubkey=bcv2A inserted=true signature=sigA observed_slot=100 observed_slot_index=1 instruction_index=6 buy_variant=routed_exact_sol_in registry_version=7",
                "2026-05-31T21:00:00Z INFO seer::binary_parser: BCV2_RPC_HYDRATION_MISSING pubkey=bcv2A commitment=processed context_slot=101 latency_ms=11 error_class=missing_on_rpc observed_slot=100 signature=sigA",
                "2026-05-31T21:00:01Z INFO seer::grpc_connection: BCV2_ACCOUNT_UPDATE_RECEIVED pubkey=bcv2A slot=102 owner=11111111111111111111111111111111 data_len=0",
            ]
        )

        result = audit.analyze_paths([path])

        self.assertEqual("PASS", result["status"])
        self.assertEqual({"exact_watch_system_empty": 1}, result["primary_bucket_counts"])
        row = result["rows"][0]
        self.assertEqual("bcv2A", row["pubkey"])
        self.assertEqual("exact_watch_system_empty", row["primary_bucket"])
        self.assertEqual({"missing_on_rpc": 1}, row["hydration_error_classes"])

    def test_rpc_ready_dominates_missing(self) -> None:
        path = self.write_log(
            [
                "BCV2_EXACT_WATCH_REGISTERED pubkey=bcv2B inserted=true signature=sigB observed_slot=200 observed_slot_index=1 instruction_index=6 buy_variant=routed_exact_sol_in registry_version=8",
                "BCV2_RPC_HYDRATION_MISSING pubkey=bcv2B commitment=processed attempt=1 attempt_count=3 context_slot=201 latency_ms=9 error_class=rpc_missing_initial observed_slot=200 signature=sigB",
                "BCV2_RPC_HYDRATION_READY pubkey=bcv2B commitment=processed attempt=2 attempt_count=3 context_slot=202 owner=Pump111111111111111111111111111111111111111 data_len=256 latency_ms=12 observed_slot=200 signature=sigB",
            ]
        )

        result = audit.analyze_paths([path])

        self.assertEqual({"rpc_ready": 1}, result["primary_bucket_counts"])
        row = result["rows"][0]
        self.assertEqual("rpc_ready", row["primary_bucket"])
        self.assertEqual({"1/3": 1, "2/3": 1}, row["hydration_attempts"])

    def test_subscribe_drop_is_audit_only_inference(self) -> None:
        path = self.write_log(
            [
                "BCV2_EXACT_WATCH_REGISTERED pubkey=bcv2C inserted=true signature=sigC observed_slot=300 observed_slot_index=1 instruction_index=6 buy_variant=routed_exact_sol_in registry_version=9",
                "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED profile=primary_global bcv2_dropped=5 tracked_bcv2=205 bcv2_sent=199 exact_payload_cap=199 from_slot=301",
                "BCV2_RPC_HYDRATION_MISSING pubkey=bcv2C commitment=processed context_slot=302 latency_ms=10 error_class=missing_on_rpc observed_slot=300 signature=sigC",
            ]
        )

        result = audit.analyze_paths([path])

        row = result["rows"][0]
        self.assertEqual("exact_watch_capacity_pressure", row["primary_bucket"])
        self.assertTrue(row["dropped_over_cap_inferred"])
        self.assertEqual("audit_only_aggregate_subscribe_marker_after_registration", row["subscribe_inference_note"])

    def test_no_bcv2_rows_is_no_go(self) -> None:
        path = self.write_log(["ordinary log line"])

        result = audit.analyze_paths([path])

        self.assertEqual("NO-GO", result["status"])
        self.assertEqual(0, result["unique_bcv2_pubkeys"])


if __name__ == "__main__":
    unittest.main()
