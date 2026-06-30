#!/usr/bin/env python3
"""Audit Shadow V2 PR14 live-confirmed calibration dataset contract.

Default mode validates the static PR14 contract only. It does not start runs,
touch R51, read live providers, submit transactions, clean artifacts, or grant
live-equivalence. A real local calibration dataset is audited only when
--dataset-root is provided.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback is explicit.
    tomllib = None  # type: ignore[assignment]


SCHEMA = "shadow_v2_live_calibration_audit_v1"
CONTRACT_SCHEMA = "shadow_v2_live_confirmed_calibration_contract_v1"
SIMULATION_CONTRACT_VERSION = "shadow_burnin_simulation_v2_20260629"
MAX_VERDICT_WITHOUT_DATASET = "SHADOW_V2_RESEARCH_GRADE_ONLY"

DEFAULT_CONTRACT = Path("configs/rollout/shadow_v2_live_confirmed_calibration_contract.toml")
DEFAULT_FIELD_MANIFEST = Path("reports/selector/shadow_v2_live_calibration_schema_manifest.csv")
DEFAULT_ACCEPTANCE_GATES = Path("reports/selector/shadow_v2_acceptance_gates.csv")

REQUIRED_RECORD_SCHEMAS = {
    "live_calibration_manifest_v1",
    "live_transaction_attempt_v1",
    "live_confirmed_entry_fill_v1",
    "live_confirmed_exit_fill_v1",
    "live_calibration_comparison_v1",
}

REQUIRED_FILES = {
    "manifest": ("live_calibration_manifest.json", "live_calibration_manifest_v1"),
    "attempts": ("live_transaction_attempts.jsonl", "live_transaction_attempt_v1"),
    "entry_fills": ("live_confirmed_entry_fills.jsonl", "live_confirmed_entry_fill_v1"),
    "exit_fills": ("live_confirmed_exit_fills.jsonl", "live_confirmed_exit_fill_v1"),
    "comparisons": ("live_calibration_comparison.jsonl", "live_calibration_comparison_v1"),
}

REQUIRED_PR14_GATES = {
    "GATE_PR14_CALIBRATION_CONTRACT",
    "GATE_PR14_FIXTURES",
    "GATE_PR14_CALIBRATION",
    "GATE_QUOTE_FILL_DIVERGENCE",
}

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
    "tx_path_changes_allowed",
    "live_collection_enabled",
    "r51_touch_allowed",
    "cleanup_allowed_before_manifest",
    "raw_jsonl_git_staging_allowed",
}

MUST_BE_TRUE = {
    "contract_only",
    "required_for_live_equivalence",
    "no_shadow_assumption_as_live",
}

REQUIRED_TELEMETRY_FIELDS = {
    "decision_ts_ms",
    "submit_ts_ms",
    "landing_ts_ms",
    "decision_to_submit_ms",
    "submit_to_land_ms",
    "landing_slot",
    "fill_status",
    "failure_mode",
    "quote_price",
    "fill_price",
    "realized_slippage_bps",
    "quote_fill_diff_bps",
    "own_impact_bps",
    "fee_bps",
    "priority_fee_lamports",
    "jito_tip_lamports",
    "account_state_delay_ms",
    "stream_delay_ms",
}

MANIFEST_REQUIRED_FIELDS = {
    "schema",
    "calibration_dataset_id",
    "created_at_wall_ms",
    "simulation_contract_version",
    "dataset_status",
    "source_mode",
    "required_for_live_equivalence",
    "max_verdict_without_dataset",
    "files",
}

ATTEMPT_REQUIRED_FIELDS = {
    "schema",
    "calibration_dataset_id",
    "attempt_id",
    "run_id",
    "session_id",
    "position_id",
    "side",
    "pool_id",
    "base_mint",
    "decision_ts_ms",
    "submit_ts_ms",
    "landing_ts_ms",
    "decision_to_submit_ms",
    "submit_to_land_ms",
    "landing_slot",
    "tx_signature",
    "fill_status",
    "failure_mode",
    "priority_fee_lamports",
    "jito_tip_lamports",
    "bundle_status",
    "compute_units_consumed",
    "min_out",
    "quote_price",
}

ENTRY_FILL_REQUIRED_FIELDS = {
    "schema",
    "calibration_dataset_id",
    "attempt_id",
    "fill_id",
    "run_id",
    "session_id",
    "position_id",
    "pool_id",
    "base_mint",
    "tx_signature",
    "fill_status",
    "decision_ts_ms",
    "submit_ts_ms",
    "landing_ts_ms",
    "landing_slot",
    "amount_in_sol_lamports",
    "amount_out_tokens_raw",
    "quote_price",
    "fill_price",
    "realized_slippage_bps",
    "quote_fill_diff_bps",
    "own_impact_bps",
    "fee_bps",
    "priority_fee_lamports",
    "jito_tip_lamports",
    "account_state_delay_ms",
    "stream_delay_ms",
    "pool_state_before_ref",
    "pool_state_after_ref",
    "confirmation_status",
}

EXIT_FILL_REQUIRED_FIELDS = {
    "schema",
    "calibration_dataset_id",
    "attempt_id",
    "fill_id",
    "run_id",
    "session_id",
    "position_id",
    "pool_id",
    "base_mint",
    "tx_signature",
    "fill_status",
    "decision_ts_ms",
    "submit_ts_ms",
    "landing_ts_ms",
    "landing_slot",
    "amount_in_tokens_raw",
    "amount_out_sol_lamports",
    "quote_price",
    "fill_price",
    "realized_slippage_bps",
    "quote_fill_diff_bps",
    "own_impact_bps",
    "fee_bps",
    "priority_fee_lamports",
    "jito_tip_lamports",
    "account_state_delay_ms",
    "stream_delay_ms",
    "pool_state_before_ref",
    "pool_state_after_ref",
    "confirmation_status",
}

COMPARISON_REQUIRED_FIELDS = {
    "schema",
    "calibration_dataset_id",
    "comparison_id",
    "position_id",
    "side",
    "model_version",
    "simulated_fill_price",
    "live_fill_price",
    "model_error_bps",
    "latency_bucket_ms",
    "slippage_error_bps",
    "outcome_match",
    "limitations",
}

REQUIRED_FIELDS_BY_SCHEMA = {
    "live_transaction_attempt_v1": ATTEMPT_REQUIRED_FIELDS,
    "live_confirmed_entry_fill_v1": ENTRY_FILL_REQUIRED_FIELDS,
    "live_confirmed_exit_fill_v1": EXIT_FILL_REQUIRED_FIELDS,
    "live_calibration_comparison_v1": COMPARISON_REQUIRED_FIELDS,
}

NON_NEGATIVE_FIELDS = {
    "decision_ts_ms",
    "submit_ts_ms",
    "landing_ts_ms",
    "decision_to_submit_ms",
    "submit_to_land_ms",
    "landing_slot",
    "priority_fee_lamports",
    "jito_tip_lamports",
    "compute_units_consumed",
    "account_state_delay_ms",
    "stream_delay_ms",
    "amount_in_sol_lamports",
    "amount_out_tokens_raw",
    "amount_in_tokens_raw",
    "amount_out_sol_lamports",
}

ALLOWED_ATTEMPT_STATUSES = {"FILLED", "NO_FILL", "FAILED"}


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is None:
        raise RuntimeError("tomllib is required; run with Python 3.11+")
    with path.open("rb") as handle:
        return tomllib.load(handle)


def load_gate_ids(path: Path) -> tuple[set[str], list[str]]:
    if not path.exists():
        return set(), [f"missing acceptance gates CSV: {path}"]

    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if "gate_id" not in (reader.fieldnames or []):
            return set(), [f"{path} missing gate_id column"]
        return {row["gate_id"].strip() for row in reader if row.get("gate_id")}, []


def load_field_manifest(path: Path) -> tuple[dict[str, set[str]], list[str]]:
    if not path.exists():
        return {}, [f"missing live calibration field manifest: {path}"]

    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        required_columns = {"record_schema", "field_name", "required", "clock_domain"}
        missing = required_columns.difference(reader.fieldnames or [])
        if missing:
            return {}, [f"{path} missing columns: {sorted(missing)}"]

        by_schema: dict[str, set[str]] = {}
        errors: list[str] = []
        for index, row in enumerate(reader, start=2):
            schema = (row.get("record_schema") or "").strip()
            field = (row.get("field_name") or "").strip()
            required = (row.get("required") or "").strip().lower() == "true"
            clock_domain = (row.get("clock_domain") or "").strip()
            if not schema or not field:
                errors.append(f"{path}:{index} missing schema or field name")
                continue
            if not clock_domain:
                errors.append(f"{path}:{index} missing clock_domain for {schema}.{field}")
            if required:
                by_schema.setdefault(schema, set()).add(field)
        return by_schema, errors


def require_contains(name: str, actual: set[str], required: set[str], blockers: list[str]) -> None:
    missing = sorted(required.difference(actual))
    if missing:
        blockers.append(f"{name} missing required values: {missing}")


def validate_contract(
    contract_path: Path,
    field_manifest_path: Path,
    acceptance_gates_path: Path,
) -> tuple[dict[str, Any], list[str]]:
    blockers: list[str] = []
    if not contract_path.exists():
        blockers.append(f"missing live calibration contract: {contract_path}")
        contract: dict[str, Any] = {}
    else:
        try:
            raw = load_toml(contract_path)
            contract = raw.get("shadow_v2_live_confirmed_calibration", {})
        except Exception as exc:  # noqa: BLE001 - surfaced as audit blocker.
            blockers.append(f"failed to parse live calibration contract TOML: {exc}")
            contract = {}

    if not isinstance(contract, dict) or not contract:
        blockers.append("missing [shadow_v2_live_confirmed_calibration] table")
        contract = {}

    if contract.get("schema") != CONTRACT_SCHEMA:
        blockers.append(f"schema must be {CONTRACT_SCHEMA}")
    if contract.get("simulation_contract_version") != SIMULATION_CONTRACT_VERSION:
        blockers.append(f"simulation_contract_version must be {SIMULATION_CONTRACT_VERSION}")
    if contract.get("contract_status") != "CONTRACT_ONLY":
        blockers.append("contract_status must be CONTRACT_ONLY")
    if contract.get("max_verdict_without_dataset") != MAX_VERDICT_WITHOUT_DATASET:
        blockers.append(f"max_verdict_without_dataset must be {MAX_VERDICT_WITHOUT_DATASET}")

    for field in sorted(MUST_BE_FALSE):
        if contract.get(field) is not False:
            blockers.append(f"{field} must be false")
    for field in sorted(MUST_BE_TRUE):
        if contract.get(field) is not True:
            blockers.append(f"{field} must be true")

    require_contains(
        "required_record_schemas",
        set(contract.get("required_record_schemas", [])),
        REQUIRED_RECORD_SCHEMAS,
        blockers,
    )
    require_contains(
        "required_telemetry_fields",
        set(contract.get("required_telemetry_fields", [])),
        REQUIRED_TELEMETRY_FIELDS,
        blockers,
    )

    gate_ids, gate_errors = load_gate_ids(acceptance_gates_path)
    blockers.extend(gate_errors)
    require_contains("acceptance_gates", gate_ids, REQUIRED_PR14_GATES, blockers)

    manifest_fields, manifest_errors = load_field_manifest(field_manifest_path)
    blockers.extend(manifest_errors)
    require_contains(
        "field_manifest_schemas",
        set(manifest_fields.keys()),
        REQUIRED_RECORD_SCHEMAS,
        blockers,
    )
    for schema, required_fields in REQUIRED_FIELDS_BY_SCHEMA.items():
        require_contains(
            f"{schema} manifest fields",
            manifest_fields.get(schema, set()),
            required_fields,
            blockers,
        )
    require_contains(
        "live_calibration_manifest_v1 manifest fields",
        manifest_fields.get("live_calibration_manifest_v1", set()),
        MANIFEST_REQUIRED_FIELDS,
        blockers,
    )

    return {
        "contract_path": str(contract_path),
        "field_manifest_path": str(field_manifest_path),
        "acceptance_gates_path": str(acceptance_gates_path),
        "contract_status": contract.get("contract_status", ""),
        "required_schema_count": len(set(contract.get("required_record_schemas", []))),
        "required_telemetry_field_count": len(set(contract.get("required_telemetry_fields", []))),
    }, blockers


def is_non_empty(value: Any) -> bool:
    if value is None:
        return False
    if isinstance(value, str):
        return bool(value.strip())
    if isinstance(value, (list, dict)):
        return bool(value)
    return True


def validate_required_fields(
    record: dict[str, Any],
    schema: str,
    required_fields: set[str],
    record_label: str,
    blockers: list[str],
) -> None:
    for field in sorted(required_fields):
        if not is_non_empty(record.get(field)):
            blockers.append(f"{record_label}: missing or empty required field {field}")

    if record.get("schema") != schema:
        blockers.append(f"{record_label}: schema must be {schema}")

    for field in sorted(NON_NEGATIVE_FIELDS.intersection(required_fields)):
        value = record.get(field)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            blockers.append(f"{record_label}: {field} must be a non-negative integer")


def validate_latency(record: dict[str, Any], record_label: str, blockers: list[str]) -> None:
    decision = record.get("decision_ts_ms")
    submit = record.get("submit_ts_ms")
    landing = record.get("landing_ts_ms")
    decision_to_submit = record.get("decision_to_submit_ms")
    submit_to_land = record.get("submit_to_land_ms")

    values = [decision, submit, landing, decision_to_submit, submit_to_land]
    if any(isinstance(value, bool) or not isinstance(value, int) for value in values):
        return

    if submit < decision:
        blockers.append(f"{record_label}: submit_ts_ms is before decision_ts_ms")
    if landing < submit:
        blockers.append(f"{record_label}: landing_ts_ms is before submit_ts_ms")
    if decision_to_submit != submit - decision:
        blockers.append(f"{record_label}: decision_to_submit_ms does not match submit-decision")
    if submit_to_land != landing - submit:
        blockers.append(f"{record_label}: submit_to_land_ms does not match landing-submit")


def validate_attempt(record: dict[str, Any], record_label: str, blockers: list[str]) -> None:
    validate_required_fields(
        record,
        "live_transaction_attempt_v1",
        ATTEMPT_REQUIRED_FIELDS,
        record_label,
        blockers,
    )
    validate_latency(record, record_label, blockers)

    status = record.get("fill_status")
    if status not in ALLOWED_ATTEMPT_STATUSES:
        blockers.append(f"{record_label}: fill_status must be one of {sorted(ALLOWED_ATTEMPT_STATUSES)}")
    if status in {"NO_FILL", "FAILED"}:
        failure_mode = str(record.get("failure_mode", "")).strip()
        if failure_mode in {"", "NONE", "UNKNOWN"}:
            blockers.append(f"{record_label}: failure_mode must be explicit for {status}")


def validate_fill_record(
    record: dict[str, Any],
    schema: str,
    required_fields: set[str],
    record_label: str,
    blockers: list[str],
) -> None:
    validate_required_fields(record, schema, required_fields, record_label, blockers)
    validate_latency(record, record_label, blockers)
    if record.get("fill_status") != "FILLED":
        blockers.append(f"{record_label}: live-confirmed fill rows must use fill_status FILLED")
    if str(record.get("tx_signature", "")).strip() in {"", "UNKNOWN"}:
        blockers.append(f"{record_label}: tx_signature must be a concrete live signature")


def read_jsonl(path: Path, schema: str, blockers: list[str]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.exists():
        blockers.append(f"missing required dataset file: {path}")
        return rows

    with path.open("r", encoding="utf-8") as handle:
        for index, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                blockers.append(f"{path}:{index}: malformed JSONL row: {exc}")
                continue
            if not isinstance(value, dict):
                blockers.append(f"{path}:{index}: JSONL row must be an object")
                continue
            if value.get("schema") != schema:
                blockers.append(f"{path}:{index}: schema must be {schema}")
            rows.append(value)
    return rows


def validate_manifest(path: Path, blockers: list[str]) -> dict[str, Any]:
    if not path.exists():
        blockers.append(f"missing required dataset file: {path}")
        return {}

    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        blockers.append(f"{path}: malformed JSON: {exc}")
        return {}

    if not isinstance(value, dict):
        blockers.append(f"{path}: manifest must be a JSON object")
        return {}

    validate_required_fields(
        value,
        "live_calibration_manifest_v1",
        MANIFEST_REQUIRED_FIELDS,
        str(path),
        blockers,
    )
    if value.get("simulation_contract_version") != SIMULATION_CONTRACT_VERSION:
        blockers.append(f"{path}: simulation_contract_version must be {SIMULATION_CONTRACT_VERSION}")
    if value.get("required_for_live_equivalence") is not True:
        blockers.append(f"{path}: required_for_live_equivalence must be true")
    if value.get("max_verdict_without_dataset") != MAX_VERDICT_WITHOUT_DATASET:
        blockers.append(f"{path}: max_verdict_without_dataset must be {MAX_VERDICT_WITHOUT_DATASET}")
    return value


def validate_dataset(dataset_root: Path) -> tuple[dict[str, Any], list[str]]:
    blockers: list[str] = []
    if not dataset_root.exists() or not dataset_root.is_dir():
        return {"dataset_root": str(dataset_root), "dataset_status": "MISSING"}, [
            f"dataset_root must be an existing directory: {dataset_root}"
        ]

    manifest_path = dataset_root / REQUIRED_FILES["manifest"][0]
    manifest = validate_manifest(manifest_path, blockers)

    attempts = read_jsonl(
        dataset_root / REQUIRED_FILES["attempts"][0],
        "live_transaction_attempt_v1",
        blockers,
    )
    entry_fills = read_jsonl(
        dataset_root / REQUIRED_FILES["entry_fills"][0],
        "live_confirmed_entry_fill_v1",
        blockers,
    )
    exit_fills = read_jsonl(
        dataset_root / REQUIRED_FILES["exit_fills"][0],
        "live_confirmed_exit_fill_v1",
        blockers,
    )
    comparisons = read_jsonl(
        dataset_root / REQUIRED_FILES["comparisons"][0],
        "live_calibration_comparison_v1",
        blockers,
    )

    status_counts: Counter[str] = Counter()
    attempt_ids: set[str] = set()
    for index, row in enumerate(attempts, start=1):
        label = f"live_transaction_attempts.jsonl:{index}"
        validate_attempt(row, label, blockers)
        if is_non_empty(row.get("attempt_id")):
            attempt_ids.add(str(row["attempt_id"]))
        if is_non_empty(row.get("fill_status")):
            status_counts[str(row["fill_status"])] += 1

    for index, row in enumerate(entry_fills, start=1):
        label = f"live_confirmed_entry_fills.jsonl:{index}"
        validate_fill_record(
            row,
            "live_confirmed_entry_fill_v1",
            ENTRY_FILL_REQUIRED_FIELDS,
            label,
            blockers,
        )
        if str(row.get("attempt_id", "")) not in attempt_ids:
            blockers.append(f"{label}: attempt_id has no matching transaction attempt")

    for index, row in enumerate(exit_fills, start=1):
        label = f"live_confirmed_exit_fills.jsonl:{index}"
        validate_fill_record(
            row,
            "live_confirmed_exit_fill_v1",
            EXIT_FILL_REQUIRED_FIELDS,
            label,
            blockers,
        )
        if str(row.get("attempt_id", "")) not in attempt_ids:
            blockers.append(f"{label}: attempt_id has no matching transaction attempt")

    for index, row in enumerate(comparisons, start=1):
        label = f"live_calibration_comparison.jsonl:{index}"
        validate_required_fields(
            row,
            "live_calibration_comparison_v1",
            COMPARISON_REQUIRED_FIELDS,
            label,
            blockers,
        )

    if not attempts:
        blockers.append("live_transaction_attempts.jsonl must contain at least one row")
    if not entry_fills:
        blockers.append("live_confirmed_entry_fills.jsonl must contain at least one row")
    if not exit_fills:
        blockers.append("live_confirmed_exit_fills.jsonl must contain at least one row")
    if not comparisons:
        blockers.append("live_calibration_comparison.jsonl must contain at least one row")
    if status_counts["FILLED"] == 0:
        blockers.append("dataset must contain at least one FILLED attempt")
    if status_counts["NO_FILL"] + status_counts["FAILED"] == 0:
        blockers.append("dataset must contain at least one NO_FILL or FAILED attempt")

    manifest_files = set(manifest.get("files", [])) if isinstance(manifest.get("files"), list) else set()
    require_contains(
        "live calibration manifest files",
        {str(value) for value in manifest_files},
        {value[0] for value in REQUIRED_FILES.values()},
        blockers,
    )

    return {
        "dataset_root": str(dataset_root),
        "dataset_status": "PROVIDED",
        "calibration_dataset_id": manifest.get("calibration_dataset_id", ""),
        "attempt_rows": len(attempts),
        "entry_fill_rows": len(entry_fills),
        "exit_fill_rows": len(exit_fills),
        "comparison_rows": len(comparisons),
        "status_counts": dict(sorted(status_counts.items())),
    }, blockers


def audit(
    contract_path: Path,
    field_manifest_path: Path,
    acceptance_gates_path: Path,
    dataset_root: Path | None,
    require_dataset: bool = False,
) -> dict[str, Any]:
    contract_result, contract_blockers = validate_contract(
        contract_path,
        field_manifest_path,
        acceptance_gates_path,
    )
    blockers = list(contract_blockers)

    dataset_result: dict[str, Any] = {
        "dataset_status": "NOT_PROVIDED",
        "dataset_root": "",
    }
    if dataset_root is not None:
        dataset_result, dataset_blockers = validate_dataset(dataset_root)
        blockers.extend(dataset_blockers)
    elif require_dataset:
        blockers.append("LIVE_CONFIRMED_CALIBRATION_DATASET_NOT_PROVIDED")

    if blockers:
        status = "BLOCKED"
    elif dataset_root is not None:
        status = "PASS"
    else:
        status = "CONTRACT_READY"

    return {
        "schema": SCHEMA,
        "simulation_contract_version": SIMULATION_CONTRACT_VERSION,
        "contract": contract_result,
        "dataset": dataset_result,
        "status": status,
        "blockers": blockers,
        "pr14_calibration_gate_pass": status == "PASS",
        "live_equivalence_grade_allowed": False,
        "live_equivalence_grade_reason": (
            "PR14 is necessary but not sufficient; final live-equivalence also requires "
            "latency model, failure/no-fill model, slippage/impact/fee model, quote/fill "
            "divergence model and severe gap audit."
        ),
        "max_verdict_without_dataset": MAX_VERDICT_WITHOUT_DATASET,
        "runtime_changed": False,
        "run_started": False,
        "r51_touched": False,
        "raw_jsonl_git_staging_allowed": False,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate PR14 Shadow V2 live-confirmed calibration contract and optional local dataset."
    )
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT)
    parser.add_argument("--field-manifest", type=Path, default=DEFAULT_FIELD_MANIFEST)
    parser.add_argument("--acceptance-gates", type=Path, default=DEFAULT_ACCEPTANCE_GATES)
    parser.add_argument("--dataset-root", type=Path)
    parser.add_argument("--require-dataset", action="store_true")
    parser.add_argument("--strict", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    result = audit(
        args.contract,
        args.field_manifest,
        args.acceptance_gates,
        args.dataset_root,
        args.require_dataset,
    )
    print(json.dumps(result, indent=2, sort_keys=True))
    if args.require_dataset and result["status"] != "PASS":
        return 1
    return 1 if args.strict and result["status"] == "BLOCKED" else 0


if __name__ == "__main__":
    sys.exit(main())
