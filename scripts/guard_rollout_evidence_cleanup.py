#!/usr/bin/env python3
"""Fail-closed guard for rollout evidence cleanup.

Default mode is dry-run.  The guard is intentionally conservative:

* cleanup scope must be explicitly allowlisted with --scope,
* broad roots and archive-volume roots are refused,
* critical evidence files block cleanup,
* execution requires a pre-delete manifest,
* execution requires archive_verified=true in both CLI and manifest,
* execution requires a second confirmation token derived from the manifest.

This script does not start Ghost, does not touch runtime configuration, and is
not imported by runtime code.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA = "rollout_evidence_cleanup_guard_v1"
REPO_ROOT = Path(__file__).resolve().parents[1]
ARCHIVE_VOLUME = Path("/mnt/HC_Volume_105935807").resolve()
ALLOWED_ROOT_NAMES = {"rollout", "shadow_run"}
DEFAULT_IGNORE_NAMES = {".DS_Store"}

CRITICAL_EVIDENCE_NAMES = {
    "gatekeeper_v2_decisions.jsonl",
    "gatekeeper_v2_buys.jsonl",
    "materialized_feature_snapshot.jsonl",
    "shadow_lifecycle.jsonl",
    "probe_shadow_lifecycle.jsonl",
    "shadow_exit_replay_v1.jsonl",
    "shadow_entries.jsonl",
    "probe_shadow_entries.jsonl",
    "selector_shadow_score_v1.jsonl",
    "RUN_LIFECYCLE_LAUNCHER_REPORT.md",
    "RUN_LIFECYCLE_LAUNCHER_REPORT.json",
}

CRITICAL_EVIDENCE_SUFFIXES = (
    "_decisions.jsonl",
    "_buys.jsonl",
    "_lifecycle.jsonl",
    "_exit_replay_v1.jsonl",
    "_manifest.json",
)

REFUSED_SCOPE_VALUES = {"", ".", "..", "*", "all", "logs", "rollout", "shadow_run"}
GLOB_CHARS = set("*?[]{}")


@dataclass(frozen=True)
class FileEntry:
    path: str
    size_bytes: int
    mtime_ns: int
    sha256: str


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def is_relative_to(child: Path, parent: Path) -> bool:
    try:
        child.relative_to(parent)
        return True
    except ValueError:
        return False


def normalize_scope(scope: str) -> str:
    return scope.strip()


def validate_scopes(scopes: list[str]) -> list[str]:
    normalized: list[str] = []
    errors: list[str] = []
    for raw in scopes:
        scope = normalize_scope(raw)
        if scope in REFUSED_SCOPE_VALUES:
            errors.append(f"refused_scope_value:{raw}")
        if "/" in scope or "\\" in scope:
            errors.append(f"scope_must_be_name_not_path:{raw}")
        if any(ch in scope for ch in GLOB_CHARS):
            errors.append(f"scope_glob_not_allowed:{raw}")
        if scope and scope not in normalized:
            normalized.append(scope)
    if not normalized:
        errors.append("explicit_scope_allowlist_required")
    if errors:
        raise SystemExit("FAIL_SCOPE_ALLOWLIST: " + ",".join(errors))
    return normalized


def validate_root(root: Path) -> Path:
    resolved = root.expanduser().resolve()
    broad_roots = {
        Path("/"),
        Path("/root"),
        Path("/tmp"),
        Path("/mnt"),
        REPO_ROOT,
        REPO_ROOT / "logs",
        ARCHIVE_VOLUME,
        ARCHIVE_VOLUME / "logs",
    }
    if resolved in broad_roots:
        raise SystemExit(f"FAIL_BROAD_ROOT_REFUSED: {resolved}")
    if is_relative_to(resolved, ARCHIVE_VOLUME):
        raise SystemExit(f"FAIL_ARCHIVE_VOLUME_ROOT_REFUSED: {resolved}")
    if resolved.name not in ALLOWED_ROOT_NAMES:
        raise SystemExit(
            "FAIL_NON_ROLLOUT_ROOT_REFUSED: "
            f"{resolved} (expected basename in {sorted(ALLOWED_ROOT_NAMES)})"
        )
    if not resolved.exists():
        raise SystemExit(f"FAIL_ROOT_MISSING: {resolved}")
    if not resolved.is_dir():
        raise SystemExit(f"FAIL_ROOT_NOT_DIRECTORY: {resolved}")
    return resolved


def target_paths(root: Path, scopes: list[str]) -> list[Path]:
    paths: list[Path] = []
    for scope in scopes:
        target = (root / scope).resolve()
        if not is_relative_to(target, root):
            raise SystemExit(f"FAIL_SCOPE_ESCAPES_ROOT: {scope} -> {target}")
        if not target.exists():
            raise SystemExit(f"FAIL_SCOPE_PATH_MISSING: {scope} -> {target}")
        if not target.is_dir():
            raise SystemExit(f"FAIL_SCOPE_PATH_NOT_DIRECTORY: {scope} -> {target}")
        paths.append(target)
    return paths


def is_critical_evidence(path: Path) -> bool:
    name = path.name
    if name in CRITICAL_EVIDENCE_NAMES:
        return True
    if any(name.endswith(suffix) for suffix in CRITICAL_EVIDENCE_SUFFIXES):
        return True
    return False


def scan_files(root: Path, targets: list[Path]) -> tuple[list[FileEntry], list[str], list[str]]:
    entries: list[FileEntry] = []
    critical: list[str] = []
    symlinks: list[str] = []
    for target in targets:
        for current_root, dirnames, filenames in os.walk(target, topdown=True, followlinks=False):
            current = Path(current_root)
            kept_dirnames = []
            for dirname in dirnames:
                child = current / dirname
                if child.is_symlink():
                    symlinks.append(str(child.relative_to(root)))
                else:
                    kept_dirnames.append(dirname)
            dirnames[:] = kept_dirnames
            for filename in sorted(filenames):
                if filename in DEFAULT_IGNORE_NAMES:
                    continue
                path = current / filename
                rel = str(path.relative_to(root))
                if path.is_symlink():
                    symlinks.append(rel)
                    continue
                if not path.is_file():
                    continue
                if is_critical_evidence(path):
                    critical.append(rel)
                stat = path.stat()
                entries.append(
                    FileEntry(
                        path=rel,
                        size_bytes=stat.st_size,
                        mtime_ns=stat.st_mtime_ns,
                        sha256=sha256_file(path),
                    )
                )
    return entries, critical, symlinks


def manifest_payload(
    *,
    root: Path,
    scopes: list[str],
    entries: list[FileEntry],
    archive_verified: bool,
    critical: list[str],
    symlinks: list[str],
) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "created_at": utc_now(),
        "root": str(root),
        "scopes": scopes,
        "archive_verified": archive_verified,
        "file_count": len(entries),
        "total_size_bytes": sum(entry.size_bytes for entry in entries),
        "critical_evidence_files": critical,
        "symlinks_refused": symlinks,
        "files": [entry.__dict__ for entry in entries],
    }


def canonical_manifest_subset(manifest: dict[str, Any]) -> str:
    subset = {
        "schema": manifest.get("schema"),
        "root": manifest.get("root"),
        "scopes": manifest.get("scopes"),
        "archive_verified": manifest.get("archive_verified"),
        "file_count": manifest.get("file_count"),
        "total_size_bytes": manifest.get("total_size_bytes"),
        "files": manifest.get("files"),
    }
    return json.dumps(subset, sort_keys=True, separators=(",", ":"))


def confirmation_token(manifest: dict[str, Any]) -> str:
    digest = hashlib.sha256(canonical_manifest_subset(manifest).encode("utf-8")).hexdigest()
    return f"DELETE_EVIDENCE:{digest[:16]}"


def write_manifest(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_manifest(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise SystemExit(f"FAIL_PRE_DELETE_MANIFEST_MISSING: {path}")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"FAIL_PRE_DELETE_MANIFEST_INVALID_JSON: {path}:{exc}") from exc
    if not isinstance(data, dict):
        raise SystemExit(f"FAIL_PRE_DELETE_MANIFEST_NOT_OBJECT: {path}")
    if data.get("schema") != SCHEMA:
        raise SystemExit(f"FAIL_PRE_DELETE_MANIFEST_SCHEMA: {data.get('schema')}")
    return data


def verify_manifest_matches_current(
    *,
    manifest: dict[str, Any],
    root: Path,
    scopes: list[str],
    current_entries: list[FileEntry],
) -> None:
    if str(root) != manifest.get("root"):
        raise SystemExit("FAIL_PRE_DELETE_MANIFEST_ROOT_MISMATCH")
    if scopes != list(manifest.get("scopes") or []):
        raise SystemExit("FAIL_PRE_DELETE_MANIFEST_SCOPE_MISMATCH")
    current = sorted((entry.__dict__ for entry in current_entries), key=lambda row: row["path"])
    manifest_files = sorted(list(manifest.get("files") or []), key=lambda row: row.get("path", ""))
    if current != manifest_files:
        raise SystemExit("FAIL_PRE_DELETE_MANIFEST_FILESET_MISMATCH")


def delete_manifest_files(root: Path, manifest: dict[str, Any]) -> int:
    files = sorted(list(manifest.get("files") or []), key=lambda row: str(row.get("path", "")), reverse=True)
    deleted = 0
    for row in files:
        rel = str(row.get("path") or "")
        path = (root / rel).resolve()
        if not is_relative_to(path, root):
            raise SystemExit(f"FAIL_MANIFEST_PATH_ESCAPES_ROOT: {rel}")
        if path.exists():
            if path.is_symlink():
                raise SystemExit(f"FAIL_REFUSE_SYMLINK_DELETE: {rel}")
            if path.is_file():
                path.unlink()
                deleted += 1
    scopes = list(manifest.get("scopes") or [])
    for scope in scopes:
        target = (root / str(scope)).resolve()
        if target.exists():
            for current_root, dirnames, _filenames in os.walk(target, topdown=False, followlinks=False):
                for dirname in dirnames:
                    directory = Path(current_root) / dirname
                    if directory.exists() and not any(directory.iterdir()):
                        directory.rmdir()
            if target.exists() and not any(target.iterdir()):
                target.rmdir()
    return deleted


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        required=True,
        help="Cleanup parent directory; expected basename is rollout or shadow_run.",
    )
    parser.add_argument(
        "--scope",
        action="append",
        default=[],
        help="Explicit scope allowlist. Repeat for every scope. Path/glob values are refused.",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        help="Existing pre-delete manifest to verify before execution.",
    )
    parser.add_argument(
        "--write-manifest",
        type=Path,
        help="Write a pre-delete manifest in dry-run mode. Does not delete anything.",
    )
    parser.add_argument(
        "--archive-verified",
        choices=("true", "false"),
        default="false",
        help="Must be true for execution. Manifest must also contain archive_verified=true.",
    )
    parser.add_argument(
        "--confirm-token",
        help="Second confirmation token required for --execute. Dry-run prints the expected token.",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Delete files only after all guards pass. Default is dry-run.",
    )
    parser.add_argument("--json", action="store_true", help="Print JSON result.")
    return parser.parse_args()


def build_result(args: argparse.Namespace) -> dict[str, Any]:
    scopes = validate_scopes(args.scope)
    root = validate_root(args.root)
    targets = target_paths(root, scopes)
    entries, critical, symlinks = scan_files(root, targets)
    archive_verified = args.archive_verified == "true"
    payload = manifest_payload(
        root=root,
        scopes=scopes,
        entries=entries,
        archive_verified=archive_verified,
        critical=critical,
        symlinks=symlinks,
    )
    expected_token = confirmation_token(payload)
    result = {
        "schema": SCHEMA,
        "status": "DRY_RUN",
        "dry_run": not args.execute,
        "root": str(root),
        "scopes": scopes,
        "target_paths": [str(path) for path in targets],
        "file_count": len(entries),
        "total_size_bytes": payload["total_size_bytes"],
        "critical_evidence_files": critical,
        "symlinks_refused": symlinks,
        "archive_verified_cli": archive_verified,
        "expected_confirm_token": expected_token,
        "manifest": str(args.manifest) if args.manifest else None,
        "write_manifest": str(args.write_manifest) if args.write_manifest else None,
        "runtime_changed": False,
        "research_run_started": False,
        "cleanup_executed": False,
    }
    if critical:
        result["status"] = "FAIL_CRITICAL_EVIDENCE_FILES_PRESENT"
        return result
    if symlinks:
        result["status"] = "FAIL_SYMLINKS_PRESENT"
        return result
    if args.write_manifest:
        if args.execute:
            raise SystemExit("FAIL_WRITE_MANIFEST_WITH_EXECUTE_REFUSED")
        write_manifest(args.write_manifest, payload)
        result["status"] = "DRY_RUN_MANIFEST_WRITTEN"
        result["manifest_written"] = str(args.write_manifest)
    if not args.execute:
        if not args.manifest and not args.write_manifest:
            result["status"] = "DRY_RUN_PRE_DELETE_MANIFEST_NOT_PROVIDED"
        return result

    if args.write_manifest:
        raise SystemExit("FAIL_WRITE_MANIFEST_WITH_EXECUTE_REFUSED")
    if not args.manifest:
        raise SystemExit("FAIL_PRE_DELETE_MANIFEST_REQUIRED")
    manifest = load_manifest(args.manifest)
    verify_manifest_matches_current(
        manifest=manifest,
        root=root,
        scopes=scopes,
        current_entries=entries,
    )
    if args.archive_verified != "true":
        raise SystemExit("FAIL_ARCHIVE_VERIFIED_CLI_REQUIRED")
    if manifest.get("archive_verified") is not True:
        raise SystemExit("FAIL_ARCHIVE_VERIFIED_MANIFEST_REQUIRED")
    expected_manifest_token = confirmation_token(manifest)
    result["expected_confirm_token"] = expected_manifest_token
    if args.confirm_token != expected_manifest_token:
        raise SystemExit("FAIL_CONFIRM_TOKEN_MISMATCH")
    deleted = delete_manifest_files(root, manifest)
    result.update(
        {
            "status": "EXECUTED",
            "dry_run": False,
            "cleanup_executed": True,
            "deleted_file_count": deleted,
        }
    )
    return result


def print_result(result: dict[str, Any], as_json: bool) -> None:
    if as_json:
        print(json.dumps(result, indent=2, sort_keys=True))
        return
    print(f"status: {result['status']}")
    print(f"dry_run: {str(result['dry_run']).lower()}")
    print(f"root: {result['root']}")
    print(f"scopes: {', '.join(result['scopes'])}")
    print(f"file_count: {result['file_count']}")
    print(f"total_size_bytes: {result['total_size_bytes']}")
    print(f"critical_evidence_files: {len(result['critical_evidence_files'])}")
    print(f"symlinks_refused: {len(result['symlinks_refused'])}")
    print(f"archive_verified_cli: {str(result['archive_verified_cli']).lower()}")
    print(f"expected_confirm_token: {result['expected_confirm_token']}")
    print(f"cleanup_executed: {str(result['cleanup_executed']).lower()}")


def main() -> int:
    args = parse_args()
    result = build_result(args)
    print_result(result, args.json)
    if str(result.get("status", "")).startswith("FAIL_"):
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
