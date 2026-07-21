#!/usr/bin/env python3
"""Unit tests for the line-scoped Clippy diagnostic gate."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


SCRIPT_PATH = Path(__file__).with_name("guard_diff_scoped_clippy.py")
SPEC = importlib.util.spec_from_file_location("guard_diff_scoped_clippy", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def diagnostic(*, path: str, start: int, end: int) -> object:
    span = MODULE.PrimarySpan(path=path, line_start=start, line_end=end)
    return MODULE.Diagnostic(
        level="warning",
        code="clippy::example",
        message="example diagnostic",
        paths=(path,),
        primary_spans=(span,),
    )


class DiffScopedClippyTests(unittest.TestCase):
    def test_preexisting_diagnostic_outside_changed_hunk_is_allowed(self) -> None:
        item = diagnostic(path="ghost-brain/src/lib.rs", start=12, end=12)
        self.assertFalse(
            MODULE.diagnostic_intersects_changed_lines(
                item, {"ghost-brain/src/lib.rs": ((80, 88),)}
            )
        )

    def test_primary_span_overlapping_changed_hunk_is_rejected(self) -> None:
        item = diagnostic(path="ghost-brain/src/lib.rs", start=84, end=86)
        self.assertTrue(
            MODULE.diagnostic_intersects_changed_lines(
                item, {"ghost-brain/src/lib.rs": ((80, 84),)}
            )
        )

    def test_diagnostic_on_another_file_is_not_attributed_to_the_diff(self) -> None:
        item = diagnostic(path="ghost-core/src/lib.rs", start=10, end=10)
        self.assertFalse(
            MODULE.diagnostic_intersects_changed_lines(
                item, {"ghost-brain/src/lib.rs": ((1, 20),)}
            )
        )


if __name__ == "__main__":
    unittest.main()
