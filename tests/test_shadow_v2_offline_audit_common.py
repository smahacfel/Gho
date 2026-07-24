#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import shadow_v2_offline_audit_common as common  # noqa: E402


def write_jsonl(path: Path, rows: list[dict | str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            if isinstance(row, str):
                fh.write(row)
            else:
                fh.write(json.dumps(row, sort_keys=True))
            fh.write("\n")


def row(position_id: str) -> dict:
    return {"envelope": {"position_id": position_id}, "position_id": position_id}


def ids(rows: list[dict]) -> list[str]:
    return [str(item["position_id"]) for item in rows]


class ShadowV2OfflineAuditCommonRotationTest(unittest.TestCase):
    def write_rotated_fixture(self, scope: Path, artifact_name: str) -> None:
        stem = artifact_name[: -len(".jsonl")]
        write_jsonl(scope / f"{stem}.part-000001.jsonl", [row("pos-a"), row("pos-b")])
        write_jsonl(scope / f"{stem}.part-000002.jsonl", [row("pos-c")])
        write_jsonl(scope / artifact_name, [row("pos-d")])

    def test_iter_canonical_rows_reads_rotated_parts_before_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            self.write_rotated_fixture(scope, "shadow_position_event_v2.jsonl")

            rows = [
                item
                for item, malformed in common.iter_canonical_rows(scope)
                if not malformed and item is not None
            ]

        self.assertEqual(ids(rows), ["pos-a", "pos-b", "pos-c", "pos-d"])

    def test_iter_replay_rows_reads_rotated_parts_before_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            self.write_rotated_fixture(scope, "shadow_replay_v2.jsonl")

            rows = [
                item
                for item, malformed in common.iter_replay_rows(scope)
                if not malformed and item is not None
            ]

        self.assertEqual(ids(rows), ["pos-a", "pos-b", "pos-c", "pos-d"])

    def test_iter_lifecycle_rows_reads_rotated_parts_before_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            self.write_rotated_fixture(scope, "shadow_lifecycle_v2.jsonl")

            rows = [
                item
                for item, malformed in common.iter_lifecycle_rows(scope)
                if not malformed and item is not None
            ]

        self.assertEqual(ids(rows), ["pos-a", "pos-b", "pos-c", "pos-d"])

    def test_iter_density_rows_reads_rotated_parts_before_base(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            self.write_rotated_fixture(scope, "shadow_path_density_v2.jsonl")

            rows = [
                item
                for item, malformed in common.iter_density_rows(scope)
                if not malformed and item is not None
            ]

        self.assertEqual(ids(rows), ["pos-a", "pos-b", "pos-c", "pos-d"])

    def test_no_parts_keeps_legacy_single_file_behavior(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            write_jsonl(scope / "shadow_position_event_v2.jsonl", [row("pos-d")])

            paths = common.artifact_jsonl_paths(scope, "shadow_position_event_v2.jsonl")
            rows, malformed = common.canonical_rows(scope)

        self.assertEqual([path.name for path in paths], ["shadow_position_event_v2.jsonl"])
        self.assertEqual(ids(rows), ["pos-d"])
        self.assertEqual(malformed, 0)

    def test_malformed_row_count_includes_rotated_parts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            write_jsonl(
                scope / "shadow_position_event_v2.part-000001.jsonl",
                [row("pos-a"), "{bad-json"],
            )
            write_jsonl(
                scope / "shadow_position_event_v2.part-000002.jsonl",
                ["[]"],
            )
            write_jsonl(scope / "shadow_position_event_v2.jsonl", [row("pos-d")])

            rows, malformed = common.canonical_rows(scope)

        self.assertEqual(ids(rows), ["pos-a", "pos-d"])
        self.assertEqual(malformed, 2)


if __name__ == "__main__":
    unittest.main()
