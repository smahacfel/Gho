#!/usr/bin/env python3
"""Offline Shadow Burnin Simulation V2 manifest audit.

PR10 scope:
- inspect existing evidence files only when a scope root is provided;
- compute sha256, line counts, JSONL row counts, malformed JSONL rows and schema coverage;
- generate pre/post manifests only when explicitly requested;
- never start runs, stop runs, clean artifacts or stage raw evidence.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sys
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


CONTRACT_VERSION = "shadow_v2_manifest_audit_v1"
SIMULATION_CONTRACT_VERSION = "shadow_burnin_simulation_v2_20260629"
MANIFEST_SCHEMA = "shadow_v2_evidence_manifest_v1"
ARTIFACT_ENTRY_SCHEMA = "shadow_v2_artifact_manifest_entry_v1"
RETENTION_POLICY = "manifest_before_cleanup_required"

DEFAULT_SCHEMA_MANIFEST = Path("reports/selector/shadow_v2_required_schema_manifest.csv")
DEFAULT_ACCEPTANCE_GATES = Path("reports/selector/shadow_v2_acceptance_gates.csv")
DEFAULT_ARTIFACT_CONTRACT = Path("reports/selector/shadow_v2_manifest_artifact_contract.csv")
EXECUTABLE_DYNAMIC_EXIT_EVIDENCE_ARTIFACT = "executable_dynamic_exit_evidence_v1.jsonl"

REQUIRED_CONTRACT_COLUMNS = {
    "artifact_name",
    "phase",
    "required_for_research_grade",
    "path_role",
    "schema_expected",
    "row_count_required",
    "sha256_required",
    "raw_jsonl_allowed_in_git",
    "notes",
}


@dataclass(frozen=True)
class ArtifactContractRow:
    artifact_name: str
    phase: str
    required_for_research_grade: bool
    path_role: str
    schema_expected: str
    row_count_required: bool
    sha256_required: bool
    raw_jsonl_allowed_in_git: bool
    notes: str


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def parse_bool(raw: str) -> bool:
    return raw.strip().lower() in {"1", "true", "yes", "y"}


def load_artifact_contract(path: Path) -> tuple[list[ArtifactContractRow], list[str]]:
    errors: list[str] = []
    if not path.exists():
        return [], [f"missing artifact contract: {path}"]

    rows: list[ArtifactContractRow] = []
    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        missing_columns = REQUIRED_CONTRACT_COLUMNS.difference(reader.fieldnames or [])
        if missing_columns:
            return [], [f"{path} missing columns: {sorted(missing_columns)}"]

        for index, row in enumerate(reader, start=2):
            try:
                rows.append(
                    ArtifactContractRow(
                        artifact_name=row["artifact_name"].strip(),
                        phase=row["phase"].strip(),
                        required_for_research_grade=parse_bool(
                            row["required_for_research_grade"]
                        ),
                        path_role=row["path_role"].strip(),
                        schema_expected=row["schema_expected"].strip(),
                        row_count_required=parse_bool(row["row_count_required"]),
                        sha256_required=parse_bool(row["sha256_required"]),
                        raw_jsonl_allowed_in_git=parse_bool(row["raw_jsonl_allowed_in_git"]),
                        notes=row["notes"].strip(),
                    )
                )
            except KeyError as exc:
                errors.append(f"{path}:{index} missing key {exc}")

    return rows, errors


def sha256_file(path: Path, max_sha_bytes: int) -> tuple[str, str]:
    size = path.stat().st_size
    if size > max_sha_bytes:
        return "", "SKIPPED_TOO_LARGE"

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest(), "OK"


def extract_schema(value: dict[str, Any]) -> str | None:
    candidates = [
        value.get("schema"),
        value.get("record_type"),
        value.get("event_type"),
    ]

    record = value.get("record")
    if isinstance(record, dict):
        candidates.append(record.get("schema"))
        envelope = record.get("envelope")
        if isinstance(envelope, dict):
            candidates.append(envelope.get("schema"))

    envelope = value.get("envelope")
    if isinstance(envelope, dict):
        candidates.append(envelope.get("schema"))

    for candidate in candidates:
        if isinstance(candidate, str) and candidate.strip():
            return candidate.strip()
    return None


def jsonl_stats(path: Path) -> tuple[int, int, Counter[str]]:
    rows = 0
    malformed = 0
    schema_counts: Counter[str] = Counter()

    with path.open("r", encoding="utf-8") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line:
                continue
            rows += 1
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if isinstance(value, dict):
                schema = extract_schema(value)
                if schema:
                    schema_counts[schema] += 1
                else:
                    schema_counts["UNKNOWN_SCHEMA"] += 1
            else:
                schema_counts["NON_OBJECT_JSON"] += 1

    return rows, malformed, schema_counts


def streamed_file_stats(
    path: Path,
    max_sha_bytes: int,
) -> tuple[int, str, str, int, int, Counter[str]]:
    size = path.stat().st_size
    sha_enabled = size <= max_sha_bytes
    digest = hashlib.sha256() if sha_enabled else None
    sha_status = "OK" if sha_enabled else "SKIPPED_TOO_LARGE"
    line_count = 0
    jsonl_rows = 0
    malformed_jsonl_rows = 0
    schema_counts: Counter[str] = Counter()
    inspect_jsonl = path.suffix == ".jsonl"

    with path.open("rb") as handle:
        for raw_line in handle:
            line_count += 1
            if digest is not None:
                digest.update(raw_line)
            if not inspect_jsonl:
                continue
            line = raw_line.strip()
            if not line:
                continue
            jsonl_rows += 1
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                malformed_jsonl_rows += 1
                continue
            if isinstance(value, dict):
                schema = extract_schema(value)
                if schema:
                    schema_counts[schema] += 1
                else:
                    schema_counts["UNKNOWN_SCHEMA"] += 1
            else:
                schema_counts["NON_OBJECT_JSON"] += 1

    sha = digest.hexdigest() if digest is not None else ""
    return line_count, sha, sha_status, jsonl_rows, malformed_jsonl_rows, schema_counts


def count_lines(path: Path) -> int:
    lines = 0
    with path.open("rb") as handle:
        for _ in handle:
            lines += 1
    return lines


def artifact_entry(path: Path, scope_root: Path, max_sha_bytes: int) -> dict[str, Any]:
    relative_path = str(path.relative_to(scope_root))
    stat = path.lstat()

    if path.is_symlink():
        return {
            "schema": ARTIFACT_ENTRY_SCHEMA,
            "relative_path": relative_path,
            "size_bytes": 0,
            "line_count": 0,
            "sha256": "",
            "sha256_status": "SYMLINK_SKIPPED",
            "jsonl_rows": 0,
            "malformed_jsonl_rows": 0,
            "schema_counts": {},
            "is_symlink": True,
            "status": "BLOCKED_SYMLINK",
        }

    (
        line_count,
        sha,
        sha_status,
        jsonl_rows,
        malformed_jsonl_rows,
        schema_counts,
    ) = streamed_file_stats(path, max_sha_bytes)

    status = "OK"
    if malformed_jsonl_rows > 0:
        status = "BLOCKED_MALFORMED_JSONL"

    return {
        "schema": ARTIFACT_ENTRY_SCHEMA,
        "relative_path": relative_path,
        "size_bytes": stat.st_size,
        "line_count": line_count,
        "sha256": sha,
        "sha256_status": sha_status,
        "jsonl_rows": jsonl_rows,
        "malformed_jsonl_rows": malformed_jsonl_rows,
        "schema_counts": dict(sorted(schema_counts.items())),
        "is_symlink": False,
        "status": status,
    }


def iter_scope_files(scope_root: Path) -> Iterable[Path]:
    for path in sorted(scope_root.rglob("*")):
        if path.is_file() or path.is_symlink():
            yield path


def contract_applies(row: ArtifactContractRow, manifest_phase: str) -> bool:
    if not row.required_for_research_grade:
        return False
    if row.phase == "always":
        return True
    if manifest_phase == "post_run" and row.phase in {"pre_run", "post_run"}:
        return True
    return row.phase == manifest_phase


def find_missing_required_artifacts(
    artifact_entries: list[dict[str, Any]],
    contract_rows: list[ArtifactContractRow],
    manifest_phase: str,
    generated_artifacts: set[str] | None = None,
    executable_dynamic_exit_evidence_enabled: bool = False,
) -> list[str]:
    present_names = {Path(entry["relative_path"]).name for entry in artifact_entries}
    present_paths = {entry["relative_path"] for entry in artifact_entries}
    if generated_artifacts:
        present_names.update(Path(artifact).name for artifact in generated_artifacts)
        present_paths.update(generated_artifacts)
    missing: list[str] = []

    for row in contract_rows:
        if not contract_applies(row, manifest_phase):
            continue
        if (
            row.artifact_name == EXECUTABLE_DYNAMIC_EXIT_EVIDENCE_ARTIFACT
            and not executable_dynamic_exit_evidence_enabled
        ):
            continue
        if row.artifact_name in present_names or row.artifact_name in present_paths:
            continue
        missing.append(row.artifact_name)

    return sorted(missing)


def find_entry_for_contract(
    artifact_entries: list[dict[str, Any]],
    row: ArtifactContractRow,
) -> dict[str, Any] | None:
    for entry in artifact_entries:
        relative_path = entry["relative_path"]
        if relative_path == row.artifact_name or Path(relative_path).name == row.artifact_name:
            return entry
    return None


def validate_artifact_requirements(
    artifact_entries: list[dict[str, Any]],
    contract_rows: list[ArtifactContractRow],
    manifest_phase: str,
    executable_dynamic_exit_evidence_enabled: bool = False,
) -> list[str]:
    blockers: list[str] = []

    for row in contract_rows:
        if not contract_applies(row, manifest_phase):
            continue
        if (
            row.artifact_name == EXECUTABLE_DYNAMIC_EXIT_EVIDENCE_ARTIFACT
            and not executable_dynamic_exit_evidence_enabled
        ):
            continue

        entry = find_entry_for_contract(artifact_entries, row)
        if entry is None:
            continue

        name = entry["relative_path"]
        if row.sha256_required and entry["sha256_status"] != "OK":
            blockers.append(f"{name}: required sha256 missing or skipped")
        if row.row_count_required and int(entry["jsonl_rows"]) <= 0:
            blockers.append(f"{name}: required JSONL row count is zero")
        if (
            row.schema_expected
            and Path(name).suffix == ".jsonl"
            and row.schema_expected not in entry["schema_counts"]
        ):
            blockers.append(f"{name}: expected schema {row.schema_expected} not found")

    return blockers


def generated_artifact_idents(scope_root: Path, paths: Iterable[Path | None]) -> set[str]:
    idents: set[str] = set()
    scope_root_resolved = scope_root.resolve()
    for path in paths:
        if path is None:
            continue
        try:
            relative = path.resolve().relative_to(scope_root_resolved)
        except ValueError:
            continue
        relative_str = str(relative)
        idents.add(relative_str)
        idents.add(relative.name)
    return idents


def aggregate_schema_coverage(entries: list[dict[str, Any]]) -> dict[str, int]:
    total: Counter[str] = Counter()
    for entry in entries:
        for schema, count in entry["schema_counts"].items():
            total[schema] += int(count)
    return dict(sorted(total.items()))


def build_manifest(
    scope_root: Path,
    manifest_phase: str,
    run_id: str,
    artifact_contract: Path,
    max_sha_bytes: int,
    generated_artifact_paths: Iterable[Path | None] = (),
    executable_dynamic_exit_evidence_enabled: bool = False,
) -> tuple[dict[str, Any], list[str]]:
    contract_rows, contract_errors = load_artifact_contract(artifact_contract)
    entries = [artifact_entry(path, scope_root, max_sha_bytes) for path in iter_scope_files(scope_root)]
    generated_artifacts = generated_artifact_idents(scope_root, generated_artifact_paths)
    missing_required = find_missing_required_artifacts(
        entries,
        contract_rows,
        manifest_phase,
        generated_artifacts,
        executable_dynamic_exit_evidence_enabled=executable_dynamic_exit_evidence_enabled,
    )

    blockers = list(contract_errors)
    blockers.extend(f"missing required artifact: {name}" for name in missing_required)
    blockers.extend(
        validate_artifact_requirements(
            entries,
            contract_rows,
            manifest_phase,
            executable_dynamic_exit_evidence_enabled=executable_dynamic_exit_evidence_enabled,
        )
    )
    for entry in entries:
        if entry["status"] != "OK":
            blockers.append(f"{entry['relative_path']}: {entry['status']}")

    manifest = {
        "schema": MANIFEST_SCHEMA,
        "schema_version": 1,
        "manifest_audit_version": CONTRACT_VERSION,
        "simulation_contract_version": SIMULATION_CONTRACT_VERSION,
        "manifest_phase": manifest_phase,
        "run_id": run_id,
        "created_at": utc_now_iso(),
        "scope_root": str(scope_root),
        "artifact_count": len(entries),
        "total_size_bytes": sum(int(entry["size_bytes"]) for entry in entries),
        "schema_coverage": aggregate_schema_coverage(entries),
        "required_artifacts_missing": missing_required,
        "executable_dynamic_exit_evidence_enabled": executable_dynamic_exit_evidence_enabled,
        "executable_dynamic_exit_evidence_status": (
            "REQUIRED_ENABLED"
            if executable_dynamic_exit_evidence_enabled
            else "NOT_REQUIRED_DISABLED"
        ),
        "retention_policy": RETENTION_POLICY,
        "raw_jsonl_git_staging_allowed": False,
        "artifacts": entries,
        "status": "PASS" if not blockers else "BLOCKED",
        "blockers": blockers,
    }
    return manifest, blockers


def write_manifest(path: Path, manifest: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_report_csv(path: Path, entries: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = [
        "relative_path",
        "size_bytes",
        "line_count",
        "sha256",
        "sha256_status",
        "jsonl_rows",
        "malformed_jsonl_rows",
        "schema_counts",
        "is_symlink",
        "status",
    ]
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for entry in entries:
            row = {key: entry.get(key, "") for key in fieldnames}
            row["schema_counts"] = json.dumps(row["schema_counts"], sort_keys=True)
            writer.writerow(row)


def audit_contract_files(
    schema_manifest: Path,
    acceptance_gates: Path,
    artifact_contract: Path,
) -> tuple[dict[str, Any], list[str]]:
    paths = [schema_manifest, acceptance_gates, artifact_contract]
    missing = [str(path) for path in paths if not path.exists()]
    contract_rows, contract_errors = load_artifact_contract(artifact_contract)

    result = {
        "schema": "shadow_v2_manifest_audit_contract_check_v1",
        "manifest_audit_version": CONTRACT_VERSION,
        "simulation_contract_version": SIMULATION_CONTRACT_VERSION,
        "checked_at": utc_now_iso(),
        "schema_manifest": str(schema_manifest),
        "acceptance_gates": str(acceptance_gates),
        "artifact_contract": str(artifact_contract),
        "artifact_contract_rows": len(contract_rows),
        "status": "CONTRACT_READY" if not missing and not contract_errors else "BLOCKED",
        "missing": missing,
        "errors": contract_errors,
        "raw_jsonl_git_staging_allowed": False,
    }
    return result, missing + contract_errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Offline Shadow V2 evidence manifest audit. Does not start runs or clean logs."
    )
    parser.add_argument("--scope-root", type=Path, help="Existing run/evidence directory to scan.")
    parser.add_argument(
        "--manifest-phase",
        choices=["pre_run", "post_run"],
        default="post_run",
        help="Manifest phase for required-artifact checks.",
    )
    parser.add_argument("--run-id", default="UNKNOWN", help="Run id stored in generated manifest.")
    parser.add_argument(
        "--write-manifest",
        type=Path,
        help="Optional output path for generated manifest JSON.",
    )
    parser.add_argument(
        "--write-report-csv",
        type=Path,
        help="Optional output path for per-artifact CSV report.",
    )
    parser.add_argument(
        "--schema-manifest",
        type=Path,
        default=DEFAULT_SCHEMA_MANIFEST,
        help="Shadow V2 required schema manifest path.",
    )
    parser.add_argument(
        "--acceptance-gates",
        type=Path,
        default=DEFAULT_ACCEPTANCE_GATES,
        help="Shadow V2 acceptance gates CSV path.",
    )
    parser.add_argument(
        "--artifact-contract",
        type=Path,
        default=DEFAULT_ARTIFACT_CONTRACT,
        help="Shadow V2 artifact contract CSV path.",
    )
    parser.add_argument(
        "--max-sha-bytes",
        type=int,
        default=512 * 1024 * 1024,
        help="Maximum file size hashed for sha256.",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit non-zero when required artifacts or contract files are missing.",
    )
    parser.add_argument(
        "--executable-dynamic-exit-evidence-enabled",
        default="false",
        help="Treat executable_dynamic_exit_evidence_v1.jsonl as a required sidecar artifact.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.scope_root is None:
        result, blockers = audit_contract_files(
            args.schema_manifest,
            args.acceptance_gates,
            args.artifact_contract,
        )
        print(json.dumps(result, indent=2, sort_keys=True))
        return 1 if args.strict and blockers else 0

    scope_root = args.scope_root
    if not scope_root.exists() or not scope_root.is_dir():
        result = {
            "schema": MANIFEST_SCHEMA,
            "manifest_audit_version": CONTRACT_VERSION,
            "status": "BLOCKED",
            "blockers": [f"scope root does not exist or is not a directory: {scope_root}"],
        }
        print(json.dumps(result, indent=2, sort_keys=True))
        return 1 if args.strict else 0

    manifest, blockers = build_manifest(
        scope_root=scope_root,
        manifest_phase=args.manifest_phase,
        run_id=args.run_id,
        artifact_contract=args.artifact_contract,
        max_sha_bytes=args.max_sha_bytes,
        generated_artifact_paths=[args.write_manifest, args.write_report_csv],
        executable_dynamic_exit_evidence_enabled=parse_bool(
            args.executable_dynamic_exit_evidence_enabled
        ),
    )

    if args.write_manifest:
        write_manifest(args.write_manifest, manifest)
    if args.write_report_csv:
        write_report_csv(args.write_report_csv, manifest["artifacts"])

    print(json.dumps({k: v for k, v in manifest.items() if k != "artifacts"}, indent=2, sort_keys=True))
    return 1 if args.strict and blockers else 0


if __name__ == "__main__":
    sys.exit(main())
