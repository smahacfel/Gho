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
) -> dict:
    env = envelope(schema, event_id, position_id)
    row = {
        "schema": "shadow_position_event_v2",
        "envelope": env,
        "event_kind": event_kind,
        "event_order_key": event_order_key,
        "canonical_payload_schema": schema,
        "canonical_payload_event_id": event_id,
        "canonical_terminal_event_id": event_id if event_kind == "TERMINAL_TRUTH" else None,
        "payload": {"record_type": schema, "record": {"envelope": env}},
    }
    if ordering_exemption is not None:
        row["ordering_exemption"] = ordering_exemption
    return row


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


if __name__ == "__main__":
    unittest.main()
