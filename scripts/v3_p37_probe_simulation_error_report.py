#!/usr/bin/env python3
"""Summarize P3.7 counterfactual probe simulation instruction errors."""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


INSTRUCTION_ERROR_RE = re.compile(r"InstructionError\((\d+),\s*Custom\((\d+)\)\)")
PUMPFUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"


def _load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for line_no, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise SystemExit(f"invalid JSONL in {path}:{line_no}: {exc}") from exc
    return rows


def _resolve_path(raw: str | None, config_path: Path) -> Path | None:
    if not raw:
        return None
    path = Path(raw)
    if path.is_absolute():
        return path
    return (config_path.parent / path).resolve()


def _config_transport_path(config_path: Path) -> Path | None:
    with config_path.open("rb") as handle:
        data = tomllib.load(handle)
    probe = data.get("p37_shadow_probe", {})
    return _resolve_path(probe.get("transport_log_path"), config_path)


def _parse_instruction_error(message: str | None) -> tuple[int | None, int | None]:
    if not message:
        return None, None
    match = INSTRUCTION_ERROR_RE.search(message)
    if not match:
        return None, None
    return int(match.group(1)), int(match.group(2))


def _best_effort_error_name(program_id: str | None, custom_code: int | None) -> tuple[str | None, str | None]:
    if custom_code == 2006 and program_id == PUMPFUN_PROGRAM_ID:
        return "anchor_constraint_seeds", "simulation_account_layout_mismatch"
    if custom_code == 2006:
        return "anchor_constraint_seeds_best_effort", "simulation_account_layout_mismatch_unclassified"
    if custom_code == 6002 and program_id == PUMPFUN_PROGRAM_ID:
        return "too_much_sol_required", "simulation_slippage_or_price_mismatch"
    if custom_code == 6002:
        return "too_much_sol_required_best_effort", "simulation_slippage_or_price_mismatch"
    if custom_code is not None:
        return "unknown_custom_program_error", "simulation_instruction_error"
    return None, None


def _anchor_error_from_logs(log_tail: list[str]) -> tuple[str | None, str | None]:
    joined = "\n".join(log_tail)
    if "TooMuchSolRequired" in joined:
        return "too_much_sol_required", "simulation_slippage_or_price_mismatch"
    if "ConstraintSeeds" in joined:
        return "anchor_constraint_seeds", "simulation_account_layout_mismatch"
    return None, None


def analyze_rows(rows: list[dict[str, Any]]) -> dict[str, Any]:
    error_rows = [
        row
        for row in rows
        if row.get("err") or row.get("execution_outcome") == "counterfactual_shadow_probe_simulation_error"
    ]
    analyzed: list[dict[str, Any]] = []
    category_counts: Counter[str] = Counter()
    code_counts: Counter[str] = Counter()
    program_counts: Counter[str] = Counter()
    bucket_counts: Counter[str] = Counter()

    for row in error_rows:
        instruction_index = row.get("simulation_error_instruction_index")
        custom_code = row.get("simulation_error_custom_code")
        if instruction_index is None or custom_code is None:
            parsed_index, parsed_code = _parse_instruction_error(row.get("err") or row.get("simulation_error_message"))
            instruction_index = instruction_index if instruction_index is not None else parsed_index
            custom_code = custom_code if custom_code is not None else parsed_code

        program_id = row.get("simulation_error_program_id")
        error_name = row.get("simulation_error_program_error_name")
        category = row.get("simulation_error_category")
        log_tail = row.get("simulation_error_log_tail") or []
        log_error_name, log_category = _anchor_error_from_logs(log_tail)
        if log_error_name:
            error_name = log_error_name
            category = log_category
        if not error_name or not category:
            error_name, fallback_category = _best_effort_error_name(program_id, custom_code)
            category = category or fallback_category

        if not log_tail:
            if category == "simulation_account_layout_mismatch_unclassified":
                category = "simulation_account_layout_mismatch_unclassified_missing_q4_fields"
            elif category is None:
                category = "simulation_error_unclassified_missing_q4_fields"

        category_counts[category or "unknown"] += 1
        code_counts[str(custom_code) if custom_code is not None else "unknown"] += 1
        program_counts[program_id or "unknown"] += 1
        bucket_counts[row.get("probe_bucket") or "unknown"] += 1

        analyzed.append(
            {
                "probe_id": row.get("probe_id"),
                "ab_record_id": row.get("ab_record_id"),
                "pool_id": row.get("pool_id"),
                "base_mint": row.get("base_mint"),
                "probe_bucket": row.get("probe_bucket"),
                "execution_outcome": row.get("execution_outcome"),
                "err": row.get("err"),
                "instruction_index": instruction_index,
                "custom_code": custom_code,
                "program_id": program_id,
                "program_name": row.get("simulation_error_program_name"),
                "program_error_name": error_name,
                "program_error_family": row.get("simulation_error_program_error_family"),
                "category": category,
                "route_kind": row.get("route_kind"),
                "buy_variant": row.get("buy_variant"),
                "token_param_role": row.get("token_param_role"),
                "amount_lamports": row.get("amount_lamports"),
                "probe_amount_source": row.get("probe_amount_source"),
                "probe_slippage_bps": row.get("probe_slippage_bps"),
                "entry_token_amount_raw": row.get("entry_token_amount_raw"),
                "min_tokens_out": row.get("min_tokens_out"),
                "instruction_account_roles": row.get("simulation_error_instruction_account_roles") or [],
                "log_tail": log_tail,
                "diagnostic_limit": None
                if row.get("simulation_error_log_tail")
                else "transport row predates Q4 log/program/account-role propagation",
            }
        )

    return {
        "schema_version": 1,
        "transport_rows": len(rows),
        "simulation_error_rows": len(error_rows),
        "category_counts": dict(category_counts),
        "custom_code_counts": dict(code_counts),
        "program_counts": dict(program_counts),
        "probe_bucket_counts": dict(bucket_counts),
        "errors": analyzed,
    }


def write_markdown(summary: dict[str, Any], output_md: Path) -> None:
    lines = [
        "# RAPORT P3.7-J3Q4 Simulation Instruction Error Analysis",
        "",
        "## Verdict",
        "",
        "```text",
        "J3Q4 diagnostic propagation: IMPLEMENTED",
        "error classification: PASS when program/log/account-role fields are present",
        "rows predating Q4 fields: diagnostic-limited",
        "small bounded collection: HOLD",
        "Phase B / P2 / live / tuning: NO-GO",
        "```",
        "",
        "Rows without `simulation_error_program_id`, instruction account roles or log tail",
        "are parsed but treated as diagnostic-limited, not fully understood.",
        "",
        "## Summary",
        "",
        "```text",
        f"transport_rows = {summary['transport_rows']}",
        f"simulation_error_rows = {summary['simulation_error_rows']}",
        f"category_counts = {summary['category_counts']}",
        f"custom_code_counts = {summary['custom_code_counts']}",
        f"program_counts = {summary['program_counts']}",
        "```",
        "",
        "## Error Rows",
        "",
    ]
    if not summary["errors"]:
        lines.append("No simulation error rows were found.")
    for row in summary["errors"]:
        lines.extend(
            [
                f"### `{row.get('probe_id')}`",
                "",
                "```text",
                f"ab_record_id = {row.get('ab_record_id')}",
                f"pool_id = {row.get('pool_id')}",
                f"base_mint = {row.get('base_mint')}",
                f"probe_bucket = {row.get('probe_bucket')}",
                f"err = {row.get('err')}",
                f"instruction_index = {row.get('instruction_index')}",
                f"custom_code = {row.get('custom_code')}",
                f"program_id = {row.get('program_id')}",
                f"program_name = {row.get('program_name')}",
                f"program_error_name = {row.get('program_error_name')}",
                f"category = {row.get('category')}",
                f"route_kind = {row.get('route_kind')}",
                f"buy_variant = {row.get('buy_variant')}",
                f"token_param_role = {row.get('token_param_role')}",
                f"amount_lamports = {row.get('amount_lamports')}",
                f"probe_amount_source = {row.get('probe_amount_source')}",
                f"probe_slippage_bps = {row.get('probe_slippage_bps')}",
                f"entry_token_amount_raw = {row.get('entry_token_amount_raw')}",
                f"min_tokens_out = {row.get('min_tokens_out')}",
                f"diagnostic_limit = {row.get('diagnostic_limit')}",
                "```",
                "",
            ]
        )
        roles = row.get("instruction_account_roles") or []
        if roles:
            lines.extend(["Instruction account roles:", ""])
            lines.extend(f"- `{role}`" for role in roles)
            lines.append("")
        log_tail = row.get("log_tail") or []
        if log_tail:
            lines.extend(["Simulation log tail:", "", "```text"])
            lines.extend(log_tail)
            lines.extend(["```", ""])

    lines.extend(
        [
            "## Decision",
            "",
            "Rows without `simulation_error_program_id`, instruction account roles or log tail",
            "are treated as pre-Q4 diagnostic-limited rows. Future probe transport rows now",
            "carry the fields needed to classify whether the error is isolated, route-specific,",
            "amount/slippage-related, or an account-layout mismatch.",
            "",
        ]
    )
    output_md.write_text("\n".join(lines))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path)
    parser.add_argument("--transport-log", type=Path)
    parser.add_argument("--output-json", type=Path, required=True)
    parser.add_argument("--output-md", type=Path, required=True)
    args = parser.parse_args()

    transport_path = args.transport_log
    if transport_path is None:
        if args.config is None:
            raise SystemExit("one of --config or --transport-log is required")
        transport_path = _config_transport_path(args.config)
    if transport_path is None:
        raise SystemExit("transport log path could not be resolved")

    rows = _load_jsonl(transport_path)
    summary = analyze_rows(rows)
    summary["transport_log_path"] = str(transport_path)
    if args.config:
        summary["config_path"] = str(args.config)

    args.output_json.parent.mkdir(parents=True, exist_ok=True)
    args.output_json.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    args.output_md.parent.mkdir(parents=True, exist_ok=True)
    write_markdown(summary, args.output_md)


if __name__ == "__main__":
    main()
