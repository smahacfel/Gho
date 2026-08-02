#!/usr/bin/env python3
"""Integration proof for the frozen ACE-EV V2 100/50/100 fit boundary.

Run with the isolated environment described by
``requirements-ace-ev-v2.txt``.  Two cohorts differ *only* in untouched TEST
outcomes.  The resulting Huber coefficients, validation-only threshold and
SELECTED membership must remain byte-for-byte equal.
"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("ace_ev_v2_fit.py")
CONTRACT_PATH = Path(__file__).parents[1] / "configs" / "rollout" / "ACE_EV_V2_CONTRACT_V1.json"
SPEC = importlib.util.spec_from_file_location("ace_ev_v2_fit", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
FIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIT)


def feature_vector(index: int) -> list[float]:
    # Full-rank deterministic features avoid a convergence warning while
    # remaining small and reproducible.
    return [
        (index % 11) / 10.0,
        (index % 7) / 7.0,
        (index % 5) / 5.0,
        (index % 13) / 13.0,
        (index % 17) / 17.0,
        (index % 19) / 19.0,
        (index % 23) / 23.0,
    ]


def row(index: int, test_target_shift: float) -> dict:
    split = (
        "TRAIN"
        if index <= FIT.TRAIN_ROWS
        else "THRESHOLD_CALIBRATION"
        if index <= FIT.TRAIN_ROWS + FIT.THRESHOLD_ROWS
        else "UNTOUCHED_TEST"
    )
    features = feature_vector(index)
    target = (
        0.04 * features[0]
        - 0.03 * features[1]
        + 0.02 * features[3]
        + 0.001 * (index % 3)
    )
    if split == "UNTOUCHED_TEST":
        target += test_target_shift
    return {
        "schema": "ace_ev_v2_candidate_outcome_v1",
        "enrollment_index": index,
        "split": split,
        "base_mint": f"mint-{index:03d}",
        "normalized_features": features,
        "terminal_net_pnl_sol": target,
        "candidate_order": {
            "decision_ingress_cutoff_ms": index,
            "birth_ts_ms": index,
            "event_slot": index,
            "bonding_curve": f"curve-{index:03d}",
            "base_mint": f"mint-{index:03d}",
        },
        "profit17_hit": target >= 0.017,
        "terminal_status": "EXIT_FILLED",
        "stress_latency_1s": {"terminal_net_pnl_sol": target - 0.002},
    }


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


@unittest.skipUnless(
    importlib.util.find_spec("numpy") is not None and importlib.util.find_spec("sklearn") is not None,
    "requires pinned NumPy and scikit-learn",
)
class AceEvV2FitIntegrationTests(unittest.TestCase):
    def test_untouched_test_outcomes_cannot_change_fit_tau_or_membership(self) -> None:
        with tempfile.TemporaryDirectory(prefix="ace-ev-v2-fit-") as temp:
            root = Path(temp)
            first_outcomes = root / "first.jsonl"
            second_outcomes = root / "second.jsonl"
            first_output = root / "first-output"
            second_output = root / "second-output"
            write_jsonl(first_outcomes, [row(index, 0.0) for index in range(1, 251)])
            write_jsonl(second_outcomes, [row(index, -0.5) for index in range(1, 251)])

            self.assertEqual(
                FIT.fit(
                    Namespace(
                        outcomes=first_outcomes,
                        contract=CONTRACT_PATH,
                        output_dir=first_output,
                    )
                ),
                0,
            )
            self.assertEqual(
                FIT.fit(
                    Namespace(
                        outcomes=second_outcomes,
                        contract=CONTRACT_PATH,
                        output_dir=second_output,
                    )
                ),
                0,
            )
            first_report = json.loads((first_output / "model_report_v1.json").read_text())
            second_report = json.loads((second_output / "model_report_v1.json").read_text())
            self.assertEqual(first_report["model"]["coef"], second_report["model"]["coef"])
            self.assertEqual(first_report["model"]["intercept"], second_report["model"]["intercept"])
            self.assertEqual(first_report["threshold"]["tau"], second_report["threshold"]["tau"])
            first_selected = [
                row["selected"]
                for row in (json.loads(line) for line in (first_output / "test_predictions_v1.jsonl").read_text().splitlines())
            ]
            second_selected = [
                row["selected"]
                for row in (json.loads(line) for line in (second_output / "test_predictions_v1.jsonl").read_text().splitlines())
            ]
            self.assertEqual(first_selected, second_selected)
            self.assertNotEqual(
                first_report["test"]["selected_mean_net_pnl_sol"],
                second_report["test"]["selected_mean_net_pnl_sol"],
            )


if __name__ == "__main__":
    unittest.main()
