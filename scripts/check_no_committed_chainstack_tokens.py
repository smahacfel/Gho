#!/usr/bin/env python3
"""Fail if tracked config files contain literal Chainstack credentials.

The scanner intentionally redacts values in its output. It checks repository
tracked files only, so old local smoke configs can stay untracked without
blocking commits.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


CHAINSTACK_VALUE_KEYS = {
    "grpc_endpoint",
    "rpc_endpoint",
    "rpc_url",
    "shadow_rpc_url",
    "primary_rpc_url",
    "fallback_rpc_url",
}
TOKEN_KEYS = {"grpc_x_token", "grpc_auth_token"}
SAFE_PLACEHOLDERS = {
    "grpc_endpoint": {"${CHAINSTACK_GRPC_ENDPOINT}", "${GHOST_SEER_GRPC_ENDPOINT}"},
    "grpc_x_token": {"", "${CHAINSTACK_GRPC_TOKEN}", "${GHOST_SEER_GRPC_X_TOKEN}"},
    "grpc_auth_token": {"", "${CHAINSTACK_GRPC_TOKEN}", "${GHOST_SEER_GRPC_AUTH_TOKEN}"},
    "rpc_endpoint": {"${CHAINSTACK_RPC_URL}", "${GHOST_SEER_RPC_ENDPOINT}"},
    "rpc_url": {"${CHAINSTACK_RPC_URL}", "${GHOST_TRIGGER_RPC_URL}"},
    "shadow_rpc_url": {"${CHAINSTACK_RPC_URL}", "${GHOST_TRIGGER_SHADOW_RPC_URL}"},
    "primary_rpc_url": {"${CHAINSTACK_RPC_URL}"},
    "fallback_rpc_url": {"", "${CHAINSTACK_RPC_URL}"},
}

ASSIGNMENT_RE = re.compile(r"^\s*([A-Za-z0-9_]+)\s*=\s*\"([^\"]*)\"")


def tracked_files() -> list[Path]:
    raw = subprocess.check_output(["git", "ls-files"], text=True)
    return [Path(line) for line in raw.splitlines() if line.strip()]


def is_config_like(path: Path) -> bool:
    path_s = path.as_posix()
    return (
        path_s == "config.toml"
        or path_s.startswith("configs/")
        or path_s.startswith("ghost-brain/ghost_brain_config")
    )


def scan_file(path: Path) -> list[tuple[int, str, str]]:
    findings: list[tuple[int, str, str]] = []
    try:
        lines = path.read_text(errors="ignore").splitlines()
    except OSError:
        return findings

    for lineno, line in enumerate(lines, 1):
        if line.lstrip().startswith("#"):
            continue
        match = ASSIGNMENT_RE.match(line)
        if not match:
            continue
        key, value = match.group(1), match.group(2).strip()
        safe_values = SAFE_PLACEHOLDERS.get(key, set())
        if key in TOKEN_KEYS and value not in safe_values and value:
            findings.append((lineno, key, "literal_token"))
        if key in CHAINSTACK_VALUE_KEYS and "core.chainstack.com" in value.lower():
            findings.append((lineno, key, "literal_chainstack_endpoint"))
    return findings


def main() -> int:
    all_findings: list[tuple[Path, int, str, str]] = []
    for path in tracked_files():
        if not is_config_like(path):
            continue
        for lineno, key, reason in scan_file(path):
            all_findings.append((path, lineno, key, reason))

    if not all_findings:
        print("OK: no literal Chainstack credentials in tracked config files")
        return 0

    print("ERROR: tracked config files contain literal Chainstack credentials")
    for path, lineno, key, reason in all_findings:
        print(f"{path}:{lineno}: {key}=<redacted> ({reason})")
    return 1


if __name__ == "__main__":
    sys.exit(main())
