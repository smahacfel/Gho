#!/usr/bin/env python3
"""Static audit for the Shadow V2 PR12 fidelity validation burnin plan.

This script validates the plan/config contract only. It does not start runs,
stop runs, touch R51, clean artifacts, read raw JSONL, or grant strategy proof.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback is explicit.
    tomllib = None  # type: ignore[assignment]


SCHEMA = "shadow_v2_validation_burnin_plan_audit_v1"
PLAN_SCHEMA = "shadow_v2_validation_burnin_plan_v1"
SIMULATION_CONTRACT_VERSION = "shadow_burnin_simulation_v2_20260629"

DEFAULT_PLAN = Path("configs/rollout/shadow_v2_fidelity_validation_burnin_plan.toml")
DEFAULT_GATES = Path("reports/selector/shadow_v2_acceptance_gates.csv")
DEFAULT_ARTIFACT_CONTRACT = Path("reports/selector/shadow_v2_manifest_artifact_contract.csv")
DEFAULT_DOWNGRADE_MATRIX = Path("reports/selector/shadow_v2_legacy_downgrade_matrix.csv")

MUST_BE_FALSE = {
    "enabled",
    "run_start_allowed",
    "runtime_approval",
    "shadow_close_only_approval",
    "active_close_approval",
    "strategy_proof_enabled",
    "rce_proof_enabled",
    "selector_proof_enabled",
    "edge_proof_enabled",
    "r51_touch_allowed",
    "cleanup_allowed_before_manifest",
    "raw_jsonl_git_staging_allowed",
}

MUST_BE_TRUE = {
    "plan_only",
    "logging_only",
    "required_pr14_for_live_equivalence",
}

REQUIRED_PATH_MODES = {
    "shadow_path_dense_3s",
    "shadow_path_standard_120s",
    "shadow_path_long_500s",
}

REQUIRED_HORIZONS_MS = {2000, 3000, 10000, 30000, 120000, 300000, 500000}

REQUIRED_ARTIFACTS = {
    "pre_run_manifest.json",
    "post_run_manifest.json",
    "shadow_position_event_v2.jsonl",
    "shadow_replay_v2.jsonl",
    "shadow_lifecycle_v2.jsonl",
    "shadow_path_density_v2.jsonl",
    "shadow_v2_manifest_report.csv",
    "shadow_v2_fidelity_validation_report.md",
    "shadow_v2_golden_traces_manifest.csv",
}

REQUIRED_RESEARCH_GATES = {
    "GATE_ENTRY_RECON_COVERAGE",
    "GATE_EXIT_RECON_COVERAGE",
    "GATE_TERMINAL_RECONCILIATION",
    "GATE_DUPLICATE_TERMINALS",
    "GATE_AMBIGUOUS_FALLBACK",
    "GATE_TEMPORAL_LEAKAGE",
    "GATE_CLOCK_DOMAIN",
    "GATE_EVENT_ORDER_KEY",
    "GATE_DENSITY_2S_3S",
    "GATE_DENSITY_300S_500S",
    "GATE_FIXTURES",
    "GATE_MANIFESTS",
}

REQUIRED_PR12_GATES = {
    "GATE_PR12_PLAN_CONTRACT",
    "GATE_PR12_NOT_STRATEGY_PROOF_GUARD",
    "GATE_PR12_RESEARCH_GRADE_GATE_COVERAGE",
}


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is None:
        raise RuntimeError("tomllib is required; run with Python 3.11+")
    with path.open("rb") as handle:
        return tomllib.load(handle)


def load_gate_ids(path: Path) -> tuple[set[str], list[str]]:
    errors: list[str] = []
    if not path.exists():
        return set(), [f"missing acceptance gates CSV: {path}"]

    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if "gate_id" not in (reader.fieldnames or []):
            return set(), [f"{path} missing gate_id column"]
        return {row["gate_id"].strip() for row in reader if row.get("gate_id")}, errors


def require_set_contains(name: str, actual: set[Any], required: set[Any], blockers: list[str]) -> None:
    missing = sorted(required.difference(actual))
    if missing:
        blockers.append(f"{name} missing required values: {missing}")


def validate_plan(plan_path: Path, gates_path: Path) -> dict[str, Any]:
    blockers: list[str] = []
    if not plan_path.exists():
        blockers.append(f"missing plan file: {plan_path}")
        plan: dict[str, Any] = {}
    else:
        try:
            raw = load_toml(plan_path)
            plan = raw.get("shadow_v2_validation_burnin_plan", {})
        except Exception as exc:  # noqa: BLE001 - surfaced as audit blocker.
            blockers.append(f"failed to parse plan TOML: {exc}")
            plan = {}

    if not isinstance(plan, dict) or not plan:
        blockers.append("missing [shadow_v2_validation_burnin_plan] table")
        plan = {}

    if plan.get("schema") != PLAN_SCHEMA:
        blockers.append(f"schema must be {PLAN_SCHEMA}")
    if plan.get("simulation_contract_version") != SIMULATION_CONTRACT_VERSION:
        blockers.append(f"simulation_contract_version must be {SIMULATION_CONTRACT_VERSION}")
    if plan.get("plan_status") != "PLAN_ONLY":
        blockers.append("plan_status must be PLAN_ONLY")
    if plan.get("validation_mode") != "FIDELITY_ONLY":
        blockers.append("validation_mode must be FIDELITY_ONLY")
    if plan.get("max_verdict_without_live_calibration") != "SHADOW_V2_RESEARCH_GRADE_ONLY":
        blockers.append("max verdict without PR14 calibration must be SHADOW_V2_RESEARCH_GRADE_ONLY")

    for field in sorted(MUST_BE_FALSE):
        if plan.get(field) is not False:
            blockers.append(f"{field} must be false")
    for field in sorted(MUST_BE_TRUE):
        if plan.get(field) is not True:
            blockers.append(f"{field} must be true")

    require_set_contains(
        "required_path_modes",
        set(plan.get("required_path_modes", [])),
        REQUIRED_PATH_MODES,
        blockers,
    )
    require_set_contains(
        "required_horizons_ms",
        {int(value) for value in plan.get("required_horizons_ms", [])},
        REQUIRED_HORIZONS_MS,
        blockers,
    )
    require_set_contains(
        "required_artifacts",
        set(plan.get("required_artifacts", [])),
        REQUIRED_ARTIFACTS,
        blockers,
    )
    require_set_contains(
        "required_research_grade_gates",
        set(plan.get("required_research_grade_gates", [])),
        REQUIRED_RESEARCH_GATES,
        blockers,
    )

    gate_ids, gate_errors = load_gate_ids(gates_path)
    blockers.extend(gate_errors)
    require_set_contains("acceptance_gates", gate_ids, REQUIRED_PR12_GATES, blockers)

    forbidden = set(plan.get("forbidden_proof_types", []))
    require_set_contains(
        "forbidden_proof_types",
        forbidden,
        {
            "strategy_proof",
            "rce_proof",
            "selector_proof",
            "edge_proof",
            "runtime_approval_proof",
            "live_equivalence_proof",
        },
        blockers,
    )

    return {
        "schema": SCHEMA,
        "plan_path": str(plan_path),
        "acceptance_gates_path": str(gates_path),
        "plan_id": plan.get("plan_id", ""),
        "plan_status": plan.get("plan_status", ""),
        "validation_mode": plan.get("validation_mode", ""),
        "run_start_allowed": plan.get("run_start_allowed"),
        "runtime_approval": plan.get("runtime_approval"),
        "strategy_proof_enabled": plan.get("strategy_proof_enabled"),
        "required_research_gate_count": len(set(plan.get("required_research_grade_gates", []))),
        "required_artifact_count": len(set(plan.get("required_artifacts", []))),
        "required_horizon_count": len(set(plan.get("required_horizons_ms", []))),
        "status": "PASS" if not blockers else "BLOCKED",
        "blockers": blockers,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate PR12 Shadow V2 fidelity validation burnin plan contract only."
    )
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--acceptance-gates", type=Path, default=DEFAULT_GATES)
    parser.add_argument("--strict", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    result = validate_plan(args.plan, args.acceptance_gates)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 1 if args.strict and result["blockers"] else 0


if __name__ == "__main__":
    sys.exit(main())
