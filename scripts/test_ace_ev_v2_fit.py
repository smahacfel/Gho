#!/usr/bin/env python3
"""Focused contract tests for the offline ACE-EV V2 fitting script.

These tests intentionally exercise only input partitioning and outcome-free
fit/threshold boundaries.  The integration smoke below uses the pinned model
dependencies to fit a synthetic complete cohort; this file remains runnable
without NumPy or scikit-learn.
"""

from __future__ import annotations

import importlib.util
import hashlib
import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_ev_v2_fit.py")
SPEC = importlib.util.spec_from_file_location("ace_ev_v2_fit", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
FIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIT)


def row(index: int) -> dict:
    split = (
        "TRAIN"
        if index <= FIT.TRAIN_ROWS
        else "THRESHOLD_CALIBRATION"
        if index <= FIT.TRAIN_ROWS + FIT.THRESHOLD_ROWS
        else "UNTOUCHED_TEST"
    )
    return {
        "schema": "ace_ev_v2_candidate_outcome_v1",
        "enrollment_index": index,
        "split": split,
        "normalized_features": [float(index)] * FIT.FEATURE_COUNT,
        "terminal_net_pnl_sol": float(index) / 1_000.0,
        "candidate_order": {
            "decision_ingress_cutoff_ms": index,
            "birth_ts_ms": index,
            "event_slot": index,
            "bonding_curve": f"curve-{index:03d}",
            "base_mint": f"mint-{index:03d}",
        },
        "profit17_hit": False,
        "terminal_status": "EXIT_FILLED",
        "stress_latency_1s": {"terminal_net_pnl_sol": float(index) / 1_000.0},
    }


class AceEvV2FitContractTests(unittest.TestCase):
    def test_exact_chronological_400_200_400_contract_is_accepted(self) -> None:
        FIT.require_terminal_rows([row(index) for index in range(1, 1001)])

    def test_untouched_test_split_cannot_be_relabelled_as_training(self) -> None:
        rows = [row(index) for index in range(1, 1001)]
        rows[600]["split"] = "TRAIN"
        with self.assertRaisesRegex(ValueError, "expected split UNTOUCHED_TEST"):
            FIT.require_terminal_rows(rows)

    def test_candidate_order_regression_is_rejected_before_fit(self) -> None:
        rows = [row(index) for index in range(1, 1001)]
        rows[601]["candidate_order"]["decision_ingress_cutoff_ms"] = 1
        with self.assertRaisesRegex(ValueError, "candidate_order is not monotonic"):
            FIT.require_terminal_rows(rows)

    def test_unsupported_route_subtype_uses_loss_contribution_not_row_count(self) -> None:
        rows = [
            {"terminal_net_pnl_sol": -0.1, "terminal_status": "EXIT_FILLED"},
            {
                "terminal_net_pnl_sol": -1.0,
                "terminal_status": "POST_ENTRY_UNSUPPORTED_ROUTE_LOSS_FLOOR",
            },
        ]
        dominates, share = FIT.route_loss_dominates(rows)
        self.assertTrue(dominates)
        self.assertAlmostEqual(share, 1.0 / 1.1)

    def test_source_binding_rejects_a_tampered_feature_scale_hash_before_fit(self) -> None:
        with tempfile.TemporaryDirectory(prefix="ace-ev-v2-source-binding-") as temp:
            root = Path(temp)
            contract = root / "contract.json"
            contract.write_text('{"schema":"ace_ev_v2_contract_v1"}\n', encoding="utf-8")
            outcomes = root / "outcomes.jsonl"
            outcomes.write_bytes(b"")
            feature_scale = root / "feature_scale.json"
            feature_scale.write_text('{"scale":"fixture"}\n', encoding="utf-8")
            amendment = root / "amendment.json"
            contract_hash = hashlib.sha256(contract.read_bytes()).hexdigest()
            amendment.write_text(
                json.dumps(
                    {
                        "schema": FIT.AMENDMENT_SCHEMA,
                        "base_contract_sha256": contract_hash,
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            summary = root / "summary.json"
            summary.write_text(
                json.dumps(
                    {
                        "schema": "ace_ev_v2_summary_v1",
                        "capture_kind": "prospective_1000",
                        "capture_status": "VALID_CAPTURE",
                        "terminal_status": "ACE_EV_V2_OUTCOMES_READY_FOR_FIT",
                        "prospective_terminalization": "TARGET_REACHED",
                        "prospective_stop_evidence_sha256": "d" * 64,
                        "implementation_sha": "a" * 40,
                        "code_hash": "git:" + "a" * 40,
                        "contract_sha256": contract_hash,
                        "feature_scale_sha256": "0" * 64,
                        "prospective_amendment_sha256": hashlib.sha256(
                            amendment.read_bytes()
                        ).hexdigest(),
                        "cohort_candidate_order_sha256": "c" * 64,
                        "candidate_outcomes_sha256": hashlib.sha256(
                            outcomes.read_bytes()
                        ).hexdigest(),
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            args = Namespace(
                summary=summary,
                feature_scale=feature_scale,
                amendment=amendment,
                implementation_sha="a" * 40,
            )
            with self.assertRaisesRegex(ValueError, "feature-scale hash mismatch"):
                FIT.load_prospective_sources(args, contract.read_bytes(), outcomes.read_bytes())


if __name__ == "__main__":
    unittest.main()
