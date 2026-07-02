#!/usr/bin/env python3
from __future__ import annotations

import subprocess
from pathlib import Path

from shadow_v2_offline_audit_common import emit, parser, read_json


FORBIDDEN_PATTERNS = [
    "runtime.log",
    ".jsonl",
    "logs/",
    "datasets/events",
    "__pycache__",
    "shadow_lifecycle",
    "shadow_exit_replay",
    "gatekeeper_v2_decisions",
    "reports/selector/shadow-burnin-v3-r51",
    ".local.toml",
]


def staged_files() -> list[str]:
    proc = subprocess.run(
        ["git", "diff", "--cached", "--name-only"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return [line for line in proc.stdout.splitlines() if line.strip()]


def main() -> int:
    args = parser("Offline Shadow V2 manifest/retention audit").parse_args()
    scope = Path(args.scope_root)
    post_manifest = scope / "post_run_manifest.json"
    manifest = read_json(post_manifest) if post_manifest.exists() else {}
    staged = staged_files()
    forbidden = [
        path
        for path in staged
        if any(pattern in path or path.endswith(pattern) for pattern in FORBIDDEN_PATTERNS)
    ]
    status = manifest.get("status")
    schema_coverage = manifest.get("schema_coverage") or {}
    artifact_count = len(manifest.get("artifacts") or [])
    total_size = manifest.get("total_size_bytes")
    strict_audit_status = "PASS" if status == "PASS" and not manifest.get("blockers") else "FAIL"
    if forbidden:
        verdict = "FAIL_MANIFEST_OR_STAGING_VIOLATION"
    elif status == "PASS" and strict_audit_status == "PASS" and schema_coverage:
        verdict = "PASS_MANIFEST_RETENTION_AUDIT"
    else:
        verdict = "BLOCKED_MANIFEST_RETENTION_GAP"
    result = {
        "audit": "manifest_retention",
        "scope_root": args.scope_root,
        "runtime_post_run_manifest_status": status,
        "strict_audit_status": strict_audit_status,
        "manifest_blockers": manifest.get("blockers") or [],
        "schema_coverage_counts": schema_coverage,
        "artifact_count": artifact_count,
        "total_size_bytes": total_size,
        "staged_file_count": len(staged),
        "raw_jsonl_not_staged": not any(path.endswith(".jsonl") for path in staged),
        "logs_not_staged": not any(path.startswith("logs/") for path in staged),
        "runtime_scope_not_staged": not any(str(args.scope_root) in path for path in staged),
        "local_configs_not_staged": not any(path.endswith(".local.toml") for path in staged),
        "forbidden_staged_files": forbidden,
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
