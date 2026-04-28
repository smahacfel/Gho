#!/usr/bin/env python3
import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONTRACT = REPO_ROOT / "configs" / "refactor" / "phase0_freeze.json"


@dataclass
class CheckResult:
    kind: str
    name: str
    passed: bool
    details: str
    observed: int | None = None
    baseline: int | None = None
    maximum: int | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Phase 0 freeze/guardrail contract for the refactor closure plan."
    )
    parser.add_argument(
        "command",
        choices=("summary", "structural-check"),
        help="Print the stored Phase 0 summary or execute the structural guardrail checks.",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=REPO_ROOT,
        help=f"Repository root to inspect (default: {REPO_ROOT})",
    )
    parser.add_argument(
        "--contract",
        type=Path,
        default=DEFAULT_CONTRACT,
        help=f"Phase 0 freeze contract path (default: {DEFAULT_CONTRACT})",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print machine-readable JSON instead of text output.",
    )
    return parser.parse_args()


def resolve_path(repo_root: Path, raw: str) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (repo_root / path).resolve()


def load_contract(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def iter_files(repo_root: Path, patterns: list[str]) -> list[Path]:
    seen: set[Path] = set()
    files: list[Path] = []
    for pattern in patterns:
        has_glob = any(ch in pattern for ch in "*?[]")
        if has_glob:
            matches = repo_root.glob(pattern)
        else:
            matches = [repo_root / pattern]
        for match in matches:
            if match.is_file():
                resolved = match.resolve()
                if resolved not in seen:
                    seen.add(resolved)
                    files.append(resolved)
    return files


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def count_pattern_in_text(text: str, pattern: str, match_mode: str) -> int:
    if match_mode == "literal":
        return text.count(pattern)
    if match_mode == "regex":
        return len(re.findall(pattern, text, flags=re.MULTILINE))
    raise ValueError(f"unsupported match mode: {match_mode}")


def count_matches(repo_root: Path, entry: dict[str, Any]) -> int:
    total = 0
    for path in iter_files(repo_root, entry["paths"]):
        total += count_pattern_in_text(read_text(path), entry["pattern"], entry["match"])
    return total


def check_monotonic_count(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    observed = count_matches(repo_root, entry)
    maximum = int(entry["max"])
    passed = observed <= maximum
    details = (
        f"{entry['description']} observed={observed} baseline={entry['baseline']} max={maximum}"
    )
    return CheckResult(
        kind="monotonic_count",
        name=entry["name"],
        passed=passed,
        details=details,
        observed=observed,
        baseline=int(entry["baseline"]),
        maximum=maximum,
    )


def check_zero_count(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    observed = count_matches(repo_root, entry)
    expected = int(entry["expected"])
    passed = observed == expected
    details = f"{entry['description']} observed={observed} expected={expected}"
    return CheckResult(
        kind="zero_count",
        name=entry["name"],
        passed=passed,
        details=details,
        observed=observed,
        baseline=expected,
        maximum=expected,
    )


def check_presence(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    path = resolve_path(repo_root, entry["path"])
    text = read_text(path)
    passed = entry["needle"] in text
    details = f"{entry['description']} path={entry['path']}"
    return CheckResult(
        kind="presence",
        name=entry["name"],
        passed=passed,
        details=details,
    )


def slice_block(text: str, start: str, end: str) -> tuple[str, str | None]:
    start_idx = text.find(start)
    if start_idx == -1:
        return "", f"start marker not found: {start}"
    end_idx = text.find(end, start_idx)
    if end_idx == -1:
        return "", f"end marker not found after start: {end}"
    return text[start_idx:end_idx], None


def check_block_absence(repo_root: Path, entry: dict[str, Any]) -> CheckResult:
    path = resolve_path(repo_root, entry["path"])
    text = read_text(path)
    block, error = slice_block(text, entry["start"], entry["end"])
    if error is not None:
        return CheckResult(
            kind="block_absence",
            name=entry["name"],
            passed=False,
            details=f"{entry['description']} path={entry['path']} error={error}",
        )
    passed = entry["needle"] not in block
    details = (
        f"{entry['description']} path={entry['path']} "
        f"start={entry['start']} end={entry['end']}"
    )
    return CheckResult(
        kind="block_absence",
        name=entry["name"],
        passed=passed,
        details=details,
    )


def build_structural_report(repo_root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    checks: list[CheckResult] = []
    for entry in contract.get("monotonic_counts", []):
        checks.append(check_monotonic_count(repo_root, entry))
    for entry in contract.get("zero_count_checks", []):
        checks.append(check_zero_count(repo_root, entry))
    for entry in contract.get("presence_checks", []):
        checks.append(check_presence(repo_root, entry))
    for entry in contract.get("block_absence_checks", []):
        checks.append(check_block_absence(repo_root, entry))
    return {
        "passed": all(check.passed for check in checks),
        "checks": [asdict(check) for check in checks],
        "phase_gates": contract.get("phase_gates", []),
    }


def build_summary(repo_root: Path, contract: dict[str, Any]) -> dict[str, Any]:
    monotonic_snapshot = []
    for entry in contract.get("monotonic_counts", []):
        observed = count_matches(repo_root, entry)
        monotonic_snapshot.append(
            {
                "name": entry["name"],
                "observed": observed,
                "baseline": entry["baseline"],
                "max": entry["max"],
                "delta_from_baseline": observed - int(entry["baseline"]),
            }
        )
    return {
        "schema_name": contract["schema_name"],
        "schema_version": contract["schema_version"],
        "plan_path": contract["plan_path"],
        "audit_path": contract["audit_path"],
        "phase_gates": contract.get("phase_gates", []),
        "critical_paths": contract.get("critical_paths", []),
        "baseline": contract.get("baseline", {}),
        "blast_radius": contract.get("blast_radius", {}),
        "monotonic_snapshot": monotonic_snapshot,
    }


def print_summary(summary: dict[str, Any]) -> None:
    baseline_summary = summary["baseline"]["summary"]
    print(
        "[ok] phase0.freeze.baseline="
        f"{baseline_summary['tests_passed']}/{baseline_summary['tests_total']} "
        f"passing, failed={baseline_summary['tests_failed']}"
    )
    print("[ok] phase0.freeze.phase_gates=" + " -> ".join(summary["phase_gates"]))
    print(
        "[ok] phase0.freeze.critical_paths="
        f"{len(summary['critical_paths'])} files"
    )
    for item in summary["monotonic_snapshot"]:
        print(
            "[ok] phase0.freeze.count."
            f"{item['name']}={item['observed']} baseline={item['baseline']} delta={item['delta_from_baseline']}"
        )


def print_structural_report(report: dict[str, Any]) -> None:
    print("[ok] phase0.freeze.phase_gates=" + " -> ".join(report["phase_gates"]))
    for check in report["checks"]:
        status = "ok" if check["passed"] else "fail"
        line = f"[{status}] structural.freeze.{check['name']}: {check['details']}"
        if check["observed"] is not None:
            line += f" observed={check['observed']}"
        print(line)


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    contract = load_contract(args.contract.resolve())

    if args.command == "summary":
        summary = build_summary(repo_root, contract)
        if args.json:
            json.dump(summary, sys.stdout, indent=2)
            sys.stdout.write("\n")
        else:
            print_summary(summary)
        return 0

    report = build_structural_report(repo_root, contract)
    if args.json:
        json.dump(report, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        print_structural_report(report)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
