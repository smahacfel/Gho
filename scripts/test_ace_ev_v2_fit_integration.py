#!/usr/bin/env python3
"""Integration proof for the frozen ACE-EV V2 400/200/400 fit boundary.

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
AMENDMENT_PATH = (
    Path(__file__).parents[1]
    / "configs"
    / "rollout"
    / "ACE_EV_V2_PROSPECTIVE_1000_AMENDMENT_V1.json"
)
SPEC = importlib.util.spec_from_file_location("ace_ev_v2_fit", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
FIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIT)


def has_pinned_model_dependencies() -> bool:
    if importlib.util.find_spec("numpy") is None or importlib.util.find_spec("sklearn") is None:
        return False
    import numpy
    import sklearn

    return (
        numpy.__version__ == FIT.PINNED_NUMPY_VERSION
        and sklearn.__version__ == FIT.PINNED_SCIKIT_LEARN_VERSION
    )


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
        "entry_status": "ENTRY_FILLED",
        "terminal_status": "EXIT_FILLED",
        "stress_latency_1s": {"terminal_net_pnl_sol": target - 0.002},
    }


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


def sha256(path: Path) -> str:
    import hashlib

    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_bound_sources(
    root: Path,
    outcomes: Path,
    *,
    implementation_sha: str,
) -> tuple[Path, Path, Path]:
    root.mkdir(parents=True, exist_ok=True)
    feature_scale = root / "feature_scale.json"
    feature_scale.write_text('{"fixture":"feature_scale"}\n', encoding="utf-8")
    summary = root / "summary_v2.json"
    summary.write_text(
        json.dumps(
            {
                "schema": "ace_ev_v2_summary_v1",
                "capture_kind": "prospective_1000",
                "capture_status": "VALID_CAPTURE",
                "terminal_status": "ACE_EV_V2_OUTCOMES_READY_FOR_FIT",
                "prospective_terminalization": "TARGET_REACHED",
                "prospective_stop_evidence_sha256": "f" * 64,
                "implementation_sha": implementation_sha,
                "code_hash": f"git:{implementation_sha}",
                "contract_sha256": sha256(CONTRACT_PATH),
                "feature_scale_sha256": sha256(feature_scale),
                "prospective_amendment_sha256": sha256(AMENDMENT_PATH),
                "cohort_candidate_order_sha256": "fixture-cohort-hash",
                "candidate_outcomes_sha256": sha256(outcomes),
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return AMENDMENT_PATH, feature_scale, summary


@unittest.skipUnless(has_pinned_model_dependencies(), "requires pinned NumPy and scikit-learn")
class AceEvV2FitIntegrationTests(unittest.TestCase):
    def test_untouched_test_outcomes_cannot_change_fit_tau_or_membership(self) -> None:
        with tempfile.TemporaryDirectory(prefix="ace-ev-v2-fit-") as temp:
            root = Path(temp)
            first_outcomes = root / "first.jsonl"
            second_outcomes = root / "second.jsonl"
            first_output = root / "first-output"
            second_output = root / "second-output"
            implementation_sha = "a" * 40
            write_jsonl(first_outcomes, [row(index, 0.0) for index in range(1, 1001)])
            write_jsonl(second_outcomes, [row(index, -0.5) for index in range(1, 1001)])
            first_amendment, first_scale, first_summary = write_bound_sources(
                root / "first-sources", first_outcomes, implementation_sha=implementation_sha
            )
            second_amendment, second_scale, second_summary = write_bound_sources(
                root / "second-sources", second_outcomes, implementation_sha=implementation_sha
            )

            self.assertEqual(
                FIT.fit(
                    Namespace(
                        outcomes=first_outcomes,
                        contract=CONTRACT_PATH,
                        amendment=first_amendment,
                        feature_scale=first_scale,
                        summary=first_summary,
                        implementation_sha=implementation_sha,
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
                        amendment=second_amendment,
                        feature_scale=second_scale,
                        summary=second_summary,
                        implementation_sha=implementation_sha,
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
