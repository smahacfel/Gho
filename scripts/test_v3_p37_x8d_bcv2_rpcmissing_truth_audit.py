#!/usr/bin/env python3

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import scripts.v3_p37_x8d_bcv2_rpcmissing_truth_audit as audit


class X8DBcv2RpcMissingTruthAuditTests(unittest.TestCase):
    def write_x8d_pr1_fixture(self, rows: list[dict]) -> Path:
        tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(tmpdir.cleanup)
        path = Path(tmpdir.name) / "x8d_pr1.json"
        path.write_text(
            json.dumps(
                {
                    "schema": "x8d_pr1_unique_bcv2_pubkey_join_v1",
                    "rows": rows,
                }
            ),
            encoding="utf-8",
        )
        return path

    def test_load_pubkey_contexts_dedupes_and_preserves_context(self) -> None:
        path = self.write_x8d_pr1_fixture(
            [
                {
                    "bcv2_pubkey": "11111111111111111111111111111111",
                    "x8d_pr1_primary_bucket": "included_rpc_missing_no_same_update",
                    "observed_slot_range": [10, 12],
                    "precheck_context_slot_range": [14, 15],
                    "same_pubkey_account_update": False,
                    "included_in_subscribe_inferred": True,
                    "dropped_over_cap_inferred": False,
                },
                {
                    "bcv2_pubkey": "11111111111111111111111111111111",
                    "x8d_pr1_primary_bucket": "duplicate_should_be_ignored",
                },
            ]
        )

        contexts = audit.load_pubkey_contexts(path)

        self.assertEqual(1, len(contexts))
        self.assertEqual("11111111111111111111111111111111", contexts[0]["pubkey"])
        self.assertEqual("included_rpc_missing_no_same_update", contexts[0]["x8d_pr1_primary_bucket"])
        self.assertEqual(10, contexts[0]["observed_slot_min"])
        self.assertEqual(12, contexts[0]["observed_slot_max"])
        self.assertEqual(15, contexts[0]["precheck_context_slot_max"])
        self.assertTrue(contexts[0]["included_in_subscribe_inferred"])

    def test_summary_classifies_ready_commitments_delays_and_missing(self) -> None:
        contexts = [
            {
                "pubkey": "ready_processed",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
            {
                "pubkey": "ready_confirmed",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
            {
                "pubkey": "ready_finalized",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
            {
                "pubkey": "missing_all",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
        ]
        attempts = [
            {"pubkey": "ready_processed", "commitment": "processed", "delay_ms": 0, "ready": True, "missing": False, "error_class": None, "provider_label": "p"},
            {"pubkey": "ready_processed", "commitment": "confirmed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
            {"pubkey": "ready_confirmed", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
            {"pubkey": "ready_confirmed", "commitment": "confirmed", "delay_ms": 250, "ready": True, "missing": False, "error_class": None, "provider_label": "p"},
            {"pubkey": "ready_finalized", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
            {"pubkey": "ready_finalized", "commitment": "confirmed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
            {"pubkey": "ready_finalized", "commitment": "finalized", "delay_ms": 1000, "ready": True, "missing": False, "error_class": None, "provider_label": "p"},
            {"pubkey": "missing_all", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
            {"pubkey": "missing_all", "commitment": "confirmed", "delay_ms": 250, "ready": False, "missing": True, "error_class": "missing_on_rpc", "provider_label": "p"},
        ]

        summary = audit.summarize_attempts(contexts, attempts)

        self.assertEqual("PR2A-C_PROVIDER_TIMING_DEPENDENT", summary["verdict"])
        self.assertEqual(4, summary["unique_bcv2_pubkeys"])
        self.assertEqual(3, summary["ready_unique_pubkeys"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["ready_on_processed"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["ready_on_confirmed"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["ready_on_finalized"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["missing_all_commitments_all_delays"])
        self.assertEqual(2, summary["audit_bucket_unique_pubkeys"]["ready_after_delay"])

    def test_zero_ready_current_missing_verdict(self) -> None:
        contexts = [
            {
                "pubkey": "a",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
            {
                "pubkey": "b",
                "same_pubkey_account_update": False,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            },
        ]
        attempts = [
            {"pubkey": "a", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
            {"pubkey": "a", "commitment": "confirmed", "delay_ms": 250, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
            {"pubkey": "b", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
            {"pubkey": "b", "commitment": "finalized", "delay_ms": 3000, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
        ]

        summary = audit.summarize_attempts(contexts, attempts)

        self.assertEqual("PR2A-B_ZERO_READY_CURRENT_MISSING", summary["verdict"])
        self.assertEqual(2, summary["primary_bucket_unique_pubkeys"]["missing_all_commitments_all_delays"])
        self.assertEqual(0, summary["ready_unique_pubkeys"])

    def test_conflicting_account_update_does_not_unlock_ready(self) -> None:
        contexts = [
            {
                "pubkey": "same_update_missing",
                "same_pubkey_account_update": True,
                "included_in_subscribe_inferred": True,
                "dropped_over_cap_inferred": False,
            }
        ]
        attempts = [
            {"pubkey": "same_update_missing", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
            {"pubkey": "same_update_missing", "commitment": "confirmed", "delay_ms": 1000, "ready": False, "missing": True, "error_class": "missing_on_rpc"},
        ]

        summary = audit.summarize_attempts(contexts, attempts)
        row = summary["pubkey_rows"][0]

        self.assertEqual("PR2A-B_ZERO_READY_CURRENT_MISSING", summary["verdict"])
        self.assertEqual("conflicting_account_update", row["primary_bucket"])
        self.assertIn("conflicting_account_update", row["audit_buckets"])
        self.assertFalse(row["ready"])

    def test_provider_timeout_invalid_pubkey_and_unavailable_are_inconclusive(self) -> None:
        contexts = [
            {"pubkey": "bad_pubkey", "same_pubkey_account_update": False},
            {"pubkey": "timeout_pubkey", "same_pubkey_account_update": False},
            {"pubkey": "no_url_pubkey", "same_pubkey_account_update": False},
        ]
        attempts = [
            {"pubkey": "bad_pubkey", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": False, "error_class": "invalid_pubkey"},
            {"pubkey": "timeout_pubkey", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": False, "error_class": "provider_timeout"},
            {"pubkey": "no_url_pubkey", "commitment": "processed", "delay_ms": 0, "ready": False, "missing": False, "error_class": "rpc_url_unavailable"},
        ]

        summary = audit.summarize_attempts(contexts, attempts)

        self.assertEqual("PR2A-INCONCLUSIVE_RPC_ERRORS", summary["verdict"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["invalid_pubkey"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["provider_timeout"])
        self.assertEqual(1, summary["primary_bucket_unique_pubkeys"]["inconclusive_rpc_error"])


if __name__ == "__main__":
    unittest.main()
