#!/usr/bin/env python3
"""Deterministic fixtures for scripts/shadow_v2_live_calibration_audit.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "shadow_v2_live_calibration_audit.py"
CONTRACT_PATH = REPO_ROOT / "configs" / "rollout" / "shadow_v2_live_confirmed_calibration_contract.toml"
FIELD_MANIFEST_PATH = REPO_ROOT / "reports" / "selector" / "shadow_v2_live_calibration_schema_manifest.csv"
GATES_PATH = REPO_ROOT / "reports" / "selector" / "shadow_v2_acceptance_gates.csv"


def load_module():
    spec = importlib.util.spec_from_file_location("shadow_v2_live_calibration_audit", SCRIPT_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def write_jsonl(path: Path, rows: list[dict[str, object]]) -> None:
    path.write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


def build_valid_dataset(root: Path) -> None:
    files = [
        "live_calibration_manifest.json",
        "live_transaction_attempts.jsonl",
        "live_confirmed_entry_fills.jsonl",
        "live_confirmed_exit_fills.jsonl",
        "live_calibration_comparison.jsonl",
    ]
    (root / "live_calibration_manifest.json").write_text(
        json.dumps(
            {
                "schema": "live_calibration_manifest_v1",
                "calibration_dataset_id": "fixture-calibration-dataset",
                "created_at_wall_ms": 1_772_400_000_000,
                "simulation_contract_version": "shadow_burnin_simulation_v2_20260629",
                "dataset_status": "LIVE_CONFIRMED_CALIBRATION_DATASET",
                "source_mode": "FIXTURE_ONLY",
                "required_for_live_equivalence": True,
                "max_verdict_without_dataset": "SHADOW_V2_RESEARCH_GRADE_ONLY",
                "files": files,
            },
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    base_attempt = {
        "schema": "live_transaction_attempt_v1",
        "calibration_dataset_id": "fixture-calibration-dataset",
        "run_id": "fixture-run",
        "session_id": "fixture-session",
        "position_id": "fixture-position",
        "pool_id": "fixture-pool",
        "base_mint": "fixture-mint",
        "decision_ts_ms": 1_000,
        "submit_ts_ms": 1_012,
        "landing_ts_ms": 1_425,
        "decision_to_submit_ms": 12,
        "submit_to_land_ms": 413,
        "landing_slot": 42,
        "priority_fee_lamports": 1_000,
        "jito_tip_lamports": 2_000,
        "bundle_status": "LANDED",
        "compute_units_consumed": 88_000,
        "min_out": 950,
        "quote_price": 0.000000031,
    }
    write_jsonl(
        root / "live_transaction_attempts.jsonl",
        [
            {
                **base_attempt,
                "attempt_id": "attempt-entry-filled",
                "side": "BUY",
                "tx_signature": "entry_signature_111",
                "fill_status": "FILLED",
                "failure_mode": "NONE",
            },
            {
                **base_attempt,
                "attempt_id": "attempt-exit-filled",
                "side": "SELL",
                "tx_signature": "exit_signature_111",
                "fill_status": "FILLED",
                "failure_mode": "NONE",
            },
            {
                **base_attempt,
                "attempt_id": "attempt-failed",
                "side": "BUY",
                "tx_signature": "failed_signature_111",
                "fill_status": "FAILED",
                "failure_mode": "MIN_OUT_FAILURE",
            },
        ],
    )

    common_fill = {
        "calibration_dataset_id": "fixture-calibration-dataset",
        "run_id": "fixture-run",
        "session_id": "fixture-session",
        "position_id": "fixture-position",
        "pool_id": "fixture-pool",
        "base_mint": "fixture-mint",
        "fill_status": "FILLED",
        "decision_ts_ms": 1_000,
        "submit_ts_ms": 1_012,
        "landing_ts_ms": 1_425,
        "landing_slot": 42,
        "quote_price": 0.000000031,
        "fill_price": 0.000000032,
        "realized_slippage_bps": 12.0,
        "quote_fill_diff_bps": 10.0,
        "own_impact_bps": 4.0,
        "fee_bps": 100.0,
        "priority_fee_lamports": 1_000,
        "jito_tip_lamports": 2_000,
        "account_state_delay_ms": 25,
        "stream_delay_ms": 18,
        "pool_state_before_ref": "pool_state_sample_v2:before",
        "pool_state_after_ref": "pool_state_sample_v2:after",
        "confirmation_status": "CONFIRMED",
    }
    write_jsonl(
        root / "live_confirmed_entry_fills.jsonl",
        [
            {
                **common_fill,
                "schema": "live_confirmed_entry_fill_v1",
                "attempt_id": "attempt-entry-filled",
                "fill_id": "entry-fill-1",
                "tx_signature": "entry_signature_111",
                "amount_in_sol_lamports": 1_000_000_000,
                "amount_out_tokens_raw": 31_000_000_000,
            }
        ],
    )
    write_jsonl(
        root / "live_confirmed_exit_fills.jsonl",
        [
            {
                **common_fill,
                "schema": "live_confirmed_exit_fill_v1",
                "attempt_id": "attempt-exit-filled",
                "fill_id": "exit-fill-1",
                "tx_signature": "exit_signature_111",
                "amount_in_tokens_raw": 31_000_000_000,
                "amount_out_sol_lamports": 1_020_000_000,
            }
        ],
    )
    write_jsonl(
        root / "live_calibration_comparison.jsonl",
        [
            {
                "schema": "live_calibration_comparison_v1",
                "calibration_dataset_id": "fixture-calibration-dataset",
                "comparison_id": "comparison-1",
                "position_id": "fixture-position",
                "side": "BUY",
                "model_version": "shadow_v2_static_fill_v1",
                "simulated_fill_price": 0.000000031,
                "live_fill_price": 0.000000032,
                "model_error_bps": 10.0,
                "latency_bucket_ms": 500,
                "slippage_error_bps": 2.0,
                "outcome_match": True,
                "limitations": ["fixture_only"],
            }
        ],
    )


def audit_dataset(root: Path, require_dataset: bool = True) -> dict[str, object]:
    module = load_module()
    return module.audit(
        CONTRACT_PATH,
        FIELD_MANIFEST_PATH,
        GATES_PATH,
        root,
        require_dataset=require_dataset,
    )


def test_default_contract_ready_without_dataset() -> None:
    module = load_module()
    result = module.audit(
        CONTRACT_PATH,
        FIELD_MANIFEST_PATH,
        GATES_PATH,
        None,
        require_dataset=False,
    )
    assert result["status"] == "CONTRACT_READY"
    assert result["blockers"] == []
    assert result["pr14_calibration_gate_pass"] is False
    assert result["live_equivalence_grade_allowed"] is False


def test_valid_fixture_dataset_passes_pr14_gate() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_valid_dataset(root)
        result = audit_dataset(root)
        assert result["status"] == "PASS"
        assert result["blockers"] == []
        assert result["pr14_calibration_gate_pass"] is True
        assert result["dataset"]["status_counts"]["FAILED"] == 1


def test_missing_required_dataset_file_blocks() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_valid_dataset(root)
        (root / "live_confirmed_exit_fills.jsonl").unlink()
        result = audit_dataset(root)
        assert result["status"] == "BLOCKED"
        assert any("live_confirmed_exit_fills.jsonl" in blocker for blocker in result["blockers"])


def test_malformed_jsonl_blocks() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_valid_dataset(root)
        with (root / "live_transaction_attempts.jsonl").open("a", encoding="utf-8") as handle:
            handle.write("{not-json}\n")
        result = audit_dataset(root)
        assert result["status"] == "BLOCKED"
        assert any("malformed JSONL row" in blocker for blocker in result["blockers"])


def test_latency_mismatch_blocks() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_valid_dataset(root)
        path = root / "live_transaction_attempts.jsonl"
        rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
        rows[0]["decision_to_submit_ms"] = 99
        write_jsonl(path, rows)
        result = audit_dataset(root)
        assert result["status"] == "BLOCKED"
        assert any("decision_to_submit_ms does not match" in blocker for blocker in result["blockers"])


def test_missing_failure_mode_blocks_failed_attempt() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_valid_dataset(root)
        path = root / "live_transaction_attempts.jsonl"
        rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
        rows[2]["failure_mode"] = "UNKNOWN"
        write_jsonl(path, rows)
        result = audit_dataset(root)
        assert result["status"] == "BLOCKED"
        assert any("failure_mode must be explicit" in blocker for blocker in result["blockers"])


def test_require_dataset_blocks_absent_dataset() -> None:
    module = load_module()
    result = module.audit(
        CONTRACT_PATH,
        FIELD_MANIFEST_PATH,
        GATES_PATH,
        None,
        require_dataset=True,
    )
    assert result["status"] == "BLOCKED"
    assert "LIVE_CONFIRMED_CALIBRATION_DATASET_NOT_PROVIDED" in result["blockers"]


if __name__ == "__main__":
    test_default_contract_ready_without_dataset()
    test_valid_fixture_dataset_passes_pr14_gate()
    test_missing_required_dataset_file_blocks()
    test_malformed_jsonl_blocks()
    test_latency_mismatch_blocks()
    test_missing_failure_mode_blocks_failed_attempt()
    test_require_dataset_blocks_absent_dataset()
    print("shadow_v2_live_calibration_audit fixtures: PASS")
