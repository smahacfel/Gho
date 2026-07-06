#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
AUDIT_SCRIPT = REPO_ROOT / "scripts" / "shadow_v2_temporal_no_lookahead_audit.py"


def envelope(schema: str, event_id: str, position_id: str) -> dict:
    return {
        "schema": schema,
        "schema_version": 1,
        "simulation_contract_version": "shadow_burnin_simulation_v2_20260629",
        "simulation_level": "MARK_ONLY",
        "measurement_grade": "DIAGNOSTIC_ONLY",
        "run_id": "run-a",
        "session_id": "session-a",
        "candidate_id": "candidate-a",
        "position_id": position_id,
        "event_id": event_id,
        "parent_event_id": None,
        "source_event_id": None,
        "pool_id": "pool-a",
        "base_mint": "mint-a",
        "bonding_curve": None,
        "produced_at_ms": 1_785_000_000_000,
        "produced_at_slot": 42,
        "temporal_class": "POST_EXIT" if schema == "shadow_terminal_truth_v2" else "POST_ENTRY",
        "clock_domain": "STREAM_OBSERVED_MS",
        "source_refs": ["fixture"],
        "quality": "fixture",
        "limitations": [],
    }


def canonical_row(
    schema: str,
    event_kind: str,
    event_id: str,
    position_id: str = "pos-a",
    event_order_key: dict | None = None,
    ordering_exemption: str | None = None,
    record_fields: dict | None = None,
    exact_or_approx: str | None = None,
    source_refs: list[str] | None = None,
    envelope_limitations: list[str] | None = None,
) -> dict:
    env = envelope(schema, event_id, position_id)
    if source_refs is not None:
        env["source_refs"] = source_refs
    if envelope_limitations is not None:
        env["limitations"] = envelope_limitations
    record = {"envelope": env}
    if record_fields:
        record.update(record_fields)
    if exact_or_approx is not None:
        record["exact_or_approx"] = exact_or_approx
    row = {
        "schema": "shadow_position_event_v2",
        "envelope": env,
        "event_kind": event_kind,
        "event_order_key": event_order_key,
        "canonical_payload_schema": schema,
        "canonical_payload_event_id": event_id,
        "canonical_terminal_event_id": event_id if event_kind == "TERMINAL_TRUTH" else None,
        "payload": {"record_type": schema, "record": record},
    }
    if ordering_exemption is not None:
        row["ordering_exemption"] = ordering_exemption
    return row


def transaction_source_event_order_key(
    *,
    slot: int | str = 42,
    block_time: int | str = 1_785_000_000,
    signature: str = "source-sig",
    transaction_index: int | str = 1,
    instruction_index: int | str = 0,
    inner_instruction_index: int | str = "UNKNOWN",
    log_index: int | str = "NOT_APPLICABLE",
    event_seq: int = 7,
) -> dict:
    return {
        "slot": slot,
        "block_time": block_time,
        "signature": signature,
        "transaction_index_or_unknown": transaction_index,
        "instruction_index_or_unknown": instruction_index,
        "inner_instruction_index_or_unknown": inner_instruction_index,
        "log_index_or_unknown": log_index,
        "event_seq_in_process": event_seq,
        "observed_at_wall_ms": 1_785_000_000_123,
    }


def unknown_transaction_source_event_order_key() -> dict:
    return transaction_source_event_order_key(
        block_time="UNKNOWN",
        signature="UNKNOWN",
        transaction_index="UNKNOWN",
        instruction_index="UNKNOWN",
        inner_instruction_index="UNKNOWN",
        event_seq=9,
    )


def derived_terminal_event_order_key() -> dict:
    return {
        "slot": "DERIVED",
        "block_time": "DERIVED",
        "signature": "DERIVED",
        "transaction_index_or_unknown": "DERIVED",
        "instruction_index_or_unknown": "DERIVED",
        "inner_instruction_index_or_unknown": "DERIVED",
        "log_index_or_unknown": "DERIVED",
        "event_seq_in_process": 10,
        "observed_at_wall_ms": 1_785_000_000_555,
    }


class ShadowV2TemporalAuditTest(unittest.TestCase):
    def run_audit(self, rows: list[dict]) -> dict:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            with (scope / "shadow_position_event_v2.jsonl").open("w", encoding="utf-8") as fh:
                for row in rows:
                    fh.write(json.dumps(row, sort_keys=True))
                    fh.write("\n")
            result = subprocess.run(
                [sys.executable, str(AUDIT_SCRIPT), "--scope-root", str(scope)],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            return json.loads(result.stdout)

    def test_shadow_v2_temporal_audit_fails_missing_required_event_order_key(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_terminal_truth_v2",
                    "TERMINAL_TRUTH",
                    "terminal-a",
                    event_order_key=None,
                )
            ]
        )

        self.assertEqual(result["verdict"], "FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION")
        self.assertEqual(result["event_order_key_missing_required_rows"], 1)
        self.assertEqual(result["event_order_key_missing_rows"], 1)

    def test_shadow_v2_temporal_audit_allows_explicit_position_created_exemption(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_position_v2",
                    "POSITION_CREATED",
                    "position-a",
                    event_order_key=None,
                    ordering_exemption="ORDERING_EXEMPT_POSITION_CREATED",
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["event_order_key_exempt_rows"], 1)
        self.assertEqual(result["event_order_key_missing_rows"], 0)
        self.assertEqual(
            result["ordering_exemption_counts"]["ORDERING_EXEMPT_POSITION_CREATED"],
            1,
        )

    def test_shadow_v2_temporal_audit_separates_account_state_proof_from_tx_order(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "pool_state_sample_v2",
                    "POOL_STATE_SAMPLE",
                    "pool-state-a",
                    event_order_key=unknown_transaction_source_event_order_key(),
                    record_fields={
                        "account_data_hash": "hash-a",
                        "source_account_pubkey": "account-a",
                        "source_account_slot": 42,
                        "source_write_version": 7,
                    },
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["account_state_source_proof_complete_count"], 1)
        self.assertEqual(result["transaction_source_proof_complete_count"], 0)
        self.assertEqual(result["unknown_required_source_count"], 0)
        self.assertGreater(result["raw_unknown_chain_order_component_count"], 0)
        self.assertEqual(result["not_applicable_accepted_count"], 1)

    def test_shadow_v2_temporal_audit_blocks_transaction_like_missing_source(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_entry_fill_v2",
                    "ENTRY_FILL",
                    "entry-fill-a",
                    event_order_key=unknown_transaction_source_event_order_key(),
                )
            ]
        )

        self.assertEqual(result["verdict"], "BLOCKED_TEMPORAL_TRANSACTION_SOURCE_JOIN")
        self.assertEqual(result["transaction_source_proof_missing_rows"], 1)
        self.assertEqual(result["unknown_required_source_rows"], 1)
        self.assertGreater(result["unknown_required_source_count"], 0)

    def test_shadow_v2_temporal_audit_allows_account_state_derived_fill_without_tx_source(
        self,
    ) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_entry_fill_v2",
                    "ENTRY_FILL",
                    "entry-fill-account-state-derived",
                    event_order_key=unknown_transaction_source_event_order_key(),
                    record_fields={
                        "pool_state_before": "pool-state-before-a",
                        "execution_label_grade": "RESEARCH_CANDIDATE",
                    },
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["account_state_derived_simulation_source_proof_count"], 1)
        self.assertEqual(result["transaction_source_proof_missing_rows"], 0)
        self.assertEqual(result["unknown_required_source_rows"], 0)

    def test_shadow_v2_temporal_audit_allows_typed_blocked_exit_fill_without_tx_source(
        self,
    ) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_exit_fill_v2",
                    "EXIT_FILL",
                    "exit-fill-blocked-by-data",
                    event_order_key=unknown_transaction_source_event_order_key(),
                    record_fields={
                        "blocked_reasons": ["BLOCKED_POOL_STATE_MISSING"],
                        "execution_label_grade": "DIAGNOSTIC_SIM",
                        "execution_simulation_ready": False,
                        "fill_status": "BLOCKED_BY_DATA",
                        "quality": "BLOCKED_BY_DATA",
                        "reconstruction_status": "EXIT_FILL_BLOCKED_BY_MISSING_POOL_STATE",
                    },
                    envelope_limitations=[
                        "EXIT_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE",
                        "EXIT_FILL_POOL_STATE_SAMPLE_MISSING",
                        "EXIT_POOL_STATE_BEFORE_UNAVAILABLE",
                        "EXIT_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED",
                    ],
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["typed_blocked_simulation_source_exempt_count"], 1)
        self.assertEqual(result["transaction_source_proof_missing_rows"], 0)
        self.assertEqual(result["unknown_required_source_rows"], 0)

    def test_shadow_v2_temporal_audit_allows_legacy_diagnostic_path_sample_without_tx_source(
        self,
    ) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_path_sample_v2",
                    "PATH_SAMPLE",
                    "legacy-path-sample-a",
                    event_order_key=unknown_transaction_source_event_order_key(),
                    record_fields={
                        "pool_state_ref": "MISSING_POOL_STATE_SAMPLE_LEGACY_LIFECYCLE_PRICE_TRUTH_ONLY",
                        "exact_or_approx": "APPROX_AMBIGUOUS_EVENT_ORDER",
                    },
                    source_refs=["shadow_lifecycle:position_closed"],
                    envelope_limitations=[
                        "LEGACY_LIFECYCLE_PRICE_TRUTH_NOT_POOL_STATE_SAMPLE",
                        "PATH_SAMPLE_POOL_STATE_PROVENANCE_MISSING",
                    ],
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["account_state_derived_simulation_source_proof_count"], 1)
        self.assertEqual(result["transaction_source_proof_missing_rows"], 0)
        self.assertEqual(result["unknown_required_source_rows"], 0)

    def test_shadow_v2_temporal_audit_rejects_event_seq_as_exact_order_substitute(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_path_sample_v2",
                    "PATH_SAMPLE",
                    "path-sample-a",
                    event_order_key=unknown_transaction_source_event_order_key(),
                    exact_or_approx="EXACT_EVENT_ORDER",
                )
            ]
        )

        self.assertEqual(result["verdict"], "FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION")
        self.assertEqual(result["event_seq_chain_order_substitute_count"], 1)

    def test_shadow_v2_temporal_audit_counts_terminal_truth_derived_order(self) -> None:
        result = self.run_audit(
            [
                canonical_row(
                    "shadow_terminal_truth_v2",
                    "TERMINAL_TRUTH",
                    "terminal-a",
                    event_order_key=derived_terminal_event_order_key(),
                )
            ]
        )

        self.assertEqual(result["verdict"], "PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT")
        self.assertEqual(result["terminal_truth_derived_count"], 1)
        self.assertEqual(result["terminal_truth_not_derived_count"], 0)


if __name__ == "__main__":
    unittest.main()
