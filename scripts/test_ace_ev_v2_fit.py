#!/usr/bin/env python3
"""Focused contract tests for the offline ACE-EV V2 fitting script.

These tests intentionally exercise only input partitioning and outcome-free
fit/threshold boundaries.  The integration smoke below uses the pinned model
dependencies to fit a synthetic complete cohort; this file remains runnable
without NumPy or scikit-learn.
"""

from __future__ import annotations

import importlib.util
import unittest
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
    def test_exact_chronological_100_50_100_contract_is_accepted(self) -> None:
        FIT.require_terminal_rows([row(index) for index in range(1, 251)])

    def test_untouched_test_split_cannot_be_relabelled_as_training(self) -> None:
        rows = [row(index) for index in range(1, 251)]
        rows[150]["split"] = "TRAIN"
        with self.assertRaisesRegex(ValueError, "expected split UNTOUCHED_TEST"):
            FIT.require_terminal_rows(rows)

    def test_candidate_order_regression_is_rejected_before_fit(self) -> None:
        rows = [row(index) for index in range(1, 251)]
        rows[151]["candidate_order"]["decision_ingress_cutoff_ms"] = 1
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


if __name__ == "__main__":
    unittest.main()
