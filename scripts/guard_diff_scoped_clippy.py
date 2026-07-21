#!/usr/bin/env python3
"""Fail a PR when Clippy introduces diagnostics in its Rust diff.

The repository has a documented full-workspace Clippy baseline waiver.  That
waiver must never hide a warning introduced by a PR, nor a warning whose
*primary span* intersects a changed Rust line.  This guard compares
machine-readable Clippy output from the base revision and the candidate head
without passing any global `-A` lint suppressions.
"""

from __future__ import annotations

import argparse
import json
import os
from dataclasses import dataclass
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Iterable


PACKAGE_PREFIXES: tuple[tuple[str, str], ...] = (
    ("ghost-core/", "ghost-core"),
    ("ghost-brain/", "ghost-brain"),
    ("ghost-launcher/", "ghost-launcher"),
    ("gui-backend/", "gui-backend"),
    ("off-chain/collector/", "ghost-collector"),
    ("off-chain/components/seer/", "seer"),
    ("off-chain/components/trigger/", "trigger"),
)


class GateError(RuntimeError):
    """A command or repository precondition prevented a trustworthy check."""


@dataclass(frozen=True)
class PrimarySpan:
    path: str
    line_start: int
    line_end: int


@dataclass(frozen=True)
class Diagnostic:
    level: str
    code: str | None
    message: str
    paths: tuple[str, ...]
    primary_spans: tuple[PrimarySpan, ...]

    @property
    def identity(self) -> tuple[str, str | None, str, tuple[str, ...]]:
        # Line/column positions intentionally do not participate: a harmless
        # location shift must not look like a newly introduced diagnostic.
        return (self.level, self.code, self.message, self.paths)


def run(command: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def checked(command: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> str:
    result = run(command, cwd=cwd, env=env)
    if result.returncode != 0:
        raise GateError(
            "command failed:\n"
            f"  {' '.join(command)}\n"
            f"  cwd: {cwd}\n"
            f"  stdout:\n{result.stdout}\n"
            f"  stderr:\n{result.stderr}"
        )
    return result.stdout


def relative_path(worktree: Path, file_name: str) -> str:
    path = Path(file_name)
    if path.is_absolute():
        try:
            return path.resolve().relative_to(worktree.resolve()).as_posix()
        except ValueError:
            return path.as_posix()
    return path.as_posix()


def parse_diagnostics(output: str, worktree: Path) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    for line in output.splitlines():
        try:
            payload = json.loads(line)
        except json.JSONDecodeError as error:
            raise GateError(f"cargo emitted non-JSON output on stdout: {line!r} ({error})") from error
        if payload.get("reason") != "compiler-message":
            continue
        message = payload.get("message", {})
        level = message.get("level")
        if level not in {"warning", "error"}:
            continue
        primary_spans = tuple(
            sorted(
                {
                    PrimarySpan(
                        path=relative_path(worktree, span["file_name"]),
                        line_start=max(int(span.get("line_start") or 1), 1),
                        line_end=max(
                            int(span.get("line_end") or span.get("line_start") or 1),
                            int(span.get("line_start") or 1),
                        ),
                    )
                    for span in message.get("spans", [])
                    if span.get("is_primary") and span.get("file_name")
                },
                key=lambda span: (span.path, span.line_start, span.line_end),
            )
        )
        primary_paths = tuple(sorted({span.path for span in primary_spans}))
        code = message.get("code")
        diagnostics.append(
            Diagnostic(
                level=level,
                code=code.get("code") if isinstance(code, dict) else None,
                message=message.get("message", "<missing diagnostic message>"),
                paths=primary_paths,
                primary_spans=primary_spans,
            )
        )
    return diagnostics


def packages_for_paths(paths: Iterable[str]) -> list[str]:
    packages: set[str] = set()
    for path in paths:
        for prefix, package in PACKAGE_PREFIXES:
            if path.startswith(prefix):
                packages.add(package)
                break
        else:
            raise GateError(
                f"changed Rust source {path!r} is outside the known workspace package map; "
                "extend PACKAGE_PREFIXES before trusting this gate"
            )
    return sorted(packages)


def clippy_diagnostics(worktree: Path, packages: list[str], target_dir: Path) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_INCREMENTAL"] = "0"
    for package in packages:
        command = [
            "cargo",
            "clippy",
            "-p",
            package,
            "--lib",
            "--tests",
            "--no-deps",
            "--message-format=json",
        ]
        result = run(command, cwd=worktree, env=env)
        diagnostics_for_package = parse_diagnostics(result.stdout, worktree)
        # This repository has a documented baseline Clippy *error* outside
        # the PR diff (`clippy::never_loop`).  Do not suppress it: retain the
        # machine-readable diagnostic and compare base/head.  A non-zero exit
        # without any compiler error is instead an infrastructure/build
        # failure and cannot safely be waived by comparison.
        if result.returncode != 0 and not any(
            diagnostic.level == "error" for diagnostic in diagnostics_for_package
        ):
            raise GateError(
                f"Clippy did not complete for package {package!r}:\n"
                f"  stdout:\n{result.stdout}\n"
                f"  stderr:\n{result.stderr}"
            )
        diagnostics.extend(diagnostics_for_package)
    return diagnostics


def changed_rust_paths(repo: Path, base: str, head: str) -> list[str]:
    output = checked(
        ["git", "diff", "--name-only", "--diff-filter=ACMR", base, head], cwd=repo
    )
    return sorted(path for path in output.splitlines() if path.endswith(".rs"))


def changed_rust_line_ranges(repo: Path, base: str, head: str) -> dict[str, tuple[tuple[int, int], ...]]:
    """Return added/modified head line ranges for Rust sources only.

    `--unified=0` makes the hunk's `+start,count` portion an exact boundary
    for the candidate source.  Deleted-only hunks intentionally contribute no
    range: a diagnostic cannot point to code that is absent from HEAD.
    """
    output = checked(
        ["git", "diff", "--unified=0", "--diff-filter=ACMR", base, head, "--", "*.rs"],
        cwd=repo,
    )
    ranges: dict[str, list[tuple[int, int]]] = {}
    current_path: str | None = None
    for line in output.splitlines():
        if line.startswith("+++ "):
            raw_path = line.removeprefix("+++ ")
            current_path = raw_path.removeprefix("b/") if raw_path != "/dev/null" else None
            continue
        if not line.startswith("@@ ") or current_path is None:
            continue
        # @@ -old[,count] +new[,count] @@
        try:
            head_range = line.split("+", 1)[1].split(" ", 1)[0]
            start_text, separator, count_text = head_range.partition(",")
            start = int(start_text)
            count = int(count_text) if separator else 1
        except (IndexError, ValueError) as error:
            raise GateError(f"cannot parse unified diff hunk: {line!r}") from error
        if count > 0:
            ranges.setdefault(current_path, []).append((start, start + count - 1))
    return {path: tuple(path_ranges) for path, path_ranges in ranges.items()}


def diagnostic_intersects_changed_lines(
    diagnostic: Diagnostic,
    changed_ranges: dict[str, tuple[tuple[int, int], ...]],
) -> bool:
    for span in diagnostic.primary_spans:
        for changed_start, changed_end in changed_ranges.get(span.path, ()):
            if span.line_start <= changed_end and changed_start <= span.line_end:
                return True
    return False


def render_diagnostic(diagnostic: Diagnostic) -> str:
    code = diagnostic.code or "no-code"
    paths = ", ".join(diagnostic.paths) if diagnostic.paths else "<no-primary-span>"
    spans = ", ".join(
        f"{span.path}:{span.line_start}-{span.line_end}" for span in diagnostic.primary_spans
    )
    location = spans or paths
    return f"[{diagnostic.level}/{code}] {location}: {diagnostic.message}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="base revision of the pull request")
    parser.add_argument("--head", default="HEAD", help="candidate revision (default: HEAD)")
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[1]
    try:
        checked(["git", "rev-parse", "--verify", f"{args.base}^{{commit}}"], cwd=repo)
        checked(["git", "rev-parse", "--verify", f"{args.head}^{{commit}}"], cwd=repo)
        changed_paths = changed_rust_paths(repo, args.base, args.head)
        changed_ranges = changed_rust_line_ranges(repo, args.base, args.head)
        packages = packages_for_paths(changed_paths)
        print(
            "diff-scoped clippy: "
            f"base={args.base} head={args.head} packages={','.join(packages) or '<none>'}"
        )
        if not packages:
            print("PASS: no Rust source file from a checked package changed.")
            return 0

        with tempfile.TemporaryDirectory(prefix="ghost-clippy-diff-") as temp_dir:
            temp_root = Path(temp_dir)
            base_worktree = temp_root / "base"
            checked(
                ["git", "worktree", "add", "--detach", str(base_worktree), args.base],
                cwd=repo,
            )
            try:
                base_diagnostics = clippy_diagnostics(
                    base_worktree, packages, temp_root / "base-target"
                )
                # Earlier workflow steps already compile the candidate head.
                # Reusing its ordinary target directory avoids a second cold
                # dependency build while the base stays isolated by design.
                head_target = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target"))
                head_diagnostics = clippy_diagnostics(repo, packages, head_target)
            finally:
                removal = run(
                    ["git", "worktree", "remove", "--force", str(base_worktree)], cwd=repo
                )
                if removal.returncode != 0:
                    raise GateError(
                        "failed to remove temporary base worktree:\n"
                        f"{removal.stderr}"
                    )

        base_identities = {diagnostic.identity for diagnostic in base_diagnostics}
        new_head_diagnostics = [
            diagnostic
            for diagnostic in head_diagnostics
            if diagnostic.identity not in base_identities
        ]
        diagnostics_in_changed_lines = [
            diagnostic
            for diagnostic in head_diagnostics
            if diagnostic_intersects_changed_lines(diagnostic, changed_ranges)
        ]
        if new_head_diagnostics or diagnostics_in_changed_lines:
            print("FAIL: diff-scoped Clippy found disallowed diagnostics.", file=sys.stderr)
            if new_head_diagnostics:
                print("New diagnostics relative to base:", file=sys.stderr)
                for diagnostic in new_head_diagnostics:
                    print(f"  {render_diagnostic(diagnostic)}", file=sys.stderr)
            if diagnostics_in_changed_lines:
                print("Diagnostics with a primary span on a changed Rust line:", file=sys.stderr)
                for diagnostic in diagnostics_in_changed_lines:
                    print(f"  {render_diagnostic(diagnostic)}", file=sys.stderr)
            return 1

        print(
            "PASS: no new Clippy diagnostics and no diagnostic with a primary span on changed Rust lines."
        )
        return 0
    except GateError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
