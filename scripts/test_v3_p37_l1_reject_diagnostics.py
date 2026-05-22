#!/usr/bin/env python3
"""Unit tests for P3.7-L1 reject diagnostics."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("v3_p37_l1_reject_diagnostics.py")
SPEC = importlib.util.spec_from_file_location("v3_p37_l1_reject_diagnostics", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
l1 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(l1)


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


class L1RejectDiagnosticsTests(unittest.TestCase):
    def build_temp_summary(
        self,
        decisions: list[dict],
        artifacts: dict[str, list[dict]] | None = None,
    ) -> dict:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config_path = root / "r16.toml"
            brain_config_path = root / "brain.toml"
            brain_config_path.write_text("[gatekeeper_v2]\nmode = \"standard\"\n", encoding="utf-8")
            config_path.write_text(
                "\n".join(
                    [
                        f'ghost_brain_config_path = "{brain_config_path.name}"',
                        "[p37_shadow_probe]",
                        'namespace = "r16-test"',
                        'run_id = "run-r16"',
                        'session_id = "session-r16"',
                        'selection_log_path = "probe_selection.jsonl"',
                        'transport_log_path = "probe_transport.jsonl"',
                        'entry_log_path = "probe_entries.jsonl"',
                        'lifecycle_log_path = "probe_lifecycle.jsonl"',
                        'skip_log_path = "probe_skips.jsonl"',
                        "[execution.shadow]",
                        'entry_log_path = "shadow_entries.jsonl"',
                        'lifecycle_log_path = "shadow_lifecycle.jsonl"',
                        "[trigger.shadow_run]",
                        'payer_strategy = "configured"',
                        'output_path = "buys.jsonl"',
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            paths = {
                "active_shadow_buys": root / "buys.jsonl",
                "probe_selection": root / "probe_selection.jsonl",
                "probe_transport": root / "probe_transport.jsonl",
                "probe_entries": root / "probe_entries.jsonl",
                "probe_lifecycle": root / "probe_lifecycle.jsonl",
                "active_shadow_entries": root / "shadow_entries.jsonl",
                "active_shadow_lifecycle": root / "shadow_lifecycle.jsonl",
                "lifecycle_labels": root / "p3_7_shadow_lifecycle_labels.jsonl",
                "probe_skips": root / "probe_skips.jsonl",
            }
            artifacts = artifacts or {}
            for name, path in paths.items():
                write_jsonl(path, artifacts.get(name, []))
            return l1.build_summary(config_path, l1.load_toml(config_path), root / "decisions.jsonl", decisions, 0, decisions)

    def test_identity_status_fails_when_rows_miss_run_session_or_hash(self) -> None:
        summary = self.build_temp_summary(
            [
                {
                    "verdict_type": "REJECT",
                    "rollout_profile": "r16-test",
                    "v3_policy_config_hash": "policy-hash",
                    "brain_config_path": "brain.toml",
                    "brain_config_hash": "brain-hash",
                }
            ]
        )
        self.assertEqual(summary["r16_artifact_identity_status"], "FAIL")
        self.assertEqual(summary["single_active_hash_status"], "FAIL")

    def test_buy_shadow_counts_are_joined_to_buy_decision_ab_record_id(self) -> None:
        base = {
            "rollout_profile": "r16-test",
            "run_id": "run-r16",
            "session_id": "session-r16",
            "v3_policy_config_hash": "policy-hash",
            "brain_config_path": "brain.toml",
            "brain_config_hash": "brain-hash",
        }
        summary = self.build_temp_summary(
            [
                {**base, "verdict_type": "BUY", "ab_record_id": "ab-buy"},
                {**base, "verdict_type": "REJECT", "ab_record_id": "ab-reject"},
            ],
            {
                "active_shadow_entries": [
                    {**base, "ab_record_id": "ab-buy"},
                    {**base, "ab_record_id": "ab-other"},
                ],
                "active_shadow_lifecycle": [{**base, "ab_record_id": "ab-buy"}],
            },
        )
        self.assertEqual(summary["r16_buy_verdict_count"], 1)
        self.assertEqual(summary["r16_buy_shadow_entry_count"], 1)
        self.assertEqual(summary["r16_buy_lifecycle_close_count"], 1)
        self.assertEqual(summary["r16_buy_shadow_entry_unmatched_count"], 1)

    def test_pdd_drift_rows_include_non_terminal_drift_evaluations(self) -> None:
        base = {
            "verdict_type": "BUY",
            "rollout_profile": "r16-test",
            "run_id": "run-r16",
            "session_id": "session-r16",
            "v3_policy_config_hash": "policy-hash",
            "brain_config_path": "brain.toml",
            "brain_config_hash": "brain-hash",
            "pdd_entry_drift_threshold_source": "elapsed_scaled",
            "pdd_entry_drift_pct": 10.0,
            "pdd_entry_drift_elapsed_ms": 3000,
            "pdd_entry_drift_anchor_price": 1.0,
            "pdd_entry_drift_current_price": 1.1,
        }
        summary = self.build_temp_summary([base])
        self.assertEqual(summary["pdd_drift_rows"], 1)
        self.assertEqual(summary["pdd_drift_anchor_rows"], 1)
        self.assertEqual(summary["pdd_drift_evaluated_rows"], 1)
        self.assertEqual(summary["pdd_drift_anchor_hydrated_rows"], 1)

    def test_pdd_drift_denominator_excludes_threshold_source_only_rows(self) -> None:
        base = {
            "verdict_type": "REJECT",
            "rollout_profile": "r16-test",
            "run_id": "run-r16",
            "session_id": "session-r16",
            "v3_policy_config_hash": "policy-hash",
            "brain_config_path": "brain.toml",
            "brain_config_hash": "brain-hash",
            "gatekeeper_terminal_gate": "pdd",
            "pdd_spike_ratio_quality": "unavailable",
            "pdd_whale_single_max_pct": 12.0,
        }
        summary = self.build_temp_summary(
            [
                {
                    **base,
                    "pdd_entry_drift_threshold_source": "elapsed_scaled",
                    "pdd_entry_drift_pct": 10.0,
                    "pdd_entry_drift_elapsed_ms": 3000,
                    "pdd_entry_drift_anchor_price": 1.0,
                    "pdd_entry_drift_current_price": 1.1,
                },
                {
                    **base,
                    "pdd_entry_drift_threshold_source": "elapsed_scaled",
                },
            ]
        )
        self.assertEqual(summary["pdd_drift_evaluated_rows"], 1)
        self.assertEqual(summary["pdd_drift_anchor_hydrated_rows"], 1)
        self.assertEqual(summary["pdd_drift_anchor_coverage_pct_among_evaluated"], 100.0)
        self.assertEqual(summary["pdd_drift_threshold_source_rows"], 2)
        self.assertEqual(summary["pdd_drift_threshold_source_only_rows"], 1)
        self.assertEqual(
            summary["diagnostic_quality"]["pdd_entry_drift_anchor_coverage_pct"],
            100.0,
        )

    def test_shadow_payer_account_not_found_is_reported(self) -> None:
        base = {
            "verdict_type": "BUY",
            "rollout_profile": "r16-test",
            "run_id": "run-r16",
            "session_id": "session-r16",
            "v3_policy_config_hash": "policy-hash",
            "brain_config_path": "brain.toml",
            "brain_config_hash": "brain-hash",
            "ab_record_id": "ab-buy",
        }
        summary = self.build_temp_summary(
            [base],
            {
                "active_shadow_buys": [
                    {
                        **base,
                        "payer_pubkey": "9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw",
                        "err": "Failed to fetch payer account: AccountNotFound: pubkey=9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw: timeout",
                    }
                ],
            },
        )
        self.assertEqual(summary["shadow_payer_strategy"], "configured")
        self.assertEqual(summary["shadow_payer_account_status"], "rpc_missing")
        self.assertEqual(summary["shadow_payer_account_not_found_rows"], 1)


if __name__ == "__main__":
    unittest.main()
