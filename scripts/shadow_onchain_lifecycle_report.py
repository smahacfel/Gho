#!/usr/bin/env python3
from __future__ import annotations

import argparse
import bisect
import json
import math
import re
import shutil
import statistics as st
import subprocess
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable, Iterator

import v3_p37_shadow_lifecycle_labeler as lifecycle_labeler
from shadow_run_report import (
    BUY_LOG_NAME,
    DEFAULT_CONFIG,
    detect_latest_run_scope,
    derive_shadow_lifecycle_log_path,
    load_toml,
    preferred_gatekeeper_decision_plane,
    resolve_config_path,
    resolve_gatekeeper_log_path,
    resolve_runtime_path,
)

LAMPORTS_PER_SOL = 1_000_000_000
PUMP_TOKEN_DECIMAL_FACTOR = 1_000_000
PUMP_TOKEN_DECIMAL_FACTOR_F64 = float(PUMP_TOKEN_DECIMAL_FACTOR)
LEGACY_PRICE_SCALE = PUMP_TOKEN_DECIMAL_FACTOR_F64
PUMP_FUN_FEE_BPS = 100
PRICE_SCALE_CANDIDATES = (1.0, LEGACY_PRICE_SCALE)
DEFAULT_MAX_BUY_MATCH_DRIFT_MS = 60_000
TRUTH_DATASET_KIND = "shadow_burnin_lifecycle_onchain"
COLLECTION_PLANE_BY_ARTIFACT = {
    "shadow": "active_shadow",
    "probe": "counterfactual_shadow_probe",
}
JOIN_METADATA_FIELDS = (
    "ab_record_id",
    "source_ab_record_id",
    "probe_id",
    "dispatch_source",
    "v3_feature_snapshot_hash",
    "v3_policy_config_hash",
    "decision_plane",
    "rollout_namespace",
    "run_id",
    "session_id",
)
ISO_TS_RE = re.compile(
    r"^(?P<head>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?P<fraction>\.\d+)?(?P<tz>Z|[+-]\d{2}:\d{2})$"
)
DIAG_ACCOUNT_UPDATE_RELAY_RE = re.compile(
    r"^(?P<timestamp>\S+).*\bDIAG_ACCOUNT_UPDATE_RELAY\b "
    r"base_mint=(?P<base_mint>\S+) bonding_curve=(?P<bonding_curve>\S+) "
    r"slot=(?P<slot>\d+) sol_reserves=(?P<sol_reserves>\d+) "
    r"token_reserves=(?P<token_reserves>\d+) complete=(?P<complete>\d+) "
    r"curve_finality=(?P<curve_finality>\S+)"
)


@dataclass(slots=True)
class Inputs:
    config_path: Path
    artifact_plane: str
    gatekeeper_buys_log: Path
    shadow_transport_log: Path
    shadow_entry_log: Path
    shadow_lifecycle_log: Path
    events_dir: Path
    system_log_base: Path
    session_run_id: str | None
    session_start_ms: int | None
    session_end_ms: int | None
    output_path: Path
    max_truth_gap_ms: int | None


@dataclass(slots=True)
class ReportOutputs:
    raw_output: Path
    manifest_output: Path
    summary_output: Path
    skipped_rows_output: Path | None
    label_output: Path
    label_summary_output: Path
    label_summary_md_output: Path
    outcome_summary_output: Path | None


@dataclass(slots=True)
class ShadowTransportRecord:
    join_metadata: dict[str, str | None]
    candidate_id: str
    base_mint: str
    pool_id: str
    decision_ts_ms: int | None
    sim_started_ts_ms: int | None
    sim_finished_ts_ms: int | None
    amount_lamports: int | None
    error_class: str | None
    live_signature: str | None


@dataclass(slots=True)
class ShadowEntryRecord:
    join_metadata: dict[str, str | None]
    candidate_id: str
    pool_id: str
    mint_id: str
    entry_price: float | None
    slot: int | None
    timestamp_ms: int | None
    execution_outcome: str | None


@dataclass(slots=True)
class ExitFillRow:
    join_metadata: dict[str, str | None]
    ordinal: int
    candidate_id: str
    position_id: str | None
    pool_id: str
    mint_id: str
    timestamp_ms: int | None
    sample_timestamp_ms: int | None
    sample_slot: int | None
    sample_age_ms: int | None
    fraction_bps: int | None
    remaining_fraction_bps: int | None
    entry_price: float | None
    exit_price: float | None
    entry_value_sol: float | None
    exit_value_sol: float | None
    truth_status: str | None
    truth_source: str | None
    sample_price_state: str | None


@dataclass(slots=True)
class PositionClosedRow:
    join_metadata: dict[str, str | None]
    candidate_id: str
    position_id: str | None
    pool_id: str
    mint_id: str
    timestamp_ms: int | None
    sample_timestamp_ms: int | None
    sample_slot: int | None
    entry_price: float | None
    entry_value_sol: float | None
    exit_value_sol: float | None
    gross_pnl_sol: float | None
    net_pnl_sol: float | None
    estimated_costs_sol: float | None
    final_pnl: float | None
    final_pnl_pct: float | None
    duration_ms: int | None
    close_reason: str | None
    total_exits: int | None
    truth_status: str | None
    truth_source: str | None
    sample_price_state: str | None


@dataclass(slots=True)
class LifecycleBundle:
    candidate_id: str
    exit_fills: list[ExitFillRow] = field(default_factory=list)
    position_closed: PositionClosedRow | None = None


@dataclass(slots=True)
class ScopeStats:
    candidate_count: int
    first_candidate_ts_ms: int | None
    latest_candidate_ts_ms: int | None
    closed_positions: int
    resolved_close_truth: int
    failed_close_truth: int
    latest_close_ts_ms: int | None


@dataclass(slots=True)
class GatekeeperBuyRow:
    join_metadata: dict[str, str | None]
    pool_id: str
    base_mint: str
    first_seen_ts_ms: int | None
    observation_start_ts_ms: int | None
    observation_end_ts_ms: int | None
    curve_t0_event_ts_ms: int | None
    timestamp_ms: int | None
    shadow_execution_outcome: str | None
    decision_verdict_buy: bool
    verdict_type: str | None
    decision_reason: str | None


@dataclass(slots=True)
class DiagUpdate:
    timestamp_ms: int
    base_mint: str
    bonding_curve: str
    slot: int
    sol_reserves_lamports: int
    token_reserves_raw: int
    complete: int
    curve_finality: str

    def spot_price_sol(self) -> float | None:
        if self.sol_reserves_lamports <= 0 or self.token_reserves_raw <= 0:
            return None
        return lamports_to_sol(self.sol_reserves_lamports) / raw_to_display_tokens(
            self.token_reserves_raw
        )


@dataclass(slots=True)
class MatchedTruth:
    update: DiagUpdate
    delta_ms: int
    direction: str


@dataclass(slots=True)
class DiagTimeline:
    timestamps_ms: list[int]
    updates: list[DiagUpdate]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Correlate full-lifecycle shadow positions with historical DIAG_ACCOUNT_UPDATE_RELAY "
            "truth and write one JSONL row per analyzed position."
        )
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help=f"Launcher config used for the session (default: {DEFAULT_CONFIG})",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help=(
            "Output JSONL path. When omitted, the script writes next to shadow_entries.jsonl "
            "using the active session scope in the file name."
        ),
    )
    parser.add_argument(
        "--outcome-summary-output",
        type=Path,
        help=(
            "Optional compact JSON output path for raportneu-style outcome rows. "
            "The full JSONL output remains the source of truth."
        ),
    )
    parser.add_argument(
        "--manifest-output",
        type=Path,
        help="Optional provenance manifest JSON path. Defaults next to --output.",
    )
    parser.add_argument(
        "--summary-output",
        type=Path,
        help="Optional denominator summary JSON path. Defaults next to --output.",
    )
    parser.add_argument(
        "--emit-skipped-rows",
        nargs="?",
        const="",
        default=None,
        metavar="PATH",
        help=(
            "Optionally write skipped candidate/fill diagnostics as JSONL. "
            "When PATH is omitted, defaults next to --output."
        ),
    )
    parser.add_argument(
        "--label-output",
        type=Path,
        help="Optional lifecycle label JSONL path. Defaults next to --output.",
    )
    parser.add_argument(
        "--label-summary-output",
        type=Path,
        help="Optional lifecycle label summary JSON path. Defaults next to --output.",
    )
    parser.add_argument(
        "--label-summary-md-output",
        type=Path,
        help="Optional lifecycle label summary Markdown path. Defaults next to --output.",
    )
    parser.add_argument(
        "--session-start-ms",
        type=int,
        help=(
            "Optional lower bound for analyzed candidate_ids. "
            "When omitted, the script defaults to the latest event-bus run."
        ),
    )
    parser.add_argument(
        "--session-end-ms",
        type=int,
        help="Optional upper bound for analyzed candidate_ids.",
    )
    parser.add_argument(
        "--all-sessions",
        action="store_true",
        help="Analyze all available sessions instead of defaulting to the latest run.",
    )
    parser.add_argument(
        "--artifact-plane",
        choices=("shadow", "probe"),
        default="shadow",
        help=(
            "Artifact plane to analyze. Use probe for [p37_shadow_probe] "
            "transport/entry/lifecycle paths."
        ),
    )
    parser.add_argument(
        "--probe",
        action="store_true",
        help="Shortcut for --artifact-plane probe.",
    )
    parser.add_argument(
        "--max-truth-gap-ms",
        type=int,
        help=(
            "Optional hard filter for DIAG match distance. "
            "Rows with entry or exit truth farther away than this threshold are skipped."
        ),
    )
    return parser.parse_args()


def resolve_inputs(args: argparse.Namespace) -> Inputs:
    config_path = resolve_config_path(args.config)
    config = load_toml(config_path)
    artifact_plane = "probe" if args.probe else args.artifact_plane
    shadow_cfg = config.get("execution", {}).get("shadow", {})
    trigger_shadow_cfg = config.get("trigger", {}).get("shadow_run", {})
    probe_cfg = config.get("p37_shadow_probe", {})
    decision_dir = resolve_runtime_path(
        config_path,
        config.get("oracle", {}).get("decision_log_path", "logs/decisions"),
    )
    preferred_plane = preferred_gatekeeper_decision_plane("shadow")
    if artifact_plane == "probe":
        missing_probe_paths = [
            field
            for field in ("transport_log_path", "entry_log_path", "lifecycle_log_path")
            if not isinstance(probe_cfg.get(field), str) or not probe_cfg.get(field)
        ]
        if missing_probe_paths:
            raise SystemExit(
                "[p37_shadow_probe] is missing required report path(s): "
                + ", ".join(missing_probe_paths)
            )
        shadow_transport_log = resolve_runtime_path(config_path, probe_cfg["transport_log_path"])
        shadow_entry_log = resolve_runtime_path(config_path, probe_cfg["entry_log_path"])
        shadow_lifecycle_log = resolve_runtime_path(config_path, probe_cfg["lifecycle_log_path"])
    else:
        shadow_entry_log = resolve_runtime_path(
            config_path,
            shadow_cfg.get("entry_log_path", "logs/shadow_run/shadow_entries.jsonl"),
        )
        shadow_lifecycle_log = resolve_runtime_path(
            config_path,
            shadow_cfg.get("lifecycle_log_path") or derive_shadow_lifecycle_log_path(shadow_entry_log),
        )
        shadow_transport_log = resolve_runtime_path(
            config_path,
            trigger_shadow_cfg.get("output_path", "logs/shadow_run/buys.jsonl"),
        )
    events_dir = resolve_runtime_path(
        config_path,
        config.get("execution", {}).get("events", {}).get("output_dir", "datasets/events"),
    )
    system_log_base = resolve_runtime_path(
        config_path, config.get("logging", {}).get("file_path", "logs/system.log")
    )
    session_run_id = None
    session_start_ms = args.session_start_ms
    if session_start_ms is None and not args.all_sessions:
        session_run_id, session_start_ms = detect_latest_run_scope(events_dir)
    session_end_ms = args.session_end_ms
    if session_start_ms is not None and session_end_ms is not None and session_end_ms < session_start_ms:
        raise SystemExit("--session-end-ms must be >= --session-start-ms")
    if args.output is None:
        scope = str(session_start_ms) if session_start_ms is not None else "all"
        prefix = "probe_shadow_onchain_lifecycle_report" if artifact_plane == "probe" else "shadow_onchain_lifecycle_report"
        output_path = shadow_entry_log.parent / f"{prefix}_{scope}.jsonl"
    else:
        output_path = resolve_runtime_path(config_path, str(args.output))
    return Inputs(
        config_path=config_path,
        artifact_plane=artifact_plane,
        gatekeeper_buys_log=resolve_gatekeeper_log_path(
            decision_dir,
            BUY_LOG_NAME,
            preferred_plane=preferred_plane,
        ),
        shadow_transport_log=shadow_transport_log,
        shadow_entry_log=shadow_entry_log,
        shadow_lifecycle_log=shadow_lifecycle_log,
        events_dir=events_dir,
        system_log_base=system_log_base,
        session_run_id=session_run_id,
        session_start_ms=session_start_ms,
        session_end_ms=session_end_ms,
        output_path=output_path,
        max_truth_gap_ms=args.max_truth_gap_ms,
    )


def default_companion_path(output_path: Path, suffix: str) -> Path:
    return output_path.with_suffix(suffix)


def resolve_optional_output(
    config_path: Path,
    raw_path: Path | str | None,
    default_path: Path | None,
) -> Path | None:
    if raw_path is None:
        return default_path
    if raw_path == "":
        return default_path
    return resolve_runtime_path(config_path, str(raw_path))


def resolve_report_outputs(args: argparse.Namespace, inputs: Inputs) -> ReportOutputs:
    return ReportOutputs(
        raw_output=inputs.output_path,
        manifest_output=resolve_optional_output(
            inputs.config_path,
            args.manifest_output,
            default_companion_path(inputs.output_path, ".manifest.json"),
        )
        or default_companion_path(inputs.output_path, ".manifest.json"),
        summary_output=resolve_optional_output(
            inputs.config_path,
            args.summary_output,
            default_companion_path(inputs.output_path, ".summary.json"),
        )
        or default_companion_path(inputs.output_path, ".summary.json"),
        skipped_rows_output=resolve_optional_output(
            inputs.config_path,
            args.emit_skipped_rows,
            default_companion_path(inputs.output_path, ".skipped.jsonl"),
        )
        if args.emit_skipped_rows is not None
        else None,
        label_output=resolve_optional_output(
            inputs.config_path,
            args.label_output,
            default_companion_path(inputs.output_path, ".labels.jsonl"),
        )
        or default_companion_path(inputs.output_path, ".labels.jsonl"),
        label_summary_output=resolve_optional_output(
            inputs.config_path,
            args.label_summary_output,
            default_companion_path(inputs.output_path, ".labels.summary.json"),
        )
        or default_companion_path(inputs.output_path, ".labels.summary.json"),
        label_summary_md_output=resolve_optional_output(
            inputs.config_path,
            args.label_summary_md_output,
            default_companion_path(inputs.output_path, ".labels.summary.md"),
        )
        or default_companion_path(inputs.output_path, ".labels.summary.md"),
        outcome_summary_output=resolve_optional_output(
            inputs.config_path,
            args.outcome_summary_output,
            None,
        ),
    )


def parse_iso_to_ms(value: str | None) -> int | None:
    if not isinstance(value, str):
        return None
    match = ISO_TS_RE.match(value.strip())
    if not match:
        return None
    head = match.group("head")
    fraction = match.group("fraction") or ""
    if fraction:
        fraction = "." + fraction[1:7].ljust(6, "0")
    tz = match.group("tz")
    if tz == "Z":
        tz = "+00:00"
    try:
        return int(datetime.fromisoformat(f"{head}{fraction}{tz}").timestamp() * 1000)
    except ValueError:
        return None


def parse_candidate_id(candidate_id: str | None) -> tuple[str, str, int] | None:
    if not isinstance(candidate_id, str):
        return None
    parts = candidate_id.rsplit("_", 2)
    if len(parts) != 3 or not parts[2].isdigit():
        return None
    return parts[0], parts[1], int(parts[2])


def extract_candidate_ts_ms(candidate_id: str | None) -> int | None:
    parsed = parse_candidate_id(candidate_id)
    return parsed[2] if parsed else None


def in_window(
    candidate_id: str | None,
    row_ts_ms: int | None,
    session_start_ms: int | None,
    session_end_ms: int | None,
) -> bool:
    candidate_ts_ms = extract_candidate_ts_ms(candidate_id)
    effective_ts = next((ts for ts in (candidate_ts_ms, row_ts_ms) if ts is not None), None)
    if session_start_ms is not None and (effective_ts is None or effective_ts < session_start_ms):
        return False
    if session_end_ms is not None and (effective_ts is None or effective_ts > session_end_ms):
        return False
    return True


def iter_jsonl_rows(path: Path) -> Iterator[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def iter_json_concat_rows(path: Path) -> Iterator[dict[str, Any]]:
    if not path.exists():
        return
    data = path.read_text(encoding="utf-8", errors="ignore")
    decoder = json.JSONDecoder()
    index = 0
    length = len(data)
    while index < length:
        while index < length and data[index].isspace():
            index += 1
        if index >= length:
            break
        try:
            row, next_index = decoder.raw_decode(data, index)
        except json.JSONDecodeError:
            index += 1
            continue
        if isinstance(row, dict):
            yield row
        index = next_index


def float_or_none(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def int_or_none(value: Any) -> int | None:
    return int(value) if isinstance(value, (int, float)) else None


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def join_metadata_from_row(row: dict[str, Any]) -> dict[str, str | None]:
    return {field: str_or_none(row.get(field)) for field in JOIN_METADATA_FIELDS}


def coalesce_join_metadata(
    *metadata_sources: dict[str, str | None] | None,
) -> dict[str, str | None]:
    merged: dict[str, str | None] = {field: None for field in JOIN_METADATA_FIELDS}
    for metadata in metadata_sources:
        if not metadata:
            continue
        for field in JOIN_METADATA_FIELDS:
            if merged[field] is None:
                merged[field] = str_or_none(metadata.get(field))
    return merged


def lamports_to_sol(value: int | float) -> float:
    return float(value) / LAMPORTS_PER_SOL


def raw_to_display_tokens(value: int | float) -> float:
    return float(value) / PUMP_TOKEN_DECIMAL_FACTOR_F64


def display_to_raw_tokens(value: float) -> int:
    return max(1, int(round(value * PUMP_TOKEN_DECIMAL_FACTOR_F64)))


def pct_diff(actual: float | None, reference: float | None) -> float | None:
    if actual is None or reference is None or not math.isfinite(actual) or not math.isfinite(reference):
        return None
    if reference == 0.0:
        return None
    return ((actual / reference) - 1.0) * 100.0


def counter_dict(counter: Counter[str]) -> dict[str, int]:
    return {key: counter[key] for key in sorted(counter)}


def collection_plane_for_artifact(artifact_plane: str) -> str:
    return COLLECTION_PLANE_BY_ARTIFACT.get(artifact_plane, artifact_plane)


def execution_verification_class_hint_for_finality(finality: str | None) -> str:
    normalized = (finality or "").strip().lower()
    if normalized == "finalized":
        return "shadow_onchain_finalized_verified"
    if normalized == "confirmed":
        return "shadow_onchain_confirmed_verified"
    if normalized == "speculative":
        return "shadow_onchain_speculative_snapshot_verified"
    if normalized:
        return "shadow_onchain_snapshot_verified_non_final"
    return "shadow_onchain_degraded_unknown_finality"


def combined_execution_verification_class_hint(finalities: Iterable[str | None]) -> str:
    hints = [execution_verification_class_hint_for_finality(value) for value in finalities]
    if not hints:
        return "shadow_onchain_degraded_unknown_finality"
    if "shadow_onchain_speculative_snapshot_verified" in hints:
        return "shadow_onchain_speculative_snapshot_verified"
    if any(hint.startswith("shadow_onchain_degraded") for hint in hints):
        return "shadow_onchain_degraded_unknown_finality"
    if any(hint == "shadow_onchain_snapshot_verified_non_final" for hint in hints):
        return "shadow_onchain_snapshot_verified_non_final"
    if all(hint == "shadow_onchain_finalized_verified" for hint in hints):
        return "shadow_onchain_finalized_verified"
    if all(
        hint in {"shadow_onchain_confirmed_verified", "shadow_onchain_finalized_verified"}
        for hint in hints
    ):
        return "shadow_onchain_confirmed_verified"
    return "shadow_onchain_degraded_unknown_finality"


def score_shadow_price_multipliers(
    logged_price: float, reference_price: float | None
) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    for multiplier in PRICE_SCALE_CANDIDATES:
        normalized = logged_price * multiplier if math.isfinite(logged_price) else None
        if (
            normalized is None
            or normalized <= 0.0
            or not math.isfinite(normalized)
            or reference_price is None
            or reference_price <= 0.0
            or not math.isfinite(reference_price)
        ):
            ratio = None
            score = None
        else:
            ratio = normalized / reference_price
            score = abs(math.log10(ratio)) if ratio > 0.0 and math.isfinite(ratio) else None
        candidates.append(
            {
                "multiplier": multiplier,
                "normalized_price": normalized,
                "reference_price": reference_price,
                "ratio_to_reference": ratio,
                "score": score,
            }
        )
    return candidates


def choose_shadow_price_multiplier(logged_price: float, reference_price: float | None) -> float:
    candidates = score_shadow_price_multipliers(logged_price, reference_price)
    valid_candidates = [candidate for candidate in candidates if candidate["score"] is not None]
    if not valid_candidates:
        if reference_price is None or reference_price <= 0.0 or not math.isfinite(reference_price):
            return LEGACY_PRICE_SCALE if logged_price < 1e-10 else 1.0
        return min(PRICE_SCALE_CANDIDATES, key=lambda multiplier: abs(multiplier - 1.0))
    return min(valid_candidates, key=lambda candidate: candidate["score"])["multiplier"]


def shadow_price_with_multiplier(price: float | None, multiplier: float) -> float | None:
    if price is None or price <= 0.0 or not math.isfinite(price):
        return None
    return price * multiplier


def simulate_buy_tokens_raw(
    sol_in_lamports: int, update: DiagUpdate, fee_bps: int = PUMP_FUN_FEE_BPS
) -> int:
    if sol_in_lamports <= 0 or update.sol_reserves_lamports <= 0 or update.token_reserves_raw <= 0:
        return 0
    fee_bps = max(0, min(10_000, int(fee_bps)))
    fee = (sol_in_lamports * fee_bps) // 10_000
    effective_sol = sol_in_lamports - fee
    invariant = update.sol_reserves_lamports * update.token_reserves_raw
    new_sol_reserves = update.sol_reserves_lamports + effective_sol
    if new_sol_reserves <= 0:
        return 0
    new_token_reserves = invariant // new_sol_reserves
    tokens_out = update.token_reserves_raw - new_token_reserves
    return max(0, tokens_out)


def calculate_sell_sol_out_lamports(
    tokens_in_raw: int, update: DiagUpdate, fee_bps: int = PUMP_FUN_FEE_BPS
) -> int:
    if tokens_in_raw <= 0 or update.sol_reserves_lamports <= 0 or update.token_reserves_raw <= 0:
        return 0
    fee_bps = max(0, min(10_000, int(fee_bps)))
    invariant = update.sol_reserves_lamports * update.token_reserves_raw
    new_token_reserves = update.token_reserves_raw + tokens_in_raw
    if new_token_reserves <= 0:
        return 0
    new_sol_reserves = invariant // new_token_reserves
    sol_out = update.sol_reserves_lamports - new_sol_reserves
    fee = (sol_out * fee_bps) // 10_000
    return max(0, sol_out - fee)


def buy_executable_price_sol(
    sol_in_lamports: int, update: DiagUpdate, fee_bps: int = PUMP_FUN_FEE_BPS
) -> tuple[float | None, int]:
    tokens_out_raw = simulate_buy_tokens_raw(sol_in_lamports, update, fee_bps=fee_bps)
    if tokens_out_raw <= 0:
        return None, 0
    price_sol = lamports_to_sol(sol_in_lamports) / raw_to_display_tokens(tokens_out_raw)
    return price_sol, tokens_out_raw


def sell_executable_price_sol(
    tokens_in_raw: int, update: DiagUpdate, fee_bps: int = PUMP_FUN_FEE_BPS
) -> tuple[float | None, int]:
    sol_out_lamports = calculate_sell_sol_out_lamports(tokens_in_raw, update, fee_bps=fee_bps)
    if sol_out_lamports <= 0 or tokens_in_raw <= 0:
        return None, 0
    price_sol = lamports_to_sol(sol_out_lamports) / raw_to_display_tokens(tokens_in_raw)
    return price_sol, sol_out_lamports


def load_shadow_transport_records(inputs: Inputs) -> dict[str, ShadowTransportRecord]:
    records: dict[str, ShadowTransportRecord] = {}
    for row in iter_jsonl_rows(inputs.shadow_transport_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        decision_ts_ms = int_or_none(row.get("decision_ts_ms"))
        if not in_window(candidate_id, decision_ts_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        records[candidate_id] = ShadowTransportRecord(
            join_metadata=join_metadata_from_row(row),
            candidate_id=candidate_id,
            base_mint=str(row.get("base_mint", "")),
            pool_id=str(row.get("pool_amm_id") or row.get("pool_id") or ""),
            decision_ts_ms=decision_ts_ms,
            sim_started_ts_ms=int_or_none(row.get("sim_started_ts_ms")),
            sim_finished_ts_ms=int_or_none(row.get("sim_finished_ts_ms")),
            amount_lamports=int_or_none(row.get("amount_lamports")),
            error_class=row.get("error_class") if isinstance(row.get("error_class"), str) else None,
            live_signature=row.get("live_signature")
            if isinstance(row.get("live_signature"), str)
            else None,
        )
    return records


def load_shadow_entries(inputs: Inputs) -> dict[str, ShadowEntryRecord]:
    records: dict[str, ShadowEntryRecord] = {}
    for row in iter_json_concat_rows(inputs.shadow_entry_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        timestamp_ms = int_or_none(row.get("timestamp_ms"))
        if not in_window(candidate_id, timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        records[candidate_id] = ShadowEntryRecord(
            join_metadata=join_metadata_from_row(row),
            candidate_id=candidate_id,
            pool_id=str(row.get("pool_id", "")),
            mint_id=str(row.get("mint_id", "")),
            entry_price=float_or_none(row.get("entry_price")),
            slot=int_or_none(row.get("slot")),
            timestamp_ms=timestamp_ms,
            execution_outcome=row.get("execution_outcome")
            if isinstance(row.get("execution_outcome"), str)
            else None,
        )
    return records


def load_lifecycle(inputs: Inputs) -> dict[str, LifecycleBundle]:
    bundles: dict[str, LifecycleBundle] = {}
    exit_ordinal = 0
    for row in iter_json_concat_rows(inputs.shadow_lifecycle_log):
        candidate_id = row.get("candidate_id")
        if not isinstance(candidate_id, str):
            continue
        timestamp_ms = int_or_none(row.get("timestamp_ms"))
        if not in_window(candidate_id, timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        record_type = row.get("record_type")
        if not isinstance(record_type, str):
            continue
        bundle = bundles.setdefault(candidate_id, LifecycleBundle(candidate_id=candidate_id))
        if record_type == "exit_filled":
            bundle.exit_fills.append(
                ExitFillRow(
                    join_metadata=join_metadata_from_row(row),
                    ordinal=exit_ordinal,
                    candidate_id=candidate_id,
                    position_id=row.get("position_id")
                    if isinstance(row.get("position_id"), str)
                    else None,
                    pool_id=str(row.get("pool_id", "")),
                    mint_id=str(row.get("mint_id", "")),
                    timestamp_ms=timestamp_ms,
                    sample_timestamp_ms=int_or_none(row.get("sample_timestamp_ms")),
                    sample_slot=int_or_none(row.get("sample_slot")),
                    sample_age_ms=int_or_none(row.get("sample_age_ms")),
                    fraction_bps=int_or_none(row.get("fraction_bps")),
                    remaining_fraction_bps=int_or_none(row.get("remaining_fraction_bps")),
                    entry_price=float_or_none(row.get("entry_price")),
                    exit_price=float_or_none(row.get("exit_price")),
                    entry_value_sol=float_or_none(row.get("entry_value_sol")),
                    exit_value_sol=float_or_none(row.get("exit_value_sol")),
                    truth_status=row.get("truth_status")
                    if isinstance(row.get("truth_status"), str)
                    else None,
                    truth_source=row.get("truth_source")
                    if isinstance(row.get("truth_source"), str)
                    else None,
                    sample_price_state=row.get("sample_price_state")
                    if isinstance(row.get("sample_price_state"), str)
                    else None,
                )
            )
            exit_ordinal += 1
        elif record_type == "position_closed":
            bundle.position_closed = PositionClosedRow(
                join_metadata=join_metadata_from_row(row),
                candidate_id=candidate_id,
                position_id=row.get("position_id") if isinstance(row.get("position_id"), str) else None,
                pool_id=str(row.get("pool_id", "")),
                mint_id=str(row.get("mint_id", "")),
                timestamp_ms=timestamp_ms,
                sample_timestamp_ms=int_or_none(row.get("sample_timestamp_ms")),
                sample_slot=int_or_none(row.get("sample_slot")),
                entry_price=float_or_none(row.get("entry_price")),
                entry_value_sol=float_or_none(row.get("entry_value_sol")),
                exit_value_sol=float_or_none(row.get("exit_value_sol")),
                gross_pnl_sol=float_or_none(row.get("gross_pnl_sol")),
                net_pnl_sol=float_or_none(row.get("net_pnl_sol")),
                estimated_costs_sol=float_or_none(row.get("estimated_costs_sol")),
                final_pnl=float_or_none(row.get("final_pnl")),
                final_pnl_pct=float_or_none(row.get("final_pnl_pct")),
                duration_ms=int_or_none(row.get("duration_ms")),
                close_reason=row.get("close_reason")
                if isinstance(row.get("close_reason"), str)
                else None,
                total_exits=int_or_none(row.get("total_exits")),
                truth_status=row.get("truth_status")
                if isinstance(row.get("truth_status"), str)
                else None,
                truth_source=row.get("truth_source")
                if isinstance(row.get("truth_source"), str)
                else None,
                sample_price_state=row.get("sample_price_state")
                if isinstance(row.get("sample_price_state"), str)
                else None,
            )
    return bundles


def build_scope_stats(
    transport_by_candidate: dict[str, ShadowTransportRecord],
    entry_by_candidate: dict[str, ShadowEntryRecord],
    lifecycle_by_candidate: dict[str, LifecycleBundle],
) -> ScopeStats:
    candidate_ids = (
        set(transport_by_candidate.keys())
        | set(entry_by_candidate.keys())
        | set(lifecycle_by_candidate.keys())
    )
    candidate_timestamps = sorted(
        ts
        for ts in (
            extract_candidate_ts_ms(candidate_id) for candidate_id in candidate_ids
        )
        if ts is not None
    )

    closed_positions = 0
    resolved_close_truth = 0
    failed_close_truth = 0
    latest_close_ts_ms: int | None = None

    for bundle in lifecycle_by_candidate.values():
        closed = bundle.position_closed
        if closed is None:
            continue
        closed_positions += 1
        if closed.truth_status == "resolved":
            resolved_close_truth += 1
        elif closed.truth_status == "failure":
            failed_close_truth += 1
        close_ts_ms = next(
            (
                ts
                for ts in (
                    closed.timestamp_ms,
                    closed.sample_timestamp_ms,
                    extract_candidate_ts_ms(bundle.candidate_id),
                )
                if ts is not None
            ),
            None,
        )
        if close_ts_ms is not None and (
            latest_close_ts_ms is None or close_ts_ms > latest_close_ts_ms
        ):
            latest_close_ts_ms = close_ts_ms

    return ScopeStats(
        candidate_count=len(candidate_ids),
        first_candidate_ts_ms=candidate_timestamps[0] if candidate_timestamps else None,
        latest_candidate_ts_ms=candidate_timestamps[-1] if candidate_timestamps else None,
        closed_positions=closed_positions,
        resolved_close_truth=resolved_close_truth,
        failed_close_truth=failed_close_truth,
        latest_close_ts_ms=latest_close_ts_ms,
    )


def load_gatekeeper_buys(inputs: Inputs) -> dict[tuple[str, str], list[GatekeeperBuyRow]]:
    rows_by_key: dict[tuple[str, str], list[GatekeeperBuyRow]] = defaultdict(list)
    for row in iter_jsonl_rows(inputs.gatekeeper_buys_log):
        base_mint = row.get("base_mint")
        pool_id = row.get("pool_id")
        if not isinstance(base_mint, str) or not isinstance(pool_id, str):
            continue
        first_seen_ts_ms = int_or_none(row.get("first_seen_ts_ms"))
        timestamp_ms = parse_iso_to_ms(row.get("timestamp"))
        if not in_window(None, first_seen_ts_ms or timestamp_ms, inputs.session_start_ms, inputs.session_end_ms):
            continue
        rows_by_key[(base_mint, pool_id)].append(
            GatekeeperBuyRow(
                join_metadata=join_metadata_from_row(row),
                pool_id=pool_id,
                base_mint=base_mint,
                first_seen_ts_ms=first_seen_ts_ms,
                observation_start_ts_ms=int_or_none(row.get("observation_start_ts_ms")),
                observation_end_ts_ms=int_or_none(row.get("observation_end_ts_ms")),
                curve_t0_event_ts_ms=int_or_none(row.get("curve_t0_event_ts_ms")),
                timestamp_ms=timestamp_ms,
                shadow_execution_outcome=row.get("shadow_execution_outcome")
                if isinstance(row.get("shadow_execution_outcome"), str)
                else None,
                decision_verdict_buy=bool(row.get("decision_verdict_buy")),
                verdict_type=row.get("verdict_type")
                if isinstance(row.get("verdict_type"), str)
                else None,
                decision_reason=row.get("decision_reason")
                if isinstance(row.get("decision_reason"), str)
                else None,
            )
        )
    for rows in rows_by_key.values():
        rows.sort(
            key=lambda row: (
                row.timestamp_ms if row.timestamp_ms is not None else -1,
                row.first_seen_ts_ms if row.first_seen_ts_ms is not None else -1,
            )
        )
    return rows_by_key


def iter_system_log_paths(base_path: Path) -> list[Path]:
    candidates = [
        path
        for path in base_path.parent.glob(f"{base_path.name}*")
        if path.is_file() and path.name.startswith(base_path.name)
    ]
    candidates.sort(key=lambda path: path.name)
    return candidates


def load_diag_updates(
    system_log_paths: Iterable[Path], relevant_mints: set[str]
) -> dict[str, DiagTimeline]:
    updates_by_mint: dict[str, list[DiagUpdate]] = defaultdict(list)
    if relevant_mints:
        rg_updates = load_diag_updates_with_rg(system_log_paths, relevant_mints)
        if rg_updates is not None:
            return rg_updates
    for path in system_log_paths:
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
                if not match:
                    continue
                base_mint = match.group("base_mint")
                if relevant_mints and base_mint not in relevant_mints:
                    continue
                timestamp_ms = parse_iso_to_ms(match.group("timestamp"))
                if timestamp_ms is None:
                    continue
                updates_by_mint[base_mint].append(
                    DiagUpdate(
                        timestamp_ms=timestamp_ms,
                        base_mint=base_mint,
                        bonding_curve=match.group("bonding_curve"),
                        slot=int(match.group("slot")),
                        sol_reserves_lamports=int(match.group("sol_reserves")),
                        token_reserves_raw=int(match.group("token_reserves")),
                        complete=int(match.group("complete")),
                        curve_finality=match.group("curve_finality"),
                    )
                )
    for updates in updates_by_mint.values():
        updates.sort(key=lambda update: update.timestamp_ms)
    return {
        mint: DiagTimeline(
            timestamps_ms=[update.timestamp_ms for update in updates],
            updates=updates,
        )
        for mint, updates in updates_by_mint.items()
    }


def load_diag_updates_with_rg(
    system_log_paths: Iterable[Path], relevant_mints: set[str]
) -> dict[str, DiagTimeline] | None:
    rg_path = shutil.which("rg")
    paths = [path for path in system_log_paths if path.exists()]
    if rg_path is None or not paths or not relevant_mints:
        return None
    mint_pattern = "|".join(sorted(re.escape(mint) for mint in relevant_mints))
    pattern = rf"DIAG_ACCOUNT_UPDATE_RELAY base_mint=({mint_pattern}) "
    command = [rg_path, "-a", "--no-heading", "-H", "-e", pattern, *[str(path) for path in paths]]
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="ignore",
        )
    except OSError:
        return None
    if completed.returncode not in (0, 1):
        return None

    updates_by_mint: dict[str, list[DiagUpdate]] = defaultdict(list)
    for raw_line in completed.stdout.splitlines():
        _, _, line = raw_line.partition(":")
        match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
        if not match:
            continue
        timestamp_ms = parse_iso_to_ms(match.group("timestamp"))
        if timestamp_ms is None:
            continue
        base_mint = match.group("base_mint")
        updates_by_mint[base_mint].append(
            DiagUpdate(
                timestamp_ms=timestamp_ms,
                base_mint=base_mint,
                bonding_curve=match.group("bonding_curve"),
                slot=int(match.group("slot")),
                sol_reserves_lamports=int(match.group("sol_reserves")),
                token_reserves_raw=int(match.group("token_reserves")),
                complete=int(match.group("complete")),
                curve_finality=match.group("curve_finality"),
            )
        )
    for updates in updates_by_mint.values():
        updates.sort(key=lambda update: update.timestamp_ms)
    return {
        mint: DiagTimeline(
            timestamps_ms=[update.timestamp_ms for update in updates],
            updates=updates,
        )
        for mint, updates in updates_by_mint.items()
    }


def match_gatekeeper_buy(
    rows: list[GatekeeperBuyRow],
    decision_ts_ms: int | None,
    execution_ts_ms: int | None,
) -> GatekeeperBuyRow | None:
    if not rows:
        return None
    anchor_ts_ms = next((value for value in (execution_ts_ms, decision_ts_ms) if value is not None), None)
    if anchor_ts_ms is None:
        return None
    best: GatekeeperBuyRow | None = None
    best_score: tuple[int, int, int] | None = None
    for row in rows:
        if not row.decision_verdict_buy:
            continue
        row_anchor_ts = next(
            (
                value
                for value in (
                    row.timestamp_ms,
                    row.observation_end_ts_ms,
                    row.first_seen_ts_ms,
                )
                if value is not None
            ),
            None,
        )
        if row_anchor_ts is None:
            continue
        if abs(row_anchor_ts - anchor_ts_ms) > DEFAULT_MAX_BUY_MATCH_DRIFT_MS:
            continue
        future_penalty = 0
        if decision_ts_ms is not None and row.first_seen_ts_ms is not None and row.first_seen_ts_ms > decision_ts_ms:
            future_penalty = row.first_seen_ts_ms - decision_ts_ms
        outcome_penalty = 0 if row.shadow_execution_outcome == "shadow_simulated" else 1
        score = (future_penalty, outcome_penalty, abs(row_anchor_ts - anchor_ts_ms))
        if best_score is None or score < best_score:
            best = row
            best_score = score
    return best


def find_causal_truth(timeline: DiagTimeline | None, target_ts_ms: int | None) -> MatchedTruth | None:
    if timeline is None or not timeline.updates or target_ts_ms is None:
        return None
    index = bisect.bisect_right(timeline.timestamps_ms, target_ts_ms) - 1
    if index < 0:
        return None
    update = timeline.updates[index]
    delta_ms = target_ts_ms - update.timestamp_ms
    return MatchedTruth(update=update, delta_ms=delta_ms, direction="before")


def find_causal_truth_with_neighbors(
    timeline: DiagTimeline | None, target_ts_ms: int | None
) -> tuple[MatchedTruth | None, int | None, int | None]:
    if timeline is None or not timeline.updates or target_ts_ms is None:
        return None, None, None
    index = bisect.bisect_right(timeline.timestamps_ms, target_ts_ms) - 1
    prev_update = timeline.updates[index] if index >= 0 else None
    next_index = index + 1 if index >= 0 else 0
    next_update = timeline.updates[next_index] if next_index < len(timeline.updates) else None
    matched = find_causal_truth(timeline, target_ts_ms)
    prev_delta = target_ts_ms - prev_update.timestamp_ms if prev_update is not None else None
    next_delta = next_update.timestamp_ms - target_ts_ms if next_update is not None else None
    return matched, prev_delta, next_delta


def record_skip(
    skipped: Counter[str],
    skipped_rows: list[dict[str, Any]],
    reason: str,
    *,
    candidate_id: str | None,
    stage: str,
    context: dict[str, Any] | None = None,
) -> None:
    skipped[reason] += 1
    row = {
        "schema_version": 1,
        "truth_dataset_kind": TRUTH_DATASET_KIND,
        "candidate_id": candidate_id,
        "stage": stage,
        "reason": reason,
    }
    if context:
        row.update(context)
    skipped_rows.append(row)


def analyze_positions(
    inputs: Inputs,
    transport_by_candidate: dict[str, ShadowTransportRecord],
    entry_by_candidate: dict[str, ShadowEntryRecord],
    lifecycle_by_candidate: dict[str, LifecycleBundle],
    gatekeeper_buys_by_key: dict[tuple[str, str], list[GatekeeperBuyRow]],
    diag_updates_by_mint: dict[str, DiagTimeline],
) -> tuple[list[dict[str, Any]], Counter[str], list[dict[str, Any]]]:
    rows: list[dict[str, Any]] = []
    skipped = Counter()
    skipped_rows: list[dict[str, Any]] = []
    for candidate_id in sorted(lifecycle_by_candidate, key=lambda value: extract_candidate_ts_ms(value) or 0):
        bundle = lifecycle_by_candidate[candidate_id]
        closed = bundle.position_closed
        if closed is None:
            record_skip(skipped, skipped_rows, "missing_position_closed", candidate_id=candidate_id, stage="lifecycle")
            continue
        if closed.truth_status != "resolved":
            record_skip(
                skipped,
                skipped_rows,
                "close_truth_not_resolved",
                candidate_id=candidate_id,
                stage="lifecycle",
                context={"truth_status": closed.truth_status},
            )
            continue
        resolved_exit_fills = [fill for fill in bundle.exit_fills if fill.truth_status == "resolved"]
        if not resolved_exit_fills:
            record_skip(skipped, skipped_rows, "no_resolved_exit_fills", candidate_id=candidate_id, stage="lifecycle")
            continue

        transport = transport_by_candidate.get(candidate_id)
        if transport is None:
            record_skip(skipped, skipped_rows, "missing_transport_record", candidate_id=candidate_id, stage="transport")
            continue
        if transport.error_class:
            record_skip(
                skipped,
                skipped_rows,
                "transport_record_has_error",
                candidate_id=candidate_id,
                stage="transport",
                context={"error_class": transport.error_class},
            )
            continue

        parsed_candidate = parse_candidate_id(candidate_id)
        if parsed_candidate is None:
            record_skip(skipped, skipped_rows, "unparseable_candidate_id", candidate_id=candidate_id, stage="identity")
            continue
        mint_id, pool_id, _ = parsed_candidate
        if closed.mint_id and closed.mint_id != mint_id:
            mint_id = closed.mint_id
        if closed.pool_id and closed.pool_id != pool_id:
            pool_id = closed.pool_id

        entry = entry_by_candidate.get(candidate_id)
        raw_shadow_entry_price = next(
            (
                price
                for price in (
                    entry.entry_price if entry is not None else None,
                    closed.entry_price,
                    resolved_exit_fills[0].entry_price,
                )
                if price is not None and price > 0.0 and math.isfinite(price)
            ),
            None,
        )
        if raw_shadow_entry_price is None:
            record_skip(skipped, skipped_rows, "missing_entry_price", candidate_id=candidate_id, stage="entry")
            continue
        entry_value_sol = next(
            (
                value
                for value in (
                    closed.entry_value_sol,
                    lamports_to_sol(transport.amount_lamports) if transport.amount_lamports else None,
                )
                if value is not None and value > 0.0 and math.isfinite(value)
            ),
            None,
        )
        if entry_value_sol is None:
            record_skip(skipped, skipped_rows, "missing_entry_value", candidate_id=candidate_id, stage="entry")
            continue

        entry_execution_ts_ms = next(
            (
                value
                for value in (
                    transport.sim_finished_ts_ms,
                    closed.timestamp_ms - closed.duration_ms
                    if closed.timestamp_ms is not None and closed.duration_ms is not None
                    else None,
                    entry.timestamp_ms if entry is not None else None,
                    transport.decision_ts_ms,
                )
                if value is not None
            ),
            None,
        )
        if entry_execution_ts_ms is None:
            record_skip(skipped, skipped_rows, "missing_entry_execution_ts", candidate_id=candidate_id, stage="entry")
            continue

        gatekeeper_buy = match_gatekeeper_buy(
            gatekeeper_buys_by_key.get((mint_id, pool_id), []),
            transport.decision_ts_ms,
            entry_execution_ts_ms,
        )
        mint_timeline = diag_updates_by_mint.get(mint_id)
        if mint_timeline is None:
            record_skip(
                skipped,
                skipped_rows,
                "missing_diag_updates",
                candidate_id=candidate_id,
                stage="truth",
                context={"mint_id": mint_id},
            )
            continue

        entry_truth, entry_prev_delta_ms, entry_next_delta_ms = find_causal_truth_with_neighbors(
            mint_timeline, entry_execution_ts_ms
        )
        if entry_truth is None:
            reason = "entry_truth_future_only" if entry_next_delta_ms is not None else "missing_entry_truth"
            record_skip(
                skipped,
                skipped_rows,
                reason,
                candidate_id=candidate_id,
                stage="entry_truth",
                context={
                    "target_ts_ms": entry_execution_ts_ms,
                    "match_prev_delta_ms": entry_prev_delta_ms,
                    "match_next_delta_ms": entry_next_delta_ms,
                },
            )
            continue
        if inputs.max_truth_gap_ms is not None and abs(entry_truth.delta_ms) > inputs.max_truth_gap_ms:
            record_skip(
                skipped,
                skipped_rows,
                "entry_truth_too_far",
                candidate_id=candidate_id,
                stage="entry_truth",
                context={
                    "target_ts_ms": entry_execution_ts_ms,
                    "match_delta_ms": entry_truth.delta_ms,
                    "max_truth_gap_ms": inputs.max_truth_gap_ms,
                },
            )
            continue

        trade_amount_lamports = next(
            (
                value
                for value in (
                    transport.amount_lamports,
                    int(round(entry_value_sol * LAMPORTS_PER_SOL)),
                )
                if value is not None and value > 0
            ),
            None,
        )
        if trade_amount_lamports is None:
            record_skip(skipped, skipped_rows, "missing_trade_amount_lamports", candidate_id=candidate_id, stage="entry")
            continue

        onchain_entry_exec_price_sol, onchain_entry_tokens_raw = buy_executable_price_sol(
            trade_amount_lamports, entry_truth.update
        )
        if onchain_entry_exec_price_sol is None or onchain_entry_tokens_raw <= 0:
            record_skip(skipped, skipped_rows, "entry_executable_quote_failed", candidate_id=candidate_id, stage="entry_quote")
            continue

        entry_price_scale_candidates = score_shadow_price_multipliers(
            raw_shadow_entry_price, onchain_entry_exec_price_sol
        )
        entry_multiplier = choose_shadow_price_multiplier(
            raw_shadow_entry_price, onchain_entry_exec_price_sol
        )
        shadow_entry_price_sol = shadow_price_with_multiplier(raw_shadow_entry_price, entry_multiplier)
        if shadow_entry_price_sol is None or shadow_entry_price_sol <= 0.0:
            record_skip(skipped, skipped_rows, "invalid_shadow_entry_price", candidate_id=candidate_id, stage="entry")
            continue

        shadow_entry_tokens_display = entry_value_sol / shadow_entry_price_sol
        if not math.isfinite(shadow_entry_tokens_display) or shadow_entry_tokens_display <= 0.0:
            record_skip(skipped, skipped_rows, "invalid_shadow_entry_token_qty", candidate_id=candidate_id, stage="entry")
            continue
        shadow_entry_tokens_raw_estimated = display_to_raw_tokens(shadow_entry_tokens_display)

        exit_fill_rows: list[dict[str, Any]] = []
        shadow_exit_value_sum = 0.0
        onchain_exit_value_sum = 0.0
        total_exit_tokens_display = 0.0
        weighted_onchain_exit_spot_sum = 0.0
        weighted_shadow_exit_spot_invariant_ok = True
        exit_truth_gap_values: list[int] = []

        for fill_index, fill in enumerate(sorted(resolved_exit_fills, key=lambda row: row.ordinal), start=1):
            fill_entry_value_sol = next(
                (
                    value
                    for value in (
                        fill.entry_value_sol,
                        entry_value_sol * ((fill.fraction_bps or 0) / 10_000.0)
                        if fill.fraction_bps is not None
                        else None,
                    )
                    if value is not None and value > 0.0 and math.isfinite(value)
                ),
                None,
            )
            if fill_entry_value_sol is None:
                record_skip(
                    skipped,
                    skipped_rows,
                    "missing_exit_fill_entry_value",
                    candidate_id=candidate_id,
                    stage="exit_fill",
                    context={"fill_index": fill_index},
                )
                exit_fill_rows = []
                break

            fill_tokens_display = fill_entry_value_sol / shadow_entry_price_sol
            if not math.isfinite(fill_tokens_display) or fill_tokens_display <= 0.0:
                record_skip(
                    skipped,
                    skipped_rows,
                    "invalid_exit_fill_token_qty",
                    candidate_id=candidate_id,
                    stage="exit_fill",
                    context={"fill_index": fill_index},
                )
                exit_fill_rows = []
                break
            fill_tokens_raw = display_to_raw_tokens(fill_tokens_display)
            fill_target_ts_ms = next(
                (value for value in (fill.sample_timestamp_ms, fill.timestamp_ms) if value is not None),
                None,
            )
            exit_truth, exit_prev_delta_ms, exit_next_delta_ms = find_causal_truth_with_neighbors(
                mint_timeline, fill_target_ts_ms
            )
            if exit_truth is None:
                reason = "exit_truth_future_only" if exit_next_delta_ms is not None else "missing_exit_truth"
                record_skip(
                    skipped,
                    skipped_rows,
                    reason,
                    candidate_id=candidate_id,
                    stage="exit_truth",
                    context={
                        "fill_index": fill_index,
                        "target_ts_ms": fill_target_ts_ms,
                        "match_prev_delta_ms": exit_prev_delta_ms,
                        "match_next_delta_ms": exit_next_delta_ms,
                    },
                )
                exit_fill_rows = []
                break
            if inputs.max_truth_gap_ms is not None and abs(exit_truth.delta_ms) > inputs.max_truth_gap_ms:
                record_skip(
                    skipped,
                    skipped_rows,
                    "exit_truth_too_far",
                    candidate_id=candidate_id,
                    stage="exit_truth",
                    context={
                        "fill_index": fill_index,
                        "target_ts_ms": fill_target_ts_ms,
                        "match_delta_ms": exit_truth.delta_ms,
                        "max_truth_gap_ms": inputs.max_truth_gap_ms,
                    },
                )
                exit_fill_rows = []
                break

            onchain_exit_exec_price_sol, onchain_exit_sol_out_lamports = sell_executable_price_sol(
                fill_tokens_raw, exit_truth.update
            )
            if onchain_exit_exec_price_sol is None or onchain_exit_sol_out_lamports <= 0:
                record_skip(
                    skipped,
                    skipped_rows,
                    "exit_executable_quote_failed",
                    candidate_id=candidate_id,
                    stage="exit_quote",
                    context={"fill_index": fill_index},
                )
                exit_fill_rows = []
                break

            shadow_fill_exit_price_sol = shadow_price_with_multiplier(fill.exit_price, entry_multiplier)
            if shadow_fill_exit_price_sol is None:
                shadow_fill_exit_price_sol = (
                    fill.exit_value_sol / fill_tokens_display if fill.exit_value_sol is not None else None
                )
            shadow_fill_exit_value_sol = next(
                (
                    value
                    for value in (
                        fill.exit_value_sol,
                        shadow_fill_exit_price_sol * fill_tokens_display
                        if shadow_fill_exit_price_sol is not None
                        else None,
                    )
                    if value is not None and value >= 0.0 and math.isfinite(value)
                ),
                None,
            )
            if shadow_fill_exit_value_sol is None:
                record_skip(
                    skipped,
                    skipped_rows,
                    "missing_shadow_exit_value",
                    candidate_id=candidate_id,
                    stage="exit_fill",
                    context={"fill_index": fill_index},
                )
                exit_fill_rows = []
                break

            onchain_spot_price_sol = exit_truth.update.spot_price_sol()
            if (
                shadow_fill_exit_price_sol is not None
                and onchain_spot_price_sol is not None
                and shadow_fill_exit_price_sol > onchain_spot_price_sol
            ):
                weighted_shadow_exit_spot_invariant_ok = False

            shadow_exit_value_sum += shadow_fill_exit_value_sol
            onchain_exit_value_sum += lamports_to_sol(onchain_exit_sol_out_lamports)
            total_exit_tokens_display += fill_tokens_display
            if onchain_spot_price_sol is not None:
                weighted_onchain_exit_spot_sum += onchain_spot_price_sol * fill_tokens_display
            exit_truth_gap_values.append(abs(exit_truth.delta_ms))

            exit_fill_rows.append(
                {
                    "fill_index": fill_index,
                    "fraction_bps": fill.fraction_bps,
                    "remaining_fraction_bps": fill.remaining_fraction_bps,
                    "shadow_entry_value_sol": fill_entry_value_sol,
                    "shadow_exit_value_sol": shadow_fill_exit_value_sol,
                    "shadow_exit_price_logged": fill.exit_price,
                    "shadow_exit_price_sol": shadow_fill_exit_price_sol,
                    "shadow_exit_vs_onchain_executable_pct": pct_diff(
                        shadow_fill_exit_price_sol, onchain_exit_exec_price_sol
                    ),
                    "shadow_exit_vs_onchain_spot_pct": pct_diff(
                        shadow_fill_exit_price_sol, onchain_spot_price_sol
                    ),
                    "tokens_sold_display": fill_tokens_display,
                    "tokens_sold_raw_estimated": fill_tokens_raw,
                    "target_ts_ms": fill_target_ts_ms,
                    "target_sample_ts_ms": fill.sample_timestamp_ms,
                    "target_sample_slot": fill.sample_slot,
                    "target_sample_age_ms": fill.sample_age_ms,
                    "truth_status": fill.truth_status,
                    "truth_source": fill.truth_source,
                    "sample_price_state": fill.sample_price_state,
                    "onchain_match_ts_ms": exit_truth.update.timestamp_ms,
                    "onchain_match_delta_ms": exit_truth.delta_ms,
                    "onchain_match_prev_delta_ms": exit_prev_delta_ms,
                    "onchain_match_next_delta_ms": exit_next_delta_ms,
                    "onchain_match_direction": exit_truth.direction,
                    "onchain_match_slot": exit_truth.update.slot,
                    "onchain_curve_finality": exit_truth.update.curve_finality,
                    "execution_verification_class_hint": execution_verification_class_hint_for_finality(
                        exit_truth.update.curve_finality
                    ),
                    "onchain_spot_price_sol": onchain_spot_price_sol,
                    "onchain_executable_price_sol": onchain_exit_exec_price_sol,
                    "onchain_executable_value_sol": lamports_to_sol(onchain_exit_sol_out_lamports),
                }
            )

        if not exit_fill_rows:
            continue
        if total_exit_tokens_display <= 0.0 or not math.isfinite(total_exit_tokens_display):
            record_skip(skipped, skipped_rows, "invalid_total_exit_qty", candidate_id=candidate_id, stage="exit")
            continue

        shadow_effective_exit_price_sol = shadow_exit_value_sum / total_exit_tokens_display
        onchain_effective_exit_price_sol = onchain_exit_value_sum / total_exit_tokens_display
        weighted_onchain_exit_spot_price_sol = (
            weighted_onchain_exit_spot_sum / total_exit_tokens_display
            if weighted_onchain_exit_spot_sum > 0.0
            else None
        )

        reported_exit_value_sol = closed.exit_value_sol
        reported_entry_value_sol = closed.entry_value_sol
        close_ts_ms = closed.timestamp_ms
        detection_to_execution_ms = (
            entry_execution_ts_ms - gatekeeper_buy.first_seen_ts_ms
            if gatekeeper_buy is not None and gatekeeper_buy.first_seen_ts_ms is not None
            else None
        )
        decision_to_execution_ms = (
            entry_execution_ts_ms - transport.decision_ts_ms
            if transport.decision_ts_ms is not None
            else None
        )
        execution_hint = combined_execution_verification_class_hint(
            [entry_truth.update.curve_finality]
            + [
                fill.get("onchain_curve_finality")
                for fill in exit_fill_rows
                if isinstance(fill.get("onchain_curve_finality"), str)
            ]
        )
        row = {
            "schema_version": 1,
            "truth_dataset_kind": TRUTH_DATASET_KIND,
            "collection_plane": collection_plane_for_artifact(inputs.artifact_plane),
            "execution_verification_class_hint": execution_hint,
            "analysis_status": "ok",
            "candidate_id": candidate_id,
            "position_id": closed.position_id,
            "mint_id": mint_id,
            "pool_id": pool_id,
            "close_reason": closed.close_reason,
            "truth_status": closed.truth_status,
            "truth_source": closed.truth_source,
            "sample_price_state": closed.sample_price_state,
            "timing": {
                "first_seen_ts_ms": gatekeeper_buy.first_seen_ts_ms if gatekeeper_buy is not None else None,
                "curve_t0_event_ts_ms": gatekeeper_buy.curve_t0_event_ts_ms
                if gatekeeper_buy is not None
                else None,
                "observation_start_ts_ms": gatekeeper_buy.observation_start_ts_ms
                if gatekeeper_buy is not None
                else None,
                "observation_end_ts_ms": gatekeeper_buy.observation_end_ts_ms
                if gatekeeper_buy is not None
                else None,
                "decision_ts_ms": transport.decision_ts_ms,
                "sim_started_ts_ms": transport.sim_started_ts_ms,
                "sim_finished_ts_ms": transport.sim_finished_ts_ms,
                "entry_execution_ts_ms": entry_execution_ts_ms,
                "close_ts_ms": close_ts_ms,
                "position_duration_ms": closed.duration_ms,
                "detection_to_execution_ms": detection_to_execution_ms,
                "decision_to_execution_ms": decision_to_execution_ms,
                "gatekeeper_buy_context_found": gatekeeper_buy is not None,
            },
            "shadow": {
                "execution_outcome": entry.execution_outcome if entry is not None else None,
                "entry_price_logged": raw_shadow_entry_price,
                "entry_price_multiplier": entry_multiplier,
                "entry_price_scale_candidates": entry_price_scale_candidates,
                "entry_truth_prev_delta_ms": entry_prev_delta_ms,
                "entry_truth_next_delta_ms": entry_next_delta_ms,
                "entry_price_sol": shadow_entry_price_sol,
                "entry_value_sol": entry_value_sol,
                "reported_entry_value_sol": reported_entry_value_sol,
                "entry_token_amount_display_estimated": shadow_entry_tokens_display,
                "entry_token_amount_raw_estimated": shadow_entry_tokens_raw_estimated,
                "effective_exit_price_sol": shadow_effective_exit_price_sol,
                "exit_value_sol_from_fills": shadow_exit_value_sum,
                "reported_exit_value_sol": reported_exit_value_sol,
                "gross_pnl_sol": closed.gross_pnl_sol,
                "net_pnl_sol": closed.net_pnl_sol,
                "estimated_costs_sol": closed.estimated_costs_sol,
                "final_pnl_sol": closed.final_pnl,
                "final_pnl_pct": closed.final_pnl_pct,
                "total_exits": closed.total_exits,
            },
            "onchain": {
                "source": "diag_account_update_relay",
                "entry": {
                    "match_ts_ms": entry_truth.update.timestamp_ms,
                    "match_delta_ms": entry_truth.delta_ms,
                    "match_prev_delta_ms": entry_prev_delta_ms,
                    "match_next_delta_ms": entry_next_delta_ms,
                    "match_direction": entry_truth.direction,
                    "match_slot": entry_truth.update.slot,
                    "curve_finality": entry_truth.update.curve_finality,
                    "execution_verification_class_hint": execution_verification_class_hint_for_finality(
                        entry_truth.update.curve_finality
                    ),
                    "spot_price_sol": entry_truth.update.spot_price_sol(),
                    "executable_price_sol": onchain_entry_exec_price_sol,
                    "token_amount_display": raw_to_display_tokens(onchain_entry_tokens_raw),
                    "token_amount_raw": onchain_entry_tokens_raw,
                },
                "exit": {
                    "effective_executable_price_sol": onchain_effective_exit_price_sol,
                    "effective_spot_price_sol": weighted_onchain_exit_spot_price_sol,
                    "total_executable_value_sol": onchain_exit_value_sum,
                    "fill_count": len(exit_fill_rows),
                    "max_abs_truth_gap_ms": max(exit_truth_gap_values) if exit_truth_gap_values else None,
                    "execution_verification_class_hint": combined_execution_verification_class_hint(
                        [
                            fill.get("onchain_curve_finality")
                            for fill in exit_fill_rows
                            if isinstance(fill.get("onchain_curve_finality"), str)
                        ]
                    ),
                },
            },
            "drift_pct": {
                "entry_vs_onchain_executable": pct_diff(
                    shadow_entry_price_sol, onchain_entry_exec_price_sol
                ),
                "entry_vs_onchain_spot": pct_diff(
                    shadow_entry_price_sol, entry_truth.update.spot_price_sol()
                ),
                "entry_token_amount_vs_onchain_executable": pct_diff(
                    shadow_entry_tokens_display, raw_to_display_tokens(onchain_entry_tokens_raw)
                ),
                "exit_vs_onchain_executable": pct_diff(
                    shadow_effective_exit_price_sol, onchain_effective_exit_price_sol
                ),
                "exit_vs_onchain_spot": pct_diff(
                    shadow_effective_exit_price_sol, weighted_onchain_exit_spot_price_sol
                ),
            },
            "consistency": {
                "shadow_exit_value_matches_closed_sol": (
                    shadow_exit_value_sum - reported_exit_value_sol
                    if reported_exit_value_sol is not None
                    else None
                ),
                "all_shadow_exit_prices_leq_onchain_spot": weighted_shadow_exit_spot_invariant_ok,
                "fee_bps": PUMP_FUN_FEE_BPS,
            },
            "exit_fills": exit_fill_rows,
        }
        row.update(
            coalesce_join_metadata(
                transport.join_metadata,
                entry.join_metadata if entry is not None else None,
                closed.join_metadata,
                gatekeeper_buy.join_metadata if gatekeeper_buy is not None else None,
                *(fill.join_metadata for fill in bundle.exit_fills),
            )
        )
        rows.append(row)
    return rows, skipped, skipped_rows


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=False))
            fh.write("\n")


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=False) + "\n",
        encoding="utf-8",
    )


def project_outcome_summary_row(row: dict[str, Any]) -> dict[str, Any]:
    timing = row.get("timing") if isinstance(row.get("timing"), dict) else {}
    shadow = row.get("shadow") if isinstance(row.get("shadow"), dict) else {}
    onchain = row.get("onchain") if isinstance(row.get("onchain"), dict) else {}
    onchain_entry = (
        onchain.get("entry") if isinstance(onchain.get("entry"), dict) else {}
    )
    exit_fills = row.get("exit_fills") if isinstance(row.get("exit_fills"), list) else []

    fills = []
    for fill in exit_fills:
        if not isinstance(fill, dict):
            continue
        fills.append(
            {
                "fill_index": fill.get("fill_index"),
                "target_sample_slot": fill.get("target_sample_slot"),
                "shadow_exit_vs_onchain_executable_pct": fill.get(
                    "shadow_exit_vs_onchain_executable_pct"
                ),
                "shadow_exit_vs_onchain_spot_pct": fill.get(
                    "shadow_exit_vs_onchain_spot_pct"
                ),
            }
        )

    return {
        "candidate_id": row.get("candidate_id"),
        "close_reason": row.get("close_reason"),
        "curve_t0_event_ts_ms": timing.get("curve_t0_event_ts_ms"),
        "entry_execution_ts_ms": timing.get("entry_execution_ts_ms"),
        "close_ts_ms": timing.get("close_ts_ms"),
        "position_duration_ms": timing.get("position_duration_ms"),
        "entry_price_logged": shadow.get("entry_price_logged"),
        "effective_exit_price_sol": shadow.get("effective_exit_price_sol"),
        "final_pnl_pct": shadow.get("final_pnl_pct"),
        "match_slot": onchain_entry.get("match_slot"),
        "fills": fills,
    }


def project_outcome_summary_rows(rows: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    return [project_outcome_summary_row(row) for row in rows]


def build_denominator_summary(
    rows: list[dict[str, Any]],
    skipped: Counter[str],
    inputs: Inputs,
    transport_by_candidate: dict[str, ShadowTransportRecord],
    entry_by_candidate: dict[str, ShadowEntryRecord],
    lifecycle_by_candidate: dict[str, LifecycleBundle],
    scope_stats: ScopeStats,
    outputs: ReportOutputs,
) -> dict[str, Any]:
    candidate_ids = (
        set(transport_by_candidate.keys())
        | set(entry_by_candidate.keys())
        | set(lifecycle_by_candidate.keys())
    )
    closed_candidate_ids = {
        candidate_id
        for candidate_id, bundle in lifecycle_by_candidate.items()
        if bundle.position_closed is not None
    }
    transport_errors = Counter(
        record.error_class
        for record in transport_by_candidate.values()
        if isinstance(record.error_class, str) and record.error_class
    )
    entry_outcomes = Counter(
        str(record.execution_outcome or "unknown") for record in entry_by_candidate.values()
    )
    resolved_lifecycle_candidates = sum(
        1
        for bundle in lifecycle_by_candidate.values()
        if bundle.position_closed is not None and bundle.position_closed.truth_status == "resolved"
    )
    no_position_closed = len(candidate_ids - closed_candidate_ids)
    missing_diag = sum(
        skipped.get(reason, 0)
        for reason in ("missing_diag_updates", "missing_entry_truth", "missing_exit_truth")
    )
    return {
        "schema_version": 1,
        "truth_dataset_kind": TRUTH_DATASET_KIND,
        "collection_plane": collection_plane_for_artifact(inputs.artifact_plane),
        "artifact_plane": inputs.artifact_plane,
        "output": str(outputs.raw_output),
        "scope_start_ms": inputs.session_start_ms,
        "scope_end_ms": inputs.session_end_ms,
        "session_run_id": inputs.session_run_id,
        "scope_candidates": scope_stats.candidate_count,
        "transport_candidates": len(transport_by_candidate),
        "transport_simulated": sum(1 for record in transport_by_candidate.values() if not record.error_class),
        "transport_errors_by_class": counter_dict(transport_errors),
        "entry_rows": len(entry_by_candidate),
        "entry_execution_outcome_counts": counter_dict(entry_outcomes),
        "lifecycle_candidates": len(lifecycle_by_candidate),
        "position_closed": scope_stats.closed_positions,
        "close_truth_resolved": scope_stats.resolved_close_truth,
        "close_truth_failed": scope_stats.failed_close_truth,
        "resolved_lifecycle_candidates": resolved_lifecycle_candidates,
        "rows_written": len(rows),
        "skipped_by_reason": counter_dict(skipped),
        "denominator_breakdown": {
            "simulated": sum(1 for record in transport_by_candidate.values() if not record.error_class),
            "closed": scope_stats.closed_positions,
            "resolved": resolved_lifecycle_candidates,
            "simulation_error": sum(transport_errors.values()),
            "no_position_closed": no_position_closed,
            "missing_diag": missing_diag,
            "missing_diag_updates": skipped.get("missing_diag_updates", 0),
            "entry_truth_future_only": skipped.get("entry_truth_future_only", 0),
            "exit_truth_future_only": skipped.get("exit_truth_future_only", 0),
            "entry_truth_too_far": skipped.get("entry_truth_too_far", 0),
            "exit_truth_too_far": skipped.get("exit_truth_too_far", 0),
        },
    }


def git_output(args: list[str]) -> str | None:
    repo_root = Path(__file__).resolve().parents[1]
    try:
        completed = subprocess.run(
            ["git", *args],
            cwd=repo_root,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        return None
    if completed.returncode != 0:
        return None
    output = completed.stdout.strip()
    return output or None


def file_provenance(path: Path) -> dict[str, Any]:
    exists = path.exists()
    payload: dict[str, Any] = {
        "path": str(path),
        "exists": exists,
        "size_bytes": None,
        "mtime_ms": None,
    }
    if exists:
        stat = path.stat()
        payload["size_bytes"] = stat.st_size
        payload["mtime_ms"] = int(stat.st_mtime * 1000)
    return payload


def build_manifest(
    *,
    inputs: Inputs,
    outputs: ReportOutputs,
    system_log_paths: list[Path],
    rows: list[dict[str, Any]],
    skipped: Counter[str],
    scope_stats: ScopeStats,
    label_summary: dict[str, Any],
) -> dict[str, Any]:
    script_path = Path(__file__).resolve()
    script_git_sha = git_output(["log", "-1", "--format=%H", "--", str(script_path)])
    if script_git_sha is None:
        script_git_sha = git_output(["rev-parse", "HEAD"])
    script_status = git_output(["status", "--short", "--", str(script_path)]) or ""
    input_paths = {
        "config": file_provenance(inputs.config_path),
        "gatekeeper_buys_log": file_provenance(inputs.gatekeeper_buys_log),
        "shadow_transport_log": file_provenance(inputs.shadow_transport_log),
        "shadow_entry_log": file_provenance(inputs.shadow_entry_log),
        "shadow_lifecycle_log": file_provenance(inputs.shadow_lifecycle_log),
        "events_dir": file_provenance(inputs.events_dir),
        "system_log_base": file_provenance(inputs.system_log_base),
        "system_log_paths": [file_provenance(path) for path in system_log_paths],
    }
    return {
        "schema_version": 1,
        "truth_dataset_kind": TRUTH_DATASET_KIND,
        "collection_plane": collection_plane_for_artifact(inputs.artifact_plane),
        "artifact_plane": inputs.artifact_plane,
        "script_path": str(script_path),
        "script_git_sha": script_git_sha,
        "script_git_dirty": bool(script_status),
        "script_git_status": script_status,
        "config_path": str(inputs.config_path),
        "input_paths": input_paths,
        "gatekeeper_buys_log": str(inputs.gatekeeper_buys_log),
        "system_log_paths": [str(path) for path in system_log_paths],
        "scope_start_ms": inputs.session_start_ms,
        "scope_end_ms": inputs.session_end_ms,
        "session_run_id": inputs.session_run_id,
        "scope_candidates": scope_stats.candidate_count,
        "rows_written": len(rows),
        "skipped_by_reason": counter_dict(skipped),
        "outputs": {
            "raw_jsonl": str(outputs.raw_output),
            "manifest_json": str(outputs.manifest_output),
            "summary_json": str(outputs.summary_output),
            "skipped_rows_jsonl": str(outputs.skipped_rows_output) if outputs.skipped_rows_output else None,
            "label_jsonl": str(outputs.label_output),
            "label_summary_json": str(outputs.label_summary_output),
            "label_summary_md": str(outputs.label_summary_md_output),
            "outcome_summary_json": str(outputs.outcome_summary_output)
            if outputs.outcome_summary_output is not None
            else None,
        },
        "label_generation_status": {
            "status": "ok",
            "rows_total": label_summary.get("rows_total"),
            "phase_f_label_status": label_summary.get("phase_f_label_status"),
        },
    }


def default_labeler_args() -> argparse.Namespace:
    return argparse.Namespace(
        entry_truth_gap_clean_ms=1500,
        entry_truth_gap_degraded_acceptable_ms=10000,
        exit_truth_gap_clean_ms=5000,
        exit_truth_gap_timestop_acceptable_ms=45000,
        exit_truth_gap_other_acceptable_ms=15000,
        entry_drift_acceptable_abs_pct=15.0,
        exit_drift_acceptable_abs_pct=5.0,
    )


def write_lifecycle_labels(
    rows: list[dict[str, Any]],
    outputs: ReportOutputs,
) -> dict[str, Any]:
    label_args = default_labeler_args()
    labels = lifecycle_labeler.build_labels(rows, label_args)
    lifecycle_labeler.write_jsonl(outputs.label_output, labels)
    summary = lifecycle_labeler.build_summary(
        labels,
        source_path=outputs.raw_output,
        output_path=outputs.label_output,
        args=label_args,
    )
    lifecycle_labeler.write_json(outputs.label_summary_output, summary)
    lifecycle_labeler.write_markdown(outputs.label_summary_md_output, summary)
    return summary


def summarize(
    rows: list[dict[str, Any]],
    skipped: Counter[str],
    inputs: Inputs,
    scope_stats: ScopeStats,
) -> str:
    close_truth_coverage_pct = (
        100.0 * scope_stats.resolved_close_truth / scope_stats.closed_positions
        if scope_stats.closed_positions
        else None
    )
    lines = [
        "Shadow lifecycle on-chain report",
        f"artifact_plane={inputs.artifact_plane}",
        f"scope_start_ms={inputs.session_start_ms} scope_end_ms={inputs.session_end_ms}",
        f"session_run_id={inputs.session_run_id}",
        f"rows_written={len(rows)} output={inputs.output_path}",
        (
            "scope_candidates="
            f"{scope_stats.candidate_count} "
            f"candidate_ts_first={scope_stats.first_candidate_ts_ms} "
            f"candidate_ts_last={scope_stats.latest_candidate_ts_ms}"
        ),
        (
            "close_truth_coverage="
            f"{scope_stats.resolved_close_truth}/{scope_stats.closed_positions} "
            f"failed={scope_stats.failed_close_truth} "
            f"pct={close_truth_coverage_pct:.2f}"
            if close_truth_coverage_pct is not None
            else f"close_truth_coverage=0/0 failed={scope_stats.failed_close_truth} pct=n/a"
        ),
    ]
    if scope_stats.latest_close_ts_ms is not None:
        lines.append(f"latest_close_ts_ms={scope_stats.latest_close_ts_ms}")
    if rows:
        entry_drifts = [
            row["drift_pct"]["entry_vs_onchain_executable"]
            for row in rows
            if isinstance(row["drift_pct"].get("entry_vs_onchain_executable"), (int, float))
        ]
        exit_drifts = [
            row["drift_pct"]["exit_vs_onchain_executable"]
            for row in rows
            if isinstance(row["drift_pct"].get("exit_vs_onchain_executable"), (int, float))
        ]
        truth_gaps = [
            abs(row["onchain"]["entry"]["match_delta_ms"])
            for row in rows
            if isinstance(row["onchain"]["entry"].get("match_delta_ms"), (int, float))
        ]
        exit_truth_gaps = [
            row["onchain"]["exit"]["max_abs_truth_gap_ms"]
            for row in rows
            if isinstance(row["onchain"]["exit"].get("max_abs_truth_gap_ms"), (int, float))
        ]
        if entry_drifts:
            lines.append(format_stat_line("entry_drift_pct", entry_drifts))
        if exit_drifts:
            lines.append(format_stat_line("exit_drift_pct", exit_drifts))
        if truth_gaps:
            lines.append(format_stat_line("entry_truth_gap_ms", truth_gaps))
        if exit_truth_gaps:
            lines.append(format_stat_line("exit_truth_gap_ms", exit_truth_gaps))
    if skipped:
        skipped_text = " ".join(f"{reason}={count}" for reason, count in skipped.most_common())
        lines.append(f"skipped {skipped_text}")
    return "\n".join(lines)


def format_stat_line(name: str, values: list[float]) -> str:
    ordered_abs = sorted(abs(value) for value in values)
    p95_index = max(0, math.ceil(len(ordered_abs) * 0.95) - 1)
    p95_abs = ordered_abs[p95_index]
    return (
        f"{name}: count={len(values)} mean={st.mean(values):.6f} "
        f"median={st.median(values):.6f} p95_abs={p95_abs:.6f}"
    )


def run_report(args: argparse.Namespace) -> dict[str, Any]:
    inputs = resolve_inputs(args)
    outputs = resolve_report_outputs(args, inputs)
    transport_by_candidate = load_shadow_transport_records(inputs)
    entry_by_candidate = load_shadow_entries(inputs)
    lifecycle_by_candidate = load_lifecycle(inputs)
    scope_stats = build_scope_stats(
        transport_by_candidate,
        entry_by_candidate,
        lifecycle_by_candidate,
    )
    gatekeeper_buys_by_key = load_gatekeeper_buys(inputs)
    system_log_paths = iter_system_log_paths(inputs.system_log_base)
    relevant_mints = {
        bundle.position_closed.mint_id
        for bundle in lifecycle_by_candidate.values()
        if bundle.position_closed is not None and bundle.position_closed.mint_id
    }
    if not relevant_mints:
        rows: list[dict[str, Any]] = []
        skipped = Counter({"no_closed_positions_in_scope": 1})
        skipped_rows = [
            {
                "schema_version": 1,
                "truth_dataset_kind": TRUTH_DATASET_KIND,
                "collection_plane": collection_plane_for_artifact(inputs.artifact_plane),
                "candidate_id": None,
                "stage": "scope",
                "reason": "no_closed_positions_in_scope",
            }
        ]
    else:
        diag_updates_by_mint = load_diag_updates(system_log_paths, relevant_mints)
        rows, skipped, skipped_rows = analyze_positions(
            inputs,
            transport_by_candidate,
            entry_by_candidate,
            lifecycle_by_candidate,
            gatekeeper_buys_by_key,
            diag_updates_by_mint,
        )
        rows.sort(
            key=lambda row: (
                row["timing"]["close_ts_ms"] if row["timing"].get("close_ts_ms") is not None else 0,
                row["candidate_id"],
            )
        )

    write_jsonl(outputs.raw_output, rows)
    if outputs.outcome_summary_output is not None:
        write_json(outputs.outcome_summary_output, project_outcome_summary_rows(rows))
    if outputs.skipped_rows_output is not None:
        write_jsonl(outputs.skipped_rows_output, skipped_rows)
    denominator_summary = build_denominator_summary(
        rows,
        skipped,
        inputs,
        transport_by_candidate,
        entry_by_candidate,
        lifecycle_by_candidate,
        scope_stats,
        outputs,
    )
    write_json(outputs.summary_output, denominator_summary)
    label_summary = write_lifecycle_labels(rows, outputs)
    manifest = build_manifest(
        inputs=inputs,
        outputs=outputs,
        system_log_paths=system_log_paths,
        rows=rows,
        skipped=skipped,
        scope_stats=scope_stats,
        label_summary=label_summary,
    )
    write_json(outputs.manifest_output, manifest)
    print(summarize(rows, skipped, inputs, scope_stats))
    return {
        "inputs": inputs,
        "outputs": outputs,
        "rows": rows,
        "skipped": skipped,
        "skipped_rows": skipped_rows,
        "summary": denominator_summary,
        "label_summary": label_summary,
        "manifest": manifest,
    }


def main() -> int:
    args = parse_args()
    run_report(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
