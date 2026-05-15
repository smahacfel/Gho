#!/usr/bin/env python3
"""Thin operational wrapper for the Rust V3 full replay validator."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import List, Optional

import v3_shadow_report


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG = REPO_ROOT / "configs" / "rollout" / "shadow-burnin.toml"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate V3 full replay payload readiness")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--decisions-log", type=Path)
    parser.add_argument("--validator-bin", type=Path)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--strict", action="store_true")
    return parser.parse_args()


def resolve_decisions_log(config: Path, decisions_log: Optional[Path]) -> Path:
    if decisions_log is not None:
        resolved = decisions_log if decisions_log.is_absolute() else (REPO_ROOT / decisions_log)
        if not resolved.exists():
            raise FileNotFoundError(f"decisions log not found: {resolved}")
        return resolved
    return v3_shadow_report.resolve_decisions_log(config)


def validator_command(decisions_log: Path, strict: bool, validator_bin: Optional[Path]) -> List[str]:
    if validator_bin is not None:
        command = [str(validator_bin), "--input", str(decisions_log), "--json"]
    else:
        command = [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "ghost-launcher",
            "--bin",
            "v3_replay",
            "--",
            "--input",
            str(decisions_log),
            "--json",
        ]
    if strict:
        command.append("--strict")
    return command


def emit_text(report: dict) -> None:
    print(f"status={report.get('status')}")
    print(f"replay_status={report.get('replay_status')}")
    print(f"v3_rows={report.get('v3_rows')}")
    print(f"bad_rows={report.get('bad_rows')}")
    print(f"status_counts={json.dumps(report.get('status_counts', {}), sort_keys=True)}")


def main() -> int:
    args = parse_args()
    decisions_log = resolve_decisions_log(args.config, args.decisions_log)
    command = validator_command(decisions_log, args.strict, args.validator_bin)
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=False,
    )

    if completed.stdout:
        if args.json:
            print(completed.stdout.rstrip())
        else:
            emit_text(json.loads(completed.stdout))
    if completed.stderr:
        print(completed.stderr.rstrip(), file=sys.stderr)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
