#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


DECLARED_HORIZONS_MS = [2_000, 3_000, 10_000, 30_000, 120_000]
UNDECLARED_LONG_HORIZONS_MS = [300_000, 500_000]
DEFAULT_SCOPE_ROOT = (
    "reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1"
)
DEFAULT_RUN_ID = "shadow-v2-l2-d3-density-ready-validation-20260705-r1"
DEFAULT_POSITION_COUNT = 25
DEFAULT_DURATION_MS = 121_000
DEFAULT_SAMPLE_INTERVAL_MS = 1_000
CREATED_AT_WALL_MS = 1_785_000_000_000
L2_D3B_NEXT_STAGE = "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a deterministic Shadow V2 L2-D3 density contract fixture scope."
    )
    parser.add_argument("--scope-root", default=DEFAULT_SCOPE_ROOT)
    parser.add_argument("--run-id", default=DEFAULT_RUN_ID)
    parser.add_argument("--positions", type=int, default=DEFAULT_POSITION_COUNT)
    parser.add_argument("--duration-ms", type=int, default=DEFAULT_DURATION_MS)
    parser.add_argument("--sample-interval-ms", type=int, default=DEFAULT_SAMPLE_INTERVAL_MS)
    return parser.parse_args()


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            fh.write("\n")


def position_id(idx: int) -> str:
    return f"l2-d3-density-pos-{idx:04d}"


def base_mint(idx: int) -> str:
    return f"L2D3DensityMint{idx:04d}"


def pool_id(idx: int) -> str:
    return f"L2D3DensityPool{idx:04d}"


def event_id(pos: str, age_ms: int) -> str:
    return f"{pos}:path:{age_ms:06d}"


def path_sample_event(run_id: str, idx: int, age_ms: int, duration_ms: int) -> dict[str, Any]:
    pos = position_id(idx)
    event = event_id(pos, age_ms)
    reason = "terminal" if age_ms == duration_ms else "heartbeat"
    return {
        "schema": "shadow_position_event_v2",
        "schema_version": 1,
        "event_kind": "PATH_SAMPLE",
        "envelope": {
            "schema": "shadow_path_sample_v2",
            "run_id": run_id,
            "session_id": run_id,
            "position_id": pos,
            "pool_id": pool_id(idx),
            "base_mint": base_mint(idx),
            "event_id": event,
            "quality": "DENSITY_VALIDATION_SCOPE",
            "measurement_grade": "DENSITY_CONTRACT_FIXTURE",
            "simulation_level": "MARK_ONLY",
            "limitations": [
                "L2_D3_DENSITY_VALIDATION_SCOPE_NOT_RESEARCH_SAMPLE",
                "L2_D3_NO_LIVE_EXECUTION",
            ],
        },
        "payload": {
            "record": {
                "age_ms": age_ms,
                "path_horizon_ms": duration_ms,
                "sample_ts_ms": CREATED_AT_WALL_MS + age_ms,
                "sampling_mode": "Standard120s",
                "sampling_reason": reason,
                "pnl_mark_bps": 0,
                "source_quality": "DENSITY_VALIDATION_SCOPE",
            }
        },
    }


def density_row(
    run_id: str,
    idx: int,
    horizon_ms: int,
    duration_ms: int,
    sample_interval_ms: int,
    sample_event_ids: list[str],
) -> dict[str, Any]:
    declared = horizon_ms in DECLARED_HORIZONS_MS
    path_points = len(sample_event_ids)
    coverage_points = min(horizon_ms, duration_ms) // sample_interval_ms + 1
    if declared:
        verdict = "EVALUABLE_EXACT"
        median_interval_ms = sample_interval_ms
        p90_interval_ms = sample_interval_ms
        max_interval_ms = sample_interval_ms
        limitations = ["PATH_SAMPLING_MODE=shadow_path_standard_120s"]
    else:
        verdict = "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY"
        median_interval_ms = None
        p90_interval_ms = None
        max_interval_ms = None
        limitations = [
            "PATH_SAMPLING_MODE=shadow_path_standard_120s",
            "HORIZON_EXCEEDS_CONFIGURED_PATH_MODE",
            "HORIZON_EXCEEDS_REPLAY_COVERAGE",
            "NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE",
        ]
    pos = position_id(idx)
    return {
        "schema": "shadow_path_density_v2",
        "schema_version": 1,
        "run_id": run_id,
        "session_id": run_id,
        "position_id": pos,
        "pool_id": pool_id(idx),
        "base_mint": base_mint(idx),
        "canonical_event_stream_ref": "shadow_position_event_v2.jsonl",
        "source_path_sample_event_ids": sample_event_ids,
        "source_canonical_high_watermark": sample_event_ids[-1],
        "horizon_ms": horizon_ms,
        "verdict": verdict,
        "path_points": path_points,
        "coverage_points": coverage_points,
        "replay_horizon_ms": duration_ms,
        "first_path_point_age_ms": 0,
        "median_interval_ms": median_interval_ms,
        "p90_interval_ms": p90_interval_ms,
        "max_interval_ms": max_interval_ms,
        "duplicate_age_count": 0,
        "non_monotonic_input": False,
        "truncated": False,
        "limitations": limitations,
        "created_at_wall_ms": CREATED_AT_WALL_MS,
    }


def replay_row(run_id: str, idx: int, sample_event_ids: list[str], duration_ms: int) -> dict[str, Any]:
    return {
        "schema": "shadow_replay_v2",
        "schema_version": 1,
        "run_id": run_id,
        "session_id": run_id,
        "position_id": position_id(idx),
        "pool_id": pool_id(idx),
        "base_mint": base_mint(idx),
        "horizon_ms": duration_ms,
        "mark_path_sample_count": len(sample_event_ids),
        "path_sample_event_ids": sample_event_ids,
        "limitations": ["L2_D3_DENSITY_VALIDATION_SCOPE_NOT_L2_F"],
    }


def lifecycle_row(run_id: str, idx: int, duration_ms: int) -> dict[str, Any]:
    return {
        "schema": "shadow_lifecycle_v2",
        "schema_version": 1,
        "run_id": run_id,
        "session_id": run_id,
        "position_id": position_id(idx),
        "pool_id": pool_id(idx),
        "base_mint": base_mint(idx),
        "duration_ms": duration_ms,
        "terminal_reason": "DENSITY_VALIDATION_SCOPE_COMPLETE",
        "limitations": ["L2_D3_DENSITY_VALIDATION_SCOPE_NOT_L2_F"],
    }


def manifest(run_id: str, positions: int, duration_ms: int, sample_interval_ms: int) -> dict[str, Any]:
    return {
        "schema": "shadow_v2_l2_d3_density_contract_fixture_manifest",
        "schema_version": 1,
        "run_id": run_id,
        "position_count": positions,
        "configured_run_seconds": duration_ms // 1_000,
        "duration_ms": duration_ms,
        "sample_interval_ms": sample_interval_ms,
        "declared_supported_horizons_ms": DECLARED_HORIZONS_MS,
        "unsupported_horizons_ms": UNDECLARED_LONG_HORIZONS_MS,
        "retention_contract_ms": duration_ms,
        "required_replay_coverage_ms": duration_ms,
        "density_contract_fixture_pass": True,
        "density_fixture_l2_f_allowed_next": False,
        "runtime_density_emission_proof": False,
        "next_stage": L2_D3B_NEXT_STAGE,
        "l2_f_research_validation_run": False,
        "l2_f_allowed_next": False,
        "runtime_approval": False,
        "research_grade": False,
        "live_equivalence": False,
        "strategy_research_unblocked": False,
        "shadow_close_only": False,
        "active_close": False,
    }


def main() -> int:
    args = parse_args()
    if args.positions <= 0:
        raise SystemExit("--positions must be positive")
    if args.duration_ms < max(DECLARED_HORIZONS_MS) + 1_000:
        raise SystemExit("--duration-ms must cover max declared horizon plus 1000ms margin")
    if args.sample_interval_ms <= 0:
        raise SystemExit("--sample-interval-ms must be positive")

    scope_root = Path(args.scope_root)
    ages = list(range(0, args.duration_ms + 1, args.sample_interval_ms))
    if ages[-1] != args.duration_ms:
        ages.append(args.duration_ms)

    canonical_rows: list[dict[str, Any]] = []
    density_rows: list[dict[str, Any]] = []
    replay_rows: list[dict[str, Any]] = []
    lifecycle_rows: list[dict[str, Any]] = []
    for idx in range(args.positions):
        sample_ids = [event_id(position_id(idx), age_ms) for age_ms in ages]
        canonical_rows.extend(
            path_sample_event(args.run_id, idx, age_ms, args.duration_ms)
            for age_ms in ages
        )
        density_rows.extend(
            density_row(
                args.run_id,
                idx,
                horizon_ms,
                args.duration_ms,
                args.sample_interval_ms,
                sample_ids,
            )
            for horizon_ms in [*DECLARED_HORIZONS_MS, *UNDECLARED_LONG_HORIZONS_MS]
        )
        replay_rows.append(replay_row(args.run_id, idx, sample_ids, args.duration_ms))
        lifecycle_rows.append(lifecycle_row(args.run_id, idx, args.duration_ms))

    write_jsonl(scope_root / "shadow_position_event_v2.jsonl", canonical_rows)
    write_jsonl(scope_root / "shadow_path_density_v2.jsonl", density_rows)
    write_jsonl(scope_root / "shadow_replay_v2.jsonl", replay_rows)
    write_jsonl(scope_root / "shadow_lifecycle_v2.jsonl", lifecycle_rows)
    (scope_root / "density_ready_validation_manifest.json").write_text(
        json.dumps(
            manifest(args.run_id, args.positions, args.duration_ms, args.sample_interval_ms),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "run_id": args.run_id,
                "scope_root": str(scope_root),
                "configured_run_seconds": args.duration_ms // 1_000,
                "position_count": args.positions,
                "path_sample_count": len(canonical_rows),
                "density_row_count": len(density_rows),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
