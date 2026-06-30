#!/usr/bin/env python3
"""Deterministic fixtures for scripts/shadow_v2_validation_burnin_plan_audit.py."""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "shadow_v2_validation_burnin_plan_audit.py"
PLAN_PATH = REPO_ROOT / "configs" / "rollout" / "shadow_v2_fidelity_validation_burnin_plan.toml"
GATES_PATH = REPO_ROOT / "reports" / "selector" / "shadow_v2_acceptance_gates.csv"


def load_module():
    spec = importlib.util.spec_from_file_location("shadow_v2_validation_burnin_plan_audit", SCRIPT_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_default_plan_passes() -> None:
    module = load_module()
    result = module.validate_plan(PLAN_PATH, GATES_PATH)
    assert result["status"] == "PASS"
    assert result["blockers"] == []
    assert result["run_start_allowed"] is False
    assert result["runtime_approval"] is False
    assert result["strategy_proof_enabled"] is False


def test_strategy_proof_flag_blocks_plan() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        tmp_plan = Path(tmp) / "plan.toml"
        text = PLAN_PATH.read_text(encoding="utf-8")
        tmp_plan.write_text(text.replace("strategy_proof_enabled = false", "strategy_proof_enabled = true"), encoding="utf-8")

        result = module.validate_plan(tmp_plan, GATES_PATH)
        assert result["status"] == "BLOCKED"
        assert any("strategy_proof_enabled" in blocker for blocker in result["blockers"])


def test_missing_required_gate_blocks_plan() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        tmp_gates = Path(tmp) / "gates.csv"
        shutil.copyfile(GATES_PATH, tmp_gates)
        text = tmp_gates.read_text(encoding="utf-8")
        text = "\n".join(
            line for line in text.splitlines() if not line.startswith("GATE_PR12_PLAN_CONTRACT,")
        ) + "\n"
        tmp_gates.write_text(text, encoding="utf-8")

        result = module.validate_plan(PLAN_PATH, tmp_gates)
        assert result["status"] == "BLOCKED"
        assert any("GATE_PR12_PLAN_CONTRACT" in blocker for blocker in result["blockers"])


if __name__ == "__main__":
    test_default_plan_passes()
    test_strategy_proof_flag_blocks_plan()
    test_missing_required_gate_blocks_plan()
    print("shadow_v2_validation_burnin_plan_audit fixtures: PASS")
