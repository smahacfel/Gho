#!/usr/bin/env python3
"""Deterministic fixtures for scripts/shadow_v2_legacy_downgrade_audit.py."""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "shadow_v2_legacy_downgrade_audit.py"
MATRIX_PATH = REPO_ROOT / "reports" / "selector" / "shadow_v2_legacy_downgrade_matrix.csv"
DOC_PATH = REPO_ROOT / "PLANS" / "AUDYT" / "RAPORT_SHADOW_V2_LEGACY_DOWNGRADE_ENFORCEMENT_PR13_20260630.md"


def load_module():
    spec = importlib.util.spec_from_file_location("shadow_v2_legacy_downgrade_audit", SCRIPT_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_default_downgrade_matrix_passes() -> None:
    module = load_module()
    result = module.audit(MATRIX_PATH, [DOC_PATH])
    assert result["status"] == "PASS"
    assert result["v1_live_equivalent_allowed"] is False
    assert result["raw_jsonl_read"] is False
    assert result["labels"]["R51"] == "ACTIVE_PARTIAL_DIAGNOSTIC_ONLY"


def test_missing_required_family_blocks() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        tmp_matrix = Path(tmp) / "matrix.csv"
        shutil.copyfile(MATRIX_PATH, tmp_matrix)
        text = "\n".join(
            line for line in tmp_matrix.read_text(encoding="utf-8").splitlines() if not line.startswith("R51,")
        ) + "\n"
        tmp_matrix.write_text(text, encoding="utf-8")

        result = module.audit(tmp_matrix, [DOC_PATH])
        assert result["status"] == "BLOCKED"
        assert any("missing downgrade row: R51" in blocker for blocker in result["blockers"])


def test_live_equivalent_allowed_use_blocks() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        tmp_matrix = Path(tmp) / "matrix.csv"
        shutil.copyfile(MATRIX_PATH, tmp_matrix)
        text = tmp_matrix.read_text(encoding="utf-8")
        text = text.replace(
            "offline path-label diagnostic under Shadow V1 assumptions",
            "live-equivalent PnL proof",
        )
        tmp_matrix.write_text(text, encoding="utf-8")

        result = module.audit(tmp_matrix, [DOC_PATH])
        assert result["status"] == "BLOCKED"
        assert any("forbidden phrase" in blocker for blocker in result["blockers"])


if __name__ == "__main__":
    test_default_downgrade_matrix_passes()
    test_missing_required_family_blocks()
    test_live_equivalent_allowed_use_blocks()
    print("shadow_v2_legacy_downgrade_audit fixtures: PASS")
