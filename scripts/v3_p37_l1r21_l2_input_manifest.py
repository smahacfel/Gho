#!/usr/bin/env python3
"""P3.7-L1R21 reproducible L2 input manifest and dataset contract audit."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import v3_p37_l1r19_executable_universe_discovery as l1r19
import v3_p37_l1r20_l2_executable_subset_preflight as l1r20
import v3_p37_mfs_lifecycle_join_key_audit as join_audit


SCHEMA_VERSION = 1
DEFAULT_OUTPUT_JSON = Path(
    "PLANS/AUDYT/MANIFEST_P3_7_L1R21_L2_INPUT_DATASET_CONTRACT_20260524.json"
)
DEFAULT_OUTPUT_MD = Path(
    "PLANS/AUDYT/RAPORT_P3_7_L1R21_L2_INPUT_MANIFEST_DATASET_CONTRACT_20260524.md"
)

EXPECTED_BUY_QUALITY_DENOMINATOR_ROWS = 85
EXPECTED_BUY_QUALITY_DIRTY_GOOD = 4
EXPECTED_EXCLUDED_NON_EXECUTABLE_ROWS = 3956
EXPECTED_EXCLUDED_UNSUPPORTED_ROUTE_ROWS = 11


def sha256_file(path: Path) -> str | None:
    if not path.exists() or not path.is_file():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def jsonl_rows(path: Path) -> int | None:
    if not path.exists() or not path.is_file() or path.suffix != ".jsonl":
        return None
    return sum(1 for _ in join_audit.iter_jsonl(path))


def file_manifest(path: Path, *, role: str) -> dict[str, Any]:
    return {
        "role": role,
        "path": str(path),
        "exists": path.exists(),
        "bytes": path.stat().st_size if path.exists() and path.is_file() else None,
        "sha256": sha256_file(path),
        "jsonl_rows": jsonl_rows(path),
    }


def artifact_manifests_for_run(run: dict[str, Any]) -> list[dict[str, Any]]:
    artifacts: list[dict[str, Any]] = []
    config_path = Path(str(run["config_path"]))
    artifacts.append(file_manifest(config_path, role="config"))
    try:
        resolved = join_audit.resolve_config_path(config_path)
        paths = join_audit.resolve_paths(resolved)
        for artifact_type, artifact_paths in sorted(paths.items()):
            for path in artifact_paths:
                artifacts.append(file_manifest(path, role=artifact_type))
    except Exception as exc:
        artifacts.append(
            {
                "role": "artifact_resolution_error",
                "path": str(config_path),
                "exists": config_path.exists(),
                "error": f"{type(exc).__name__}: {exc}",
            }
        )
    for label_path in run.get("label_files", []):
        artifacts.append(file_manifest(Path(str(label_path)), role="lifecycle_label_file"))
    for feature_path in run.get("feature_availability_files", []):
        artifacts.append(file_manifest(Path(str(feature_path)), role="feature_availability_file"))

    deduped: dict[tuple[str, str], dict[str, Any]] = {}
    for artifact in artifacts:
        deduped[(artifact["role"], artifact["path"])] = artifact
    return list(deduped.values())


def run_manifest(run: dict[str, Any]) -> dict[str, Any]:
    return {
        "namespace": run["namespace"],
        "config_path": run["config_path"],
        "run_dir": run.get("run_dir"),
        "decision_rows_total": run.get("decision_rows_total", 0),
        "route_executable_rows": run.get("route_executable_rows", 0),
        "route_non_executable_rows": run.get("route_non_executable_rows", 0),
        "successful_entry_rows": run.get("successful_entry_rows", 0),
        "lifecycle_labeled_rows": run.get("lifecycle_labeled_rows", 0),
        "buy_quality_denominator_rows": run.get("buy_quality_denominator_rows", 0),
        "buy_quality_bad": run.get("buy_quality_bad", 0),
        "buy_quality_dirty_good": run.get("buy_quality_dirty_good", 0),
        "buy_quality_good": run.get("buy_quality_good", 0),
        "buy_quality_not_executable": run.get("buy_quality_not_executable", 0),
        "feature_join_executable_labeled_rows": run.get(
            "feature_join_executable_labeled_rows",
            0,
        ),
        "artifacts": artifact_manifests_for_run(run),
    }


def default_expected_contract() -> dict[str, Any]:
    return {
        "allowed_runs": sorted(l1r20.DEFAULT_ALLOWED_L2_NAMESPACES),
        "blocked_runs": sorted(l1r20.DEFAULT_HARD_BLOCKED_NAMESPACES),
        "buy_quality_denominator_rows": EXPECTED_BUY_QUALITY_DENOMINATOR_ROWS,
        "buy_quality_dirty_good": EXPECTED_BUY_QUALITY_DIRTY_GOOD,
        "dirty_good_rate": EXPECTED_BUY_QUALITY_DIRTY_GOOD
        / EXPECTED_BUY_QUALITY_DENOMINATOR_ROWS,
        "excluded_non_executable_rows": EXPECTED_EXCLUDED_NON_EXECUTABLE_ROWS,
        "excluded_unsupported_route_rows": EXPECTED_EXCLUDED_UNSUPPORTED_ROUTE_ROWS,
    }


def validate_contract(preflight_report: dict[str, Any], expected: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    if preflight_report.get("preflight_status") != "pass":
        failures.append("l1r20_preflight_not_pass")
    if preflight_report.get("final_decision") != "GO_L2_EXECUTABLE_SUBSET_LOCKED":
        failures.append("l1r20_final_decision_not_locked")
    if sorted(preflight_report.get("requested_l2_namespaces", [])) != sorted(
        expected["allowed_runs"]
    ):
        failures.append("allowed_l2_run_set_mismatch")

    input_totals = preflight_report.get("input_totals", {})
    excluded_totals = preflight_report.get("excluded_totals", {})
    checks = (
        (
            "buy_quality_denominator_rows_mismatch",
            input_totals.get("buy_quality_denominator_rows"),
            expected["buy_quality_denominator_rows"],
        ),
        (
            "buy_quality_dirty_good_mismatch",
            input_totals.get("buy_quality_dirty_good"),
            expected["buy_quality_dirty_good"],
        ),
        (
            "excluded_non_executable_rows_mismatch",
            excluded_totals.get("excluded_non_executable_rows"),
            expected["excluded_non_executable_rows"],
        ),
        (
            "excluded_unsupported_route_rows_mismatch",
            excluded_totals.get("excluded_unsupported_route_rows"),
            expected["excluded_unsupported_route_rows"],
        ),
    )
    for reason, actual, expected_value in checks:
        if actual != expected_value:
            failures.append(reason)
    if preflight_report.get("missing_requested_l2_namespaces"):
        failures.append("missing_allowed_l2_namespace")
    if preflight_report.get("blocked_requested_l2_namespaces"):
        failures.append("blocked_namespace_in_l2_input")
    if preflight_report.get("disallowed_requested_l2_namespaces"):
        failures.append("unknown_or_disallowed_namespace_in_l2_input")
    if preflight_report.get("unusable_requested_l2_namespaces"):
        failures.append("unusable_namespace_in_l2_input")
    return failures


def build_manifest(
    config_paths: list[Path],
    *,
    l2_input_namespaces: list[str] | None = None,
    allow_unsupported_override: bool = False,
) -> dict[str, Any]:
    preflight = l1r20.build_preflight_report(
        config_paths,
        l2_input_namespaces=l2_input_namespaces,
        allow_unsupported_override=allow_unsupported_override,
    )
    expected = default_expected_contract()
    failures = validate_contract(preflight, expected)
    input_totals = preflight["input_totals"]
    eligible_not_in_buy_quality_denominator = (
        int(input_totals.get("executable_eligible_rows") or 0)
        - int(input_totals.get("buy_quality_denominator_rows") or 0)
    )
    manifest_status = "pass" if not failures else "fail"
    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-L1R21 L2 Input Manifest / Dataset Contract Audit",
        "manifest_status": manifest_status,
        "final_decision": (
            "GO_L2_INPUT_MANIFEST_LOCKED"
            if manifest_status == "pass"
            else "BLOCK_L2_INPUT_MANIFEST_CONTRACT"
        ),
        "contract_failures": failures,
        "expected_contract": expected,
        "actual_contract": {
            "allowed_runs": sorted(preflight.get("requested_l2_namespaces", [])),
            "blocked_runs": sorted(l1r20.DEFAULT_HARD_BLOCKED_NAMESPACES),
            "buy_quality_denominator_rows": input_totals.get("buy_quality_denominator_rows"),
            "buy_quality_dirty_good": input_totals.get("buy_quality_dirty_good"),
            "dirty_good_rate": input_totals.get("dirty_good_rate"),
            "excluded_non_executable_rows": preflight["excluded_totals"].get(
                "excluded_non_executable_rows"
            ),
            "excluded_unsupported_route_rows": preflight["excluded_totals"].get(
                "excluded_unsupported_route_rows"
            ),
        },
        "denominator_gap": {
            "executable_eligible_rows": input_totals.get("executable_eligible_rows"),
            "buy_quality_denominator_rows": input_totals.get("buy_quality_denominator_rows"),
            "eligible_not_in_buy_quality_denominator": eligible_not_in_buy_quality_denominator,
            "meaning": "execution eligible does not equal buy-quality label eligible",
        },
        "input_totals": input_totals,
        "excluded_totals": preflight["excluded_totals"],
        "allowed_run_manifests": [run_manifest(run) for run in preflight["l2_input_runs"]],
        "blocked_runs": preflight["excluded_runs"],
        "hard_fail_conditions": [
            "unknown_or_unlisted_run_in_l2_input",
            "denominator_mismatch",
            "dirty_good_count_mismatch",
            "blocked_namespace_in_l2_input",
            "non_executable_route_rows_in_buy_quality_denominator",
        ],
        "source_preflight": {
            "preflight_status": preflight["preflight_status"],
            "final_decision": preflight["final_decision"],
            "blockers": preflight["blockers"],
        },
    }


def fmt(value: Any) -> str:
    if value is None:
        return "n/a"
    if isinstance(value, float):
        return f"{value:.4f}"
    return str(value)


def render_markdown(manifest: dict[str, Any]) -> str:
    lines = [
        "# P3.7-L1R21 L2 Input Manifest / Dataset Contract Audit",
        "",
        "## Verdict",
        "",
        f"- manifest_status: `{manifest['manifest_status']}`",
        f"- final_decision: `{manifest['final_decision']}`",
    ]
    for failure in manifest["contract_failures"]:
        lines.append(f"- contract_failure: `{failure}`")
    lines.extend(
        [
            "",
            "## Locked Contract",
            "",
            "- allowed_runs: `J4C`, `R16-r1`",
            "- blocked_runs: `R16-r3..R16-r13` and every unknown/unlisted run",
            "- failure_mode: denominator or namespace mismatch is a hard fail",
            "- this is not scoring, threshold tuning, policy promotion, P2/live, or full R16 route-universe approval",
            "",
            "## Expected Vs Actual",
            "",
            "| field | expected | actual |",
            "| --- | ---: | ---: |",
        ]
    )
    for key, expected_value in manifest["expected_contract"].items():
        if isinstance(expected_value, list):
            continue
        actual = manifest["actual_contract"].get(key)
        lines.append(f"| `{key}` | {fmt(expected_value)} | {fmt(actual)} |")
    gap = manifest["denominator_gap"]
    lines.extend(
        [
            "",
            "## Denominator Gap",
            "",
            f"- executable_eligible_rows: `{gap['executable_eligible_rows']}`",
            f"- buy_quality_denominator_rows: `{gap['buy_quality_denominator_rows']}`",
            f"- eligible_not_in_buy_quality_denominator: `{gap['eligible_not_in_buy_quality_denominator']}`",
            f"- meaning: {gap['meaning']}",
            "",
            "## Allowed Run Manifests",
            "",
            "| namespace | decisions | route_exec | route_non_exec | lifecycle_labels | buy_denominator | bad | dirty_good | good | feature_join_exec_labels | artifacts |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for run in manifest["allowed_run_manifests"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{run['namespace']}`",
                    fmt(run["decision_rows_total"]),
                    fmt(run["route_executable_rows"]),
                    fmt(run["route_non_executable_rows"]),
                    fmt(run["lifecycle_labeled_rows"]),
                    fmt(run["buy_quality_denominator_rows"]),
                    fmt(run["buy_quality_bad"]),
                    fmt(run["buy_quality_dirty_good"]),
                    fmt(run["buy_quality_good"]),
                    fmt(run["feature_join_executable_labeled_rows"]),
                    fmt(len(run["artifacts"])),
                ]
            )
            + " |"
        )
    lines.extend(
        [
            "",
            "## Input Artifacts",
            "",
            "| namespace | role | path | rows | sha256 |",
            "| --- | --- | --- | ---: | --- |",
        ]
    )
    for run in manifest["allowed_run_manifests"]:
        for artifact in run["artifacts"]:
            if not artifact.get("exists"):
                continue
            sha = artifact.get("sha256")
            lines.append(
                "| "
                + " | ".join(
                    [
                        f"`{run['namespace']}`",
                        f"`{artifact['role']}`",
                        f"`{artifact['path']}`",
                        fmt(artifact.get("jsonl_rows")),
                        f"`{sha}`" if sha else "`n/a`",
                    ]
                )
                + " |"
            )
    lines.extend(
        [
            "",
            "## Blocked Run Classes",
            "",
            "| namespace | class | decisions | route_non_exec | unsupported_route | buy_denominator |",
            "| --- | --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for row in manifest["blocked_runs"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['namespace']}`",
                    f"`{row['exclusion_class']}`",
                    fmt(row["decision_rows_total"]),
                    fmt(row["route_non_executable_rows"]),
                    fmt(row["execution_feasibility_reject_rows"]),
                    fmt(row["buy_quality_denominator_rows"]),
                ]
            )
            + " |"
        )
    lines.extend(["", "## Hard Fail Conditions", ""])
    for condition in manifest["hard_fail_conditions"]:
        lines.append(f"- `{condition}`")
    return "\n".join(lines) + "\n"


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        action="append",
        dest="configs",
        help="Discovery config path. Defaults to L1R19 J4C/R16/L1 config set.",
    )
    parser.add_argument(
        "--l2-input-namespace",
        action="append",
        dest="l2_input_namespaces",
        help="Namespace proposed as L2 input. Defaults to the locked J4C/R16-r1 set.",
    )
    parser.add_argument("--allow-unsupported-override", action="store_true")
    parser.add_argument("--output-json", type=Path, default=DEFAULT_OUTPUT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_OUTPUT_MD)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    config_paths = [Path(path) for path in (args.configs or l1r19.DEFAULT_CONFIGS)]
    manifest = build_manifest(
        config_paths,
        l2_input_namespaces=args.l2_input_namespaces,
        allow_unsupported_override=args.allow_unsupported_override,
    )
    write_json(args.output_json, manifest)
    write_text(args.output_md, render_markdown(manifest))
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True))
    if manifest["manifest_status"] != "pass":
        raise SystemExit(2)


if __name__ == "__main__":
    main()
