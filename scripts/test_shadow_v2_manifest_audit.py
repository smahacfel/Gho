#!/usr/bin/env python3
"""Deterministic fixtures for scripts/shadow_v2_manifest_audit.py."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "shadow_v2_manifest_audit.py"
ARTIFACT_CONTRACT = REPO_ROOT / "reports" / "selector" / "shadow_v2_manifest_artifact_contract.csv"


def load_module():
    spec = importlib.util.spec_from_file_location("shadow_v2_manifest_audit", SCRIPT_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def write_fixture_scope(root: Path) -> None:
    (root / "pre_run_manifest.json").write_text(
        '{"schema":"shadow_v2_evidence_manifest_v1","manifest_phase":"pre_run"}\n',
        encoding="utf-8",
    )
    (root / "post_run_manifest.json").write_text(
        '{"schema":"shadow_v2_evidence_manifest_v1","manifest_phase":"post_run"}\n',
        encoding="utf-8",
    )
    (root / "shadow_position_event_v2.jsonl").write_text(
        '{"schema":"shadow_position_event_v2","event_id":"event-1"}\n'
        '{"envelope":{"schema":"shadow_position_event_v2"},"event_id":"event-2"}\n',
        encoding="utf-8",
    )
    (root / "shadow_replay_v2.jsonl").write_text(
        '{"schema":"shadow_replay_v2","position_id":"pos-1"}\n',
        encoding="utf-8",
    )
    (root / "shadow_lifecycle_v2.jsonl").write_text(
        '{"schema":"shadow_lifecycle_v2","position_id":"pos-1"}\n',
        encoding="utf-8",
    )
    (root / "shadow_path_density_v2.jsonl").write_text(
        '{"schema":"shadow_path_density_v2","position_id":"pos-1"}\n',
        encoding="utf-8",
    )
    (root / "shadow_v2_manifest_report.csv").write_text(
        "relative_path,size_bytes,line_count,sha256,status\n",
        encoding="utf-8",
    )


def test_complete_manifest_passes() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture_scope(root)

        manifest, blockers = module.build_manifest(
            scope_root=root,
            manifest_phase="post_run",
            run_id="fixture-run",
            artifact_contract=ARTIFACT_CONTRACT,
            max_sha_bytes=1024 * 1024,
        )

        assert blockers == []
        assert manifest["status"] == "PASS"
        assert manifest["artifact_count"] == 7
        assert manifest["required_artifacts_missing"] == []
        assert manifest["raw_jsonl_git_staging_allowed"] is False
        assert manifest["schema_coverage"]["shadow_position_event_v2"] == 2
        assert all(entry["sha256_status"] == "OK" for entry in manifest["artifacts"])


def test_missing_required_artifact_blocks_manifest() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture_scope(root)
        (root / "shadow_lifecycle_v2.jsonl").unlink()

        manifest, blockers = module.build_manifest(
            scope_root=root,
            manifest_phase="post_run",
            run_id="fixture-run",
            artifact_contract=ARTIFACT_CONTRACT,
            max_sha_bytes=1024 * 1024,
        )

        assert manifest["status"] == "BLOCKED"
        assert "shadow_lifecycle_v2.jsonl" in manifest["required_artifacts_missing"]
        assert any("missing required artifact" in blocker for blocker in blockers)


def test_malformed_jsonl_blocks_manifest() -> None:
    module = load_module()
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_fixture_scope(root)
        with (root / "shadow_replay_v2.jsonl").open("a", encoding="utf-8") as handle:
            handle.write("{not json}\n")

        manifest, blockers = module.build_manifest(
            scope_root=root,
            manifest_phase="post_run",
            run_id="fixture-run",
            artifact_contract=ARTIFACT_CONTRACT,
            max_sha_bytes=1024 * 1024,
        )

        replay_entry = next(
            entry
            for entry in manifest["artifacts"]
            if entry["relative_path"] == "shadow_replay_v2.jsonl"
        )
        assert manifest["status"] == "BLOCKED"
        assert replay_entry["malformed_jsonl_rows"] == 1
        assert any("BLOCKED_MALFORMED_JSONL" in blocker for blocker in blockers)


if __name__ == "__main__":
    test_complete_manifest_passes()
    test_missing_required_artifact_blocks_manifest()
    test_malformed_jsonl_blocks_manifest()
    print("shadow_v2_manifest_audit fixtures: PASS")
