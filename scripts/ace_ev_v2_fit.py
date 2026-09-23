#!/usr/bin/env python3
"""Fit the one frozen ACE-EV V2 PROSPECTIVE_1000 Huber model offline only.

This script consumes exactly the run-bound terminal cohort emitted by
``ace_ev_v2_probe``.  It has no RPC, no capture, no Event Bus, and no
authority over a running launcher.  TRAIN (1-400) is the only fitting
partition.  The middle 200 rows freeze ``tau`` from predictions; the final
400 rows remain untouched until the one final evaluation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import sys
import warnings
from pathlib import Path
from typing import Any

# Must be set before NumPy / scikit-learn imports so the frozen model is not
# dependent on host BLAS parallelism.
for _thread_var in (
    "OPENBLAS_NUM_THREADS",
    "OMP_NUM_THREADS",
    "MKL_NUM_THREADS",
    "VECLIB_MAXIMUM_THREADS",
    "NUMEXPR_NUM_THREADS",
):
    os.environ[_thread_var] = "1"


SCHEMA = "ace_ev_v2_huber_fit_v1"
PREDICTIONS_SCHEMA = "ace_ev_v2_test_predictions_v1"
FEATURE_COUNT = 7
TRAIN_ROWS = 400
THRESHOLD_ROWS = 200
TEST_ROWS = 400
EXPECTED_ROWS = TRAIN_ROWS + THRESHOLD_ROWS + TEST_ROWS
EPSILON = 1.35
ALPHA = 1.0
TOL = 1e-5
MAX_ITER = 1000
PINNED_NUMPY_VERSION = "2.3.2"
PINNED_SCIKIT_LEARN_VERSION = "1.7.1"
AMENDMENT_SCHEMA = "ace_ev_v2_prospective_1000_amendment_v1"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, raw in enumerate(handle, 1):
            if not raw.endswith("\n"):
                raise ValueError(f"{path}:{line_number}: non-final JSONL line is not newline-terminated")
            if not raw.strip():
                raise ValueError(f"{path}:{line_number}: blank JSONL row")
            value = json.loads(raw)
            if not isinstance(value, dict):
                raise ValueError(f"{path}:{line_number}: row is not an object")
            rows.append(value)
    return rows


def load_contract(path: Path) -> tuple[bytes, dict[str, Any]]:
    data = path.read_bytes()
    value = json.loads(data)
    if value.get("schema") != "ace_ev_v2_contract_v1":
        raise ValueError("ACE-EV V2 contract schema mismatch")
    return data, value


def load_prospective_sources(
    args: argparse.Namespace, contract_bytes: bytes, outcomes_bytes: bytes
) -> tuple[bytes, bytes, dict[str, Any]]:
    summary_bytes = args.summary.read_bytes()
    summary = json.loads(summary_bytes)
    if not isinstance(summary, dict) or summary.get("schema") != "ace_ev_v2_summary_v1":
        raise ValueError("source summary schema mismatch")
    if summary.get("capture_kind") != "prospective_1000":
        raise ValueError("source summary capture_kind is not prospective_1000")
    if summary.get("capture_status") != "VALID_CAPTURE":
        raise ValueError("source summary capture_status is not VALID_CAPTURE")
    if summary.get("terminal_status") != "ACE_EV_V2_OUTCOMES_READY_FOR_FIT":
        raise ValueError("source summary terminal_status is not fit-ready")
    if summary.get("prospective_terminalization") != "TARGET_REACHED":
        raise ValueError("source summary prospective terminalization is not TARGET_REACHED")
    if not isinstance(summary.get("prospective_stop_evidence_sha256"), str) or not summary[
        "prospective_stop_evidence_sha256"
    ]:
        raise ValueError("source summary prospective stop-evidence hash missing")
    if summary.get("implementation_sha") != args.implementation_sha:
        raise ValueError("source summary implementation_sha mismatch")
    if summary.get("code_hash") != f"git:{args.implementation_sha}":
        raise ValueError("source summary code_hash mismatch")
    if summary.get("contract_sha256") != sha256_bytes(contract_bytes):
        raise ValueError("source summary contract hash mismatch")
    scale_bytes = args.feature_scale.read_bytes()
    if summary.get("feature_scale_sha256") != sha256_bytes(scale_bytes):
        raise ValueError("source summary feature-scale hash mismatch")
    amendment_bytes = args.amendment.read_bytes()
    amendment = json.loads(amendment_bytes)
    if not isinstance(amendment, dict) or amendment.get("schema") != AMENDMENT_SCHEMA:
        raise ValueError("prospective amendment schema mismatch")
    if amendment.get("base_contract_sha256") != sha256_bytes(contract_bytes):
        raise ValueError("prospective amendment contract hash mismatch")
    if summary.get("prospective_amendment_sha256") != sha256_bytes(amendment_bytes):
        raise ValueError("source summary amendment hash mismatch")
    if not isinstance(summary.get("cohort_candidate_order_sha256"), str) or not summary[
        "cohort_candidate_order_sha256"
    ]:
        raise ValueError("source summary cohort hash missing")
    if summary.get("candidate_outcomes_sha256") != sha256_bytes(outcomes_bytes):
        raise ValueError("source summary outcomes hash mismatch")
    return summary_bytes, scale_bytes, amendment


def require_terminal_rows(rows: list[dict[str, Any]]) -> None:
    if len(rows) != EXPECTED_ROWS:
        raise ValueError(f"expected exactly {EXPECTED_ROWS} terminal rows, got {len(rows)}")
    expected_splits = (["TRAIN"] * TRAIN_ROWS) + (["THRESHOLD_CALIBRATION"] * THRESHOLD_ROWS) + (["UNTOUCHED_TEST"] * TEST_ROWS)
    previous_order: tuple[Any, ...] | None = None
    for index, (row, expected_split) in enumerate(zip(rows, expected_splits), 1):
        if row.get("schema") != "ace_ev_v2_candidate_outcome_v1":
            raise ValueError(f"row {index}: candidate outcome schema mismatch")
        if row.get("enrollment_index") != index:
            raise ValueError(f"row {index}: enrollment order is not contiguous")
        if row.get("split") != expected_split:
            raise ValueError(f"row {index}: expected split {expected_split}, got {row.get('split')}")
        features = row.get("normalized_features")
        if not isinstance(features, list) or len(features) != FEATURE_COUNT:
            raise ValueError(f"row {index}: normalized F1-F7 unavailable")
        if not all(isinstance(value, (int, float)) and float(value) == float(value) for value in features):
            raise ValueError(f"row {index}: non-finite normalized feature")
        target = row.get("terminal_net_pnl_sol")
        if not isinstance(target, (int, float)) or float(target) != float(target):
            raise ValueError(f"row {index}: terminal_net_pnl_sol unavailable")
        order = row.get("candidate_order")
        if not isinstance(order, dict):
            raise ValueError(f"row {index}: candidate_order missing")
        order_key = (
            order.get("decision_ingress_cutoff_ms"),
            order.get("birth_ts_ms"),
            order.get("event_slot"),
            order.get("bonding_curve"),
            order.get("base_mint"),
        )
        if any(value is None for value in order_key):
            raise ValueError(f"row {index}: candidate_order incomplete")
        if previous_order is not None and order_key < previous_order:
            raise ValueError(f"row {index}: chronological candidate_order is not monotonic")
        previous_order = order_key


def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def median(values: list[float]) -> float:
    ordered = sorted(values)
    midpoint = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[midpoint]
    return (ordered[midpoint - 1] + ordered[midpoint]) / 2.0


def hit_rate(rows: list[dict[str, Any]]) -> float:
    return sum(bool(row["profit17_hit"]) for row in rows) / len(rows)


def create_output_dir(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.mkdir(exist_ok=False)


def write_new_json(path: Path, value: Any) -> None:
    encoded = json.dumps(value, sort_keys=True, indent=2, allow_nan=False).encode("utf-8") + b"\n"
    with path.open("xb") as handle:
        handle.write(encoded)
        handle.flush()
        os.fsync(handle.fileno())


def write_new_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("xb") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True, allow_nan=False).encode("utf-8") + b"\n")
        handle.flush()
        os.fsync(handle.fileno())


def route_loss_dominates(rows: list[dict[str, Any]]) -> tuple[bool, float]:
    all_losses = sum(max(-float(row["terminal_net_pnl_sol"]), 0.0) for row in rows)
    route_losses = sum(
        max(-float(row["terminal_net_pnl_sol"]), 0.0)
        for row in rows
        if row.get("terminal_status") == "POST_ENTRY_UNSUPPORTED_ROUTE_LOSS_FLOOR"
    )
    share = route_losses / all_losses if all_losses > 0.0 else 0.0
    return share > 0.5, share


def fit(args: argparse.Namespace) -> int:
    try:
        import numpy as np
        import sklearn
        from sklearn.exceptions import ConvergenceWarning
        from sklearn.linear_model import HuberRegressor
    except ImportError as error:
        print(
            "ACE_EV_V2_MODEL_DEPENDENCY_MISSING: install pinned numpy and scikit-learn "
            f"before fitting: {error}",
            file=sys.stderr,
        )
        return 2

    contract_bytes, contract = load_contract(args.contract)
    outcomes_bytes = args.outcomes.read_bytes()
    summary_bytes, scale_bytes, amendment = load_prospective_sources(
        args, contract_bytes, outcomes_bytes
    )
    if np.__version__ != PINNED_NUMPY_VERSION or sklearn.__version__ != PINNED_SCIKIT_LEARN_VERSION:
        print(
            "ACE_EV_V2_MODEL_DEPENDENCY_VERSION_MISMATCH: "
            f"numpy={np.__version__} expected={PINNED_NUMPY_VERSION} "
            f"scikit_learn={sklearn.__version__} expected={PINNED_SCIKIT_LEARN_VERSION}",
            file=sys.stderr,
        )
        return 2
    rows = read_jsonl(args.outcomes)
    require_terminal_rows(rows)
    if amendment.get("target_terminal_outcomes") != EXPECTED_ROWS:
        raise ValueError("prospective amendment target does not equal 1000")
    create_output_dir(args.output_dir)

    train = rows[:TRAIN_ROWS]
    threshold_rows = rows[TRAIN_ROWS : TRAIN_ROWS + THRESHOLD_ROWS]
    test = rows[TRAIN_ROWS + THRESHOLD_ROWS :]
    x_train = np.asarray([row["normalized_features"] for row in train], dtype=float)
    y_train = np.asarray([row["terminal_net_pnl_sol"] for row in train], dtype=float)
    x_threshold = np.asarray([row["normalized_features"] for row in threshold_rows], dtype=float)
    x_test = np.asarray([row["normalized_features"] for row in test], dtype=float)

    model = HuberRegressor(
        epsilon=EPSILON,
        alpha=ALPHA,
        fit_intercept=True,
        tol=TOL,
        max_iter=MAX_ITER,
        warm_start=False,
    )
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always", ConvergenceWarning)
        model.fit(x_train, y_train)
    convergence_warnings = [str(item.message) for item in caught if issubclass(item.category, ConvergenceWarning)]
    if convergence_warnings:
        report = {
            "schema": SCHEMA,
            "terminal_status": "ACE_EV_V2_INCONCLUSIVE",
            "reason": "huber_convergence_warning",
            "convergence_warnings": convergence_warnings,
            "source_outcomes_sha256": sha256_bytes(outcomes_bytes),
            "contract_sha256": sha256_bytes(contract_bytes),
            "python_version": sys.version,
            "platform": platform.platform(),
            "numpy_version": np.__version__,
            "scikit_learn_version": sklearn.__version__,
            "model": {
                "kind": "HuberRegressor",
                "epsilon": EPSILON,
                "alpha": ALPHA,
                "fit_intercept": True,
                "tol": TOL,
                "max_iter": MAX_ITER,
                "warm_start": False,
                "blas_threads": 1,
            },
        }
        write_new_json(args.output_dir / "model_report_v1.json", report)
        print("ACE_EV_V2_INCONCLUSIVE reason=huber_convergence_warning")
        return 0

    threshold_predictions = model.predict(x_threshold)
    tau = max(0.0, float(np.quantile(threshold_predictions, 0.75, method="linear")))
    test_predictions = model.predict(x_test)
    selected_rows = [row for row, prediction in zip(test, test_predictions) if float(prediction) >= tau]
    rest_rows = [row for row, prediction in zip(test, test_predictions) if float(prediction) < tau]
    selected_pnl = [float(row["terminal_net_pnl_sol"]) for row in selected_rows]
    rest_pnl = [float(row["terminal_net_pnl_sol"]) for row in rest_rows]
    selected_mean = mean(selected_pnl) if selected_pnl else None
    rest_mean = mean(rest_pnl) if rest_pnl else None
    selected_median = median(selected_pnl) if selected_pnl else None
    rest_median = median(rest_pnl) if rest_pnl else None
    selected_hit_rate = hit_rate(selected_rows) if selected_rows else None
    rest_hit_rate = hit_rate(rest_rows) if rest_rows else None
    stress_selected_pnl = [float(row["stress_latency_1s"]["terminal_net_pnl_sol"]) for row in selected_rows]
    stress_rest_pnl = [float(row["stress_latency_1s"]["terminal_net_pnl_sol"]) for row in rest_rows]
    stress_selected_mean = mean(stress_selected_pnl) if stress_selected_pnl else None
    stress_rest_mean = mean(stress_rest_pnl) if stress_rest_pnl else None
    positive_selected_pnl = [max(value, 0.0) for value in selected_pnl]
    positive_sum = sum(positive_selected_pnl)
    top_1_positive_share = max(positive_selected_pnl, default=0.0) / positive_sum if positive_sum > 0.0 else None
    top_3_positive_share = (
        sum(sorted(positive_selected_pnl, reverse=True)[:3]) / positive_sum
        if positive_sum > 0.0
        else None
    )
    test_entry_filled_count = sum(row.get("entry_status") == "ENTRY_FILLED" for row in test)
    test_exit_filled_count = sum(row.get("terminal_status") == "EXIT_FILLED" for row in test)
    selected_entry_filled_count = sum(row.get("entry_status") == "ENTRY_FILLED" for row in selected_rows)
    selected_exit_filled_count = sum(row.get("terminal_status") == "EXIT_FILLED" for row in selected_rows)
    adequate_executable_exposure = test_entry_filled_count >= 60 and test_exit_filled_count >= 25

    positive = (
        adequate_executable_exposure
        and len(selected_rows) >= 80
        and selected_entry_filled_count >= 20
        and selected_exit_filled_count >= 10
        and selected_mean is not None
        and rest_mean is not None
        and selected_median is not None
        and rest_median is not None
        and selected_hit_rate is not None
        and rest_hit_rate is not None
        and stress_selected_mean is not None
        and stress_rest_mean is not None
        and selected_mean > 0.0
        and selected_mean > rest_mean
        and selected_median >= rest_median
        and selected_hit_rate > rest_hit_rate
        and top_1_positive_share is not None
        and top_1_positive_share <= 0.25
        and top_3_positive_share is not None
        and top_3_positive_share <= 0.5
        and stress_selected_mean > stress_rest_mean
    )
    falsified = (
        adequate_executable_exposure
        and len(selected_rows) >= 20
        and selected_mean is not None
        and rest_mean is not None
        and selected_median is not None
        and rest_median is not None
        and selected_hit_rate is not None
        and rest_hit_rate is not None
        and selected_mean <= 0.0
        and selected_mean - rest_mean <= 0.0
        and selected_median <= rest_median
        and selected_hit_rate <= rest_hit_rate
    )
    _, route_loss_share = route_loss_dominates(test)
    if not adequate_executable_exposure:
        terminal_status = "ACE_EV_V2_INCONCLUSIVE"
        subtype = "insufficient_executable_exposure"
    elif positive:
        terminal_status = "ACE_EV_V2_POSITIVE_SIGNAL"
        subtype = None
    elif falsified:
        terminal_status = "ACE_EV_V2_FALSIFIED"
        dominant, _ = route_loss_dominates(test)
        subtype = "unsupported_route_dominant" if dominant else None
    else:
        terminal_status = "ACE_EV_V2_INCONCLUSIVE"
        subtype = None

    prediction_rows: list[dict[str, Any]] = []
    for row, prediction in zip(test, test_predictions):
        prediction_rows.append(
            {
                "schema": PREDICTIONS_SCHEMA,
                "enrollment_index": row["enrollment_index"],
                "base_mint": row["base_mint"],
                "candidate_order": row["candidate_order"],
                "predicted_robust_net_pnl_sol": float(prediction),
                "tau": tau,
                "selected": bool(float(prediction) >= tau),
                "terminal_net_pnl_sol": row["terminal_net_pnl_sol"],
                "stress_terminal_net_pnl_sol": row["stress_latency_1s"]["terminal_net_pnl_sol"],
                "profit17_hit": row["profit17_hit"],
                "terminal_status": row["terminal_status"],
            }
        )

    report = {
        "schema": SCHEMA,
        "terminal_status": terminal_status,
        "terminal_status_subtype": subtype,
        "source_outcomes_sha256": sha256_bytes(outcomes_bytes),
        "source_summary_sha256": sha256_bytes(summary_bytes),
        "source_feature_scale_sha256": sha256_bytes(scale_bytes),
        "source_amendment_sha256": sha256_bytes(args.amendment.read_bytes()),
        "contract_sha256": sha256_bytes(contract_bytes),
        "python_version": sys.version,
        "platform": platform.platform(),
        "numpy_version": np.__version__,
        "scikit_learn_version": sklearn.__version__,
        "model": {
            "kind": "HuberRegressor",
            "target": "terminal_net_pnl_sol",
            "prediction": "predicted_robust_net_pnl_sol",
            "epsilon": EPSILON,
            "alpha": ALPHA,
            "fit_intercept": True,
            "tol": TOL,
            "max_iter": MAX_ITER,
            "warm_start": False,
            "blas_threads": 1,
            "coef": [float(value) for value in model.coef_],
            "intercept": float(model.intercept_),
            "n_iter": int(model.n_iter_),
        },
        "splits": {"train": TRAIN_ROWS, "threshold_calibration": THRESHOLD_ROWS, "untouched_test": TEST_ROWS},
        "threshold": {"tau": tau, "formula": "max(0, p75(validation_predictions))"},
        "threshold_calibration_outcomes_used_for_fit_or_tau": False,
        "test_outcomes_used_for_fit_or_tau": False,
        "test": {
            "selected_count": len(selected_rows),
            "rest_count": len(rest_rows),
            "selected_mean_net_pnl_sol": selected_mean,
            "rest_mean_net_pnl_sol": rest_mean,
            "delta_mean_net_pnl_sol": None if selected_mean is None or rest_mean is None else selected_mean - rest_mean,
            "selected_median_net_pnl_sol": selected_median,
            "rest_median_net_pnl_sol": rest_median,
            "selected_profit17_hit_rate": selected_hit_rate,
            "rest_profit17_hit_rate": rest_hit_rate,
            "top_1_positive_pnl_share": top_1_positive_share,
            "top_3_positive_pnl_share": top_3_positive_share,
            "entry_filled_count": test_entry_filled_count,
            "exit_filled_count": test_exit_filled_count,
            "selected_entry_filled_count": selected_entry_filled_count,
            "selected_exit_filled_count": selected_exit_filled_count,
            "adequate_executable_exposure": adequate_executable_exposure,
            "stress_latency_1s_selected_mean_net_pnl_sol": stress_selected_mean,
            "stress_latency_1s_rest_mean_net_pnl_sol": stress_rest_mean,
            "unsupported_route_loss_share_of_test_losses": route_loss_share,
        },
        "convergence_warnings": [],
    }
    write_new_json(args.output_dir / "model_report_v1.json", report)
    write_new_jsonl(args.output_dir / "test_predictions_v1.jsonl", prediction_rows)
    print(terminal_status)
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--outcomes", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--amendment", type=Path, required=True)
    parser.add_argument("--feature-scale", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    parser.add_argument("--implementation-sha", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args()


if __name__ == "__main__":
    try:
        raise SystemExit(fit(parse_args()))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"ACE_EV_V2_MODEL_INPUT_INVALID: {error}", file=sys.stderr)
        raise SystemExit(2)
